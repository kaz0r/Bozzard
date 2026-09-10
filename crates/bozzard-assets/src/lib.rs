//! CPU imports and background loading. No GPU or window dependencies.
pub mod job;
mod package;
mod pbr;
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bozzard_scene::{AssetKind, AssetSource};
use glam::{Mat3, Mat4, Vec3};
pub use package::{ModelPackage, package_gltf};
pub use pbr::{Filter, PbrMaterial, Sampler, SurfaceShading, TextureMap, Wrap};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DECODED_IMAGE_BYTES: usize = 128 * 1024 * 1024;
// Sponza's complete set of shared PBR maps decodes to 272 MiB.
const MAX_GLTF_IMAGE_BYTES: usize = 512 * 1024 * 1024;
const MAX_VERTICES: usize = 1_000_000;
const MAX_PARTS: usize = 4096;
const MAX_NODE_DEPTH: usize = 256;
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
    /// glTF primitive material slices. Empty for OBJ, which uses the scene material.
    pub parts: Vec<MeshPart>,
    /// Material features present in the source but outside the current renderer.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct MeshPart {
    pub start: u32,
    pub count: u32,
    /// glTF baseColorFactor, including alpha.
    pub color: [f32; 4],
    pub image: Option<Arc<ImageData>>,
    /// Present for glTF `alphaMode: MASK`.
    pub alpha_cutoff: Option<f32>,
    pub shading: Option<SurfaceShading>,
}

#[derive(Clone, Debug)]
pub enum AssetData {
    Image(ImageData),
    Mesh(MeshData),
}

#[derive(Clone)]
pub struct Entry {
    pub id: String,
    source: AssetSource,
    state: LoadState,
    data: Option<Arc<AssetData>>,
    revision: u64,
    // Compare bytes, so same-size edits and coarse filesystem timestamps cannot hide changes.
    observed: Option<Arc<SourceSnapshot>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceSnapshot {
    primary: Result<Vec<u8>, String>,
    dependencies: Vec<SourceDependency>,
}

impl Entry {
    pub fn state(&self) -> &LoadState {
        &self.state
    }
    pub fn data(&self) -> Option<&AssetData> {
        self.data.as_deref()
    }
    /// Immutable data identity survives catalog snapshots and Undo/Redo.
    pub fn shared_data(&self) -> Option<Arc<AssetData>> {
        self.data.clone()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Clone)]
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

    /// Reuse decoded data when catalog paths still resolve to the same file.
    pub fn for_catalog(
        &self,
        root: &Path,
        sources: &BTreeMap<String, AssetSource>,
    ) -> Result<Self> {
        let mut next = Self::new(root, sources)?;
        for entry in &mut next.entries {
            if let Some(old) = self.handle(&entry.id).and_then(|h| self.get(h)) {
                let old_path = self.root.join(&old.source.path);
                let new_path = root.join(&entry.source.path);
                let same_path = old_path == new_path
                    || std::fs::canonicalize(&old_path)
                        .ok()
                        .zip(std::fs::canonicalize(&new_path).ok())
                        .is_some_and(|(a, b)| a == b);
                if old.source.kind == entry.source.kind && same_path {
                    let source = entry.source.clone();
                    *entry = old.clone();
                    entry.source = source;
                }
            }
        }
        Ok(next)
    }

    pub fn load_pending(&mut self) -> Result<()> {
        let mut pending = Self::new(
            &self.root,
            &self
                .entries
                .iter()
                .filter(|entry| entry.observed.is_none())
                .map(|entry| (entry.id.clone(), entry.source.clone()))
                .collect(),
        )?;
        pending.refresh();
        pending.require_ready()?;
        for entry in &mut self.entries {
            if let Some(loaded) = pending.handle(&entry.id).and_then(|h| pending.get(h)) {
                *entry = loaded.clone();
            }
        }
        Ok(())
    }

    /// Returns every changed state, including failures. A failed reload keeps data/revision intact.
    /// Call at a bounded interval, not every frame. Imports run on the calling thread for now.
    pub fn refresh(&mut self) -> Vec<Handle> {
        self.refresh_with(&job::Progress::default())
            .expect("uncancelled refresh")
    }

