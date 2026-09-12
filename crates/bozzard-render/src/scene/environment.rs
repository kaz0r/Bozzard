use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentSettings {
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
    pub ground: [f32; 3],
    pub intensity: f32,
    pub background: bool,
}
impl Default for EnvironmentSettings {
    fn default() -> Self {
        Self {
            zenith: [0.15, 0.32, 0.65],
            horizon: [0.65, 0.7, 0.8],
            ground: [0.12, 0.1, 0.08],
            intensity: 0.35,
            background: true,
        }
    }
}
impl EnvironmentSettings {
    pub fn disabled() -> Self {
        Self {
            intensity: 0.,
            background: false,
            ..Default::default()
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [self.zenith, self.horizon, self.ground]
                .iter()
                .flatten()
                .all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
            "invalid environment color"
        );
        ensure!(
            self.intensity.is_finite() && (0.0..=1000.0).contains(&self.intensity),
            "invalid environment intensity"
        );
        Ok(())
    }
}
pub(super) struct Environment {
    pub layout: wgpu::BindGroupLayout,
    pub binding: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    irradiance: wgpu::Texture,
    reflection: wgpu::Texture,
    brdf: wgpu::Texture,
    bake: wgpu::RenderPipeline,
    integrate: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    ready: bool,
}
fn texture(gpu: &Gpu, size: u32, cube: bool, levels: u32) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("environment integration map"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: if cube { 6 } else { 1 },
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}
impl Environment {
    pub fn new(gpu: &Gpu) -> Self {
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(112),
            },
            count: None,
        }];
        for binding in [1, 2, 4] {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: if binding == 4 {
                        wgpu::TextureViewDimension::D2
                    } else {
                        wgpu::TextureViewDimension::Cube
                    },
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("environment lighting layout"),
                entries: &entries,
            });
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("environment colors and camera"),
            size: 112,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let irradiance = texture(gpu, 32, true, 1);
        let reflection = texture(gpu, 128, true, 8);
        let brdf = texture(gpu, 64, false, 1);
        let cube_view = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::Cube),
                ..Default::default()
            })
        };
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment trilinear clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("environment lighting maps"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&cube_view(&irradiance)),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&cube_view(&reflection)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        &brdf.create_view(&Default::default()),
                    ),
                },
            ],
        });
        let source = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("environment convolution"),
                source: wgpu::ShaderSource::Wgsl(include_str!("environment_bake.wgsl").into()),
            });
        let sky_shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("procedural sky"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
                        "{}\n{}",
                        include_str!("environment_sample.wgsl"),
                        include_str!("environment_sky.wgsl")
                    )
                    .into(),
                ),
            });
        let sky_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sky layout"),
                bind_group_layouts: &[None, None, None, Some(&layout)],
                immediate_size: 0,
            });
        let pipeline = |shader: &wgpu::ShaderModule, entry: &str, sky: bool| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("environment pass"),
                    layout: if sky { Some(&sky_layout) } else { None },
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &if sky {
                            let mut targets =
                                geometry::color_targets(wgpu::TextureFormat::Rgba16Float, false);
                            for target in targets.iter_mut().skip(1) {
                                target.as_mut().unwrap().write_mask = wgpu::ColorWrites::empty();
                            }
                            targets.to_vec()
                        } else {
                            vec![Some(wgpu::ColorTargetState {
                                format: wgpu::TextureFormat::Rgba16Float,
                                blend: None,
                                write_mask: wgpu::ColorWrites::ALL,
                            })]
                        },
                    }),
                    primitive: Default::default(),
                    depth_stencil: sky.then_some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::Always),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        Self {
            layout,
            binding,
            uniform,
            irradiance,
            reflection,
            brdf,
            bake: pipeline(&source, "fs_weights", false),
            integrate: pipeline(&source, "fs_brdf", false),
            sky: pipeline(&sky_shader, "fs_main", true),
            ready: false,
        }
    }
    /// Convolution is linear in sky-band colors. Bake basis weights once, then edits
    /// only update this tiny uniform instead of regenerating all cubemap faces.
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        settings: EnvironmentSettings,
        inverse: Mat4,
        require_brdf: bool,
    ) -> Result<()> {
        settings.validate()?;
        if !self.ready && (settings.intensity > 0. || require_brdf) {
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("bake environment basis and BRDF"),
                });
            for (texture, diffuse, lut) in [
                (&self.irradiance, true, false),
                (&self.reflection, false, false),
                (&self.brdf, false, true),
            ] {
                let pipeline = if lut { &self.integrate } else { &self.bake };
                for level in 0..texture.mip_level_count() {
                    for face in 0..texture.depth_or_array_layers() {
                        let size = (texture.width() >> level).max(1);
                        let data =
                            gpu.device
                                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                    label: Some("environment bake parameters"),
                                    contents: &float_bytes([
                                        face as f32,
                                        level as f32 / 7.,
                                        size as f32,
                                        if diffuse { 1. } else { 0. },
                                    ]),
                                    usage: wgpu::BufferUsages::UNIFORM,
                                });
                        let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("environment bake parameters"),
                            layout: &pipeline.get_bind_group_layout(0),
                            entries: &[wgpu::BindGroupEntry {
                                binding: 0,
                                resource: data.as_entire_binding(),
                            }],
                        });
                        let view = texture.create_view(&wgpu::TextureViewDescriptor {
                            dimension: Some(wgpu::TextureViewDimension::D2),
                            base_mip_level: level,
                            mip_level_count: Some(1),
                            base_array_layer: face,
                            array_layer_count: Some(1),
                            ..Default::default()
                        });
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("environment convolution face"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            ..Default::default()
                        });
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &binding, &[]);
                        pass.draw(0..3, 0..1);
                    }
                }
            }
            gpu.queue.submit([encoder.finish()]);
            self.ready = true;
        }
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            &float_bytes(
                settings
                    .zenith
                    .into_iter()
                    .chain([settings.intensity])
                    .chain(settings.horizon)
                    .chain([0.])
                    .chain(settings.ground)
                    .chain([0.])
                    .chain(inverse.to_cols_array()),
            ),
        );
        Ok(())
    }
    pub fn background(&self, pass: &mut wgpu::RenderPass<'_>, settings: EnvironmentSettings) {
        if settings.background && settings.intensity > 0. {
            pass.set_pipeline(&self.sky);
            pass.set_bind_group(3, &self.binding, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
