use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParticleKind {
    Smoke,
    Ash,
    Sparks,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
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
    instances: wgpu::Buffer,
    capacity: usize,
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
        let pipeline=gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {label:Some("soft particles"),layout:Some(&pipeline_layout),vertex:wgpu::VertexState {module:&shader,entry_point:Some("vs_main"),compilation_options:Default::default(),buffers:&[Some(wgpu::VertexBufferLayout {array_stride:64,step_mode:wgpu::VertexStepMode::Instance,attributes:&wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4,3=>Float32x4]})]},fragment:Some(wgpu::FragmentState {module:&shader,entry_point:Some("fs_main"),compilation_options:Default::default(),targets:&[geometry::color_targets(wgpu::TextureFormat::Rgba16Float,true)[0].clone(),geometry::color_targets(wgpu::TextureFormat::Rgba16Float,true)[2].clone()]}),primitive:Default::default(),depth_stencil:None,multisample:Default::default(),multiview_mask:None,cache:None});
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle camera and lighting"),
            size: 240,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = Self::binding(gpu, &layout, &uniform, depth);
        let instances = Self::buffer(gpu, 1);
        Self {
            pipeline,
            uniform,
            binding,
            instances,
            capacity: 1,
            count: 0,
            size,
        }
    }
    fn buffer(gpu: &Gpu, capacity: usize) -> wgpu::Buffer {
        gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle instances"),
            size: (capacity * 64) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    fn binding(
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::Buffer,
        depth: &wgpu::TextureView,
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
    ) -> Result<()> {
        ensure!(
            scene.particles.len() <= 16_384,
            "particle frame budget exceeded"
        );
        for p in &scene.particles {
            p.validate()?;
        }
        if self.size != size {
            self.binding = Self::binding(
                gpu,
                &self.pipeline.get_bind_group_layout(0),
                &self.uniform,
                depth,
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
        let mut particles: Vec<_> = scene.particles.iter().collect();
        particles.sort_by(|a, b| {
            forward
                .dot(b.position)
                .total_cmp(&forward.dot(a.position))
                .then(a.id.cmp(&b.id))
        });
        self.count = particles.len() as u32;
        if particles.len() > self.capacity {
            self.capacity = particles.len().next_power_of_two();
            self.instances = Self::buffer(gpu, self.capacity);
        }
        if particles.is_empty() {
            return Ok(());
        }
        gpu.queue.write_buffer(
            &self.instances,
            0,
            &float_bytes(particles.into_iter().flat_map(|p| {
                [
                    p.position.x,
                    p.position.y,
                    p.position.z,
                    p.size,
                    p.velocity.x,
                    p.velocity.y,
                    p.velocity.z,
                    p.rotation,
                    p.color[0],
                    p.color[1],
                    p.color[2],
                    p.opacity,
                    match p.kind {
                        ParticleKind::Smoke => 0.,
                        ParticleKind::Ash => 1.,
                        ParticleKind::Sparks => 2.,
                    },
                    p.softness,
                    p.trail_length,
                    p.seed,
                ]
            })),
        );
        Ok(())
    }
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        hdr: &wgpu::TextureView,
        motion: &wgpu::TextureView,
        shadows: &wgpu::BindGroup,
    ) {
        if self.count == 0 {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lit smoke ash and spark trails"),
            color_attachments: &[hdr, motion].map(|view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.binding, &[]);
        pass.set_bind_group(2, shadows, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..6, 0..self.count);
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
