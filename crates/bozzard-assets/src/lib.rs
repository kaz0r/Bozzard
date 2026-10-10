//! CPU imports and background loading. No GPU or window dependencies.
pub mod animation;
pub mod audio;
mod collision;
mod cook_source;
pub use cook_source::CookSource;
pub mod cooked_model;
mod fonts;
pub mod gi;
pub mod job;
mod mesh_export;
mod optimize;
mod package;
mod pbr;
mod picking;
mod simplify;
pub mod texture;
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bozzard_scene::{AssetKind, AssetSource};
use glam::{Mat3, Mat4, Vec3};
pub use mesh_export::mesh_gltf;
pub use package::{ModelPackage, SourcePackage, package_gltf, package_model};
pub use pbr::{Filter, PbrMaterial, Sampler, SurfaceShading, TextureMap, Wrap};
pub mod materials;
pub use picking::{MeshHit, MeshPickStats};
pub use simplify::{Simplification, SimplifySettings, simplify_mesh};
pub mod blockout;
mod gltf_import;
mod importer;
mod portable;
mod source;
mod store;
pub mod terrain;
#[cfg(test)]
mod tests;
use gltf_import::*;
use importer::*;
use portable::mime_for_uri;
pub use portable::{portable_gltf, portable_model};
use source::*;
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

/// How thoroughly a refresh looks for changed sources.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RefreshScan {
    /// Read only sources whose files changed size, timestamps or identity (device, inode
    /// and status-change time on Unix), whose last read failed, or whose last read came
    /// too soon after a change for the timestamps to vouch for it. Metadata can still miss
    /// an edit: a server or clock that stamps files more than two seconds in the past, a
    /// network mount that caches attributes, or on Windows a replacement that keeps the
    /// size and modification time. Hosts therefore also `Verify` every
    /// [`RefreshScan::VERIFY_INTERVAL`].
    Changed,
    /// Read every source and compare it with what was decoded.
    #[default]
    Verify,
    /// Verify, and probe audio files again even when their size and timestamp match.
    Reload,
}
impl RefreshScan {
    /// How often hosts that scan with `Changed` read every source anyway.
    pub const VERIFY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
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
    /// Offline block-compressed mip chains; original pixels remain available to CPU tools.
    pub compressed: Option<Arc<texture::CookedTexture>>,
}

#[derive(Clone, Debug)]
pub struct MeshData {
    pub skin: Option<animation::Skin>,
    /// Position, normal, UV. Right handed, Y up; UV origin at the top left.
    pub vertices: Vec<[f32; 8]>,
    pub indices: Vec<u32>,
    /// glTF primitive / OBJ material slices. Empty for an unpartitioned OBJ.
    pub parts: Vec<MeshPart>,
    /// Material features present in the source but outside the current renderer.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct MeshPart {
    /// Persistent source/geometry signature used to guard scene overrides.
    pub source_key: String,
    /// Source node/mesh/primitive label, for editor inspection only.
    pub name: String,
    pub material_name: Option<String>,
    pub start: u32,
    pub count: u32,
    /// glTF baseColorFactor, including alpha.
    pub color: [f32; 4],
    pub image: Option<Arc<ImageData>>,
    /// Present for glTF `alphaMode: MASK`.
    pub alpha_cutoff: Option<f32>,
    pub shading: Option<SurfaceShading>,
}

