//! Decals: images projected down a box's local -Y onto the opaque surfaces inside it. They
//! draw in a pass of their own after the opaque scene and before transparent surfaces,
//! reading the depth buffer to find the surface under each pixel.
use super::*;

/// One decal: the unit box (-0.5..0.5 on every axis) mapped onto its world volume.
#[derive(Clone, Debug, PartialEq)]
pub struct DecalItem {
    pub volume: Mat4,
    pub texture: TextureKind,
    /// Linear RGB and opacity, multiplied with the image.
    pub color: [f32; 4],
    /// Smallest accepted dot product between a surface normal and the box's local +Y.
    pub min_facing: f32,
    /// Shade with the scene's lights like a painted surface.
    pub lit: bool,
}

pub const MAX_DECALS: usize = 1024;
/// Volume, inverse volume, colour, parameters.
const RECORD_FLOATS: usize = 16 + 16 + 4 + 4;

fn shader_source() -> String {
    [
        include_str!("decals.wgsl"),
        include_str!("shadow_sample.wgsl"),
        include_str!("local_lights.wgsl"),
        include_str!("fog.wgsl"),
    ]
    .join("\n")
}

pub(super) struct Decals {
    pipeline: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    pub(super) image_layout: wgpu::BindGroupLayout,
    pub(super) sampler: wgpu::Sampler,
    records: wgpu::Buffer,
    capacity: usize,
    frame: Option<wgpu::BindGroup>,
    /// One image binding per drawn decal, in record order.
    images: Vec<wgpu::BindGroup>,
}

impl Decals {
    pub(super) fn new(gpu: &Gpu, shadows: &wgpu::BindGroupLayout) -> Self {
        let frame_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("decal frame"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(FRAME_UNIFORM_BYTES as u64),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let image_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("decal image"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("decals"),
                bind_group_layouts: &[Some(&frame_layout), Some(&image_layout), Some(shadows)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("decals"),
                source: wgpu::ShaderSource::Wgsl(shader_source().into()),
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("projected decals"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                // Both windings rasterize; the shader keeps back faces, which also cover a
                // box the camera stands inside.
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("decal image"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });
        Self {
            pipeline,
            frame_layout,
            image_layout,
            sampler,
            records: Self::record_buffer(gpu, 16),
            capacity: 16,
            frame: None,
            images: Vec::new(),
        }
    }
    fn record_buffer(gpu: &Gpu, records: usize) -> wgpu::Buffer {
        gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("decal records"),
            size: (records * RECORD_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    /// Upload this frame's decals. `images` pairs each decal to draw with its image binding;
    /// decals whose image is not resident yet were already left out.
    pub(super) fn prepare(
        &mut self,
        gpu: &Gpu,
        drawn: &[(&DecalItem, wgpu::BindGroup)],
        frame: &wgpu::Buffer,
        depth: &wgpu::TextureView,
    ) -> Result<()> {
        ensure!(drawn.len() <= MAX_DECALS, "more than {MAX_DECALS} decals");
        self.images.clear();
        if drawn.is_empty() {
            self.frame = None;
            return Ok(());
        }
        let mut floats = Vec::with_capacity(drawn.len() * RECORD_FLOATS);
        for (decal, image) in drawn {
            ensure!(
                decal.volume.is_finite()
                    && decal.volume.determinant().abs() > 1e-12
                    && decal.color.iter().all(|c| c.is_finite())
                    && decal.min_facing.is_finite(),
                "decal volume and colour must be finite and the volume invertible"
            );
            floats.extend(decal.volume.to_cols_array());
            floats.extend(decal.volume.inverse().to_cols_array());
            floats.extend(decal.color);
            floats.extend([
                decal.min_facing,
                if decal.lit { 1.0 } else { 0.0 },
                0.0,
                decal.volume.determinant().signum(),
            ]);
            self.images.push(image.clone());
        }
        if drawn.len() > self.capacity {
            self.capacity = drawn.len().next_power_of_two();
            self.records = Self::record_buffer(gpu, self.capacity);
        }
        gpu.queue
            .write_buffer(&self.records, 0, &float_bytes(floats));
        self.frame = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("decal frame"),
            layout: &self.frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.records.as_entire_binding(),
                },
            ],
        }));
        Ok(())
    }
    pub(super) fn has_decals(&self) -> bool {
        self.frame.is_some()
    }
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, shadows: &wgpu::BindGroup) {
        let Some(frame) = &self.frame else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(2, shadows, &[]);
        for (index, image) in self.images.iter().enumerate() {
            pass.set_bind_group(1, image, &[]);
            pass.draw(0..36, index as u32..index as u32 + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decal_shader_validates() {
        let source = super::shader_source();
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}
