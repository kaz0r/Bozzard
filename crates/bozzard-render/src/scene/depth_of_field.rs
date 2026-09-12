use super::*;
use post_process::FrameInput;

struct Targets {
    size: [u32; 2],
    prefiltered: wgpu::TextureView,
    far: wgpu::TextureView,
    near: wgpu::TextureView,
    color: wgpu::TextureView,
    bindings: [wgpu::BindGroup; 3],
}
pub(super) struct Dof {
    pipelines: [wgpu::RenderPipeline; 3],
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
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
impl Dof {
    pub fn new(gpu: &Gpu) -> Self {
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
                label: Some("depth of field inputs"),
                entries: &[
                    texture_binding(0, wgpu::TextureSampleType::Float { filterable: true }),
                    texture_binding(1, wgpu::TextureSampleType::Depth),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(112),
                        },
                        count: None,
                    },
                    texture_binding(3, wgpu::TextureSampleType::Float { filterable: true }),
                    texture_binding(4, wgpu::TextureSampleType::Float { filterable: true }),
                    texture_binding(5, wgpu::TextureSampleType::Float { filterable: true }),
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("camera lens layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("bokeh depth of field"),
                source: wgpu::ShaderSource::Wgsl(include_str!("depth_of_field.wgsl").into()),
            });
        let pipeline = |entry, count| {
            let targets = vec![
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL
                });
                count
            ];
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
                        targets: &targets,
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        Self {
            pipelines: [
                pipeline("prefilter_main", 1),
                pipeline("gather_main", 2),
                pipeline("composite_main", 1),
            ],
            layout,
            uniform: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("camera lens controls"),
                size: 112,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("bokeh linear clamp"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            dummy: texture(gpu, [1, 1], "unused bokeh input"),
            targets: None,
        }
    }
    fn binding(
        &self,
        gpu: &Gpu,
        hdr: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        layers: [&wgpu::TextureView; 3],
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bokeh textures"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(hdr),
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
                    resource: wgpu::BindingResource::TextureView(layers[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(layers[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(layers[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        hdr: &wgpu::TextureView,
        settings: DepthOfField,
        frame: &FrameInput<'_>,
        source_changed: bool,
    ) -> Result<bool> {
        if frame.raw || !settings.enabled || settings.max_blur_radius == 0. {
            return Ok(self.targets.take().is_some());
        }
        let depth = frame.depth.context("depth of field requires scene depth")?;
        let changed = self.targets.as_ref().is_none_or(|t| t.size != frame.size);
        if changed {
            let half = frame.size.map(|v| v.div_ceil(2));
            let prefiltered = texture(gpu, half, "half-resolution color and circle of confusion");
            let far = texture(gpu, half, "far bokeh");
            let near = texture(gpu, half, "near bokeh and coverage");
            let color = texture(gpu, frame.size, "HDR after depth of field");
            let bindings = [
                self.binding(gpu, hdr, depth, [&self.dummy; 3]),
                self.binding(gpu, hdr, depth, [&prefiltered, &self.dummy, &self.dummy]),
                self.binding(gpu, hdr, depth, [&self.dummy, &far, &near]),
            ];
            self.targets = Some(Targets {
                size: frame.size,
                prefiltered,
                far,
                near,
                color,
                bindings,
            });
        }
        if source_changed && !changed {
            let t = self.targets.as_ref().unwrap();
            let bindings = [
                self.binding(gpu, hdr, depth, [&self.dummy; 3]),
                self.binding(gpu, hdr, depth, [&t.prefiltered, &self.dummy, &self.dummy]),
                self.binding(gpu, hdr, depth, [&self.dummy, &t.far, &t.near]),
            ];
            self.targets.as_mut().unwrap().bindings = bindings;
        }
        let inverse = frame.view_projection.inverse();
        let origin = inverse.project_point3(Vec3::ZERO);
        let forward = (inverse.project_point3(Vec3::new(0., 0., 0.5)) - origin).normalize();
        let focal = settings.focal_length_mm * 0.001;
        let scale = focal * focal / (settings.aperture * (settings.focus_distance - focal))
            * frame.size[1] as f32
            / (4. * 0.024);
        let radius = settings.max_blur_radius * frame.size[1] as f32 / 2160.;
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            &float_bytes(inverse.to_cols_array().into_iter().chain([
                origin.x,
                origin.y,
                origin.z,
                0.,
                forward.x,
                forward.y,
                forward.z,
                0.,
                settings.focus_distance,
                scale,
                radius,
                0.,
            ])),
        );
        Ok(changed)
    }
    pub fn output(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.color)
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(t) = &self.targets else {
            return;
        };
        for (i, views) in [vec![&t.prefiltered], vec![&t.far, &t.near], vec![&t.color]]
            .into_iter()
            .enumerate()
        {
            let attachments: Vec<_> = views
                .into_iter()
                .map(|view| {
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })
                })
                .collect();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bokeh depth of field"),
                color_attachments: &attachments,
                ..Default::default()
            });
            pass.set_pipeline(&self.pipelines[i]);
            pass.set_bind_group(0, &t.bindings[i], &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn bokeh_shader_validates() {
        let source = include_str!("depth_of_field.wgsl");
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
