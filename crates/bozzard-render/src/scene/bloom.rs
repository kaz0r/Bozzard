use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BloomSettings {
    pub enabled: bool,
    pub intensity: f32,
    /// Scene-linear radiance, before exposure; independent of display brightness.
    pub threshold: f32,
    /// Weight of wider pyramid levels, controlling the glow radius.
    pub scatter: f32,
    pub anamorphic: f32,
}
impl Default for BloomSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: 0.15,
            threshold: 1.,
            scatter: 0.7,
            anamorphic: 0.,
        }
    }
}
impl BloomSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.anamorphic.is_finite() && (0.0..=1.).contains(&self.anamorphic),
            "invalid anamorphic bloom"
        );
        ensure!(
            self.intensity.is_finite() && (0.0..=10.).contains(&self.intensity),
            "bloom intensity must be in 0..10"
        );
        ensure!(
            self.threshold.is_finite() && (0.0..=60_000.).contains(&self.threshold),
            "bloom threshold must be in 0..60000"
        );
        ensure!(
            self.scatter.is_finite() && (0.0..=1.).contains(&self.scatter),
            "bloom scatter must be in 0..1"
        );
        Ok(())
    }
}
struct Level {
    down: wgpu::TextureView,
    up: wgpu::TextureView,
    down_binding: wgpu::BindGroup,
    up_binding: Option<wgpu::BindGroup>,
}
pub(super) struct Bloom {
    source_dirty: bool,
    prefilter: wgpu::RenderPipeline,
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    black: wgpu::TextureView,
    levels: Vec<Level>,
    size: [u32; 2],
}
fn texture(gpu: &Gpu, size: [u32; 2]) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("bloom pyramid"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
impl Bloom {
    pub fn new(gpu: &Gpu) -> Self {
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("bloom inputs"),
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
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(16),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("bloom"),
                source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()),
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("bloom"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipeline = |entry: &'static str| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba16Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom linear clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bloom settings"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // WebGPU zero-initialization makes this a black fallback without a render pass.
        let black = texture(gpu, [1, 1]);
        Self {
            prefilter: pipeline("prefilter"),
            downsample: pipeline("downsample"),
            upsample: pipeline("upsample"),
            layout,
            sampler,
            uniform,
            black,
            levels: Vec::new(),
            source_dirty: false,
            size: [0, 0],
        }
    }
    fn binding(
        &self,
        gpu: &Gpu,
        high: &wgpu::TextureView,
        low: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom inputs"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(high),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(low),
                },
            ],
        })
    }
    /// Returns whether the display pass must rebind its bloom image.
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        hdr: &wgpu::TextureView,
        size: [u32; 2],
        settings: BloomSettings,
        active: bool,
    ) -> bool {
        if !active {
            let changed = !self.levels.is_empty();
            self.levels.clear();
            return changed;
        }
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            &float_bytes([
                settings.threshold,
                settings.threshold * 0.5,
                settings.scatter,
                settings.anamorphic,
            ]),
        );
        if self.size == size && !self.levels.is_empty() {
            if self.source_dirty {
                self.levels[0].down_binding = self.binding(gpu, hdr, &self.black);
                self.source_dirty = false;
            }
            return false;
        }
        self.levels.clear();
        self.size = size;
        let mut dims = size.map(|d| d.div_ceil(2));
        for _ in 0..6 {
            let down = texture(gpu, dims);
            let up = texture(gpu, dims);
            let previous = self.levels.last().map(|l| &l.down).unwrap_or(hdr);
            let down_binding = self.binding(gpu, previous, &self.black);
            self.levels.push(Level {
                down,
                up,
                down_binding,
                up_binding: None,
            });
            if dims == [1, 1] {
                break;
            }
            dims = dims.map(|d| d.div_ceil(2));
        }
        for i in (0..self.levels.len() - 1).rev() {
            let next = &self.levels[i + 1];
            let low = if i + 2 == self.levels.len() {
                &next.down
            } else {
                &next.up
            };
            self.levels[i].up_binding = Some(self.binding(gpu, &self.levels[i].down, low));
        }
        true
    }
    pub fn invalidate(&mut self) {
        self.source_dirty = true;
    }
    pub fn output(&self) -> &wgpu::TextureView {
        self.levels
            .first()
            .map(|l| {
                if self.levels.len() == 1 {
                    &l.down
                } else {
                    &l.up
                }
            })
            .unwrap_or(&self.black)
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder) {
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    target: &wgpu::TextureView,
                    pipeline: &wgpu::RenderPipeline,
                    binding: &wgpu::BindGroup| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bloom filter"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, binding, &[]);
            pass.draw(0..3, 0..1);
        };
        for (i, level) in self.levels.iter().enumerate() {
            pass(
                encoder,
                &level.down,
                if i == 0 {
                    &self.prefilter
                } else {
                    &self.downsample
                },
                &level.down_binding,
            );
        }
        for level in self.levels.iter().rev() {
            if let Some(binding) = &level.up_binding {
                pass(encoder, &level.up, &self.upsample, binding);
            }
        }
    }
}
