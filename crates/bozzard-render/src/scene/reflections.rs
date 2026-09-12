use super::*;
pub(super) struct Reflections {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    target: Option<(wgpu::TextureView, [u32; 2])>,
    binding: Option<wgpu::BindGroup>,
    active: bool,
}
fn shader_source() -> String {
    [
        include_str!("environment_sample.wgsl"),
        include_str!("fog.wgsl"),
        include_str!("reflections.wgsl"),
    ]
    .join("\n")
}
impl Reflections {
    pub fn new(gpu: &Gpu, environment: &wgpu::BindGroupLayout) -> Self {
        let mut entries = Vec::new();
        for i in 0..4 {
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
            binding: 4,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 5,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(208),
            },
            count: None,
        });
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("reflection inputs"),
                entries: &entries,
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("reflection pipeline layout"),
                bind_group_layouts: &[Some(&layout), None, None, Some(environment)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("screen space reflections"),
                source: wgpu::ShaderSource::Wgsl(shader_source().into()),
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("screen space reflections"),
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
        Self {
            layout,
            pipeline,
            uniform: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("reflection controls"),
                size: 208,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            target: None,
            binding: None,
            active: false,
        }
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        source: &wgpu::TextureView,
        settings: ScreenSpaceReflections,
        frame: &post_process::FrameInput<'_>,
        source_changed: bool,
    ) -> bool {
        let old = self.active;
        self.active =
            !frame.raw && settings.enabled && settings.strength > 0. && frame.geometry.is_some();
        if !self.active {
            return old;
        }
        let resized = self
            .target
            .as_ref()
            .is_none_or(|(_, size)| *size != frame.size);
        if resized {
            self.target = Some((
                geometry::color_texture(gpu, frame.size, "reflected HDR scene"),
                frame.size,
            ));
        }
        if resized || source_changed || !old {
            let g = frame.geometry.unwrap();
            let views = [source, frame.depth.unwrap(), &g.normal, &g.specular];
            let mut entries: Vec<_> = views
                .into_iter()
                .enumerate()
                .map(|(i, view)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: wgpu::BindingResource::TextureView(view),
                })
                .collect();
            entries.push(wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 5,
                resource: self.uniform.as_entire_binding(),
            });
            self.binding = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("reflection frame inputs"),
                layout: &self.layout,
                entries: &entries,
            }));
        }
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            &float_bytes(
                frame
                    .view_projection
                    .to_cols_array()
                    .into_iter()
                    .chain(frame.view_projection.inverse().to_cols_array())
                    .chain([
                        frame.size[0] as f32,
                        frame.size[1] as f32,
                        settings.roughness_cutoff,
                        settings.strength,
                        settings.steps as f32,
                        settings.max_distance,
                        settings.thickness,
                        0.,
                    ])
                    .chain(frame.fog.uniform(false)),
            ),
        );
        resized || !old
    }
    pub fn output(&self) -> Option<&wgpu::TextureView> {
        self.active.then(|| &self.target.as_ref().unwrap().0)
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, environment: Option<&wgpu::BindGroup>) {
        if !self.active {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("depth traced material reflections"),
            color_attachments: &[geometry::attachment(
                &self.target.as_ref().unwrap().0,
                wgpu::Color::TRANSPARENT,
            )],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, self.binding.as_ref().unwrap(), &[]);
        pass.set_bind_group(3, environment.unwrap(), &[]);
        pass.draw(0..3, 0..1);
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn reflections_shader_validates() {
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
