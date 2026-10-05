//! Capability-selected stable object storage with compact per-group ID lists.
//! Portable devices retain 64-record uniforms; native visibility and insertion
//! edits rewrite four-byte references without moving unchanged object records.
use super::*;

pub(super) const MAX_NATIVE_INSTANCES: usize = 1024;
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Identity {
    Surface(preparation::SurfaceIdentity),
    Position(usize),
}
#[derive(Default)]
struct Table {
    identities: HashMap<Identity, usize>,
    slots: Vec<Option<usize>>,
    free: Vec<usize>,
    draw_slots: Vec<u32>,
    unique: bool,
    seen: std::collections::HashSet<preparation::SurfaceIdentity>,
}
impl Table {
    fn assign(&mut self, identities: &[Option<preparation::SurfaceIdentity>]) {
        self.seen.clear();
        let unique = identities
            .iter()
            .all(|id| id.is_some_and(|id| self.seen.insert(id)));
        if unique != self.unique || self.slots.len() > identities.len().saturating_mul(4).max(8192)
        {
            *self = Self {
                unique,
                ..Default::default()
            };
        }
        self.slots.fill(None);
        self.draw_slots.clear();
        for (index, &identity) in identities.iter().enumerate() {
            let id = if unique {
                Identity::Surface(identity.unwrap())
            } else {
                Identity::Position(index)
            };
            let slot = if let Some(&slot) = self.identities.get(&id) {
                slot
            } else {
                let slot = self.free.pop().unwrap_or_else(|| {
                    self.slots.push(None);
                    self.slots.len() - 1
                });
                self.identities.insert(id, slot);
                slot
            };
            self.slots[slot] = Some(index);
            self.draw_slots.push(slot as u32);
        }
        self.identities.retain(|_, slot| {
            if self.slots[*slot].is_some() {
                true
            } else {
                self.free.push(*slot);
                false
            }
        });
        let len = self
            .slots
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |i| i + 1);
        self.slots.truncate(len);
        self.free.retain(|&slot| slot < len);
    }
}
#[derive(Default)]
pub(super) struct Arena {
    buffers: Option<(wgpu::Buffer, wgpu::Buffer, wgpu::Buffer)>,
    object_capacity: usize,
    id_capacity: usize,
    table: Table,
    identities: Vec<Option<preparation::SurfaceIdentity>>,
    sizes: Vec<usize>,
    starts: Vec<u32>,
    textures: HashMap<TextureKind, wgpu::BindGroup>,
    object_bytes: Vec<u8>,
    parameter_bytes: Vec<u8>,
    id_bytes: Vec<u8>,
    revisions: Vec<u64>,
    parameter_revisions: Vec<u64>,
    needed: Vec<bool>,
    changed: Vec<bool>,
    parameters_changed: Vec<bool>,
    ids_changed: Vec<bool>,
}
impl Arena {
    fn assign_identities(
        &mut self,
        identities: impl Iterator<Item = Option<preparation::SurfaceIdentity>> + Clone,
    ) -> bool {
        // Exact membership/order comparison avoids rebuilding the hash tables
        // on transform, material, visibility and camera-only changes.
        if self.identities.iter().copied().eq(identities.clone()) {
            return true;
        }
        self.identities.clear();
        self.identities.extend(identities);
        self.table.assign(&self.identities);
        false
    }

