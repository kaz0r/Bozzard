use super::*;

#[derive(Clone, Copy, Debug)]
pub struct DisplaySettings {
    pub bloom: BloomSettings,
    pub exposure_ev: f32,
    pub tone_mapping: bool,
}
impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            bloom: BloomSettings::default(),
            exposure_ev: 0.,
            tone_mapping: true,
        }
    }
}
impl DisplaySettings {
    pub fn validate(&self) -> Result<()> {
        self.bloom.validate()?;
        ensure!(
            self.exposure_ev.is_finite() && (-16.0..=16.0).contains(&self.exposure_ev),
            "invalid exposure"
        );
        Ok(())
    }
}
pub(super) struct Display {
    bloom: bloom::Bloom,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    target: Option<(wgpu::TextureView, wgpu::BindGroup, [u32; 2])>,
    srgb_target: bool,
}
impl Display {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("HDR display transform"),
                source: wgpu::ShaderSource::Wgsl(include_str!("display.wgsl").into()),
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("HDR display pass"),
                layout: None,
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
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("display settings"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            bloom: bloom::Bloom::new(gpu),
            pipeline,
            uniform,
            target: None,
            srgb_target: format.is_srgb(),
        }
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        size: [u32; 2],
        settings: DisplaySettings,
        raw: bool,
    ) -> Result<()> {
        settings.validate()?;
        ensure!(
            !raw || !self.srgb_target,
            "raw linear diagnostics need a non-sRGB output target"
        );
        if self.target.as_ref().is_none_or(|(_, _, old)| *old != size) {
            let view = gpu
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("linear HDR scene"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            self.bloom.prepare(
                gpu,
                &view,
                size,
                settings.bloom,
                !raw && settings.bloom.enabled && settings.bloom.intensity > 0.,
            );
            let binding = self.binding(gpu, &view);
            self.target = Some((view, binding, size));
        } else if self.bloom.prepare(
            gpu,
            &self.target.as_ref().unwrap().0,
            size,
            settings.bloom,
            !raw && settings.bloom.enabled && settings.bloom.intensity > 0.,
        ) {
            let binding = self.binding(gpu, &self.target.as_ref().unwrap().0);
            self.target.as_mut().unwrap().1 = binding;
        }
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            &float_bytes([
                if raw { 1. } else { settings.exposure_ev.exp2() },
                if !raw && settings.tone_mapping {
                    1.
                } else {
                    0.
                },
                if !raw && !self.srgb_target { 1. } else { 0. },
                if !raw && settings.bloom.enabled {
                    settings.bloom.intensity
                } else {
                    0.
                },
                if raw { 0. } else { 1. }, // FXAA is display-only, never diagnostic.
                0.,
                0.,
                0.,
            ]),
        );
        Ok(())
    }
    fn binding(&self, gpu: &Gpu, view: &wgpu::TextureView) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HDR scene and bloom"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(self.bloom.output()),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.bloom.sampler),
                },
            ],
        })
    }
    pub fn hdr(&self) -> &wgpu::TextureView {
        &self.target.as_ref().unwrap().0
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        self.bloom.draw(encoder);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("exposure tone mapping and display encoding"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.target.as_ref().unwrap().1, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_fxaa_shader_validates() {
        let module = wgpu::naga::front::wgsl::parse_str(include_str!("display.wgsl")).unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }

    // Run with: cargo test -p bozzard-render display_fxaa -- --nocapture
    #[test]
    fn display_fxaa() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("FXAA diagonal fixture"),
                source: wgpu::ShaderSource::Wgsl(
                    r#"
                @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                    let p = array<vec2<f32>,3>(vec2<f32>(-1,-1),vec2<f32>(3,-1),vec2<f32>(-1,3));
                    return vec4<f32>(p[i],0,1);
                }
                @fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
                    return vec4<f32>(vec3<f32>(select(0.0,1.0,p.x > p.y*0.6+4.0)),0.375);
                }
            "#
                    .into(),
                ),
            });
        let fixture = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("FXAA fixture"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
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
            });
        let capture = |display: &mut Display,
                       size: [u32; 2],
                       settings,
                       raw,
                       format|
         -> Result<crate::Frame> {
            display.prepare(&gpu, size, settings, raw)?;
            let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("FXAA readback"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: display.hdr(),
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&fixture);
                pass.draw(0..3, 0..1);
            }
            display.draw(&mut encoder, &output.create_view(&Default::default()));
            gpu.queue.submit([encoder.finish()]);
            crate::read_texture(&gpu, &output, size[0], size[1])
        };
        let unorm = wgpu::TextureFormat::Rgba8Unorm;
        let srgb = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut display = Display::new(&gpu, unorm);
        let mut hardware = Display::new(&gpu, srgb);
        let mut settings = DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        };
        settings.bloom.enabled = false;
        for size in [[32, 32], [47, 29], [1, 1]] {
            let raw = capture(&mut display, size, settings, true, unorm)?;
            let aa = capture(&mut display, size, settings, false, unorm)?;
            assert!(raw.rgba.chunks_exact(4).all(|p| p[0] == 0 || p[0] == 255));
            assert!(aa.rgba.chunks_exact(4).all(|p| p[3] == 96));
            if size[0] > 1 {
                assert!(
                    aa.rgba
                        .chunks_exact(4)
                        .filter(|p| p[0] > 0 && p[0] < 255)
                        .count()
                        > 10,
                    "diagonal must gain partial coverage"
                );
                assert_eq!(&aa.rgba[..4], &raw.rgba[..4], "flat black must stay sharp");
                let corner = (size[0] as usize - 1) * 4;
                assert_eq!(
                    &aa.rgba[corner..corner + 4],
                    &raw.rgba[corner..corner + 4],
                    "flat white must stay sharp"
                );
            } else {
                assert_eq!(aa.rgba, raw.rgba);
            }
            settings.bloom.enabled = true;
            settings.bloom.threshold = 0.1;
            settings.bloom.intensity = 0.2;
            let bloom = capture(&mut display, size, settings, false, unorm)?;
            let encoded = capture(&mut hardware, size, settings, false, srgb)?;
            assert!(
                bloom
                    .rgba
                    .iter()
                    .zip(&encoded.rgba)
                    .all(|(a, b)| a.abs_diff(*b) <= 1),
                "hardware/software sRGB parity"
            );
            if size[0] > 1 {
                assert_ne!(bloom.rgba, aa.rgba, "bloom must contribute");
            }
            assert_eq!(
                capture(&mut display, size, settings, true, unorm)?.rgba,
                raw.rgba,
                "raw must bypass bloom and FXAA"
            );
            settings.bloom.enabled = false;
        }
        Ok(())
    }
}
