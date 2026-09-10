use super::*;

#[derive(Clone, Copy, Debug)]
pub struct DisplaySettings {
    pub exposure_ev: f32,
    pub tone_mapping: bool,
}
impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            exposure_ev: 0.,
            tone_mapping: true,
        }
    }
}
impl DisplaySettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.exposure_ev.is_finite() && (-16.0..=16.0).contains(&self.exposure_ev),
            "invalid exposure"
        );
        Ok(())
    }
}
pub(super) struct Display {
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
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
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
            let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("HDR scene input"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.uniform.as_entire_binding(),
                    },
                ],
            });
            self.target = Some((view, binding, size));
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
                0.,
            ]),
        );
        Ok(())
    }
    pub fn hdr(&self) -> &wgpu::TextureView {
        &self.target.as_ref().unwrap().0
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
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
