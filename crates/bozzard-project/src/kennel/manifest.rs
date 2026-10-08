//! `<name>.pkg.json`: what a package contains, where each file comes from, and which engine
//! it needs. Every payload file is pinned by size and SHA-256.
use super::{MAX_FILE, MAX_FILES, valid_name};
use crate::content::{Location, address, digest, portable_file_set, portable_path};
use anyhow::{Context, Result, bail, ensure};
use bozzard_scene::AssetKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Rhai scripts are bounded by the engine's per-script limit.
const MAX_SCRIPT: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Editor tooling hint; ignored by Kennel.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub kennel: u32,
    pub name: String,
    pub version: String,
    pub title: String,
    pub summary: String,
    pub category: Category,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub authors: Vec<String>,
    /// SPDX expression for the files the package authors itself. Bins may carry their own.
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    pub engine: Engine,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
    /// Environment variables a build should set, as folders relative to the installation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub build_env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bins: Vec<Bin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<Script>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<Asset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<PackageFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Native SDKs and the glue an engine feature needs.
    Integration,
    /// Rhai libraries and gameplay rules.
    Scripts,
    /// Meshes, images, materials and fonts.
    Art,
    Audio,
    /// A starting point for a new project.
    Template,
    /// Authoring and build helpers.
    Tool,
}
impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Integration => "integration",
            Self::Scripts => "scripts",
            Self::Art => "art",
            Self::Audio => "audio",
            Self::Template => "template",
            Self::Tool => "tool",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Engine {
    /// Semver requirement on the Bozzard engine version.
    pub bozzard: String,
    /// Lowest native script API the package's scripts need.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub script_api: u32,
    /// Cargo features the engine build must enable. Kennel reports them; it cannot check them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// A native library for specific `<os>-<arch>` targets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bin {
    pub targets: Vec<String>,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Downloaded from upstream instead of stored in the registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

/// An upstream download pinned by its own SHA-256. With `archive`, `member` names the file
/// inside it; without, the download is the file itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<Archive>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Archive {
    #[serde(rename = "tar.gz")]
    TarGz,
}

/// A Rhai script. `id` is the scene catalog ID it is meant to use, so imports between a
/// package's scripts resolve once a scene lists them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub id: String,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// A typed scene asset (anything but a script).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: String,
    pub kind: AssetKind,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// Documentation, notices and other supporting files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// One file an installation receives.
#[derive(Clone, Copy, Debug)]
pub struct Payload<'a> {
    pub path: &'a str,
    pub bytes: u64,
    pub sha256: &'a str,
    pub source: Option<&'a Source>,
}