    pub(super) fn buffers(&self) -> (&wgpu::Buffer, &wgpu::Buffer) {
        let (objects, parameters, _) = self
            .buffers
            .as_ref()
            .expect("prepared native instance arena");
        (objects, parameters)
    }
    pub(super) fn object_slot(&self, draw_index: usize) -> u32 {
        self.table.draw_slots[draw_index]
    }
}
pub(in crate::scene) fn supported(gpu: &Gpu) -> bool {
    let limits = gpu.device.limits();
    gpu.adapter
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::VERTEX_STORAGE)
        && gpu
            .device
            .features()
            .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
        && limits.max_storage_buffers_per_shader_stage >= 3
        && limits.max_storage_buffer_binding_size as usize >= BUFFER_BYTES
}
fn max_records(gpu: &Gpu) -> usize {
    let limits = gpu.device.limits();
    (limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size)
        / OBJECT_UNIFORM_BYTES as u64) as usize
}
pub(super) fn fits(gpu: &Gpu, draws: usize) -> bool {
    draws.saturating_mul(4).saturating_add(8192) <= max_records(gpu)
}
fn layout_ranges(
    sizes: &mut Vec<usize>,
    retained_slots: usize,
    batches: &[Batch],
    starts: &mut Vec<u32>,
) -> usize {
    sizes.truncate(retained_slots);
    sizes.resize(retained_slots, 2);
    for batch in batches.iter().filter(|b| b.indices.len() > 1) {
        let slot = batch.slot.unwrap();
        sizes.resize(sizes.len().max(slot + 1), 2);
        sizes[slot] = sizes[slot].max(batch.indices.len().next_power_of_two());
    }
    let mut total = 0;
    starts.clear();
    starts.extend(sizes.iter().map(|&size| {
        let start = total as u32;
        total += size;
        start
    }));
    total
}
fn write_runs(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    bytes: &[u8],
    stride: usize,
    changed: &[bool],
) -> usize {
    let mut start = None;
    let mut written = 0;
    for (slot, &dirty) in changed.iter().chain(std::iter::once(&false)).enumerate() {
        if dirty {
            start.get_or_insert(slot);
        } else if let Some(first) = start.take() {
            let first = first * stride;
            let end = slot * stride;
            queue.write_buffer(buffer, first as u64, &bytes[first..end]);
            written += end - first;
        }
    }
    written
}
pub(super) fn prepare(
    renderer: &mut SceneRenderer,
    gpu: &Gpu,
    draws: &[PreparedDraw],
    batches: &mut [Batch],
    bindings: &mut Vec<InstanceBinding>,
) -> Result<(usize, usize, usize)> {
    let mut arena = std::mem::take(&mut renderer.instancing.arena);
    if bindings.is_empty() {
        arena.textures.clear();
        arena.sizes.clear();
    }
    renderer.stats.native_object_membership_reused =
        arena.assign_identities(draws.iter().map(preparation::surface_identity));
    let required = arena.table.slots.len();
    let required_ids = layout_ranges(&mut arena.sizes, bindings.len(), batches, &mut arena.starts);
    ensure!(
        required <= max_records(gpu) && required_ids <= max_records(gpu) * 64,
        "native instance arena exceeds device limit"
    );
    let reallocated = arena.buffers.is_none()
        || required > arena.object_capacity
        || required_ids > arena.id_capacity
        || arena.object_capacity > 8192 && required < arena.object_capacity / 4;
    if reallocated {
        arena.object_capacity = required.next_power_of_two().max(1024).min(max_records(gpu));
        arena.id_capacity = required_ids
            .next_power_of_two()
            .max(1024)
            .min(max_records(gpu) * 64);
        let buffer = |label, size| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        arena.buffers = Some((
            buffer(
                "stable native object table",
                (arena.object_capacity * OBJECT_UNIFORM_BYTES) as u64,
            ),
            buffer(
                "stable native graph parameter table",
                (arena.object_capacity * GRAPH_PARAMETER_RECORD_BYTES) as u64,
            ),
            buffer(
                "native per-group object ID lists",
                (arena.id_capacity * 4) as u64,
            ),
        ));
        arena.textures.clear();
        arena.revisions.fill(0);
        arena.parameter_revisions.fill(0);
        arena.id_bytes.clear();
    }
    let old_parameter_len = arena.parameter_revisions.len();
    arena
        .object_bytes
        .resize(required * OBJECT_UNIFORM_BYTES, 0);
    arena
        .parameter_bytes
        .resize(required * GRAPH_PARAMETER_RECORD_BYTES, 0);
    arena.revisions.resize(required, 0);
    arena.parameter_revisions.resize(required, 0);
    arena.needed.clear();
    arena.needed.resize(required, false);
    for batch in batches.iter().filter(|b| b.indices.len() > 1) {
        for &index in &batch.indices {
            arena.needed[arena.table.draw_slots[index] as usize] = true;
        }
    }
    let (objects, parameters, ids) = arena.buffers.as_ref().unwrap();
    arena.changed.clear();
    arena.changed.resize(required, false);
    arena.parameters_changed.clear();
    arena.parameters_changed.resize(required, false);
    for (slot, &needed) in arena.needed.iter().enumerate() {
        if !needed {
            continue;
        }
        let object = &renderer.objects[arena.table.slots[slot].unwrap()];
        if !renderer.state_caching
            || reallocated
            || arena.revisions[slot] != object.uniform_revision
        {
            arena.object_bytes[slot * OBJECT_UNIFORM_BYTES..(slot + 1) * OBJECT_UNIFORM_BYTES]
                .copy_from_slice(object.uniform.as_ref().unwrap());
            arena.revisions[slot] = object.uniform_revision;
            arena.changed[slot] = true;
        }
        if !renderer.state_caching
            || arena.parameter_revisions[slot] != object.parameter_revision
            || reallocated && !object.numeric_parameters.is_empty()
            || !reallocated && slot >= old_parameter_len
        {
            arena.parameter_bytes
                [slot * GRAPH_PARAMETER_RECORD_BYTES..(slot + 1) * GRAPH_PARAMETER_RECORD_BYTES]
                .copy_from_slice(&graph_parameter_bytes(&object.numeric_parameters));
            arena.parameter_revisions[slot] = object.parameter_revision;
            arena.parameters_changed[slot] = true;
        }
    }
    let object_written = write_runs(
        &gpu.queue,
        objects,
        &arena.object_bytes,
        OBJECT_UNIFORM_BYTES,
        &arena.changed,
    );
    let parameter_written = write_runs(
        &gpu.queue,
        parameters,
        &arena.parameter_bytes,
        GRAPH_PARAMETER_RECORD_BYTES,
        &arena.parameters_changed,
    );
    let old_ids = arena.id_bytes.len();
    arena.id_bytes.resize(required_ids * 4, 0);
    arena.ids_changed.clear();
    arena.ids_changed.resize(required_ids, false);
    if arena.textures.len() > bindings.len() + 8 {
        let mut active: std::collections::HashSet<_> =
            bindings.iter().map(|b| &b.texture).collect();
        active.extend(
            batches
                .iter()
                .filter(|b| b.indices.len() > 1)
                .map(|b| &draws[b.indices[0]].object.material.texture),
        );
        arena.textures.retain(|texture, _| active.contains(texture));
    }
    for batch in batches.iter_mut().filter(|b| b.indices.len() > 1) {
        let slot = batch.slot.unwrap();
        let texture = &draws[batch.indices[0]].object.material.texture;
        if !arena.textures.contains_key(texture) {
            let binding = renderer.texture_binding(
                gpu,
                texture,
                objects,
                &renderer.instancing.layout,
                Some(parameters),
                Some(ids),
            )?;
            arena.textures.insert(texture.clone(), binding);
        }
        let binding = &arena.textures[texture];
        if slot == bindings.len() {
            bindings.push(InstanceBinding {
                buffer: objects.clone(),
                texture: texture.clone(),
                binding: binding.clone(),
                bytes: Vec::new(),
                revisions: Vec::new(),
                parameter_buffer: Some(parameters.clone()),
                parameter_bytes: Vec::new(),
                parameter_revisions: Vec::new(),
                first_instance: arena.starts[slot],
            });
        }
        let value = &mut bindings[slot];
        if value.buffer != *objects {
            value.buffer = objects.clone();
        }
        if value.parameter_buffer.as_ref() != Some(parameters) {
            value.parameter_buffer = Some(parameters.clone());
        }
        if value.binding != *binding {
            value.binding = binding.clone();
        }
        if value.texture != *texture {
            value.texture = texture.clone();
        }
        value.first_instance = arena.starts[slot];
        batch.first_instance = arena.starts[slot];
        for (instance, &index) in batch.indices.iter().enumerate() {
            let position = batch.first_instance as usize + instance;
            let start = position * 4;
            let id = arena.table.draw_slots[index].to_ne_bytes();
            if !renderer.state_caching || start >= old_ids || arena.id_bytes[start..start + 4] != id
            {
                arena.id_bytes[start..start + 4].copy_from_slice(&id);
                arena.ids_changed[position] = true;
            }
        }
    }
    let id_written = write_runs(&gpu.queue, ids, &arena.id_bytes, 4, &arena.ids_changed);
    renderer.stats.instance_id_bytes += id_written;
    renderer.instancing.arena = arena;
    Ok((
        object_written,
        usize::from(reallocated) * 3,
        parameter_written,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn identity(id: u64) -> Option<preparation::SurfaceIdentity> {
        Some(preparation::SurfaceIdentity { id, part: 0 })
    }
    #[test]
    fn stable_gpu_slots_survive_early_insertion_removal_and_invalid_id_fallback() {
        let mut table = Table::default();
        table.assign(&[identity(1), identity(2), identity(3)]);
        let slots = table.draw_slots.clone();
        table.assign(&[identity(99), identity(1), identity(2), identity(3)]);
        assert_eq!(&table.draw_slots[1..], &slots);
        table.assign(&[identity(1), identity(3)]);
        assert_eq!(table.draw_slots, [slots[0], slots[2]]);
        table.assign(&[identity(1), identity(1), None]);
        assert!(!table.unique);
        let mut slots = table.draw_slots.clone();
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(slots.len(), 3);
    }
    #[test]
    fn unchanged_membership_skips_hash_remapping_without_hiding_order_or_identity_changes() {
        let mut arena = Arena::default();
        let ids = [identity(1), identity(2), identity(3)];
        assert!(!arena.assign_identities(ids.into_iter()));
        let slots = arena.table.draw_slots.clone();
        assert!(arena.assign_identities(ids.into_iter()));
        assert_eq!(arena.table.draw_slots, slots);
        assert!(!arena.assign_identities([identity(3), identity(1), identity(2)].into_iter()));
        assert_eq!(arena.table.draw_slots, [slots[2], slots[0], slots[1]]);
        assert!(!arena.assign_identities([identity(3), identity(1)].into_iter()));
        assert!(arena.assign_identities([identity(3), identity(1)].into_iter()));
        assert!(!arena.assign_identities([identity(3), identity(3)].into_iter()));
        assert!(!arena.table.unique);
        assert!(arena.assign_identities([identity(3), identity(3)].into_iter()));
        assert!(!arena.assign_identities([None, None].into_iter()));
        assert!(arena.assign_identities([None, None].into_iter()));
        assert_eq!(arena.table.draw_slots, [0, 1]);
        assert!(!arena.assign_identities([identity(8), identity(9)].into_iter()));
        assert!(arena.table.unique);
    }
    #[test]
    fn native_ranges_reserve_only_used_references_and_keep_slots_stable() {
        let mut sizes = vec![];
        let mut starts = vec![];
        let batches = vec![
            Batch {
                indices: vec![0; 1000],
                slot: Some(0),
                first_instance: 0,
            },
            Batch {
                indices: vec![1; 2],
                slot: Some(1),
                first_instance: 0,
            },
        ];
        assert_eq!(layout_ranges(&mut sizes, 0, &batches, &mut starts), 1026);
        assert_eq!(starts, [0, 1024]);
        assert_eq!(
            layout_ranges(&mut sizes, 2, &batches[1..], &mut starts),
            1026
        );
        assert_eq!(starts, [0, 1024]);
        assert_eq!(
            layout_ranges(
                &mut vec![],
                0,
                &(0..10_000)
                    .map(|slot| Batch {
                        indices: vec![0, 1],
                        slot: Some(slot),
                        first_instance: 0
                    })
                    .collect::<Vec<_>>(),
                &mut starts
            ),
            20_000
        );
    }
}