impl MeshData {
    /// Immutable GPU sibling preserving indexed attribute bits and primitive order.
    /// Authoring, source signatures and picking retain the original CPU geometry.
    pub fn optimized_for_upload(&self, progress: &job::Progress) -> Result<Self> {
        simplify::validate_geometry(self)?;
        if let Some(skin) = &self.skin {
            anyhow::ensure!(
                skin.vertices.len() == self.vertices.len()
                    && !skin.rig.bindings.is_empty()
                    && skin.rig.bindings.len() <= 4096,
                "invalid uploaded skin size"
            );
            for (index, influence) in skin.vertices.iter().enumerate() {
                if index.is_multiple_of(4096) {
                    progress.check()?;
                }
                let mut sum = 0.;
                for axis in 0..4 {
                    let weight = f32::from_bits(influence[axis + 4]);
                    anyhow::ensure!(
                        (influence[axis] as usize) < skin.rig.bindings.len()
                            && weight.is_finite()
                            && weight >= 0.,
                        "invalid uploaded skin influence"
                    );
                    sum += weight;
                }
                anyhow::ensure!(
                    (sum - 1.).abs() < 0.001,
                    "uploaded skin weights must sum to one"
                );
            }
        }
        optimize::mesh(self, progress)
    }
    pub fn part_bounds(&self, index: usize) -> Option<[Vec3; 2]> {
        let part = self.parts.get(index)?;
        let indices = self
            .indices
            .get(part.start as usize..(part.start + part.count) as usize)?;
        if indices.is_empty() {
            return None;
        }
        Some(indices.iter().fold(
            [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)],
            |[min, max], i| {
                let p = Vec3::from_slice(&self.vertices[*i as usize][..3]);
                [min.min(p), max.max(p)]
            },
        ))
    }
    fn with_surface_keys(mut self) -> Self {
        for part in &mut self.parts {
            // Deterministic FNV-1a over source labels and indexed geometry. Deliberately
            // excludes image pixels and material factors so those can be reloaded.
            let mut hash = 0xcbf29ce484222325u64;
            let mut feed = |b: u8| {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
            };
            for b in part.source_key.bytes().chain([0]) {
                feed(b);
            }
            for b in part
                .name
                .bytes()
                .chain([0])
                .chain(part.material_name.as_deref().unwrap_or("").bytes())
                .chain([0])
            {
                feed(b);
            }
            for index in &self.indices[part.start as usize..(part.start + part.count) as usize] {
                for value in self.vertices[*index as usize] {
                    for byte in value.to_le_bytes() {
                        feed(byte);
                    }
                }
            }
            part.source_key = format!("{hash:016x}");
        }
        self
    }
}

// Names are copied per surface, so cap display metadata independently of source size.
// Source files remain unchanged; controls/newlines cannot distort virtual list rows.
fn inspection_name(name: &str) -> String {
    let mut chars = name.chars();
    let mut label: String = chars
        .by_ref()
        .take(128)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if chars.next().is_some() {
        label.push('…');
    }
    label
}

#[derive(Clone, Debug)]
pub enum AssetData {
    Material(Box<materials::MaterialData>),
    Font(bozzard_text::Font),
    ComputeShader(Arc<bozzard_compute::Kernel>),
    Audio(audio::AudioData),
    Prefab(bozzard_scene::Prefab),
    Image(ImageData),
    Mesh(MeshData),
    /// Rhai source, read as text. The scene runtime compiles it; nothing here interprets it.
    Script(String),
}

#[derive(Clone)]
pub struct Entry {
    audio_stamp: Option<audio::Stamp>,
    pub id: String,
    source: AssetSource,
    state: LoadState,
    data: Option<Arc<AssetData>>,
    mesh_index: Option<Arc<picking::MeshIndex>>,
    revision: u64,
    content_fingerprint: Option<u64>,
    // Compare content digests, so same-size edits and coarse timestamps cannot hide changes.
    observed: Option<Arc<ObservedSource>>,
    /// Metadata that lets a scan for changes skip reading an untouched source again.
    stamps: Option<Arc<SourceStamps>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceSnapshot {
    primary: Result<Vec<u8>, String>,
    dependencies: Vec<SourceDependency>,
}

#[derive(Clone)]
pub struct AssetStore {
    id: u64,
    root: PathBuf,
    entries: Vec<Entry>,
    handles: BTreeMap<String, Handle>,
    publication: Arc<()>,
    canonical_ids: Arc<std::sync::OnceLock<std::collections::HashMap<usize, String>>>,
}
