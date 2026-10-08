//! Installing packages into a project's `kennel/` folder and recording them in
//! `kennel.lock.json`. The verbatim manifest copy in each installation pins every file.
use super::manifest::Manifest;
use super::registry::{Registry, write_sourced};
use super::*;
use crate::content::{Staging, copy_hash, hex, inventory, lock_exclusive, read_bounded};
use crate::{MANIFEST, Project, runtime::SCRIPT_API_VERSION};
use anyhow::{Context, Result, bail, ensure};
use bozzard_assets::job::Progress;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    pub kennel: u32,
    #[serde(default)]
    pub packages: BTreeMap<String, LockedPackage>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedPackage {
    pub version: String,
    /// The registry the package came from, as given to the installer.
    pub registry: String,
    /// SHA-256 of the installed manifest copy, which pins every other file.
    pub manifest_sha256: String,
    /// Targets whose bins were installed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
}

impl Lockfile {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(LOCKFILE);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                kennel: KENNEL,
                packages: BTreeMap::new(),
            }),
            Err(error) => Err(error.into()),
            Ok(_) => {
                let lockfile: Self = serde_json::from_slice(&read_bounded(&path, MAX_DOCUMENT)?)
                    .with_context(|| format!("parsing {}", path.display()))?;
                ensure!(lockfile.kennel == KENNEL, "unsupported {LOCKFILE} version");
                for (name, locked) in &lockfile.packages {
                    valid_name(name)?;
                    crate::content::digest(&locked.manifest_sha256)?;
                }
                Ok(lockfile)
            }
        }
    }
    fn save(&self, root: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)? + "\n";
        bozzard_demo::save_json(&json, &root.join(LOCKFILE))
    }
}

#[derive(Clone, Debug, Default)]
pub struct InstallOptions {
    /// Install bins for every target, not just this machine's.
    pub all_targets: bool,
    /// Replace an installation whose files were modified.
    pub force: bool,
    /// Upstream download cache; defaults to [`cache_directory`].
    pub cache: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub directory: PathBuf,
    pub files: usize,
    pub targets: Vec<String>,
    /// Engine Cargo features the package needs; the installer cannot check them.
    pub features: Vec<String>,
    /// Build environment variables and the absolute folders to set them to.
    pub build_env: Vec<(String, PathBuf)>,
    /// The same content was already installed.
    pub unchanged: bool,
}

/// The folder holding a project's manifest. Accepts that folder or the manifest file.
pub fn project_root(project: &Path) -> Result<PathBuf> {
    let manifest = if project.is_dir() {
        project.join(MANIFEST)
    } else {
        project.to_owned()
    };
    Project::load(&manifest)?;
    Ok(manifest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?)
}

/// Installs `name` and its dependencies into `<project>/kennel/`, dependencies first.
pub fn install(
    project: &Path,
    name: &str,
    registry: &Registry,
    options: InstallOptions,
    progress: &Progress,
) -> Result<Vec<Installed>> {
    let root = project_root(project)?;
    let folder = root.join(INSTALL_DIR);
    fs::create_dir_all(&folder)?;
    let _lock = lock_exclusive(
        &folder.join(".lock"),
        "Waiting for another Kennel install",
        progress,
    )?;
    let mut lockfile = Lockfile::load(&root)?;
    let mut order = Vec::new();
    resolve(registry, name, None, &mut Vec::new(), &mut order, progress)?;
    let cache = match &options.cache {
        Some(cache) => cache.clone(),
        None => cache_directory()?,
    };
    order
        .iter()
        .map(|(manifest, bytes)| {
            let target = Target {
                root: &root,
                cache: &cache,
                options: &options,
            };
            install_one(target, registry, manifest, bytes, &mut lockfile, progress)
        })
        .collect()
}

