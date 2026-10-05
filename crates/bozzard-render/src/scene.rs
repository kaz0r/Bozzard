use crate::Gpu;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;
mod temporal_settings;
pub use temporal_settings::{MotionBlur, ScreenSpaceReflections, TemporalAntiAliasing};
pub(crate) mod geometry;
mod particles;
mod skinning;
pub use skinning::{SkinData, SkinPose};
mod reflections;
mod temporal;
pub use particles::{Particle, ParticleKind, ParticleSimulation};
mod hud;
mod sprites;
pub use sprites::{SpriteGeometry, SpriteMesh, SpriteQuad};
mod text;
pub use text::{ScreenText, TextAlignment, TextMesh, text_bounds};
pub use variants::ShaderWarmup;
mod host;
pub(crate) use host::host_text;
mod draw_state;
mod frame_scratch;
mod instancing;
mod object_cache;
mod preparation;
mod submission;
mod variants;
mod visibility;
pub use instancing::BatchingStats;
pub use visibility::{BatchPlanRebuildReason, FrameStats};
mod occlusion;
pub use occlusion::OcclusionResult;
mod fog;
pub use fog::FogSettings;
mod environment;
pub use environment::EnvironmentSettings;
mod bloom;
pub use bloom::BloomSettings;
mod optics_settings;
pub use optics_settings::{AutoExposure, DepthOfField};
mod auto_exposure;
mod depth_of_field;
mod display;
mod display_settings;
mod post_process;
mod volumetric;
mod volumetric_settings;
pub use display_settings::{
    AmbientOcclusion, ColorGrading, DisplaySettings, FilmGrain, HeatDistortion, ToneMapper,
    Vignette,
};
pub use volumetric_settings::VolumetricFog;
mod gi;
pub use gi::IrradianceVolume;
mod lighting;
mod local_lights;
pub use local_lights::{
    LocalLight, LocalShadowSettings, MAX_LOCAL_LIGHTS, MAX_SHADOWED_POINT_LIGHTS,
    MAX_SHADOWED_SPOT_LIGHTS, SpotShadowSettings,
};
mod local_shadow_maps;
mod point_shadows;
mod shadows;
mod spot_shadows;
mod sun_cache;
mod sun_fit;
mod upload;
pub use lighting::Lighting;
pub use upload::{
    BlockCompression, CompressedImage, PendingUpload, UploadContext, UploadData, UploadProgress,
    UploadSource, upload_memory_bytes,
};
type ImageCache = BTreeMap<(usize, u32, u32, bool), (wgpu::TextureView, bool)>;
const OBJECT_UNIFORM_BYTES: usize = 256;
const FRAME_UNIFORM_BYTES: usize = 320;
const GRAPH_PARAMETER_SLOTS: usize = 16;
const GRAPH_PARAMETER_RECORD_BYTES: usize = GRAPH_PARAMETER_SLOTS * 16;
const GRAPH_PARAMETER_BUFFER_BYTES: usize = GRAPH_PARAMETER_RECORD_BYTES * 64;

#[derive(Clone, Debug, PartialEq)]
pub enum MeshKind {
    Sprite(SpriteMesh),
    Text(TextMesh),
    SharedText(std::sync::Arc<TextMesh>),
    Quad,
    Cube,
    Sphere,
    Imported(String),
    ModelPart(String, usize),
}
impl MeshKind {
    pub(crate) fn text(&self) -> Option<&TextMesh> {
        match self {
            Self::Text(text) => Some(text),
            Self::SharedText(text) => Some(text),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextureKind {
    /// Linear color with straight alpha; identity includes the runtime world generation.
    Generated(bozzard_compute::Handle),
    Text,
    White,
    Checker,
    Normals,
    ProceduralChecker,
    Toon,
    Imported(String),
    ModelPart(String, usize),
}

#[derive(Clone, Debug)]
pub struct Material {
    pub metallic: Option<f32>,
    pub roughness: Option<f32>,
    pub surface_overrides: std::sync::Arc<[SurfaceMaterialOverride]>,
    pub tint: [f32; 3],
    pub uv_scale: [f32; 2],
    pub texture: TextureKind,
    pub lit: bool,
    /// Compiled shader graph surface override; pipelines are cached by content hash.
    pub shader: Option<std::sync::Arc<ShaderSource>>,
}

/// A shader graph compiled to its `graph_material_surface` WGSL function.
#[derive(Clone, Debug)]
pub struct ShaderSource {
    /// Content hash of `surface`; keys the renderer's pipeline cache.
    pub id: u64,
    /// Legacy literal-program hash preserves opaque equal-depth draw ordering
    /// while `id` can share a pipeline across numeric parameter values.
    pub opaque_sort_id: u64,
    pub surface: String,
    /// Numeric graph inputs do not change topology or pipeline identity.
    pub numeric_parameters: std::sync::Arc<[[f32; 4]]>,
}

#[derive(Clone, Debug)]
pub struct SurfaceMaterialOverride {
    pub surface: u32,
    pub source: String,
    pub transform: Mat4,
    pub texture: Option<TextureKind>,
    pub uv_scale: [f32; 2],
    pub tint: [f32; 3],
    pub metallic: Option<f32>,
    pub roughness: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct DrawItem {
    /// Stable runtime identity. Zero disables object motion tracking.
    pub motion_id: u64,
    pub model: Mat4,
    pub mesh: MeshKind,
    pub material: Material,
}

/// Render data only: does not borrow an ECS world or know about scene serialization.
#[derive(Clone, Debug)]
pub struct RenderScene {
    pub skin_poses: BTreeMap<u64, SkinPose>,
    pub particles: Vec<Particle>,
    pub fog: FogSettings,
    pub gi: Option<IrradianceVolume>,
    pub lights: Vec<LocalLight>,
    pub environment: EnvironmentSettings,
    pub display: DisplaySettings,
    pub lighting: Lighting,
    pub view_projection: Mat4,
    pub items: Vec<DrawItem>,
    /// Clock for shader graph Time nodes. Simulation time: advances only while
    /// the simulation runs, so editing never animates materials. Producers
    /// that preview effects keep `display.time_seconds` for particles and
    /// atmosphere independent of this.
    pub shader_time: f32,
}

/// CPU-side material surface uploaded as part of a static model.
pub struct ModelPart<'a> {
    pub source_key: &'a str,
    pub start: u32,
    pub count: u32,
    pub color: [f32; 4],
    pub alpha_cutoff: Option<f32>,
    pub image: Option<ModelImage<'a>>,
    pub shading: Option<crate::ModelShading<'a>>,
}
#[derive(Clone)]
pub struct ModelImage<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}
#[derive(Clone, Copy, Debug)]
pub struct ModelUploadStats {
    pub surfaces: usize,
    pub unique_images: usize,
    pub texture_bytes: usize,
    pub cpu_upload_ms: f64,
    pub prepare_ms: f64,
    pub upload_slices: usize,
    pub max_slice_bytes: usize,
    pub max_slice_cpu_ms: f64,
}
struct UploadedPart {
    source_key: String,
    mesh: MeshBuffers,
    texture: Option<wgpu::TextureView>,
    color: [f32; 4],
    cutoff: Option<f32>,
    translucent: bool,
    center: Vec3,
    sampler: wgpu::Sampler,
    shading: Option<crate::pbr::UploadedShading>,
}
struct PreparedDraw {
    preparation: preparation::DrawPreparation,
    source_item: usize,
    deformation: u64,
    shared_geometry: Option<instancing::GeometryKey>,
    world_geometry_units: Option<usize>,
    pbr_override: [f32; 2],
    shader: Option<u64>,
    pbr: bool,
    raster: u8,
    object: DrawItem,
    opacity: f32,
    cutoff: f32,
    transparent: bool,
    depth: f32,
}
struct DrawCall<'a> {
    instances: u32,
    first_instance: u32,
    indirect: Option<(&'a wgpu::Buffer, u64)>,
}

/// One shader graph's two host flavors, opaque and transparent each.
#[derive(Clone)]
struct GraphPipelines {
    basic: [Option<wgpu::RenderPipeline>; 2],
    pbr: [Option<wgpu::RenderPipeline>; 2],
    instanced: [Option<wgpu::RenderPipeline>; 2],
}
struct MeshBuffers {
    bounds: [Vec3; 2],
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    vertex_offset: u64,
}
struct ObjectBinding {
    resources: Option<ObjectResources>,
    texture: TextureKind,
    uniform: Option<[u8; OBJECT_UNIFORM_BYTES]>,
    uniform_revision: u64,
    dirty: bool,
    source: Option<ObjectUniformSource>,
    light_revision: u64,
    light_bounds: Option<[Vec3; 2]>,
    transform: Option<(Mat4, Mat4, f32)>,
    numeric_parameters: std::sync::Arc<[[f32; 4]]>,
    parameter_revision: u64,
    parameter_dirty: bool,
}
struct ObjectResources {
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    parameters: Option<wgpu::Buffer>,
}
impl ObjectBinding {
    fn new(texture: TextureKind) -> Self {
        Self {
            resources: None,
            texture,
            uniform: None,
            uniform_revision: 0,
            dirty: true,
            source: None,
            light_revision: 0,
            light_bounds: None,
            transform: None,
            numeric_parameters: std::sync::Arc::from([]),
            parameter_revision: 0,
            parameter_dirty: true,
        }
    }
    fn binding(&self) -> &wgpu::BindGroup {
        &self
            .resources
            .as_ref()
            .expect("individual draw prepared")
            .binding
    }
}

/// Object-only inputs stay unchanged when the camera, lighting, fog or graph clock moves.
struct ObjectUniformSource {
    model: Mat4,
    previous_model: Option<Mat4>,
    tail: [f32; 8],
    surface: [f32; 4],
    double_sided: bool,
    light_mask: u32,
}
fn same_float_bits<const N: usize>(a: [f32; N], b: [f32; N]) -> bool {
    a.map(f32::to_bits) == b.map(f32::to_bits)
}
fn same_matrix_bits(a: Mat4, b: Mat4) -> bool {
    same_float_bits(a.to_cols_array(), b.to_cols_array())
}
impl PartialEq for ObjectUniformSource {
    fn eq(&self, other: &Self) -> bool {
        same_matrix_bits(self.model, other.model)
            && match (self.previous_model, other.previous_model) {
                (Some(a), Some(b)) => same_matrix_bits(a, b),
                (None, None) => true,
                _ => false,
            }
            && same_float_bits(self.tail, other.tail)
            && same_float_bits(self.surface, other.surface)
            && self.double_sided == other.double_sided
            && self.light_mask == other.light_mask
    }
}
struct DepthTarget {
    view: wgpu::TextureView,
    size: [u32; 2],
}

/// Indexed geometry, per-object matrices/materials, sampled textures, and depth testing.
/// HDR opaque/transparent passes. Imported color images are sRGB; procedural colors are linear.
pub struct SceneRenderer {
    profiler: crate::profiling::GpuProfiler,
    skinning: skinning::Skinning,
    sprites: sprites::Sprites,
    hud: Option<hud::HudRenderer>,
    output_format: wgpu::TextureFormat,
    hud_scale: f32,
    geometry: Option<geometry::GeometryBuffers>,
    motion_history: geometry::MotionHistory,
    particles: Option<particles::Particles>,
    text: Option<text::TextRenderer>,
    world_text: text::WorldCache,
    stats: FrameStats,
    surface_preparation: preparation::SurfacePreparation,
    surface_preparation_caching: bool,
    culling: bool,
    early_frustum_acceptance: bool,
    shadow_preparation_cache: bool,
    sun_fit_caching: bool,
    shadow_metadata_reuse: bool,
    occlusion: occlusion::Occlusion,
    state_caching: bool,
    light_selection: local_lights::LightSelection,
    instancing: instancing::Instancing,
    environment: environment::Environment,
    display: display::Display,
    shadows: shadows::Shadows,
    shadow_frame: Option<shadows::ShadowFrame>,
    pipeline: [wgpu::RenderPipeline; 2],
    transparent_pipeline: [wgpu::RenderPipeline; 2],
    layout: wgpu::BindGroupLayout,
    frame_buffer: wgpu::Buffer,
    frame_uniform: Option<Vec<u8>>,
    graph_parameter_defaults: wgpu::Buffer,
    /// Shader graph modules by content hash; compiled on first use per frame.
    graphs: BTreeMap<(u64, bool), std::sync::Arc<GraphPipelines>>,
    idle_graphs: std::collections::VecDeque<(u64, bool)>,
    neutral_normal: wgpu::TextureView,
    quad: MeshBuffers,
    cube: MeshBuffers,
    sphere: MeshBuffers,
    white: wgpu::TextureView,
    checker: wgpu::TextureView,
    sampler: wgpu::Sampler,
    model_sampler: wgpu::Sampler,
    mipmaps: crate::mipmap::Mipmaps,
    linear_mipmaps: crate::mipmap::Mipmaps,
    pbr: crate::pbr::PbrRenderer,
    objects: Vec<ObjectBinding>,
    object_identities: Vec<Option<preparation::SurfaceIdentity>>,
    uniform_serial: u64,
    surface_variants: variants::VariantCache,
    shader_optimizations: bool,
    hud_batching_enabled: bool,
    submission: submission::Submission,
    frame_scratch: frame_scratch::Scratch,
    depth: Option<DepthTarget>,
    imported_meshes: BTreeMap<String, MeshBuffers>,
    models: BTreeMap<String, Vec<UploadedPart>>,
    transparent_textures: BTreeSet<String>,
    imported_textures: BTreeMap<String, wgpu::TextureView>,
    generated_textures: BTreeMap<bozzard_compute::Handle, wgpu::TextureView>,
    generated_revision: (u64, u64),
    model_upload_stats: BTreeMap<String, ModelUploadStats>,
}

pub(crate) fn float_bytes(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

fn graph_parameter_bytes(parameters: &[[f32; 4]]) -> [u8; GRAPH_PARAMETER_RECORD_BYTES] {
    let mut bytes = [0; GRAPH_PARAMETER_RECORD_BYTES];
    for (slot, value) in bytes.chunks_exact_mut(4).zip(parameters.iter().flatten()) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Splice a graph's `graph_material_surface` over the host's stock call site.
/// Only fs_main calls it with fragment inputs, so the replacement is unique.
fn graph_module_text(pbr: bool, source: &ShaderSource) -> String {
    let host = host_text(pbr)
        .replacen(
            "override stock_surface: bool = true;",
            "override stock_surface: bool = false;",
            1,
        )
        .replacen(
            "default_material_surface(in",
            "graph_material_surface(in",
            1,
        );
    format!("{host}\n{}", source.surface)
}

pub(crate) fn previous_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: 32,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![8 => Float32x3],
    }
}
fn scene_pipeline(
    gpu: &Gpu,
    label: &str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    pbr: bool,
    transparent: bool,
    auxiliary: bool,
) -> wgpu::RenderPipeline {
    let basic_buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
        }),
        None,
        Some(previous_vertex_layout()),
    ];
    let pbr_buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 48,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x2, 5 => Float32x2, 6 => Float32x2, 7 => Float32x2],
        }),
        Some(previous_vertex_layout()),
    ];
    let buffers: &[Option<wgpu::VertexBufferLayout>] =
        if pbr { &pbr_buffers } else { &basic_buffers };
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &geometry::color_targets(
                    wgpu::TextureFormat::Rgba16Float,
                    transparent,
                    auxiliary,
                ),
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(!transparent),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}

