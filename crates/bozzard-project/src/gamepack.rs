//! Compressed, deterministic native game data. Checksums detect corruption, not authorship.
//! The filesystem-based importers use a private temporary installation for the lifetime
//! of the player. No editable content is installed alongside the distributed executable.
use crate::{MANIFEST, Project};
use anyhow::{Context, Result, ensure};
use bozzard_assets::job::Progress;
use flate2::{Compression, bufread::ZlibDecoder, write::ZlibEncoder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{BufReader, Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const GAMEPACK: &str = "gamepack.bpack";
const MAGIC: &[u8; 8] = b"BOZZGAME";
const VERSION: u32 = 1;
const MAX_FILES: usize = 8192;
const MAX_INDEX: usize = 8 * 1024 * 1024;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
    bytes: u64,
    sha256: [u8; 32],
}

fn validate(files: &[File]) -> Result<()> {
    ensure!(
        (1..=MAX_FILES).contains(&files.len()),
        "invalid gamepack file count"
    );
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for file in files {
        ensure!(
            !file.path.is_empty()
                && file.path.len() <= 1024
                && !file.path.contains(['\\', ':'])
                && !file.path.chars().any(char::is_control)
                && file
                    .path
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
                && Path::new(&file.path)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)))
                && names.insert(file.path.to_lowercase()),
            "invalid or duplicate gamepack path: {}",
            file.path
        );
        ensure!(file.bytes <= MAX_FILE, "gamepack file exceeds 512 MiB");
        total = total
            .checked_add(file.bytes)
            .context("gamepack size overflow")?;
        ensure!(total <= MAX_TOTAL, "gamepack content exceeds 4 GiB");
    }
    ensure!(names.contains(MANIFEST), "gamepack has no project manifest");
    Ok(())
}

fn copy_checked(
    input: &mut impl Read,
    output: &mut impl Write,
    limit: u64,
    progress: &Progress,
) -> Result<(u64, [u8; 32])> {
    let mut buffer = [0; 64 * 1024];
    let mut bytes = 0;
    let mut hash = Sha256::new();
    loop {
        progress.check()?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        ensure!(bytes <= limit, "gamepack payload exceeds declared size");
        output.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
    }
    Ok((bytes, hash.finalize().into()))
}