fn resolve(
    registry: &Registry,
    name: &str,
    required_by: Option<(&str, &str)>,
    chain: &mut Vec<String>,
    order: &mut Vec<(Manifest, Vec<u8>)>,
    progress: &Progress,
) -> Result<()> {
    if let Some(start) = chain.iter().position(|entry| entry == name) {
        bail!(
            "dependency cycle: {} -> {name}",
            chain[start..].join(" -> ")
        );
    }
    let satisfies = |manifest: &Manifest| -> Result<()> {
        if let Some((dependent, requirement)) = required_by {
            ensure!(
                semver::VersionReq::parse(requirement)?.matches(&manifest.version()?),
                "{dependent} needs {name} {requirement}, but the registry has {}",
                manifest.version
            );
        }
        Ok(())
    };
    if let Some((manifest, _)) = order.iter().find(|(manifest, _)| manifest.name == name) {
        return satisfies(manifest);
    }
    let (manifest, bytes) = registry.manifest(name, progress)?;
    satisfies(&manifest)?;
    ensure!(
        manifest.supports_engine()?,
        "{name} {} needs Bozzard {}; this engine is {ENGINE_VERSION}",
        manifest.version,
        manifest.engine.bozzard
    );
    ensure!(
        manifest.supports_script_api(),
        "{name} {} needs script API {}; this engine provides {SCRIPT_API_VERSION}",
        manifest.version,
        manifest.engine.script_api
    );
    chain.push(name.to_owned());
    for (dependency, requirement) in &manifest.dependencies {
        resolve(
            registry,
            dependency,
            Some((name, requirement)),
            chain,
            order,
            progress,
        )?;
    }
    chain.pop();
    order.push((manifest, bytes));
    Ok(())
}

/// Where and how one package is installed.
#[derive(Clone, Copy)]
struct Target<'a> {
    root: &'a Path,
    cache: &'a Path,
    options: &'a InstallOptions,
}

fn install_one(
    target: Target,
    registry: &Registry,
    manifest: &Manifest,
    bytes: &[u8],
    lockfile: &mut Lockfile,
    progress: &Progress,
) -> Result<Installed> {
    let Target {
        root,
        cache,
        options,
    } = target;
    let name = &manifest.name;
    let directory = root.join(INSTALL_DIR).join(name);
    let available = manifest.targets();
    let targets = if options.all_targets || available.is_empty() {
        available
    } else {
        let host = host_target();
        ensure!(
            available.contains(&host),
            "{name} has no binaries for {host} (available: {}); --all-targets installs them all",
            available.join(", ")
        );
        vec![host]
    };
    let locked = LockedPackage {
        version: manifest.version.clone(),
        registry: registry.location().to_owned(),
        manifest_sha256: hex(Sha256::digest(bytes)),
        targets,
    };
    let report = |files, unchanged| Installed {
        name: name.clone(),
        version: manifest.version.clone(),
        directory: directory.clone(),
        files,
        targets: locked.targets.clone(),
        features: manifest.engine.features.clone(),
        build_env: manifest
            .build_env
            .iter()
            .map(|(variable, folder)| (variable.clone(), directory.join(folder)))
            .collect(),
        unchanged,
    };

    let exists = fs::symlink_metadata(&directory).is_ok();
    if exists {
        match lockfile.packages.get(name) {
            Some(current) => match verify_package(root, name, current, progress) {
                Ok(files)
                    if current.manifest_sha256 == locked.manifest_sha256
                        && current.targets == locked.targets =>
                {
                    return Ok(report(files, true));
                }
                Ok(_) => {}
                Err(error) => ensure!(
                    options.force,
                    "{error:#}; --force replaces the modified installation"
                ),
            },
            None => ensure!(
                options.force,
                "{INSTALL_DIR}/{name} exists but is not in {LOCKFILE}; --force replaces it"
            ),
        }
    }

    let stage = Staging::with_prefix(&root.join(INSTALL_DIR), ".kennel-install")?;
    let candidate = stage.path.join("package");
    fs::create_dir(&candidate)?;
    let mut files = 0;
    for item in manifest.selected(&locked.targets) {
        progress.stage(format!("Installing {name}/{}", item.path))?;
        let path = candidate.join(item.path);
        fs::create_dir_all(path.parent().context("package file has no folder")?)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        match item.source {
            Some(source) => write_sourced(&item, source, cache, &mut file, progress)
                .with_context(|| format!("{name}: fetching {}", item.path))?,
            None => {
                let (mut reader, _) = registry
                    .package_file(name, item.path)?
                    .open(item.bytes, progress)?;
                let (bytes, sha256) = copy_hash(&mut reader, &mut file, item.bytes, progress)?;
                ensure!(
                    bytes == item.bytes && sha256 == item.sha256,
                    "{name}: {} does not match its manifest entry",
                    item.path
                );
            }
        }
        file.sync_all()?;
        files += 1;
    }
    fs::write(candidate.join(Manifest::file_name(name)), bytes)?;
    progress.check()?;

    let backup = stage.path.join("previous");
    if exists {
        fs::rename(&directory, &backup)?;
    }
    if let Err(error) = fs::rename(&candidate, &directory) {
        if exists {
            let _ = fs::rename(&backup, &directory);
        }
        return Err(error).context("publishing the installation");
    }
    let previous = lockfile.packages.insert(name.clone(), locked.clone());
    if let Err(error) = lockfile.save(root) {
        let _ = fs::rename(&directory, stage.path.join("failed"));
        if exists {
            let _ = fs::rename(&backup, &directory);
        }
        match previous {
            Some(previous) => lockfile.packages.insert(name.clone(), previous),
            None => lockfile.packages.remove(name),
        };
        return Err(error).context("writing the lockfile");
    }
    Ok(report(files, false))
}

