use super::*;
use post_process::FrameInput;

struct Targets {
    size: [u32; 2],
    scattering: wgpu::TextureView,
    color: wgpu::TextureView,
    trace_binding: wgpu::BindGroup,
    composite_binding: wgpu::BindGroup,
}
/// Lazily constructed; disabled fog owns no frame-sized intermediate textures.
pub(super) struct Volumetric {
    trace: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    dummy: wgpu::TextureView,
    targets: Option<Targets>,
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
fn shader_source() -> String {
    [
        include_str!("shadow_sample.wgsl"),
        include_str!("local_lights.wgsl"),
        include_str!("volumetric.wgsl"),
    ]
    .join("\n")
}
impl Volumetric {
    pub fn new(gpu: &Gpu, shadow_layout: &wgpu::BindGroupLayout) -> Self {
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
                label: Some("volumetric inputs"),
                entries: &[
                    texture_binding(0, wgpu::TextureSampleType::Float { filterable: true }),
                    texture_binding(1, wgpu::TextureSampleType::Depth),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(192),
                        },
                        count: None,
                    },
                    texture_binding(3, wgpu::TextureSampleType::Float { filterable: true }),
                ],
            });
        let trace_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("volumetric light and shadow layout"),
                bind_group_layouts: &[Some(&layout), None, Some(shadow_layout)],
                immediate_size: 0,
            });
        let composite_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("volumetric composite layout"),
                bind_group_layouts: &[Some(&layout), None, Some(shadow_layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("shadowed volumetric scattering"),
                source: wgpu::ShaderSource::Wgsl(shader_source().into()),
            });
        let pipeline = |entry, layout: &wgpu::PipelineLayout| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(entry),
                    layout: Some(layout),
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
            trace: pipeline("trace_main", &trace_layout),
            composite: pipeline("composite_main", &composite_layout),
            layout,
            uniform: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("volumetric settings"),
                size: 192,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            dummy: texture(gpu, [1, 1], "unused volume input"),
            targets: None,
        }
    }
    fn binding(
        &self,
        gpu: &Gpu,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        scatter: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("volumetric textures"),
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
                    resource: wgpu::BindingResource::TextureView(scatter),
                },
            ],
        })
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        hdr: &wgpu::TextureView,
        display: DisplaySettings,
        frame: &FrameInput<'_>,
        source_changed: bool,
    ) -> Result<bool> {
        let fog = display.volumetric_fog;
        if frame.raw || !fog.enabled || fog.density == 0. {
            return Ok(self.targets.take().is_some());
        }
        let depth = frame.depth.context("volumetric fog requires scene depth")?;
        ensure!(
            frame.shadows.is_some(),
            "volumetric fog requires scene light bindings"
        );
        let changed = source_changed || self.targets.as_ref().is_none_or(|t| t.size != frame.size);
        if changed {
            let scattering = texture(
                gpu,
                frame.size.map(|d| d.div_ceil(2)),
                "half resolution scattering and transmittance",
            );
            let color = texture(gpu, frame.size, "HDR after volumetric scattering");
            self.targets = Some(Targets {
                size: frame.size,
                trace_binding: self.binding(gpu, hdr, depth, &self.dummy),
                composite_binding: self.binding(gpu, hdr, depth, &scattering),
                scattering,
                color,
            });
        }
        let light = frame.lighting;
        let direction = Vec3::from(light.sun_direction).normalize();
        let ambient: [f32; 3] = std::array::from_fn(|i| {
            (light.ambient_color[i] * light.ambient_intensity
                + frame.environment.horizon[i] * frame.environment.intensity * 0.25)
                * fog.ambient
        });
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
                        fog.density,
                        fog.base_height,
                        fog.height_falloff,
                        fog.start_distance,
                        fog.albedo[0],
                        fog.albedo[1],
                        fog.albedo[2],
                        fog.anisotropy,
                        fog.max_distance,
                        fog.noise_amount,
                        fog.noise_scale,
                        fog.light_intensity,
                        fog.wind[0],
                        fog.wind[1],
                        fog.wind[2],
                        display.time_seconds % 4096.,
                        direction.x,
                        direction.y,
                        direction.z,
                        light.sun_intensity,
                        light.sun_color[0],
                        light.sun_color[1],
                        light.sun_color[2],
                        0.,
                        ambient[0],
                        ambient[1],
                        ambient[2],
                        0.,
                        frame.size[0] as f32,
                        frame.size[1] as f32,
                        fog.steps as f32,
                        0.,
                    ]),
            ),
        );
        Ok(changed)
    }
    pub fn output(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.color)
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, shadows: Option<&wgpu::BindGroup>) {
        let Some(targets) = &self.targets else {
            return;
        };
        for (target, pipeline, binding, shadow_binding) in [
            (
                &targets.scattering,
                &self.trace,
                &targets.trace_binding,
                Some(shadows.expect("prepared volumetric light bindings")),
            ),
            (
                &targets.color,
                &self.composite,
                &targets.composite_binding,
                Some(shadows.expect("prepared volumetric light bindings")),
            ),
        ] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("volumetric fog and light shafts"),
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
            if let Some(binding) = shadow_binding {
                pass.set_bind_group(2, binding, &[]);
            }
            pass.draw(0..3, 0..1);
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn volumetric_shader_validates() {
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
