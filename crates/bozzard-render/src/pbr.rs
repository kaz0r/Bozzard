use crate::{Gpu, ModelImage};
use anyhow::{Result, ensure};
use wgpu::util::DeviceExt;

#[derive(Clone)]
pub struct MaterialMap<'a> {
    pub image: ModelImage<'a>,
    pub sampler: wgpu::SamplerDescriptor<'static>,
}
#[derive(Clone)]
pub struct ModelShading<'a> {
    pub vertex_start: u32,
    /// Tangent XYZW, then normal, metallic/roughness, occlusion and emissive UVs.
    pub vertices: &'a [[f32; 12]],
    pub metallic: f32,
    pub roughness: f32,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    pub emissive_factor: [f32; 3],
    pub double_sided: bool,
    pub base_color_sampler: wgpu::SamplerDescriptor<'static>,
    pub normal: Option<MaterialMap<'a>>,
    pub metallic_roughness: Option<MaterialMap<'a>>,
    pub occlusion: Option<MaterialMap<'a>>,
    pub emissive: Option<MaterialMap<'a>>,
}
impl ModelShading<'_> {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            !self.vertices.is_empty() && self.vertices.iter().flatten().all(|v| v.is_finite()),
            "invalid shading vertices"
        );
        ensure!(
            self.vertices
                .iter()
                .all(|v| v[..3].iter().map(|x| x * x).sum::<f32>() > 1e-12
                    && (v[3].abs() - 1.).abs() < 0.0001),
            "invalid shading tangent frame"
        );
        ensure!(
            [self.metallic, self.roughness, self.occlusion_strength]
                .into_iter()
                .chain(self.emissive_factor)
                .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
                && self.normal_scale.is_finite(),
            "invalid PBR material factors"
        );
        Ok(())
    }
}

pub(crate) struct UploadedShading {
    pub vertices: wgpu::Buffer,
    pub binding: wgpu::BindGroup,
}
#[derive(Clone)]
pub(crate) struct PbrRenderer {
    pub opaque: wgpu::RenderPipeline,
    pub transparent: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    neutral_normal: wgpu::TextureView,
    white: wgpu::TextureView,
    sampler: wgpu::Sampler,
}

impl PbrRenderer {
    pub fn new(
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        object_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(32),
            },
            count: None,
        }];
        for slot in 0..4 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 1 + slot * 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 2 + slot * 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            });
        }
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("PBR material layout"),
                entries: &entries,
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("PBR pipeline layout"),
                bind_group_layouts: &[Some(object_layout), Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("metallic roughness PBR"),
                source: wgpu::ShaderSource::Wgsl(include_str!("pbr.wgsl").into()),
            });
        let pipeline = |transparent| {
            gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("PBR scene"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0=>Float32x3, 1=>Float32x3, 2=>Float32x2] }),
                    Some(wgpu::VertexBufferLayout { array_stride: 48, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![3=>Float32x4, 4=>Float32x2, 5=>Float32x2, 6=>Float32x2, 7=>Float32x2] }),
                ] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: if transparent { Some(wgpu::BlendState::ALPHA_BLENDING) } else { None }, write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: Default::default(), depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(!transparent), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview_mask: None, cache: None,
        })
        };
        let opaque = pipeline(false);
        let transparent = pipeline(true);
        let image = |rgba: &[u8]| {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("PBR neutral texture"),
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
                rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                texture.size(),
            );
            texture.create_view(&Default::default())
        };
        Self {
            opaque,
            transparent,
            layout,
            neutral_normal: image(&[128, 128, 255, 255]),
            white: image(&[255; 4]),
            sampler: gpu.device.create_sampler(&Default::default()),
        }
    }

    pub fn upload(
        &self,
        gpu: &Gpu,
        material: &ModelShading<'_>,
        views: [Option<wgpu::TextureView>; 4],
    ) -> UploadedShading {
        let vertices = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("PBR tangent and UV attributes"),
                contents: &crate::scene::float_bytes(material.vertices.iter().flatten().copied()),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.bind(gpu, material, views, vertices)
    }

    pub(crate) fn bind(
        &self,
        gpu: &Gpu,
        material: &ModelShading<'_>,
        views: [Option<wgpu::TextureView>; 4],
        vertices: wgpu::Buffer,
    ) -> UploadedShading {
        let uniform = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("PBR factors"),
                contents: &crate::scene::float_bytes([
                    material.metallic,
                    material.roughness,
                    material.normal_scale,
                    material.occlusion_strength,
                    material.emissive_factor[0],
                    material.emissive_factor[1],
                    material.emissive_factor[2],
                    if material.double_sided { 1. } else { 0. },
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let maps = [
            &material.normal,
            &material.metallic_roughness,
            &material.occlusion,
            &material.emissive,
        ];
        let samplers: Vec<_> = maps
            .iter()
            .map(|map| {
                map.as_ref()
                    .map(|map| gpu.device.create_sampler(&map.sampler))
            })
            .collect();
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }];
        for (i, view) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: 1 + i as u32 * 2,
                resource: wgpu::BindingResource::TextureView(view.as_ref().unwrap_or(if i == 0 {
                    &self.neutral_normal
                } else {
                    &self.white
                })),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 2 + i as u32 * 2,
                resource: wgpu::BindingResource::Sampler(
                    samplers[i].as_ref().unwrap_or(&self.sampler),
                ),
            });
        }
        UploadedShading {
            binding: gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("PBR material"),
                layout: &self.layout,
                entries: &entries,
            }),
            vertices,
        }
    }
}