/// Checks one installation against its lockfile entry: the manifest copy's hash, then every
/// selected file's size and hash, and no extra files. Returns the payload file count.
fn verify_package(
    root: &Path,
    name: &str,
    locked: &LockedPackage,
    progress: &Progress,
) -> Result<usize> {
    let directory = root.join(INSTALL_DIR).join(name);
    let label = format!("{INSTALL_DIR}/{name}");
    let manifest_file = Manifest::file_name(name);
    let bytes = read_bounded(&directory.join(&manifest_file), MAX_DOCUMENT)
        .with_context(|| format!("{label}/{manifest_file} is missing"))?;
    ensure!(
        hex(Sha256::digest(&bytes)) == locked.manifest_sha256,
        "{label}/{manifest_file} was modified"
    );
    let manifest = Manifest::parse(&bytes)?;
    ensure!(
        manifest.name == name && manifest.version == locked.version,
        "{label} does not hold {name} {}",
        locked.version
    );
    let mut found: BTreeMap<String, (u64, String)> = inventory(&directory, progress)
        .with_context(|| format!("reading {label}"))?
        .into_iter()
        .filter(|file| file.path != manifest_file)
        .map(|file| (file.path, (file.bytes, file.sha256)))
        .collect();
    let mut files = 0;
    for item in manifest.selected(&locked.targets) {
        let (bytes, sha256) = found
            .remove(item.path)
            .with_context(|| format!("{label}/{} is missing", item.path))?;
        ensure!(
            bytes == item.bytes && sha256 == item.sha256,
            "{label}/{} was modified",
            item.path
        );
        files += 1;
    }
    if let Some(extra) = found.keys().next() {
        bail!("{label}/{extra} is not part of the package");
    }
    Ok(files)
}

/// Verifies every installed package; returns `(name, version, files)` for each.
pub fn verify(project: &Path, progress: &Progress) -> Result<Vec<(String, String, usize)>> {
    let root = project_root(project)?;
    let lockfile = Lockfile::load(&root)?;
    let mut verified = Vec::new();
    for (name, locked) in &lockfile.packages {
        let files = verify_package(&root, name, locked, progress)?;
        verified.push((name.clone(), locked.version.clone(), files));
    }
    let folder = root.join(INSTALL_DIR);
    if folder.is_dir() {
        for entry in fs::read_dir(&folder)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            ensure!(
                name.starts_with('.') || lockfile.packages.contains_key(&name),
                "{INSTALL_DIR}/{name} is not in {LOCKFILE}"
            );
        }
    }
    Ok(verified)
}

/// Removes an installed package. Refuses when another installed package depends on it, or
/// when its files were modified (unless `force`).
pub fn remove(project: &Path, name: &str, force: bool, progress: &Progress) -> Result<()> {
    let root = project_root(project)?;
    let folder = root.join(INSTALL_DIR);
    let _lock = lock_exclusive(
        &folder.join(".lock"),
        "Waiting for another Kennel install",
        progress,
    )?;
    let mut lockfile = Lockfile::load(&root)?;
    let locked = lockfile
        .packages
        .get(name)
        .with_context(|| format!("{name} is not installed"))?;
    if !force {
        verify_package(&root, name, locked, progress)
            .map_err(|error| anyhow::anyhow!("{error:#}; --force removes it anyway"))?;
    }
    for other in lockfile.packages.keys().filter(|other| *other != name) {
        let file = folder.join(other).join(Manifest::file_name(other));
        let manifest = Manifest::parse(&read_bounded(&file, MAX_DOCUMENT)?)?;
        ensure!(
            !manifest.dependencies.contains_key(name),
            "{other} depends on {name}; remove {other} first"
        );
    }
    let stage = Staging::with_prefix(&folder, ".kennel-remove")?;
    let directory = folder.join(name);
    let moved = stage.path.join("package");
    let existed = fs::symlink_metadata(&directory).is_ok();
    if existed {
        fs::rename(&directory, &moved)?;
    }
    let locked = lockfile.packages.remove(name).expect("checked above");
    if let Err(error) = lockfile.save(&root) {
        if existed {
            let _ = fs::rename(&moved, &directory);
        }
        lockfile.packages.insert(name.to_owned(), locked);
        return Err(error).context("writing the lockfile");
    }
    Ok(())
}