    pub fn refresh_with(&mut self, progress: &job::Progress) -> Result<Vec<Handle>> {
        let mut changed = Vec::new();
        let total = self.entries.len();
        for (index, entry) in self.entries.iter_mut().enumerate() {
            progress.stage(format!("Checking {} ({}/{total})", entry.id, index + 1))?;
            let path = self.root.join(&entry.source.path);
            let snapshot = source_snapshot(&path).map_err(|error| format!("{error:#}"));
            let snapshot = match snapshot {
                Ok(snapshot) => snapshot,
                Err(error) => SourceSnapshot {
                    primary: Err(error),
                    dependencies: Vec::new(),
                },
            };
            if entry.observed.as_deref() == Some(&snapshot) {
                continue;
            }
            progress.stage(format!("Decoding {} ({}/{total})", entry.id, index + 1))?;
            let loaded = match &snapshot.primary {
                Ok(bytes) => import(entry.source.kind, &path, bytes, &snapshot),
                Err(error) => Err(anyhow::anyhow!(error.clone())),
            };
            progress.check()?;
            entry.observed = Some(Arc::new(snapshot));
            match loaded {
                Ok(data) => {
                    entry.data = Some(Arc::new(data));
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
        Ok(changed)
    }

    /// The worker owns a cheap snapshot; callers publish only when their catalog still matches.
    pub fn refresh_job(&self) -> Result<job::Job<(Self, Vec<Handle>)>> {
        let mut store = self.clone();
        job::Job::start("Checking assets", move |progress| {
            let changed = store.refresh_with(&progress)?;
            Ok((store, changed))
        })
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

type SourceDependency = (PathBuf, Result<Vec<u8>, String>);

fn read_dependencies(paths: BTreeSet<PathBuf>) -> Result<Vec<SourceDependency>> {
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

fn source_snapshot(path: &Path) -> Result<SourceSnapshot> {
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

fn obj_mtllibs(bytes: &[u8]) -> impl Iterator<Item = &str> {
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

fn import(
    kind: AssetKind,
    path: &Path,
    bytes: &[u8],
    snapshot: &SourceSnapshot,
) -> Result<AssetData> {
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
            Ok(AssetData::Image(ImageData {
                width: image.width(),
                height: image.height(),
                rgba: image.into_raw(),
            }))
        }
        AssetKind::Mesh => {
            if matches!(extension.as_str(), "gltf" | "glb") {
                return Ok(AssetData::Mesh(import_gltf(path, bytes, snapshot)?));
            }
            ensure!(extension == "obj", "mesh import supports OBJ/glTF/GLB");
            let (models, materials) = tobj::load_obj_buf(
                &mut Cursor::new(bytes),
                &tobj::LoadOptions {
                    single_index: true,
                    triangulate: true,
                    ignore_points: true,
                    ignore_lines: true,
                },
                |mtl| {
                    let Ok(resource) = resource_path(path, &mtl.to_string_lossy()) else {
                        return Err(tobj::LoadError::OpenFileFailed);
                    };
                    let Some((_, Ok(contents))) = snapshot
                        .dependencies
                        .iter()
                        .find(|(candidate, _)| *candidate == resource)
                    else {
                        return Err(tobj::LoadError::OpenFileFailed);
                    };
                    tobj::load_mtl_buf(&mut Cursor::new(contents))
                },
            )
            .context("parsing OBJ")?;
            let materials = materials.context("parsing OBJ material library")?;
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            let mut parts = Vec::new();
            let mut warnings = Vec::new();
            let mut image_bytes = 0usize;
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
                let start = u32::try_from(indices.len()).context("mesh index count overflow")?;
                indices.extend(mesh.indices.into_iter().map(|i| base + i));
                if !materials.is_empty() {
                    let material = mesh.material_id.and_then(|index| materials.get(index));
                    let mut color = [1.0, 1.0, 1.0, 1.0];
                    let mut image = None;
                    if let Some(material) = material {
                        if let Some(diffuse) = material.diffuse {
                            color[..3].copy_from_slice(&diffuse);
                        }
                        color[3] = material.dissolve.unwrap_or(1.0);
                        if let Some(texture) = &material.diffuse_texture {
                            let mtl_path = obj_mtl_path(path, bytes, snapshot, &material.name)?;
                            let texture_path = resource_path(&mtl_path, texture)?;
                            let contents = snapshot
                                .dependencies
                                .iter()
                                .find(|(candidate, _)| *candidate == texture_path)
                                .context("OBJ diffuse texture was not observed")?
                                .1
                                .as_ref()
                                .map_err(|error| anyhow::anyhow!(error.clone()))?;
                            image = Some(Arc::new(decoded_image(contents, "OBJ diffuse texture")?));
                        }
                        if material.normal_texture.is_some()
                            || material.specular_texture.is_some()
                            || material.emissive.is_some()
                        {
                            warnings.push("OBJ normal, specular, and emissive material properties are not rendered".into());
                        }
                    }
                    ensure!(
                        color
                            .iter()
                            .all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                        "invalid OBJ diffuse color or opacity"
                    );
                    ensure!(parts.len() < MAX_PARTS, "OBJ has too many material parts");
                    image_bytes += image.as_ref().map_or(0, |image| image.rgba.len());
                    ensure!(
                        image_bytes <= MAX_DECODED_IMAGE_BYTES,
                        "decoded OBJ images exceed 128 MiB"
                    );
                    parts.push(MeshPart {
                        start,
                        count: u32::try_from(indices.len()).context("mesh index count overflow")?
                            - start,
                        color,
                        image,
                        alpha_cutoff: None,
                        shading: None,
                    });
                }
            }
            ensure!(
                !vertices.is_empty() && !indices.is_empty() && indices.len() <= 3_000_000,
                "empty or oversized triangle mesh"
            );
            Ok(AssetData::Mesh(MeshData {
                vertices,
                indices,
                parts,
                warnings,
            }))
        }
    }
}

fn obj_mtl_path(
    path: &Path,
    obj: &[u8],
    snapshot: &SourceSnapshot,
    material_name: &str,
) -> Result<PathBuf> {
    for uri in obj_mtllibs(obj) {
        let mtl_path = resource_path(path, uri)?;
        let Some((_, Ok(contents))) = snapshot
            .dependencies
            .iter()
            .find(|(candidate, _)| *candidate == mtl_path)
        else {
            continue;
        };
        let (materials, _) = tobj::load_mtl_buf(&mut Cursor::new(contents))
            .context("parsing OBJ material library")?;
        if materials
            .iter()
            .any(|material| material.name == material_name)
        {
            return Ok(mtl_path);
        }
    }
    bail!("could not locate OBJ material library for '{material_name}'")
}

fn resource_path(source: &Path, uri: &str) -> Result<PathBuf> {
    ensure!(!uri.contains('\0'), "resource URI contains NUL");
    ensure!(
        !uri.contains(':') && !uri.starts_with('/') && !uri.starts_with('\\'),
        "only relative local resource URIs are supported: {uri}"
    );
    let decoded = percent_decode(uri)?;
    let relative = Path::new(&decoded);
    ensure!(
        relative.components().all(|part| matches!(
            part,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )),
        "resource URI escapes its glTF directory: {uri}"
    );
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(relative))
}

fn percent_decode(uri: &str) -> Result<String> {
    let mut result = Vec::with_capacity(uri.len());
    let bytes = uri.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            ensure!(
                i + 2 < bytes.len(),
                "invalid percent escape in resource URI: {uri}"
            );
            let value = |byte: u8| -> Result<u8> {
                match byte {
                    b'0'..=b'9' => Ok(byte - b'0'),
                    b'a'..=b'f' => Ok(byte - b'a' + 10),
                    b'A'..=b'F' => Ok(byte - b'A' + 10),
                    _ => bail!("invalid percent escape in resource URI: {uri}"),
                }
            };
            result.push(value(bytes[i + 1])? * 16 + value(bytes[i + 2])?);
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result).context("resource URI is not UTF-8")
}

fn data_uri(uri: &str) -> Result<Vec<u8>> {
    let (header, encoded) = uri.split_once(',').context("malformed data URI")?;
    ensure!(
        header.starts_with("data:") && header.ends_with(";base64"),
        "only base64 data URIs are supported"
    );
    let bytes = STANDARD
        .decode(encoded)
        .context("decoding base64 data URI")?;
    ensure!(
        bytes.len() as u64 <= MAX_SOURCE_BYTES,
        "data URI exceeds 32 MiB"
    );
    Ok(bytes)
}

fn snapshot_resource<'a>(path: &Path, uri: &str, snapshot: &'a SourceSnapshot) -> Result<&'a [u8]> {
    if uri.starts_with("data:") {
        bail!("data URI cannot borrow source bytes")
    }
    let resource = resource_path(path, uri)?;
    snapshot
        .dependencies
        .iter()
        .find(|(candidate, _)| *candidate == resource)
        .context("resource was not observed")?
        .1
        .as_ref()
        .map(Vec::as_slice)
        .map_err(|error| anyhow::anyhow!(error.clone()))
}

fn decoded_image(bytes: &[u8], label: &str) -> Result<ImageData> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .with_context(|| format!("recognizing image {label}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .with_context(|| format!("decoding image {label}"))?
        .into_rgba8();
    Ok(ImageData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

fn gltf_preflight(bytes: &[u8]) -> Result<serde_json::Value> {
    let json: serde_json::Value = if bytes.starts_with(b"glTF") {
        let glb = gltf::binary::Glb::from_slice(bytes).context("parsing GLB")?;
        serde_json::from_slice(&glb.json).context("parsing GLB JSON")?
    } else {
        serde_json::from_slice(bytes).context("parsing glTF JSON")?
    };
    ensure!(
        json.get("extensionsRequired")
            .and_then(serde_json::Value::as_array)
            .is_none_or(Vec::is_empty),
        "required glTF extensions are not supported by this importer"
    );
    for key in ["skins", "animations"] {
        ensure!(
            json.get(key)
                .and_then(serde_json::Value::as_array)
                .is_none_or(Vec::is_empty),
            "static glTF import does not support {key}"
        );
    }
    if let Some(meshes) = json.get("meshes").and_then(serde_json::Value::as_array) {
        ensure!(
            meshes.iter().all(|mesh| mesh.get("weights").is_none()
                && mesh
                    .get("primitives")
                    .and_then(serde_json::Value::as_array)
                    .is_none_or(|p| p.iter().all(|x| x.get("targets").is_none()))),
            "static glTF import does not support morph targets"
        );
    }
    let extensions = json
        .get("extensionsUsed")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str);
    for extension in extensions {
        ensure!(
            !matches!(
                extension,
                "KHR_draco_mesh_compression"
                    | "EXT_meshopt_compression"
                    | "KHR_texture_basisu"
                    | "EXT_texture_webp"
                    | "MSFT_texture_dds"
            ),
            "static glTF import does not support compressed geometry/textures ({extension})"
        );
    }
    Ok(json)
}

fn import_gltf(path: &Path, bytes: &[u8], snapshot: &SourceSnapshot) -> Result<MeshData> {
    gltf_preflight(bytes)?;
    let gltf = gltf::Gltf::from_slice(bytes).context("parsing validated glTF")?;
    let mut buffers = Vec::new();
    let mut buffer_bytes = 0usize;
    for buffer in gltf.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => gltf
                .blob
                .as_deref()
                .context("GLB buffer is missing BIN chunk")?
                .to_vec(),
            gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => data_uri(uri)?,
            gltf::buffer::Source::Uri(uri) => snapshot_resource(path, uri, snapshot)?.to_vec(),
        };
        ensure!(
            data.len() >= buffer.length(),
            "glTF buffer {} is shorter than declared",
            buffer.index()
        );
        buffer_bytes += data.len();
        ensure!(
            buffer_bytes <= 128 * 1024 * 1024,
            "decoded model buffers exceed 128 MiB"
        );
        buffers.push(data);
    }
    let mut warnings = Vec::new();
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .context("glTF has no scene")?;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut parts = Vec::new();
    let mut images = ModelImages::default();
    let mut visited = BTreeSet::new();
    for node in scene.nodes() {
        append_node(
            node,
            Mat4::IDENTITY,
            &buffers,
            path,
            snapshot,
            &mut vertices,
            &mut indices,
            &mut parts,
            &mut warnings,
            &mut images,
            &mut visited,
            0,
        )?;
    }
    ensure!(
        !vertices.is_empty() && !indices.is_empty() && indices.len() <= 3_000_000,
        "empty or oversized triangle mesh"
    );
    Ok(MeshData {
        vertices,
        indices,
        parts,
        warnings,
    })
}

// Shared accumulators keep recursive traversal allocation bounded.
#[allow(clippy::too_many_arguments)]
fn append_node(
    node: gltf::Node<'_>,
    parent: Mat4,
    buffers: &[Vec<u8>],
    path: &Path,
    snapshot: &SourceSnapshot,
    vertices: &mut Vec<[f32; 8]>,
    indices: &mut Vec<u32>,
    parts: &mut Vec<MeshPart>,
    warnings: &mut Vec<String>,
    images: &mut ModelImages,
    visited: &mut BTreeSet<usize>,
    depth: usize,
) -> Result<()> {
    ensure!(visited.len() < 65_536, "glTF scene exceeds 65536 nodes");
    ensure!(
        visited.insert(node.index()),
        "glTF scene contains a cycle or shared child node"
    );
    ensure!(
        depth <= MAX_NODE_DEPTH,
        "glTF node hierarchy exceeds 256 levels"
    );
    ensure!(
        node.skin().is_none(),
        "static glTF import does not support skinned nodes"
    );
    ensure!(
        node.weights().is_none(),
        "static glTF import does not support morph-weighted nodes"
    );
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        ensure!(
            mesh.weights().is_none(),
            "static glTF import does not support mesh morph weights"
        );
        for primitive in mesh.primitives() {
            append_primitive(
                primitive, transform, buffers, path, snapshot, vertices, indices, parts, warnings,
                images,
            )?;
        }
    }
    for child in node.children() {
        append_node(
            child,
            transform,
            buffers,
            path,
            snapshot,
            vertices,
            indices,
            parts,
            warnings,
            images,
            visited,
            depth + 1,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_primitive(
    primitive: gltf::Primitive<'_>,
    transform: Mat4,
    buffers: &[Vec<u8>],
    path: &Path,
    snapshot: &SourceSnapshot,
    vertices: &mut Vec<[f32; 8]>,
    indices: &mut Vec<u32>,
    parts: &mut Vec<MeshPart>,
    warnings: &mut Vec<String>,
    images: &mut ModelImages,
) -> Result<()> {
    ensure!(
        primitive.mode() == gltf::mesh::Mode::Triangles,
        "glTF primitive mode must be TRIANGLES"
    );
    ensure!(
        primitive.morph_targets().next().is_none(),
        "static glTF import does not support morph targets"
    );
    if primitive.get(&gltf::Semantic::Colors(0)).is_some() {
        warnings.push("Vertex colors are not rendered; base-color materials are used".into());
    }
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
    let material = primitive.material();
    let pbr = material.pbr_metallic_roughness();
    let texcoord_set = pbr
        .base_color_texture()
        .map_or(0, |texture| texture.tex_coord());
    let positions: Vec<Vec3> = reader
        .read_positions()
        .context("glTF primitive lacks POSITION")?
        .map(Vec3::from_array)
        .collect();
    ensure!(!positions.is_empty(), "glTF primitive has no vertices");
    ensure!(
        vertices.len() + positions.len() <= MAX_VERTICES,
        "mesh exceeds one million vertices"
    );
    let mut primitive_indices: Vec<u32> = reader
        .read_indices()
        .map(|v| v.into_u32().collect())
        .unwrap_or_else(|| (0..positions.len() as u32).collect());
    ensure!(
        primitive_indices.len().is_multiple_of(3)
            && primitive_indices
                .iter()
                .all(|&i| (i as usize) < positions.len()),
        "invalid glTF triangle indices"
    );
    let determinant = transform.determinant();
    ensure!(
        determinant.is_finite() && determinant != 0.0,
        "glTF node has a singular transform"
    );
    let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
    let missing_normals = reader.read_normals().is_none();
    let mut normals: Vec<Vec3> = reader
        .read_normals()
        .map(|v| v.map(Vec3::from_array).collect())
        .unwrap_or_else(|| vec![Vec3::ZERO; positions.len()]);
    ensure!(
        normals.len() == positions.len(),
        "glTF normal count does not match POSITION"
    );
    if missing_normals {
        for triangle in primitive_indices.chunks_exact(3) {
            let normal = (positions[triangle[1] as usize] - positions[triangle[0] as usize])
                .cross(positions[triangle[2] as usize] - positions[triangle[0] as usize]);
            for &index in triangle {
                normals[index as usize] += normal;
            }
        }
    }
    if determinant < 0.0 {
        for triangle in primitive_indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }
    let texcoords: Vec<[f32; 2]> = match reader.read_tex_coords(texcoord_set) {
        Some(values) => values.into_f32().collect(),
        None if pbr.base_color_texture().is_none() => vec![[0.0, 0.0]; positions.len()],
        None => bail!("glTF base color texture requires TEXCOORD_{texcoord_set}"),
    };
    ensure!(
        texcoords.len() == positions.len(),
        "glTF texture coordinate count does not match POSITION"
    );
    let base = u32::try_from(vertices.len()).context("mesh vertex count overflow")?;
    let shading = pbr::import_surface(
        &primitive,
        transform,
        buffers,
        path,
        snapshot,
        images,
        &positions,
        &normals,
        &primitive_indices,
        base,
        warnings,
    )?;
    for ((position, normal), uv) in positions.into_iter().zip(normals).zip(texcoords) {
        ensure!(
            position.is_finite() && normal.is_finite() && uv.into_iter().all(f32::is_finite),
            "glTF contains non-finite geometry"
        );
        let normal = (normal_matrix * normal)
            .try_normalize()
            .context("glTF contains degenerate geometry or normals")?;
        let position = transform.transform_point3(position);
        ensure!(
            position.is_finite(),
            "glTF transformed position is non-finite"
        );
        vertices.push([
            position.x, position.y, position.z, normal.x, normal.y, normal.z, uv[0], uv[1],
        ]);
    }
    let start = u32::try_from(indices.len()).context("mesh index count overflow")?;
    indices.extend(primitive_indices.into_iter().map(|index| base + index));
    let mut color = pbr.base_color_factor();
    let alpha_cutoff = match material.alpha_mode() {
        gltf::material::AlphaMode::Mask => Some(material.alpha_cutoff().unwrap_or(0.5)),
        gltf::material::AlphaMode::Opaque => {
            color[3] = 1.0;
            None
        }
        gltf::material::AlphaMode::Blend => None,
    };
    let image = pbr
        .base_color_texture()
        .map(|texture| {
            images.load(
                texture.texture().source(),
                matches!(material.alpha_mode(), gltf::material::AlphaMode::Opaque),
                buffers,
                path,
                snapshot,
            )
        })
        .transpose()?;
    ensure!(parts.len() < MAX_PARTS, "glTF has too many primitive parts");
    parts.push(MeshPart {
        start,
        count: u32::try_from(indices.len()).context("mesh index count overflow")? - start,
        color,
        image,
        alpha_cutoff,
        shading: Some(shading),
    });
    Ok(())
}

#[derive(Default)]
struct ModelImages {
    images: BTreeMap<(usize, bool), Arc<ImageData>>,
    bytes: usize,
}
impl ModelImages {
    fn load(
        &mut self,
        image: gltf::Image<'_>,
        opaque: bool,
        buffers: &[Vec<u8>],
        path: &Path,
        snapshot: &SourceSnapshot,
    ) -> Result<Arc<ImageData>> {
        let key = (image.index(), opaque);
        if let Some(image) = self.images.get(&key) {
            return Ok(image.clone());
        }
        let mut decoded = image_for(image, buffers, path, snapshot)?;
        if opaque {
            for pixel in decoded.rgba.chunks_exact_mut(4) {
                pixel[3] = 255;
            }
        }
        self.bytes = self
            .bytes
            .checked_add(decoded.rgba.len())
            .context("decoded image byte count overflow")?;
        ensure!(
            self.bytes <= MAX_GLTF_IMAGE_BYTES,
            "unique decoded glTF images exceed 512 MiB"
        );
        let decoded = Arc::new(decoded);
        self.images.insert(key, decoded.clone());
        Ok(decoded)
    }
}

fn image_for(
    image: gltf::Image<'_>,
    buffers: &[Vec<u8>],
    path: &Path,
    snapshot: &SourceSnapshot,
) -> Result<ImageData> {
    let bytes = match image.source() {
        gltf::image::Source::View { view, .. } => {
            let data = buffers
                .get(view.buffer().index())
                .context("image buffer is missing")?;
            let start = view.offset();
            let end = start
                .checked_add(view.length())
                .context("image buffer view overflow")?;
            data.get(start..end)
                .context("image buffer view exceeds buffer")?
                .to_vec()
        }
        gltf::image::Source::Uri { uri, .. } if uri.starts_with("data:") => data_uri(uri)?,
        gltf::image::Source::Uri { uri, .. } => snapshot_resource(path, uri, snapshot)?.to_vec(),
    };
    decoded_image(&bytes, "glTF base color texture")
}

/// Read a glTF/GLB and return an ordinary JSON `.gltf` whose buffers and images are data URIs.
/// Resource URIs are restricted to files below the source directory, just like normal imports.
pub fn portable_gltf(path: &Path) -> Result<Vec<u8>> {
    let snapshot = source_snapshot(path)?;
    let bytes = snapshot
        .primary
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    let gltf = gltf::Gltf::from_slice(bytes).context("parsing validated glTF")?;
    let mut json = gltf_preflight(bytes)?;
    let buffer_values = json
        .get_mut("buffers")
        .and_then(serde_json::Value::as_array_mut)
        .context("glTF has invalid buffers")?;
    for buffer in gltf.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => gltf
                .blob
                .as_deref()
                .context("GLB buffer is missing BIN chunk")?
                .to_vec(),
            gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => continue,
            gltf::buffer::Source::Uri(uri) => snapshot_resource(path, uri, &snapshot)?.to_vec(),
        };
        buffer_values[buffer.index()]["uri"] = serde_json::Value::String(format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(data)
        ));
    }
    if let Some(image_values) = json
        .get_mut("images")
        .and_then(serde_json::Value::as_array_mut)
    {
        for image in gltf.images() {
            let (uri, mime_type) = match image.source() {
                gltf::image::Source::Uri { uri, .. } if uri.starts_with("data:") => continue,
                gltf::image::Source::Uri { uri, mime_type } => {
                    (uri, mime_type.unwrap_or_else(|| mime_for_uri(uri)))
                }
                gltf::image::Source::View { .. } => continue,
            };
            let data = snapshot_resource(path, uri, &snapshot)?;
            image_values[image.index()]["uri"] = serde_json::Value::String(format!(
                "data:{mime_type};base64,{}",
                STANDARD.encode(data)
            ));
        }
    }
    serde_json::to_vec(&json).context("serializing portable glTF")
}

