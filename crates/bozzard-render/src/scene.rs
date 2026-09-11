use crate::Gpu;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;
mod visibility;
pub use visibility::FrameStats;
mod environment;
pub use environment::EnvironmentSettings;
mod bloom;
pub use bloom::BloomSettings;
mod display;
pub use display::DisplaySettings;
mod gi;
pub use gi::IrradianceVolume;
mod lighting;
mod local_lights;
pub use local_lights::{LocalLight, MAX_LOCAL_LIGHTS};
mod shadows;
mod upload;
pub use lighting::Lighting;
pub use upload::{PendingUpload, UploadContext, UploadData, UploadProgress, UploadSource};
type ImageCache = BTreeMap<(usize, u32, u32, bool), (wgpu::TextureView, bool)>;

#[derive(Clone, Debug)]
pub enum MeshKind {
    Quad,
    Cube,
    Imported(String),
    ModelPart(String, usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureKind {
    White,
    Checker,
    Imported(String),
    ModelPart(String, usize),
}

#[derive(Clone, Debug)]
pub struct Material {
    pub surface_overrides: std::sync::Arc<[SurfaceMaterialOverride]>,
    pub tint: [f32; 3],
    pub uv_scale: [f32; 2],
    pub texture: TextureKind,
    pub lit: bool,
}

#[derive(Clone, Debug)]
pub struct SurfaceMaterialOverride {
    pub surface: u32,
    pub source: String,
    pub tint: [f32; 3],
    pub metallic: Option<f32>,
    pub roughness: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct DrawItem {
    pub model: Mat4,
    pub mesh: MeshKind,
    pub material: Material,
}

/// Render data only: does not borrow an ECS world or know about scene serialization.
#[derive(Clone, Debug)]
pub struct RenderScene {
    pub gi: Option<IrradianceVolume>,
    pub lights: Vec<LocalLight>,
    pub environment: EnvironmentSettings,
    pub display: DisplaySettings,
    pub lighting: Lighting,
    pub view_projection: Mat4,
    pub items: Vec<DrawItem>,
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
    object: DrawItem,
    opacity: f32,
    cutoff: f32,
    transparent: bool,
    depth: f32,
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
}
struct DepthTarget {
    view: wgpu::TextureView,
    size: [u32; 2],
}

/// Indexed geometry, per-object matrices/materials, sampled textures, and depth testing.
/// HDR opaque/transparent passes. Imported color images are sRGB; procedural colors are linear.
pub struct SceneRenderer {
    stats: FrameStats,
    culling: bool,
    state_caching: bool,
    environment: environment::Environment,
    display: display::Display,
    shadows: shadows::Shadows,
    pipeline: wgpu::RenderPipeline,
    transparent_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    quad: MeshBuffers,
    cube: MeshBuffers,
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

impl SceneRenderer {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let environment = environment::Environment::new(gpu);
        let display = display::Display::new(gpu, format);
        let format = wgpu::TextureFormat::Rgba16Float;
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("scene object layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(368),
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
                ],
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
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        "{}\n{}\n{}\n{}\n{}",
                        include_str!("scene/environment_sample.wgsl"),
                        include_str!("scene/shadow_sample.wgsl"),
                        include_str!("scene/local_lights.wgsl"),
                        include_str!("scene/gi.wgsl"),
                        include_str!("scene.wgsl")
                    )
                    .into(),
                ),
            });
        let make_pipeline = |transparent: bool| {
            gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene pipeline"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2] })] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: transparent.then_some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(!transparent), depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview_mask: None, cache: None,
        })
        };
        let pipeline = make_pipeline(false);
        let transparent_pipeline = make_pipeline(true);
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
            stats: Default::default(),
            culling: true,
            state_caching: true,
            environment,
            display,
            shadows,
            pbr,
            pipeline,
            transparent_pipeline,
            layout,
            quad,
            cube: cube(gpu),
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

    fn object_binding(&self, gpu: &Gpu, key: &TextureKind) -> Result<ObjectBinding> {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene object uniform"),
            size: 368,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = |texture: &wgpu::TextureView| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene object bindings"),
                layout: &self.layout,
                entries: &[
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
                            } else {
                                &self.sampler
                            },
                        ),
                    },
                ],
            })
        };
        let texture = match key {
            TextureKind::White => &self.white,
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
        })
    }

    pub fn clear_imported(&mut self) {
        self.model_upload_stats.clear();
        self.objects.clear();
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
        self.objects.clear();
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
        self.objects.clear();
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
        self.objects.clear();
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
        self.objects.clear();
        Ok(())
    }
    fn prepare(&self, scene: &RenderScene) -> Vec<PreparedDraw> {
        let mut draws = Vec::new();
        let mut add = |object: DrawItem,
                       opacity: f32,
                       cutoff: Option<f32>,
                       translucent: bool,
                       center: Vec3,
                       pbr_override: [f32; 2]| {
            let depth = scene
                .view_projection
                .project_point3(object.model.transform_point3(center))
                .z;
            draws.push(PreparedDraw {
                pbr_override,
                object,
                opacity,
                cutoff: cutoff.unwrap_or(0.0),
                transparent: cutoff.is_none() && translucent,
                depth,
            });
        };
        for object in &scene.items {
            if let MeshKind::Imported(id) = &object.mesh
                && let Some(parts) = self.models.get(id)
            {
                let overrides: BTreeMap<_, _> = object
                    .material
                    .surface_overrides
                    .iter()
                    .map(|v| (v.surface as usize, v))
                    .collect();
                for (index, part) in parts.iter().enumerate() {
                    let mut item = object.clone();
                    item.mesh = MeshKind::ModelPart(id.clone(), index);
                    for (tint, color) in item.material.tint.iter_mut().zip(part.color) {
                        *tint *= color;
                    }
                    let translucent = if item.material.texture == TextureKind::White {
                        if part.texture.is_some() {
                            item.material.texture = TextureKind::ModelPart(id.clone(), index);
                        }
                        part.translucent
                    } else {
                        part.color[3] < 1.0
                            || matches!(&item.material.texture, TextureKind::Imported(id) if self.transparent_textures.contains(id))
                    };
                    let override_value = overrides
                        .get(&index)
                        .filter(|value| value.source == part.source_key);
                    if let Some(value) = override_value {
                        for (tint, multiplier) in item.material.tint.iter_mut().zip(value.tint) {
                            *tint *= multiplier;
                        }
                    }
                    let factors = override_value.map_or([-1.; 2], |v| {
                        [v.metallic.unwrap_or(-1.), v.roughness.unwrap_or(-1.)]
                    });
                    add(
                        item,
                        part.color[3],
                        part.cutoff,
                        translucent,
                        part.center,
                        factors,
                    );
                }
            } else {
                let transparent = matches!(&object.material.texture, TextureKind::Imported(id) if self.transparent_textures.contains(id));
                add(object.clone(), 1.0, None, transparent, Vec3::ZERO, [-1.; 2]);
            }
        }
        // Opaque first; translucent surfaces back-to-front by projected center.
        draws.sort_by(|a, b| {
            a.transparent.cmp(&b.transparent).then_with(|| {
                if a.transparent {
                    b.depth.total_cmp(&a.depth)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
        });
        draws
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
    /// Diagnostic linear readback: bypass exposure, tone mapping and display encoding.
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
                item.material.surface_overrides.len() <= 4096,
                "too many material overrides"
            );
            let mut surfaces = BTreeSet::new();
            for value in item.material.surface_overrides.iter() {
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
        self.environment
            .prepare(gpu, scene.environment, scene.view_projection.inverse())?;
        self.display.prepare(gpu, size, scene.display, raw)?;
        if self.depth.as_ref().is_none_or(|d| d.size != size) {
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.depth = Some(DepthTarget {
                view: texture.create_view(&Default::default()),
                size,
            });
        }
        self.prepare_gi(gpu, scene.gi.as_ref())?;
        scene.lighting.validate()?;
        let lights = local_lights::uniform(&scene.lights)?;
        gpu.queue
            .write_buffer(&self.shadows.local_lights, 0, &lights);
        let draws = self.prepare(scene);
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
        for (draw, binding) in draws.iter().zip(&self.objects) {
            let object = &draw.object;
            let mvp = scene.view_projection * object.model;
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
            gpu.queue.write_buffer(
                &binding.buffer,
                0,
                &float_bytes(
                    mvp.to_cols_array()
                        .into_iter()
                        .chain(normal.to_cols_array())
                        .chain(tail)
                        .chain(object.model.to_cols_array())
                        .chain(scene.view_projection.inverse().to_cols_array())
                        .chain([
                            size[0] as f32,
                            size[1] as f32,
                            object.model.determinant().signum(),
                            if self.shading(&object.mesh).is_none_or(|s| s.double_sided) {
                                1.
                            } else {
                                0.
                            },
                        ])
                        .chain(scene.lighting.uniform())
                        .chain([draw.pbr_override[0], draw.pbr_override[1], 0., 0.]),
                ),
            );
        }
        self.update_shadows(gpu, scene, &draws)?;
        self.stats.prepare_ms = started.elapsed().as_secs_f64() * 1000.;
        let encode_started = std::time::Instant::now();
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scene frame"),
            });
        (self.stats.shadow_draws, self.stats.shadow_triangles) =
            self.draw_shadows(&mut encoder, scene, &draws);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene opaque pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.display.hdr(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.018,
                            g: 0.025,
                            b: 0.04,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
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
            self.environment.background(&mut pass, scene.environment);
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
                let key = (shading.is_some(), draw.transparent);
                if !self.state_caching || last_pipeline != Some(key) {
                    pass.set_pipeline(match key {
                        (true, false) => &self.pbr.opaque,
                        (true, true) => &self.pbr.transparent,
                        (false, false) => &self.pipeline,
                        (false, true) => &self.transparent_pipeline,
                    });
                    pass.set_bind_group(2, &self.shadows.sample_binding, &[]);
                    pass.set_bind_group(3, &self.environment.binding, &[]);
                    self.stats.pipeline_binds += 1;
                    last_pipeline = Some(key);
                }
                let mesh = match &object.mesh {
                    MeshKind::Quad => &self.quad,
                    MeshKind::Cube => &self.cube,
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
        self.display.draw(&mut encoder, target);
        let commands = encoder.finish();
        self.stats.encode_ms = encode_started.elapsed().as_secs_f64() * 1000.;
        let submit_started = std::time::Instant::now();
        gpu.queue.submit([commands]);
        self.stats.submit_ms = submit_started.elapsed().as_secs_f64() * 1000.;
        self.stats.cpu_ms = started.elapsed().as_secs_f64() * 1000.;
        Ok(())
    }
}
