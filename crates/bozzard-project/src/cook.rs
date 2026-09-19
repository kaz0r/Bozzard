//! Dependency-addressed, versioned offline cooking shared by exports and content packs.
use anyhow::{Context, Result, ensure};
use bozzard_assets::{CookSource, job::Progress, texture::Compression};
use bozzard_scene::AssetKind;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookTarget {
    #[default]
    Source,
    Rgba,
    Bc,
    Astc,
    Universal,
}
impl CookTarget {
    pub const ALL: [Self; 5] = [
        Self::Universal,
        Self::Bc,
        Self::Astc,
        Self::Rgba,
        Self::Source,
    ];
    pub fn is_source(&self) -> bool {
        *self == Self::Source
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Source => "Original assets",
            Self::Rgba => "Cooked models · lossless textures",
            Self::Bc => "BC3 · desktop GPUs",
            Self::Astc => "ASTC · Apple/mobile GPUs",
            Self::Universal => "BC3 + ASTC · portable",
        }
    }
    fn formats(self) -> &'static [Compression] {
        match self {
            Self::Bc => &[Compression::Bc3],
            Self::Astc => &[Compression::Astc4x4],
            Self::Universal => &[Compression::Bc3, Compression::Astc4x4],
            _ => &[],
        }
    }
    pub(crate) fn extension(self, kind: AssetKind) -> Option<&'static str> {
        match (self, kind) {
            (Self::Source, _) => None,
            (_, AssetKind::Mesh) => Some("bmesh"),
            (Self::Rgba, AssetKind::Image) => Some("png"),
            (_, AssetKind::Image) => Some("btex"),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CookReport {
    pub built: usize,
    pub reused: usize,
    pub copied: usize,
    pub cooked_bytes: u64,
}

pub(crate) struct Cache {
    root: PathBuf,
    target: CookTarget,
    pub report: CookReport,
}
impl Cache {
    pub fn new(root: PathBuf, target: CookTarget) -> Self {
        Self {
            root,
            target,
            report: CookReport::default(),
        }
    }
    pub fn cook(&mut self, source: &CookSource, progress: &Progress) -> Result<Vec<u8>> {
        // Bump this namespace whenever importer behavior, codec settings, dependencies or
        // payload schemas change. Content hashes include every external buffer/map/library.
        let mut key = Sha256::new();
        key.update(b"bozzard-cook-v1:mesh1:texture1:texpresso2.0.2:astcenc0.5.0");
        key.update(serde_json::to_vec(&self.target)?);
        key.update(source.digest());
        let key: [u8; 32] = key.finalize().into();
        let filename: String = key.iter().map(|b| format!("{b:02x}")).collect();
        let path = self.root.join(format!("{filename}.cache"));
        progress.stage("Checking cooked asset cache")?;
        let bytes = if let Some(bytes) = read_cache(&path, &key)? {
            self.report.reused += 1;
            bytes
        } else {
            let bytes = source.cook(self.target.formats(), progress)?;
            progress.check()?;
            fs::create_dir_all(&self.root)?;
            write_cache(&path, &key, &bytes)?;
            self.report.built += 1;
            bytes
        };
        self.report.cooked_bytes += bytes.len() as u64;
        Ok(bytes)
    }
}

fn read_cache(path: &Path, key: &[u8; 32]) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context("reading cooked asset cache"),
    };
    const MAX: u64 = bozzard_assets::cooked_model::MAX_FILE_BYTES as u64 + 72;
    let length = file.metadata()?.len();
    if !(72..=MAX).contains(&length) {
        return Ok(None);
    }
    let mut header = [0; 72];
    let mut file = file.take(MAX + 1);
    if file.read_exact(&mut header).is_err() || &header[..8] != b"BOZZCOOK" || &header[8..40] != key
    {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity((length - 72) as usize);
    file.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX - 72 || Sha256::digest(&bytes).as_slice() != &header[40..] {
        return Ok(None);
    }
    Ok(Some(bytes))
}
fn write_cache(path: &Path, key: &[u8; 32], bytes: &[u8]) -> Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (stage, mut file) = loop {
        let stage = path.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)
        {
            Ok(file) => break (stage, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).context("creating cooked cache staging file"),
        }
    };
    let result = (|| {
        ensure!(
            bytes.len() <= bozzard_assets::cooked_model::MAX_FILE_BYTES,
            "cooked asset is too large"
        );
        file.write_all(b"BOZZCOOK")?;
        file.write_all(key)?;
        file.write_all(&Sha256::digest(bytes))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Rename replaces an old corrupt entry atomically; concurrent identical cooks have
        // the same key and immutable output. On Windows remove only that cache entry first.
        #[cfg(windows)]
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&stage, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&stage);
    }
    result
}
