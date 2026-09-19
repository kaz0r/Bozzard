//! Cooked content bundles and address catalogs. Files are published only after validation.
mod archive;
mod build;
mod download;
use crate::{CookReport, CookTarget};
use anyhow::{Context, Result, ensure};
use bozzard_assets::job::Progress;
use bozzard_scene::{AssetKind, AssetSource, Layer};
pub use build::{PreparedPack, prepare_pack};
pub use download::{ContentCatalog, ContentStore, default_cache_directory, load_catalog};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

const VERSION: u32 = 1;
const MAX_FILES: usize = 8192;
const MAX_ENTRIES: usize = 1024;
const MAX_INDEX: u64 = 8 * 1024 * 1024;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_PACK: u64 = 4 * 1024 * 1024 * 1024;
const INDEX_FILE: &str = ".bozzard-content-index.json";

/// Authoring input. Scene paths and asset paths are relative to this document.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackSpec {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub cook: CookTarget,
    #[serde(default)]
    pub scenes: BTreeMap<String, String>,
    #[serde(default)]
    pub assets: BTreeMap<String, AssetSource>,
}
impl PackSpec {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == VERSION, "unsupported content spec version");
        address(&self.id)?;
        ensure!(
            !self.id.contains('/'),
            "pack id must be one address segment"
        );
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 120
                && !self.name.chars().any(char::is_control),
            "invalid pack name"
        );
        ensure!(
            self.cook != CookTarget::Source,
            "content packs require rgba, bc, astc or universal cooking"
        );
        ensure!(
            self.scenes.len() <= 64
                && (1..=MAX_ENTRIES).contains(&(self.scenes.len() + self.assets.len())),
            "pack needs 1..1024 entries, at most 64 scenes"
        );
        for (name, path) in &self.scenes {
            address(name)?;
            portable_path(path)?;
            ensure!(
                !self.assets.contains_key(name),
                "duplicate content address '{name}'"
            );
        }
        for (name, asset) in &self.assets {
            address(name)?;
            portable_path(&asset.path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackReference {
    pub location: String,
    pub bytes: u64,
    pub sha256: String,
    pub index_sha256: String,
}
impl PackReference {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.location.is_empty() && self.location.len() <= 4096,
            "invalid pack location"
        );
        ensure!(
            (16..=MAX_PACK).contains(&self.bytes),
            "content pack exceeds size limit"
        );
        digest(&self.sha256)?;
        digest(&self.index_sha256)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Address {
    pub pack: String,
    pub entry: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub packs: BTreeMap<String, PackReference>,
    pub addresses: BTreeMap<String, Address>,
}
impl Catalog {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION
                && (1..=1024).contains(&self.packs.len())
                && (1..=4096).contains(&self.addresses.len()),
            "invalid content catalog size/version"
        );
        for (id, pack) in &self.packs {
            address(id)?;
            pack.validate()?;
        }
        for (id, value) in &self.addresses {
            address(id)?;
            address(&value.entry)?;
            ensure!(
                self.packs.contains_key(&value.pack),
                "address '{id}' refers to unknown pack '{}'",
                value.pack
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Entry {
    Scene {
        path: String,
        /// Additive chunks may have no camera; standalone player entry points need one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<Layer>,
    },
    Asset {
        path: String,
        kind: AssetKind,
    },
}
impl Entry {
    fn path(&self) -> &str {
        match self {
            Self::Scene { path, .. } | Self::Asset { path, .. } => path,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEntry {
    path: String,
    bytes: u64,
    sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u32,
    id: String,
    name: String,
    cook: CookTarget,
    entries: BTreeMap<String, Entry>,
    files: Vec<FileEntry>,
}
impl Index {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == VERSION && self.cook != CookTarget::Source,
            "unsupported content pack version/target"
        );
        address(&self.id)?;
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 120
                && !self.name.chars().any(char::is_control),
            "invalid pack name"
        );
        ensure!(
            (1..=MAX_ENTRIES).contains(&self.entries.len())
                && self
                    .entries
                    .values()
                    .filter(|e| matches!(e, Entry::Scene { .. }))
                    .count()
                    <= 64
                && (1..=MAX_FILES).contains(&self.files.len()),
            "invalid content index counts"
        );
        let mut total = 0_u64;
        let mut previous = None;
        let mut portable = std::collections::BTreeSet::new();
        for file in &self.files {
            portable_path(&file.path)?;
            let folded = file.path.to_ascii_lowercase();
            ensure!(
                file.path.is_ascii()
                    && folded.split('/').next() != Some(INDEX_FILE)
                    && portable.insert(folded),
                "content files must use unique portable ASCII paths"
            );
            digest(&file.sha256)?;
            ensure!(
                previous.is_none_or(|p: &str| p < file.path.as_str()),
                "pack paths must be unique and sorted"
            );
            previous = Some(&file.path);
            ensure!(file.bytes <= MAX_FILE, "content file exceeds 512 MiB");
            total = total
                .checked_add(file.bytes)
                .context("pack size overflow")?;
            ensure!(total <= MAX_PACK, "content pack exceeds 4 GiB");
        }
        for file in &portable {
            for (offset, _) in file.match_indices('/') {
                ensure!(
                    !portable.contains(&file[..offset]),
                    "content file conflicts with a parent directory"
                );
            }
        }
        for (id, entry) in &self.entries {
            address(id)?;
            ensure!(
                self.files
                    .binary_search_by(|f| f.path.as_str().cmp(entry.path()))
                    .is_ok(),
                "entry '{id}' has no payload"
            );
        }
        Ok(())
    }
}

/// A validated immutable installation. Retain this handle while using resolved files.
pub struct MountedPack {
    root: PathBuf,
    index: Index,
    reference: PackReference,
}
impl MountedPack {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn id(&self) -> &str {
        &self.index.id
    }
    pub fn name(&self) -> &str {
        &self.index.name
    }
    pub fn entries(&self) -> &BTreeMap<String, Entry> {
        &self.index.entries
    }
}
#[derive(Clone)]
pub struct ResolvedContent {
    pack: Arc<MountedPack>,
    entry: Entry,
}
impl ResolvedContent {
    pub fn entry(&self) -> &Entry {
        &self.entry
    }
    pub fn pack(&self) -> &Arc<MountedPack> {
        &self.pack
    }
    pub fn path(&self) -> PathBuf {
        self.pack.root.join(self.entry.path())
    }
    pub fn scene_view(&self) -> Result<Layer> {
        match &self.entry {
            Entry::Scene { view, .. } => {
                view.context("content scene is an additive chunk without a starting view")
            }
            _ => anyhow::bail!("content address is an asset, not a scene"),
        }
    }
    pub fn scene_path(&self) -> Result<PathBuf> {
        ensure!(
            matches!(self.entry, Entry::Scene { .. }),
            "content address is an asset, not a scene"
        );
        Ok(self.path())
    }
    pub fn asset_source(&self) -> Result<AssetSource> {
        match &self.entry {
            Entry::Asset { path, kind } => Ok(AssetSource {
                kind: *kind,
                path: path.clone(),
            }),
            _ => anyhow::bail!("content address is a scene, not an asset"),
        }
    }
}

fn digest(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid SHA-256 digest"
    );
    Ok(())
}
fn address(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value.split('/').all(|s| !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))),
        "content addresses use 1..128 ASCII letters, digits, '_', '-' and nonempty '/' segments"
    );
    Ok(())
}
fn portable_path(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 512
            && !value.contains(['\\', ':'])
            && value.split('/').all(|s| {
                if s.is_empty()
                    || s == "."
                    || s == ".."
                    || s.ends_with([' ', '.'])
                    || s.chars().any(|c| c.is_control() || "<>\"|?*".contains(c))
                {
                    return false;
                }
                let stem = s.split('.').next().unwrap().to_ascii_uppercase();
                !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    && !(stem.len() == 4
                        && (stem.starts_with("COM") || stem.starts_with("LPT"))
                        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
            }),
        "content path must be a portable relative file path: {value}"
    );
    Ok(())
}
