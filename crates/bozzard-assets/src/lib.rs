//! Synchronous CPU imports and reload state. No GPU or window dependencies.
use anyhow::{Context, Result, bail, ensure};
use bozzard_scene::{AssetKind, AssetSource};
use glam::Vec3;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_VERTICES: usize = 1_000_000;
static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    store: u64,
    index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    Pending,
    Ready,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct MeshData {
    /// Position, normal, UV. Right handed, Y up; UV origin at the top left.
    pub vertices: Vec<[f32; 8]>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug)]
pub enum AssetData {
    Image(ImageData),
    Mesh(MeshData),
}

pub struct Entry {
    pub id: String,
    source: AssetSource,
    state: LoadState,
    data: Option<AssetData>,
    revision: u64,
    // Compare bytes, so same-size edits and coarse filesystem timestamps cannot hide changes.
    observed: Option<Result<Vec<u8>, String>>,
}

impl Entry {
    pub fn state(&self) -> &LoadState {
        &self.state
    }
    pub fn data(&self) -> Option<&AssetData> {
        self.data.as_ref()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

pub struct AssetStore {
    id: u64,
    root: PathBuf,
    entries: Vec<Entry>,
    handles: BTreeMap<String, Handle>,
}

impl AssetStore {
    pub fn new(root: &Path, sources: &BTreeMap<String, AssetSource>) -> Result<Self> {
        let id = NEXT_STORE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| anyhow::anyhow!("asset store ID space exhausted"))?;
        let mut handles = BTreeMap::new();
        let entries = sources
            .iter()
            .enumerate()
            .map(|(index, (name, source))| {
                handles.insert(name.clone(), Handle { store: id, index });
                Entry {
                    id: name.clone(),
                    source: source.clone(),
                    state: LoadState::Pending,
                    data: None,
                    revision: 0,
                    observed: None,
                }
            })
            .collect();
        Ok(Self {
            id,
            root: root.to_path_buf(),
            entries,
            handles,
        })
    }

    pub fn handle(&self, id: &str) -> Option<Handle> {
        self.handles.get(id).copied()
    }
    pub fn get(&self, handle: Handle) -> Option<&Entry> {
        if handle.store != self.id {
            return None;
        }
        self.entries.get(handle.index)
    }
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// Returns every changed state, including failures. A failed reload keeps data/revision intact.
    /// Call at a bounded interval, not every frame. Imports run on the calling thread for now.
    pub fn refresh(&mut self) -> Vec<Handle> {
        let mut changed = Vec::new();
        for (index, entry) in self.entries.iter_mut().enumerate() {
            let path = self.root.join(&entry.source.path);
            let bytes = read_source(&path).map_err(|error| format!("{error:#}"));
            if entry.observed.as_ref() == Some(&bytes) {
                continue;
            }
            let loaded = match &bytes {
                Ok(bytes) => import(entry.source.kind, &path, bytes),
                Err(error) => Err(anyhow::anyhow!(error.clone())),
            };
            entry.observed = Some(bytes);
            match loaded {
                Ok(data) => {
                    entry.data = Some(data);
                    entry.revision += 1;
                    entry.state = LoadState::Ready;
                }
                Err(error) => {
                    entry.state = LoadState::Failed(format!("asset '{}': {error:#}", entry.id))
                }
            }
            changed.push(Handle {
                store: self.id,
                index,
            });
        }
        changed
    }

    pub fn require_ready(&self) -> Result<()> {
        for entry in &self.entries {
            match &entry.state {
                LoadState::Ready => {}
                LoadState::Pending => bail!("asset '{}' has not loaded", entry.id),
                LoadState::Failed(message) => bail!("{message}"),
            }
        }
        Ok(())
    }
}

fn read_source(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_SOURCE_BYTES,
        "source exceeds 32 MiB: {}",
        path.display()
    );
    Ok(bytes)
}

