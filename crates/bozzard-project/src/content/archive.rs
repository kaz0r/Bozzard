use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) struct Staging {
    pub path: PathBuf,
}
impl Staging {
    pub fn new(parent: &Path) -> Result<Self> {
        fs::create_dir_all(parent)?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = parent.join(format!(
                ".bozzard-content-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e).context("creating content staging folder"),
            }
        }
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(super) fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
pub(super) fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    ensure!(
        length <= limit,
        "content file is too large: {}",
        path.display()
    );
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "content file is too large: {}",
        path.display()
    );
    Ok(bytes)
}
fn copy_hash(
    input: &mut impl Read,
    output: &mut impl Write,
    limit: u64,
    progress: &Progress,
) -> Result<(u64, String)> {
    let mut buffer = vec![0; 64 * 1024];
    let mut digest = Sha256::new();
    let mut total = 0;
    loop {
        progress.check()?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        ensure!(total <= limit, "content payload exceeds declared size");
        output.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
    }
    Ok((total, hex(digest.finalize())))
}
pub(super) fn inventory(root: &Path, progress: &Progress) -> Result<Vec<FileEntry>> {
    let mut pending = vec![root.to_owned()];
    let mut files = Vec::new();
    let mut directories = 0;
    while let Some(directory) = pending.pop() {
        directories += 1;
        ensure!(directories <= MAX_FILES, "too many content folders");
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
                "content bundles cannot contain links or special files"
            );
            ensure!(files.len() < MAX_FILES, "content bundle exceeds file limit");
            let path = entry.path();
            let name = path
                .strip_prefix(root)?
                .to_str()
                .context("content filename must be UTF-8")?
                .replace('\\', "/");
            portable_path(&name)?;
            let (bytes, sha256) = copy_hash(
                &mut fs::File::open(&path)?,
                &mut std::io::sink(),
                MAX_FILE,
                progress,
            )?;
            files.push(FileEntry {
                path: name,
                bytes,
                sha256,
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

pub(super) fn write_pack(
    root: &Path,
    path: &Path,
    index: &Index,
    progress: &Progress,
) -> Result<PackReference> {
    index.validate()?;
    let metadata = serde_json::to_vec(index)?;
    ensure!(
        metadata.len() as u64 <= MAX_INDEX,
        "content index exceeds 8 MiB"
    );
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut append = |data: &[u8]| -> Result<()> {
        size += data.len() as u64;
        ensure!(size <= MAX_PACK, "content pack exceeds 4 GiB");
        output.write_all(data)?;
        hash.update(data);
        Ok(())
    };
    append(b"BOZZPACK")?;
    append(&VERSION.to_le_bytes())?;
    append(&(metadata.len() as u32).to_le_bytes())?;
    append(&metadata)?;
    let mut buffer = vec![0; 64 * 1024];
    for file in &index.files {
        progress.stage(format!("Bundling {}", file.path))?;
        let mut input = fs::File::open(root.join(&file.path))?;
        let mut copied = 0;
        let mut file_hash = Sha256::new();
        loop {
            progress.check()?;
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            ensure!(copied <= file.bytes, "content changed while bundling");
            append(&buffer[..count])?;
            file_hash.update(&buffer[..count]);
        }
        ensure!(
            copied == file.bytes && hex(file_hash.finalize()) == file.sha256,
            "content changed while bundling"
        );
    }
    output.sync_all()?;
    Ok(PackReference {
        location: "content.bpack".into(),
        bytes: size,
        sha256: hex(hash.finalize()),
        index_sha256: hex(Sha256::digest(&metadata)),
    })
}

struct CheckedReader<'a, R> {
    reader: R,
    hash: Sha256,
    read: u64,
    limit: u64,
    progress: &'a Progress,
}
impl<R: Read> Read for CheckedReader<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.progress.check().map_err(std::io::Error::other)?;
        let n = self.reader.read(bytes)?;
        self.read += n as u64;
        if self.read > self.limit {
            return Err(std::io::Error::other("content pack exceeds declared size"));
        }
        self.hash.update(&bytes[..n]);
        self.progress
            .set_fraction((self.read as f64 / self.limit as f64) as f32)
            .map_err(std::io::Error::other)?;
        Ok(n)
    }
}

