//! Retained command recording and bounded consecutive native indirect runs.
//! Resource handles, offsets and draw ranges constitute command identity; changing
//! data inside a retained buffer does not require recording another bundle.
use super::draw_state::DrawState;
use super::*;
mod geometry;

const MIN_BUNDLE_DRAWS: usize = 128;
const MIN_INDIRECT_RUN: usize = 8;
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Record<'a> {
    pub pipeline: &'a wgpu::RenderPipeline,
    pub groups: [Option<&'a wgpu::BindGroup>; 4],
    pub vertices: [Option<(&'a wgpu::Buffer, u64)>; 3],
    pub indices: &'a wgpu::Buffer,
    pub index_count: u32,
    pub instances: u32,
    pub first_instance: u32,
    pub geometry_stable: bool,
}
impl Record<'_> {
    pub fn triangles(&self) -> u64 {
        u64::from(self.index_count / 3) * u64::from(self.instances)
    }
    fn same_state(&self, other: &Self) -> bool {
        self.pipeline == other.pipeline
            && self.groups == other.groups
            && self.vertices == other.vertices
            && self.indices == other.indices
    }
    fn bind(&self, pass: &mut wgpu::RenderPass<'_>, state: &mut DrawState, cache: bool) {
        state.pipeline(pass, self.pipeline, cache);
        for (slot, group) in self.groups.iter().enumerate() {
            if let Some(group) = group {
                state.group(pass, slot, group, cache);
            }
        }
        for (slot, vertex) in self.vertices.iter().enumerate() {
            if let Some((buffer, offset)) = vertex {
                state.vertex(pass, slot, buffer, *offset, cache);
            }
        }
        state.index(pass, self.indices, cache);
    }
}
#[derive(Clone)]
struct CachedRecord {
    pipeline: wgpu::RenderPipeline,
    groups: [Option<wgpu::BindGroup>; 4],
    vertices: [Option<(wgpu::Buffer, u64)>; 3],
    indices: wgpu::Buffer,
    index_count: u32,
    instances: u32,
    first_instance: u32,
    geometry_stable: bool,
}
impl CachedRecord {
    fn from_record(record: &Record<'_>) -> Self {
        Self {
            pipeline: record.pipeline.clone(),
            groups: record.groups.map(|g| g.cloned()),
            vertices: record.vertices.map(|v| v.map(|(b, o)| (b.clone(), o))),
            indices: record.indices.clone(),
            index_count: record.index_count,
            instances: record.instances,
            first_instance: record.first_instance,
            geometry_stable: record.geometry_stable,
        }
    }
    fn as_record(&self) -> Record<'_> {
        Record {
            pipeline: &self.pipeline,
            groups: self.groups.each_ref().map(Option::as_ref),
            vertices: self
                .vertices
                .each_ref()
                .map(|v| v.as_ref().map(|(b, o)| (b, *o))),
            indices: &self.indices,
            index_count: self.index_count,
            instances: self.instances,
            first_instance: self.first_instance,
            geometry_stable: self.geometry_stable,
        }
    }
}
#[derive(Clone, Copy, Default, Debug)]
pub(super) struct Work {
    pub bundle_compilations: usize,
    pub bundle_replays: usize,
    pub indirect_runs: usize,
    pub indirect_draws: usize,
    pub indirect_bytes: usize,
    pub direct_draws: usize,
}
#[derive(Default)]
struct BundleCache {
    records: Vec<CachedRecord>,
    mask: u8,
    bundle: Option<wgpu::RenderBundle>,
}
#[derive(Default)]
struct IndirectCache {
    buffer: Option<wgpu::Buffer>,
    capacity: usize,
    bytes: Vec<u8>,
    runs: Vec<(usize, usize)>,
    geometry: geometry::Arena,
}
pub(super) struct Submission {
    bundles_enabled: bool,
    indirect_enabled: bool,
    bundle: BundleCache,
    indirect: IndirectCache,
    mode: Mode,
    work: Work,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Direct,
    Bundle,
    Indirect,
}
impl Default for Submission {
    fn default() -> Self {
        Self {
            bundles_enabled: true,
            indirect_enabled: true,
            bundle: Default::default(),
            indirect: Default::default(),
            mode: Mode::Direct,
            work: Default::default(),
        }
    }
}
fn run_ranges(records: &[Record], geometry: bool) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < records.len() {
        let mut end = start + 1;
        while end < records.len()
            && (if geometry {
                records[start].pipeline == records[end].pipeline
                    && records[start].groups == records[end].groups
                    && records[start].geometry_stable
                    && records[end].geometry_stable
            } else {
                records[start].same_state(&records[end])
            })
        {
            end += 1;
        }
        if end - start >= MIN_INDIRECT_RUN {
            runs.push((start, end));
        }
        start = end;
    }
    runs
}
fn argument_bytes(records: &[Record]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(records.len() * 20);
    for record in records {
        for word in [
            record.index_count,
            record.instances,
            0,
            0,
            record.first_instance,
        ] {
            bytes.extend_from_slice(&word.to_ne_bytes());
        }
    }
    bytes
}
impl Submission {
    pub fn invalidate(&mut self) {
        self.bundle = Default::default();
        self.indirect = Default::default();
        self.mode = Mode::Direct;
    }
    pub fn candidate(&mut self, draws: usize, native: bool) -> bool {
        if self.bundle.records.len() > draws.saturating_mul(4).max(MIN_BUNDLE_DRAWS) {
            self.bundle = Default::default();
        }
        (self.bundles_enabled && draws >= MIN_BUNDLE_DRAWS)
            || (self.indirect_enabled && native && draws >= MIN_INDIRECT_RUN)
    }
    pub fn set_bundles_enabled(&mut self, enabled: bool) {
        self.bundles_enabled = enabled;
        if !enabled {
            self.bundle = Default::default();
        }
    }
    pub fn set_indirect_enabled(&mut self, enabled: bool) {
        self.indirect_enabled = enabled;
        if !enabled {
            self.indirect = Default::default();
        }
    }
    /// Eligible records have no GPU-produced visibility arguments or particle
    /// interleave. Their order is already certified by the planner.
    pub fn prepare(
        &mut self,
        gpu: &Gpu,
        records: &[Record],
        mask: u8,
        eligible: bool,
        native_arena: bool,
    ) {
        self.work = Default::default();
        self.mode = Mode::Direct;
        if !eligible || records.is_empty() {
            return;
        }
        if self.bundles_enabled && records.len() >= MIN_BUNDLE_DRAWS {
            if self.bundle.bundle.is_none()
                || self.bundle.mask != mask
                || (self.bundle.records.len() != records.len()
                    || self
                        .bundle
                        .records
                        .iter()
                        .zip(records)
                        .any(|(old, new)| old.as_record() != *new))
            {
                let formats = [
                    Some(wgpu::TextureFormat::Rgba16Float),
                    (mask & 1 != 0).then_some(wgpu::TextureFormat::Rgba16Float),
                    (mask & 2 != 0).then_some(wgpu::TextureFormat::Rgba16Float),
                    (mask & 4 != 0).then_some(wgpu::TextureFormat::Rgba16Float),
                ];
                let mut encoder =
                    gpu.device
                        .create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
                            label: Some("retained scene surface commands"),
                            color_formats: &formats,
                            depth_stencil: Some(wgpu::RenderBundleDepthStencil {
                                format: wgpu::TextureFormat::Depth32Float,
                                depth_read_only: false,
                                stencil_read_only: true,
                            }),
                            sample_count: 1,
                            multiview: None,
                        });
                let mut last_pipeline = None;
                let mut occupied_groups = [false; 4];
                for record in records {
                    if last_pipeline != Some(record.pipeline) {
                        // A disappearing earlier group shifts later Metal
                        // registers. Clear the logical slot before reassigning
                        // even an identical group, bypassing redundant filtering.
                        for (slot, occupied) in occupied_groups.iter_mut().enumerate() {
                            if std::mem::take(occupied) {
                                encoder.set_bind_group(slot as u32, None::<&wgpu::BindGroup>, &[]);
                            }
                        }
                        last_pipeline = Some(record.pipeline);
                    }
                    encoder.set_pipeline(record.pipeline);
                    for (slot, group) in record.groups.iter().enumerate() {
                        if let Some(group) = group {
                            encoder.set_bind_group(slot as u32, *group, &[]);
                            occupied_groups[slot] = true;
                        }
                    }
                    for (slot, vertex) in record.vertices.iter().enumerate() {
                        if let Some((buffer, offset)) = vertex {
                            encoder.set_vertex_buffer(slot as u32, buffer.slice(*offset..));
                        }
                    }
                    encoder.set_index_buffer(record.indices.slice(..), wgpu::IndexFormat::Uint32);
                    encoder.draw_indexed(
                        0..record.index_count,
                        0,
                        record.first_instance..record.first_instance + record.instances,
                    );
                }
                self.bundle.bundle = Some(encoder.finish(&wgpu::RenderBundleDescriptor {
                    label: Some("retained scene surface commands"),
                }));
                self.bundle.records.clear();
                self.bundle
                    .records
                    .extend(records.iter().map(CachedRecord::from_record));
                self.bundle.mask = mask;
                self.work.bundle_compilations = 1;
            }
            self.mode = Mode::Bundle;
            return;
        }
        let downlevel = gpu.adapter.get_downlevel_capabilities().flags;
        if self.indirect_enabled
            && native_arena
            && downlevel.contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION)
            && gpu.device.features().contains(
                wgpu::Features::INDIRECT_FIRST_INSTANCE | wgpu::Features::MULTI_DRAW_INDIRECT_COUNT,
            )
        {
            self.indirect.geometry.disable();
            self.indirect.runs = run_ranges(records, false);
            let geometry_runs = run_ranges(records, true);
            let packed = if geometry_runs.iter().map(|(a, b)| b - a).sum::<usize>()
                > self.indirect.runs.iter().map(|(a, b)| b - a).sum::<usize>()
            {
                self.indirect
                    .geometry
                    .prepare(gpu, records, &geometry_runs)
                    .inspect(|_| {
                        self.indirect.runs = geometry_runs;
                    })
            } else {
                None
            };
            if self.indirect.runs.is_empty() {
                return;
            }
            let bytes = packed.unwrap_or_else(|| argument_bytes(records));
            let allocate = self.indirect.buffer.is_none() || bytes.len() > self.indirect.capacity;
            if allocate {
                self.indirect.capacity = bytes.len().next_power_of_two();
                self.indirect.buffer = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("retained native surface draw arguments"),
                    size: self.indirect.capacity as u64,
                    usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if allocate || self.indirect.bytes != bytes {
                gpu.queue
                    .write_buffer(self.indirect.buffer.as_ref().unwrap(), 0, &bytes);
                self.work.indirect_bytes = bytes.len();
                self.indirect.bytes = bytes;
            }
            self.mode = Mode::Indirect;
        }
    }
    pub fn draw(
        &mut self,
        pass: &mut wgpu::RenderPass<'_>,
        records: &[Record],
        state: &mut DrawState,
        cache: bool,
    ) -> Work {
        if self.mode == Mode::Bundle {
            pass.execute_bundles(std::iter::once(self.bundle.bundle.as_ref().unwrap()));
            state.reset();
            self.work.bundle_replays = 1;
        } else {
            let mut index = 0;
            let mut run = 0;
            while index < records.len() {
                let record = &records[index];
                record.bind(pass, state, cache);
                if self.mode == Mode::Indirect
                    && self
                        .indirect
                        .runs
                        .get(run)
                        .is_some_and(|&(start, _)| start == index)
                {
                    let end = self.indirect.runs[run].1;
                    self.indirect.geometry.bind(pass, record, state, cache);
                    pass.multi_draw_indexed_indirect(
                        self.indirect.buffer.as_ref().unwrap(),
                        index as u64 * 20,
                        (end - index) as u32,
                    );
                    self.work.indirect_runs += 1;
                    self.work.indirect_draws += end - index;
                    index = end;
                    run += 1;
                } else {
                    pass.draw_indexed(
                        0..record.index_count,
                        0,
                        record.first_instance..record.first_instance + record.instances,
                    );
                    self.work.direct_draws += 1;
                    index += 1;
                }
            }
        }
        self.work
    }
}
impl SceneRenderer {
    pub fn set_render_bundles_enabled(&mut self, enabled: bool) {
        self.submission.set_bundles_enabled(enabled);
    }
    pub fn set_native_multi_draw_enabled(&mut self, enabled: bool) {
        self.submission.set_indirect_enabled(enabled);
    }
}
