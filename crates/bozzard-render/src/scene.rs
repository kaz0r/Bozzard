use crate::Gpu;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;
mod temporal_settings;
pub use temporal_settings::{MotionBlur, ScreenSpaceReflections, TemporalAntiAliasing};
pub(crate) mod geometry;
mod particles;
mod reflections;
mod temporal;
pub use particles::{Particle, ParticleKind};
mod hud;
mod text;
pub use text::{ScreenText, TextAlignment, TextMesh, text_bounds};
mod visibility;
pub use visibility::FrameStats;
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
mod upload;
pub use lighting::Lighting;
pub use upload::{PendingUpload, UploadContext, UploadData, UploadProgress, UploadSource};
type ImageCache = BTreeMap<(usize, u32, u32, bool), (wgpu::TextureView, bool)>;
const OBJECT_UNIFORM_BYTES: usize = 496;

#[derive(Clone, Debug, PartialEq)]
pub enum MeshKind {
    Text(TextMesh),
    Quad,
    Cube,
    Sphere,
    Imported(String),
    ModelPart(String, usize),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextureKind {
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
    pub surface: String,
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
    pbr_override: [f32; 2],
    shader: Option<u64>,
    pbr: bool,
    object: DrawItem,
    opacity: f32,
    cutoff: f32,
    transparent: bool,
    depth: f32,
}

/// One shader graph's two host flavors, opaque and transparent each.
struct GraphPipelines {
    basic: [wgpu::RenderPipeline; 2],
    pbr: [wgpu::RenderPipeline; 2],
}
struct MeshBuffers {
    bounds: [Vec3; 2],
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    vertex_offset: u64,
}
struct ObjectBinding {
    buffer: wgpu::Buffer,
    texture: TextureKind,
    binding: wgpu::BindGroup,
    uniform: Option<[u8; OBJECT_UNIFORM_BYTES]>,
}
struct DepthTarget {
    view: wgpu::TextureView,
    size: [u32; 2],
}

/// Indexed geometry, per-object matrices/materials, sampled textures, and depth testing.
/// HDR opaque/transparent passes. Imported color images are sRGB; procedural colors are linear.
pub struct SceneRenderer {
    hud: Option<hud::HudRenderer>,
    output_format: wgpu::TextureFormat,
    hud_scale: f32,
    geometry: Option<geometry::GeometryBuffers>,
    motion_history: geometry::MotionHistory,
    particles: Option<particles::Particles>,
    text: Option<text::TextRenderer>,
    stats: FrameStats,
    culling: bool,
    state_caching: bool,
    environment: environment::Environment,
    display: display::Display,
    shadows: shadows::Shadows,
    shadow_frame: Option<shadows::ShadowFrame>,
    pipeline: [wgpu::RenderPipeline; 2],
    transparent_pipeline: [wgpu::RenderPipeline; 2],
    layout: wgpu::BindGroupLayout,
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
    depth: Option<DepthTarget>,
    imported_meshes: BTreeMap<String, MeshBuffers>,
    models: BTreeMap<String, Vec<UploadedPart>>,
    transparent_textures: BTreeSet<String>,
    imported_textures: BTreeMap<String, wgpu::TextureView>,
    model_upload_stats: BTreeMap<String, ModelUploadStats>,
}

pub(crate) fn float_bytes(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// Full WGSL for one host flavor: shared lighting includes plus the host fragment shader.
fn host_text(pbr: bool) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}",
        include_str!("scene/environment_sample.wgsl"),
        include_str!("scene/shadow_sample.wgsl"),
        include_str!("scene/local_lights.wgsl"),
        include_str!("scene/gi.wgsl"),
        include_str!("scene/effects.wgsl"),
        include_str!("scene/fog.wgsl"),
        if pbr {
            include_str!("pbr.wgsl")
        } else {
            include_str!("scene.wgsl")
        },
    )
}

