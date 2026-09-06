use crate::Gpu;
use anyhow::{Context, Result, ensure};
use glam::Mat4;
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

#[derive(Clone, Debug)]
pub enum MeshKind {
    Quad,
    Cube,
    Imported(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureKind {
    White,
    Checker,
    Imported(String),
}

#[derive(Clone, Debug)]
pub struct Material {
    pub tint: [f32; 3],
    pub uv_scale: [f32; 2],
    pub texture: TextureKind,
    pub lit: bool,
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
    pub view_projection: Mat4,
    pub items: Vec<DrawItem>,
}

struct MeshBuffers {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
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
/// Opaque objects only. Imported images are sampled as sRGB; procedural colors are linear.
pub struct SceneRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    quad: MeshBuffers,
    cube: MeshBuffers,
    white: wgpu::TextureView,
    checker: wgpu::TextureView,
    sampler: wgpu::Sampler,
    objects: Vec<ObjectBinding>,
    depth: Option<DepthTarget>,
    imported_meshes: BTreeMap<String, MeshBuffers>,
    imported_textures: BTreeMap<String, wgpu::TextureView>,
}

fn float_bytes(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

fn mesh(gpu: &Gpu, vertices: &[[f32; 8]], indices: &[u32]) -> MeshBuffers {
    MeshBuffers {
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
                            min_binding_size: wgpu::BufferSize::new(160),
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
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("scene pipeline layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("scene shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
            });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene pipeline"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2] })] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(), bias: Default::default() }),
            multisample: Default::default(), multiview_mask: None, cache: None,
        });
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
            pipeline,
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
            depth: None,
            imported_meshes: BTreeMap::new(),
            imported_textures: BTreeMap::new(),
        }
    }

    fn object_binding(&self, gpu: &Gpu, key: &TextureKind) -> Result<ObjectBinding> {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene object uniform"),
            size: 160,
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
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
        };
        let texture = match key {
            TextureKind::White => &self.white,
            TextureKind::Checker => &self.checker,
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
        self.objects.clear();
        self.imported_meshes.clear();
        self.imported_textures.clear();
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
        self.imported_textures
            .insert(id.into(), texture.create_view(&Default::default()));
        // Drop bind groups referring to old texture views; the next draw rebuilds them.
        self.objects.clear();
        Ok(())
    }

    pub fn draw(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        size: [u32; 2],
        scene: &RenderScene,
    ) -> Result<()> {
        ensure!(
            size.iter()
                .all(|s| *s > 0 && *s <= gpu.device.limits().max_texture_dimension_2d),
            "invalid render dimensions"
        );
        ensure!(
            scene.view_projection.is_finite(),
            "non-finite view/projection matrix"
        );
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
        self.objects.truncate(scene.items.len());
        for (index, object) in scene.items.iter().enumerate() {
            if index == self.objects.len() {
                self.objects
                    .push(self.object_binding(gpu, &object.material.texture)?);
            } else if self.objects[index].texture != object.material.texture {
                self.objects[index] = self.object_binding(gpu, &object.material.texture)?;
            }
            if let MeshKind::Imported(id) = &object.mesh {
                ensure!(
                    self.imported_meshes.contains_key(id),
                    "mesh '{id}' is not uploaded"
                );
            }
        }
        for (object, binding) in scene.items.iter().zip(&self.objects) {
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
                1.0,
                material.uv_scale[0],
                material.uv_scale[1],
                if material.lit { 1.0 } else { 0.0 },
                0.0,
            ];
            gpu.queue.write_buffer(
                &binding.buffer,
                0,
                &float_bytes(
                    mvp.to_cols_array()
                        .into_iter()
                        .chain(normal.to_cols_array())
                        .chain(tail),
                ),
            );
        }
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scene frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene opaque pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            pass.set_pipeline(&self.pipeline);
            for (object, binding) in scene.items.iter().zip(&self.objects) {
                let mesh = match &object.mesh {
                    MeshKind::Quad => &self.quad,
                    MeshKind::Cube => &self.cube,
                    MeshKind::Imported(id) => &self.imported_meshes[id],
                };
                pass.set_bind_group(0, &binding.binding, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.count, 0, 0..1);
            }
        }
        gpu.queue.submit([encoder.finish()]);
        Ok(())
    }
}
