//! Compact resources for shadow-only singletons, independent of color bindings.
use super::*;
struct Entry {
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    texture: TextureKind,
    bytes: [u8; 96],
}
#[derive(Default)]
pub(in crate::scene) struct Cache {
    pub layout: Option<wgpu::BindGroupLayout>,
    entries: Vec<Option<Entry>>,
    pub allocations: usize,
    pub write_bytes: usize,
}
impl Cache {
    pub fn invalidate(&mut self) {
        self.entries.clear();
    }
    pub fn binding(&self, index: usize) -> Option<&wgpu::BindGroup> {
        self.entries.get(index)?.as_ref().map(|e| &e.binding)
    }
    pub fn prepare(
        &mut self,
        renderer: &SceneRenderer,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        needed: &[bool],
    ) -> Result<()> {
        self.allocations = 0;
        self.write_bytes = 0;
        self.entries.truncate(draws.len());
        self.entries.resize_with(draws.len(), || None);
        if !needed.iter().any(|v| *v) {
            return Ok(());
        }
        let layout = self.layout.get_or_insert_with(|| {
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("compact shadow singleton"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: wgpu::BufferSize::new(96),
                            },
                            count: None,
                        },
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
                    ],
                })
        });
        for (index, draw) in draws.iter().enumerate().filter(|(i, _)| needed[*i]) {
            let color = renderer.objects[index]
                .uniform
                .as_ref()
                .expect("validated shadow object uniform");
            let mut bytes = [0u8; 96];
            bytes[..64].copy_from_slice(&color[96..160]);
            bytes[64..72].copy_from_slice(&color[80..88]);
            bytes[72..76].copy_from_slice(&color[76..80]);
            bytes[76..80].copy_from_slice(&color[92..96]);
            bytes[80..88].copy_from_slice(&color[160..168]);
            let texture = &draw.object.material.texture;
            let current = &mut self.entries[index];
            let allocation = current.is_none();
            if allocation || current.as_ref().is_some_and(|e| e.texture != *texture) {
                let buffer = current.as_ref().map_or_else(
                    || {
                        gpu.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("compact shadow singleton"),
                            size: 96,
                            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        })
                    },
                    |e| e.buffer.clone(),
                );
                let sampler = match texture {
                    TextureKind::ModelPart(id, part) => &renderer.models[id][*part].sampler,
                    TextureKind::Text => &renderer.model_sampler,
                    _ => &renderer.sampler,
                };
                let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("compact shadow singleton"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buffer.as_entire_binding(),
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
                    ],
                });
                let previous_bytes = current.as_ref().map_or([0; 96], |e| e.bytes);
                *current = Some(Entry {
                    buffer,
                    binding,
                    texture: texture.clone(),
                    bytes: previous_bytes,
                });
                self.allocations += usize::from(allocation);
            }
            let entry = current.as_mut().unwrap();
            if allocation || !renderer.state_caching || entry.bytes != bytes {
                gpu.queue.write_buffer(&entry.buffer, 0, &bytes);
                entry.bytes = bytes;
                self.write_bytes += 96;
            }
        }
        Ok(())
    }
}
impl SceneRenderer {
    pub(in crate::scene) fn prepare_shadow_singletons(
        &mut self,
        gpu: &Gpu,
        draws: &[PreparedDraw],
        needed: &[bool],
    ) -> Result<()> {
        let mut cache = std::mem::take(&mut self.shadows.singletons);
        let result = cache.prepare(self, gpu, draws, needed);
        self.shadows.singletons = cache;
        result
    }
}
