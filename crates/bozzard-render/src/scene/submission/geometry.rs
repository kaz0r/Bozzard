//! Cold-only copies of immutable geometry into shared indirect streams. Rebased
//! indirect base_vertex/first_index preserve original vertex/index contents.
use super::*;
const MAX_GEOMETRY_BYTES: u64 = 32 * 1024 * 1024;
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key<'a> {
    vertices: [Option<(&'a wgpu::Buffer, u64)>; 3],
    indices: &'a wgpu::Buffer,
    count: u32,
}
impl<'a> Key<'a> {
    fn from_record(record: &Record<'a>) -> Self {
        Self {
            vertices: record.vertices,
            indices: record.indices,
            count: record.index_count,
        }
    }
}
#[derive(Clone)]
struct Source {
    vertices: [Option<(wgpu::Buffer, u64)>; 3],
    indices: wgpu::Buffer,
    count: u32,
}
impl Source {
    fn key(&self) -> Key<'_> {
        Key {
            vertices: self
                .vertices
                .each_ref()
                .map(|v| v.as_ref().map(|(b, o)| (b, *o))),
            indices: &self.indices,
            count: self.count,
        }
    }
    fn from_key(key: Key<'_>) -> Self {
        Self {
            vertices: key.vertices.map(|v| v.map(|(b, o)| (b.clone(), o))),
            indices: key.indices.clone(),
            count: key.count,
        }
    }
}
#[derive(Default)]
pub(in crate::scene) struct Arena {
    sources: Vec<Source>,
    vertices: [Option<wgpu::Buffer>; 3],
    indices: Option<wgpu::Buffer>,
    base: Vec<(u32, i32)>,
    enabled: bool,
}
fn vertex_count(key: Key<'_>) -> Option<u64> {
    key.vertices
        .iter()
        .enumerate()
        .filter_map(|(slot, v)| {
            v.as_ref().map(|(b, o)| {
                let stride = if slot == 1 { 48 } else { 32 };
                b.size().checked_sub(*o).map(|bytes| bytes / stride)
            })
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .min()
        .filter(|&count| count > 0)
}
impl Arena {
    pub(in crate::scene) fn prepare(
        &mut self,
        gpu: &Gpu,
        records: &[Record<'_>],
        runs: &[(usize, usize)],
    ) -> Option<Vec<u8>> {
        self.enabled = false;
        let mut keys = Vec::new();
        // wgpu Buffer Eq/Hash use immutable handle identity, never storage contents.
        #[allow(clippy::mutable_key_type)]
        let mut lookup = std::collections::HashMap::new();
        let mut mesh = vec![usize::MAX; records.len()];
        for &(start, end) in runs {
            for index in start..end {
                let record = &records[index];
                let key = Key::from_record(record);
                if !record.geometry_stable
                    || !record
                        .indices
                        .usage()
                        .contains(wgpu::BufferUsages::COPY_SRC)
                    || record
                        .vertices
                        .iter()
                        .flatten()
                        .any(|(b, _)| !b.usage().contains(wgpu::BufferUsages::COPY_SRC))
                {
                    return None;
                }
                let slot = *lookup.entry(key).or_insert_with(|| {
                    let slot = keys.len();
                    keys.push(key);
                    slot
                });
                mesh[index] = slot;
            }
        }
        if self.sources.len() != keys.len()
            || self
                .sources
                .iter()
                .zip(&keys)
                .any(|(old, new)| old.key() != *new)
        {
            let counts: Vec<_> = keys
                .iter()
                .map(|&key| vertex_count(key))
                .collect::<Option<_>>()?;
            // Stream identity and offsets stay immutable while this lookup exists.
            #[allow(clippy::mutable_key_type)]
            let mut vertex_lookup = std::collections::HashMap::new();
            let mut vertex_sources = Vec::new();
            let mut vertex_bases = Vec::with_capacity(keys.len());
            let mut vertices = 0u64;
            for (&key, &count) in keys.iter().zip(&counts) {
                let base = *vertex_lookup
                    .entry((key.vertices, count))
                    .or_insert_with(|| {
                        let base = vertices;
                        vertices += count;
                        vertex_sources.push((key.vertices, count, base));
                        base
                    });
                vertex_bases.push(base);
            }
            let indices = keys.iter().map(|k| u64::from(k.count)).sum::<u64>();
            let present: [bool; 3] =
                std::array::from_fn(|slot| keys.iter().any(|k| k.vertices[slot].is_some()));
            let size = vertices
                * (u64::from(present[0]) * 32
                    + u64::from(present[1]) * 48
                    + u64::from(present[2]) * 32)
                + indices * 4;
            if size > MAX_GEOMETRY_BYTES
                || vertices > i32::MAX as u64
                || indices > u32::MAX as u64
                || vertices * 48 > gpu.device.limits().max_buffer_size
                || indices * 4 > gpu.device.limits().max_buffer_size
            {
                return None;
            }
            self.vertices = std::array::from_fn(|slot| {
                present[slot].then(|| {
                    gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("native shared immutable vertex streams"),
                        size: vertices * if slot == 1 { 48 } else { 32 },
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                })
            });
            self.indices = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native shared immutable index stream"),
                size: indices * 4,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("cold indirect geometry arena copies"),
                });
            self.base.clear();
            for (streams, count, base) in vertex_sources {
                for (slot, source) in streams.iter().enumerate() {
                    if let Some((buffer, offset)) = source {
                        let stride = if slot == 1 { 48 } else { 32 };
                        encoder.copy_buffer_to_buffer(
                            buffer,
                            *offset,
                            self.vertices[slot].as_ref().unwrap(),
                            base * stride,
                            count * stride,
                        );
                    }
                }
            }
            let mut index = 0;
            for (&key, &base) in keys.iter().zip(&vertex_bases) {
                self.base.push((index as u32, base as i32));
                encoder.copy_buffer_to_buffer(
                    key.indices,
                    0,
                    self.indices.as_ref().unwrap(),
                    index * 4,
                    u64::from(key.count) * 4,
                );
                index += u64::from(key.count);
            }
            gpu.queue.submit(std::iter::once(encoder.finish()));
            self.sources.clear();
            self.sources
                .extend(keys.iter().copied().map(Source::from_key));
        }
        let mut bytes = Vec::with_capacity(records.len() * 20);
        for (record, mesh) in records.iter().zip(mesh) {
            let (first, base) = if mesh == usize::MAX {
                (0, 0)
            } else {
                self.base[mesh]
            };
            for word in [
                record.index_count,
                record.instances,
                first,
                base as u32,
                record.first_instance,
            ] {
                bytes.extend_from_slice(&word.to_ne_bytes());
            }
        }
        self.enabled = true;
        Some(bytes)
    }
    pub(super) fn bind(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        record: &Record<'_>,
        state: &mut DrawState,
        cache: bool,
    ) {
        if !self.enabled {
            return;
        }
        for slot in 0..3 {
            if record.vertices[slot].is_some() {
                state.vertex(pass, slot, self.vertices[slot].as_ref().unwrap(), 0, cache);
            }
        }
        state.index(pass, self.indices.as_ref().unwrap(), cache);
    }
    pub(in crate::scene) fn disable(&mut self) {
        self.enabled = false;
    }
    /// Packed (first index, base vertex) of a mesh copied by the last `prepare`.
    pub(in crate::scene) fn base_of(
        &self,
        vertices: &wgpu::Buffer,
        offset: u64,
        indices: &wgpu::Buffer,
        count: u32,
    ) -> Option<(u32, i32)> {
        if !self.enabled {
            return None;
        }
        let key = Key {
            vertices: [Some((vertices, offset)), None, None],
            indices,
            count,
        };
        self.sources
            .iter()
            .position(|source| source.key() == key)
            .map(|slot| self.base[slot])
    }
    /// Bind the packed position stream and index stream for indirect runs.
    pub(in crate::scene) fn bind_streams(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        state: &mut DrawState,
        cache: bool,
    ) {
        state.vertex(pass, 0, self.vertices[0].as_ref().unwrap(), 0, cache);
        state.index(pass, self.indices.as_ref().unwrap(), cache);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::util::DeviceExt;
    #[test]
    fn copied_streams_preserve_authored_indices_base_vertices_and_first_instances()
    -> anyhow::Result<()> {
        let gpu = pollster::block_on(Gpu::request_prefer_software(&crate::instance(
            crate::Backend::native(),
        )))?;
        let module=gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("geometry arena parity shader"),source:wgpu::ShaderSource::Wgsl(
            "struct Out {@builtin(position) position:vec4<f32>,@location(0) color:vec4<f32>} @vertex fn vs(@location(0) position:vec3<f32>,@builtin(instance_index) instance:u32)->Out {var out:Out;out.position=vec4<f32>(position,1.0);out.color=select(vec4<f32>(0.1,0.8,0.2,1.0),vec4<f32>(0.8,0.1,0.6,1.0),instance>0u);return out;} @fragment fn fs(in:Out)->@location(0) vec4<f32>{return in.color;}".into())});
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("geometry arena parity pipeline"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0=>Float32x3],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let meshes: Vec<_> = (0..2)
            .map(|i| {
                let x = if i == 0 { -0.5 } else { 0.5 };
                let vertices = [
                    [x - 0.4, -0.7, 0., 0., 0., 1., 0., 0.],
                    [x + 0.4, -0.7, 0., 0., 0., 1., 0., 0.],
                    [x, 0.7, 0., 0., 0., 1., 0., 0.],
                ];
                let indices = if i == 0 { [0u32, 1, 2] } else { [2u32, 1, 0] };
                (
                    gpu.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: None,
                            contents: &float_bytes(vertices.iter().flatten().copied()),
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_SRC,
                        }),
                    gpu.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: None,
                            contents: &indices
                                .iter()
                                .flat_map(|i| i.to_ne_bytes())
                                .collect::<Vec<_>>(),
                            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_SRC,
                        }),
                )
            })
            .collect();
        let first = if gpu
            .device
            .features()
            .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
        {
            3
        } else {
            0
        };
        let records: Vec<_> = meshes
            .iter()
            .enumerate()
            .map(|(i, (vertices, indices))| Record {
                pipeline: &pipeline,
                groups: [None; 4],
                vertices: [Some((vertices, 0)), None, None],
                indices,
                index_count: 3,
                instances: 1,
                first_instance: if i == 0 { 0 } else { first },
                geometry_stable: true,
            })
            .collect();
        let mut arena = Arena::default();
        let bytes = arena
            .prepare(&gpu, &records, &[(0, 2)])
            .expect("small immutable geometry must pack");
        assert_eq!(u32::from_ne_bytes(bytes[28..32].try_into().unwrap()), 3);
        assert_eq!(i32::from_ne_bytes(bytes[32..36].try_into().unwrap()), 3);
        let arguments = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &bytes,
                usage: wgpu::BufferUsages::INDIRECT,
            });
        let capture = |packed| {
            crate::capture_offscreen(&gpu, 64, 64, |target| {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: target,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    });
                    let mut state = DrawState::default();
                    for (index, record) in records.iter().enumerate() {
                        record.bind(&mut pass, &mut state, true);
                        if packed {
                            arena.bind(&mut pass, record, &mut state, true);
                            pass.draw_indexed_indirect(&arguments, index as u64 * 20);
                        } else {
                            pass.draw_indexed(
                                0..record.index_count,
                                0,
                                record.first_instance..record.first_instance + record.instances,
                            );
                        }
                    }
                }
                gpu.queue.submit(std::iter::once(encoder.finish()));
                Ok(())
            })
        };
        assert_eq!(capture(false)?.rgba, capture(true)?.rgba);
        assert_eq!(arena.prepare(&gpu, &records, &[(0, 2)]).unwrap(), bytes);
        Ok(())
    }
}
