//! Repack heavily fragmented local-light masks into a retained compact stream.
//! Each map/static layer has distinct storage because queue writes precede draws.
use super::*;
#[derive(Default)]
pub(super) struct Cache {
    entries: BTreeMap<usize, Entry>,
}
struct Entry {
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    texture: TextureKind,
    bytes: Vec<u8>,
}
pub(super) struct Plan {
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
            let mut runs = 0;
            let mut prior = false;
            for &index in &batch.indices {
                let current = accepted[index];
                runs += usize::from(current && !prior);
                prior = current;
            }
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
            let texture = &renderer.objects[batch.indices[0]].texture;
            let mut allocation = false;
            if let std::collections::btree_map::Entry::Vacant(entry) = self.entries.entry(slot) {
                allocation = true;
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("local shadow compacted ranges"),
                    size: instancing::SHADOW_BUFFER_BYTES as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let binding = binding(renderer, device, texture, &buffer);
                entry.insert(Entry {
                    buffer,
                    binding,
                    texture: texture.clone(),
                    bytes: Vec::new(),
                });
            }
            let entry = self.entries.get_mut(&slot).unwrap();
            used.insert(slot);
            if entry.texture != *texture {
                entry.binding = binding(renderer, device, texture, &entry.buffer);
                entry.texture = texture.clone();
            }
            let mut bytes = Vec::with_capacity(batch.indices.len() * 96);
            for &index in &batch.indices {
                let color = renderer.objects[index].uniform.as_ref().unwrap();
                bytes.extend_from_slice(&color[96..160]);
                bytes.extend_from_slice(&color[80..88]);
                bytes.extend_from_slice(&color[76..80]);
                bytes.extend_from_slice(&color[92..96]);
                bytes.extend_from_slice(&color[160..168]);
                bytes.extend_from_slice(&[0; 8]);
            }
            if allocation || !renderer.state_caching || entry.bytes != bytes {
                queue.write_buffer(&entry.buffer, 0, &bytes);
                result.bytes += bytes.len();
                entry.bytes = bytes;
            }
            result.bindings.insert(slot, entry.binding.clone());
            result.saved += runs - 1;
        }
        self.entries.retain(|slot, _| used.contains(slot));
        result
    }
}
fn binding(
    renderer: &SceneRenderer,
    device: &wgpu::Device,
    texture: &TextureKind,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    let sampler = match texture {
        TextureKind::ModelPart(id, index) => &renderer.models[id][*index].sampler,
        TextureKind::Text => &renderer.model_sampler,
        _ => &renderer.sampler,
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("local shadow compacted ranges"),
        layout: renderer.instancing.shadow_layout.as_ref().unwrap(),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(
                    renderer
                        .texture_view(texture)
                        .expect("validated compact shadow texture"),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
