//! Registries: reading `index.json` and manifests from a folder or HTTPS base, generating the
//! index for a registry checkout, checking every package in it, and fetching upstream files.
use super::manifest::{Archive, Category, Manifest, Payload, Source};
use super::*;
use crate::content::{Location, copy_hash, hex, inventory, lock_exclusive, read_bounded};
use anyhow::{Context, Result, ensure};
use bozzard_assets::job::Progress;
use bozzard_scene::AssetSource;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Canonical JSON Schemas for editor tooling. `kennel index` writes them into the registry and
/// `kennel check` requires the registry's copies to match.
pub(super) const SCHEMAS: [(&str, &str); 2] = [
    (
        "schema/pkg.schema.json",
        include_str!("schema/pkg.schema.json"),
    ),
    (
        "schema/index.schema.json",
        include_str!("schema/index.schema.json"),
    ),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub kennel: u32,
    pub packages: BTreeMap<String, IndexEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    pub version: String,
    pub title: String,
    pub summary: String,
    pub category: Category,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// `packages/<name>/<name>.pkg.json`, relative to the index.
    pub manifest: String,
    /// SHA-256 of the manifest file's bytes.
    pub sha256: String,
}

impl IndexEntry {
    /// Whether every word of `query` appears in the package's name, title, summary or tags,
    /// ignoring case. An empty query matches every package.
    pub fn matches(&self, name: &str, query: &str) -> bool {
        let haystack = format!(
            "{name} {} {} {}",
            self.title,
            self.summary,
            self.tags.join(" ")
        )
        .to_lowercase();
        query
            .split_whitespace()
            .all(|word| haystack.contains(&word.to_lowercase()))
    }
}

impl Index {
    pub fn manifest_path(name: &str) -> String {
        format!("{PACKAGES_DIR}/{name}/{}", Manifest::file_name(name))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.kennel == KENNEL, "unsupported Kennel index version");
        ensure!(
            self.packages.len() <= MAX_PACKAGES,
            "Kennel index lists more than {MAX_PACKAGES} packages"
        );
        for (name, entry) in &self.packages {
            valid_name(name)?;
            ensure!(
                entry.manifest == Self::manifest_path(name),
                "index entry {name} must point at {}",
                Self::manifest_path(name)
            );
            crate::content::digest(&entry.sha256)?;
            semver::Version::parse(&entry.version)
                .with_context(|| format!("index entry {name} has no semver version"))?;
        }
        Ok(())
    }
}

/// An opened registry: its validated index, and where its files are read from.
pub struct Registry {
    location: String,
    index_location: Location,
    pub index: Index,
}

impl Registry {
    /// `location` is a registry folder or an HTTPS base URL (HTTP on loopback only).
    pub fn open(location: &str, progress: &Progress) -> Result<Self> {
        progress.stage("Loading Kennel index")?;
        let index_location = if location.starts_with("http://") || location.starts_with("https://")
        {
            Location::parse(&format!("{}/{INDEX_FILE}", location.trim_end_matches('/')))?
        } else {
            let path = Path::new(location).join(INDEX_FILE);
            Location::parse(path.to_str().context("registry path must be UTF-8")?)?
        };
        let bytes = fetch(&index_location, MAX_DOCUMENT, progress)
            .with_context(|| format!("reading Kennel index from {location}"))?;
        let index: Index = serde_json::from_slice(&bytes).context("parsing Kennel index")?;
        index.validate()?;
        Ok(Self {
            location: location.to_owned(),
            index_location,
            index,
        })
    }

    /// The registry as given to [`Registry::open`]; lockfiles record it.
    pub fn location(&self) -> &str {
        &self.location
    }

