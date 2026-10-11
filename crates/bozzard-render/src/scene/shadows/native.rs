//! Native depth casting. Compact 96-byte caster records live in one storage
//! table indexed by draw position; every shadow pass appends its accepted
//! casters to a per-frame instance-ID stream and draws each depth group once.
//! Depth passes keep the minimum over all fragments, so groups may be drawn in
//! any order: runs sharing a pipeline and binding collapse into
//! multi-draw-indirect calls over packed immutable geometry.
use super::*;
use std::cell::RefCell;

const RECORD_BYTES: usize = 96;
const MIN_INDIRECT_RUN: usize = 2;

/// Compact depth fields copied from established color bytes (see depth.rs).
fn compact(color: &[u8; OBJECT_UNIFORM_BYTES]) -> [u8; RECORD_BYTES] {
    let mut bytes = [0; RECORD_BYTES];
    bytes[..64].copy_from_slice(&color[96..160]);
    bytes[64..72].copy_from_slice(&color[80..88]);
    bytes[72..76].copy_from_slice(&color[76..80]);
    bytes[76..80].copy_from_slice(&color[92..96]);
    bytes[80..88].copy_from_slice(&color[160..168]);
    bytes
}

#[derive(Default)]
struct Frame {
    ids: Vec<u32>,
    args: Vec<u8>,
    indirect_runs: usize,
    indirect_draws: usize,
}

pub(in crate::scene) struct NativeShadows {
    pub enabled: bool,
    layout: Option<wgpu::BindGroupLayout>,
    objects: Option<wgpu::Buffer>,
    object_capacity: usize,
    bytes: Vec<u8>,
    revisions: Vec<u64>,
    dirty: Vec<bool>,
    ids: Option<wgpu::Buffer>,
    id_capacity: usize,
    args: Option<wgpu::Buffer>,
    arg_capacity: usize,
    // `None` is the shared binding of every opaque group, which samples nothing.
    bindings: HashMap<Option<TextureKind>, wgpu::BindGroup>,
    pipelines: RefCell<BTreeMap<(bool, ShadowCoverage), wgpu::RenderPipeline>>,
    geometry: submission::geometry::Arena,
    geometry_packed: bool,
    indirect: bool,
    frame: RefCell<Frame>,
    previous_ids: Vec<u8>,
    previous_args: Vec<u8>,
    pub record_bytes: usize,
    pub id_bytes: usize,
    pub indirect_runs: usize,
    pub indirect_draws: usize,
}
impl Default for NativeShadows {
    fn default() -> Self {
        Self {
            enabled: true,
            layout: None,
            objects: None,
            object_capacity: 0,
            bytes: Vec::new(),
            revisions: Vec::new(),
            dirty: Vec::new(),
            ids: None,
            id_capacity: 0,
            args: None,
            arg_capacity: 0,
            bindings: HashMap::new(),
            pipelines: Default::default(),
            geometry: Default::default(),
            geometry_packed: false,
            indirect: false,
            frame: Default::default(),
            previous_ids: Vec::new(),
            previous_args: Vec::new(),
            record_bytes: 0,
            id_bytes: 0,
            indirect_runs: 0,
            indirect_draws: 0,
        }
    }
}

fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let storage = |binding, visibility, size| wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(size),
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("native shadow casters"),
        entries: &[
            storage(0, wgpu::ShaderStages::VERTEX_FRAGMENT, RECORD_BYTES as u64),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            storage(3, wgpu::ShaderStages::VERTEX, 4),
        ],
    })
}

/// The compact instanced caster, reading `objects[instance_ids[instance]]`.
pub(super) fn module_text() -> String {
    super::module_text(true, true)
        .replace(
            &format!(
                "@group(0) @binding(0) var<uniform> objects: array<ObjectUniform, {}>;",
                instancing::MAX_SHADOW_INSTANCES
            ),
            "@group(0) @binding(0) var<storage, read> objects: array<ObjectUniform>;\n@group(0) @binding(3) var<storage, read> instance_ids: array<u32>;",
        )
        .replace(
            "object = objects[instance];\nvar out: VertexOutput;\nout.instance = instance;",
            "let object_slot = instance_ids[instance];\nobject = objects[object_slot];\nvar out: VertexOutput;\nout.instance = object_slot;",
        )
}

fn indirect_supported(gpu: &Gpu) -> bool {
    gpu.adapter
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION)
        && gpu.device.features().contains(
            wgpu::Features::INDIRECT_FIRST_INSTANCE | wgpu::Features::MULTI_DRAW_INDIRECT_COUNT,
        )
}

