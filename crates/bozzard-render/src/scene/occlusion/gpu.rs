use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

struct Targets {
    size: [u32; 2],
    depth: wgpu::TextureView,
    pyramid: wgpu::TextureView,
    reductions: Vec<(wgpu::BindGroup, [u32; 2])>,
    bytes: u64,
}
struct Slot {
    buffer: wgpu::Buffer,
    capacity: usize,
    state: Arc<AtomicU8>,
    frame: u64,
    generation: u64,
    tested: usize,
    reference: Vec<(u32, u32)>,
}
pub(super) struct Resources {
    depth_pipeline: wgpu::RenderPipeline,
    depth_uniform: wgpu::Buffer,
    depth_binding: wgpu::BindGroup,
    tiles: wgpu::ComputePipeline,
    reduce: wgpu::ComputePipeline,
    cull: wgpu::ComputePipeline,
    targets: Option<Targets>,
    candidates: wgpu::Buffer,
    pub arguments: wgpu::Buffer,
    count: wgpu::Buffer,
    cull_binding: Option<wgpu::BindGroup>,
    capacity: usize,
    slots: Vec<Slot>,
    pending: Option<usize>,
    pub result: Option<OcclusionResult>,
    pub visibility: Vec<bool>,
    pub visibility_generation: Option<u64>,
}
fn buffer(gpu: &Gpu, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage,
        mapped_at_creation: false,
    })
}
fn buffer_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
fn texture_entry(depth: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: if depth {
                wgpu::TextureSampleType::Depth
            } else {
                wgpu::TextureSampleType::Float { filterable: false }
            },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
fn compute(
    gpu: &Gpu,
    label: &str,
    text: &str,
    entries: &[wgpu::BindGroupLayoutEntry],
) -> wgpu::ComputePipeline {
    let bindings = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries,
        });
    let layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&bindings)],
            immediate_size: 0,
        });
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(text.into()),
        });
    gpu.device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        })
}
impl Resources {
    pub fn new(gpu: &Gpu) -> Self {
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("occluder transforms"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(80 * MAX_OCCLUDERS as u64),
                    },
                    count: None,
                }],
            });
        let depth_uniform = buffer(
            gpu,
            "occluder transforms",
            80 * MAX_OCCLUDERS as u64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let depth_binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("occluder transforms"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: depth_uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("occlusion depth layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("opaque occluder depth"),
                source: wgpu::ShaderSource::Wgsl(include_str!("depth.wgsl").into()),
            });
        let depth_pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("opaque occluder depth"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let storage = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::R32Float,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        };
        let tiles = compute(
            gpu,
            "occlusion depth tiles",
            include_str!("tiles.wgsl"),
            &[texture_entry(true), storage],
        );
        let reduce = compute(
            gpu,
            "occlusion maximum depth pyramid",
            include_str!("reduce.wgsl"),
            &[texture_entry(false), storage],
        );
        let cull = compute(
            gpu,
            "occlusion indirect culling",
            include_str!("cull.wgsl"),
            &[
                texture_entry(false),
                buffer_entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
                buffer_entry(2, wgpu::BufferBindingType::Storage { read_only: false }),
                buffer_entry(3, wgpu::BufferBindingType::Uniform),
            ],
        );
        let capacity = 64;
        Self {
            depth_pipeline,
            depth_uniform,
            depth_binding,
            tiles,
            reduce,
            cull,
            targets: None,
            candidates: buffer(
                gpu,
                "occlusion bounds",
                capacity as u64 * 32,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            ),
            arguments: buffer(
                gpu,
                "occlusion draw arguments",
                capacity as u64 * 20,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::INDIRECT
                    | wgpu::BufferUsages::COPY_SRC,
            ),
            count: buffer(
                gpu,
                "occlusion batch count",
                16,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            cull_binding: None,
            capacity,
            slots: Vec::new(),
            pending: None,
            result: None,
            visibility: Vec::new(),
            visibility_generation: None,
        }
    }
    pub fn prepare(&mut self, gpu: &Gpu, size: [u32; 2], candidates: &[u8]) {
        let count = candidates.len() / 32;
        if count > self.capacity {
            self.capacity = count.next_power_of_two();
            self.candidates = buffer(
                gpu,
                "occlusion bounds",
                self.capacity as u64 * 32,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            self.arguments = buffer(
                gpu,
                "occlusion draw arguments",
                self.capacity as u64 * 20,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::INDIRECT
                    | wgpu::BufferUsages::COPY_SRC,
            );
            self.cull_binding = None;
        }
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            self.targets = Some(Targets::new(gpu, size, &self.tiles, &self.reduce));
            self.cull_binding = None;
        }
        if self.cull_binding.is_none() {
            self.cull_binding = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("occlusion culling inputs"),
                layout: &self.cull.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &self.targets.as_ref().unwrap().pyramid,
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.candidates.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.arguments.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.count.as_entire_binding(),
                    },
                ],
            }));
        }
        gpu.queue.write_buffer(&self.candidates, 0, candidates);
        gpu.queue
            .write_buffer(&self.count, 0, &(count as u32).to_le_bytes());
        while self.slots.len() < 3 {
            self.slots.push(Slot {
                buffer: buffer(
                    gpu,
                    "occlusion diagnostics",
                    self.capacity as u64 * 20,
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                ),
                capacity: self.capacity,
                state: Arc::new(AtomicU8::new(0)),
                frame: 0,
                generation: 0,
                tested: 0,
                reference: Vec::new(),
            });
        }
        for slot in &mut self.slots {
            if slot.state.load(Ordering::Acquire) == 0 && slot.capacity < self.capacity {
                slot.buffer = buffer(
                    gpu,
                    "occlusion diagnostics",
                    self.capacity as u64 * 20,
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                );
                slot.capacity = self.capacity;
            }
        }
    }
    pub fn bytes(&self) -> u64 {
        self.targets.as_ref().map_or(0, |t| t.bytes)
            + self.capacity as u64 * 52
            + self
                .slots
                .iter()
                .map(|s| s.capacity as u64 * 20)
                .sum::<u64>()
            + 80 * MAX_OCCLUDERS as u64
            + 16
    }
    #[allow(clippy::too_many_arguments)]
    pub fn encode_depth(
        &self,
        renderer: &SceneRenderer,
        gpu: &Gpu,
        encoder: &mut crate::profiling::Encoder,
        vp: Mat4,
        draws: &[PreparedDraw],
        occluders: &[(usize, Projection)],
    ) {
        let mut uniforms = [0u8; 80 * MAX_OCCLUDERS];
        for (slot, (index, _)) in occluders.iter().enumerate() {
            let object = &draws[*index].object;
            let double_sided = double_sided(renderer, object);
            for (bytes, value) in uniforms[slot * 80..(slot + 1) * 80]
                .chunks_exact_mut(4)
                .zip((vp * object.model).to_cols_array().into_iter().chain([
                    if double_sided { 1. } else { 0. },
                    object.model.determinant().signum(),
                    0.,
                    0.,
                ]))
            {
                bytes.copy_from_slice(&value.to_le_bytes());
            }
        }
        gpu.queue
            .write_buffer(&self.depth_uniform, 0, &uniforms[..occluders.len() * 80]);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("occlusion opaque depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.targets.as_ref().unwrap().depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.depth_pipeline);
        pass.set_bind_group(0, &self.depth_binding, &[]);
        for (slot, (index, _)) in occluders.iter().enumerate() {
            let mesh = renderer.mesh_for(&draws[*index].object);
            pass.set_vertex_buffer(0, mesh.vertices.slice(mesh.vertex_offset..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, slot as u32..slot as u32 + 1);
        }
    }
    pub fn encode(
        &mut self,
        encoder: &mut crate::profiling::Encoder,
        count: usize,
        tested: usize,
        candidates: &[u8],
        generation: u64,
    ) {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("occlusion depth reduction and culling"),
                ..Default::default()
            });
            // Compute usage scopes are per dispatch, so each reduction can read
            // the preceding mip without creating a new compute pass/encoder.
            for (level, (binding, size)) in
                self.targets.as_ref().unwrap().reductions.iter().enumerate()
            {
                pass.set_pipeline(if level == 0 {
                    &self.tiles
                } else {
                    &self.reduce
                });
                pass.set_bind_group(0, binding, &[]);
                pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
            }
            pass.set_pipeline(&self.cull);
            pass.set_bind_group(0, self.cull_binding.as_ref().unwrap(), &[]);
            pass.dispatch_workgroups((count as u32).div_ceil(64), 1, 1);
        }
        if let Some(index) = self
            .slots
            .iter()
            .position(|s| s.state.load(Ordering::Acquire) == 0 && s.capacity >= count)
        {
            let slot = &mut self.slots[index];
            slot.frame = encoder.frame;
            slot.generation = generation;
            slot.tested = tested;
            slot.reference.clear();
            slot.reference.extend(candidates.chunks_exact(32).map(|c| {
                (
                    u32::from_le_bytes(c[20..24].try_into().unwrap()),
                    u32::from_le_bytes(c[24..28].try_into().unwrap()),
                )
            }));
            encoder.copy_buffer_to_buffer(&self.arguments, 0, &slot.buffer, 0, count as u64 * 20);
            self.pending = Some(index);
        }
    }
    pub fn submitted(&mut self) {
        if let Some(index) = self.pending.take() {
            let slot = &self.slots[index];
            let state = slot.state.clone();
            state.store(1, Ordering::Release);
            slot.buffer.map_async(
                wgpu::MapMode::Read,
                0..slot.reference.len() as u64 * 20,
                move |result| {
                    state.store(if result.is_ok() { 2 } else { 3 }, Ordering::Release);
                },
            );
        }
    }
    pub fn poll(&mut self, gpu: &Gpu) {
        // A previous frame may have failed before queue submission. This runs
        // even when culling is disabled for the new frame.
        self.pending = None;
        if self
            .slots
            .iter()
            .any(|slot| slot.state.load(Ordering::Acquire) == 1)
        {
            let _ = gpu.device.poll(wgpu::PollType::Poll);
        }
        for slot in &mut self.slots {
            let state = slot.state.load(Ordering::Acquire);
            if state == 2 {
                let mut result = OcclusionResult {
                    frame_id: slot.frame,
                    tested_batches: slot.tested,
                    ..Default::default()
                };
                let newest = self.result.is_none_or(|old| old.frame_id < slot.frame);
                {
                    let Ok(bytes) = slot
                        .buffer
                        .get_mapped_range(0..slot.reference.len() as u64 * 20)
                    else {
                        slot.buffer.unmap();
                        slot.state.store(0, Ordering::Release);
                        continue;
                    };
                    if newest {
                        self.visibility.clear();
                    }
                    for (draw, &(indices, instances)) in bytes.chunks_exact(20).zip(&slot.reference)
                    {
                        let visible = u32::from_le_bytes(draw[4..8].try_into().unwrap()) != 0;
                        if newest {
                            self.visibility.push(visible);
                        }
                        if !visible {
                            result.culled_batches += 1;
                            result.culled_surfaces += instances as usize;
                            result.skipped_triangles +=
                                u64::from(indices / 3) * u64::from(instances);
                        }
                    }
                }
                slot.buffer.unmap();
                if newest {
                    self.result = Some(result);
                    self.visibility_generation = Some(slot.generation);
                }
                slot.state.store(0, Ordering::Release);
            } else if state == 3 {
                slot.state.store(0, Ordering::Release);
            }
        }
    }
}
impl Targets {
    fn new(
        gpu: &Gpu,
        size: [u32; 2],
        tiles: &wgpu::ComputePipeline,
        reduce: &wgpu::ComputePipeline,
    ) -> Self {
        let depth = gpu
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("occlusion depth"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let base = [
            size[0].div_ceil(8).next_power_of_two(),
            size[1].div_ceil(8).next_power_of_two(),
        ];
        let levels = base[0].max(base[1]).ilog2() + 1;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("occlusion maximum depth pyramid"),
            size: wgpu::Extent3d {
                width: base[0],
                height: base[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let mut previous = depth.clone();
        let mut reductions = Vec::new();
        let mut bytes = u64::from(size[0]) * u64::from(size[1]) * 4;
        for level in 0..levels {
            let destination = texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let dimensions = [(base[0] >> level).max(1), (base[1] >> level).max(1)];
            bytes += u64::from(dimensions[0]) * u64::from(dimensions[1]) * 4;
            let pipeline = if level == 0 { tiles } else { reduce };
            let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("occlusion reduction level"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&previous),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&destination),
                    },
                ],
            });
            reductions.push((binding, dimensions));
            previous = destination;
        }
        Self {
            size,
            depth,
            pyramid: texture.create_view(&Default::default()),
            reductions,
            bytes,
        }
    }
}
