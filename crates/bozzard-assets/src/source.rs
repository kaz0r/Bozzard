//! Source snapshots: a model's bytes and every file it references, read once and fingerprinted.
use super::*;
use std::time::{Duration, SystemTime};

pub(crate) fn read_source(path: &Path) -> Result<Vec<u8>> {
    let limit = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bmesh"))
    {
        cooked_model::MAX_FILE_BYTES as u64
    } else {
        MAX_SOURCE_BYTES
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "source exceeds {} MiB: {}",
        limit / (1024 * 1024),
        path.display()
    );
    Ok(bytes)
}

pub(crate) type SourceDependency = (PathBuf, Result<Vec<u8>, String>);

pub(crate) fn read_dependencies(paths: BTreeSet<PathBuf>) -> Result<Vec<SourceDependency>> {
    ensure!(paths.len() <= 256, "model exceeds 256 external resources");
    let mut total = 0usize;
    let mut dependencies = Vec::new();
    for path in paths {
        let contents = read_source(&path).map_err(|error| format!("{error:#}"));
        if let Ok(bytes) = &contents {
            total += bytes.len();
        }
        ensure!(
            total <= 128 * 1024 * 1024,
            "model source dependencies exceed 128 MiB"
        );
        dependencies.push((path, contents));
    }
    Ok(dependencies)
}

pub(crate) fn source_snapshot(path: &Path) -> Result<SourceSnapshot> {
    let primary = read_source(path).map_err(|error| format!("{error:#}"));
    let mut dependencies = Vec::new();
    let extension = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "gltf" | "glb") {
        if let Ok(bytes) = &primary {
            let gltf = gltf::Gltf::from_slice(bytes).context("parsing glTF dependencies")?;
            let mut paths = BTreeSet::new();
            for buffer in gltf.buffers() {
                if let gltf::buffer::Source::Uri(uri) = buffer.source()
                    && !uri.starts_with("data:")
                {
                    paths.insert(resource_path(path, uri)?);
                }
            }
            for image in gltf.images() {
                if let gltf::image::Source::Uri { uri, .. } = image.source()
                    && !uri.starts_with("data:")
                {
                    paths.insert(resource_path(path, uri)?);
                }
            }
            dependencies = read_dependencies(paths)?;
        }
    } else if extension == "obj"
        && let Ok(bytes) = &primary
    {
        let mut paths = BTreeSet::new();
        for mtl in obj_mtllibs(bytes) {
            paths.insert(resource_path(path, mtl)?);
        }
        let material_paths: Vec<_> = paths.iter().cloned().collect();
        for material_path in material_paths {
            if let Ok(mtl_bytes) = read_source(&material_path) {
                let (materials, _) = tobj::load_mtl_buf(&mut Cursor::new(mtl_bytes))
                    .context("parsing OBJ material library")?;
                for material in materials {
                    if let Some(texture) = material.diffuse_texture {
                        paths.insert(resource_path(&material_path, &texture)?);
                    }
                }
            }
        }
        dependencies = read_dependencies(paths)?;
    }
    Ok(SourceSnapshot {
        primary,
        dependencies,
    })
}

pub(crate) fn obj_mtllibs(bytes: &[u8]) -> impl Iterator<Item = &str> {
    std::str::from_utf8(bytes)
        .unwrap_or("")
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            line.strip_prefix("mtllib")
                .and_then(|rest| rest.strip_prefix(char::is_whitespace))
                .map(str::trim)
        })
        .filter(|path| !path.is_empty())
}

pub(crate) fn fingerprint_snapshot(snapshot: &SourceSnapshot) -> u64 {
    // Content only: Save As and root rebasing must not invalidate the bake.
    let mut hash = 0xcbf29ce484222325_u64;
    for bytes in std::iter::once(&snapshot.primary)
        .chain(snapshot.dependencies.iter().map(|(_, bytes)| bytes))
        .flatten()
    {
        for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    hash
}

/// Length and 128-bit SipHash-1-3 of a source file, kept in place of its bytes. The key is
/// random per process and digests never leave it, so no file can be prepared to match
/// another file's digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Digest {
    length: u64,
    hash: u128,
}
impl Digest {
    pub(crate) fn of(bytes: &[u8]) -> Self {
        use siphasher::sip128::Hasher128;
        use std::hash::{BuildHasher, Hasher};
        static KEYS: std::sync::OnceLock<(u64, u64)> = std::sync::OnceLock::new();
        let &(k0, k1) = KEYS.get_or_init(|| {
            let random = std::collections::hash_map::RandomState::new();
            (random.hash_one(0_u8), random.hash_one(1_u8))
        });
        let mut hasher = siphasher::sip128::SipHasher13::new_with_keys(k0, k1);
        hasher.write(bytes);
        Self {
            length: bytes.len() as u64,
            hash: hasher.finish128().as_u128(),
        }
    }
}

