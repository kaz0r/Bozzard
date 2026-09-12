use super::*;

/// GPU-resident exposure state, shared as a uniform with the final display pass.
pub(super) struct Exposure {
    pub state: wgpu::Buffer,
    pipelines: Option<Pipelines>,
    previous_time: Option<f32>,
    active: bool,
}
struct Pipelines {
    histogram: wgpu::ComputePipeline,
    adapt: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    bins: wgpu::Buffer,
    binding: Option<wgpu::BindGroup>,
}
impl Exposure {
    pub fn new(gpu: &Gpu) -> Self {
        let state = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("GPU exposure state"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::UNIFORM
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue
            .write_buffer(&state, 0, &float_bytes([1., 0., 0., 0.]));
        Self {
            state,
            pipelines: None,
            previous_time: None,
            active: false,
        }
    }
    pub fn reset(&mut self) {
        self.previous_time = None;
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        source: &wgpu::TextureView,
        display: DisplaySettings,
        raw: bool,
        source_changed: bool,
    ) {
        let settings = display.auto_exposure;
        let active = !raw && settings.enabled && settings.strength > 0.;
        if !active {
            if self.active {
                gpu.queue
                    .write_buffer(&self.state, 0, &float_bytes([1., 0., 0., 0.]));
            }
            self.active = false;
            self.previous_time = None;
            if let Some(pipelines) = &mut self.pipelines {
                pipelines.binding = None;
            }
            return;
        }
        let time = display.time_seconds;
        // Edit previews (time zero), first use, rewinds and explicit cuts meter immediately.
        // Equal nonzero simulation times hold adaptation during pause and repeated draws.
        let reset = time == 0. || self.previous_time.is_none_or(|previous| time < previous);
        let dt = self
            .previous_time
            .map_or(0., |previous| (time - previous).clamp(0., 10.));
        let pipelines = self.pipelines.get_or_insert_with(|| Pipelines::new(gpu));
        if source_changed || pipelines.binding.is_none() {
            pipelines.binding = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("exposure metering source"),
                layout: &pipelines.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: pipelines.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: pipelines.bins.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.state.as_entire_binding(),
                    },
                ],
            }));
        }
        gpu.queue.write_buffer(
            &pipelines.uniform,
            0,
            &float_bytes([
                settings.min_ev,
                settings.max_ev,
                settings.target_gray,
                settings.strength,
                settings.speed_up,
                settings.speed_down,
                dt,
                if reset { 1. } else { 0. },
                settings.center_weight,
                0.,
                0.,
                0.,
            ]),
        );
        self.previous_time = Some(time);
        self.active = true;
    }
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder) {
        if !self.active {
            return;
        }
        let pipelines = self.pipelines.as_ref().unwrap();
        encoder.clear_buffer(&pipelines.bins, 0, None);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("histogram eye adaptation"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, pipelines.binding.as_ref().unwrap(), &[]);
        pass.set_pipeline(&pipelines.histogram);
        pass.dispatch_workgroups(16, 16, 1);
        pass.set_pipeline(&pipelines.adapt);
        pass.dispatch_workgroups(1, 1, 1);
    }
}
impl Pipelines {
    fn new(gpu: &Gpu) -> Self {
        let buffer = |binding, ty, size| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(size),
            },
            count: None,
        };
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("exposure compute inputs"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    buffer(1, wgpu::BufferBindingType::Uniform, 48),
                    buffer(
                        2,
                        wgpu::BufferBindingType::Storage { read_only: false },
                        1024,
                    ),
                    buffer(3, wgpu::BufferBindingType::Storage { read_only: false }, 16),
                ],
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("exposure layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("histogram exposure shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("auto_exposure.wgsl").into()),
            });
        let pipeline = |entry| {
            gpu.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        };
        Self {
            histogram: pipeline("histogram"),
            adapt: pipeline("adapt"),
            layout,
            uniform: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("auto exposure controls"),
                size: 48,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            bins: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("luminance histogram"),
                size: 1024,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            binding: None,
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn exposure_shader_validates() {
        let source = include_str!("auto_exposure.wgsl");
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