/// Write the runs of changed `stride`-byte entries, returning bytes written.
fn write_changed(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    previous: &[u8],
    bytes: &[u8],
    stride: usize,
) -> usize {
    let mut written = 0;
    let mut start = None;
    let entries = bytes.len() / stride;
    for entry in 0..=entries {
        let range = entry * stride..(entry + 1) * stride;
        let dirty = entry < entries && previous.get(range.clone()) != Some(&bytes[range.clone()]);
        if dirty {
            start.get_or_insert(range.start);
        } else if let Some(first) = start.take() {
            queue.write_buffer(buffer, first as u64, &bytes[first..range.start]);
            written += range.start - first;
        }
    }
    written
}

impl NativeShadows {
    pub fn forget_texture(&mut self, retired: &impl Fn(&TextureKind) -> bool) {
        self.bindings
            .retain(|texture, _| !texture.as_ref().is_some_and(retired));
    }
    pub fn invalidate(&mut self) {
        self.bindings.clear();
        self.revisions.clear();
        self.previous_ids.clear();
        self.previous_args.clear();
        self.geometry_packed = false;
    }
    fn reserve(
        buffer: &mut Option<wgpu::Buffer>,
        capacity: &mut usize,
        gpu: &Gpu,
        needed: usize,
        usage: wgpu::BufferUsages,
        label: &str,
    ) -> bool {
        if buffer.is_some() && needed <= *capacity {
            return false;
        }
        *capacity = needed.max(256).next_power_of_two();
        *buffer = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: *capacity as u64,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        true
    }
    /// Upload changed caster records and size this frame's ID/argument streams
    /// from exact upper bounds, before any pass binds them.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        renderer: &SceneRenderer,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        groups: &[instancing::Batch],
        geometry_changed: bool,
        id_bound: usize,
        record_bound: usize,
    ) -> Result<()> {
        self.record_bytes = 0;
        self.id_bytes = 0;
        self.indirect_runs = 0;
        self.indirect_draws = 0;
        let frame = self.frame.get_mut();
        frame.ids.clear();
        frame.args.clear();
        frame.indirect_runs = 0;
        frame.indirect_draws = 0;
        let device = &renderer.shadows.device;
        if self.layout.is_none() {
            self.layout = Some(layout(device));
        }
        let mut rebind = Self::reserve(
            &mut self.objects,
            &mut self.object_capacity,
            gpu,
            draws.len().max(1) * RECORD_BYTES,
            wgpu::BufferUsages::STORAGE,
            "native shadow caster records",
        );
        if rebind {
            self.revisions.clear();
        }
        rebind |= Self::reserve(
            &mut self.ids,
            &mut self.id_capacity,
            gpu,
            id_bound.max(1) * 4,
            wgpu::BufferUsages::STORAGE,
            "native shadow instance IDs",
        );
        if rebind {
            self.bindings.clear();
            self.previous_ids.clear();
        }
        if Self::reserve(
            &mut self.args,
            &mut self.arg_capacity,
            gpu,
            record_bound.max(1) * 20,
            wgpu::BufferUsages::INDIRECT,
            "native shadow draw arguments",
        ) {
            self.previous_args.clear();
        }
        // Records for every keyed caster, by draw position; revisions follow the
        // color uniforms, so stationary casters upload nothing.
        self.bytes.resize(draws.len() * RECORD_BYTES, 0);
        self.revisions.resize(draws.len(), 0);
        self.dirty.clear();
        self.dirty.resize(draws.len(), false);
        for group in groups.iter().filter(|g| g.slot.is_some()) {
            for &index in &group.indices {
                let object = &renderer.objects[index];
                if !renderer.state_caching || self.revisions[index] != object.uniform_revision {
                    let record =
                        compact(object.uniform.as_ref().expect("validated caster uniform"));
                    self.bytes[index * RECORD_BYTES..(index + 1) * RECORD_BYTES]
                        .copy_from_slice(&record);
                    self.revisions[index] = object.uniform_revision;
                    self.dirty[index] = true;
                }
            }
        }
        let objects = self.objects.as_ref().unwrap();
        let mut start = None;
        for (index, &dirty) in self.dirty.iter().chain(std::iter::once(&false)).enumerate() {
            if dirty {
                start.get_or_insert(index);
            } else if let Some(first) = start.take() {
                let range = first * RECORD_BYTES..index * RECORD_BYTES;
                gpu.queue
                    .write_buffer(objects, range.start as u64, &self.bytes[range.clone()]);
                self.record_bytes += range.len();
            }
        }
        // Bindings: one shared opaque group, one per alpha-tested texture.
        let opaque_binding = |texture: &TextureKind| -> Result<wgpu::BindGroup> {
            let sampler = match texture {
                TextureKind::ModelPart(id, part) => &renderer.models[id][*part].sampler,
                TextureKind::Text => &renderer.model_sampler,
                _ => &renderer.sampler,
            };
            Ok(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("native shadow casters"),
                layout: self.layout.as_ref().unwrap(),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: objects.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(
                            renderer.texture_view(texture)?,
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.ids.as_ref().unwrap().as_entire_binding(),
                    },
                ],
            }))
        };
        let masked = groups
            .iter()
            .filter(|g| g.slot.is_some())
            .map(|group| (group.indices[0], &draws[group.indices[0]]))
            .filter(|(index, draw)| {
                renderer.shadow_coverage_at(*index, draw) == ShadowCoverage::Masked
            })
            .map(|(_, draw)| Some(&draw.object.material.texture));
        for texture in std::iter::once(None).chain(masked) {
            if let std::collections::hash_map::Entry::Vacant(entry) =
                self.bindings.entry(texture.cloned())
            {
                entry.insert(opaque_binding(texture.unwrap_or(&TextureKind::White))?);
            }
        }
        if self.bindings.len() > groups.len() + 8 {
            let active: std::collections::HashSet<_> = groups
                .iter()
                .filter(|g| g.slot.is_some())
                .map(|g| &draws[g.indices[0]].object.material.texture)
                .collect();
            self.bindings
                .retain(|key, _| key.as_ref().is_none_or(|texture| active.contains(texture)));
        }
        // Pack immutable meshes once per depth-group population for indirect runs.
        self.indirect = indirect_supported(gpu) && renderer.submission.indirect_enabled();
        if !self.indirect {
            self.geometry.disable();
            self.geometry_packed = false;
        } else if geometry_changed || !self.geometry_packed {
            let pipeline = &renderer.shadows.pipeline;
            let records: Vec<submission::Record<'_>> = groups
                .iter()
                .filter(|g| g.slot.is_some())
                .filter_map(|group| {
                    let draw = &draws[group.indices[0]];
                    if draw.deformation != 0 || draw.shared_geometry.is_some() {
                        return None;
                    }
                    let mesh = renderer.mesh_for(&draw.object);
                    Some(submission::Record {
                        pipeline,
                        groups: [None; 4],
                        vertices: [Some((&mesh.vertices, mesh.vertex_offset)), None, None],
                        indices: &mesh.indices,
                        index_count: mesh.count,
                        instances: 1,
                        first_instance: 0,
                        geometry_stable: true,
                    })
                })
                .collect();
            self.geometry_packed = !records.is_empty()
                && self
                    .geometry
                    .prepare(gpu, &records, &[(0, records.len())])
                    .is_some();
        }
        Ok(())
    }
    fn pipeline(
        &self,
        renderer: &SceneRenderer,
        point: bool,
        coverage: ShadowCoverage,
    ) -> wgpu::RenderPipeline {
        self.pipelines
            .borrow_mut()
            .entry((point, coverage))
            .or_insert_with(|| {
                pipeline_variant(
                    &renderer.shadows.device,
                    self.layout.as_ref().unwrap(),
                    &renderer.shadows.caster_layout,
                    PipelineSpec {
                        instanced: true,
                        point,
                        compact: true,
                        coverage,
                        native: true,
                    },
                )
            })
            .clone()
    }
    /// Draw every keyed group's accepted members with one instanced command,
    /// or one indirect record inside a shared-state run.
    pub fn encode(
        &self,
        renderer: &SceneRenderer,
        pass: &mut wgpu::RenderPass<'_>,
        draws: &[PreparedDraw],
        groups: &[&instancing::Batch],
        point: bool,
        casts: &dyn Fn(usize) -> bool,
    ) -> (usize, u64) {
        struct Entry<'a> {
            coverage: ShadowCoverage,
            binding: &'a wgpu::BindGroup,
            mesh: &'a MeshBuffers,
            packed: Option<(u32, i32)>,
            instances: u32,
            first_instance: u32,
        }
        let mut frame = self.frame.borrow_mut();
        let mut entries = Vec::with_capacity(groups.len());
        for group in groups {
            let start = frame.ids.len();
            frame.ids.extend(
                group
                    .indices
                    .iter()
                    .copied()
                    .filter(|&index| casts(index))
                    .map(|index| index as u32),
            );
            let instances = frame.ids.len() - start;
            if instances == 0 {
                continue;
            }
            let index = group.indices[0];
            let draw = &draws[index];
            let coverage = renderer.shadow_coverage_at(index, draw);
            let texture =
                (coverage == ShadowCoverage::Masked).then(|| draw.object.material.texture.clone());
            let mesh = renderer.mesh_for(&draw.object);
            let stable = draw.deformation == 0 && draw.shared_geometry.is_none();
            entries.push(Entry {
                coverage,
                binding: &self.bindings[&texture],
                mesh,
                packed: (stable && self.indirect && self.geometry_packed)
                    .then(|| {
                        self.geometry.base_of(
                            &mesh.vertices,
                            mesh.vertex_offset,
                            &mesh.indices,
                            mesh.count,
                        )
                    })
                    .flatten(),
                instances: instances as u32,
                first_instance: start as u32,
            });
        }
        // Order is free for depth: gather shared pipelines and bindings.
        entries.sort_by(|a, b| {
            a.coverage
                .cmp(&b.coverage)
                .then_with(|| a.binding.cmp(b.binding))
        });
        let mut counts = (0, 0);
        let mut state = draw_state::DrawState::default();
        let mut index = 0;
        while index < entries.len() {
            let entry = &entries[index];
            state.pipeline(
                pass,
                &self.pipeline(renderer, point, entry.coverage),
                renderer.state_caching,
            );
            state.group(pass, 0, entry.binding, renderer.state_caching);
            let mut end = index + 1;
            while end < entries.len()
                && entries[end].coverage == entry.coverage
                && entries[end].binding == entry.binding
                && entry.packed.is_some()
                && entries[end].packed.is_some()
            {
                end += 1;
            }
            if end - index >= MIN_INDIRECT_RUN {
                let offset = frame.args.len();
                for entry in &entries[index..end] {
                    let (first, base) = entry.packed.unwrap();
                    for word in [
                        entry.mesh.count,
                        entry.instances,
                        first,
                        base as u32,
                        entry.first_instance,
                    ] {
                        frame.args.extend_from_slice(&word.to_ne_bytes());
                    }
                    counts.0 += 1;
                    counts.1 += u64::from(entry.mesh.count / 3) * u64::from(entry.instances);
                }
                self.geometry
                    .bind_streams(pass, &mut state, renderer.state_caching);
                pass.multi_draw_indexed_indirect(
                    self.args.as_ref().unwrap(),
                    offset as u64,
                    (end - index) as u32,
                );
                frame.indirect_runs += 1;
                frame.indirect_draws += end - index;
                index = end;
                continue;
            }
            state.vertex(
                pass,
                0,
                &entry.mesh.vertices,
                entry.mesh.vertex_offset,
                renderer.state_caching,
            );
            state.index(pass, &entry.mesh.indices, renderer.state_caching);
            pass.draw_indexed(
                0..entry.mesh.count,
                0,
                entry.first_instance..entry.first_instance + entry.instances,
            );
            counts.0 += 1;
            counts.1 += u64::from(entry.mesh.count / 3) * u64::from(entry.instances);
            index += 1;
        }
        counts
    }
    /// Queue this frame's ID and argument streams; runs before submission.
    pub fn flush(&mut self, gpu: &Gpu) {
        let frame = self.frame.get_mut();
        self.indirect_runs = frame.indirect_runs;
        self.indirect_draws = frame.indirect_draws;
        if frame.ids.is_empty() && frame.args.is_empty() {
            return;
        }
        let ids: Vec<u8> = frame.ids.iter().flat_map(|id| id.to_ne_bytes()).collect();
        debug_assert!(ids.len() <= self.id_capacity);
        debug_assert!(frame.args.len() <= self.arg_capacity);
        self.id_bytes = write_changed(
            &gpu.queue,
            self.ids.as_ref().unwrap(),
            &self.previous_ids,
            &ids,
            4,
        );
        if !frame.args.is_empty() {
            write_changed(
                &gpu.queue,
                self.args.as_ref().unwrap(),
                &self.previous_args,
                &frame.args,
                20,
            );
        }
        self.previous_ids = ids;
        self.previous_args.clear();
        self.previous_args.extend_from_slice(&frame.args);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_caster_shader_validates_and_reads_listed_slots() {
        let source = module_text();
        assert!(source.contains("var<storage, read> objects: array<ObjectUniform>"));
        assert!(source.contains("let object_slot = instance_ids[instance];"));
        assert!(source.contains("object = objects[in.instance];"));
        let module = wgpu::naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }
    #[test]
    fn compact_records_match_the_portable_depth_layout() {
        let color = std::array::from_fn(|i| i as u8);
        let record = compact(&color);
        assert_eq!(&record[..64], &color[96..160]);
        assert_eq!(&record[64..72], &color[80..88]);
        assert_eq!(&record[72..76], &color[76..80]);
        assert_eq!(&record[76..80], &color[92..96]);
        assert_eq!(&record[80..88], &color[160..168]);
        assert!(record[88..].iter().all(|b| *b == 0));
    }
}
