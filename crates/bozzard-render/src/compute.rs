//! Native, reusable compute resources. Receives validated presentation commands, never an ECS.
use crate::{Gpu, GpuFrameTiming, profiling::GpuProfiler, wgpu};
use anyhow::{Context, Result, bail, ensure};
use bozzard_compute::{
    Batch, BindingKind, Capabilities, Command, CompletionSink, Handle, Kernel, Resource,
    ResourceKind, Runtime, Submission, TextureFormat, Ticket,
};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context as TaskContext, Poll, Waker},
};

const MAX_PIPELINES: usize = 128;
const MAX_BIND_GROUPS: usize = 256;
const UPLOAD_SLOTS: usize = 3;
static NEXT_DEVICE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
enum Allocation {
    Buffer(wgpu::Buffer),
    Texture {
        texture: wgpu::Texture,
        view: wgpu::TextureView,
    },
    Sampler(wgpu::Sampler),
}
impl Allocation {
    fn binding(&self) -> wgpu::BindingResource<'_> {
        match self {
            Self::Buffer(buffer) => buffer.as_entire_binding(),
            Self::Texture { view, .. } => wgpu::BindingResource::TextureView(view),
            Self::Sampler(sampler) => wgpu::BindingResource::Sampler(sampler),
        }
    }
    fn buffer(&self) -> Result<&wgpu::Buffer> {
        match self {
            Self::Buffer(buffer) => Ok(buffer),
            _ => bail!("compute resource is not a buffer"),
        }
    }
}
#[derive(Clone)]
struct Pipeline {
    pipeline: wgpu::ComputePipeline,
    layouts: Vec<wgpu::BindGroupLayout>,
    uniform: Option<wgpu::Buffer>,
    last_used: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PipelineKey {
    revision: u64,
    entry: String,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BindingKey {
    pipeline: PipelineKey,
    resources: Vec<Handle>,
}
struct Bindings {
    groups: Vec<wgpu::BindGroup>,
    last_used: u64,
}

struct Upload {
    buffer: wgpu::Buffer,
    capacity: u64,
    // 0 mapped and reusable; 1 submitted/remapping; 2 mapping failed.
    state: Arc<AtomicU8>,
}
struct Readback {
    buffer: wgpu::Buffer,
    capacity: u64,
    // 0 free; 1 submitted/mapping; 2 mapped; 3 failed.
    state: Arc<AtomicU8>,
    request: Option<(Ticket, u64, CompletionSink)>,
}
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct ExecutorStats {
    pub pipelines: usize,
    pub bind_groups: usize,
    pub allocations: usize,
    pub staging_bytes: u64,
    pub readback_bytes: u64,
    pub pipeline_compilations: u64,
    pub bind_group_creations: u64,
    pub resource_creations: u64,
    pub upload_allocations: u64,
    pub readback_allocations: u64,
    pub submissions: u64,
    pub profile_frame: u64,
    pub encode_ms: f64,
    pub submit_ms: f64,
}

/// One coordinator per play world. Call before drawing any viewport and even when surface
/// acquisition is unavailable. Submission is independent from render-target dimensions.
pub struct Executor {
    capabilities: Capabilities,
    world: Option<u64>,
    resources: BTreeMap<Handle, Allocation>,
    pipelines: BTreeMap<PipelineKey, Pipeline>,
    bindings: BTreeMap<BindingKey, Bindings>,
    uploads: Vec<Upload>,
    readbacks: Vec<Readback>,
    profiler: GpuProfiler,
    clock: u64,
    stats: ExecutorStats,
    in_flight: OnceLock<Arc<AtomicUsize>>,
    texture_revision: u64,
    completion: Option<CompletionSink>,
}
impl Executor {
    pub fn new(gpu: &Gpu) -> Self {
        let l = gpu.device.limits();
        let device_generation = NEXT_DEVICE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("compute device identities exhausted");
        let capabilities = Capabilities {
            backend: Some(gpu.adapter.get_info().backend.to_str().into()),
            device_generation,
            max_buffer_bytes: l
                .max_buffer_size
                .min(l.max_storage_buffer_binding_size)
                .min(bozzard_compute::MAX_BUFFER_BYTES as u64),
            max_uniform_bytes: l.max_uniform_buffer_binding_size.min(u64::from(u32::MAX)) as u32,
            max_texture_dimension: l.max_texture_dimension_2d,
            max_workgroup_size: [
                l.max_compute_workgroup_size_x,
                l.max_compute_workgroup_size_y,
                l.max_compute_workgroup_size_z,
            ],
            max_workgroup_invocations: l.max_compute_invocations_per_workgroup,
            max_workgroup_bytes: l.max_compute_workgroup_storage_size,
            max_workgroups: l.max_compute_workgroups_per_dimension,
            max_storage_buffers: l.max_storage_buffers_per_shader_stage,
            max_storage_textures: l.max_storage_textures_per_shader_stage,
            max_sampled_textures: l.max_sampled_textures_per_shader_stage,
            max_samplers: l.max_samplers_per_shader_stage,
        };
        Self {
            capabilities,
            world: None,
            resources: BTreeMap::new(),
            pipelines: BTreeMap::new(),
            bindings: BTreeMap::new(),
            uploads: Vec::new(),
            readbacks: Vec::new(),
            profiler: GpuProfiler::default(),
            clock: 0,
            stats: ExecutorStats::default(),
            in_flight: OnceLock::new(),
            completion: None,
            texture_revision: 0,
        }
    }
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    /// Preflight before a source revision becomes visible to scripts. Queued jobs retain their
    /// immutable old kernels and failed edits leave the scene's last working revision intact.
    pub fn validate_kernel(&mut self, gpu: &Gpu, kernel: &Kernel) -> Result<()> {
        for entry in kernel.entries() {
            self.prepare_pipeline(
                gpu,
                &PipelineKey {
                    revision: kernel.id(),
                    entry: entry.name.clone(),
                },
                kernel,
            )?;
        }
        Ok(())
    }
    pub fn clear_world(&mut self) {
        self.completion = None;
        if self
            .resources
            .values()
            .any(|a| matches!(a, Allocation::Texture { .. }))
        {
            self.texture_revision = self.texture_revision.saturating_add(1);
        }
        self.world = None;
        self.resources.clear();
        self.bindings.clear();
    }
    pub fn texture_revision(&self) -> (u64, u64) {
        (self.capabilities.device_generation, self.texture_revision)
    }
    pub fn texture_views(&self) -> impl Iterator<Item = (Handle, wgpu::TextureView)> + '_ {
        self.resources
            .iter()
            .filter_map(|(handle, allocation)| match allocation {
                Allocation::Texture { view, .. } => Some((*handle, view.clone())),
                _ => None,
            })
    }
    pub fn set_profiling(&mut self, enabled: bool) {
        self.profiler.enabled = enabled;
    }
    pub fn statistics(&self) -> ExecutorStats {
        ExecutorStats {
            pipelines: self.pipelines.len(),
            bind_groups: self.bindings.len(),
            allocations: self.resources.len(),
            staging_bytes: self.uploads.iter().map(|s| s.capacity).sum(),
            readback_bytes: self.readbacks.iter().map(|s| s.capacity).sum(),
            ..self.stats
        }
    }
    /// A generated texture identity remains distinct from imported image IDs. Views are linear
    /// color with straight alpha. Wgpu retains submitted references across release/replacement.
    pub fn texture_view(&self, handle: Handle) -> Option<&wgpu::TextureView> {
        match self.resources.get(&handle) {
            Some(Allocation::Texture { view, .. }) => Some(view),
            _ => None,
        }
    }
    pub fn texture(&self, handle: Handle) -> Option<&wgpu::Texture> {
        match self.resources.get(&handle) {
            Some(Allocation::Texture { texture, .. }) => Some(texture),
            _ => None,
        }
    }
    /// Poll once without blocking. Mapped bytes go to the CPU inbox; `Runtime::begin_tick`
    /// determines when gameplay can observe them. No polling is needed for an unused executor.
    pub fn has_pending(&self) -> bool {
        self.uploads
            .iter()
            .any(|s| s.state.load(Ordering::Acquire) == 1)
            || self.readbacks.iter().any(|s| s.request.is_some())
            || self.in_flight_count() > 0
    }
    fn in_flight_count(&self) -> usize {
        self.in_flight
            .get()
            .map_or(0, |count| count.load(Ordering::Acquire))
    }
    pub fn poll(&mut self, gpu: &Gpu) -> Result<Vec<GpuFrameTiming>> {
        if let Some(error) = gpu.failure() {
            if let Some(sink) = self.completion.take() {
                sink.device_failed(error.to_owned());
            }
            bail!("{error}");
        }
        if self.has_pending()
            && let Err(error) = gpu
                .device
                .poll(wgpu::PollType::Poll)
                .context("polling compute completion")
        {
            if let Some(sink) = self.completion.take() {
                sink.device_failed(format!("{error:#}"));
            }
            return Err(error);
        }
        for slot in &mut self.readbacks {
            let state = slot.state.load(Ordering::Acquire);
            if state < 2 {
                continue;
            }
            let Some((ticket, bytes, sink)) = slot.request.take() else {
                continue;
            };
            let result = if state == 2 {
                slot.buffer
                    .get_mapped_range(0..bytes)
                    .map(|mapped| Arc::<[u8]>::from(&*mapped))
                    .map_err(|error| error.to_string())
            } else {
                Err("GPU readback mapping failed".into())
            };
            slot.buffer.unmap();
            slot.state.store(0, Ordering::Release);
            sink.readback_done(ticket, result);
        }
        self.profiler.poll(gpu)
    }
    pub fn submit(&mut self, gpu: &Gpu, runtime: &mut Runtime) -> Result<bool> {
        ensure!(
            runtime.capabilities().device_generation == self.capabilities.device_generation,
            "compute runtime belongs to another device; install the executor capabilities before ticking scripts"
        );
        if self.world != Some(runtime.world()) {
            // Dropping logical references is safe: in-flight wgpu commands retain allocations.
            self.clear_world();
            self.world = Some(runtime.world());
        }
        runtime.submit_with(|batch, sink| match self.encode_submit(gpu, &batch, sink) {
            Ok(true) => Submission::Submitted,
            Ok(false) => Submission::Deferred,
            Err(error) => Submission::Rejected(format!("{error:#}")),
        })
    }
    fn encode_submit(
        &mut self,
        gpu: &Gpu,
        batch: &Batch<'_>,
        sink: CompletionSink,
    ) -> Result<bool> {
        if let Some(error) = gpu.failure() {
            sink.device_failed(error.to_owned());
            bail!("{error}");
        }
        self.completion = Some(sink.clone());
        // These executor-wide bounds also cover callbacks from a world that has just unloaded.
        if self.in_flight_count() >= UPLOAD_SLOTS {
            return Ok(false);
        }
        let requested = batch
            .requests
            .iter()
            .filter(|r| matches!(r.command, Command::Readback { .. }))
            .count();
        let occupied = self
            .readbacks
            .iter()
            .filter(|s| s.request.is_some())
            .count();
        if requested + occupied > bozzard_compute::MAX_READBACKS {
            return Ok(false);
        }
        self.clock = self.clock.saturating_add(1);
        let encode_started = std::time::Instant::now();
        let upload_bytes: u64 = batch
            .requests
            .iter()
            .map(|r| match &r.command {
                Command::Write { bytes, .. } => align8(bytes.len() as u64),
                Command::Dispatch { params, .. } => align8(params.len() as u64),
                _ => 0,
            })
            .sum();
        let Some(upload_index) = self.upload_slot(gpu, upload_bytes)? else {
            return Ok(false);
        };
        let upload = upload_index.map(|i| self.uploads[i].buffer.clone());
        let mut readbacks = Vec::new();
        let mut released = Vec::new();
        let mut submitted = false;
        let result = scoped(gpu, "compute batch", || -> Result<()> {
            let mut encoder = self.profiler.encoder(gpu);
            let mut mapped = upload
                .as_ref()
                .map(|b| b.get_mapped_range_mut(..))
                .transpose()?;
            let mut offset = 0;
            for request in batch.requests {
                match &request.command {
                    Command::Create(resource) => self.create_resource(gpu, resource)?,
                    Command::Write {
                        resource,
                        offset: target_offset,
                        bytes,
                    } => {
                        let target = self
                            .resources
                            .get(&resource.handle)
                            .context("compute write references an unavailable resource")?
                            .buffer()?;
                        copy_upload(
                            &mut encoder,
                            upload.as_ref().unwrap(),
                            mapped.as_mut().unwrap(),
                            &mut offset,
                            target,
                            *target_offset,
                            bytes,
                        );
                    }
                    Command::Dispatch {
                        asset,
                        kernel,
                        entry,
                        resources,
                        params,
                        groups,
                        ..
                    } => {
                        let key = PipelineKey {
                            revision: kernel.id(),
                            entry: entry.clone(),
                        };
                        self.prepare_pipeline(gpu, &key, kernel)
                            .with_context(|| format!("compute shader '{asset}' entry '{entry}'"))?;
                        let key = BindingKey {
                            pipeline: key,
                            resources: resources.iter().map(|r| r.handle).collect(),
                        };
                        self.prepare_bindings(gpu, &key, kernel)?;
                        let pipeline = &self.pipelines[&key.pipeline];
                        if let Some(uniform) = &pipeline.uniform {
                            copy_upload(
                                &mut encoder,
                                upload.as_ref().unwrap(),
                                mapped.as_mut().unwrap(),
                                &mut offset,
                                uniform,
                                0,
                                params,
                            );
                        }
                        let label = format!("Compute {asset}::{entry}");
                        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some(&label),
                            ..Default::default()
                        });
                        pass.set_pipeline(&pipeline.pipeline);
                        for (group, binding) in self.bindings[&key].groups.iter().enumerate() {
                            pass.set_bind_group(group as u32, binding, &[]);
                        }
                        pass.dispatch_workgroups(groups[0], groups[1], groups[2]);
                    }
                    Command::Readback {
                        ticket,
                        resource,
                        offset,
                        bytes,
                    } => {
                        let index = self.readback_slot(gpu, *bytes, &readbacks)?;
                        let source = self
                            .resources
                            .get(&resource.handle)
                            .context("compute readback references an unavailable resource")?
                            .buffer()?;
                        encoder.copy_buffer_to_buffer(
                            source,
                            *offset,
                            &self.readbacks[index].buffer,
                            0,
                            *bytes,
                        );
                        readbacks.push((index, *ticket, *bytes));
                    }
                    Command::Release(resource) => released.push(resource.handle),
                }
            }
            drop(mapped);
            if let Some(buffer) = &upload {
                buffer.unmap();
            }
            // Encoder uploads and each parameter snapshot remain in command order. No queued
            // write_buffer can overwrite data intended for an earlier dispatch in this batch.
            self.stats.encode_ms = encode_started.elapsed().as_secs_f64() * 1000.;
            self.stats.profile_frame = encoder.frame;
            let submit_started = std::time::Instant::now();
            self.profiler.submit(gpu, encoder);
            self.stats.submit_ms = submit_started.elapsed().as_secs_f64() * 1000.;
            submitted = true;
            Ok(())
        });
        if submitted {
            let in_flight = self
                .in_flight
                .get_or_init(|| Arc::new(AtomicUsize::new(0)))
                .clone();
            in_flight.fetch_add(1, Ordering::AcqRel);
            let completion = sink.clone();
            let serial = batch.serial;
            gpu.queue.on_submitted_work_done(move || {
                in_flight.fetch_sub(1, Ordering::AcqRel);
                completion.submitted_work_done(serial);
            });
        }
        // If encoding failed before unmapping/submission, discard this slot. Otherwise remap it
        // asynchronously. Both paths leave bounded storage usable by the next batch.
        if let Some(index) = upload_index {
            let slot = &self.uploads[index];
            if result.is_err() {
                slot.buffer.unmap();
                slot.state.store(2, Ordering::Release);
            } else {
                slot.state.store(1, Ordering::Release);
                let state = slot.state.clone();
                slot.buffer
                    .map_async(wgpu::MapMode::Write, .., move |result| {
                        state.store(if result.is_ok() { 0 } else { 2 }, Ordering::Release);
                    });
            }
        }
        if let Err(error) = result {
            for request in batch.requests {
                if let Command::Create(resource) | Command::Release(resource) = &request.command {
                    if matches!(
                        self.resources.remove(&resource.handle),
                        Some(Allocation::Texture { .. })
                    ) {
                        self.texture_revision = self.texture_revision.saturating_add(1);
                    }
                    self.bindings
                        .retain(|key, _| !key.resources.contains(&resource.handle));
                }
            }
            return Err(error);
        }
        for (index, ticket, bytes) in readbacks {
            let slot = &mut self.readbacks[index];
            slot.request = Some((ticket, bytes, sink.clone()));
            slot.state.store(1, Ordering::Release);
            let state = slot.state.clone();
            slot.buffer
                .map_async(wgpu::MapMode::Read, 0..bytes, move |result| {
                    state.store(if result.is_ok() { 2 } else { 3 }, Ordering::Release);
                });
        }
        for handle in released {
            if matches!(
                self.resources.remove(&handle),
                Some(Allocation::Texture { .. })
            ) {
                self.texture_revision = self.texture_revision.saturating_add(1);
            }
            self.bindings
                .retain(|key, _| !key.resources.contains(&handle));
        }
        self.stats.submissions += 1;
        Ok(true)
    }
    fn create_resource(&mut self, gpu: &Gpu, resource: &Resource) -> Result<()> {
        if self.resources.contains_key(&resource.handle) {
            return Ok(());
        }
        ensure!(
            self.resources.len() < bozzard_compute::MAX_RESOURCES,
            "GPU compute resource budget exceeded"
        );
        let allocation = scoped(gpu, &resource.name, || {
            Ok(match &resource.kind {
                ResourceKind::Buffer { bytes, .. } => {
                    Allocation::Buffer(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&resource.name),
                        size: *bytes,
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_SRC
                            | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }))
                }
                ResourceKind::Texture {
                    width,
                    height,
                    format,
                } => {
                    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some(&resource.name),
                        size: wgpu::Extent3d {
                            width: *width,
                            height: *height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: texture_format(*format),
                        usage: wgpu::TextureUsages::STORAGE_BINDING
                            | wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_SRC,
                        view_formats: &[],
                    });
                    let view = texture.create_view(&Default::default());
                    Allocation::Texture { texture, view }
                }
                ResourceKind::Sampler { linear } => {
                    Allocation::Sampler(gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                        label: Some(&resource.name),
                        mag_filter: if *linear {
                            wgpu::FilterMode::Linear
                        } else {
                            wgpu::FilterMode::Nearest
                        },
                        min_filter: if *linear {
                            wgpu::FilterMode::Linear
                        } else {
                            wgpu::FilterMode::Nearest
                        },
                        ..Default::default()
                    }))
                }
            })
        })?;
        if matches!(allocation, Allocation::Texture { .. }) {
            self.texture_revision = self.texture_revision.saturating_add(1);
        }
        self.resources.insert(resource.handle, allocation);
        self.stats.resource_creations += 1;
        Ok(())
    }
    fn prepare_pipeline(&mut self, gpu: &Gpu, key: &PipelineKey, kernel: &Kernel) -> Result<()> {
        if let Some(pipeline) = self.pipelines.get_mut(key) {
            pipeline.last_used = self.clock;
            return Ok(());
        }
        let entry = kernel.entry(&key.entry)?;
        self.capabilities.validate_entry(entry)?;
        let pipeline = scoped(gpu, &format!("compute entry {}", key.entry), || {
            let group_count = entry
                .bindings
                .iter()
                .map(|b| b.group + 1)
                .max()
                .unwrap_or(0);
            let mut layouts = Vec::with_capacity(group_count as usize);
            for group in 0..group_count {
                let entries: Vec<_> = entry
                    .bindings
                    .iter()
                    .filter(|b| b.group == group)
                    .map(|binding| {
                        let ty = match &binding.kind {
                            BindingKind::Uniform(layout) => wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: wgpu::BufferSize::new(u64::from(
                                    layout.minimum_size(),
                                )),
                            },
                            BindingKind::Storage { layout, writable } => {
                                wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Storage {
                                        read_only: !writable,
                                    },
                                    has_dynamic_offset: false,
                                    min_binding_size: wgpu::BufferSize::new(u64::from(
                                        layout.minimum_size(),
                                    )),
                                }
                            }
                            BindingKind::SampledTexture => wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            BindingKind::StorageTexture(format) => {
                                wgpu::BindingType::StorageTexture {
                                    access: wgpu::StorageTextureAccess::WriteOnly,
                                    format: texture_format(*format),
                                    view_dimension: wgpu::TextureViewDimension::D2,
                                }
                            }
                            BindingKind::Sampler => {
                                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
                            }
                        };
                        wgpu::BindGroupLayoutEntry {
                            binding: binding.binding,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty,
                            count: None,
                        }
                    })
                    .collect();
                layouts.push(gpu.device.create_bind_group_layout(
                    &wgpu::BindGroupLayoutDescriptor {
                        label: Some(&key.entry),
                        entries: &entries,
                    },
                ));
            }
            let layout = gpu
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(&key.entry),
                    bind_group_layouts: &layouts.iter().map(Some).collect::<Vec<_>>(),
                    immediate_size: 0,
                });
            let module = gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(&key.entry),
                    source: wgpu::ShaderSource::Wgsl(kernel.source().into()),
                });
            let pipeline = gpu
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(&key.entry),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(&key.entry),
                    compilation_options: Default::default(),
                    cache: None,
                });
            let uniform = entry.parameters().map(|params| {
                gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("compute params"),
                    size: u64::from(params.minimum_size()),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            Ok(Pipeline {
                pipeline,
                layouts,
                uniform,
                last_used: self.clock,
            })
        })?;
        if self.pipelines.len() >= MAX_PIPELINES {
            let oldest = self
                .pipelines
                .iter()
                .min_by_key(|(_, p)| p.last_used)
                .map(|(key, _)| key.clone())
                .unwrap();
            self.pipelines.remove(&oldest);
            self.bindings.retain(|key, _| key.pipeline != oldest);
        }
        self.pipelines.insert(key.clone(), pipeline);
        self.stats.pipeline_compilations += 1;
        Ok(())
    }
    fn prepare_bindings(&mut self, gpu: &Gpu, key: &BindingKey, kernel: &Kernel) -> Result<()> {
        if let Some(bindings) = self.bindings.get_mut(key) {
            bindings.last_used = self.clock;
            return Ok(());
        }
        let pipeline = &self.pipelines[&key.pipeline];
        let entry = kernel.entry(&key.pipeline.entry)?;
        let groups = scoped(gpu, "compute bindings", || {
            let mut resources = key.resources.iter();
            let mut entries: Vec<Vec<wgpu::BindGroupEntry<'_>>> =
                vec![Vec::new(); pipeline.layouts.len()];
            for binding in &entry.bindings {
                let resource = if matches!(binding.kind, BindingKind::Uniform(_)) {
                    pipeline.uniform.as_ref().unwrap().as_entire_binding()
                } else {
                    self.resources
                        .get(resources.next().context("missing compute resource")?)
                        .context("compute dispatch references an unavailable resource")?
                        .binding()
                };
                entries[binding.group as usize].push(wgpu::BindGroupEntry {
                    binding: binding.binding,
                    resource,
                });
            }
            Ok(pipeline
                .layouts
                .iter()
                .zip(&entries)
                .map(|(layout, entries)| {
                    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&key.pipeline.entry),
                        layout,
                        entries,
                    })
                })
                .collect())
        })?;
        if self.bindings.len() >= MAX_BIND_GROUPS {
            let oldest = self
                .bindings
                .iter()
                .min_by_key(|(_, b)| b.last_used)
                .map(|(key, _)| key.clone())
                .unwrap();
            self.bindings.remove(&oldest);
        }
        self.bindings.insert(
            key.clone(),
            Bindings {
                groups,
                last_used: self.clock,
            },
        );
        self.stats.bind_group_creations += 1;
        Ok(())
    }
    /// Some(None) needs no upload storage; None applies backpressure until a mapped slot returns.
    fn upload_slot(&mut self, gpu: &Gpu, bytes: u64) -> Result<Option<Option<usize>>> {
        if bytes == 0 {
            return Ok(Some(None));
        }
        ensure!(
            bytes
                <= bozzard_compute::MAX_UPLOAD_BYTES as u64
                    + bozzard_compute::MAX_COMMANDS as u64 * 8,
            "compute upload exceeds staging budget"
        );
        let index = self
            .uploads
            .iter()
            .position(|s| s.state.load(Ordering::Acquire) != 1);
        let index = match index {
            Some(index) => index,
            None if self.uploads.len() < UPLOAD_SLOTS => self.uploads.len(),
            None => return Ok(None),
        };
        if index == self.uploads.len()
            || self.uploads[index].capacity < bytes
            || self.uploads[index].state.load(Ordering::Acquire) == 2
        {
            let capacity = bytes.max(256).next_power_of_two();
            let buffer = scoped(gpu, "compute upload staging", || {
                Ok(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("compute upload staging"),
                    size: capacity,
                    usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::MAP_WRITE,
                    mapped_at_creation: true,
                }))
            })?;
            let upload = Upload {
                buffer,
                capacity,
                state: Arc::new(AtomicU8::new(0)),
            };
            if index == self.uploads.len() {
                self.uploads.push(upload);
            } else {
                self.uploads[index] = upload;
            }
            self.stats.upload_allocations += 1;
        }
        Ok(Some(Some(index)))
    }
    fn readback_slot(
        &mut self,
        gpu: &Gpu,
        bytes: u64,
        reserved: &[(usize, Ticket, u64)],
    ) -> Result<usize> {
        ensure!(
            bytes <= bozzard_compute::MAX_READBACK_BYTES,
            "compute readback exceeds staging budget"
        );
        let index = self
            .readbacks
            .iter()
            .enumerate()
            .position(|(i, s)| {
                s.request.is_none() && !reserved.iter().any(|(index, _, _)| *index == i)
            })
            .unwrap_or(self.readbacks.len());
        ensure!(
            index < bozzard_compute::MAX_READBACKS,
            "compute readback staging pool is full"
        );
        if index == self.readbacks.len() || self.readbacks[index].capacity < bytes {
            let capacity = bytes.max(256).next_power_of_two();
            let buffer = scoped(gpu, "compute readback staging", || {
                Ok(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("compute readback staging"),
                    size: capacity,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }))
            })?;
            let readback = Readback {
                buffer,
                capacity,
                state: Arc::new(AtomicU8::new(0)),
                request: None,
            };
            if index == self.readbacks.len() {
                self.readbacks.push(readback);
            } else {
                self.readbacks[index] = readback;
            }
            self.stats.readback_allocations += 1;
        }
        Ok(index)
    }
}
fn align8(bytes: u64) -> u64 {
    bytes.div_ceil(8) * 8
}
fn copy_upload(
    encoder: &mut wgpu::CommandEncoder,
    upload: &wgpu::Buffer,
    mapped: &mut wgpu::BufferViewMut,
    offset: &mut u64,
    target: &wgpu::Buffer,
    target_offset: u64,
    bytes: &[u8],
) {
    mapped
        .slice(*offset as usize..*offset as usize + bytes.len())
        .copy_from_slice(bytes);
    encoder.copy_buffer_to_buffer(upload, *offset, target, target_offset, bytes.len() as u64);
    *offset += align8(bytes.len() as u64);
}
fn texture_format(format: TextureFormat) -> wgpu::TextureFormat {
    match format {
        TextureFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        TextureFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
    }
}
/// Native wgpu-core error scopes return a ready future (wgpu 30); poll once, never wait for
/// device execution. An unexpected asynchronous backend fails clearly instead of blocking.
fn scoped<T>(gpu: &Gpu, label: &str, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let memory = gpu.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = gpu.device.push_error_scope(wgpu::ErrorFilter::Internal);
    let validation = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = action();
    let mut errors = Vec::new();
    for scope in [validation, internal, memory] {
        let mut future = std::pin::pin!(scope.pop());
        match future
            .as_mut()
            .poll(&mut TaskContext::from_waker(Waker::noop()))
        {
            Poll::Ready(Some(error)) => errors.push(error.to_string()),
            Poll::Ready(None) => {}
            Poll::Pending => errors.push(
                "native error scope unexpectedly deferred; asynchronous backends are unsupported"
                    .into(),
            ),
        }
    }
    ensure!(errors.is_empty(), "{label}: {}", errors.join("; "));
    result.with_context(|| label.to_owned())
}