/// Convert a source model into a self-contained JSON glTF when it has material data.
/// Plain OBJ files return `Ok(None)` because their original bytes contain all supported data.
pub fn portable_model(path: &Path) -> Result<Option<Vec<u8>>> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "gltf" | "glb") {
        return portable_gltf(path).map(Some);
    }
    ensure!(extension == "obj", "portable model supports OBJ/glTF/GLB");
    let snapshot = source_snapshot(path)?;
    let bytes = snapshot
        .primary
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    let AssetData::Mesh(mesh) = import(AssetKind::Mesh, path, bytes, &snapshot)? else {
        unreachable!()
    };
    if mesh.parts.is_empty() {
        Ok(None)
    } else {
        portable_mesh_gltf(&mesh).map(Some)
    }
}

fn portable_mesh_gltf(mesh: &MeshData) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    let append = |data: &mut Vec<u8>, bytes: Vec<u8>| {
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
        let offset = data.len();
        data.extend(bytes);
        (offset, data.len() - offset)
    };
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let fallback = MeshPart {
        start: 0,
        count: u32::try_from(mesh.indices.len()).context("mesh index count overflow")?,
        color: [1.0; 4],
        image: None,
        alpha_cutoff: None,
        shading: None,
    };
    let source_parts: Vec<&MeshPart> = if mesh.parts.is_empty() {
        vec![&fallback]
    } else {
        mesh.parts.iter().collect()
    };
    let mut materials = Vec::new();
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let mut primitives = Vec::new();
    for part in source_parts {
        let start = usize::try_from(part.start).context("mesh part offset overflow")?;
        let count = usize::try_from(part.count).context("mesh part count overflow")?;
        let values = mesh
            .indices
            .get(
                start
                    ..start
                        .checked_add(count)
                        .context("mesh part range overflow")?,
            )
            .context("mesh part range exceeds indices")?;
        let mut remap = BTreeMap::new();
        let mut compact = Vec::new();
        let local_indices: Vec<u32> = values
            .iter()
            .map(|index| {
                *remap.entry(*index).or_insert_with(|| {
                    let next = compact.len() as u32;
                    compact.push(mesh.vertices[*index as usize]);
                    next
                })
            })
            .collect();
        let mut attributes = serde_json::Map::new();
        for (name, range, kind) in [
            ("POSITION", 0..3, "VEC3"),
            ("NORMAL", 3..6, "VEC3"),
            ("TEXCOORD_0", 6..8, "VEC2"),
        ] {
            let bytes = compact
                .iter()
                .flat_map(|v| v[range.clone()].iter().flat_map(|x| x.to_le_bytes()))
                .collect();
            let (offset, length) = append(&mut data, bytes);
            let view = views.len();
            views.push(serde_json::json!({"buffer":0,"byteOffset":offset,"byteLength":length}));
            let mut accessor = serde_json::json!({"bufferView":view,"componentType":5126,"count":compact.len(),"type":kind});
            if name == "POSITION" {
                let mut min = [f32::INFINITY; 3];
                let mut max = [f32::NEG_INFINITY; 3];
                for v in &compact {
                    for axis in 0..3 {
                        min[axis] = min[axis].min(v[axis]);
                        max[axis] = max[axis].max(v[axis]);
                    }
                }
                accessor["min"] = serde_json::json!(min);
                accessor["max"] = serde_json::json!(max);
            }
            attributes.insert(name.into(), serde_json::json!(accessors.len()));
            accessors.push(accessor);
        }
        let index_bytes: Vec<u8> = local_indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        let (offset, length) = append(&mut data, index_bytes);
        let view = views.len();
        views.push(
            serde_json::json!({"buffer":0,"byteOffset":offset,"byteLength":length,"target":34963}),
        );
        let accessor = accessors.len();
        accessors.push(serde_json::json!({"bufferView":view,"componentType":5125,"count":count,"type":"SCALAR"}));
        let texture_index = if let Some(image) = &part.image {
            let rgba = image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
                .context("invalid decoded image dimensions")?;
            let mut png = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(rgba)
                .write_to(&mut png, image::ImageFormat::Png)
                .context("encoding portable PNG")?;
            let image_index = images.len();
            images.push(serde_json::json!({"uri":format!("data:image/png;base64,{}", STANDARD.encode(png.into_inner()))}));
            let texture_index = textures.len();
            textures.push(serde_json::json!({"source":image_index}));
            Some(texture_index)
        } else {
            None
        };
        let mut pbr = serde_json::json!({"baseColorFactor":part.color});
        if let Some(index) = texture_index {
            pbr["baseColorTexture"] = serde_json::json!({"index":index});
        }
        let has_alpha = part.color[3] < 1.0
            || part
                .image
                .as_ref()
                .is_some_and(|image| image.rgba.chunks_exact(4).any(|pixel| pixel[3] != 255));
        let mut material = serde_json::json!({"pbrMetallicRoughness":pbr});
        if let Some(cutoff) = part.alpha_cutoff {
            material["alphaMode"] = serde_json::json!("MASK");
            material["alphaCutoff"] = serde_json::json!(cutoff);
        } else if has_alpha {
            material["alphaMode"] = serde_json::json!("BLEND");
        }
        let material_index = materials.len();
        materials.push(material);
        primitives.push(serde_json::json!({"attributes":attributes,"indices":accessor,"material":material_index,"mode":4}));
    }
    serde_json::to_vec(&serde_json::json!({"asset":{"version":"2.0","generator":"bozzard-assets"},"buffers":[{"byteLength":data.len(),"uri":format!("data:application/octet-stream;base64,{}", STANDARD.encode(data))}],"bufferViews":views,"accessors":accessors,"images":images,"textures":textures,"materials":materials,"meshes":[{"primitives":primitives}],"nodes":[{"mesh":0}],"scenes":[{"nodes":[0]}],"scene":0})).context("serializing portable OBJ glTF")
}