pub(super) fn unpack(
    reader: impl Read,
    root: &Path,
    expected: &PackReference,
    progress: &Progress,
) -> Result<Index> {
    expected.validate()?;
    let mut input = CheckedReader {
        reader,
        hash: Sha256::new(),
        read: 0,
        limit: expected.bytes,
        progress,
    };
    let mut header = [0; 16];
    input.read_exact(&mut header)?;
    ensure!(
        &header[..8] == b"BOZZPACK" && u32::from_le_bytes(header[8..12].try_into()?) == VERSION,
        "unsupported content pack header"
    );
    let size = u32::from_le_bytes(header[12..].try_into()?) as usize;
    ensure!(size as u64 <= MAX_INDEX, "content index exceeds 8 MiB");
    let mut metadata = vec![0; size];
    input.read_exact(&mut metadata)?;
    ensure!(
        hex(Sha256::digest(&metadata)) == expected.index_sha256,
        "content index checksum mismatch"
    );
    let index: Index = serde_json::from_slice(&metadata)?;
    index.validate()?;
    let payload: u64 = index.files.iter().map(|f| f.bytes).sum();
    ensure!(
        16 + size as u64 + payload == expected.bytes,
        "content index sizes do not match the bundle"
    );
    for file in &index.files {
        progress.stage(format!("Installing {}", file.path))?;
        let path = root.join(&file.path);
        fs::create_dir_all(path.parent().context("content file parent")?)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let (bytes, hash) = copy_hash(
            &mut (&mut input).take(file.bytes),
            &mut output,
            file.bytes,
            progress,
        )?;
        ensure!(
            bytes == file.bytes && hash == file.sha256,
            "content file checksum mismatch: {}",
            file.path
        );
    }
    ensure!(
        input.read(&mut [0; 1])? == 0 && input.read == expected.bytes,
        "truncated content bundle"
    );
    ensure!(
        hex(input.hash.finalize()) == expected.sha256,
        "content bundle checksum mismatch"
    );
    fs::write(root.join(INDEX_FILE), metadata)?;
    Ok(index)
}

/// Check every indexed file, including parent directories, before reusing an installation.
pub(super) fn verify_cache(
    root: &Path,
    expected: &PackReference,
    progress: &Progress,
) -> Result<Index> {
    ensure!(
        fs::symlink_metadata(root)?.is_dir(),
        "content cache is not a directory"
    );
    ensure!(
        fs::symlink_metadata(root.join(INDEX_FILE))?.is_file(),
        "content cache index is not a file"
    );
    let metadata = read_bounded(&root.join(INDEX_FILE), MAX_INDEX)?;
    ensure!(
        hex(Sha256::digest(&metadata)) == expected.index_sha256,
        "cached index checksum mismatch"
    );
    let index: Index = serde_json::from_slice(&metadata)?;
    index.validate()?;
    ensure!(
        16 + metadata.len() as u64 + index.files.iter().map(|f| f.bytes).sum::<u64>()
            == expected.bytes,
        "cached index has wrong bundle size"
    );
    // Reconstruct the archive digest in the same pass as per-file verification.
    let mut archive_hash = Sha256::new();
    archive_hash.update(b"BOZZPACK");
    archive_hash.update(VERSION.to_le_bytes());
    archive_hash.update((metadata.len() as u32).to_le_bytes());
    archive_hash.update(&metadata);
    for file in &index.files {
        progress.stage(format!("Checking cached {}", file.path))?;
        let mut path = root.to_path_buf();
        let segments: Vec<_> = file.path.split('/').collect();
        for (i, segment) in segments.iter().enumerate() {
            path.push(segment);
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                if i + 1 == segments.len() {
                    metadata.is_file() && metadata.len() == file.bytes
                } else {
                    metadata.is_dir()
                },
                "invalid cached content path"
            );
        }
        let (bytes, hash) = copy_hash(
            &mut fs::File::open(path)?,
            &mut HashWriter(&mut archive_hash),
            file.bytes,
            progress,
        )?;
        ensure!(
            bytes == file.bytes && hash == file.sha256,
            "cached content checksum mismatch"
        );
    }
    ensure!(
        hex(archive_hash.finalize()) == expected.sha256,
        "cached bundle checksum mismatch"
    );
    Ok(index)
}

