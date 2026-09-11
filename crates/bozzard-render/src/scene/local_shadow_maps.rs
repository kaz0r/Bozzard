use super::*;

struct Caster {
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
}
pub(super) struct ShadowMaps {
    pub uniform: wgpu::Buffer,
    pub depth: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
    casters: Vec<Caster>,
    matrices: Vec<Mat4>,
    resolution: u32,
}
fn target(gpu: &Gpu, count: usize, resolution: u32) -> (wgpu::TextureView, Vec<wgpu::TextureView>) {
    let resolution = if count == 0 { 1 } else { resolution };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("local shadow depth array"),
        size: wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: count.max(1) as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layers = (0..count)
        .map(|i| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    (depth, layers)
}

impl ShadowMaps {
    pub fn new(
        gpu: &Gpu,
        caster_layout: &wgpu::BindGroupLayout,
        capacity: usize,
        resolution: u32,
    ) -> Self {
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("local shadow receivers"),
            size: capacity as u64 * 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let casters = (0..capacity)
            .map(|_| {
                let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("local shadow caster"),
                    size: 80,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("local shadow caster"),
                    layout: caster_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                });
                Caster { uniform, binding }
            })
            .collect();
        let (depth, layers) = target(gpu, 0, resolution);
        Self {
            uniform,
            depth,
            layers,
            casters,
            matrices: Vec::new(),
            resolution,
        }
    }
    /// Map ordering is rebuilt each frame; every pass has its own buffer because
    /// queue writes all precede command execution.
    pub fn update(&mut self, gpu: &Gpu, maps: &[(Mat4, LocalShadowSettings)]) -> Result<bool> {
        ensure!(
            maps.len() <= self.casters.len(),
            "too many local shadow maps"
        );
        let changed = maps.len() != self.layers.len();
        if changed {
            ensure!(
                self.resolution <= gpu.device.limits().max_texture_dimension_2d
                    && maps.len() as u32 <= gpu.device.limits().max_texture_array_layers,
                "local shadow maps exceed device limits"
            );
            (self.depth, self.layers) = target(gpu, maps.len(), self.resolution);
        }
        let mut bytes = vec![0; self.casters.len() * 80];
        for (slot, (matrix, shadow)) in maps.iter().enumerate() {
            let row = float_bytes(matrix.to_cols_array().into_iter().chain([
                shadow.bias,
                shadow.normal_bias,
                0.,
                1. / self.resolution as f32,
            ]));
            bytes[slot * 80..(slot + 1) * 80].copy_from_slice(&row);
            gpu.queue.write_buffer(&self.casters[slot].uniform, 0, &row);
        }
        gpu.queue.write_buffer(&self.uniform, 0, &bytes);
        self.matrices = maps.iter().map(|(m, _)| *m).collect();
        Ok(changed)
    }
    pub fn draw(
        &self,
        renderer: &SceneRenderer,
        encoder: &mut wgpu::CommandEncoder,
        draws: &[PreparedDraw],
        pipeline: &wgpu::RenderPipeline,
    ) -> (usize, u64) {
        let mut counts = (0, 0);
        for (slot, matrix) in self.matrices.iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("local shadow casters"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.layers[slot],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(1, &self.casters[slot].binding, &[]);
            let (draws, triangles) = renderer.draw_shadow_casters(&mut pass, draws, Some(*matrix));
            counts.0 += draws;
            counts.1 += triangles;
        }
        counts
    }
}