    /// A package manifest, checked against the hash the index pins.
    pub fn manifest(&self, name: &str, progress: &Progress) -> Result<(Manifest, Vec<u8>)> {
        let entry = self
            .index
            .packages
            .get(name)
            .with_context(|| format!("Kennel has no package '{name}'"))?;
        let bytes = fetch(
            &self.index_location.resolve(&entry.manifest)?,
            MAX_DOCUMENT,
            progress,
        )
        .with_context(|| format!("reading {}", entry.manifest))?;
        ensure!(
            hex(Sha256::digest(&bytes)) == entry.sha256,
            "{name}: manifest does not match the registry index. A just-pushed registry can be \
             served stale for a few minutes; retry, or use a registry URL pinned to a commit"
        );
        let manifest = Manifest::parse(&bytes)?;
        ensure!(
            manifest.name == name && manifest.version == entry.version,
            "{name}: manifest name/version disagrees with the registry index"
        );
        Ok((manifest, bytes))
    }

    /// Where a file stored in a package folder is read from.
    pub(super) fn package_file(&self, name: &str, path: &str) -> Result<Location> {
        self.index_location
            .resolve(&format!("{PACKAGES_DIR}/{name}/{path}"))
    }

    /// A UTF-8 file stored in a package folder, such as its README, checked against the
    /// manifest's size and hash. `manifest` comes from [`Registry::manifest`].
    pub fn package_text(
        &self,
        manifest: &Manifest,
        path: &str,
        progress: &Progress,
    ) -> Result<String> {
        let name = &manifest.name;
        let item = manifest
            .payload()
            .find(|item| item.path == path && item.source.is_none())
            .with_context(|| format!("{name} stores no file {path}"))?;
        ensure!(item.bytes <= MAX_DOCUMENT, "{name}: {path} exceeds 1 MiB");
        let bytes = fetch(&self.package_file(name, path)?, MAX_DOCUMENT, progress)
            .with_context(|| format!("reading {name}/{path}"))?;
        ensure!(
            bytes.len() as u64 == item.bytes && hex(Sha256::digest(&bytes)) == item.sha256,
            "{name}: {path} does not match its manifest entry"
        );
        String::from_utf8(bytes).with_context(|| format!("{name}: {path} is not UTF-8"))
    }
}

fn fetch(location: &Location, limit: u64, progress: &Progress) -> Result<Vec<u8>> {
    let (reader, _) = location.open(limit, progress)?;
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "document exceeds 1 MiB");
    Ok(bytes)
}

