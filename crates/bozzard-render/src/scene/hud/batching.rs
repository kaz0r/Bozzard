//! Retained ordered runs. Geometry stays on the GPU; only cold membership edits
//! copy/rebase it. Neither adjacent draw order nor scissor boundaries change.
use super::*;
const MAX_ITEMS: usize = 170;
const UNIFORM_BYTES: u64 = 96 * MAX_ITEMS as u64;
type Draw<'a> = (&'a MeshBuffers, ScreenText, f32, &'a Material, [u32; 4]);
#[derive(PartialEq)]
struct Source {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    offset: u64,
    count: u32,
}
impl Source {
    fn new(mesh: &MeshBuffers) -> Self {
        Self {
            vertices: mesh.vertices.clone(),
            indices: mesh.indices.clone(),
            offset: mesh.vertex_offset,
            count: mesh.count,
        }
    }
}
struct Run {
    sources: Vec<Source>,
    texture: TextureKind,
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
    bytes: Vec<u8>,
    merged: Option<(wgpu::Buffer, wgpu::Buffer, wgpu::Buffer, u32)>,
}
pub(super) struct Batching {
    merged: wgpu::RenderPipeline,
    single: wgpu::RenderPipeline,
    rebase: wgpu::ComputePipeline,
    sampler: wgpu::Sampler,
    runs: Vec<Run>,
    pub draw_calls: usize,
    pub uniform_bytes: usize,
    pub geometry_copies: usize,
}
impl Batching {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("ordered UI batch"),
                source: wgpu::ShaderSource::Wgsl(include_str!("batch.wgsl").into()),
            });
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("ordered UI batch"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
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
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("ordered UI batch"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipeline = |merged| {
            gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:Some("ordered UI batch"),layout:Some(&pipeline_layout),
            vertex:wgpu::VertexState { module:&shader,entry_point:Some(if merged {"vs_main"} else {"vs_single"}),compilation_options:Default::default(),buffers:&[
                Some(wgpu::VertexBufferLayout {array_stride:32,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2]}),
                if merged {Some(wgpu::VertexBufferLayout {array_stride:4,step_mode:wgpu::VertexStepMode::Vertex,attributes:&wgpu::vertex_attr_array![3=>Uint32]})} else {None},
            ] },
            fragment:Some(wgpu::FragmentState {module:&shader,entry_point:Some("fs_main"),compilation_options:Default::default(),targets:&[Some(wgpu::ColorTargetState {format,blend:Some(wgpu::BlendState::ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL})]}),
            primitive:Default::default(),depth_stencil:None,multisample:Default::default(),multiview_mask:None,cache:None,
        })
        };
        let rebase_shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("UI index rebasing"),
                source: wgpu::ShaderSource::Wgsl(include_str!("rebase.wgsl").into()),
            });
        let rebase = gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("UI index rebasing"),
                layout: None,
                module: &rebase_shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        Self {
            merged: pipeline(true),
            single: pipeline(false),
            rebase,
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("UI image sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            runs: Vec::new(),
            draw_calls: 0,
            uniform_bytes: 0,
            geometry_copies: 0,
        }
    }
    pub fn invalidate(&mut self) {
        self.runs.clear();
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        encoder: &mut crate::profiling::Encoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        renderer: &SceneRenderer,
        draws: &[Draw<'_>],
        encode: bool,
        scale: f32,
    ) -> Result<()> {
        self.draw_calls = 0;
        self.uniform_bytes = 0;
        self.geometry_copies = 0;
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < draws.len() {
            let mut end = start + 1;
            let mut vertices = u64::from(draws[start].0.count / 6 * 4);
            let mut indices = u64::from(draws[start].0.count);
            while end < draws.len()
                && end - start < MAX_ITEMS
                && draws[end].3.texture == draws[start].3.texture
                && draws[end].4 == draws[start].4
            {
                let next_vertices = u64::from(draws[end].0.count / 6 * 4);
                let next_indices = u64::from(draws[end].0.count);
                if (vertices + next_vertices) * 32 > gpu.device.limits().max_buffer_size
                    || (indices + next_indices) * 4
                        > gpu.device.limits().max_storage_buffer_binding_size
                {
                    break;
                }
                vertices += next_vertices;
                indices += next_indices;
                end += 1;
            }
            ranges.push(start..end);
            start = end;
        }
        self.runs.truncate(ranges.len());
        for (slot, range) in ranges.iter().enumerate() {
            let members = &draws[range.clone()];
            let sources: Vec<_> = members.iter().map(|d| Source::new(d.0)).collect();
            if self
                .runs
                .get(slot)
                .is_none_or(|run| run.sources != sources || run.texture != members[0].3.texture)
            {
                let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("UI batch uniforms"),
                    size: UNIFORM_BYTES,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("UI batch binding"),
                    layout: &self.single.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(
                                renderer.texture_view(&members[0].3.texture)?,
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                let merged = if sources.len() > 1 {
                    Some(self.merge(gpu, encoder, &sources))
                } else {
                    None
                };
                self.geometry_copies += usize::from(merged.is_some()) * sources.len() * 2;
                let run = Run {
                    sources,
                    texture: members[0].3.texture.clone(),
                    uniform,
                    binding,
                    bytes: Vec::new(),
                    merged,
                };
                if slot < self.runs.len() {
                    self.runs[slot] = run;
                } else {
                    self.runs.push(run);
                }
            }
            let run = &mut self.runs[slot];
            let mut first = None;
            let mut last = 0;
            if run.bytes.len() != members.len() * 96 {
                run.bytes.resize(members.len() * 96, 0);
                first = Some(0);
                last = run.bytes.len();
            }
            for (index, (_, screen, opacity, material, _)) in members.iter().enumerate() {
                let mut bytes = [0u8; 96];
                for (target, value) in bytes.chunks_exact_mut(4).zip(
                    screen
                        .matrix(size, scale)
                        .to_cols_array()
                        .into_iter()
                        .chain(material.tint)
                        .chain([
                            *opacity,
                            if encode { 1. } else { 0. },
                            if material.texture == TextureKind::Text {
                                1.
                            } else {
                                0.
                            },
                            0.,
                            0.,
                        ]),
                ) {
                    target.copy_from_slice(&value.to_le_bytes());
                }
                let offset = index * 96;
                if !renderer.state_caching || run.bytes[offset..offset + 96] != bytes {
                    run.bytes[offset..offset + 96].copy_from_slice(&bytes);
                    first.get_or_insert(offset);
                    last = offset + 96;
                }
            }
            if let Some(first) = first {
                gpu.queue
                    .write_buffer(&run.uniform, first as u64, &run.bytes[first..last]);
                self.uniform_bytes += last - first;
            }
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ordered UI batches after display"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        let mut state = draw_state::DrawState::default();
        for (run, range) in self.runs.iter().zip(&ranges) {
            let clip = draws[range.start].4;
            state.pipeline(
                &mut pass,
                if run.merged.is_some() {
                    &self.merged
                } else {
                    &self.single
                },
                renderer.state_caching,
            );
            state.group(&mut pass, 0, &run.binding, renderer.state_caching);
            pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
            let count = if let Some((vertices, indices, objects, count)) = &run.merged {
                state.vertex(&mut pass, 0, vertices, 0, renderer.state_caching);
                state.vertex(&mut pass, 1, objects, 0, renderer.state_caching);
                state.index(&mut pass, indices, renderer.state_caching);
                *count
            } else {
                let source = &run.sources[0];
                state.vertex(
                    &mut pass,
                    0,
                    &source.vertices,
                    source.offset,
                    renderer.state_caching,
                );
                state.index(&mut pass, &source.indices, renderer.state_caching);
                source.count
            };
            pass.draw_indexed(0..count, 0, 0..1);
            self.draw_calls += 1;
        }
        Ok(())
    }
    fn merge(
        &self,
        gpu: &Gpu,
        encoder: &mut crate::profiling::Encoder,
        sources: &[Source],
    ) -> (wgpu::Buffer, wgpu::Buffer, wgpu::Buffer, u32) {
        let vertex_count: u64 = sources.iter().map(|s| u64::from(s.count / 6 * 4)).sum();
        let index_count: u32 = sources.iter().map(|s| s.count).sum();
        let vertices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained UI vertices"),
            size: vertex_count * 32,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let indices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained UI indices"),
            size: u64::from(index_count) * 4,
            usage: wgpu::BufferUsages::INDEX
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let mut ids = Vec::with_capacity(vertex_count as usize * 4);
        let mut metadata = [0u8; 16 * MAX_ITEMS];
        let mut base = 0u32;
        let mut first = 0u32;
        for (slot, source) in sources.iter().enumerate() {
            let count = source.count / 6 * 4;
            encoder.copy_buffer_to_buffer(
                &source.vertices,
                source.offset,
                &vertices,
                u64::from(base) * 32,
                u64::from(count) * 32,
            );
            encoder.copy_buffer_to_buffer(
                &source.indices,
                0,
                &indices,
                u64::from(first) * 4,
                u64::from(source.count) * 4,
            );
            for _ in 0..count {
                ids.extend_from_slice(&(slot as u32).to_le_bytes());
            }
            for (out, value) in metadata[slot * 16..(slot + 1) * 16]
                .chunks_exact_mut(4)
                .zip([first, source.count, base, 0])
            {
                out.copy_from_slice(&value.to_le_bytes());
            }
            base += count;
            first += source.count;
        }
        let objects = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("UI vertex object indices"),
                contents: &ids,
                usage: wgpu::BufferUsages::VERTEX,
            });
        let metadata = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("UI index ranges"),
                contents: &metadata,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("UI index rebasing"),
            layout: &self.rebase.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: indices.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: metadata.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("UI index rebasing"),
            ..Default::default()
        });
        pass.set_pipeline(&self.rebase);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(
            sources.iter().map(|s| s.count).max().unwrap().div_ceil(64),
            sources.len() as u32,
            1,
        );
        (vertices, indices, objects, index_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batch_shaders_validate_with_baseline_capabilities() {
        for source in [include_str!("batch.wgsl"), include_str!("rebase.wgsl")] {
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
    #[test]
    fn ordered_hud_runs_match_singletons_and_skip_warm_work() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut optimized = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut reference = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        reference.set_hud_batching_enabled(false);
        let mut scene = RenderScene {
            skin_poses: Default::default(),
            shader_time: 0.,
            particles: vec![],
            fog: Default::default(),
            gi: None,
            lights: vec![],
            environment: EnvironmentSettings::disabled(),
            display: DisplaySettings::default(),
            lighting: Lighting::default(),
            view_projection: Mat4::IDENTITY,
            items: Vec::new(),
        };
        for index in 0..200 {
            let mut sprite = SpriteMesh::new(vec![SpriteQuad {
                rect: [0., 0., 14. + (index % 7) as f32, 16.],
                uv: [0., 0., 1., 1.],
            }])?;
            sprite.screen = Some(ScreenText {
                anchor: [0.; 2],
                offset: [
                    20. + (index % 20) as f32 * 8.,
                    20. + (index / 20) as f32 * 8.,
                ],
            });
            sprite.opacity = 0.35 + (index % 3) as f32 * 0.1;
            scene.items.push(DrawItem {
                motion_id: index + 1,
                model: Mat4::IDENTITY,
                mesh: MeshKind::Sprite(sprite),
                material: Material {
                    metallic: None,
                    roughness: None,
                    surface_overrides: Default::default(),
                    tint: [(index % 7) as f32 / 7., 0.5, 0.8],
                    uv_scale: [1.; 2],
                    texture: TextureKind::White,
                    lit: false,
                    shader: None,
                },
            });
        }
        for index in 0..80 {
            scene.items.push(DrawItem {
                motion_id: index + 201,
                model: Mat4::IDENTITY,
                mesh: MeshKind::SharedText(std::sync::Arc::new(TextMesh {
                    text: format!("Label {}", index % 5),
                    font_size: 12.,
                    screen: Some(ScreenText {
                        anchor: [0.; 2],
                        offset: [
                            20. + (index % 8) as f32 * 28.,
                            140. + (index / 8) as f32 * 6.,
                        ],
                    }),
                    opacity: 0.6,
                    ..Default::default()
                })),
                material: Material {
                    metallic: None,
                    roughness: None,
                    surface_overrides: Default::default(),
                    tint: [0.9, 0.4, 0.2],
                    uv_scale: [1.; 2],
                    texture: TextureKind::Text,
                    lit: false,
                    shader: None,
                },
            });
        }
        let capture = |renderer: &mut SceneRenderer, scene: &RenderScene| {
            crate::capture_offscreen(&gpu, 320, 240, |target| {
                renderer.draw_linear(&gpu, target, [320, 240], scene)
            })
        };
        let mut compare = |scene: &RenderScene| -> Result<()> {
            assert_eq!(
                capture(&mut optimized, scene)?.rgba,
                capture(&mut reference, scene)?.rgba,
                "ordered HUD pixels differ"
            );
            Ok(())
        };
        compare(&scene)?;
        assert_eq!(optimized.hud.as_ref().unwrap().draw_calls, 3);
        assert_eq!(reference.hud.as_ref().unwrap().draw_calls, 280);
        assert!(optimized.hud.as_ref().unwrap().geometry_copies > 0);
        // A stable view does not copy geometry, rewrite uniforms or lay out labels.
        assert_eq!(
            capture(&mut optimized, &scene)?.rgba,
            capture(&mut reference, &scene)?.rgba
        );
        let hud = optimized.hud.as_ref().unwrap();
        assert_eq!(hud.geometry_copies, 0);
        assert_eq!(hud.uniform_bytes, 0);
        assert_eq!(optimized.text.as_ref().unwrap().layouts, 0);
        for edit in 0..4 {
            match edit {
                0 => scene.items[7].material.tint = [0.8, 0.1, 0.3],
                1 => {
                    if let MeshKind::Sprite(sprite) = &mut scene.items[8].mesh {
                        sprite.clip = Some([25., 25., 70., 60.]);
                    }
                }
                2 => scene.items.swap(4, 180),
                _ => scene.items[2].material.texture = TextureKind::Checker,
            }
            assert_eq!(
                capture(&mut optimized, &scene)?.rgba,
                capture(&mut reference, &scene)?.rgba,
                "HUD edit {edit} changed ordering/coverage"
            );
        }
        optimized.set_hud_scale(2.);
        reference.set_hud_scale(2.);
        assert_eq!(
            capture(&mut optimized, &scene)?.rgba,
            capture(&mut reference, &scene)?.rgba
        );
        scene.items.clear();
        capture(&mut optimized, &scene)?;
        assert!(optimized.hud.is_none());
        assert!(optimized.text.is_none());
        println!(
            "hud_runs_proof 280->3draws warm_uniform_bytes=0 geometry_copies=0 layouts=0 order_clip_texture_density_edits_equal"
        );
        Ok(())
    }
}
