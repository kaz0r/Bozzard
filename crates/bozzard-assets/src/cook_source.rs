//! Immutable source/dependency snapshots for reproducible offline cooking.
use super::*;
use sha2::{Digest, Sha256};

pub struct CookSource {
    kind: AssetKind,
    path: PathBuf,
    snapshot: SourceSnapshot,
    digest: [u8; 32],
}
impl CookSource {
    /// Package embedded maps without rereading source files after a snapshot.
    pub fn image_bytes(name: &str, bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            bytes.len() as u64 <= MAX_SOURCE_BYTES,
            "image source exceeds 32 MiB"
        );
        let path = PathBuf::from(name);
        let mut digest = Sha256::new();
        digest.update(b"bozzard-embedded-image-v1");
        digest.update(
            path.extension()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .as_bytes(),
        );
        digest.update(&bytes);
        Ok(Self {
            kind: AssetKind::Image,
            path,
            snapshot: SourceSnapshot {
                primary: Ok(bytes),
                dependencies: Vec::new(),
            },
            digest: digest.finalize().into(),
        })
    }
    pub fn read(kind: AssetKind, path: &Path, progress: &job::Progress) -> Result<Self> {
        ensure!(
            matches!(kind, AssetKind::Mesh | AssetKind::Image),
            "only meshes and images are cooked"
        );
        progress.stage("Reading cooking dependencies")?;
        let path = path.canonicalize()?;
        let snapshot = source_snapshot(&path)?;
        let mut digest = Sha256::new();
        let mut write = |bytes: &[u8]| {
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        };
        write(format!("{kind:?}").as_bytes());
        let filename = path.file_name().and_then(|p| p.to_str()).unwrap_or("");
        let format = if kind == AssetKind::Mesh && filename.ends_with(".terrain.json") {
            "terrain.json".into()
        } else if kind == AssetKind::Mesh && filename.ends_with(".brush.json") {
            "brush.json".into()
        } else {
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
        };
        write(format.as_bytes());
        write(
            snapshot
                .primary
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?,
        );
        for (dependency, bytes) in &snapshot.dependencies {
            progress.check()?;
            // Relative names affect URI interpretation, but relocating a whole project does not.
            write(
                dependency
                    .strip_prefix(path.parent().context("source parent")?)?
                    .to_str()
                    .context("dependency path is not UTF-8")?
                    .replace('\\', "/")
                    .as_bytes(),
            );
            write(bytes.as_ref().map_err(|e| anyhow::anyhow!(e.clone()))?);
        }
        Ok(Self {
            kind,
            path,
            snapshot,
            digest: digest.finalize().into(),
        })
    }
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
    pub fn content_fingerprint(&self) -> u64 {
        fingerprint_snapshot(&self.snapshot)
    }
    pub fn cook(
        &self,
        formats: &[texture::Compression],
        progress: &job::Progress,
    ) -> Result<Vec<u8>> {
        progress.stage("Decoding source for cooking")?;
        match self.decode()? {
            AssetData::Mesh(mesh) => cooked_model::encode(&mesh, formats, progress),
            AssetData::Image(image) if formats.is_empty() => {
                use image::ImageEncoder;
                let mut bytes = Vec::new();
                image::codecs::png::PngEncoder::new(&mut bytes).write_image(
                    &image.rgba,
                    image.width,
                    image.height,
                    image::ExtendedColorType::Rgba8,
                )?;
                ensure!(
                    bytes.len() as u64 <= MAX_SOURCE_BYTES,
                    "cooked PNG exceeds 32 MiB"
                );
                progress.check()?;
                Ok(bytes)
            }
            AssetData::Image(image) => {
                let cooked = texture::cook(&image, formats, &[true], progress)?;
                texture::encode(&image, &cooked)
            }
            _ => unreachable!("CookSource only accepts meshes and images"),
        }
    }
    /// Decode exactly the bytes used for the cache key, never a second filesystem read.
    pub fn decode(&self) -> Result<AssetData> {
        import(
            self.kind,
            &self.path,
            self.snapshot
                .primary
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?,
            &self.snapshot,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_edits_cannot_change_the_payload_of_an_existing_snapshot() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("bozzard-cook-snapshot-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/assets");
        for name in ["courier.gltf", "courier.bin", "courier-paint.png"] {
            std::fs::copy(fixtures.join(name), root.join(name))?;
        }
        let path = root.join("courier.gltf");
        let progress = job::Progress::default();
        let snapshot = CookSource::read(AssetKind::Mesh, &path, &progress)?;
        let before = snapshot.cook(&[], &progress)?;
        std::fs::copy(fixtures.join("palette.png"), root.join("courier-paint.png"))?;
        let changed = CookSource::read(AssetKind::Mesh, &path, &progress)?;
        assert_ne!(snapshot.digest(), changed.digest());
        assert_ne!(before, changed.cook(&[], &progress)?);
        std::fs::remove_dir_all(root)?;
        assert_eq!(snapshot.cook(&[], &progress)?, before);
        Ok(())
    }
}