fn import(kind: AssetKind, path: &Path, bytes: &[u8]) -> Result<AssetData> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match kind {
        AssetKind::Image => {
            ensure!(
                matches!(extension.as_str(), "png" | "jpg" | "jpeg"),
                "image import supports PNG/JPEG"
            );
            let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(4096);
            limits.max_image_height = Some(4096);
            limits.max_alloc = Some(128 * 1024 * 1024);
            reader.limits(limits);
            let image = reader.decode().context("decoding image")?.into_rgba8();
            // The first material pass is opaque; silently discarding alpha would hide authoring errors.
            ensure!(
                image.pixels().all(|p| p.0[3] == 255),
                "transparent images need an alpha material (not implemented yet)"
            );
            Ok(AssetData::Image(ImageData {
                width: image.width(),
                height: image.height(),
                rgba: image.into_raw(),
            }))
        }
        AssetKind::Mesh => {
            ensure!(extension == "obj", "mesh import supports OBJ");
            let (models, materials) = tobj::load_obj_buf(
                &mut Cursor::new(bytes),
                &tobj::LoadOptions {
                    single_index: true,
                    triangulate: true,
                    ignore_points: true,
                    ignore_lines: true,
                },
                |_| Err(tobj::LoadError::MaterialParseError),
            )
            .context("parsing OBJ")?;
            // Textures/materials are explicit scene dependencies; do not silently follow an MTL file.
            materials.context(
                "OBJ material libraries are unsupported; assign the material in the scene",
            )?;
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            for model in models {
                let mesh = model.mesh;
                let count = mesh.positions.len() / 3;
                ensure!(
                    vertices.len() + count <= MAX_VERTICES,
                    "mesh exceeds one million vertices"
                );
                ensure!(
                    mesh.indices.len().is_multiple_of(3),
                    "OBJ did not produce triangles"
                );
                ensure!(
                    mesh.indices.iter().all(|i| (*i as usize) < count),
                    "invalid mesh index"
                );
                ensure!(
                    mesh.positions
                        .iter()
                        .chain(&mesh.normals)
                        .chain(&mesh.texcoords)
                        .all(|v| v.is_finite()),
                    "non-finite mesh data"
                );
                ensure!(
                    mesh.normals.is_empty() || mesh.normals.len() == count * 3,
                    "incomplete normals"
                );
                ensure!(
                    mesh.texcoords.is_empty() || mesh.texcoords.len() == count * 2,
                    "incomplete UVs"
                );
                let mut normals = vec![Vec3::ZERO; count];
                if mesh.normals.is_empty() {
                    for triangle in mesh.indices.chunks_exact(3) {
                        let position = |i: u32| {
                            Vec3::from_slice(&mesh.positions[i as usize * 3..i as usize * 3 + 3])
                        };
                        let n = (position(triangle[1]) - position(triangle[0]))
                            .cross(position(triangle[2]) - position(triangle[0]));
                        for &i in triangle {
                            normals[i as usize] += n;
                        }
                    }
                } else {
                    for (normal, values) in normals.iter_mut().zip(mesh.normals.chunks_exact(3)) {
                        *normal = Vec3::from_slice(values);
                    }
                }
                let base = vertices.len() as u32;
                for (i, normal) in normals.into_iter().enumerate() {
                    let normal = normal
                        .try_normalize()
                        .context("mesh contains a zero/invalid normal or degenerate geometry")?;
                    let p = &mesh.positions[i * 3..i * 3 + 3];
                    let uv = if mesh.texcoords.is_empty() {
                        [0.0, 0.0]
                    } else {
                        [mesh.texcoords[i * 2], 1.0 - mesh.texcoords[i * 2 + 1]]
                    };
                    vertices.push([p[0], p[1], p[2], normal.x, normal.y, normal.z, uv[0], uv[1]]);
                }
                indices.extend(mesh.indices.into_iter().map(|i| base + i));
            }
            ensure!(
                !vertices.is_empty() && !indices.is_empty() && indices.len() <= 3_000_000,
                "empty or oversized triangle mesh"
            );
            Ok(AssetData::Mesh(MeshData { vertices, indices }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PNG: &[u8] = include_bytes!("../../../examples/demo/scenes/assets/palette.png");
    const NEXT_PNG: &[u8] =
        include_bytes!("../../../examples/demo/scenes/assets/palette-reloaded.png");
    const OBJ: &[u8] = include_bytes!("../../../examples/demo/scenes/assets/quad.obj");
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "bozzard-assets-{}-{}",
                std::process::id(),
                NEXT_STORE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reload_keeps_handles_and_last_good_data_through_corruption_deletion_and_recovery() {
        let dir = Temp::new();
        let sources = BTreeMap::from([(
            "palette".into(),
            AssetSource {
                kind: AssetKind::Image,
                path: "palette.png".into(),
            },
        )]);
        let mut store = AssetStore::new(&dir.0, &sources).unwrap();
        let h = store.handle("palette").unwrap();
        assert_eq!(store.get(h).unwrap().state(), &LoadState::Pending);
        let other = AssetStore::new(&dir.0, &sources).unwrap();
        assert!(other.get(h).is_none());
        std::fs::write(dir.0.join("palette.png"), PNG).unwrap();
        assert_eq!(store.refresh(), vec![h]);
        assert_eq!(store.get(h).unwrap().revision(), 1);
        assert!(store.refresh().is_empty());
        std::fs::write(dir.0.join("palette.png"), b"broken").unwrap();
        assert_eq!(store.refresh(), vec![h]);
        assert!(matches!(
            store.get(h).unwrap().state(),
            LoadState::Failed(_)
        ));
        assert_eq!(store.get(h).unwrap().revision(), 1);
        let AssetData::Image(image) = store.get(h).unwrap().data().unwrap() else {
            panic!()
        };
        assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
        assert!(store.refresh().is_empty());
        std::fs::remove_file(dir.0.join("palette.png")).unwrap();
        assert_eq!(store.refresh(), vec![h]);
        assert_eq!(store.get(h).unwrap().revision(), 1);
        std::fs::write(dir.0.join("palette.png"), NEXT_PNG).unwrap();
        assert_eq!(store.refresh(), vec![h]);
        store.require_ready().unwrap();
        assert_eq!(store.get(h).unwrap().revision(), 2);
        let AssetData::Image(image) = store.get(h).unwrap().data().unwrap() else {
            panic!()
        };
        assert_eq!(&image.rgba[..4], &[0, 255, 255, 255]);
    }

    #[test]
    fn obj_import_generates_normals_and_flips_uv_origin() {
        let AssetData::Mesh(mesh) = import(AssetKind::Mesh, Path::new("quad.obj"), OBJ).unwrap()
        else {
            panic!()
        };
        assert_eq!(mesh.indices.len(), 6);
        assert_eq!(mesh.vertices[0], [-0.5, -0.5, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0]);
        for invalid in [
            b"f 1 2 3".as_slice(),
            b"v 0 0 0\nf 1 1 1",
            b"mtllib ignored.mtl\nv 0 0 0\nf 1 1 1",
        ] {
            assert!(import(AssetKind::Mesh, Path::new("bad.obj"), invalid).is_err());
        }
    }

    #[test]
    fn jpeg_import_and_image_limits_are_enforced() {
        let encode = |image: image::DynamicImage, format| {
            let mut output = Cursor::new(Vec::new());
            image.write_to(&mut output, format).unwrap();
            output.into_inner()
        };
        let jpeg = encode(
            image::RgbImage::from_pixel(2, 2, image::Rgb([100, 150, 200])).into(),
            image::ImageFormat::Jpeg,
        );
        let AssetData::Image(decoded) =
            import(AssetKind::Image, Path::new("photo.jpg"), &jpeg).unwrap()
        else {
            panic!()
        };
        assert_eq!((decoded.width, decoded.height), (2, 2));
        assert!(
            decoded.rgba[..3]
                .iter()
                .zip([100, 150, 200])
                .all(|(a, b)| a.abs_diff(b) <= 4)
        );
        let alpha = encode(
            image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 128])).into(),
            image::ImageFormat::Png,
        );
        assert!(import(AssetKind::Image, Path::new("alpha.png"), &alpha).is_err());
        let wide = encode(
            image::RgbImage::new(4097, 1).into(),
            image::ImageFormat::Png,
        );
        assert!(import(AssetKind::Image, Path::new("wide.png"), &wide).is_err());
    }
}
