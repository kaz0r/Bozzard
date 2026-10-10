//! Source snapshots: a model's bytes and every file it references, read once and fingerprinted.
use super::*;

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
