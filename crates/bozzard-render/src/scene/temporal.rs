use super::*;
struct Targets {
    size: [u32; 2],
    colors: [wgpu::TextureView; 2],
    surfaces: [wgpu::TextureView; 2],
    blur: wgpu::TextureView,
    tiles: wgpu::TextureView,
}
pub(super) struct Temporal {
    layout: wgpu::BindGroupLayout,
    taa: wgpu::RenderPipeline,
    motion: wgpu::RenderPipeline,
    tilemax: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    targets: Option<Targets>,
    bindings: Option<(wgpu::BindGroup, wgpu::BindGroup)>,
    index: usize,
    active: bool,
    blur: bool,
    initialized: bool,
}
impl Temporal {
    pub fn new(gpu: &Gpu) -> Self {
        let mut entries = Vec::new();
        for i in 0..6 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: i,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: if i == 1 {
                        wgpu::TextureSampleType::Depth
                    } else {
                        wgpu::TextureSampleType::Float { filterable: true }
                    },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 6,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 7,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(192),
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 8,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("temporal inputs"),
                entries: &entries,
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("temporal pipeline layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("temporal resolve and velocity blur"),
                source: wgpu::ShaderSource::Wgsl(include_str!("temporal.wgsl").into()),
            });
        let pipeline = |entry, count| {
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
                        targets: &(0..count)
                            .map(|_| {
                                Some(wgpu::ColorTargetState {
                                    format: wgpu::TextureFormat::Rgba16Float,
                                    blend: None,
                                    write_mask: wgpu::ColorWrites::ALL,
                                })
                            })
                            .collect::<Vec<_>>(),
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        Self {
            taa: pipeline("fs_taa", 2),
            motion: pipeline("fs_motion", 1),
            tilemax: pipeline("fs_tilemax", 1),
            layout,
            uniform: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("temporal controls"),
                size: 192,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            targets: None,
            bindings: None,
            index: 0,
            active: false,
            blur: false,
            initialized: false,
        }
    }
    pub fn reset(&mut self) {
        self.initialized = false;
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        source: &wgpu::TextureView,
        settings: DisplaySettings,
        frame: &post_process::FrameInput<'_>,
    ) -> bool {
        let old_active = self.active;
        self.active = !frame.raw
            && frame.geometry.is_some()
            && (settings.temporal_aa.enabled || settings.motion_blur.enabled);
        if !self.active {
            self.reset();
            return old_active;
        }
        let old_blur = self.blur;
        self.blur = settings.motion_blur.enabled
            && settings.motion_blur.shutter_angle > 0.
            && settings.motion_blur.max_radius > 0.;
        let size = frame.size;
        let resized = self.targets.as_ref().is_none_or(|t| t.size != size);
        if resized {
            self.targets = Some(Targets {
                size,
                colors: std::array::from_fn(|_| {
                    geometry::color_texture(gpu, size, "temporal HDR history")
                }),
                surfaces: std::array::from_fn(|_| {
                    geometry::color_texture(gpu, size, "temporal surface history")
                }),
                blur: geometry::color_texture(gpu, size, "motion blur output"),
                tiles: geometry::color_texture(
                    gpu,
                    size.map(|d| d.div_ceil(16)),
                    "maximum velocity tiles",
                ),
            });
            self.reset();
        }
        self.index = 1 - self.index;
        let targets = self.targets.as_ref().unwrap();
        let geometry = frame.geometry.unwrap();
        let bind = |current, tiles| {
            let views = [
                current,
                frame.depth.unwrap(),
                &geometry.motion,
                &geometry.normal,
                &targets.colors[1 - self.index],
                &targets.surfaces[1 - self.index],
            ];
            let mut entries: Vec<_> = views
                .into_iter()
                .enumerate()
                .map(|(i, view)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: wgpu::BindingResource::TextureView(view),
                })
                .collect();
            entries.push(wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 7,
                resource: self.uniform.as_entire_binding(),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(tiles),
            });
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("temporal frame bindings"),
                layout: &self.layout,
                entries: &entries,
            })
        };
        self.bindings = Some((
            bind(source, &targets.colors[1 - self.index]),
            bind(&targets.colors[self.index], &targets.tiles),
        ));
        let t = frame.temporal;
        let valid = t.valid && self.initialized;
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            &float_bytes(
                frame
                    .view_projection
                    .inverse()
                    .to_cols_array()
                    .into_iter()
                    .chain(t.previous_vp.to_cols_array())
                    .chain([
                        size[0] as f32,
                        size[1] as f32,
                        if settings.temporal_aa.enabled {
                            settings.temporal_aa.history_weight
                        } else {
                            0.
                        },
                        if valid { 1. } else { 0. },
                        (settings.motion_blur.max_radius * size[1] as f32 / 1080.).min(128.),
                        if valid {
                            settings.motion_blur.shutter_angle / 360. * t.motion_scale
                        } else {
                            0.
                        },
                        settings.motion_blur.samples as f32,
                        if t.repeated && valid && settings.temporal_aa.enabled {
                            1.
                        } else {
                            0.
                        },
                        (t.jitter[0] - t.previous_jitter[0]) / size[0] as f32,
                        (t.jitter[1] - t.previous_jitter[1]) / size[1] as f32,
                        0.,
                        0.,
                        if settings.volumetric_fog.enabled || settings.heat_distortion.enabled {
                            0.25
                        } else {
                            0.
                        },
                        0.,
                        0.,
                        0.,
                    ]),
            ),
        );
        self.initialized = true;
        resized || !old_active || old_blur != self.blur || !self.blur
    }
    pub fn output(&self) -> Option<&wgpu::TextureView> {
        self.active.then(|| {
            let t = self.targets.as_ref().unwrap();
            if self.blur {
                &t.blur
            } else {
                &t.colors[self.index]
            }
        })
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder) {
        if !self.active {
            return;
        }
        let t = self.targets.as_ref().unwrap();
        let (taa, motion) = self.bindings.as_ref().unwrap();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("temporal antialiasing resolve"),
                color_attachments: &[
                    geometry::attachment(&t.colors[self.index], wgpu::Color::TRANSPARENT),
                    geometry::attachment(&t.surfaces[self.index], wgpu::Color::TRANSPARENT),
                ],
                ..Default::default()
            });
            pass.set_pipeline(&self.taa);
            pass.set_bind_group(0, taa, &[]);
            pass.draw(0..3, 0..1);
        }
        if self.blur {
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("maximum object velocity tiles"),
                    color_attachments: &[geometry::attachment(&t.tiles, wgpu::Color::TRANSPARENT)],
                    ..Default::default()
                });
                pass.set_pipeline(&self.tilemax);
                pass.set_bind_group(0, taa, &[]);
                pass.draw(0..3, 0..1);
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("camera and object motion blur"),
                color_attachments: &[geometry::attachment(&t.blur, wgpu::Color::TRANSPARENT)],
                ..Default::default()
            });
            pass.set_pipeline(&self.motion);
            pass.set_bind_group(0, motion, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn temporal_shader_validates() {
        let source = include_str!("temporal.wgsl");
        let module = wgpu::naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
}