fn bounds(vertices: &[[f32; 8]], indices: &[u32]) -> [Vec3; 2] {
    indices.iter().fold(
        [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)],
        |[min, max], i| {
            let p = Vec3::from_slice(&vertices[*i as usize][..3]);
            [min.min(p), max.max(p)]
        },
    )
}

fn mesh(gpu: &Gpu, vertices: &[[f32; 8]], indices: &[u32]) -> MeshBuffers {
    MeshBuffers {
        bounds: bounds(vertices, indices),
        vertices: gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("scene mesh vertices"),
                contents: &float_bytes(vertices.iter().flatten().copied()),
                usage: wgpu::BufferUsages::VERTEX
                    | wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC,
            }),
        indices: gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("scene mesh indices"),
                contents: &indices
                    .iter()
                    .flat_map(|i| i.to_le_bytes())
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_SRC,
            }),
        count: indices.len() as u32,
        vertex_offset: 0,
    }
}

/// UV sphere, radius 0.5, cube-compatible vertex layout.
fn sphere(gpu: &Gpu) -> MeshBuffers {
    const RINGS: u32 = 24;
    const SEGMENTS: u32 = 32;
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for ring in 0..=RINGS {
        let v = ring as f32 / RINGS as f32;
        let phi = v * std::f32::consts::PI;
        for segment in 0..=SEGMENTS {
            let u = segment as f32 / SEGMENTS as f32;
            let theta = u * std::f32::consts::TAU;
            let (sin_phi, cos_phi) = phi.sin_cos();
            let (sin_theta, cos_theta) = theta.sin_cos();
            let normal = [sin_phi * cos_theta, cos_phi, sin_phi * sin_theta];
            vertices.push([
                normal[0] * 0.5,
                normal[1] * 0.5,
                normal[2] * 0.5,
                normal[0],
                normal[1],
                normal[2],
                u,
                1. - v,
            ]);
        }
    }
    let stride = SEGMENTS + 1;
    for ring in 0..RINGS {
        for segment in 0..SEGMENTS {
            let a = ring * stride + segment;
            let b = a + stride;
            indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    mesh(gpu, &vertices, &indices)
}

fn cube(gpu: &Gpu) -> MeshBuffers {
    let faces = [
        (
            [0., 0., 1.],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0., 0., -1.],
            [
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [1., 0., 0.],
            [
                [0.5, -0.5, 0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
            ],
        ),
        (
            [-1., 0., 0.],
            [
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0., 1., 0.],
            [
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0., -1., 0.],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
            ],
        ),
    ];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (normal, points) in faces {
        let base = vertices.len() as u32;
        for (p, uv) in points
            .into_iter()
            .zip([[0., 1.], [1., 1.], [1., 0.], [0., 0.]])
        {
            vertices.push([
                p[0], p[1], p[2], normal[0], normal[1], normal[2], uv[0], uv[1],
            ]);
        }
        indices.extend([0, 1, 2, 0, 2, 3].map(|i| base + i));
    }
    mesh(gpu, &vertices, &indices)
}

fn texture(gpu: &Gpu, checker: bool) -> wgpu::TextureView {
    // These procedural palette values are authored in linear space, unlike imported sRGB images.
    let bright = [240, 180, 70, 255];
    let dark = [20, 90, 105, 255];
    let pixels = if checker {
        [bright, dark, dark, bright]
    } else {
        [[255; 4]; 4]
    };
    let extent = wgpu::Extent3d {
        width: 2,
        height: 2,
        depth_or_array_layers: 1,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("built-in scene texture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        pixels.as_flattened(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8),
            rows_per_image: Some(2),
        },
        extent,
    );
    texture.create_view(&Default::default())
}

/// Neutral tangent-space normal map (flat +Z) for graph texture slots on basic meshes.
fn neutral_texture(gpu: &Gpu) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("neutral normal texture"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[128, 128, 255, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    texture.create_view(&Default::default())
}

impl SceneRenderer {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let output_format = format;
        let environment = environment::Environment::new(gpu);
        let display = display::Display::new(gpu, format);
        let format = wgpu::TextureFormat::Rgba16Float;
        let mut entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(OBJECT_UNIFORM_BYTES as u64),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ];
        // Graph texture slots; procedural meshes bind neutral placeholders.
        for slot in 1..5u32 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: slot * 2 + 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: slot * 2 + 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 11,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(FRAME_UNIFORM_BYTES as u64),
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 12,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(GRAPH_PARAMETER_BUFFER_BYTES as u64),
            },
            count: None,
        });
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("scene object layout"),
                entries: &entries,
            });
        entries[0].ty = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(instancing::BUFFER_BYTES as u64),
        };
        let instance_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("instanced object layout"),
                    entries: &entries,
                });
        let native_instance_layout = instancing::native_arena_supported(gpu).then(|| {
            for entry in &mut entries {
                if entry.binding == 0 || entry.binding == 12 {
                    entry.ty = wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(256),
                    };
                }
            }
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 13,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(4),
                },
                count: None,
            });
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("native shared object storage layout"),
                    entries: &entries,
                })
        });
        let shadows = shadows::Shadows::new(gpu, &layout);
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("scene pipeline layout"),
                bind_group_layouts: &[
                    Some(&layout),
                    None,
                    Some(&shadows.sample_layout),
                    Some(&environment.layout),
                ],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("scene shader"),
                source: wgpu::ShaderSource::Wgsl(host_text(false).into()),
            });
        let make_pipeline = |transparent: bool, auxiliary: bool| {
            scene_pipeline(
                gpu,
                "scene pipeline",
                &pipeline_layout,
                &shader,
                false,
                transparent,
                auxiliary,
            )
        };
        let pipeline = [make_pipeline(false, false), make_pipeline(false, true)];
        let transparent_pipeline = [make_pipeline(true, false), make_pipeline(true, true)];
        let pbr = crate::pbr::PbrRenderer::new(
            gpu,
            format,
            &layout,
            &shadows.sample_layout,
            &environment.layout,
        );
        let quad = mesh(
            gpu,
            &[
                [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
                [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
                [0.5, 0.5, 0., 0., 0., 1., 1., 0.],
                [-0.5, 0.5, 0., 0., 0., 1., 0., 0.],
            ],
            &[0, 1, 2, 0, 2, 3],
        );
        Self {
            skinning: skinning::Skinning::default(),
            sprites: Default::default(),
            hud: None,
            output_format,
            hud_scale: 1.,
            geometry: None,
            motion_history: Default::default(),
            particles: None,
            text: None,
            world_text: Default::default(),
            stats: Default::default(),
            profiler: Default::default(),
            surface_preparation: Default::default(),
            surface_preparation_caching: true,
            culling: true,
            early_frustum_acceptance: true,
            shadow_preparation_cache: true,
            sun_fit_caching: true,
            shadow_metadata_reuse: true,
            occlusion: Default::default(),
            state_caching: true,
            light_selection: Default::default(),
            instancing: instancing::Instancing::new(instance_layout, native_instance_layout),
            environment,
            display,
            shadows,
            shadow_frame: None,
            pbr,
            pipeline,
            transparent_pipeline,
            layout,
            frame_buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene shared frame uniform"),
                size: FRAME_UNIFORM_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            frame_uniform: None,
            graph_parameter_defaults: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("empty graph numeric parameters"),
                size: GRAPH_PARAMETER_BUFFER_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: false,
            }),
            graphs: BTreeMap::new(),
            idle_graphs: Default::default(),
            neutral_normal: neutral_texture(gpu),
            quad,
            cube: cube(gpu),
            sphere: sphere(gpu),
            white: texture(gpu, false),
            checker: texture(gpu, true),
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("scene nearest repeat sampler"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            }),
            objects: Vec::new(),
            object_identities: Vec::new(),
            uniform_serial: 0,
            surface_variants: Default::default(),
            shader_optimizations: true,
            hud_batching_enabled: true,
            submission: Default::default(),
            frame_scratch: Default::default(),
            model_sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("model trilinear repeat sampler"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            mipmaps: crate::mipmap::Mipmaps::new(gpu, wgpu::TextureFormat::Rgba8UnormSrgb),
            linear_mipmaps: crate::mipmap::Mipmaps::new(gpu, wgpu::TextureFormat::Rgba8Unorm),
            depth: None,
            imported_meshes: BTreeMap::new(),
            models: BTreeMap::new(),
            transparent_textures: BTreeSet::new(),
            imported_textures: BTreeMap::new(),
            generated_textures: BTreeMap::new(),
            generated_revision: (0, 0),
            model_upload_stats: BTreeMap::new(),
        }
    }

    /// Compile both host flavors for one shader graph. WGSL errors panic like the
    /// startup modules; graphs are validated before codegen, so errors are engine bugs.
    fn compile_graph_variant(
        &self,
        gpu: &Gpu,
        source: &ShaderSource,
        auxiliary: bool,
        pbr: bool,
        transparent: bool,
    ) -> wgpu::RenderPipeline {
        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shader graph pipeline layout"),
                bind_group_layouts: &[
                    Some(&self.layout),
                    pbr.then(|| self.pbr.material_layout()),
                    Some(&self.shadows.sample_layout),
                    Some(&self.environment.layout),
                ],
                immediate_size: 0,
            });
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("shader graph module"),
                source: wgpu::ShaderSource::Wgsl(graph_module_text(pbr, source).into()),
            });
        scene_pipeline(
            gpu,
            "shader graph variant",
            &layout,
            &module,
            pbr,
            transparent,
            auxiliary,
        )
    }

    fn prepare_graph_variants(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        output_mask: u8,
    ) {
        let auxiliary = output_mask != 0;
        for batch in batches.iter().filter(|batch| batch.slot.is_none()) {
            let draw = &draws[batch.indices[0]];
            if self.variant_key(draw, 1, output_mask).is_some() {
                continue;
            }
            let Some(source) = &draw.object.material.shader else {
                continue;
            };
            let key = (source.id, auxiliary);
            let slot = usize::from(draw.transparent);
            let variants = if draw.pbr {
                &self.graphs[&key].pbr
            } else {
                &self.graphs[&key].basic
            };
            if variants[slot].is_some() {
                continue;
            }
            let pipeline =
                self.compile_graph_variant(gpu, source, auxiliary, draw.pbr, draw.transparent);
            let variants = std::sync::Arc::make_mut(self.graphs.get_mut(&key).unwrap());
            if draw.pbr {
                variants.pbr[slot] = Some(pipeline);
            } else {
                variants.basic[slot] = Some(pipeline);
            }
            self.stats.graph_variant_compilations += 1;
        }
    }

    fn object_resources(
        &self,
        gpu: &Gpu,
        key: &TextureKind,
        numeric: bool,
    ) -> Result<ObjectResources> {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene object uniform"),
            size: OBJECT_UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let parameters = numeric.then(|| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("individual graph numeric parameters"),
                size: GRAPH_PARAMETER_BUFFER_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let binding =
            self.texture_binding(gpu, key, &buffer, &self.layout, parameters.as_ref(), None)?;
        Ok(ObjectResources {
            buffer,
            binding,
            parameters,
        })
    }

    fn texture_binding(
        &self,
        gpu: &Gpu,
        key: &TextureKind,
        buffer: &wgpu::Buffer,
        layout: &wgpu::BindGroupLayout,
        parameters: Option<&wgpu::Buffer>,
        instance_ids: Option<&wgpu::Buffer>,
    ) -> Result<wgpu::BindGroup> {
        let bind = |texture: &wgpu::TextureView| {
            let mut entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(texture),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(
                        if let TextureKind::ModelPart(id, index) = key {
                            &self.models[id][*index].sampler
                        } else if *key == TextureKind::Text {
                            &self.model_sampler
                        } else {
                            &self.sampler
                        },
                    ),
                },
            ];
            if let Some(ids) = instance_ids {
                entries.push(wgpu::BindGroupEntry {
                    binding: 13,
                    resource: ids.as_entire_binding(),
                });
            }
            for slot in 1..5u32 {
                let view = if slot == 1 {
                    &self.neutral_normal
                } else {
                    &self.white
                };
                entries.push(wgpu::BindGroupEntry {
                    binding: slot * 2 + 1,
                    resource: wgpu::BindingResource::TextureView(view),
                });
                entries.push(wgpu::BindGroupEntry {
                    binding: slot * 2 + 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                });
            }
            entries.push(wgpu::BindGroupEntry {
                binding: 11,
                resource: self.frame_buffer.as_entire_binding(),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 12,
                resource: parameters
                    .unwrap_or(&self.graph_parameter_defaults)
                    .as_entire_binding(),
            });
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene object bindings"),
                layout,
                entries: &entries,
            })
        };
        let texture = self.texture_view(key)?;
        Ok(bind(texture))
    }

    fn texture_view(&self, key: &TextureKind) -> Result<&wgpu::TextureView> {
        Ok(match key {
            // A backpressured creation may reach extraction before its first GPU submission.
            TextureKind::Generated(handle) => {
                self.generated_textures.get(handle).unwrap_or(&self.white)
            }
            TextureKind::Text => self
                .text
                .as_ref()
                .and_then(|t| t.view.as_ref())
                .context("text atlas is not uploaded")?,
            TextureKind::White
            | TextureKind::Normals
            | TextureKind::ProceduralChecker
            | TextureKind::Toon => &self.white,
            TextureKind::Checker => &self.checker,
            TextureKind::ModelPart(id, index) => self
                .models
                .get(id)
                .and_then(|parts| parts.get(*index))
                .and_then(|part| part.texture.as_ref())
                .context("model texture is not uploaded")?,
            TextureKind::Imported(id) => self
                .imported_textures
                .get(id)
                .with_context(|| format!("texture '{id}' is not uploaded"))?,
        })
    }

    fn invalidate_object_bindings(&mut self) {
        self.world_text.clear();
        self.surface_preparation.clear();
        self.occlusion.invalidate();
        if let Some(hud) = &mut self.hud {
            hud.invalidate();
        }
        self.objects.clear();
        self.submission.invalidate();
        self.object_identities.clear();
        self.shadows.singletons.invalidate();
        self.instancing.bindings.clear();
        self.instancing.shadow_bindings.clear();
        self.instancing.clear_depth_plan();
        self.shadow_frame = None;
        self.shadows.sun_cache.clear();
        self.shadows.sun_fit.clear();
        self.shadows.spots.invalidate();
        self.shadows.points.invalidate();
    }

    /// Update only when resource identities change. Dispatching into an existing texture keeps
    /// object bind groups intact; the GPU sees the new pixels through the same texture view.
    pub fn set_generated_textures(
        &mut self,
        revision: (u64, u64),
        views: impl Iterator<Item = (bozzard_compute::Handle, wgpu::TextureView)>,
    ) {
        if self.generated_revision == revision {
            return;
        }
        self.generated_revision = revision;
        let next: BTreeMap<_, _> = views.collect();
        if self.generated_textures.keys().eq(next.keys()) {
            return;
        }
        self.generated_textures = next;
        self.invalidate_object_bindings();
    }

    pub fn clear_imported(&mut self) {
        self.model_upload_stats.clear();
        self.invalidate_object_bindings();
        self.imported_meshes.clear();
        self.imported_textures.clear();
        self.models.clear();
        self.skinning = Default::default();
        self.transparent_textures.clear();
    }

    /// Retire one catalog entry without invalidating unrelated GPU resources.
    pub fn remove_asset(&mut self, id: &str) {
        self.skinning.remove(id);
        self.imported_meshes.remove(id);
        self.models.remove(id);
        self.imported_textures.remove(id);
        self.transparent_textures.remove(id);
        self.model_upload_stats.remove(id);
        self.invalidate_object_bindings();
    }

    /// Give an immutable uploaded image another catalog ID without copying GPU
    /// storage. Texture-view handles keep the allocation alive independently.
    pub fn alias_image(&mut self, source: &str, target: &str) -> Result<()> {
        let image = self
            .imported_textures
            .get(source)
            .context("image alias source missing")?
            .clone();
        let transparent = self.transparent_textures.contains(source);
        self.remove_asset(target);
        self.imported_textures.insert(target.into(), image);
        if transparent {
            self.transparent_textures.insert(target.into());
        }
        Ok(())
    }

    pub fn upload_mesh(
        &mut self,
        gpu: &Gpu,
        id: &str,
        vertices: &[[f32; 8]],
        indices: &[u32],
    ) -> Result<()> {
        ensure!(
            !vertices.is_empty() && !indices.is_empty() && indices.len().is_multiple_of(3),
            "mesh needs indexed triangles"
        );
        ensure!(
            vertices.len() <= 1_000_000 && indices.len() <= 3_000_000,
            "mesh exceeds renderer limits"
        );
        ensure!(
            vertices.iter().flatten().all(|v| v.is_finite())
                && indices.iter().all(|i| (*i as usize) < vertices.len()),
            "invalid mesh data"
        );
        self.models.remove(id);
        self.model_upload_stats.remove(id);
        self.imported_textures.remove(id);
        self.transparent_textures.remove(id);
        self.invalidate_object_bindings();
        self.imported_meshes
            .insert(id.into(), mesh(gpu, vertices, indices));
        Ok(())
    }

    pub fn upload_image(
        &mut self,
        gpu: &Gpu,
        id: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        ensure!(
            width > 0
                && height > 0
                && width <= 4096
                && height <= 4096
                && width <= gpu.device.limits().max_texture_dimension_2d
                && height <= gpu.device.limits().max_texture_dimension_2d,
            "invalid image dimensions"
        );
        ensure!(
            rgba.len() == width as usize * height as usize * 4,
            "invalid RGBA image length"
        );
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(id),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        if rgba.chunks_exact(4).any(|pixel| pixel[3] < 255) {
            self.transparent_textures.insert(id.into());
        } else {
            self.transparent_textures.remove(id);
        }
        self.imported_textures
            .insert(id.into(), texture.create_view(&Default::default()));
        self.imported_meshes.remove(id);
        self.models.remove(id);
        self.model_upload_stats.remove(id);
        // Drop bind groups referring to old texture views; the next draw rebuilds them.
        self.invalidate_object_bindings();
        Ok(())
    }

    pub fn model_upload_stats(&self, id: &str) -> Option<ModelUploadStats> {
        self.model_upload_stats.get(id).copied()
    }

    fn upload_material_image(
        &self,
        gpu: &Gpu,
        image: &ModelImage<'_>,
        srgb: bool,
        cache: &mut ImageCache,
    ) -> Result<wgpu::TextureView> {
        ensure!(
            image.width > 0
                && image.height > 0
                && image.width <= 4096
                && image.height <= 4096
                && image.width <= gpu.device.limits().max_texture_dimension_2d
                && image.height <= gpu.device.limits().max_texture_dimension_2d
                && image.rgba.len() == image.width as usize * image.height as usize * 4,
            "invalid PBR image"
        );
        let key = (
            image.rgba.as_ptr() as usize,
            image.width,
            image.height,
            srgb,
        );
        if let Some((view, _)) = cache.get(&key) {
            return Ok(view.clone());
        }
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("PBR texture"),
            size: wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: crate::mipmap::levels(image.width, image.height),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            texture.as_image_copy(),
            image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            texture.size(),
        );
        if srgb {
            self.mipmaps.generate(gpu, &texture);
        } else {
            self.linear_mipmaps.generate(gpu, &texture);
        }
        let view = texture.create_view(&Default::default());
        cache.insert(
            key,
            (view.clone(), image.rgba.chunks_exact(4).any(|p| p[3] < 255)),
        );
        Ok(view)
    }

    /// Validate and upload a complete skinned model, including its deformation source.
    pub fn upload_skinned_model(
        &mut self,
        gpu: &Gpu,
        id: &str,
        vertices: &[[f32; 8]],
        indices: &[u32],
        parts: &[ModelPart<'_>],
        skin: SkinData<'_>,
    ) -> Result<()> {
        ensure!(!parts.is_empty(), "skinned models need surfaces");
        let bounds = skinning::Source::bounds(&skin, vertices)?;
        let part_bounds = skinning::Source::part_bounds(&skin, vertices, indices, parts)?;
        let bytes: Vec<u8> = skin
            .vertices
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let weights = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skin influences"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&weights, 0, &bytes);
        self.upload_model(gpu, id, vertices, indices, parts)?;
        self.skinning.sources.insert(
            id.into(),
            skinning::Source {
                signature: skin.signature,
                bindings: skin.bindings,
                weights,
                count: vertices.len(),
                bounds,
                part_bounds,
            },
        );
        Ok(())
    }
    /// Upload all model surfaces before replacing the prior GPU model.
    pub fn upload_model(
        &mut self,
        gpu: &Gpu,
        id: &str,
        vertices: &[[f32; 8]],
        indices: &[u32],
        parts: &[ModelPart<'_>],
    ) -> Result<()> {
        let started = std::time::Instant::now();
        if parts.is_empty() {
            return self.upload_mesh(gpu, id, vertices, indices);
        }
        ensure!(
            !vertices.is_empty()
                && vertices.len() <= 1_000_000
                && !indices.is_empty()
                && indices.len() <= 3_000_000,
            "invalid model size"
        );
        ensure!(
            vertices.iter().flatten().all(|v| v.is_finite())
                && indices.iter().all(|i| (*i as usize) < vertices.len()),
            "invalid model geometry"
        );
        let shared = mesh(gpu, vertices, indices);
        let mut uploaded = Vec::new();
        // Shared CPU image slices map to one GPU allocation within a model upload.
        // Keys never escape this call, so pointer reuse across later loads is irrelevant.
        let mut textures: ImageCache = BTreeMap::new();
        for part in parts {
            if let Some(shading) = &part.shading {
                shading.validate()?;
            }
            let start = part.start as usize;
            let end = start
                .checked_add(part.count as usize)
                .context("model index range overflow")?;
            ensure!(
                part.count > 0 && part.count.is_multiple_of(3) && end <= indices.len(),
                "invalid model surface range"
            );
            if let Some(shading) = &part.shading {
                let base = shading.vertex_start as usize;
                ensure!(
                    base.checked_add(shading.vertices.len())
                        .is_some_and(|end| end <= vertices.len())
                        && indices[start..end].iter().all(|i| (*i as usize) >= base
                            && (*i as usize) < base + shading.vertices.len()),
                    "shading attributes do not cover model surface"
                );
            }
            ensure!(
                part.color
                    .iter()
                    .all(|c| c.is_finite() && (0.0..=1.0).contains(c))
                    && part
                        .alpha_cutoff
                        .is_none_or(|a| a.is_finite() && (0.0..=1.0).contains(&a)),
                "invalid model material"
            );
            let mut image_translucent = false;
            let texture = if let Some(image) = &part.image {
                ensure!(
                    image.width > 0
                        && image.height > 0
                        && image.width <= 4096
                        && image.height <= 4096
                        && image.width <= gpu.device.limits().max_texture_dimension_2d
                        && image.height <= gpu.device.limits().max_texture_dimension_2d
                        && image.rgba.len() == image.width as usize * image.height as usize * 4,
                    "invalid model image"
                );
                let key = (
                    image.rgba.as_ptr() as usize,
                    image.width,
                    image.height,
                    true,
                );
                if let Some((view, translucent)) = textures.get(&key) {
                    image_translucent = *translucent;
                    Some(view.clone())
                } else {
                    let size = wgpu::Extent3d {
                        width: image.width,
                        height: image.height,
                        depth_or_array_layers: 1,
                    };
                    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("model base color"),
                        size,
                        mip_level_count: crate::mipmap::levels(image.width, image.height),
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_DST
                            | wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    });
                    gpu.queue.write_texture(
                        texture.as_image_copy(),
                        image.rgba,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(image.width * 4),
                            rows_per_image: Some(image.height),
                        },
                        size,
                    );
                    self.mipmaps.generate(gpu, &texture);
                    let view = texture.create_view(&Default::default());
                    image_translucent = image.rgba.chunks_exact(4).any(|p| p[3] < 255);
                    textures.insert(key, (view.clone(), image_translucent));
                    Some(view)
                }
            } else {
                None
            };
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for &index in &indices[start..end] {
                let p = Vec3::from_slice(&vertices[index as usize][..3]);
                min = min.min(p);
                max = max.max(p);
            }
            uploaded.push(UploadedPart {
                source_key: part.source_key.to_owned(),
                mesh: MeshBuffers {
                    bounds: [min, max],
                    vertices: shared.vertices.clone(),
                    indices: gpu
                        .device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("model surface indices"),
                            contents: &indices[start..end]
                                .iter()
                                .flat_map(|i| {
                                    (i - part.shading.as_ref().map_or(0, |s| s.vertex_start))
                                        .to_le_bytes()
                                })
                                .collect::<Vec<_>>(),
                            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_SRC,
                        }),
                    count: part.count,
                    vertex_offset: part
                        .shading
                        .as_ref()
                        .map_or(0, |s| s.vertex_start as u64 * 32),
                },
                texture,
                color: part.color,
                cutoff: part.alpha_cutoff,
                translucent: part.color[3] < 1.0 || image_translucent,
                center: min * 0.5 + max * 0.5,
                sampler: part.shading.as_ref().map_or_else(
                    || self.model_sampler.clone(),
                    |s| gpu.device.create_sampler(&s.base_color_sampler),
                ),
                shading: if let Some(shading) = &part.shading {
                    let mut views = [None, None, None, None];
                    for (slot, map) in [
                        &shading.normal,
                        &shading.metallic_roughness,
                        &shading.occlusion,
                        &shading.emissive,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if let Some(map) = map {
                            views[slot] = Some(self.upload_material_image(
                                gpu,
                                &map.image,
                                slot == 3,
                                &mut textures,
                            )?);
                        }
                    }
                    Some(self.pbr.upload(gpu, shading, views))
                } else {
                    None
                },
            });
        }
        self.imported_meshes.remove(id);
        self.models.insert(id.into(), uploaded);
        self.imported_textures.remove(id);
        self.transparent_textures.remove(id);
        self.model_upload_stats.insert(
            id.into(),
            ModelUploadStats {
                surfaces: parts.len(),
                unique_images: textures.len(),
                texture_bytes: textures
                    .keys()
                    .map(|(_, width, height, _)| crate::mipmap::texture_bytes(*width, *height))
                    .sum(),
                cpu_upload_ms: started.elapsed().as_secs_f64() * 1000.0,
                prepare_ms: 0.0,
                upload_slices: 1,
                max_slice_cpu_ms: started.elapsed().as_secs_f64() * 1000.0,
                max_slice_bytes: vertices.len() * 32
                    + indices.len() * 4
                    + parts
                        .iter()
                        .filter_map(|p| p.shading.as_ref())
                        .map(|s| s.vertices.len() * 48)
                        .sum::<usize>()
                    + textures
                        .keys()
                        .map(|(_, w, h, _)| crate::mipmap::texture_bytes(*w, *h))
                        .sum::<usize>(),
            },
        );
        self.invalidate_object_bindings();
        Ok(())
    }
    fn prepared_record<'a>(
        &'a self,
        draw: &PreparedDraw,
        binding: &'a wgpu::BindGroup,
        output_mask: u8,
        instances: u32,
        first_instance: u32,
    ) -> submission::Record<'a> {
        let object = &draw.object;
        let auxiliary = output_mask != 0;
        let shading = match &object.mesh {
            MeshKind::ModelPart(id, index) => self.models[id][*index].shading.as_ref(),
            _ => None,
        };
        let key = (
            shading.is_some(),
            draw.shader,
            draw.transparent,
            instances > 1,
        );
        let pipeline = if let Some(key) = self.variant_key(draw, instances, output_mask) {
            &self.surface_variants.pipelines[&key]
        } else if instances > 1
            && let Some(id) = draw.shader
        {
            self.graphs[&(id, auxiliary)].instanced[usize::from(shading.is_some())]
                .as_ref()
                .unwrap()
        } else if instances > 1 {
            self.instancing.pipelines.as_ref().unwrap().pipelines[usize::from(auxiliary)]
                [usize::from(shading.is_some())]
            .as_ref()
            .unwrap()
        } else {
            match (key.0, key.1, key.2) {
                (true, Some(id), false) => self.graphs[&(id, auxiliary)].pbr[0].as_ref().unwrap(),
                (true, Some(id), true) => self.graphs[&(id, auxiliary)].pbr[1].as_ref().unwrap(),
                (false, Some(id), false) => {
                    self.graphs[&(id, auxiliary)].basic[0].as_ref().unwrap()
                }
                (false, Some(id), true) => self.graphs[&(id, auxiliary)].basic[1].as_ref().unwrap(),
                (true, None, false) => &self.pbr.opaque[usize::from(auxiliary)],
                (true, None, true) => &self.pbr.transparent[usize::from(auxiliary)],
                (false, None, false) => &self.pipeline[usize::from(auxiliary)],
                (false, None, true) => &self.transparent_pipeline[usize::from(auxiliary)],
            }
        };
        let merged = self.world_text.entry(draw.source_item);
        let mesh = merged.map(|entry| &entry.mesh).unwrap_or_else(|| {
            self.skinning
                .mesh(object)
                .unwrap_or_else(|| match &object.mesh {
                    MeshKind::Text(text) => self.text.as_ref().unwrap().mesh(text).unwrap(),
                    MeshKind::SharedText(text) => self.text.as_ref().unwrap().mesh(text).unwrap(),
                    MeshKind::Sprite(sprite) => self.sprites.mesh(sprite).unwrap(),
                    MeshKind::Quad => &self.quad,
                    MeshKind::Cube => &self.cube,
                    MeshKind::Sphere => &self.sphere,
                    MeshKind::Imported(id) => &self.imported_meshes[id],
                    MeshKind::ModelPart(id, index) => &self.models[id][*index].mesh,
                })
        });
        submission::Record {
            pipeline,
            groups: [
                Some(merged.map_or(binding, |entry| &entry.binding)),
                shading.map(|s| &s.binding),
                Some(&self.shadows.sample_binding),
                Some(&self.environment.binding),
            ],
            vertices: [
                Some((&mesh.vertices, mesh.vertex_offset)),
                shading.map(|s| (self.skinning.tangents(object).unwrap_or(&s.vertices), 0)),
                (output_mask & 2 != 0).then(|| {
                    (
                        self.skinning.previous(object).unwrap_or(&mesh.vertices),
                        mesh.vertex_offset,
                    )
                }),
            ],
            indices: &mesh.indices,
            index_count: mesh.count,
            instances: if merged.is_some() { 1 } else { instances },
            first_instance: if merged.is_some() { 0 } else { first_instance },
            geometry_stable: draw.deformation == 0,
        }
    }
    fn draw_prepared(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        draw: &PreparedDraw,
        binding: &wgpu::BindGroup,
        output_mask: u8,
        call: DrawCall<'_>,
        state: &mut draw_state::DrawState,
    ) -> (u64, usize) {
        let record = self.prepared_record(
            draw,
            binding,
            output_mask,
            call.instances,
            call.first_instance,
        );
        let binds = usize::from(state.pipeline(pass, record.pipeline, self.state_caching));
        for (slot, group) in record.groups.iter().enumerate() {
            if let Some(group) = group {
                state.group(pass, slot, group, self.state_caching);
            }
        }
        for (slot, vertex) in record.vertices.iter().enumerate() {
            if let Some((buffer, offset)) = vertex {
                state.vertex(pass, slot, buffer, *offset, self.state_caching);
            }
        }
        state.index(pass, record.indices, self.state_caching);
        if let Some((buffer, offset)) = call
            .indirect
            .filter(|_| self.world_text.entry(draw.source_item).is_none())
        {
            pass.draw_indexed_indirect(buffer, offset);
        } else {
            pass.draw_indexed(
                0..record.index_count,
                0,
                record.first_instance..record.first_instance + record.instances,
            );
        }
        (record.triangles(), binds)
    }
    /// Logical-to-physical pixel scale for HUD text; world rendering is unchanged.
    pub fn set_hud_scale(&mut self, scale: f32) {
        self.hud_scale = if scale.is_finite() {
            scale.clamp(0.25, 8.)
        } else {
            1.
        };
    }
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        size: [u32; 2],
        scene: &RenderScene,
    ) -> Result<()> {
        self.draw_frame(gpu, target, size, scene, false)
    }
    /// Diagnostic linear readback: bypass fog, bloom, exposure, tone mapping and display encoding.
    /// Requires a non-sRGB output. Normal editor/player rendering must use draw.
    pub fn draw_linear(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        size: [u32; 2],
        scene: &RenderScene,
    ) -> Result<()> {
        self.draw_frame(gpu, target, size, scene, true)
    }
    fn draw_frame(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        size: [u32; 2],
        scene: &RenderScene,
        raw: bool,
    ) -> Result<()> {
        let result = self.draw_frame_inner(gpu, target, size, scene, raw);
        if result.is_err() {
            // Skin commands now share the frame encoder. A failed frame drops that
            // encoder before submission, so its cached poses must be retried.
            self.skinning.invalidate();
            self.motion_history.abort();
            self.world_text.clear();
            self.surface_preparation.clear();
            self.stats.surface_preparation_bytes = 0;
            self.shadow_frame = None;
        }
        result
    }
    fn draw_frame_inner(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        size: [u32; 2],
        scene: &RenderScene,
        raw: bool,
    ) -> Result<()> {
        let started = std::time::Instant::now();
        self.stats = FrameStats::default();
        self.shadows.sun_cache.reset_work_stats();
        self.shadows.spots.reset_work_stats();
        self.shadows.points.reset_work_stats();
        self.stats.viewport_size = size;
        ensure!(
            size.iter()
                .all(|s| *s > 0 && *s <= gpu.device.limits().max_texture_dimension_2d),
            "invalid render dimensions"
        );
        ensure!(
            scene.view_projection.is_finite() && scene.view_projection.inverse().is_finite(),
            "invalid view/projection matrix"
        );
        for item in &scene.items {
            ensure!(
                item.material
                    .metallic
                    .iter()
                    .chain(item.material.roughness.iter())
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "invalid surface factors"
            );
            ensure!(
                item.material.surface_overrides.len() <= 4096,
                "too many material overrides"
            );
            let mut surfaces = BTreeSet::new();
            for value in item.material.surface_overrides.iter() {
                ensure!(
                    value.transform.is_finite() && value.transform.inverse().is_finite(),
                    "invalid surface transform"
                );
                ensure!(
                    value.uv_scale.iter().all(|v| v.is_finite() && *v > 0.0),
                    "invalid surface UV scale"
                );
                ensure!(
                    value.surface < 4096 && surfaces.insert(value.surface),
                    "invalid or duplicate material override surface"
                );
                ensure!(
                    value.source.len() == 16
                        && value
                            .source
                            .bytes()
                            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
                    "invalid override source signature"
                );
                ensure!(
                    value
                        .tint
                        .iter()
                        .chain(value.metallic.iter())
                        .chain(value.roughness.iter())
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                    "invalid material override factors"
                );
            }
        }
        scene.fog.validate()?;
        scene.display.validate()?;
        let stores = geometry::stores(scene, raw, self.state_caching);
        let auxiliary = stores.iter().any(|store| *store);
        let output_mask = if self.shader_optimizations {
            variants::mask(stores)
        } else if auxiliary {
            7
        } else {
            0
        };
        let (view_projection, temporal_frame) = self.motion_history.begin(scene, size, raw);
        self.environment.prepare(
            gpu,
            scene.environment,
            view_projection.inverse(),
            !raw && scene.display.reflections.enabled,
        )?;
        self.environment.prepare_mask(gpu, output_mask);
        if self.depth.as_ref().is_none_or(|d| d.size != size) {
            self.geometry = None;
            if let Some(particles) = &mut self.particles {
                particles.invalidate_depth();
            }
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scene depth target"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.depth = Some(DepthTarget {
                view: texture.create_view(&Default::default()),
                size,
            });
        }
        if auxiliary && self.geometry.is_none() {
            self.geometry = Some(geometry::GeometryBuffers::new(gpu, size));
        }
        self.display.prepare(
            gpu,
            scene.display,
            post_process::FrameInput {
                size,
                raw,
                view_projection,
                geometry: self.geometry.as_ref(),
                environment_gpu: Some(&self.environment),
                fog: scene.fog,
                temporal: temporal_frame,
                depth: Some(&self.depth.as_ref().unwrap().view),
                lighting: scene.lighting,
                environment: scene.environment,
                shadows: Some(&self.shadows),
            },
        )?;
        self.prepare_gi(gpu, scene.gi.as_ref())?;
        scene.lighting.validate()?;
        let lights = local_lights::uniform(&scene.lights)?;
        gpu.queue
            .write_buffer(&self.shadows.local_lights, 0, &lights);
        self.prepare_text(gpu, scene)?;
        let mut encoder = self.profiler.encoder(gpu);
        self.stats.frame_id = encoder.frame;
        self.skinning.prepare(
            gpu,
            scene,
            &self.models,
            &mut encoder,
            view_projection,
            self.culling,
        )?;
        self.stats.skinning_dispatches = self.skinning.dispatches;
        self.stats.skinning_shared_copies = self.skinning.shared_copies;
        self.stats.skinning_shared_actors = self.skinning.shared_actors;
        self.stats.skinning_culled_actors = self.skinning.culled_actors;
        self.sprites.prepare(gpu, &scene.items)?;
        let mut draws = self.prepare(scene);
        for draw in &mut draws {
            draw.shared_geometry = self
                .skinning
                .instance_geometry(&draw.object, output_mask & 2 != 0);
            if draw.shader.is_none()
                && !draw.pbr
                && draw.deformation == 0
                && (draw
                    .object
                    .mesh
                    .text()
                    .is_some_and(|text| text.screen.is_none())
                    || matches!(&draw.object.mesh, MeshKind::Sprite(sprite) if sprite.screen.is_none()))
            {
                let count = self.mesh_for(&draw.object).count;
                draw.world_geometry_units = count.is_multiple_of(6).then_some(count as usize / 6);
            }
        }
        // Keep all active pipelines and a bounded set of recently absent previews.
        let mut graph_sources: BTreeMap<(u64, bool), &ShaderSource> = BTreeMap::new();
        for draw in &draws {
            if let Some(shader) = &draw.object.material.shader {
                graph_sources.insert((shader.id, auxiliary), shader.as_ref());
            }
        }
        ensure!(
            graph_sources.len() <= 256,
            "a rendered view supports at most 256 active shader variants"
        );
        self.idle_graphs
            .retain(|id| !graph_sources.contains_key(id));
        for id in self
            .graphs
            .keys()
            .filter(|id| !graph_sources.contains_key(id))
        {
            if !self.idle_graphs.contains(id) {
                self.idle_graphs.push_back(*id);
            }
        }
        while self.idle_graphs.len() > if self.state_caching { 8 } else { 0 } {
            self.graphs.remove(&self.idle_graphs.pop_front().unwrap());
        }
        for (id, _) in graph_sources {
            if let std::collections::btree_map::Entry::Vacant(entry) = self.graphs.entry(id) {
                let pipelines = GraphPipelines {
                    basic: [None, None],
                    pbr: [None, None],
                    instanced: [None, None],
                };
                entry.insert(std::sync::Arc::new(pipelines));
                self.stats.graph_compilations += 1;
            }
        }
        self.stats.resident_graphs = self.graphs.len();
        self.remap_object_bindings(&draws);
        self.objects.truncate(draws.len());
        for (index, draw) in draws.iter().enumerate() {
            let object = &draw.object;
            self.texture_view(&object.material.texture)?;
            if index == self.objects.len() {
                self.objects
                    .push(ObjectBinding::new(object.material.texture.clone()));
            } else if self.objects[index].texture != object.material.texture {
                self.objects[index].texture = object.material.texture.clone();
                self.objects[index].resources = None;
                self.objects[index].dirty = true;
            }
            if let MeshKind::ModelPart(id, index) = &object.mesh {
                ensure!(
                    self.models
                        .get(id)
                        .is_some_and(|parts| *index < parts.len()),
                    "model surface is not uploaded"
                );
            }
            if let MeshKind::Imported(id) = &object.mesh {
                ensure!(
                    self.imported_meshes.contains_key(id),
                    "mesh '{id}' is not uploaded"
                );
            }
        }
        let mut bounds = std::mem::take(&mut self.frame_scratch.bounds);
        bounds.clear();
        bounds.extend(draws.iter().map(|d| self.mesh_for(&d.object).bounds));
        let visibility_started = std::time::Instant::now();
        let mut visible = std::mem::take(&mut self.frame_scratch.visible);
        self.visibility(scene, &draws, &bounds, &mut visible);
        let mut frustum_visible = std::mem::take(&mut self.frame_scratch.frustum);
        frustum_visible.clear();
        frustum_visible.extend_from_slice(&visible);
        self.apply_cached_instance_occlusion(gpu, &draws, view_projection, size, &mut visible);
        self.stats.visibility_ms = visibility_started.elapsed().as_secs_f64() * 1000.;
        self.light_selection.update(&scene.lights);
        self.stats.scene_items = scene.items.len();
        self.stats.surfaces = draws.len();
        self.stats.visible_surfaces = frustum_visible.iter().filter(|v| **v).count();
        let mut visible_items = std::mem::take(&mut self.frame_scratch.items);
        visible_items.clear();
        visible_items.resize(scene.items.len(), false);
        for (draw, &is_visible) in draws.iter().zip(&frustum_visible) {
            visible_items[draw.source_item] |= is_visible;
        }
        self.stats.visible_items = visible_items.iter().filter(|v| **v).count();
        self.stats.culled_surfaces = draws.len() - self.stats.visible_surfaces;
        let inverse_view_projection = view_projection.inverse().to_cols_array();
        let lighting_uniform = scene.lighting.uniform();
        let fog_uniform = scene.fog.uniform(raw);
        let frame_uniform = float_bytes(
            view_projection
                .to_cols_array()
                .into_iter()
                .chain(temporal_frame.previous_vp.to_cols_array())
                .chain(inverse_view_projection)
                .chain([size[0] as f32, size[1] as f32, 0., 0.])
                .chain(lighting_uniform)
                .chain(fog_uniform)
                .chain([scene.shader_time, 0., 0., 0.]),
        );
        debug_assert_eq!(frame_uniform.len(), FRAME_UNIFORM_BYTES);
        let camera_changed = self
            .frame_uniform
            .as_ref()
            .is_none_or(|previous| previous[..64] != frame_uniform[..64]);
        if !self.state_caching || self.frame_uniform.as_ref() != Some(&frame_uniform) {
            // Queue writes survive a later draw error. Invalidate before writing
            // so reverting to the last good camera must upload its constants.
            self.frame_uniform = None;
            gpu.queue
                .write_buffer(&self.frame_buffer, 0, &frame_uniform);
            self.stats.frame_uniform_bytes = frame_uniform.len();
        }
        for (index, (draw, binding)) in draws.iter().zip(&mut self.objects).enumerate() {
            let object = &draw.object;
            let parameters = object
                .material
                .shader
                .as_ref()
                .map(|s| &s.numeric_parameters);
            ensure!(
                parameters.is_none_or(|p| p.len() <= GRAPH_PARAMETER_SLOTS
                    && p.iter().flatten().all(|v| v.is_finite())),
                "invalid graph numeric parameters"
            );
            let parameters_equal = parameters.map_or_else(
                || binding.numeric_parameters.is_empty(),
                |parameters| {
                    std::sync::Arc::ptr_eq(parameters, &binding.numeric_parameters)
                        || parameters.len() == binding.numeric_parameters.len()
                            && parameters
                                .iter()
                                .flatten()
                                .zip(binding.numeric_parameters.iter().flatten())
                                .all(|(a, b)| a.to_bits() == b.to_bits())
                },
            );
            if !parameters_equal || !self.state_caching {
                binding.numeric_parameters = parameters
                    .cloned()
                    .unwrap_or_else(|| std::sync::Arc::from([]));
                self.uniform_serial = self.uniform_serial.wrapping_add(1);
                binding.parameter_revision = self.uniform_serial;
                binding.parameter_dirty = true;
                if binding
                    .resources
                    .as_ref()
                    .is_some_and(|r| r.parameters.is_none())
                    && !binding.numeric_parameters.is_empty()
                {
                    binding.resources = None;
                    binding.dirty = true;
                }
            }
            let previous_model = self.motion_history.previous_model(object);
            let material = &object.material;
            let tail = [
                material.tint[0],
                material.tint[1],
                material.tint[2],
                draw.opacity,
                material.uv_scale[0],
                material.uv_scale[1],
                if material.lit { 1.0 } else { 0.0 },
                draw.cutoff,
            ];
            let double_sided = match &object.mesh {
                MeshKind::ModelPart(id, index) => self.models[id][*index]
                    .shading
                    .as_ref()
                    .is_none_or(|s| s.double_sided),
                _ => true,
            };
            let surface = [
                draw.pbr_override[0],
                draw.pbr_override[1],
                match material.texture {
                    TextureKind::Normals => 1.,
                    TextureKind::ProceduralChecker => 2.,
                    TextureKind::Toon => 3.,
                    _ => 0.,
                },
                if draw.transparent || previous_model.is_none() {
                    1.
                } else {
                    0.
                },
            ];
            let lights_present = !scene.lights.is_empty();
            let light_cache_hit = lights_present
                && self.state_caching
                && binding.light_revision == self.light_selection.revision()
                && binding.light_bounds == Some(bounds[index])
                && binding.source.as_ref().is_some_and(|source| {
                    same_matrix_bits(source.model, object.model)
                        && source.tail[6].to_bits() == tail[6].to_bits()
                });
            let light_mask = if !lights_present {
                0
            } else if light_cache_hit {
                binding.source.as_ref().unwrap().light_mask
            } else {
                self.stats.light_mask_builds += 1;
                self.light_selection
                    .mask(object.model, bounds[index], material.lit)
            };
            if visible[index] && material.lit {
                self.stats.local_light_slots += scene.lights.len();
                self.stats.local_light_candidates += light_mask.count_ones() as usize;
            }
            let source = ObjectUniformSource {
                model: object.model,
                previous_model,
                tail,
                surface,
                double_sided,
                light_mask,
            };
            let unchanged = self.state_caching && binding.source.as_ref() == Some(&source);
            // Keep the original combined-matrix validation, including on camera
            // changes, without revalidating every stationary object each frame.
            if camera_changed || !unchanged {
                ensure!(
                    (view_projection * object.model).is_finite(),
                    "invalid object matrix"
                );
            }
            if unchanged {
                if lights_present && !light_cache_hit {
                    binding.light_revision = self.light_selection.revision();
                    binding.light_bounds = Some(bounds[index]);
                }
                continue;
            }
            let (normal, determinant) = if self.state_caching
                && let Some((model, normal, determinant)) = binding.transform
                && same_matrix_bits(model, object.model)
            {
                (normal, determinant)
            } else {
                self.stats.normal_matrix_builds += 1;
                (
                    object.model.inverse().transpose(),
                    object.model.determinant().signum(),
                )
            };
            ensure!(
                object.model.is_finite() && normal.is_finite(),
                "invalid object matrix"
            );
            self.stats.object_uniform_builds += 1;
            binding.transform = Some((object.model, normal, determinant));
            let values = normal
                .to_cols_array()
                .into_iter()
                .chain(tail)
                .chain(object.model.to_cols_array())
                .chain([
                    determinant,
                    if double_sided { 1. } else { 0. },
                    (light_mask & 0xffff) as f32,
                    (light_mask >> 16) as f32,
                ])
                .chain(surface)
                .chain(previous_model.unwrap_or(object.model).to_cols_array());
            let mut uniform = [0; OBJECT_UNIFORM_BYTES];
            debug_assert_eq!(values.clone().count() * 4, uniform.len());
            for (slot, value) in uniform.chunks_exact_mut(4).zip(values) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            if !self.state_caching || binding.uniform.as_ref() != Some(&uniform) {
                binding.uniform = Some(uniform);
                self.uniform_serial = self.uniform_serial.wrapping_add(1);
                binding.uniform_revision = self.uniform_serial;
                binding.dirty = true;
            }
            binding.source = Some(source);
            if lights_present && !light_cache_hit {
                binding.light_revision = self.light_selection.revision();
                binding.light_bounds = Some(bounds[index]);
            }
        }
        // Publish the stamp only after all objects validate, so an error cannot
        // make a retry skip validation against a newly changed camera.
        self.frame_uniform = Some(frame_uniform);
        for (index, draw) in draws.iter_mut().enumerate() {
            let determinant = self.objects[index]
                .transform
                .expect("validated object transform")
                .2;
            draw.raster =
                variants::raster_class(&self.models, draw, self.shader_optimizations, determinant);
        }
        self.instancing
            .set_transparent_runs_allowed(raw || scene.particles.is_empty());
        let batches =
            self.prepare_instances(gpu, &draws, &bounds, &visible, view_projection, output_mask)?;
        self.stats.native_instance_arena = self.instancing.arena_enabled();
        let mut world_text = std::mem::take(&mut self.world_text);
        let world_result = world_text.prepare(
            self,
            gpu,
            &mut encoder,
            &draws,
            &batches,
            !scene.particles.is_empty() && !raw,
        );
        self.world_text = world_text;
        world_result?;
        self.stats.world_draws_saved = self.world_text.draws_saved;
        self.stats.world_geometry_copies = self.world_text.geometry_copies;
        self.stats.world_id_bytes = self.world_text.id_bytes;
        self.prepare_graph_variants(gpu, &draws, &batches, output_mask);
        self.prepare_instanced_graphs(gpu, &draws, &batches, output_mask);
        self.prepare_surface_variants(
            gpu,
            &draws,
            &batches,
            output_mask,
            !scene.particles.is_empty() && !raw,
        );
        let occlusion = self.prepare_occlusion(
            gpu,
            &mut encoder,
            view_projection,
            size,
            &draws,
            &frustum_visible,
            &batches,
        );
        let state_started = std::time::Instant::now();
        let reuse_metadata = self.shadow_metadata_reuse && self.state_caching;
        let mut shadow_frame =
            (!reuse_metadata).then(|| shadows::ShadowFrame::new(scene, &draws, self.culling));
        let mut comparison = if reuse_metadata {
            self.shadow_frame.as_ref().map_or_else(
                || shadows::Comparison::cold(draws.len()),
                |p| p.compare(scene, &draws, self.culling),
            )
        } else {
            let frame = shadow_frame.as_ref().unwrap();
            self.stats.shadow_metadata_built_casters =
                draws.iter().filter(|d| d.object.material.lit).count();
            self.stats.shadow_metadata_key_clones = self.stats.shadow_metadata_built_casters * 2;
            let whole = self.state_caching && self.shadow_frame.as_ref() == Some(frame);
            shadows::Comparison {
                whole,
                sun: self.state_caching
                    && self
                        .shadow_frame
                        .as_ref()
                        .is_some_and(|p| p.same_sun(frame)),
                opaque: self.shadow_preparation_cache
                    && !whole
                    && self.state_caching
                    && (scene.lights.iter().any(LocalLight::casts_shadow)
                        || scene.lighting.shadows && scene.lighting.sun_intensity > 0.)
                    && self
                        .shadow_frame
                        .as_ref()
                        .is_some_and(|p| p.same_local_casters(frame)),
                stable_mask: Vec::new(),
                unchanged: Vec::new(),
            }
        };
        self.stats.shadow_state_ms = state_started.elapsed().as_secs_f64() * 1000.;
        self.stats.shadow_cache_hit = self.state_caching && comparison.whole;
        let mut sun_changed = !self.state_caching || !comparison.sun;
        let same_opaque_casters = self.shadow_preparation_cache
            && !self.stats.shadow_cache_hit
            && self.state_caching
            && (scene.lights.iter().any(LocalLight::casts_shadow)
                || scene.lighting.shadows && scene.lighting.sun_intensity > 0.)
            && comparison.opaque;
        let mut previous_frame = None;
        let mut spot_changes = Vec::new();
        let mut point_changes = Vec::new();
        let mut shadow_batches = None;
        let mut sun_plan = None;
        if !self.stats.shadow_cache_hit {
            // Invalidate before queueing writes: a later frame error must not leave a
            // valid-looking stamp paired with partially updated shadow uniforms.
            previous_frame = self.shadow_frame.take();
            let mut uniform_unchanged = false;
            if sun_changed {
                let update =
                    self.update_shadows(gpu, scene, &draws, &bounds, same_opaque_casters)?;
                uniform_unchanged = update.0;
                sun_changed = !update.1;
                self.stats.sun_shadow_fit_reused = update.1;
            }
            self.update_spot_shadows(gpu, scene)?;
            self.update_point_shadows(gpu, scene)?;
            let spots = self
                .shadows
                .spots
                .changes(self, &draws, same_opaque_casters);
            let points = self
                .shadows
                .points
                .changes(self, &draws, same_opaque_casters);
            self.stats.local_shadow_caster_checks = spots.caster_checks + points.caster_checks;
            self.stats.local_shadow_maps_reused_without_scan =
                spots.reused_maps + points.reused_maps;
            spot_changes = spots.updates;
            point_changes = points.updates;
            self.shadows.spots.invalidate_changes(&spot_changes);
            self.shadows.points.invalidate_changes(&point_changes);
            self.stats.shadow_maps_rendered = usize::from(sun_changed)
                + spot_changes
                    .iter()
                    .chain(&point_changes)
                    .filter(|c| c.is_some())
                    .count();
            if sun_changed && scene.lighting.shadows && scene.lighting.sun_intensity > 0.
                || spot_changes
                    .iter()
                    .chain(&point_changes)
                    .any(Option::is_some)
            {
                if self.instancing.shadow_batching() {
                    shadow_batches = Some(self.prepare_shadow_instances(gpu, &draws)?);
                }
                if shadow_batches
                    .as_ref()
                    .unwrap_or(&batches)
                    .iter()
                    .any(|b| b.slot.is_some())
                {
                    self.prepare_instanced_shadows(gpu);
                }
            } else {
                self.instancing.shadow_bindings.truncate(8);
            }
            if sun_changed
                && uniform_unchanged
                && self.shadow_preparation_cache
                && self.state_caching
                && let Some(previous) = &previous_frame
            {
                let state_started = std::time::Instant::now();
                let mask = if reuse_metadata {
                    std::mem::take(&mut comparison.stable_mask)
                } else {
                    previous.stable_casters(&draws, self.culling)
                };
                self.stats.shadow_state_ms += state_started.elapsed().as_secs_f64() * 1000.;
                let geometry_work = draws
                    .iter()
                    .enumerate()
                    .filter(|(_, draw)| !draw.transparent && draw.object.material.lit)
                    .fold((0_u64, 0_u64), |(s, d), (index, draw)| {
                        let triangles = u64::from(self.mesh_for(&draw.object).count / 3);
                        if mask[index] {
                            (s + triangles, d)
                        } else {
                            (s, d + triangles)
                        }
                    });
                sun_plan = self.shadows.sun_cache.prepare(
                    &gpu.device,
                    &draws,
                    shadow_batches.as_ref().unwrap_or(&batches),
                    mask,
                    &self.shadows.uniform_row,
                    self.shadows.resolution,
                    geometry_work,
                );
                if let Some(plan) = &sun_plan {
                    self.stats.sun_static_cache_reused = !plan.rebuild;
                    self.stats.sun_static_casters = plan.static_mask.iter().filter(|m| **m).count();
                    self.stats.sun_dynamic_casters =
                        plan.dynamic_mask.iter().filter(|m| **m).count();
                    self.stats.sun_depth_copies = 1;
                    self.stats.shadow_maps_rendered += usize::from(plan.rebuild);
                }
            }
            self.stats.shadow_cache_hit = self.stats.shadow_maps_rendered == 0;
            if !reuse_metadata {
                let state_started = std::time::Instant::now();
                drop(previous_frame.take());
                self.stats.shadow_state_ms += state_started.elapsed().as_secs_f64() * 1000.;
            }
        }
        self.stats.sun_bounds_cache_bytes = self.shadows.sun_fit.bytes();
        // Packed color and shadow groups already contain their uniforms.
        // Upload individual buffers for singletons and ineligible casters.
        let sun_individuals = !self.stats.shadow_cache_hit
            && sun_changed
            && scene.lighting.shadows
            && scene.lighting.sun_intensity > 0.;
        let local_individuals = spot_changes
            .iter()
            .chain(&point_changes)
            .any(Option::is_some);
        let mut individual = std::mem::take(&mut self.frame_scratch.individual);
        individual.clear();
        individual.resize(draws.len(), false);
        let mut shadow_individual = std::mem::take(&mut self.frame_scratch.shadow);
        shadow_individual.clear();
        shadow_individual.resize(draws.len(), false);
        for batch in &batches {
            if batch.slot.is_none() {
                individual[batch.indices[0]] = true;
            } else if !self.instancing.shadow_batching()
                && sun_individuals
                && !batch
                    .indices
                    .iter()
                    .all(|&index| !draws[index].transparent && draws[index].object.material.lit)
            {
                for &index in &batch.indices {
                    shadow_individual[index] |=
                        !draws[index].transparent && draws[index].object.material.lit;
                }
            }
        }
        if let Some(batches) = &shadow_batches {
            for batch in batches.iter().filter(|b| b.slot.is_none()) {
                for &index in &batch.indices {
                    shadow_individual[index] = true;
                }
            }
        }
        for (index, draw) in draws.iter().enumerate() {
            shadow_individual[index] |= !draw.transparent
                && draw.object.material.lit
                && !self.instancing.shadow_batching()
                && (local_individuals
                    || sun_individuals && (!visible[index] || sun_plan.is_some()));
        }
        self.prepare_shadow_singletons(gpu, &draws, &shadow_individual)?;
        self.stats.shadow_singleton_bytes = self.shadows.singletons.write_bytes;
        self.stats.shadow_singleton_allocations = self.shadows.singletons.allocations;
        self.stats.local_shadow_receiver_bytes =
            self.shadows.spots.receiver_write_bytes + self.shadows.points.receiver_write_bytes;
        self.stats.local_shadow_receiver_writes =
            self.shadows.spots.receiver_writes + self.shadows.points.receiver_writes;
        for (index, &needed) in individual.iter().enumerate() {
            if needed && self.objects[index].resources.is_none() {
                let resources = self.object_resources(
                    gpu,
                    &self.objects[index].texture,
                    !self.objects[index].numeric_parameters.is_empty(),
                )?;
                self.objects[index].resources = Some(resources);
                self.objects[index].dirty = true;
                self.objects[index].parameter_dirty = true;
                self.stats.object_buffer_allocations += 1;
            }
            let binding = &mut self.objects[index];
            if needed && binding.parameter_dirty {
                if let Some(buffer) = binding
                    .resources
                    .as_ref()
                    .and_then(|r| r.parameters.as_ref())
                {
                    let bytes = graph_parameter_bytes(&binding.numeric_parameters);
                    gpu.queue.write_buffer(buffer, 0, &bytes);
                    self.stats.graph_parameter_bytes += bytes.len();
                }
                binding.parameter_dirty = false;
            }
            if needed && binding.dirty {
                gpu.queue.write_buffer(
                    &binding.resources.as_ref().unwrap().buffer,
                    0,
                    binding.uniform.as_ref().unwrap(),
                );
                binding.dirty = false;
                self.stats.object_uniform_writes += 1;
            }
        }
        let has_particles = !raw && !scene.particles.is_empty();
        let transparent: Vec<usize> = if has_particles {
            draws
                .iter()
                .zip(&visible)
                .enumerate()
                .filter(|(_, (d, v))| d.transparent && **v)
                .map(|(i, _)| i)
                .collect()
        } else {
            Vec::new()
        };
        if has_particles {
            self.stats.particles = scene.particles.len();
            self.stats.particle_triangles = scene.particles.len() as u64 * 2;
            let particles = self.particles.get_or_insert_with(|| {
                particles::Particles::new(
                    gpu,
                    &self.shadows.sample_layout,
                    &self.depth.as_ref().unwrap().view,
                    size,
                )
            });
            particles.prepare(
                gpu,
                scene,
                view_projection,
                &self.depth.as_ref().unwrap().view,
                size,
                &transparent
                    .iter()
                    .map(|i| draws[*i].depth)
                    .collect::<Vec<_>>(),
            )?;
            (
                self.stats.particle_compute_dispatches,
                self.stats.particle_descriptor_bytes,
            ) = particles.work();
        }
        self.stats.prepare_ms = started.elapsed().as_secs_f64() * 1000.;
        let encode_started = std::time::Instant::now();
        if has_particles {
            self.particles.as_ref().unwrap().encode(&mut encoder);
        }
        if !self.stats.shadow_cache_hit {
            let batches = shadow_batches.as_ref().unwrap_or(&batches);
            if sun_changed {
                (self.stats.shadow_draws, self.stats.shadow_triangles) =
                    self.draw_shadows(gpu, &mut encoder, scene, &draws, batches, sun_plan.as_ref());
            }
            let (spot_draws, spot_triangles) =
                self.shadows
                    .spots
                    .draw(self, &mut encoder, &draws, batches, false, &spot_changes);
            self.stats.shadow_draws += spot_draws;
            self.stats.shadow_triangles += spot_triangles;
            let (point_draws, point_triangles) =
                self.shadows
                    .points
                    .draw(self, &mut encoder, &draws, batches, true, &point_changes);
            self.stats.shadow_draws += point_draws;
            self.stats.shadow_triangles += point_triangles;
        }
        self.stats.auxiliary_targets = output_mask.count_ones() as usize;
        self.stats.shadow_range_draws_saved = self.shadows.spots.range_draws_saved.get()
            + self.shadows.points.range_draws_saved.get()
            + self.shadows.sun_cache.range_draws_saved.get();
        self.stats.shadow_range_bytes = self.shadows.spots.range_write_bytes.get()
            + self.shadows.points.range_write_bytes.get()
            + self.shadows.sun_cache.range_write_bytes.get();
        self.stats.local_static_depth_copies = self.shadows.spots.static_depth_copies.get()
            + self.shadows.points.static_depth_copies.get();
        self.stats.local_static_triangles_skipped =
            self.shadows.spots.static_triangles_skipped.get()
                + self.shadows.points.static_triangles_skipped.get();
        self.stats.geometry_allocated_bytes = if self.geometry.is_some() {
            24 * u64::from(size[0]) * u64::from(size[1])
        } else {
            0
        };
        self.stats.geometry_store_bytes = stores.iter().filter(|s| **s).count() as u64
            * 8
            * u64::from(size[0])
            * u64::from(size[1]);
        let mut pass_stats = FrameStats::default();
        let mut submission = std::mem::take(&mut self.submission);
        let submission_candidate = !has_particles
            && occlusion != occlusion::Mode::Indirect
            && submission.candidate(batches.len(), self.instancing.arena_enabled());
        {
            let records: Vec<_> = if submission_candidate {
                batches
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        occlusion != occlusion::Mode::Cached || self.occlusion.batch_visible(*index)
                    })
                    .map(|(_, batch)| {
                        let binding = match batch.slot {
                            Some(slot) => &self.instancing.bindings[slot].binding,
                            None => self.objects[batch.indices[0]].binding(),
                        };
                        self.prepared_record(
                            &draws[batch.indices[0]],
                            binding,
                            output_mask,
                            batch.indices.len() as u32,
                            batch.first_instance,
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            submission.prepare(
                gpu,
                &records,
                output_mask,
                submission_candidate,
                self.instancing.arena_enabled(),
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene opaque pass"),
                color_attachments: &[
                    geometry::attachment(
                        self.display.hdr(),
                        wgpu::Color {
                            r: 0.018,
                            g: 0.025,
                            b: 0.04,
                            a: 1.,
                        },
                    ),
                    (output_mask & 1 != 0)
                        .then(|| {
                            geometry::auxiliary_attachment(
                                &self.geometry.as_ref().unwrap().normal,
                                stores[0],
                            )
                        })
                        .flatten(),
                    (output_mask & 2 != 0)
                        .then(|| {
                            geometry::auxiliary_attachment(
                                &self.geometry.as_ref().unwrap().motion,
                                stores[1],
                            )
                        })
                        .flatten(),
                    (output_mask & 4 != 0)
                        .then(|| {
                            geometry::auxiliary_attachment(
                                &self.geometry.as_ref().unwrap().specular,
                                stores[2],
                            )
                        })
                        .flatten(),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.as_ref().unwrap().view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            self.environment
                .background(&mut pass, scene.environment, output_mask);
            let mut last_pipeline = draw_state::DrawState::default();
            if submission_candidate {
                let work =
                    submission.draw(&mut pass, &records, &mut last_pipeline, self.state_caching);
                pass_stats.render_bundle_compilations = work.bundle_compilations;
                pass_stats.render_bundle_replays = work.bundle_replays;
                pass_stats.multi_draw_indirect_runs = work.indirect_runs;
                pass_stats.multi_draw_indirect_draws = work.indirect_draws;
                pass_stats.multi_draw_indirect_bytes = work.indirect_bytes;
                for record in &records {
                    pass_stats.color_draws += 1;
                    pass_stats.color_triangles += record.triangles();
                    pass_stats.instanced_draws += usize::from(record.instances > 1);
                    pass_stats.instanced_surfaces += if record.instances > 1 {
                        record.instances as usize
                    } else {
                        0
                    };
                }
                pass_stats.pipeline_binds += last_pipeline.counts.pipelines;
            } else {
                for (batch_index, batch) in batches.iter().enumerate() {
                    if occlusion == occlusion::Mode::Cached
                        && !self.occlusion.batch_visible(batch_index)
                    {
                        continue;
                    }
                    let draw = &draws[batch.indices[0]];
                    if has_particles && draw.transparent {
                        continue;
                    }
                    let binding = match batch.slot {
                        Some(slot) => &self.instancing.bindings[slot].binding,
                        None => self.objects[batch.indices[0]].binding(),
                    };
                    let count = batch.indices.len() as u32;
                    let (triangles, binds) = self.draw_prepared(
                        &mut pass,
                        draw,
                        binding,
                        output_mask,
                        DrawCall {
                            instances: count,
                            first_instance: batch.first_instance,
                            indirect: (occlusion == occlusion::Mode::Indirect)
                                .then(|| (self.occlusion.arguments(), batch_index as u64 * 20)),
                        },
                        &mut last_pipeline,
                    );
                    pass_stats.color_draws += 1;
                    pass_stats.instanced_draws += usize::from(count > 1);
                    pass_stats.instanced_surfaces += if count > 1 { count as usize } else { 0 };
                    pass_stats.color_triangles += triangles;
                    pass_stats.pipeline_binds += binds;
                }
            }
            pass_stats.material_binds = last_pipeline.counts.groups;
            pass_stats.vertex_binds = last_pipeline.counts.vertices;
            pass_stats.index_binds = last_pipeline.counts.indices;
        }
        self.submission = submission;
        self.stats.color_draws += pass_stats.color_draws;
        self.stats.color_triangles += pass_stats.color_triangles;
        self.stats.instanced_draws += pass_stats.instanced_draws;
        self.stats.instanced_surfaces += pass_stats.instanced_surfaces;
        self.stats.pipeline_binds += pass_stats.pipeline_binds;
        self.stats.material_binds += pass_stats.material_binds;
        self.stats.vertex_binds += pass_stats.vertex_binds;
        self.stats.index_binds += pass_stats.index_binds;
        self.stats.render_bundle_compilations = pass_stats.render_bundle_compilations;
        self.stats.render_bundle_replays = pass_stats.render_bundle_replays;
        self.stats.multi_draw_indirect_runs = pass_stats.multi_draw_indirect_runs;
        self.stats.multi_draw_indirect_draws = pass_stats.multi_draw_indirect_draws;
        self.stats.multi_draw_indirect_bytes = pass_stats.multi_draw_indirect_bytes;
        if has_particles {
            let load = |view, store| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: if store {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    },
                })
            };
            let geometry = self.geometry.as_ref().unwrap();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("transparent surfaces and sorted particles"),
                color_attachments: &[
                    load(self.display.hdr(), true),
                    load(&geometry.normal, stores[0]),
                    load(&geometry.motion, stores[1]),
                    load(&geometry.specular, stores[2]),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.as_ref().unwrap().view,
                    depth_ops: None,
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            let particles = self.particles.as_ref().unwrap();
            for (bucket, &index) in transparent.iter().enumerate() {
                particles.draw_bucket(&mut pass, &self.shadows.sample_binding, bucket as u32);
                let (triangles, binds) = self.draw_prepared(
                    &mut pass,
                    &draws[index],
                    self.objects[index].binding(),
                    7,
                    DrawCall {
                        instances: 1,
                        first_instance: 0,
                        indirect: None,
                    },
                    &mut draw_state::DrawState::default(),
                );
                self.stats.color_draws += 1;
                self.stats.color_triangles += triangles;
                self.stats.pipeline_binds += binds;
            }
            particles.draw_bucket(
                &mut pass,
                &self.shadows.sample_binding,
                transparent.len() as u32,
            );
        }
        self.display.draw(
            &mut encoder,
            target,
            Some(&self.shadows.sample_binding),
            Some(&self.environment.binding),
        );
        if scene.items.iter().any(|i| match &i.mesh {
            MeshKind::Text(t) => t.screen.is_some(),
            MeshKind::SharedText(t) => t.screen.is_some(),
            MeshKind::Sprite(s) => s.screen.is_some(),
            _ => false,
        }) {
            let mut hud = self
                .hud
                .take()
                .unwrap_or_else(|| hud::HudRenderer::new(gpu, self.output_format));
            let result = hud.draw(
                gpu,
                &mut encoder,
                target,
                size,
                scene,
                self,
                raw,
                self.hud_scale,
            );
            self.stats.hud_draws = hud.draw_calls;
            self.stats.hud_uniform_bytes = hud.uniform_bytes;
            self.stats.hud_geometry_copies = hud.geometry_copies;
            self.hud = Some(hud);
            result?;
        } else {
            self.hud = None;
        }
        self.stats.encode_ms = encode_started.elapsed().as_secs_f64() * 1000.;
        let submit_started = std::time::Instant::now();
        self.profiler.submit(gpu, encoder);
        self.occlusion.submitted();
        if has_particles {
            self.particles.as_mut().unwrap().submitted();
        }
        self.stats.submit_ms = submit_started.elapsed().as_secs_f64() * 1000.;
        self.shadows.spots.finish(spot_changes);
        self.shadows.points.finish(point_changes);
        if let Some(plan) = &sun_plan {
            self.shadows
                .sun_cache
                .finish(plan, &draws, &self.shadows.uniform_row);
        } else {
            self.shadows.sun_cache.age_unused();
        }
        if self.stats.shadow_cache_hit {
            self.shadows.spots.age_unused_static_depth();
            self.shadows.points.age_unused_static_depth();
        }
        let state_started = std::time::Instant::now();
        if reuse_metadata {
            if let Some(mut frame) = previous_frame.or_else(|| self.shadow_frame.take()) {
                frame.refresh(
                    scene,
                    &draws,
                    self.culling,
                    &comparison.unchanged,
                    &mut self.stats,
                );
                self.shadow_frame = Some(frame);
            } else {
                self.stats.shadow_metadata_built_casters =
                    draws.iter().filter(|d| d.object.material.lit).count();
                self.stats.shadow_metadata_key_clones =
                    self.stats.shadow_metadata_built_casters * 2;
                self.shadow_frame = Some(shadows::ShadowFrame::new(scene, &draws, self.culling));
            }
        } else {
            self.shadow_frame = shadow_frame.take();
        }
        self.stats.shadow_state_ms += state_started.elapsed().as_secs_f64() * 1000.;
        self.motion_history.finish(&draws);
        self.instancing.frame_batches = batches;
        if let Some(batches) = shadow_batches {
            self.instancing.shadow_frame_batches = batches;
        }
        if self.state_caching && self.surface_preparation_caching {
            self.surface_preparation.draws = draws;
        }
        self.frame_scratch = frame_scratch::Scratch {
            bounds,
            visible,
            frustum: frustum_visible,
            items: visible_items,
            individual,
            shadow: shadow_individual,
        };
        self.frame_scratch.compact();
        self.stats.frame_scratch_bytes = self.frame_scratch.bytes();
        self.stats.cpu_ms = started.elapsed().as_secs_f64() * 1000.;
        Ok(())
    }
}

impl SceneRenderer {
    /// Capture GPU passes when the device supports timestamps. Readback never blocks rendering.
    pub fn set_profiling_enabled(&mut self, enabled: bool) {
        self.profiler.enabled = enabled;
    }
    pub fn poll_gpu_profiles(&mut self, gpu: &Gpu) -> Result<Vec<crate::GpuFrameTiming>> {
        self.profiler.poll(gpu)
    }
    pub fn skipped_gpu_profiles(&self) -> u64 {
        self.profiler.skipped
    }
    /// Discard eye-adaptation history on a camera cut, scene change, or independent capture.
    /// The next enabled auto-exposure frame starts from its current metered target.
    pub fn reset_display_history(&mut self) {
        self.display.reset_history();
        self.motion_history.reset();
    }
}
