//! Consecutive stock world-text and sprite geometry shares native object storage.
//! Authored primitive order is preserved; per-vertex IDs select each object's
//! transform, tint, light mask and temporal transform in the ordinary shader.
use super::*;
use std::collections::HashMap;
const MAX_ITEMS: usize = 1024;

#[derive(PartialEq)]
struct Source {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    offset: u64,
    count: u32,
    bounds: [Vec3; 2],
}
impl Source {
    fn new(mesh: &MeshBuffers) -> Self {
        Self {
            vertices: mesh.vertices.clone(),
            indices: mesh.indices.clone(),
            offset: mesh.vertex_offset,
            count: mesh.count,
            bounds: mesh.bounds,
        }
    }
    fn matches(&self, mesh: &MeshBuffers) -> bool {
        self.vertices == mesh.vertices
            && self.indices == mesh.indices
            && self.offset == mesh.vertex_offset
            && self.count == mesh.count
            && self.bounds == mesh.bounds
    }
}
pub(in crate::scene) struct Entry {
    pub mesh: MeshBuffers,
    pub binding: wgpu::BindGroup,
    sources: Vec<Source>,
    slots: Vec<u32>,
    ids: wgpu::Buffer,
    objects: wgpu::Buffer,
    parameters: wgpu::Buffer,
    texture: TextureKind,
    texture_view: wgpu::TextureView,
}
#[derive(Default)]
pub(in crate::scene) struct Cache {
    entries: BTreeMap<usize, Entry>,
    lookup: HashMap<usize, usize>,
    rebase: Option<wgpu::ComputePipeline>,
    pub draws_saved: usize,
    pub geometry_copies: usize,
    pub id_bytes: usize,
}
fn eligible(draw: &PreparedDraw) -> bool {
    !draw.pbr
        && draw.shader.is_none()
        && draw.deformation == 0
        && match &draw.object.mesh {
            MeshKind::Text(text) => text.screen.is_none(),
            MeshKind::SharedText(text) => text.screen.is_none(),
            MeshKind::Sprite(sprite) => sprite.screen.is_none(),
            _ => false,
        }
}
impl Cache {
    pub fn entry(&self, source_item: usize) -> Option<&Entry> {
        self.lookup
            .get(&source_item)
            .and_then(|key| self.entries.get(key))
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lookup.clear();
    }
    pub fn reset_work_stats(&mut self) {
        self.draws_saved = 0;
        self.geometry_copies = 0;
        self.id_bytes = 0;
    }
    pub fn prepare(
        &mut self,
        renderer: &SceneRenderer,
        gpu: &Gpu,
        encoder: &mut crate::profiling::Encoder,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        has_particles: bool,
    ) -> Result<()> {
        self.reset_work_stats();
        self.lookup.clear();
        if !renderer.instancing.arena_enabled() || has_particles {
            self.entries.clear();
            return Ok(());
        }
        let mut used = BTreeSet::new();
        for batch in batches.iter().filter(|batch| batch.indices.len() > 1) {
            if !batch.indices.iter().all(|&i| eligible(&draws[i])) {
                continue;
            }
            let meshes: Vec<_> = batch
                .indices
                .iter()
                .map(|&i| renderer.mesh_for(&draws[i].object))
                .collect();
            // Identical geometry already uses ordinary instancing, including
            // large tilemaps/text that exceed the heterogeneous merge budget.
            if meshes.iter().all(|mesh| {
                mesh.vertices == meshes[0].vertices
                    && mesh.indices == meshes[0].indices
                    && mesh.vertex_offset == meshes[0].vertex_offset
                    && mesh.count == meshes[0].count
            }) {
                continue;
            }
            let vertices: u64 = meshes
                .iter()
                .map(|source| u64::from(source.count / 6 * 4))
                .sum();
            let indices: u64 = meshes.iter().map(|source| u64::from(source.count)).sum();
            ensure!(
                meshes.len() <= MAX_ITEMS
                    && meshes.iter().all(|source| source.count.is_multiple_of(6))
                    && vertices * 32 <= gpu.device.limits().max_buffer_size
                    && indices * 4 <= gpu.device.limits().max_storage_buffer_binding_size
                    && vertices * 4 <= gpu.device.limits().max_storage_buffer_binding_size
                    && indices <= u64::from(u32::MAX),
                "world glyph grouping exceeds conservative device budget"
            );
            let key = renderer.instancing.object_slot(batch.indices[0]) as usize;
            self.lookup.insert(draws[batch.indices[0]].source_item, key);
            used.insert(key);
            let texture = &draws[batch.indices[0]].object.material.texture;
            let slots: Vec<_> = batch
                .indices
                .iter()
                .map(|&i| renderer.instancing.object_slot(i))
                .collect();
            let (objects, parameters) = renderer.instancing.arena_buffers();
            let view = renderer.texture_view(texture)?;
            if self.entries.get(&key).is_none_or(|entry| {
                entry.sources.len() != meshes.len()
                    || entry
                        .sources
                        .iter()
                        .zip(&meshes)
                        .any(|(source, mesh)| !source.matches(mesh))
            }) {
                let sources: Vec<_> = meshes.iter().map(|mesh| Source::new(mesh)).collect();
                self.pipeline(gpu);
                let mesh = self.merge(gpu, encoder, &sources);
                self.geometry_copies += sources.len() * 2;
                let ids = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("world glyph object slots"),
                    size: vertices * 4,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let binding = renderer.native_text_binding(gpu, texture, &ids)?;
                self.entries.insert(
                    key,
                    Entry {
                        mesh,
                        binding,
                        sources,
                        slots: Vec::new(),
                        ids,
                        objects: objects.clone(),
                        parameters: parameters.clone(),
                        texture: texture.clone(),
                        texture_view: view.clone(),
                    },
                );
            }
            let entry = self.entries.get_mut(&key).unwrap();
            if entry.objects != *objects
                || entry.parameters != *parameters
                || entry.texture != *texture
                || entry.texture_view != *view
            {
                entry.binding = renderer.native_text_binding(gpu, texture, &entry.ids)?;
                entry.objects = objects.clone();
                entry.parameters = parameters.clone();
                entry.texture = texture.clone();
                entry.texture_view = view.clone();
            }
            if !renderer.state_caching || entry.slots != slots {
                let mut bytes = Vec::with_capacity(vertices as usize * 4);
                for (source, &slot) in entry.sources.iter().zip(&slots) {
                    for _ in 0..source.count / 6 * 4 {
                        bytes.extend_from_slice(&slot.to_le_bytes());
                    }
                }
                gpu.queue.write_buffer(&entry.ids, 0, &bytes);
                self.id_bytes += bytes.len();
                entry.slots = slots;
            }
            self.draws_saved += batch.indices.len() - 1;
        }
        self.entries.retain(|key, _| used.contains(key));
        Ok(())
    }
    fn pipeline(&mut self, gpu: &Gpu) {
        if self.rebase.is_some() {
            return;
        }
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("world glyph index rebasing"),
                source: wgpu::ShaderSource::Wgsl(include_str!("rebase.wgsl").into()),
            });
        self.rebase = Some(
            gpu.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("world glyph index rebasing"),
                    layout: None,
                    module: &shader,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                }),
        );
    }
    fn merge(
        &self,
        gpu: &Gpu,
        encoder: &mut crate::profiling::Encoder,
        sources: &[Source],
    ) -> MeshBuffers {
        let vertex_count: u64 = sources.iter().map(|s| u64::from(s.count / 6 * 4)).sum();
        let index_count: u32 = sources.iter().map(|s| s.count).sum();
        let vertices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained world glyph vertices"),
            size: vertex_count * 32,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let indices = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained world glyph indices"),
            size: u64::from(index_count) * 4,
            usage: wgpu::BufferUsages::INDEX
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let mut metadata = [0u8; MAX_ITEMS * 16];
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
            for (out, value) in metadata[slot * 16..slot * 16 + 16]
                .chunks_exact_mut(4)
                .zip([first, source.count, base, 0])
            {
                out.copy_from_slice(&value.to_le_bytes());
            }
            base += count;
            first += source.count;
        }
        let metadata = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("world glyph index ranges"),
                contents: &metadata,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let rebase = self.rebase.as_ref().unwrap();
        let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world glyph index rebasing"),
            layout: &rebase.get_bind_group_layout(0),
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
            label: Some("world glyph index rebasing"),
            ..Default::default()
        });
        pass.set_pipeline(rebase);
        pass.set_bind_group(0, &binding, &[]);
        pass.dispatch_workgroups(
            sources.iter().map(|s| s.count).max().unwrap().div_ceil(64),
            sources.len() as u32,
            1,
        );
        MeshBuffers {
            vertices,
            indices,
            count: index_count,
            vertex_offset: 0,
            bounds: sources.iter().fold(
                [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)],
                |bounds, source| {
                    [
                        bounds[0].min(source.bounds[0]),
                        bounds[1].max(source.bounds[1]),
                    ]
                },
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heterogeneous_world_text_and_sprites_preserve_order_motion_and_warm_streams() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut renderers = std::array::from_fn::<_, 2, _>(|_| {
            SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm)
        });
        renderers[0].set_instancing_enabled(false);
        for renderer in &mut renderers {
            renderer.set_occlusion_enabled(false);
        }
        let material = |texture| Material {
            metallic: None,
            roughness: None,
            surface_overrides: Default::default(),
            tint: [0.7, 0.4, 0.9],
            uv_scale: [1.; 2],
            texture,
            lit: false,
            shader: None,
        };
        let mut scene = RenderScene {
            skin_poses: Default::default(),
            shader_time: 0.,
            particles: vec![],
            fog: Default::default(),
            gi: None,
            lights: vec![],
            environment: EnvironmentSettings::disabled(),
            display: DisplaySettings {
                tone_mapping: false,
                ..Default::default()
            },
            lighting: Lighting {
                shadows: false,
                ..Default::default()
            },
            view_projection: glam::camera::rh::proj::directx::orthographic(
                -3., 3., -2., 2., 0.1, 20.,
            ),
            items: vec![],
        };
        scene.display.temporal_aa.enabled = true;
        for index in 0..200 {
            scene.items.push(DrawItem {
                motion_id: index + 1,
                model: Mat4::from_translation(Vec3::new(
                    (index % 20) as f32 * 0.25 - 2.6,
                    (index / 20) as f32 * 0.23 - 0.7,
                    -6. - index as f32 * 0.001,
                )) * Mat4::from_rotation_z((index % 7) as f32 * 0.02),
                mesh: MeshKind::Text(TextMesh {
                    text: format!("label{index:03}"),
                    font_size: 0.15 + (index % 5) as f32 * 0.003,
                    opacity: 0.4 + (index % 4) as f32 * 0.1,
                    ..Default::default()
                }),
                material: material(TextureKind::Text),
            });
        }
        for index in 0..200 {
            let mut sprite = SpriteMesh::new(vec![SpriteQuad {
                rect: [-0.25, 0.2, 0.4 + index as f32 * 0.0005, 0.35],
                uv: [0., 0., 1., 1.],
            }])?;
            sprite.opacity = 0.3 + (index % 5) as f32 * 0.1;
            scene.items.push(DrawItem {
                motion_id: index + 1001,
                model: Mat4::from_translation(Vec3::new(
                    (index % 20) as f32 * 0.25 - 2.5,
                    (index / 20) as f32 * 0.23 - 1.2,
                    -4. - index as f32 * 0.001,
                )) * Mat4::from_rotation_z((index % 9) as f32 * -0.04),
                mesh: MeshKind::Sprite(sprite),
                material: material(TextureKind::White),
            });
        }
        let capture = |renderer: &mut SceneRenderer, scene: &RenderScene| {
            crate::capture_offscreen(&gpu, 256, 192, |target| {
                renderer.draw(&gpu, target, [256, 192], scene)
            })
        };
        for tick in 0..8 {
            if tick == 2 {
                for item in &mut scene.items {
                    item.model *= Mat4::from_translation(Vec3::new(0.05, -0.03, 0.));
                    item.material.tint = [0.3, 0.8, 0.4];
                }
            }
            if tick == 3 {
                let MeshKind::Text(text) = &mut scene.items[13].mesh else {
                    unreachable!()
                };
                text.text = "edited Ω glyphs".into();
                let MeshKind::Sprite(sprite) = &mut scene.items[230].mesh else {
                    unreachable!()
                };
                sprite.geometry = SpriteGeometry::new(vec![
                    SpriteQuad {
                        rect: [-0.2, 0.2, 0.2, 0.2],
                        uv: [0., 0., 0.5, 1.],
                    },
                    SpriteQuad {
                        rect: [0., 0.2, 0.2, 0.2],
                        uv: [0.5, 0., 0.5, 1.],
                    },
                ])?;
            }
            if tick == 4 {
                scene.items.remove(1);
                let mut inserted = scene.items[0].clone();
                inserted.motion_id = 9001;
                let MeshKind::Text(text) = &mut inserted.mesh else {
                    unreachable!()
                };
                text.text = "early inserted".into();
                scene.items.insert(0, inserted);
            }
            if tick == 5 {
                for renderer in &mut renderers {
                    renderer.upload_image(
                        &gpu,
                        "world-edit",
                        2,
                        2,
                        &[
                            255, 0, 0, 128, 0, 255, 0, 64, 0, 0, 255, 180, 255, 255, 255, 100,
                        ],
                    )?;
                }
                scene.items[240].material.texture = TextureKind::Imported("world-edit".into());
            }
            if tick == 6 {
                renderers[1].set_native_instance_arena_enabled(false);
            }
            if tick == 7 {
                renderers[1].set_native_instance_arena_enabled(true);
            }
            assert_eq!(
                capture(&mut renderers[0], &scene)?.rgba,
                capture(&mut renderers[1], &scene)?.rgba,
                "world merged order/alpha/motion pixels tick{tick}"
            );
            if tick == 0 && renderers[1].instancing.arena_enabled() {
                assert_eq!(renderers[0].stats.color_draws, 400);
                assert_eq!(renderers[1].stats.color_draws, 2);
                assert_eq!(renderers[1].world_text.draws_saved, 398);
                assert!(renderers[1].world_text.geometry_copies > 0);
                assert!(renderers[1].world_text.id_bytes > 0);
            }
            if (tick == 1 || tick == 2) && renderers[1].instancing.arena_enabled() {
                assert_eq!(renderers[1].world_text.geometry_copies, 0);
                assert_eq!(renderers[1].world_text.id_bytes, 0);
            }
            if tick == 6 {
                assert_eq!(renderers[1].world_text.draws_saved, 0);
            }
        }
        // This light validates as data but its shadow matrix overflows after
        // cold glyph copies are recorded. A retry must recreate that stream.
        let MeshKind::Text(text) = &mut scene.items[5].mesh else {
            unreachable!()
        };
        text.text.push_str(" retry");
        let mut failed = scene.clone();
        failed.lights.push(LocalLight {
            directional: false,
            position: [3e38, 0., 0.],
            direction: [0., 0., -1.],
            color: [1.; 3],
            intensity: 1.,
            range: 0.001,
            spot_angles: Some([0., 0.1]),
            shadows: Some(Default::default()),
        });
        assert!(capture(&mut renderers[1], &failed).is_err());
        assert!(renderers[1].world_text.entries.is_empty());
        assert_eq!(
            capture(&mut renderers[0], &scene)?.rgba,
            capture(&mut renderers[1], &scene)?.rgba,
            "retry must rebuild unsubmitted glyph streams"
        );
        assert!(renderers[1].world_text.geometry_copies > 0);
        // Particle interleaving consumes full motion outputs and requires the
        // ordered transparent singleton path for both renderers.
        scene.particles.push(Particle {
            simulation: None,
            id: 1,
            position: Vec3::new(0., 0., -5.),
            velocity: Vec3::ZERO,
            size: 0.8,
            rotation: 0.,
            color: [1., 0.6, 0.2],
            opacity: 0.6,
            kind: ParticleKind::Smoke,
            softness: 0.2,
            trail_length: 0.,
            seed: 0.4,
        });
        assert_eq!(
            capture(&mut renderers[0], &scene)?.rgba,
            capture(&mut renderers[1], &scene)?.rgba,
            "particle interleaving must preserve transparent draw order"
        );
        assert_eq!(renderers[1].world_text.draws_saved, 0);
        scene.items.clear();
        scene.particles.clear();
        capture(&mut renderers[1], &scene)?;
        assert!(renderers[1].world_text.entries.is_empty());
        println!(
            "world_text_sprite_proof 400->2draws warmcopies0 warmIDbytes0 exact_alpha_order_motion_transform_tint_glyph_texture_insert_remove_portable_fallback_failed_retry_particles"
        );
        Ok(())
    }
}
