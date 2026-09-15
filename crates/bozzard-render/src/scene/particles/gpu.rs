//! Bounded GPU motion, stable back-to-front sorting, and indirect batches between transparent surfaces.
use super::*;
use std::collections::{BTreeMap, HashSet};
const LIMIT: usize = 16_384;
const MESH_LIMIT: usize = 16_384;
fn buffer(gpu: &Gpu, name: &str, bytes: usize, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(name),
        size: bytes.max(16) as u64,
        usage,
        mapped_at_creation: false,
    })
}
fn pipeline(gpu: &Gpu, name: &str, source: &str, entry: &str) -> wgpu::ComputePipeline {
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    gpu.device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(name),
            layout: None,
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
}
fn binding(
    gpu: &Gpu,
    pipeline: &wgpu::ComputePipeline,
    buffers: &[&wgpu::Buffer],
) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("particle compute buffers"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>(),
    })
}
pub(super) struct Simulation {
    simulate: wgpu::ComputePipeline,
    simulate_binding: wgpu::BindGroup,
    sort: wgpu::ComputePipeline,
    sort_binding: wgpu::BindGroup,
    gather: wgpu::ComputePipeline,
    gather_binding: wgpu::BindGroup,
    buckets: wgpu::ComputePipeline,
    bucket_binding: wgpu::BindGroup,
    input: wgpu::Buffer,
    camera: wgpu::Buffer,
    counts: wgpu::Buffer,
    depths: wgpu::Buffer,
    stages: wgpu::Buffer,
    stride: u64,
    stage_count: u32,
    pub output: wgpu::Buffer,
    pub indirect: wgpu::Buffer,
    slots: BTreeMap<(u64, u64), u32>,
    free: Vec<u32>,
    live: HashSet<(u64, u64)>,
    bytes: Vec<u8>,
    dirty: bool,
    needs_submission: bool,
    previous: Vec<Particle>,
    previous_matrix: Mat4,
    previous_depths: Vec<f32>,
    count: u32,
    sorted_count: u32,
    meshes: u32,
}
impl Simulation {
    pub fn new(gpu: &Gpu) -> Self {
        use wgpu::BufferUsages as U;
        let input = buffer(
            gpu,
            "particle descriptors",
            LIMIT * 128,
            U::STORAGE | U::COPY_DST,
        );
        let state = buffer(
            gpu,
            "particle motion state",
            LIMIT * 32,
            U::STORAGE | U::COPY_SRC,
        );
        let unsorted = buffer(gpu, "simulated particle instances", LIMIT * 64, U::STORAGE);
        let records = buffer(gpu, "particle depth order", LIMIT * 16, U::STORAGE);
        let output = buffer(
            gpu,
            "sorted particle instances",
            LIMIT * 64,
            U::STORAGE | U::COPY_SRC,
        );
        let indirect = buffer(
            gpu,
            "transparent particle intervals",
            (MESH_LIMIT + 1) * 16,
            U::STORAGE | U::INDIRECT | U::COPY_SRC,
        );
        let camera = buffer(
            gpu,
            "particle simulation camera",
            80,
            U::UNIFORM | U::COPY_DST,
        );
        let counts = buffer(gpu, "particle draw counts", 16, U::UNIFORM | U::COPY_DST);
        let depths = buffer(
            gpu,
            "transparent surface depths",
            MESH_LIMIT * 4,
            U::STORAGE | U::COPY_DST,
        );
        let simulate = pipeline(
            gpu,
            "particle motion",
            include_str!("simulation.wgsl"),
            "simulate",
        );
        let simulate_binding = binding(
            gpu,
            &simulate,
            &[&input, &state, &unsorted, &records, &camera],
        );
        let gather = pipeline(
            gpu,
            "particle instance gather",
            include_str!("gather.wgsl"),
            "gather",
        );
        let gather_binding = binding(gpu, &gather, &[&records, &unsorted, &output, &counts]);
        let buckets = pipeline(
            gpu,
            "particle transparent intervals",
            include_str!("buckets.wgsl"),
            "buckets",
        );
        let bucket_binding = binding(gpu, &buckets, &[&records, &depths, &indirect, &counts]);
        let stride = u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment).max(16);
        let stages = buffer(
            gpu,
            "bitonic sort stages",
            stride as usize * 105,
            U::UNIFORM | U::COPY_DST,
        );
        let sort_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("particle sort layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: wgpu::BufferSize::new(16),
                        },
                        count: None,
                    },
                ],
            });
        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("particle sort"),
                bind_group_layouts: &[Some(&sort_layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("particle bitonic sort"),
                source: wgpu::ShaderSource::Wgsl(include_str!("sort.wgsl").into()),
            });
        let sort = gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("particle bitonic sort"),
                layout: Some(&layout),
                module: &shader,
                entry_point: Some("sort"),
                compilation_options: Default::default(),
                cache: None,
            });
        let sort_binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle sort buffers"),
            layout: &sort_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: records.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &stages,
                        offset: 0,
                        size: wgpu::BufferSize::new(16),
                    }),
                },
            ],
        });
        Self {
            simulate,
            simulate_binding,
            sort,
            sort_binding,
            gather,
            gather_binding,
            buckets,
            bucket_binding,
            input,
            camera,
            counts,
            depths,
            stages,
            stride,
            stage_count: 0,
            output,
            indirect,
            slots: BTreeMap::new(),
            free: (0..LIMIT as u32).rev().collect(),
            live: HashSet::new(),
            bytes: Vec::new(),
            dirty: true,
            needs_submission: true,
            previous: Vec::new(),
            previous_matrix: Mat4::ZERO,
            previous_depths: Vec::new(),
            count: 0,
            sorted_count: 0,
            meshes: 0,
        }
    }
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        particles: &[Particle],
        matrix: Mat4,
        depths: &[f32],
    ) -> Result<()> {
        ensure!(
            particles.len() <= LIMIT && depths.len() <= MESH_LIMIT,
            "particle or transparency budget exceeded"
        );
        self.dirty = self.needs_submission
            || self.previous != particles
            || self.previous_matrix != matrix
            || self.previous_depths != depths;
        if !self.dirty {
            return Ok(());
        }
        self.live.clear();
        for p in particles {
            p.validate()?;
            if let Some(s) = p.simulation {
                ensure!(
                    self.live.insert((s.epoch, p.id)),
                    "duplicate GPU particle identity"
                );
            }
        }
        self.slots.retain(|key, slot| {
            if self.live.contains(key) {
                true
            } else {
                self.free.push(*slot);
                false
            }
        });
        self.bytes.clear();
        self.bytes.reserve(particles.len() * 128);
        for p in particles {
            let (slot, reset) = if let Some(s) = p.simulation {
                if let Some(slot) = self.slots.get(&(s.epoch, p.id)) {
                    (*slot, false)
                } else {
                    let slot = self.free.pop().context("particle slot budget exhausted")?;
                    self.slots.insert((s.epoch, p.id), slot);
                    (slot, true)
                }
            } else {
                (0, false)
            };
            let source = [
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
            ];
            self.bytes
                .extend(source.into_iter().flat_map(f32::to_le_bytes));
            let sim = p.simulation.map_or([0.; 12], |s| {
                [
                    s.age,
                    s.reference_age,
                    s.time,
                    s.gravity,
                    s.drag,
                    s.turbulence,
                    s.speed,
                    0.,
                    s.wind[0],
                    s.wind[1],
                    s.wind[2],
                    if reset { 1. } else { 0. },
                ]
            });
            self.bytes
                .extend(sim.into_iter().flat_map(f32::to_le_bytes));
            self.bytes.extend(
                [
                    slot,
                    p.id as u32,
                    (p.id >> 32) as u32,
                    u32::from(p.simulation.is_some()),
                ]
                .into_iter()
                .flat_map(u32::to_le_bytes),
            );
        }
        self.count = particles.len() as u32;
        self.meshes = depths.len() as u32;
        if !self.bytes.is_empty() {
            gpu.queue.write_buffer(&self.input, 0, &self.bytes);
        }
        let sorted_count = particles.len().max(1).next_power_of_two() as u32;
        if sorted_count != self.sorted_count {
            self.sorted_count = sorted_count;
            let mut bytes = Vec::new();
            self.stage_count = 0;
            let mut k = 2;
            while k <= sorted_count {
                let mut j = k / 2;
                while j > 0 {
                    let offset = bytes.len();
                    bytes.resize(offset + self.stride as usize, 0);
                    for (i, value) in [k, j, sorted_count, 0].into_iter().enumerate() {
                        bytes[offset + i * 4..offset + i * 4 + 4]
                            .copy_from_slice(&value.to_le_bytes());
                    }
                    self.stage_count += 1;
                    j /= 2;
                }
                k *= 2;
            }
            if !bytes.is_empty() {
                gpu.queue.write_buffer(&self.stages, 0, &bytes);
            }
        }
        let mut camera = float_bytes(matrix.to_cols_array());
        camera.extend(
            [self.count, self.sorted_count, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes),
        );
        gpu.queue.write_buffer(&self.camera, 0, &camera);
        gpu.queue.write_buffer(
            &self.counts,
            0,
            &[self.count, self.meshes, 0, 0]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        if !depths.is_empty() {
            gpu.queue.write_buffer(
                &self.depths,
                0,
                &float_bytes(
                    depths
                        .iter()
                        .map(|v| if v.is_finite() { *v } else { -f32::MAX }),
                ),
            );
        }
        self.needs_submission = true;
        self.previous.clear();
        self.previous.extend_from_slice(particles);
        self.previous_matrix = matrix;
        self.previous_depths.clear();
        self.previous_depths.extend_from_slice(depths);
        Ok(())
    }
    pub fn work(&self) -> (u32, usize) {
        if self.dirty {
            (3 + self.stage_count, self.bytes.len())
        } else {
            (0, 0)
        }
    }
    pub fn submitted(&mut self) {
        self.needs_submission = false;
    }
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        if !self.dirty {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("particle simulation and transparent sorting"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.simulate);
        pass.set_bind_group(0, &self.simulate_binding, &[]);
        pass.dispatch_workgroups(self.sorted_count.div_ceil(64), 1, 1);
        pass.set_pipeline(&self.sort);
        for stage in 0..self.stage_count {
            pass.set_bind_group(
                0,
                &self.sort_binding,
                &[(stage as u64 * self.stride) as u32],
            );
            pass.dispatch_workgroups(self.sorted_count.div_ceil(64), 1, 1);
        }
        pass.set_pipeline(&self.gather);
        pass.set_bind_group(0, &self.gather_binding, &[]);
        pass.dispatch_workgroups(self.count.max(1).div_ceil(64), 1, 1);
        pass.set_pipeline(&self.buckets);
        pass.set_bind_group(0, &self.bucket_binding, &[]);
        pass.dispatch_workgroups((self.meshes + 1).div_ceil(64), 1, 1);
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn compute_shaders_validate_on_baseline_limits() {
        for source in [
            include_str!("simulation.wgsl"),
            include_str!("sort.wgsl"),
            include_str!("gather.wgsl"),
            include_str!("buckets.wgsl"),
        ] {
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
}
