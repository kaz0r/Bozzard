use super::*;

pub(super) struct Plan {
    pub static_mask: Vec<bool>,
    pub dynamic_mask: Vec<bool>,
    pub rebuild: bool,
}
#[derive(Default)]
pub(super) struct Cache {
    entry: Option<Entry>,
}
struct Entry {
    depth: wgpu::TextureView,
    binding: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    resolution: u32,
    row: Vec<u8>,
    casters: Vec<shadows::ShadowCaster>,
    valid: bool,
}
impl Cache {
    pub fn clear(&mut self) {
        self.entry = None;
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        static_mask: Vec<bool>,
        row: &[u8],
        resolution: u32,
    ) -> Option<Plan> {
        let count = static_mask.iter().filter(|v| **v).count();
        let dynamic_count = draws
            .iter()
            .enumerate()
            .filter(|(i, d)| !d.transparent && d.object.material.lit && !static_mask[*i])
            .count();
        // A full-depth copy can cost more than a small instanced scene. Require
        // substantial geometry and at least 32 static groups before allocating.
        let groups = batches
            .iter()
            .filter(|b| b.indices.iter().any(|i| static_mask[*i]))
            .count();
        if resolution == 1
            || count < 64
            || count <= dynamic_count
            || dynamic_count == 0
            || groups < 32
        {
            return None;
        }
        if self
            .entry
            .as_ref()
            .is_none_or(|e| e.resolution != resolution)
        {
            self.entry = Some(Entry::new(gpu, resolution));
        }
        let entry = self.entry.as_mut().unwrap();
        let matches = entry.valid
            && entry.row == row
            && entry.casters.len() == count
            && entry
                .casters
                .iter()
                .zip(
                    draws
                        .iter()
                        .zip(&static_mask)
                        .filter(|(_, s)| **s)
                        .map(|(d, _)| d),
                )
                .all(|(c, d)| c.matches(d));
        if !matches {
            entry.valid = false;
        }
        let dynamic_mask = draws
            .iter()
            .enumerate()
            .map(|(i, d)| !d.transparent && d.object.material.lit && !static_mask[i])
            .collect();
        Some(Plan {
            static_mask,
            dynamic_mask,
            rebuild: !matches,
        })
    }
    pub fn depth(&self) -> &wgpu::TextureView {
        &self.entry.as_ref().unwrap().depth
    }
    pub fn copy(&self, pass: &mut wgpu::RenderPass<'_>) {
        let entry = self.entry.as_ref().unwrap();
        pass.set_pipeline(&entry.pipeline);
        pass.set_bind_group(0, &entry.binding, &[]);
        pass.draw(0..3, 0..1);
    }
    pub fn finish(&mut self, plan: &Plan, draws: &[PreparedDraw], row: &[u8]) {
        if plan.rebuild {
            let entry = self.entry.as_mut().unwrap();
            entry.row = row.to_vec();
            entry.casters = draws
                .iter()
                .zip(&plan.static_mask)
                .filter(|(_, s)| **s)
                .map(|(d, _)| shadows::ShadowCaster::new(d))
                .collect();
            entry.valid = true;
        }
    }
}
impl Entry {
    fn new(gpu: &Gpu, resolution: u32) -> Self {
        let depth = shadows::target(gpu, resolution);
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("sun cached depth source"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
        let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sun cached depth source"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&depth),
            }],
        });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sun cached depth copy"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shadow_copy.wgsl").into()),
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sun cached depth copy"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sun cached depth copy"),
                layout: Some(&pipeline_layout),
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
                    targets: &[],
                }),
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        Self {
            depth,
            binding,
            pipeline,
            resolution,
            row: Vec::new(),
            casters: Vec::new(),
            valid: false,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn depth_copy_shader_validates() {
        let source = include_str!("shadow_copy.wgsl");
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