impl Manifest {
    pub fn file_name(name: &str) -> String {
        format!("{name}.pkg.json")
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() as u64 <= super::MAX_DOCUMENT,
            "package manifest exceeds 1 MiB"
        );
        let manifest: Self = serde_json::from_slice(bytes).context("parsing package manifest")?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn version(&self) -> Result<semver::Version> {
        semver::Version::parse(&self.version)
            .with_context(|| format!("{}: version '{}' is not semver", self.name, self.version))
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.kennel == super::KENNEL, "unsupported Kennel version");
        valid_name(&self.name)?;
        let name = &self.name;
        self.version()?;
        text(&self.title, 80).with_context(|| format!("{name}: invalid title"))?;
        text(&self.summary, 200).with_context(|| format!("{name}: invalid summary"))?;
        ensure!(self.tags.len() <= 8, "{name}: at most 8 tags");
        let mut tags = BTreeSet::new();
        for tag in &self.tags {
            ensure!(
                (1..=32).contains(&tag.len())
                    && tag
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    && tags.insert(tag),
                "{name}: tags are unique, 1..32 lowercase letters, digits and '-': '{tag}'"
            );
        }
        ensure!(
            (1..=16).contains(&self.authors.len()),
            "{name}: list 1..16 authors"
        );
        for author in &self.authors {
            text(author, 120).with_context(|| format!("{name}: invalid author"))?;
        }
        text(&self.license, 120).with_context(|| format!("{name}: invalid license"))?;
        if let Some(repository) = &self.repository {
            ensure!(
                repository.starts_with("https://")
                    && repository.len() <= 512
                    && url::Url::parse(repository).is_ok(),
                "{name}: repository must be an https URL"
            );
        }
        semver::VersionReq::parse(&self.engine.bozzard)
            .with_context(|| format!("{name}: engine.bozzard is not a semver requirement"))?;
        ensure!(
            self.engine.features.len() <= 16
                && self.engine.features.iter().all(|f| {
                    (1..=64).contains(&f.len())
                        && f.bytes().all(|b| {
                            b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b)
                        })
                }),
            "{name}: engine.features holds at most 16 Cargo feature names"
        );
        ensure!(
            self.dependencies.len() <= 32,
            "{name}: at most 32 dependencies"
        );
        for (dependency, requirement) in &self.dependencies {
            valid_name(dependency)?;
            ensure!(dependency != name, "{name} cannot depend on itself");
            semver::VersionReq::parse(requirement).with_context(|| {
                format!("{name}: dependency {dependency} needs a semver requirement")
            })?;
        }

        let count = self.bins.len() + self.scripts.len() + self.assets.len() + self.files.len();
        ensure!(count <= MAX_FILES, "{name}: at most {MAX_FILES} files");
        for item in self.payload() {
            digest(item.sha256).with_context(|| format!("{name}: {}", item.path))?;
            ensure!(
                item.bytes <= MAX_FILE,
                "{name}: {} exceeds 256 MiB",
                item.path
            );
        }
        portable_file_set(self.payload().map(|item| item.path), &Self::file_name(name))
            .with_context(|| format!("{name}: payload paths"))?;

        for bin in &self.bins {
            let path = &bin.path;
            ensure!(
                (1..=8).contains(&bin.targets.len()),
                "{name}: {path} needs 1..8 targets"
            );
            let mut targets = BTreeSet::new();
            for target in &bin.targets {
                ensure!(
                    valid_target(target) && targets.insert(target),
                    "{name}: {path} has an invalid or repeated target '{target}' (use <os>-<arch>, e.g. linux-x86_64)"
                );
            }
            if let Some(license) = &bin.license {
                text(license, 120).with_context(|| format!("{name}: {path} license"))?;
            }
            if let Some(source) = &bin.source {
                Location::parse(&source.url)
                    .ok()
                    .filter(|location| matches!(location, Location::Http(_)))
                    .with_context(|| format!("{name}: {path} source must be an https URL"))?;
                ensure!(
                    source.url.len() <= 2048,
                    "{name}: {path} source URL is too long"
                );
                digest(&source.sha256).with_context(|| format!("{name}: {path} source"))?;
                match (source.archive, &source.member) {
                    (Some(Archive::TarGz), Some(member)) => ensure!(
                        !member.is_empty() && member.len() <= 1024 && !member.starts_with('/'),
                        "{name}: {path} source member must be a relative archive path"
                    ),
                    (None, None) => ensure!(
                        source.sha256 == bin.sha256,
                        "{name}: {path} is downloaded directly, so its source sha256 must equal its own"
                    ),
                    _ => bail!("{name}: {path} source needs both archive and member, or neither"),
                }
            }
        }

        let mut ids = BTreeSet::new();
        let prefix = format!("{name}/");
        let ids_iter = self
            .scripts
            .iter()
            .map(|s| &s.id)
            .chain(self.assets.iter().map(|a| &a.id));
        for id in ids_iter {
            ensure!(
                id.strip_prefix(&prefix)
                    .is_some_and(|rest| address(rest).is_ok())
                    && ids.insert(id),
                "{name}: script and asset ids are unique and start with '{prefix}' followed by letters, digits, '_', '-' or '/': '{id}'"
            );
        }
        for script in &self.scripts {
            ensure!(
                script.path.ends_with(".rhai") || script.path.ends_with(".rs"),
                "{name}: script {} must end in .rhai or .rs",
                script.path
            );
            ensure!(
                script.bytes <= MAX_SCRIPT,
                "{name}: script {} exceeds 1 MiB",
                script.path
            );
        }
        for asset in &self.assets {
            ensure!(
                asset.kind != AssetKind::Script,
                "{name}: list script {} under scripts, not assets",
                asset.path
            );
        }

        ensure!(
            self.build_env.len() <= 8,
            "{name}: at most 8 build_env entries"
        );
        for (variable, folder) in &self.build_env {
            ensure!(
                (1..=64).contains(&variable.len())
                    && !variable.starts_with(|c: char| c.is_ascii_digit())
                    && variable
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
                "{name}: build_env names use A-Z, 0-9 and '_': '{variable}'"
            );
            portable_path(folder)?;
            let folder_prefix = format!("{folder}/");
            ensure!(
                self.bins
                    .iter()
                    .any(|bin| bin.path.starts_with(&folder_prefix)),
                "{name}: build_env {variable} must name a folder that holds bins"
            );
        }
        Ok(())
    }

    /// Every file the package can install, across all targets.
    pub fn payload(&self) -> impl Iterator<Item = Payload<'_>> {
        let bins = self.bins.iter().map(|b| Payload {
            path: &b.path,
            bytes: b.bytes,
            sha256: &b.sha256,
            source: b.source.as_ref(),
        });
        let scripts = self.scripts.iter().map(|s| Payload {
            path: &s.path,
            bytes: s.bytes,
            sha256: &s.sha256,
            source: None,
        });
        let assets = self.assets.iter().map(|a| Payload {
            path: &a.path,
            bytes: a.bytes,
            sha256: &a.sha256,
            source: None,
        });
        let files = self.files.iter().map(|f| Payload {
            path: &f.path,
            bytes: f.bytes,
            sha256: &f.sha256,
            source: None,
        });
        bins.chain(scripts).chain(assets).chain(files)
    }

    /// Files an installation for `targets` receives: everything but bins for other targets.
    pub fn selected<'a>(&'a self, targets: &'a [String]) -> impl Iterator<Item = Payload<'a>> {
        let excluded: BTreeSet<&str> = self
            .bins
            .iter()
            .filter(|bin| !bin.targets.iter().any(|t| targets.contains(t)))
            .map(|bin| bin.path.as_str())
            .collect();
        self.payload()
            .filter(move |item| !excluded.contains(item.path))
    }

    /// Every target any bin supports, sorted.
    pub fn targets(&self) -> Vec<String> {
        let all: BTreeSet<_> = self.bins.iter().flat_map(|b| b.targets.iter()).collect();
        all.into_iter().cloned().collect()
    }
}

/// `<os>-<arch>` with Rust's `std::env::consts` spellings.
fn valid_target(target: &str) -> bool {
    target.len() <= 64
        && target.split_once('-').is_some_and(|(os, arch)| {
            [os, arch].iter().all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            })
        })
}

fn text(value: &str, limit: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control),
        "expected 1..{limit} bytes without control characters"
    );
    Ok(())
}