fn mime_for_uri(uri: &str) -> &'static str {
    match uri
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PNG: &[u8] = include_bytes!("../../../examples/demo/scenes/assets/palette.png");
    const NEXT_PNG: &[u8] =
        include_bytes!("../../../examples/demo/scenes/assets/palette-reloaded.png");
    const OBJ: &[u8] = include_bytes!("../../../examples/demo/scenes/assets/quad.obj");
    fn no_dependencies() -> SourceSnapshot {
        SourceSnapshot {
            primary: Ok(Vec::new()),
            dependencies: Vec::new(),
        }
    }
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
    fn background_refresh_keeps_live_data_until_published_and_recovers() {
        use std::time::{Duration, Instant};
        let wait = |job: job::Job<(AssetStore, Vec<Handle>)>| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(result) = job.poll() {
                    break result.unwrap();
                }
                assert!(Instant::now() < deadline, "refresh timed out");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        let dir = Temp::new();
        let sources = BTreeMap::from([(
            "palette".into(),
            AssetSource {
                kind: AssetKind::Image,
                path: "palette.png".into(),
            },
        )]);
        std::fs::write(dir.0.join("palette.png"), PNG).unwrap();
        let mut store = AssetStore::new(&dir.0, &sources).unwrap();
        store.refresh();
        let handle = store.handle("palette").unwrap();
        let original = store.get(handle).unwrap().data().unwrap() as *const AssetData;
        let (unchanged, changes) = wait(store.refresh_job().unwrap());
        assert!(changes.is_empty());
        assert_eq!(
            unchanged.get(handle).unwrap().data().unwrap() as *const AssetData,
            original
        );
        std::fs::write(dir.0.join("palette.png"), b"broken").unwrap();
        let (failed, changes) = wait(store.refresh_job().unwrap());
        assert_eq!(changes, vec![handle]);
        assert_eq!(store.get(handle).unwrap().state(), &LoadState::Ready);
        assert!(matches!(
            failed.get(handle).unwrap().state(),
            LoadState::Failed(_)
        ));
        assert_eq!(
            failed.get(handle).unwrap().data().unwrap() as *const AssetData,
            original
        );
        std::fs::write(dir.0.join("palette.png"), NEXT_PNG).unwrap();
        let (recovered, changes) = wait(failed.refresh_job().unwrap());
        assert_eq!(changes, vec![handle]);
        assert_eq!(recovered.get(handle).unwrap().revision(), 2);
        assert_eq!(store.get(handle).unwrap().revision(), 1);
        recovered.require_ready().unwrap();
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
        let AssetData::Mesh(mesh) = import(
            AssetKind::Mesh,
            Path::new("quad.obj"),
            OBJ,
            &no_dependencies(),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(mesh.indices.len(), 6);
        assert_eq!(mesh.vertices[0], [-0.5, -0.5, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0]);
        for invalid in [
            b"f 1 2 3".as_slice(),
            b"v 0 0 0\nf 1 1 1",
            b"mtllib ignored.mtl\nv 0 0 0\nf 1 1 1",
        ] {
            assert!(
                import(
                    AssetKind::Mesh,
                    Path::new("bad.obj"),
                    invalid,
                    &no_dependencies()
                )
                .is_err()
            );
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
        let AssetData::Image(decoded) = import(
            AssetKind::Image,
            Path::new("photo.jpg"),
            &jpeg,
            &no_dependencies(),
        )
        .unwrap() else {
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
        let AssetData::Image(decoded) = import(
            AssetKind::Image,
            Path::new("alpha.png"),
            &alpha,
            &no_dependencies(),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(decoded.rgba[3], 128);
        let wide = encode(
            image::RgbImage::new(4097, 1).into(),
            image::ImageFormat::Png,
        );
        assert!(
            import(
                AssetKind::Image,
                Path::new("wide.png"),
                &wide,
                &no_dependencies()
            )
            .is_err()
        );
    }

    fn triangle_bytes() -> Vec<u8> {
        [[0.0_f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
            .into_iter()
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect()
    }

    fn gltf_document(buffer_uri: &str, image_uri: Option<&str>) -> Vec<u8> {
        let mut pbr = serde_json::json!({ "baseColorFactor": [0.25, 0.5, 0.75, 0.5] });
        if image_uri.is_some() {
            pbr["baseColorTexture"] = serde_json::json!({ "index": 0 });
        }
        let mut document = serde_json::json!({
            "asset": { "version": "2.0" },
            "buffers": [{ "byteLength": 36, "uri": buffer_uri }],
            "bufferViews": [{ "buffer": 0, "byteLength": 36 }],
            "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }],
            "materials": [{ "pbrMetallicRoughness": pbr, "alphaMode": "MASK", "alphaCutoff": 0.3 }],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "material": 0 }] }],
            "nodes": [{ "mesh": 0, "translation": [2.0, 0.0, 0.0], "scale": [-1.0, 1.0, 1.0] }],
            "scenes": [{ "nodes": [0] }], "scene": 0
        });
        if let Some(uri) = image_uri {
            document["bufferViews"][0]["byteStride"] = serde_json::json!(12);
            document["accessors"].as_array_mut().unwrap().push(
                serde_json::json!({"bufferView":0,"componentType":5126,"count":3,"type":"VEC2"}),
            );
            document["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] =
                serde_json::json!(1);
            document["images"] = serde_json::json!([{ "uri": uri }]);
            document["textures"] = serde_json::json!([{ "source": 0 }]);
        }
        serde_json::to_vec(&document).unwrap()
    }

    #[test]
    fn gltf_data_uri_bakes_transforms_materials_textures_and_winding() {
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([4, 5, 6, 128]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
        let buffer_uri = format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(triangle_bytes())
        );
        let image_uri = format!(
            "data:image/png;base64,{}",
            STANDARD.encode(encoded.into_inner())
        );
        let bytes = gltf_document(&buffer_uri, Some(&image_uri));
        let AssetData::Mesh(mesh) = import(
            AssetKind::Mesh,
            Path::new("model.gltf"),
            &bytes,
            &no_dependencies(),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(mesh.vertices[0][0], 2.0);
        assert_eq!(mesh.indices, vec![0, 2, 1]);
        assert_eq!(
            mesh.vertices[0][5], 1.0,
            "mirrored winding and generated normals agree"
        );
        assert_eq!(mesh.parts[0].color, [0.25, 0.5, 0.75, 0.5]);
        assert_eq!(mesh.parts[0].alpha_cutoff, Some(0.3));
        assert_eq!(
            mesh.parts[0].image.as_ref().unwrap().rgba,
            vec![4, 5, 6, 128]
        );
    }

    #[test]
    fn gltf_external_dependencies_reload_and_portabilize() {
        let dir = Temp::new();
        std::fs::write(dir.0.join("model.gltf"), gltf_document("mesh.bin", None)).unwrap();
        std::fs::write(dir.0.join("mesh.bin"), triangle_bytes()).unwrap();
        let sources = BTreeMap::from([(
            "model".into(),
            AssetSource {
                kind: AssetKind::Mesh,
                path: "model.gltf".into(),
            },
        )]);
        let mut store = AssetStore::new(&dir.0, &sources).unwrap();
        let handle = store.handle("model").unwrap();
        assert_eq!(store.refresh(), vec![handle]);
        store.require_ready().unwrap();
        assert_eq!(store.get(handle).unwrap().revision(), 1);
        std::fs::write(dir.0.join("mesh.bin"), b"short").unwrap();
        assert_eq!(store.refresh(), vec![handle]);
        assert!(matches!(
            store.get(handle).unwrap().state(),
            LoadState::Failed(_)
        ));
        assert_eq!(store.get(handle).unwrap().revision(), 1);
        std::fs::write(dir.0.join("mesh.bin"), triangle_bytes()).unwrap();
        assert_eq!(store.refresh(), vec![handle]);
        assert_eq!(store.get(handle).unwrap().revision(), 2);
        let portable = portable_gltf(&dir.0.join("model.gltf")).unwrap();
        assert!(
            std::str::from_utf8(&portable)
                .unwrap()
                .contains("data:application/octet-stream;base64,")
        );
        assert!(
            import(
                AssetKind::Mesh,
                Path::new("portable.gltf"),
                &portable,
                &no_dependencies()
            )
            .is_ok()
        );
    }
    #[test]
    fn gltf_surfaces_share_decoded_images_without_merging_alpha_variants() {
        let buffer = format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(triangle_bytes())
        );
        let texture = format!("data:image/png;base64,{}", STANDARD.encode(PNG));
        let mut json: serde_json::Value =
            serde_json::from_slice(&gltf_document(&buffer, Some(&texture))).unwrap();
        let primitive = json["meshes"][0]["primitives"][0].clone();
        json["meshes"][0]["primitives"]
            .as_array_mut()
            .unwrap()
            .push(primitive.clone());
        let mut opaque = json["materials"][0].clone();
        opaque["alphaMode"] = serde_json::json!("OPAQUE");
        json["materials"][0]["alphaMode"] = serde_json::json!("BLEND");
        json["materials"].as_array_mut().unwrap().push(opaque);
        let mut third = primitive;
        third["material"] = serde_json::json!(1);
        json["meshes"][0]["primitives"]
            .as_array_mut()
            .unwrap()
            .push(third);
        let bytes = serde_json::to_vec(&json).unwrap();
        let AssetData::Mesh(mesh) = import(
            AssetKind::Mesh,
            Path::new("shared.gltf"),
            &bytes,
            &no_dependencies(),
        )
        .unwrap() else {
            panic!()
        };
        assert!(Arc::ptr_eq(
            mesh.parts[0].image.as_ref().unwrap(),
            mesh.parts[1].image.as_ref().unwrap()
        ));
        assert!(!Arc::ptr_eq(
            mesh.parts[0].image.as_ref().unwrap(),
            mesh.parts[2].image.as_ref().unwrap()
        ));
    }

    #[test]
    fn gltf_pbr_maps_share_images_preserve_uvs_samplers_and_tangents() {
        let mut buffer = triangle_bytes();
        buffer.extend(
            [[0.0_f32, 0.0], [0.0, 1.0], [1.0, 0.0]]
                .into_iter()
                .flatten()
                .flat_map(f32::to_le_bytes),
        );
        let uri = format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(&buffer)
        );
        let texture = format!("data:image/png;base64,{}", STANDARD.encode(PNG));
        let mut json: serde_json::Value =
            serde_json::from_slice(&gltf_document(&uri, Some(&texture))).unwrap();
        json["buffers"][0]["byteLength"] = serde_json::json!(buffer.len());
        json["bufferViews"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"buffer":0,"byteOffset":36,"byteLength":24}));
        json["accessors"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}));
        json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_1"] = serde_json::json!(2);
        json["samplers"] = serde_json::json!([
            {"wrapS":33071,"wrapT":33648,"magFilter":9728,"minFilter":9729},
            {"minFilter":9986}
        ]);
        json["textures"] = serde_json::json!([{"source":0,"sampler":0},{"source":0,"sampler":1}]);
        let material = &mut json["materials"][0];
        material["pbrMetallicRoughness"]["metallicRoughnessTexture"] =
            serde_json::json!({"index":1});
        material["pbrMetallicRoughness"]["metallicFactor"] = serde_json::json!(0.3);
        material["pbrMetallicRoughness"]["roughnessFactor"] = serde_json::json!(0.7);
        material["normalTexture"] = serde_json::json!({"index":0,"texCoord":1,"scale":0.6});
        material["occlusionTexture"] = serde_json::json!({"index":1,"strength":0.4});
        material["emissiveTexture"] = serde_json::json!({"index":0});
        material["emissiveFactor"] = serde_json::json!([0.1, 0.2, 0.3]);
        material["doubleSided"] = serde_json::json!(true);
        let load = |json: &serde_json::Value| {
            import(
                AssetKind::Mesh,
                Path::new("pbr.gltf"),
                &serde_json::to_vec(json).unwrap(),
                &no_dependencies(),
            )
        };
        let AssetData::Mesh(mesh) = load(&json).unwrap() else {
            panic!()
        };
        let part = &mesh.parts[0];
        let shading = part.shading.as_ref().unwrap();
        let m = &shading.material;
        assert_eq!(
            (
                m.metallic,
                m.roughness,
                m.normal_scale,
                m.occlusion_strength
            ),
            (0.3, 0.7, 0.6, 0.4)
        );
        assert_eq!(m.emissive_factor, [0.1, 0.2, 0.3]);
        assert!(m.double_sided);
        for map in [&m.normal, &m.metallic_roughness, &m.occlusion, &m.emissive]
            .into_iter()
            .flatten()
        {
            assert!(Arc::ptr_eq(part.image.as_ref().unwrap(), &map.image));
        }
        assert_eq!(
            m.base_color_sampler,
            Sampler {
                wrap_u: Wrap::Clamp,
                wrap_v: Wrap::Mirror,
                mag: Filter::Nearest,
                min: Filter::Linear,
                mip: None
            }
        );
        assert_eq!(
            m.metallic_roughness.as_ref().unwrap().sampler.min,
            Filter::Nearest
        );
        assert_eq!(
            m.metallic_roughness.as_ref().unwrap().sampler.mip,
            Some(Filter::Linear)
        );
        assert_eq!(&shading.vertices[1][4..6], &[0., 1.]);
        assert_eq!(&shading.vertices[1][6..8], &[1., 0.]);
        // Normal UV1 swaps UV axes; mirrored node flips tangent handedness back.
        assert_eq!(&shading.vertices[0][..4], &[0., 1., 0., 1.]);
        let mut missing_uv = json.clone();
        missing_uv["materials"][0]["emissiveTexture"]["texCoord"] = serde_json::json!(3);
        assert!(
            load(&missing_uv)
                .unwrap_err()
                .to_string()
                .contains("TEXCOORD_3")
        );

        let tangent_offset = buffer.len();
        buffer.extend(
            [[1.0_f32, 0.0, 0.0, 1.0]; 3]
                .into_iter()
                .flatten()
                .flat_map(f32::to_le_bytes),
        );
        json["buffers"][0]["byteLength"] = serde_json::json!(buffer.len());
        json["buffers"][0]["uri"] = serde_json::json!(format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(&buffer)
        ));
        json["bufferViews"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"buffer":0,"byteOffset":tangent_offset,"byteLength":48}));
        json["accessors"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"bufferView":2,"componentType":5126,"count":3,"type":"VEC4"}));
        json["meshes"][0]["primitives"][0]["attributes"]["TANGENT"] = serde_json::json!(3);
        let AssetData::Mesh(mesh) = load(&json).unwrap() else {
            panic!()
        };
        assert_eq!(
            &mesh.parts[0].shading.as_ref().unwrap().vertices[0][..4],
            &[-1., 0., 0., -1.]
        );
        for i in 0..3 {
            let offset = tangent_offset + i * 16;
            buffer[offset..offset + 16].copy_from_slice(
                &[0.0_f32, 0., 1., 1.]
                    .into_iter()
                    .flat_map(f32::to_le_bytes)
                    .collect::<Vec<_>>(),
            );
        }
        json["buffers"][0]["uri"] = serde_json::json!(format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(&buffer)
        ));
        let AssetData::Mesh(mesh) = load(&json).unwrap() else {
            panic!()
        };
        assert_eq!(
            &mesh.parts[0].shading.as_ref().unwrap().vertices[0][..4],
            &[0., 1., 0., 1.]
        );
        assert!(
            mesh.warnings
                .iter()
                .any(|w| w.contains("Repaired 3 degenerate"))
        );
    }

    #[test]
    fn gltf_uses_requested_uv_set_and_accepts_small_nonzero_scale() {
        let buffer = format!(
            "data:application/octet-stream;base64,{}",
            STANDARD.encode(triangle_bytes())
        );
        let texture = format!("data:image/png;base64,{}", STANDARD.encode(PNG));
        let mut json: serde_json::Value =
            serde_json::from_slice(&gltf_document(&buffer, Some(&texture))).unwrap();
        json["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["texCoord"] =
            serde_json::json!(1);
        json["meshes"][0]["primitives"][0]["attributes"]
            .as_object_mut()
            .unwrap()
            .remove("TEXCOORD_0");
        json["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_1"] = serde_json::json!(1);
        json["nodes"][0]["scale"] = serde_json::json!([0.001, 0.001, 0.001]);
        let bytes = serde_json::to_vec(&json).unwrap();
        let AssetData::Mesh(mesh) = import(
            AssetKind::Mesh,
            Path::new("small.gltf"),
            &bytes,
            &no_dependencies(),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(&mesh.vertices[1][6..], &[1.0, 0.0]);
        let mut cyclic = json.clone();
        cyclic["nodes"][0]["children"] = serde_json::json!([0]);
        assert!(
            import(
                AssetKind::Mesh,
                Path::new("cycle.gltf"),
                &serde_json::to_vec(&cyclic).unwrap(),
                &no_dependencies()
            )
            .is_err()
        );

        for (key, value) in [
            ("animations", serde_json::json!([{}])),
            ("skins", serde_json::json!([{}])),
            (
                "extensionsRequired",
                serde_json::json!(["KHR_texture_transform"]),
            ),
        ] {
            let mut invalid = json.clone();
            invalid[key] = value;
            assert!(gltf_preflight(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
    }
    #[test]
    fn model_texture_reload_preserves_last_good_and_recovers() {
        let dir = Temp::new();
        std::fs::write(
            dir.0.join("model.gltf"),
            gltf_document("mesh.bin", Some("paint.png")),
        )
        .unwrap();
        std::fs::write(dir.0.join("mesh.bin"), triangle_bytes()).unwrap();
        std::fs::write(dir.0.join("paint.png"), PNG).unwrap();
        let sources = BTreeMap::from([(
            "model".into(),
            AssetSource {
                kind: AssetKind::Mesh,
                path: "model.gltf".into(),
            },
        )]);
        let mut store = AssetStore::new(&dir.0, &sources).unwrap();
        store.refresh();
        store.require_ready().unwrap();
        let handle = store.handle("model").unwrap();
        std::fs::write(dir.0.join("paint.png"), b"broken").unwrap();
        assert_eq!(store.refresh(), vec![handle]);
        assert_eq!(store.get(handle).unwrap().revision(), 1);
        assert!(store.get(handle).unwrap().data().is_some());
        assert!(store.require_ready().is_err());
        std::fs::write(dir.0.join("paint.png"), NEXT_PNG).unwrap();
        assert_eq!(store.refresh(), vec![handle]);
        store.require_ready().unwrap();
        assert_eq!(store.get(handle).unwrap().revision(), 2);
    }
}
