//! Construction-time wgpu helpers the full-screen passes share.
use super::*;

/// An HDR (RGBA16F) target that a pass renders and a later pass samples.
pub(super) fn color_texture(gpu: &Gpu, size: [u32; 2], label: &str) -> wgpu::TextureView {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// A full-screen-triangle pass: the shader's `vs_main`, then `entry` writing one unblended
/// target per format.
pub(super) fn fullscreen_pipeline(
    gpu: &Gpu,
    label: &str,
    layout: Option<&wgpu::PipelineLayout>,
    shader: &wgpu::ShaderModule,
    entry: &str,
    formats: &[wgpu::TextureFormat],
) -> wgpu::RenderPipeline {
    let targets: Vec<_> = formats
        .iter()
        .map(|&format| {
            Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })
        })
        .collect();
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout,
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
                targets: &targets,
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}