/// Downloads an upstream file once into the content-addressed cache, or reuses a cached copy
/// after re-hashing it.
fn fetch_source(source: &Source, cache: &Path, progress: &Progress) -> Result<PathBuf> {
    let cache = cache.join("sources");
    fs::create_dir_all(&cache)?;
    let _lock = lock_exclusive(
        &cache.join(format!(".{}.lock", source.sha256)),
        "Waiting for another Kennel download",
        progress,
    )?;
    let path = cache.join(&source.sha256);
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
        let (_, sha256) = copy_hash(
            &mut fs::File::open(&path)?,
            &mut std::io::sink(),
            MAX_ARCHIVE,
            progress,
        )?;
        if sha256 == source.sha256 {
            return Ok(path);
        }
        fs::remove_file(&path)?;
    }
    progress.stage(format!("Downloading {}", source.url))?;
    let (mut reader, _) = Location::parse(&source.url)?.open(MAX_ARCHIVE, progress)?;
    let partial = cache.join(format!(".{}.{}.partial", source.sha256, std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = fs::File::create(&partial)?;
        let (_, sha256) = copy_hash(&mut reader, &mut file, MAX_ARCHIVE, progress)?;
        ensure!(
            sha256 == source.sha256,
            "{} does not match its pinned sha256 (got {sha256})",
            source.url
        );
        file.sync_all()?;
        fs::rename(&partial, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result.map(|()| path)
}

/// Writes an upstream-sourced payload file, checked against the manifest's size and hash.
pub(super) fn write_sourced(
    item: &Payload,
    source: &Source,
    cache: &Path,
    output: &mut impl Write,
    progress: &Progress,
) -> Result<()> {
    let archive = fetch_source(source, cache, progress)?;
    let mut input = fs::File::open(&archive)?;
    let (bytes, sha256) = match (source.archive, &source.member) {
        (Some(Archive::TarGz), Some(member)) => {
            tar::extract_member(input, member, output, item.bytes, progress)?
        }
        _ => copy_hash(&mut input, output, item.bytes, progress)?,
    };
    ensure!(
        bytes == item.bytes && sha256 == item.sha256,
        "{}: upstream file does not match the manifest ({bytes} bytes, sha256 {sha256})",
        item.path
    );
    Ok(())
}

struct Package {
    manifest: Manifest,
    bytes: Vec<u8>,
    folder: PathBuf,
}

/// Reads every `packages/<name>/<name>.pkg.json` in a registry checkout, sorted by name.
fn scan(registry: &Path) -> Result<Vec<Package>> {
    let root = registry.join(PACKAGES_DIR);
    let mut packages = Vec::new();
    for entry in fs::read_dir(&root).with_context(|| format!("reading {}", root.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .context("package folder names must be UTF-8")?;
        ensure!(
            entry.file_type()?.is_dir(),
            "{PACKAGES_DIR}/ holds only package folders, found {name}"
        );
        valid_name(name)?;
        let file = entry.path().join(Manifest::file_name(name));
        let bytes = read_bounded(&file, MAX_DOCUMENT)
            .with_context(|| format!("reading {}", file.display()))?;
        let manifest =
            Manifest::parse(&bytes).with_context(|| format!("checking {}", file.display()))?;
        ensure!(
            manifest.name == name,
            "{} names package '{}', not its folder '{name}'",
            file.display(),
            manifest.name
        );
        packages.push(Package {
            manifest,
            bytes,
            folder: entry.path(),
        });
    }
    ensure!(
        packages.len() <= MAX_PACKAGES,
        "registry holds more than {MAX_PACKAGES} packages"
    );
    packages.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    Ok(packages)
}

fn render_index(packages: &[Package]) -> Result<Vec<u8>> {
    let index = Index {
        schema: Some("schema/index.schema.json".into()),
        kennel: KENNEL,
        packages: packages
            .iter()
            .map(|package| {
                let manifest = &package.manifest;
                let entry = IndexEntry {
                    version: manifest.version.clone(),
                    title: manifest.title.clone(),
                    summary: manifest.summary.clone(),
                    category: manifest.category,
                    tags: manifest.tags.clone(),
                    manifest: Index::manifest_path(&manifest.name),
                    sha256: hex(Sha256::digest(&package.bytes)),
                };
                (manifest.name.clone(), entry)
            })
            .collect(),
    };
    index.validate()?;
    let mut bytes = serde_json::to_vec_pretty(&index)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Regenerates `index.json` and the schema copies of a registry checkout.
pub fn build_index(registry: &Path) -> Result<Index> {
    let packages = scan(registry)?;
    let bytes = render_index(&packages)?;
    bozzard_demo::save_json(std::str::from_utf8(&bytes)?, &registry.join(INDEX_FILE))?;
    for (path, schema) in SCHEMAS {
        bozzard_demo::save_json(schema, &registry.join(path))?;
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheckReport {
    pub packages: usize,
    pub files: usize,
    pub scripts: usize,
    pub assets: usize,
    /// Upstream files downloaded and verified (with `fetch`).
    pub sources: usize,
}

/// Validates a registry checkout: manifests, the exact file set of each package folder,
/// dependencies, script compilation, typed asset loading, and an up-to-date index and schema.
/// With a `fetch` cache folder, also downloads every upstream file and checks it against its
/// manifest.
pub fn check(registry: &Path, fetch: Option<&Path>, progress: &Progress) -> Result<CheckReport> {
    let packages = scan(registry)?;
    let by_name: BTreeMap<&str, &Package> = packages
        .iter()
        .map(|package| (package.manifest.name.as_str(), package))
        .collect();
    let mut report = CheckReport {
        packages: packages.len(),
        ..Default::default()
    };
    for package in &packages {
        let manifest = &package.manifest;
        let name = &manifest.name;
        progress.stage(format!("Checking {name}"))?;
        for (dependency, requirement) in &manifest.dependencies {
            let found = by_name.get(dependency.as_str()).with_context(|| {
                format!("{name} depends on {dependency}, which is not in the registry")
            })?;
            ensure!(
                semver::VersionReq::parse(requirement)?.matches(&found.manifest.version()?),
                "{name} needs {dependency} {requirement}, but the registry has {}",
                found.manifest.version
            );
        }

        let manifest_file = Manifest::file_name(name);
        let found: BTreeMap<String, (u64, String)> = inventory(&package.folder, progress)?
            .into_iter()
            .filter(|file| file.path != manifest_file)
            .map(|file| (file.path, (file.bytes, file.sha256)))
            .collect();
        let mut listed = BTreeSet::new();
        for item in manifest.payload() {
            let path = item.path;
            if item.source.is_some() {
                ensure!(
                    !found.contains_key(path),
                    "{name}: {path} is downloaded from its source; remove the copy from the registry"
                );
                continue;
            }
            let (bytes, sha256) = found
                .get(path)
                .with_context(|| format!("{name}: {path} is listed but missing"))?;
            ensure!(
                *bytes == item.bytes && *sha256 == item.sha256,
                "{name}: {path} does not match its manifest entry (actual {bytes} bytes, sha256 {sha256})"
            );
            listed.insert(path);
            report.files += 1;
        }
        if let Some(path) = found.keys().find(|path| !listed.contains(path.as_str())) {
            anyhow::bail!("{name}: {path} is not listed in {manifest_file}");
        }

        // Scripts compile as one catalog with their dependencies' scripts, so imports resolve.
        let mut sources = BTreeMap::new();
        let mut pending = vec![package];
        let mut seen = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current.manifest.name.as_str()) {
                continue;
            }
            for script in &current.manifest.scripts {
                let bytes = read_bounded(&current.folder.join(&script.path), script.bytes)?;
                let text = String::from_utf8(bytes).with_context(|| {
                    format!("{}: {} is not UTF-8", current.manifest.name, script.path)
                })?;
                sources.insert(script.id.clone(), text);
            }
            pending.extend(
                current
                    .manifest
                    .dependencies
                    .keys()
                    .filter_map(|dependency| by_name.get(dependency.as_str())),
            );
        }
        if !sources.is_empty() {
            bozzard_scene::check_script_sources(sources, progress)
                .with_context(|| format!("{name}: scripts do not compile"))?;
        }
        report.scripts += manifest.scripts.len();

        if !manifest.assets.is_empty() {
            let catalog: BTreeMap<_, _> = manifest
                .assets
                .iter()
                .map(|asset| {
                    let source = AssetSource {
                        kind: asset.kind,
                        path: asset.path.clone(),
                    };
                    (asset.id.clone(), source)
                })
                .collect();
            let mut store = bozzard_assets::AssetStore::new(&package.folder, &catalog)?;
            store.load_pending_with(progress)?;
            store
                .require_ready()
                .with_context(|| format!("{name}: assets do not load"))?;
            report.assets += catalog.len();
        }

        if let Some(cache) = fetch {
            for item in manifest.payload() {
                if let Some(source) = item.source {
                    write_sourced(&item, source, cache, &mut std::io::sink(), progress)
                        .with_context(|| format!("{name}: fetching {}", item.path))?;
                    report.sources += 1;
                }
            }
        }
    }

    let stale = |path: &str| {
        format!(
            "{path} is out of date; run `bozzard-project kennel index {}`",
            registry.display()
        )
    };
    let index = read_bounded(&registry.join(INDEX_FILE), MAX_DOCUMENT).ok();
    ensure!(
        index.as_deref() == Some(render_index(&packages)?.as_slice()),
        stale(INDEX_FILE)
    );
    for (path, schema) in SCHEMAS {
        let current = read_bounded(&registry.join(path), MAX_DOCUMENT).ok();
        ensure!(current.as_deref() == Some(schema.as_bytes()), stale(path));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kennel::manifest::{Asset, Bin, Engine, PackageFile, Script};
    use serde_json::Value;

    fn keys(value: &Value) -> BTreeSet<String> {
        value.as_object().unwrap().keys().cloned().collect()
    }

    /// The schemas are hand-written for editors; keep their field names identical to serde's.
    #[test]
    fn schemas_name_every_serialized_field() {
        let pkg: Value = serde_json::from_str(SCHEMAS[0].1).unwrap();
        let index: Value = serde_json::from_str(SCHEMAS[1].1).unwrap();
        let file = |path: &str| (path.to_owned(), 1, "0".repeat(64));
        let source = Source {
            url: "https://example.com/a.tar.gz".into(),
            sha256: "1".repeat(64),
            archive: Some(Archive::TarGz),
            member: Some("a/lib.so".into()),
        };
        let manifest = Manifest {
            schema: Some("../../schema/pkg.schema.json".into()),
            kennel: KENNEL,
            name: "kit".into(),
            version: "1.0.0".into(),
            title: "Kit".into(),
            summary: "Every field.".into(),
            category: Category::Tool,
            tags: vec!["tag".into()],
            authors: vec!["Bozz".into()],
            license: "MIT OR Apache-2.0".into(),
            repository: Some("https://example.com".into()),
            engine: Engine {
                bozzard: ">=0.1.0".into(),
                script_api: 1,
                features: vec!["steam".into()],
            },
            dependencies: [("base".into(), "^1".into())].into(),
            build_env: [("KIT_SDK".into(), "sdk".into())].into(),
            bins: vec![{
                let (path, bytes, sha256) = file("sdk/lib.so");
                Bin {
                    targets: vec!["linux-x86_64".into()],
                    path,
                    bytes,
                    sha256,
                    license: Some("Proprietary".into()),
                    source: Some(source.clone()),
                }
            }],
            scripts: vec![{
                let (path, bytes, sha256) = file("scripts/a.rhai");
                Script {
                    id: "kit/a".into(),
                    path,
                    bytes,
                    sha256,
                }
            }],
            assets: vec![{
                let (path, bytes, sha256) = file("art/a.png");
                Asset {
                    id: "kit/art".into(),
                    kind: bozzard_scene::AssetKind::Image,
                    path,
                    bytes,
                    sha256,
                }
            }],
            files: vec![{
                let (path, bytes, sha256) = file("README.md");
                PackageFile {
                    path,
                    bytes,
                    sha256,
                }
            }],
        };
        manifest.validate().unwrap();
        let value = serde_json::to_value(&manifest).unwrap();
        let defs = &pkg["$defs"];
        let checks = [
            (&value, &pkg["properties"]),
            (&value["engine"], &pkg["properties"]["engine"]["properties"]),
            (&value["bins"][0], &defs["bin"]["properties"]),
            (&value["bins"][0]["source"], &defs["source"]["properties"]),
            (&value["scripts"][0], &defs["script"]["properties"]),
            (&value["assets"][0], &defs["asset"]["properties"]),
            (&value["files"][0], &defs["file"]["properties"]),
        ];
        for (serialized, schema) in checks {
            assert_eq!(keys(serialized), keys(schema));
        }

        let entry = IndexEntry {
            version: "1.0.0".into(),
            title: "Kit".into(),
            summary: "Every field.".into(),
            category: Category::Tool,
            tags: vec!["tag".into()],
            manifest: Index::manifest_path("kit"),
            sha256: "2".repeat(64),
        };
        let listing = Index {
            schema: Some("schema/index.schema.json".into()),
            kennel: KENNEL,
            packages: [("kit".into(), entry)].into(),
        };
        listing.validate().unwrap();
        let value = serde_json::to_value(&listing).unwrap();
        assert_eq!(keys(&value), keys(&index["properties"]));
        assert_eq!(
            keys(&value["packages"]["kit"]),
            keys(&index["$defs"]["entry"]["properties"])
        );
    }
}
