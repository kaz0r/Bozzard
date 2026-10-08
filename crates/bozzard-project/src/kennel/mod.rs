//! Kennel, Bozzard's package store. A registry is a folder or HTTPS base holding `index.json`
//! and `packages/<name>/<name>.pkg.json`. Hashes chain index → manifest → file → upstream
//! archive, so an installation is exactly what the registry published. Checksums are not
//! signatures: install packages from registries whose game code you intend to run.
mod install;
mod manifest;
mod registry;
mod tar;

use crate::content::default_cache_directory;
use anyhow::{Result, ensure};
pub use install::{
    InstallOptions, Installed, LockedPackage, Lockfile, install, project_root, remove, verify,
};
pub use manifest::{
    Archive, Asset, Bin, Category, Engine, Manifest, PackageFile, Payload, Script, Source,
};
pub use registry::{CheckReport, Index, IndexEntry, Registry, build_index, check};
use std::path::PathBuf;

/// Format version of manifests, indexes and lockfiles.
pub const KENNEL: u32 = 1;
pub const DEFAULT_REGISTRY: &str =
    "https://raw.githubusercontent.com/Bozzard-Engine/bozzard-plugin-library/main";
pub const INDEX_FILE: &str = "index.json";
pub const PACKAGES_DIR: &str = "packages";
/// Installed packages live in `<project>/kennel/<name>/`.
pub const INSTALL_DIR: &str = "kennel";
pub const LOCKFILE: &str = "kennel.lock.json";
/// The engine version package requirements are matched against.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

const MAX_DOCUMENT: u64 = 1024 * 1024;
const MAX_PACKAGES: usize = 4096;
const MAX_FILES: usize = 1024;
const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
const MAX_UNPACKED: u64 = 1024 * 1024 * 1024;

/// The `<os>-<arch>` target bins are selected for by default.
pub fn host_target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// `BOZZARD_KENNEL_REGISTRY`, or the public registry.
pub fn default_registry() -> String {
    std::env::var("BOZZARD_KENNEL_REGISTRY")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY.to_owned())
}

/// Content-addressed upstream downloads: `BOZZARD_KENNEL_CACHE`, or beside the content cache.
pub fn cache_directory() -> Result<PathBuf> {
    match std::env::var_os("BOZZARD_KENNEL_CACHE").filter(|value| !value.is_empty()) {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(default_cache_directory()?.with_file_name("kennel")),
    }
}

/// Package names: 1..64 lowercase letters, digits and inner '-'.
fn valid_name(name: &str) -> Result<()> {
    ensure!(
        (1..=64).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !name.starts_with('-')
            && !name.ends_with('-'),
        "package names use 1..64 lowercase letters, digits and inner '-': '{name}'"
    );
    Ok(())
}
