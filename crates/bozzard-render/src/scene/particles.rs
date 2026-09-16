#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleSimulation {
    pub epoch: u64,
    pub age: f32,
    pub reference_age: f32,
    pub time: f32,
    pub gravity: f32,
    pub drag: f32,
    pub turbulence: f32,
    pub wind: [f32; 3],
    pub speed: f32,
}

use super::*;
#[path = "particles/gpu.rs"]
mod gpu;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticleKind {
    Smoke,
    Ash,
    Sparks,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub simulation: Option<ParticleSimulation>,
    pub id: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub size: f32,
    pub rotation: f32,
    pub color: [f32; 3],
    pub opacity: f32,
    pub kind: ParticleKind,
    pub softness: f32,
    pub trail_length: f32,
    pub seed: f32,
}
impl Particle {
    pub fn validate(&self) -> Result<()> {
        if let Some(s) = self.simulation {
            ensure!(
                s.epoch > 0
                    && [
                        s.age,
                        s.reference_age,
                        s.time,
                        s.gravity,
                        s.drag,
                        s.turbulence,
                        s.speed
                    ]
                    .iter()
                    .chain(&s.wind)
                    .all(|v| v.is_finite())
                    && (0.0..=60.).contains(&s.age)
                    && (0.0..=s.age).contains(&s.reference_age)
                    && s.time >= 0.
                    && (-30.0..=30.).contains(&s.gravity)
                    && (0.0..=10.).contains(&s.drag)
                    && (0.0..=10.).contains(&s.turbulence)
                    && (0.0..=8.).contains(&s.speed)
                    && s.wind.iter().all(|v| v.abs() <= 100.),
                "invalid GPU particle simulation"
            );
        }
        ensure!(
            self.position.is_finite()
                && self.velocity.is_finite()
                && self.rotation.is_finite()
                && self.seed.is_finite(),
            "invalid particle pose"
        );
        ensure!(
            self.size.is_finite() && self.size > 0. && self.size <= 100_000.,
            "invalid particle size"
        );
        ensure!(
            self.opacity.is_finite()
                && (0.0..=1.).contains(&self.opacity)
                && self
                    .color
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
            "invalid particle color"
        );
        ensure!(
            self.softness.is_finite()
                && (0.001..=10.).contains(&self.softness)
                && self.trail_length.is_finite()
                && (0.0..=1.).contains(&self.trail_length),
            "invalid particle edge or trail"
        );
        Ok(())
    }
}
pub(super) struct Particles {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
    simulation: gpu::Simulation,
    count: u32,
    size: [u32; 2],
}
fn shader_source() -> String {
    [
        include_str!("shadow_sample.wgsl"),
        include_str!("local_lights.wgsl"),
        include_str!("particles.wgsl"),
    ]
    .join("\n")
}
impl Particles {
    pub fn new(
        gpu: &Gpu,
        shadows: &wgpu::BindGroupLayout,
        depth: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Self {
        let simulation = gpu::Simulation::new(gpu);
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("soft particle inputs"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(240),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("particle light layout"),
                bind_group_layouts: &[Some(&layout), None, Some(shadows)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("smoke ash and luminous trails"),
                source: wgpu::ShaderSource::Wgsl(shader_source().into()),
            });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sorted soft particles"),
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
                    targets: &geometry::color_targets(wgpu::TextureFormat::Rgba16Float, true, true),
                }),
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle camera and lighting"),
            size: 240,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = Self::binding(gpu, &layout, &uniform, depth, &simulation.output);
        Self {
            pipeline,
            uniform,
            binding,
            simulation,
            count: 0,
            size,
        }
    }
    fn binding(
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::Buffer,
        depth: &wgpu::TextureView,
        instances: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle depth binding"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: instances.as_entire_binding(),
                },
            ],
        })
    }
    pub fn invalidate_depth(&mut self) {
        self.size = [0, 0];
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        scene: &RenderScene,
        view_projection: Mat4,
        depth: &wgpu::TextureView,
        size: [u32; 2],
        transparent_depths: &[f32],
    ) -> Result<()> {
        ensure!(
            scene.particles.len() <= 16_384,
            "particle frame budget exceeded"
        );
        if self.size != size {
            self.binding = Self::binding(
                gpu,
                &self.pipeline.get_bind_group_layout(0),
                &self.uniform,
                depth,
                &self.simulation.output,
            );
            self.size = size;
        }
        let inverse = view_projection.inverse();
        let origin = inverse.project_point3(Vec3::ZERO);
        let right = (inverse.project_point3(Vec3::X) - origin).normalize();
        let up = (inverse.project_point3(Vec3::Y) - origin).normalize();
        let forward = (inverse.project_point3(Vec3::new(0., 0., 0.5)) - origin).normalize();
        let light = scene.lighting;
        let sun = Vec3::from(light.sun_direction).normalize();
        let ambient: [f32; 3] = std::array::from_fn(|i| {
            light.ambient_color[i] * light.ambient_intensity
                + scene.environment.horizon[i] * scene.environment.intensity * 0.25
        });
        gpu.queue.write_buffer(
            &self.uniform,
            0,
            &float_bytes(
                scene
                    .view_projection
                    .to_cols_array()
                    .into_iter()
                    .chain(inverse.to_cols_array())
                    .chain([
                        right.x,
                        right.y,
                        right.z,
                        0.,
                        up.x,
                        up.y,
                        up.z,
                        0.,
                        forward.x,
                        forward.y,
                        forward.z,
                        0.,
                        sun.x,
                        sun.y,
                        sun.z,
                        light.sun_intensity,
                        light.sun_color[0],
                        light.sun_color[1],
                        light.sun_color[2],
                        0.,
                        ambient[0],
                        ambient[1],
                        ambient[2],
                        0.,
                        size[0] as f32,
                        size[1] as f32,
                        0.,
                        0.,
                    ]),
            ),
        );
        self.count = scene.particles.len() as u32;
        self.simulation.prepare(
            gpu,
            &scene.particles,
            scene.view_projection,
            transparent_depths,
        )
    }
    pub fn work(&self) -> (u32, usize) {
        self.simulation.work()
    }
    pub fn submitted(&mut self) {
        self.simulation.submitted();
    }
    pub fn encode(&self, encoder: &mut crate::profiling::Encoder) {
        self.simulation.encode(encoder);
    }
    pub fn draw_bucket(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        shadows: &wgpu::BindGroup,
        index: u32,
    ) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.binding, &[]);
        pass.set_bind_group(2, shadows, &[]);
        pass.draw_indirect(&self.simulation.indirect, index as u64 * 16);
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn particle_shader_validates() {
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