/// Compress a validated, relocatable project tree into a new pack file.
pub fn write(root: &Path, destination: &Path, progress: &Progress) -> Result<()> {
    let mut pending = vec![root.to_owned()];
    let mut files = Vec::new();
    let mut directories = 0;
    while let Some(directory) = pending.pop() {
        directories += 1;
        ensure!(directories <= MAX_FILES, "too many gamepack directories");
        for entry in fs::read_dir(directory)? {
            progress.check()?;
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
                continue;
            }
            ensure!(
                kind.is_file(),
                "gamepack cannot contain links or special files"
            );
            ensure!(files.len() < MAX_FILES, "too many gamepack files");
            let path = entry.path();
            let (bytes, sha256) = copy_checked(
                &mut fs::File::open(&path)?,
                &mut std::io::sink(),
                MAX_FILE,
                progress,
            )?;
            files.push(File {
                path: path
                    .strip_prefix(root)?
                    .to_str()
                    .context("gamepack path must be UTF-8")?
                    .replace('\\', "/"),
                bytes,
                sha256,
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    validate(&files)?;
    let index = serde_json::to_vec(&files)?;
    ensure!(index.len() <= MAX_INDEX, "gamepack index exceeds 8 MiB");
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    output.write_all(MAGIC)?;
    output.write_all(&VERSION.to_le_bytes())?;
    output.write_all(&(index.len() as u32).to_le_bytes())?;
    output.write_all(&Sha256::digest(&index))?;
    // Compress the index too, so the package has no loose JSON or script text.
    let mut encoder = ZlibEncoder::new(output, Compression::default());
    encoder.write_all(&index)?;
    for file in &files {
        progress.stage(format!("Compressing {}", file.path))?;
        let (bytes, hash) = copy_checked(
            &mut fs::File::open(root.join(&file.path))?,
            &mut encoder,
            file.bytes,
            progress,
        )?;
        ensure!(
            bytes == file.bytes && hash == file.sha256,
            "game content changed while packing"
        );
    }
    let output = encoder.finish()?;
    ensure!(
        output.metadata()?.len() <= MAX_TOTAL + MAX_INDEX as u64 + 1024 * 1024,
        "compressed gamepack exceeds size limit"
    );
    output.sync_all()?;
    Ok(())
}

/// Owns a verified private installation. Keep this handle alive while using its paths.
pub struct GamePack {
    directory: PathBuf,
}
impl Drop for GamePack {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
impl GamePack {
    fn temporary() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let directory = std::env::temp_dir().join(format!(
                "bozzard-game-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            match builder.create(&directory) {
                Ok(()) => return Ok(Self { directory }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("creating private game data folder"),
            }
        }
    }
    pub fn open(path: &Path, progress: &Progress) -> Result<Self> {
        progress.stage("Opening gamepack.bpack")?;
        let file =
            fs::File::open(path).with_context(|| format!("opening gamepack {}", path.display()))?;
        ensure!(
            file.metadata()?.len() <= MAX_TOTAL + MAX_INDEX as u64 + 1024 * 1024,
            "gamepack exceeds size limit"
        );
        let mut reader = BufReader::new(file);
        let mut header = [0; 48];
        reader
            .read_exact(&mut header)
            .context("truncated gamepack header")?;
        ensure!(
            &header[..8] == MAGIC && u32::from_le_bytes(header[8..12].try_into()?) == VERSION,
            "unsupported gamepack header/version"
        );
        let size = u32::from_le_bytes(header[12..16].try_into()?) as usize;
        ensure!(size <= MAX_INDEX, "gamepack index exceeds 8 MiB");
        let mut decoder = ZlibDecoder::new(reader);
        let mut index = vec![0; size];
        decoder
            .read_exact(&mut index)
            .context("reading gamepack index")?;
        ensure!(
            Sha256::digest(&index)[..] == header[16..],
            "gamepack index checksum mismatch"
        );
        let files: Vec<File> = serde_json::from_slice(&index)?;
        validate(&files)?;
        let pack = Self::temporary()?;
        for file in files {
            progress.stage(format!("Loading {}", file.path))?;
            let path = pack.root().join(file.path);
            fs::create_dir_all(path.parent().context("gamepack file parent")?)?;
            let mut output = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)?;
            let (bytes, hash) = copy_checked(
                &mut (&mut decoder).take(file.bytes),
                &mut output,
                file.bytes,
                progress,
            )?;
            ensure!(
                bytes == file.bytes && hash == file.sha256,
                "gamepack file checksum mismatch: {}",
                path.display()
            );
        }
        ensure!(
            decoder.read(&mut [0; 1])? == 0,
            "gamepack has unexpected payload"
        );
        ensure!(
            decoder.into_inner().read(&mut [0; 1])? == 0,
            "gamepack has trailing compressed data"
        );
        // Validate the starting scene's containment before callers use it.
        Project::load(&pack.project_path())?;
        progress.check()?;
        Ok(pack)
    }
    pub fn root(&self) -> &Path {
        &self.directory
    }
    pub fn project_path(&self) -> PathBuf {
        self.root().join(MANIFEST)
    }
    /// Materialize diagnostic content when the caller explicitly requests a scene snapshot.
    pub fn copy_to(&self, destination: &Path) -> Result<()> {
        fn copy(source: &Path, target: &Path) -> Result<()> {
            fs::create_dir_all(target)?;
            for entry in fs::read_dir(source)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    copy(&entry.path(), &target.join(entry.file_name()))?;
                } else {
                    fs::copy(entry.path(), target.join(entry.file_name()))?;
                }
            }
            Ok(())
        }
        ensure!(
            !destination.exists(),
            "snapshot content folder already exists"
        );
        copy(self.root(), destination)
    }
}

/// Resolve the pack only in the executable's distribution locations, never in cwd.
pub fn bundled_gamepack(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    let adjacent = directory.join(GAMEPACK);
    if adjacent.is_file() {
        return Some(adjacent);
    }
    if directory.file_name().is_some_and(|n| n == "MacOS") {
        let resources = directory.parent()?.join("Resources/game").join(GAMEPACK);
        if resources.is_file() {
            return Some(resources);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Result<GamePack> {
        let root = GamePack::temporary()?;
        fs::write(
            root.root().join(MANIFEST),
            br#"{"version":1,"name":"Packed test","start_scene":"scene.json","view":"3d"}"#,
        )?;
        fs::write(root.root().join("scene.json"), b"{}")?;
        fs::create_dir(root.root().join("assets"))?;
        fs::write(
            root.root().join("assets/game.rhai"),
            b"fn on_update(me, dt) { print(42); }".repeat(1000),
        )?;
        Ok(root)
    }
    #[test]
    fn pack_is_compressed_deterministic_relocatable_and_temporary() -> Result<()> {
        let source = source()?;
        let output = GamePack::temporary()?;
        let first = output.root().join("first.bpack");
        let second = output.root().join("second.bpack");
        write(source.root(), &first, &Default::default())?;
        write(source.root(), &second, &Default::default())?;
        let bytes = fs::read(&first)?;
        assert_eq!(bytes, fs::read(second)?);
        assert!(bytes.len() < 2000);
        assert!(!bytes.windows(12).any(|w| w == b"fn on_update"));
        drop(source);
        let pack = GamePack::open(&first, &Default::default())?;
        assert_eq!(
            fs::read(pack.root().join("assets/game.rhai"))?,
            b"fn on_update(me, dt) { print(42); }".repeat(1000)
        );
        let temporary = pack.root().to_owned();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&temporary)?.permissions().mode() & 0o777,
                0o700
            );
        }
        drop(pack);
        assert!(!temporary.exists());
        Ok(())
    }
    #[test]
    fn damaged_truncated_wrong_version_and_trailing_packs_fail() -> Result<()> {
        let source = source()?;
        let output = GamePack::temporary()?;
        let path = output.root().join(GAMEPACK);
        write(source.root(), &path, &Default::default())?;
        let original = fs::read(&path)?;
        for cutoff in [0, 8, 16, 47, 48, original.len() - 1] {
            fs::write(&path, &original[..cutoff])?;
            assert!(
                GamePack::open(&path, &Default::default()).is_err(),
                "cutoff {cutoff}"
            );
        }
        for index in [0, 8, 16, 47, original.len() / 2, original.len() - 1] {
            let mut bytes = original.clone();
            bytes[index] ^= 1;
            fs::write(&path, bytes)?;
            assert!(
                GamePack::open(&path, &Default::default()).is_err(),
                "damage {index}"
            );
        }
        let mut bytes = original;
        bytes.push(0);
        fs::write(&path, bytes)?;
        assert!(GamePack::open(&path, &Default::default()).is_err());
        Ok(())
    }
    #[test]
    fn pack_rejects_paths_duplicates_and_excessive_sizes() {
        let manifest = || File {
            path: MANIFEST.into(),
            bytes: 1,
            sha256: [0; 32],
        };
        for path in [
            "../escape",
            "/absolute",
            "a/../../escape",
            "a\\escape",
            "C:/escape",
            "a//b",
            "a/./b",
            "",
            "a\nfile",
            "BOZZARD.PROJECT.JSON",
        ] {
            assert!(
                validate(&[
                    manifest(),
                    File {
                        path: path.into(),
                        bytes: 1,
                        sha256: [0; 32]
                    }
                ])
                .is_err(),
                "{path:?}"
            );
        }
        assert!(
            validate(&[File {
                bytes: MAX_FILE + 1,
                ..manifest()
            }])
            .is_err()
        );
        assert!(
            validate(
                &(0..MAX_FILES + 1)
                    .map(|i| File {
                        path: format!("{i}"),
                        bytes: 0,
                        sha256: [0; 32]
                    })
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
        assert!(
            validate(&[File {
                path: "scene.json".into(),
                ..manifest()
            }])
            .is_err()
        );
    }
    #[test]
    fn discovery_prefers_pack_beside_binary_or_in_macos_resources() -> Result<()> {
        let root = GamePack::temporary()?;
        let adjacent = root.root().join(GAMEPACK);
        fs::write(&adjacent, b"pack")?;
        assert_eq!(bundled_gamepack(&root.root().join("Game")), Some(adjacent));
        let resources = root.root().join("Game.app/Contents/Resources/game");
        fs::create_dir_all(&resources)?;
        fs::write(resources.join(GAMEPACK), b"pack")?;
        assert_eq!(
            bundled_gamepack(&root.root().join("Game.app/Contents/MacOS/Game")),
            Some(resources.join(GAMEPACK))
        );
        Ok(())
    }
}
