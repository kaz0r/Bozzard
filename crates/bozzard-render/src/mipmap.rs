use crate::Gpu;

/// GPU downsampling avoids retaining a second decoded image pyramid on the CPU.
pub(crate) struct Mipmaps {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

pub(crate) fn levels(width: u32, height: u32) -> u32 {
    width.max(height).ilog2() + 1
}

pub(crate) fn texture_bytes(width: u32, height: u32) -> usize {
    (0..levels(width, height))
        .map(|level| ((width >> level).max(1) * (height >> level).max(1)) as usize * 4)
        .sum()
}

impl Mipmaps {
    pub(crate) fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("linear color mipmaps"),
                source: wgpu::ShaderSource::Wgsl(include_str!("mipmap.wgsl").into()),
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sRGB mipmap downsample"),
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
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mipmap linear clamp"),
            min_filter: wgpu::FilterMode::Linear,
            mag_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { pipeline, sampler }
    }

    pub(crate) fn generate(&self, gpu: &Gpu, texture: &wgpu::Texture) {
        for level in 1..texture.mip_level_count() {
            self.generate_rows(gpu, texture, level, 0, (texture.height() >> level).max(1));
        }
    }

    pub(crate) fn generate_rows(
        &self,
        gpu: &Gpu,
        texture: &wgpu::Texture,
        level: u32,
        row: u32,
        rows: u32,
    ) {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("generate model mipmaps"),
            });
        let layout = self.pipeline.get_bind_group_layout(0);
        {
            let view = |mip| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            };
            let source = view(level - 1);
            let target = view(level);
            let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mipmap source"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("downsample mip level"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: if row == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &binding, &[]);
            pass.set_scissor_rect(0, row, (texture.width() >> level).max(1), rows);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rectangular_mip_chains_reach_one_pixel() {
        assert_eq!(levels(1, 1), 1);
        assert_eq!(texture_bytes(1, 1), 4);
        assert_eq!(levels(7, 3), 3);
        assert_eq!(texture_bytes(7, 3), (21 + 3 + 1) * 4);
        assert_eq!(texture_bytes(1, 8), (8 + 4 + 2 + 1) * 4);
    }
}