/// What the last read of a source saw: digests of the file and of everything it
/// references, or why each could not be read. Decoded entries keep this, not the bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedSource {
    pub(crate) primary: Result<Digest, String>,
    pub(crate) dependencies: Vec<(PathBuf, Result<Digest, String>)>,
}
impl SourceSnapshot {
    pub(crate) fn observed(&self) -> ObservedSource {
        let digest = |bytes: &Result<Vec<u8>, String>| {
            bytes.as_ref().map(|b| Digest::of(b)).map_err(Clone::clone)
        };
        ObservedSource {
            primary: digest(&self.primary),
            dependencies: self
                .dependencies
                .iter()
                .map(|(path, bytes)| (path.clone(), digest(bytes)))
                .collect(),
        }
    }
}

/// A filesystem may give writes this close together one timestamp, and its clock may lag
/// the system clock slightly, so a change this close to a read cannot be told apart from
/// one just after it.
pub(crate) const SETTLE: Duration = Duration::from_secs(2);

/// File metadata that changes when a file is written, replaced or retargeted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileStamp {
    length: u64,
    modified: Option<SystemTime>,
    /// Device, inode and status-change time. The system sets the status-change time on
    /// every write, rename or permission change, and tools that restore a modification
    /// time cannot restore it.
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl FileStamp {
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        })
    }
    /// True when every change the file records happened at least `SETTLE` before
    /// `read_started`. A later write then gets a later timestamp, and so a different stamp.
    fn settled(&self, read_started: SystemTime) -> bool {
        let before = |time: Option<SystemTime>| {
            time.is_some_and(|time| read_started.duration_since(time).is_ok_and(|d| d >= SETTLE))
        };
        #[cfg(unix)]
        let changed = u64::try_from(self.identity.2)
            .ok()
            .zip(u32::try_from(self.identity.3).ok())
            .map(|(seconds, nanos)| std::time::UNIX_EPOCH + Duration::new(seconds, nanos));
        #[cfg(not(unix))]
        let changed = self.modified;
        before(self.modified) && before(changed)
    }
}

/// Metadata of every file a source read used, taken just after the read. A scan for
/// changes skips reading the source again while all of it is unchanged, but only after a
/// settled read: one that succeeded, whose files all changed last at least `SETTLE` before
/// it started and none of which disappeared after being read. Otherwise the file could
/// have changed during or just after the read without changing its metadata.
#[derive(Clone, Debug)]
pub(crate) struct SourceStamps {
    /// Catalog path these stamps were taken for.
    source: PathBuf,
    files: Vec<(PathBuf, Option<FileStamp>)>,
    pub(crate) settled: bool,
}
impl SourceStamps {
    /// `read` is the path that was read for `source` (a reader may normalize it).
    pub(crate) fn capture(
        source: &Path,
        read: &Path,
        observed: &ObservedSource,
        read_started: SystemTime,
    ) -> Self {
        // A failed read does not record every file it reached, such as a missing parent
        // material, so it is read again on every scan.
        let mut settled = observed.primary.is_ok();
        let files = std::iter::once((read, observed.primary.is_ok()))
            .chain(
                observed
                    .dependencies
                    .iter()
                    .map(|(path, bytes)| (path.as_path(), bytes.is_ok())),
            )
            .map(|(path, was_read)| {
                let stamp = FileStamp::of(path);
                settled &= match &stamp {
                    Some(stamp) => stamp.settled(read_started),
                    // Removed after it was read: it may have changed in between.
                    None => !was_read,
                };
                (path.to_path_buf(), stamp)
            })
            .collect();
        Self {
            source: source.to_path_buf(),
            files,
            settled,
        }
    }
    /// True when `source` was read settled and its files still have the same metadata.
    pub(crate) fn unchanged(&self, source: &Path) -> bool {
        self.settled
            && self.source == source
            && self
                .files
                .iter()
                .all(|(path, stamp)| FileStamp::of(path) == *stamp)
    }
}