struct HashWriter<'a>(&'a mut Sha256);
impl Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Validate typed scene/assets and require every transitive file reference to stay in the index.
pub(super) fn validate_content(root: &Path, index: &Index, progress: &Progress) -> Result<()> {
    let root = root.canonicalize()?;
    let files: BTreeSet<_> = index.files.iter().map(|f| root.join(&f.path)).collect();
    let mut pending = Vec::new();
    let mut scenes = Vec::new();
    for entry in index.entries.values() {
        match entry {
            Entry::Scene { path, view } => {
                let path = root.join(path);
                let scene = bozzard_scene::Scene::from_json(std::str::from_utf8(&read_bounded(
                    &path,
                    64 * 1024 * 1024,
                )?)?)?;
                ensure!(
                    view.is_none_or(|view| scene.views.contains_key(&view)),
                    "content scene has no requested view"
                );
                pending.extend(
                    scene
                        .assets
                        .values()
                        .cloned()
                        .map(|a| (path.parent().unwrap().to_owned(), a)),
                );
                scenes.push((path, scene));
            }
            Entry::Asset { path, kind } => {
                let source = AssetSource {
                    path: path.clone(),
                    kind: *kind,
                };
                pending.push((root.clone(), source));
            }
        }
    }
    let mut seen: BTreeSet<_> = scenes.iter().map(|(path, _)| path.clone()).collect();
    let mut cursor = 0;
    while cursor < scenes.len() {
        progress.check()?;
        let (path, scene) = &scenes[cursor];
        let mut children = Vec::new();
        for input in scene.runtime_scene_sources.values() {
            match input {
                bozzard_scene::scene_loading::SceneSource::File { path: relative } => {
                    portable_path(relative)?;
                    let child = path.parent().unwrap().join(relative).canonicalize()?;
                    ensure!(
                        child.starts_with(&root) && files.contains(&child),
                        "runtime scene resolves outside the bundle index"
                    );
                    if seen.insert(child.clone()) {
                        children.push(child);
                    }
                }
                bozzard_scene::scene_loading::SceneSource::Content { catalog, .. } => {
                    ensure!(
                        catalog.starts_with("https://") || catalog.starts_with("http://"),
                        "local content catalogs must be cooked into the bundle"
                    );
                }
            }
        }
        ensure!(
            seen.len() <= 64,
            "content exceeds 64 transitive scene files"
        );
        cursor += 1;
        for child in children {
            let scene = bozzard_scene::Scene::from_json(std::str::from_utf8(&read_bounded(
                &child,
                64 * 1024 * 1024,
            )?)?)?;
            pending.extend(
                scene
                    .assets
                    .values()
                    .cloned()
                    .map(|asset| (child.parent().unwrap().to_owned(), asset)),
            );
            scenes.push((child, scene));
        }
    }
    // Check every reference before a typed loader can read outside the index. Decode
    // transitive assets as well, even for a prefab that no scene currently instantiates.
    let assets = validate_references(&root, &files, &mut pending, progress)?;
    let mut store = bozzard_assets::AssetStore::new(&root, &assets)?;
    store.load_pending_with(progress)?;
    store.require_ready()?;
    for (path, scene) in scenes {
        for level in std::iter::once(&scene).chain(scene.runtime_scenes.values().map(Arc::as_ref)) {
            progress.check()?;
            let runtime = bozzard_demo::SceneDemo::new_with_prefabs(level, Some(&path))?;
            let mut scene_assets = store.for_catalog(
                path.parent().unwrap(),
                &runtime.instance().document().assets,
            )?;
            scene_assets.load_pending_with(progress)?;
            scene_assets.require_ready()?;
            scene_assets.validate_scene_resources(runtime.instance().document())?;
        }
    }
    Ok(())
}
fn validate_references(
    root: &Path,
    files: &BTreeSet<PathBuf>,
    pending: &mut Vec<(PathBuf, AssetSource)>,
    progress: &Progress,
) -> Result<BTreeMap<String, AssetSource>> {
    let mut seen = BTreeSet::new();
    let mut assets = BTreeMap::new();
    while let Some((directory, source)) = pending.pop() {
        progress.check()?;
        portable_path(&source.path)?;
        let path = directory.join(&source.path).canonicalize()?;
        ensure!(
            path.starts_with(root) && files.contains(&path),
            "content asset resolves outside the bundle index"
        );
        if !seen.insert((format!("{:?}", source.kind), path.clone())) {
            continue;
        }
        ensure!(
            seen.len() <= MAX_FILES,
            "too many transitive content assets"
        );
        assets.insert(
            format!("asset-{}", assets.len()),
            AssetSource {
                kind: source.kind,
                path: path
                    .strip_prefix(root)?
                    .to_str()
                    .context("content asset path")?
                    .replace('\\', "/"),
            },
        );
        if source.kind == AssetKind::Mesh {
            ensure!(
                path.extension().is_some_and(|e| e == "bmesh"),
                "content meshes must be cooked BMESH assets"
            );
        }
        if source.kind == AssetKind::Prefab {
            let prefab = bozzard_scene::Prefab::from_json(std::str::from_utf8(&read_bounded(
                &path,
                32 * 1024 * 1024,
            )?)?)?;
            pending.extend(
                prefab
                    .assets
                    .into_values()
                    .map(|a| (path.parent().unwrap().to_owned(), a)),
            );
        }
    }
    Ok(assets)
}
