//! Repack heavily fragmented shadow masks into a retained compact stream.
//! Each map/static layer has distinct storage because queue writes precede draws.
use super::*;
#[derive(Default)]
pub(in crate::scene) struct Cache {
    entries: BTreeMap<usize, Entry>,
}
struct Entry {
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    texture: TextureKind,
    /// The bound view: replacing an asset keeps its key but retires the view.
    view: wgpu::TextureView,
    bytes: Vec<u8>,
}
pub(in crate::scene) struct Plan {
    pub batches: Vec<instancing::Batch>,
    pub bindings: BTreeMap<usize, wgpu::BindGroup>,
    pub saved: usize,
    pub bytes: usize,
}
impl Cache {
    pub fn clear(&mut self) {
        self.entries.clear();
    }
    pub fn prepare(
        &mut self,
        renderer: &SceneRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batches: &[instancing::Batch],
        accepted: &[bool],
    ) -> Plan {
        let mut result = Plan {
            batches: batches.to_vec(),
            bindings: BTreeMap::new(),
            saved: 0,
            bytes: 0,
        };
        let mut used = BTreeSet::new();
        for batch in &mut result.batches {
            let Some(slot) = batch.slot else {
                continue;
            };
            let runs = accepted_runs(&batch.indices, accepted);
            // Two cheap draws avoid introducing upload/packing work. Three or
            // more fragmented runs pay for one compact draw on this map.
            if runs < 3 {
                continue;
            }
            batch.indices.retain(|&index| accepted[index]);
            batch.first_instance = 0;
            if batch.indices.is_empty() {
                continue;
            }
            used.insert(slot);
            let (binding, bytes) = self.pack(renderer, device, queue, slot, &batch.indices);
            result.bytes += bytes;
            result.bindings.insert(slot, binding);
            result.saved += runs - 1;
        }
        self.entries.retain(|slot, _| used.contains(slot));
        result
    }
    /// Sun layers have no per-light frustum. Repack their compatible accepted
    /// population across old spatial chunks only when at least two draws go away.
    pub fn prepare_certified_subset(
        &mut self,
        renderer: &SceneRenderer,
        gpu: &Gpu,
        mut subset: Vec<instancing::Batch>,
        saved: usize,
    ) -> Option<Plan> {
        if saved < 2 {
            self.clear();
            return None;
        }
        // Every synthetic slot is explicitly backed by this layer's buffer,
        // including one-record groups. It must never address an unrelated old
        // full-scene shadow binding or require a skipped individual color upload.
        for (slot, batch) in subset.iter_mut().enumerate() {
            batch.slot = Some(slot);
            batch.first_instance = 0;
        }
        let mut result = Plan {
            batches: subset,
            bindings: BTreeMap::new(),
            saved,
            bytes: 0,
        };
        for batch in &result.batches {
            let slot = batch.slot.unwrap();
            let (binding, bytes) =
                self.pack(renderer, &gpu.device, &gpu.queue, slot, &batch.indices);
            result.bytes += bytes;
            result.bindings.insert(slot, binding);
        }
        self.entries.retain(|slot, _| *slot < result.batches.len());
        Some(result)
    }
    fn pack(
        &mut self,
        renderer: &SceneRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: usize,
        indices: &[usize],
    ) -> (wgpu::BindGroup, usize) {
        debug_assert!(!indices.is_empty() && indices.len() <= instancing::MAX_SHADOW_INSTANCES);
        let mut written = 0;
        let texture = &renderer.objects[indices[0]].texture;
        // Entries outlive the frames that drew their casters, so compare the
        // view itself, as render bundles and merged world text do.
        let view = renderer
            .texture_view(texture)
            .expect("validated compact shadow texture");
        let mut allocation = false;
        if let std::collections::btree_map::Entry::Vacant(entry) = self.entries.entry(slot) {
            allocation = true;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shadow compacted ranges"),
                size: instancing::SHADOW_BUFFER_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let binding = binding(renderer, device, texture, view, &buffer);
            entry.insert(Entry {
                buffer,
                binding,
                texture: texture.clone(),
                view: view.clone(),
                bytes: Vec::new(),
            });
        }
        let entry = self.entries.get_mut(&slot).unwrap();
        if entry.texture != *texture || entry.view != *view {
            entry.binding = binding(renderer, device, texture, view, &entry.buffer);
            entry.texture = texture.clone();
            entry.view = view.clone();
        }
        let mut bytes = Vec::with_capacity(indices.len() * 96);
        for &index in indices {
            let color = renderer.objects[index].uniform.as_ref().unwrap();
            bytes.extend_from_slice(&color[96..160]);
            bytes.extend_from_slice(&color[80..88]);
            bytes.extend_from_slice(&color[76..80]);
            bytes.extend_from_slice(&color[92..96]);
            bytes.extend_from_slice(&color[160..168]);
            bytes.extend_from_slice(&[0; 8]);
        }
        if allocation || !renderer.state_caching {
            queue.write_buffer(&entry.buffer, 0, &bytes);
            written += bytes.len();
        } else {
            // Uniform rows retain their original accepted order. Upload
            // only dirty runs; stale rows past a shortened batch are never
            // addressed by its instance range.
            for range in changed_rows(&entry.bytes, &bytes) {
                queue.write_buffer(&entry.buffer, range.start as u64, &bytes[range.clone()]);
                written += range.len();
            }
        }
        entry.bytes = bytes;
        (entry.binding.clone(), written)
    }
}

