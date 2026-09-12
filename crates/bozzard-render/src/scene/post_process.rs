use super::*;

pub(super) struct FrameInput<'a> {
    pub environment_gpu: Option<&'a environment::Environment>,
    pub fog: FogSettings,
    pub geometry: Option<&'a geometry::GeometryBuffers>,
    pub temporal: geometry::TemporalFrame,
    pub size: [u32; 2],
    pub raw: bool,
    pub view_projection: Mat4,
    pub depth: Option<&'a wgpu::TextureView>,
    pub lighting: Lighting,
    pub environment: EnvironmentSettings,
    pub shadows: Option<&'a shadows::Shadows>,
}
impl Default for FrameInput<'_> {
    fn default() -> Self {
        Self {
            environment_gpu: None,
            fog: Default::default(),
            geometry: None,
            temporal: Default::default(),
            size: [1, 1],
            raw: false,
            view_projection: Mat4::IDENTITY,
            depth: None,
            lighting: Lighting::default(),
            environment: EnvironmentSettings::disabled(),
            shadows: None,
        }
    }
}
struct Targets {
    size: [u32; 2],
    ao: wgpu::TextureView,
    color: wgpu::TextureView,
    ao_binding: wgpu::BindGroup,
    composite_binding: wgpu::BindGroup,
}
pub(super) struct PostProcess {
    ao_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    dummy: wgpu::TextureView,
    targets: Option<Targets>,
    ao_enabled: bool,
}
fn texture(gpu: &Gpu, size: [u32; 2], label: &'static str) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
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
impl PostProcess {
    pub fn invalidate(&mut self) {
        self.targets = None;
    }
    pub fn new(gpu: &Gpu) -> Self {
        let texture_binding = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("depth post-process inputs"),
                entries: &[
                    texture_binding(0, wgpu::TextureSampleType::Float { filterable: true }),
                    texture_binding(1, wgpu::TextureSampleType::Depth),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(112),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    texture_binding(4, wgpu::TextureSampleType::Float { filterable: true }),
                ],
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("depth post-process layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("SSAO and heat shimmer"),
                source: wgpu::ShaderSource::Wgsl(include_str!("post_process.wgsl").into()),
            });
        let pipeline = |entry| {
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
        Self {
            ao_pipeline: pipeline("ao_main"),
            composite_pipeline: pipeline("composite_main"),
            layout,
            uniform: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("depth post settings"),
                size: 112,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("post linear clamp"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            dummy: texture(gpu, [1, 1], "unused AO input"),
            targets: None,
            ao_enabled: false,
        }
    }
    fn binding(
        &self,
        gpu: &Gpu,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        ao: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post-process textures"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(color),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(ao),
                },
            ],
        })
    }
    /// True when the HDR source for downstream bloom/display has changed.
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        hdr: &wgpu::TextureView,
        settings: DisplaySettings,
        frame: &FrameInput<'_>,
    ) -> Result<bool> {
        self.ao_enabled = !frame.raw
            && settings.ambient_occlusion.enabled
            && settings.ambient_occlusion.intensity > 0.;
        let heat_enabled = !frame.raw
            && settings.heat_distortion.enabled
            && settings.heat_distortion.strength > 0.;
        if !self.ao_enabled && !heat_enabled {
            return Ok(self.targets.take().is_some());
        }
        let depth = frame
            .depth
            .context("depth post-processing requires a scene depth texture")?;
        let changed = self.targets.as_ref().is_none_or(|t| t.size != frame.size);
        if changed {
            let ao = texture(
                gpu,
                frame.size.map(|d| d.div_ceil(2)),
                "half resolution SSAO",
            );
            let color = texture(gpu, frame.size, "HDR after depth effects");
            self.targets = Some(Targets {
                size: frame.size,
                ao_binding: self.binding(gpu, hdr, depth, &self.dummy),
                composite_binding: self.binding(gpu, hdr, depth, &ao),
                ao,
                color,
            });
        }
        let ao = settings.ambient_occlusion;
        let heat = settings.heat_distortion;
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            &float_bytes(
                frame
                    .view_projection
                    .inverse()
                    .to_cols_array()
                    .into_iter()
                    .chain([
                        ao.intensity,
                        ao.radius,
                        ao.bias,
                        if self.ao_enabled { 1. } else { 0. },
                        if heat_enabled { heat.strength } else { 0. },
                        heat.threshold,
                        heat.speed,
                        heat.rise,
                        settings.time_seconds % 4096.,
                        frame.size[0] as f32,
                        frame.size[1] as f32,
                        0.,
                    ]),
            ),
        );
        Ok(changed)
    }
    pub fn output(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.color)
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(targets) = &self.targets else {
            return;
        };
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    target: &wgpu::TextureView,
                    pipeline: &wgpu::RenderPipeline,
                    binding: &wgpu::BindGroup| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("depth post-process"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, binding, &[]);
            pass.draw(0..3, 0..1);
        };
        if self.ao_enabled {
            pass(encoder, &targets.ao, &self.ao_pipeline, &targets.ao_binding);
        }
        pass(
            encoder,
            &targets.color,
            &self.composite_pipeline,
            &targets.composite_binding,
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn depth_effects_shader_validates() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("post_process.wgsl")).unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}