/// Splice a graph's `graph_material_surface` over the host's stock call site.
/// Only fs_main calls it with fragment inputs, so the replacement is unique.
fn graph_module_text(pbr: bool, source: &ShaderSource) -> String {
    let host = host_text(pbr).replacen(
        "default_material_surface(in",
        "graph_material_surface(in",
        1,
    );
    format!("{host}\n{}", source.surface)
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
    let basic_buffers = [Some(wgpu::VertexBufferLayout {
        array_stride: 32,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
    })];
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
                usage: wgpu::BufferUsages::VERTEX,
            }),
        indices: gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("scene mesh indices"),
                contents: &indices
                    .iter()
                    .flat_map(|i| i.to_le_bytes())
                    .collect::<Vec<_>>(),
                usage: wgpu::BufferUsages::INDEX,
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
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("scene object layout"),
                entries: &entries,
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
            hud: None,
            output_format,
            hud_scale: 1.,
            geometry: None,
            motion_history: Default::default(),
            particles: None,
            text: None,
            stats: Default::default(),
            culling: true,
            state_caching: true,
            environment,
            display,
            shadows,
            shadow_frame: None,
            pbr,
            pipeline,
            transparent_pipeline,
            layout,
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
            model_upload_stats: BTreeMap::new(),
        }
    }

    /// Compile both host flavors for one shader graph. WGSL errors panic like the
    /// startup modules; graphs are validated before codegen, so errors are engine bugs.
    fn compile_graph(
        &self,
        gpu: &Gpu,
        source: &ShaderSource,
        auxiliary: bool,
    ) -> Result<GraphPipelines> {
        // The basic host leaves group 1 unused, so it keeps an automatic (empty)
        // layout; the PBR host binds the shared material map group.
        let layout = |group1: Option<&wgpu::BindGroupLayout>| {
            gpu.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("shader graph pipeline layout"),
                    bind_group_layouts: &[
                        Some(&self.layout),
                        group1,
                        Some(&self.shadows.sample_layout),
                        Some(&self.environment.layout),
                    ],
                    immediate_size: 0,
                })
        };
        let flavor = |pbr: bool| {
            let pipeline_layout = if pbr {
                layout(Some(self.pbr.material_layout()))
            } else {
                layout(None)
            };
            let module = gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("shader graph module"),
                    source: wgpu::ShaderSource::Wgsl(graph_module_text(pbr, source).into()),
                });
            let name = if pbr { "pbr" } else { "basic" };
            [
                scene_pipeline(
                    gpu,
                    &format!("shader graph {name} opaque"),
                    &pipeline_layout,
                    &module,
                    pbr,
                    false,
                    auxiliary,
                ),
                scene_pipeline(
                    gpu,
                    &format!("shader graph {name} transparent"),
                    &pipeline_layout,
                    &module,
                    pbr,
                    true,
                    auxiliary,
                ),
            ]
        };
        Ok(GraphPipelines {
            basic: flavor(false),
            pbr: flavor(true),
        })
    }

    fn object_binding(&self, gpu: &Gpu, key: &TextureKind) -> Result<ObjectBinding> {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene object uniform"),
            size: OBJECT_UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
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
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene object bindings"),
                layout: &self.layout,
                entries: &entries,
            })
        };
        let texture = match key {
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
        };
        let binding = bind(texture);
        Ok(ObjectBinding {
            buffer,
            texture: key.clone(),
            binding,
            uniform: None,
        })
    }

    fn invalidate_object_bindings(&mut self) {
        self.objects.clear();
        self.shadow_frame = None;
        self.shadows.spots.invalidate();
        self.shadows.points.invalidate();
    }

    pub fn clear_imported(&mut self) {
        self.model_upload_stats.clear();
        self.invalidate_object_bindings();
        self.imported_meshes.clear();
        self.imported_textures.clear();
        self.models.clear();
        self.transparent_textures.clear();
    }

    /// Retire one catalog entry without invalidating unrelated GPU resources.
    pub fn remove_asset(&mut self, id: &str) {
        self.imported_meshes.remove(id);
        self.models.remove(id);
        self.imported_textures.remove(id);
        self.transparent_textures.remove(id);
        self.model_upload_stats.remove(id);
        self.invalidate_object_bindings();
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
                            usage: wgpu::BufferUsages::INDEX,
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
    fn prepare(&self, scene: &RenderScene) -> Vec<PreparedDraw> {
        let mut draws = Vec::new();
        let mut add = |object: DrawItem,
                       opacity: f32,
                       cutoff: Option<f32>,
                       translucent: bool,
                       center: Vec3,
                       pbr_override: [f32; 2],
                       pbr: bool| {
            let depth = scene
                .view_projection
                .project_point3(object.model.transform_point3(center))
                .z;
            draws.push(PreparedDraw {
                pbr_override,
                pbr,
                shader: object.material.shader.as_ref().map(|s| s.id),
                object,
                opacity,
                cutoff: cutoff.unwrap_or(0.0),
                transparent: cutoff.is_none() && translucent,
                depth,
            });
        };
        for object in &scene.items {
            if let MeshKind::Text(text) = &object.mesh {
                if text.screen.is_some() {
                    continue;
                }
                if let Some(mesh) = self.text.as_ref().and_then(|t| t.mesh(text)) {
                    add(
                        object.clone(),
                        text.opacity,
                        None,
                        true,
                        (mesh.bounds[0] + mesh.bounds[1]) * 0.5,
                        [-1.; 2],
                        false,
                    );
                }
                continue;
            }
            if let MeshKind::Imported(id) | MeshKind::ModelPart(id, _) = &object.mesh
                && let Some(parts) = self.models.get(id)
            {
                let overrides: BTreeMap<_, _> = object
                    .material
                    .surface_overrides
                    .iter()
                    .map(|v| (v.surface as usize, v))
                    .collect();
                let (start, count) = match &object.mesh {
                    MeshKind::ModelPart(_, index) => (*index, 1),
                    _ => (0, parts.len()),
                };
                // Expanded surface entities already identify their part. Slice iterators
                // skip directly to it instead of scanning every sibling for every entity.
                for (index, part) in parts.iter().enumerate().skip(start).take(count) {
                    let mut item = object.clone();
                    if matches!(object.mesh, MeshKind::ModelPart(..)) {
                        item.model *= Mat4::from_translation(-part.center);
                    }
                    item.mesh = MeshKind::ModelPart(id.clone(), index);
                    for (tint, color) in item.material.tint.iter_mut().zip(part.color) {
                        *tint *= color;
                    }
                    let override_value = overrides
                        .get(&index)
                        .filter(|value| value.source == part.source_key);
                    if let Some(value) = override_value {
                        item.model *= Mat4::from_translation(part.center)
                            * value.transform
                            * Mat4::from_translation(-part.center);
                        for (uv, scale) in item.material.uv_scale.iter_mut().zip(value.uv_scale) {
                            *uv *= scale;
                        }
                        if let Some(texture) = &value.texture {
                            item.material.texture = texture.clone();
                        }
                    }
                    let translucent = if item.material.texture == TextureKind::White
                        && override_value.is_none_or(|v| v.texture.is_none())
                    {
                        if part.texture.is_some() {
                            item.material.texture = TextureKind::ModelPart(id.clone(), index);
                        }
                        part.translucent
                    } else {
                        part.color[3] < 1.0
                            || matches!(&item.material.texture, TextureKind::Imported(id) if self.transparent_textures.contains(id))
                    };
                    if let Some(value) = override_value {
                        for (tint, multiplier) in item.material.tint.iter_mut().zip(value.tint) {
                            *tint *= multiplier;
                        }
                    }
                    let factors = [
                        override_value
                            .and_then(|v| v.metallic)
                            .or(item.material.metallic)
                            .unwrap_or(-1.),
                        override_value
                            .and_then(|v| v.roughness)
                            .or(item.material.roughness)
                            .unwrap_or(-1.),
                    ];
                    add(
                        item,
                        part.color[3],
                        part.cutoff,
                        translucent,
                        part.center,
                        factors,
                        part.shading.is_some(),
                    );
                }
            } else {
                let transparent = matches!(&object.material.texture, TextureKind::Imported(id) if self.transparent_textures.contains(id));
                add(
                    object.clone(),
                    1.0,
                    None,
                    transparent,
                    Vec3::ZERO,
                    [
                        object.material.metallic.unwrap_or(-1.),
                        object.material.roughness.unwrap_or(-1.),
                    ],
                    false,
                );
            }
        }
        // Opaque first, grouped by shader pipeline; translucent surfaces back-to-front by projected center.
        draws.sort_by(|a, b| {
            a.transparent.cmp(&b.transparent).then_with(|| {
                if a.transparent {
                    b.depth.total_cmp(&a.depth)
                } else {
                    a.shader.cmp(&b.shader).then_with(|| a.pbr.cmp(&b.pbr))
                }
            })
        });
        draws
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
        let started = std::time::Instant::now();
        self.stats = FrameStats::default();
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
        let (view_projection, temporal_frame) = self.motion_history.begin(scene, size, raw);
        self.environment.prepare(
            gpu,
            scene.environment,
            view_projection.inverse(),
            !raw && scene.display.reflections.enabled,
        )?;
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
        let draws = self.prepare(scene);
        // Keep all active pipelines and a bounded set of recently absent previews.
        let mut graph_sources: BTreeMap<(u64, bool), std::sync::Arc<ShaderSource>> =
            BTreeMap::new();
        for draw in &draws {
            if let Some(shader) = &draw.object.material.shader {
                graph_sources.insert((shader.id, auxiliary), shader.clone());
            }
        }
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
        for (id, source) in graph_sources {
            if !self.graphs.contains_key(&id) {
                let pipelines = self.compile_graph(gpu, &source, auxiliary)?;
                self.graphs.insert(id, std::sync::Arc::new(pipelines));
                self.stats.graph_compilations += 1;
            }
        }
        self.stats.resident_graphs = self.graphs.len();
        self.objects.truncate(draws.len());
        for (index, draw) in draws.iter().enumerate() {
            let object = &draw.object;
            if index == self.objects.len() {
                self.objects
                    .push(self.object_binding(gpu, &object.material.texture)?);
            } else if self.objects[index].texture != object.material.texture {
                self.objects[index] = self.object_binding(gpu, &object.material.texture)?;
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
        let visible = self.visibility(scene, &draws);
        self.stats.scene_items = scene.items.len();
        self.stats.surfaces = draws.len();
        self.stats.visible_surfaces = visible.iter().filter(|v| **v).count();
        self.stats.culled_surfaces = draws.len() - self.stats.visible_surfaces;
        let inverse_view_projection = view_projection.inverse().to_cols_array();
        let lighting_uniform = scene.lighting.uniform();
        let fog_uniform = scene.fog.uniform(raw);
        for (draw, binding) in draws.iter().zip(&mut self.objects) {
            let object = &draw.object;
            let mvp = view_projection * object.model;
            let previous_model = self.motion_history.previous_model(object);
            let previous_mvp = temporal_frame.previous_vp * previous_model.unwrap_or(object.model);
            let normal = object.model.inverse().transpose();
            ensure!(
                mvp.is_finite() && normal.is_finite(),
                "invalid object matrix"
            );
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
            let values = mvp
                .to_cols_array()
                .into_iter()
                .chain(normal.to_cols_array())
                .chain(tail)
                .chain(object.model.to_cols_array())
                .chain(inverse_view_projection)
                .chain([
                    size[0] as f32,
                    size[1] as f32,
                    object.model.determinant().signum(),
                    if double_sided { 1. } else { 0. },
                ])
                .chain(lighting_uniform)
                .chain([
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
                ])
                .chain(fog_uniform)
                .chain(previous_mvp.to_cols_array())
                .chain([
                    if material.shader.is_some() {
                        scene.shader_time
                    } else {
                        0.
                    },
                    0.,
                    0.,
                    0.,
                ]);
            let mut uniform = [0; OBJECT_UNIFORM_BYTES];
            debug_assert_eq!(values.clone().count() * 4, uniform.len());
            for (slot, value) in uniform.chunks_exact_mut(4).zip(values) {
                slot.copy_from_slice(&value.to_le_bytes());
            }
            if !self.state_caching || binding.uniform.as_ref() != Some(&uniform) {
                gpu.queue.write_buffer(&binding.buffer, 0, &uniform);
                binding.uniform = Some(uniform);
                self.stats.object_uniform_writes += 1;
            }
        }
        let shadow_frame = shadows::ShadowFrame::new(scene, &draws, self.culling);
        self.stats.shadow_cache_hit =
            self.state_caching && self.shadow_frame.as_ref() == Some(&shadow_frame);
        let sun_changed = !self.state_caching
            || !self
                .shadow_frame
                .as_ref()
                .is_some_and(|previous| previous.same_sun(&shadow_frame));
        let mut spot_changes = Vec::new();
        let mut point_changes = Vec::new();
        if !self.stats.shadow_cache_hit {
            // Invalidate before queueing writes: a later frame error must not leave a
            // valid-looking stamp paired with partially updated shadow uniforms.
            self.shadow_frame = None;
            if sun_changed {
                self.update_shadows(gpu, scene, &draws)?;
            }
            self.update_spot_shadows(gpu, scene)?;
            self.update_point_shadows(gpu, scene)?;
            spot_changes = self.shadows.spots.changes(self, &draws);
            point_changes = self.shadows.points.changes(self, &draws);
            self.shadows.spots.invalidate_changes(&spot_changes);
            self.shadows.points.invalidate_changes(&point_changes);
            self.stats.shadow_maps_rendered = usize::from(sun_changed)
                + spot_changes
                    .iter()
                    .chain(&point_changes)
                    .filter(|c| c.is_some())
                    .count();
        }
        if !raw && !scene.particles.is_empty() {
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
            )?;
        }
        self.stats.prepare_ms = started.elapsed().as_secs_f64() * 1000.;
        let encode_started = std::time::Instant::now();
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scene frame"),
            });
        if !self.stats.shadow_cache_hit {
            if sun_changed {
                (self.stats.shadow_draws, self.stats.shadow_triangles) =
                    self.draw_shadows(&mut encoder, scene, &draws);
            }
            let (spot_draws, spot_triangles) = self.shadows.spots.draw(
                self,
                &mut encoder,
                &draws,
                &self.shadows.pipeline,
                &spot_changes,
            );
            self.stats.shadow_draws += spot_draws;
            self.stats.shadow_triangles += spot_triangles;
            let (point_draws, point_triangles) = self.shadows.points.draw(
                self,
                &mut encoder,
                &draws,
                &self.shadows.point_pipeline,
                &point_changes,
            );
            self.stats.shadow_draws += point_draws;
            self.stats.shadow_triangles += point_triangles;
        }
        self.stats.auxiliary_targets = if auxiliary { 3 } else { 0 };
        self.stats.geometry_allocated_bytes = if self.geometry.is_some() {
            24 * u64::from(size[0]) * u64::from(size[1])
        } else {
            0
        };
        self.stats.geometry_store_bytes = stores.iter().filter(|s| **s).count() as u64
            * 8
            * u64::from(size[0])
            * u64::from(size[1]);
        {
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
                    auxiliary
                        .then(|| {
                            geometry::auxiliary_attachment(
                                &self.geometry.as_ref().unwrap().normal,
                                stores[0],
                            )
                        })
                        .flatten(),
                    auxiliary
                        .then(|| {
                            geometry::auxiliary_attachment(
                                &self.geometry.as_ref().unwrap().motion,
                                stores[1],
                            )
                        })
                        .flatten(),
                    auxiliary
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
                .background(&mut pass, scene.environment, auxiliary);
            let mut last_pipeline = None;
            for ((draw, binding), visible) in draws.iter().zip(&self.objects).zip(&visible) {
                if !visible {
                    continue;
                }
                let object = &draw.object;
                let shading = match &object.mesh {
                    MeshKind::ModelPart(id, index) => self.models[id][*index].shading.as_ref(),
                    _ => None,
                };
                let key = (shading.is_some(), draw.shader, draw.transparent);
                let pipeline = match key {
                    (true, Some(id), false) => &self.graphs[&(id, auxiliary)].pbr[0],
                    (true, Some(id), true) => &self.graphs[&(id, auxiliary)].pbr[1],
                    (false, Some(id), false) => &self.graphs[&(id, auxiliary)].basic[0],
                    (false, Some(id), true) => &self.graphs[&(id, auxiliary)].basic[1],
                    (true, None, false) => &self.pbr.opaque[usize::from(auxiliary)],
                    (true, None, true) => &self.pbr.transparent[usize::from(auxiliary)],
                    (false, None, false) => &self.pipeline[usize::from(auxiliary)],
                    (false, None, true) => &self.transparent_pipeline[usize::from(auxiliary)],
                };
                if !self.state_caching || last_pipeline != Some(key) {
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(2, &self.shadows.sample_binding, &[]);
                    pass.set_bind_group(3, &self.environment.binding, &[]);
                    self.stats.pipeline_binds += 1;
                    last_pipeline = Some(key);
                }
                let mesh = match &object.mesh {
                    MeshKind::Text(text) => self.text.as_ref().unwrap().mesh(text).unwrap(),
                    MeshKind::Quad => &self.quad,
                    MeshKind::Cube => &self.cube,
                    MeshKind::Sphere => &self.sphere,
                    MeshKind::Imported(id) => &self.imported_meshes[id],
                    MeshKind::ModelPart(id, index) => &self.models[id][*index].mesh,
                };
                pass.set_bind_group(0, &binding.binding, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(mesh.vertex_offset..));
                if let Some(shading) = shading {
                    pass.set_bind_group(1, &shading.binding, &[]);
                    pass.set_vertex_buffer(1, shading.vertices.slice(..));
                }
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.count, 0, 0..1);
                self.stats.color_triangles += u64::from(mesh.count / 3);
            }
        }
        if !raw && !scene.particles.is_empty() {
            self.particles.as_ref().unwrap().draw(
                &mut encoder,
                self.display.hdr(),
                &self.geometry.as_ref().unwrap().motion,
                &self.shadows.sample_binding,
            );
        }
        self.display.draw(
            &mut encoder,
            target,
            Some(&self.shadows.sample_binding),
            Some(&self.environment.binding),
        );
        if scene
            .items
            .iter()
            .any(|i| matches!(&i.mesh, MeshKind::Text(t) if t.screen.is_some()))
        {
            let hud = self
                .hud
                .get_or_insert_with(|| hud::HudRenderer::new(gpu, self.output_format));
            hud.draw(
                gpu,
                &mut encoder,
                target,
                size,
                scene,
                self.text.as_ref().unwrap(),
                raw,
                self.hud_scale,
            )?;
        } else {
            self.hud = None;
        }
        let commands = encoder.finish();
        self.stats.encode_ms = encode_started.elapsed().as_secs_f64() * 1000.;
        let submit_started = std::time::Instant::now();
        gpu.queue.submit([commands]);
        self.stats.submit_ms = submit_started.elapsed().as_secs_f64() * 1000.;
        self.shadows.spots.finish(spot_changes);
        self.shadows.points.finish(point_changes);
        self.shadow_frame = Some(shadow_frame);
        self.motion_history.finish(&draws);
        self.stats.cpu_ms = started.elapsed().as_secs_f64() * 1000.;
        Ok(())
    }
}

impl SceneRenderer {
    /// Discard eye-adaptation history on a camera cut, scene change, or independent capture.
    /// The next enabled auto-exposure frame starts from its current metered target.
    pub fn reset_display_history(&mut self) {
        self.display.reset_history();
        self.motion_history.reset();
    }
}