/// Cold quality admission shared with the retained Sun topology certificate.
/// The certificate validates source member order, binding class and mask before
/// reusing this count; local-map compaction keeps its per-batch policy above.
pub(in crate::scene) fn subset_draws_saved(
    batches: &[instancing::Batch],
    accepted: &[bool],
    subset_count: usize,
) -> usize {
    batches
        .iter()
        .map(|batch| {
            if batch.slot.is_some() {
                accepted_runs(&batch.indices, accepted)
            } else {
                batch.indices.iter().filter(|&&i| accepted[i]).count()
            }
        })
        .sum::<usize>()
        .saturating_sub(subset_count)
}
fn binding(
    renderer: &SceneRenderer,
    device: &wgpu::Device,
    texture: &TextureKind,
    view: &wgpu::TextureView,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let sampler = match texture {
        TextureKind::ModelPart(id, index) => &renderer.models[id][*index].sampler,
        TextureKind::Text => &renderer.model_sampler,
        _ => &renderer.sampler,
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow compacted ranges"),
        layout: renderer.instancing.shadow_layout.as_ref().unwrap(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn accepted_runs(indices: &[usize], accepted: &[bool]) -> usize {
    let mut prior = false;
    indices.iter().fold(0, |runs, &index| {
        let current = accepted[index];
        let count = runs + usize::from(current && !prior);
        prior = current;
        count
    })
}

fn changed_rows(previous: &[u8], current: &[u8]) -> Vec<std::ops::Range<usize>> {
    const STRIDE: usize = 96;
    let mut ranges = Vec::new();
    let mut first = None;
    for offset in (0..current.len()).step_by(STRIDE) {
        let end = offset + STRIDE;
        if previous.get(offset..end) != Some(&current[offset..end]) {
            first.get_or_insert(offset);
        } else if let Some(start) = first.take() {
            ranges.push(start..offset);
        }
    }
    if let Some(start) = first {
        ranges.push(start..current.len());
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_runs_preserve_source_order_and_nonzero_offsets() {
        let indices = [5, 0, 4, 1, 3, 2, 6];
        let accepted = [true, true, true, false, false, false, false];
        assert_eq!(accepted_runs(&indices, &accepted), 3);
        assert_eq!(
            indices
                .into_iter()
                .filter(|&i| accepted[i])
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(accepted_runs(&[5, 0, 1, 4, 2], &accepted), 2);
        assert_eq!(accepted_runs(&indices, &[false; 7]), 0);
    }

    #[test]
    fn retained_rows_upload_only_dirty_runs_and_new_tail() {
        let original = vec![0; 5 * 96];
        assert!(changed_rows(&original, &original).is_empty());
        let mut edited = original.clone();
        edited[96 + 3] = 1;
        edited[3 * 96 + 7] = 2;
        assert_eq!(changed_rows(&original, &edited), [96..192, 288..384]);
        edited[2 * 96] = 3;
        let one_run = changed_rows(&original, &edited);
        assert_eq!(one_run.len(), 1);
        assert_eq!(one_run[0], 96..384);
        assert!(changed_rows(&original, &original[..96]).is_empty());
        let new_tail = changed_rows(&original[..96], &original);
        assert_eq!(new_tail.len(), 1);
        assert_eq!(new_tail[0], 96..480);
    }
}
