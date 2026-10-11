use super::*;
pub(super) mod compaction;
mod spatial;

// Independent spot/point banks retain at most 64 MiB of extra depth.
use sun_cache::policy::{LOCAL_DEPTH_BUDGET_BYTES, retained_depth_bytes};

struct Caster {
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
    row: Vec<u8>,
}
pub(super) struct Changes {
    pub updates: Vec<Option<CasterUpdate>>,
    pub caster_checks: usize,
    pub reused_maps: usize,
}
pub(super) struct CasterUpdate {
    casters: Vec<shadows::ShadowCaster>,
    // The same exact frustum results validate the cache and encode its replacement.
    accepted: Vec<bool>,
    stable: Vec<bool>,
    static_certificate: std::cell::RefCell<Option<sun_cache::Certificate>>,
}
impl CasterUpdate {
    /// Casters this map's frustum accepted: an upper bound on its static and
    /// dynamic layers together, which partition that set.
    pub fn accepted_count(&self) -> usize {
        self.accepted.iter().filter(|accepted| **accepted).count()
    }
}
pub(super) struct ShadowMaps {
    device: wgpu::Device,
    queue: wgpu::Queue,
    range_enabled: bool,
    spatial_enabled: bool,
    spatial_index: std::cell::RefCell<Option<spatial::CasterIndex>>,
    range_layers: std::cell::RefCell<Vec<[compaction::Cache; 2]>>,
    pub range_draws_saved: std::cell::Cell<usize>,
    pub range_write_bytes: std::cell::Cell<usize>,
    pub static_depth_copies: std::cell::Cell<usize>,
    pub static_triangles_skipped: std::cell::Cell<u64>,
    pub uniform: wgpu::Buffer,
    pub depth: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
    casters: Vec<Caster>,
    matrices: Vec<Mat4>,
    resolution: u32,
    retained: Vec<Option<Vec<shadows::ShadowCaster>>>,
    receiver_bytes: Vec<u8>,
    pub receiver_write_bytes: usize,
    pub receiver_writes: usize,
    static_layers: std::cell::RefCell<Vec<sun_cache::Cache>>,
    static_depth_budget: u64,
}
fn target(gpu: &Gpu, count: usize, resolution: u32) -> (wgpu::TextureView, Vec<wgpu::TextureView>) {
    let resolution = if count == 0 { 1 } else { resolution };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("local shadow depth array"),
        size: wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: count.max(1) as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let layers = (0..count)
        .map(|i| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    (depth, layers)
}

impl ShadowMaps {
    pub fn new(
        gpu: &Gpu,
        caster_layout: &wgpu::BindGroupLayout,
        capacity: usize,
        resolution: u32,
    ) -> Self {
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("local shadow receivers"),
            size: capacity as u64 * 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let casters = (0..capacity)
            .map(|_| {
                let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("local shadow caster"),
                    size: 80,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("local shadow caster"),
                    layout: caster_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    }],
                });
                Caster {
                    uniform,
                    binding,
                    row: Vec::new(),
                }
            })
            .collect();
        let (depth, layers) = target(gpu, 0, resolution);
        Self {
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            range_enabled: true,
            spatial_enabled: true,
            spatial_index: Default::default(),
            range_layers: Default::default(),
            range_draws_saved: Default::default(),
            range_write_bytes: Default::default(),
            static_depth_copies: Default::default(),
            static_triangles_skipped: Default::default(),
            uniform,
            depth,
            layers,
            casters,
            matrices: Vec::new(),
            resolution,
            retained: Vec::new(),
            receiver_bytes: Vec::new(),
            receiver_write_bytes: 0,
            receiver_writes: 0,
            static_layers: Default::default(),
            static_depth_budget: LOCAL_DEPTH_BUDGET_BYTES,
        }
    }
    /// Map ordering is rebuilt each frame; every pass has its own buffer because
    /// queue writes all precede command execution.
    pub fn update(&mut self, gpu: &Gpu, maps: &[(Mat4, LocalShadowSettings)]) -> Result<bool> {
        ensure!(
            maps.len() <= self.casters.len(),
            "too many local shadow maps"
        );
        let changed = maps.len() != self.layers.len();
        if changed {
            if maps.is_empty() {
                *self.spatial_index.borrow_mut() = None;
            }
            ensure!(
                self.resolution <= gpu.device.limits().max_texture_dimension_2d
                    && maps.len() as u32 <= gpu.device.limits().max_texture_array_layers,
                "local shadow maps exceed device limits"
            );
            (self.depth, self.layers) = target(gpu, maps.len(), self.resolution);
            self.retained = vec![None; maps.len()];
            *self.range_layers.borrow_mut() = (0..maps.len())
                .map(|_| std::array::from_fn(|_| compaction::Cache::default()))
                .collect();
            *self.static_layers.borrow_mut() = (0..maps.len())
                .map(|_| sun_cache::Cache::default())
                .collect();
        }
        self.receiver_write_bytes = 0;
        self.receiver_writes = 0;
        let old_len = self.receiver_bytes.len();
        self.receiver_bytes.resize(self.casters.len() * 80, 0);
        let mut dirty_start = None;
        let mut dirty_end = 0;
        for (slot, (matrix, shadow)) in maps.iter().enumerate() {
            let row = float_bytes(matrix.to_cols_array().into_iter().chain([
                shadow.bias,
                shadow.normal_bias,
                0.,
                1. / self.resolution as f32,
            ]));
            let start = slot * 80;
            let end = start + 80;
            if start >= old_len || self.receiver_bytes[start..end] != row {
                self.receiver_bytes[start..end].copy_from_slice(&row);
                dirty_start.get_or_insert(start);
                dirty_end = end;
            } else if let Some(first) = dirty_start.take() {
                gpu.queue.write_buffer(
                    &self.uniform,
                    first as u64,
                    &self.receiver_bytes[first..start],
                );
                self.receiver_write_bytes += start - first;
                self.receiver_writes += 1;
            }
            if self.casters[slot].row != row {
                self.retained[slot] = None;
                gpu.queue.write_buffer(&self.casters[slot].uniform, 0, &row);
                self.casters[slot].row = row;
            }
        }
        let tail = maps.len() * 80;
        if let Some(last) = self.receiver_bytes[tail..].iter().rposition(|v| *v != 0) {
            dirty_end = (tail + last + 1).div_ceil(80) * 80;
            self.receiver_bytes[tail..].fill(0);
            dirty_start.get_or_insert(tail);
        }
        if let Some(first) = dirty_start {
            gpu.queue.write_buffer(
                &self.uniform,
                first as u64,
                &self.receiver_bytes[first..dirty_end],
            );
            self.receiver_write_bytes += dirty_end - first;
            self.receiver_writes += 1;
        }
        self.matrices.clear();
        self.matrices.extend(maps.iter().map(|(m, _)| *m));
        Ok(changed)
    }
    pub fn reset_work_stats(&mut self) {
        self.receiver_write_bytes = 0;
        self.receiver_writes = 0;
        self.range_draws_saved.set(0);
        self.range_write_bytes.set(0);
        self.static_depth_copies.set(0);
        self.static_triangles_skipped.set(0);
    }
    pub fn set_spatial_enabled(&mut self, enabled: bool) {
        self.spatial_enabled = enabled;
    }
    pub fn set_range_enabled(&mut self, enabled: bool) {
        self.range_enabled = enabled;
    }
    /// Release only optional static sources, preserving current working maps.
    pub fn release_static_depth(&self) {
        for cache in &mut *self.static_layers.borrow_mut() {
            cache.clear();
        }
    }
    /// Age all static sources once when the complete working maps are reused.
    pub fn age_unused_static_depth(&self) {
        for cache in &mut *self.static_layers.borrow_mut() {
            cache.age_unused();
        }
    }
    pub fn invalidate(&mut self) {
        self.retained.fill(None);
        *self.spatial_index.get_mut() = None;
        for layers in self.range_layers.get_mut() {
            for cache in layers {
                cache.clear();
            }
        }
        for cache in self.static_layers.get_mut() {
            cache.clear();
        }
    }

    /// Use the exact caster predicate used by the depth pass, independently for
    /// each spot map / point-light face. Entering and leaving a frustum both change the key.
    pub fn changes(
        &self,
        renderer: &SceneRenderer,
        draws: &[PreparedDraw],
        same_casters: bool,
    ) -> Changes {
        if renderer.state_caching && same_casters && self.retained.iter().all(Option::is_some) {
            return Changes {
                updates: (0..self.matrices.len()).map(|_| None).collect(),
                caster_checks: 0,
                reused_maps: self.matrices.len(),
            };
        }
        let mut caster_checks = 0;
        let mut reused_maps = 0;
        let mut retained_index = self.spatial_index.borrow_mut();
        let use_index = self.spatial_enabled
            && renderer.culling
            && draws.len() >= 256
            && !self.matrices.is_empty();
        if use_index {
            if let Some(index) = &mut *retained_index {
                index.refresh(renderer, draws);
            } else {
                *retained_index = Some(spatial::CasterIndex::new(renderer, draws));
            }
        }
        let updates = self
            .matrices
            .iter()
            .enumerate()
            .map(|(slot, matrix)| {
                // Projection/settings edits and resized textures invalidate the
                // retained map in update(), even when caster state is unchanged.
                if renderer.state_caching && same_casters && self.retained[slot].is_some() {
                    reused_maps += 1;
                    return None;
                }
                let mut accepted = vec![false; draws.len()];
                let candidates = if use_index {
                    retained_index.as_ref().unwrap().query(*matrix)
                } else {
                    (0..draws.len()).collect()
                };
                let casters: Vec<_> = candidates
                    .into_iter()
                    .map(|i| (i, &draws[i]))
                    .filter(|(_, d)| !d.transparent && d.object.material.lit)
                    .inspect(|_| caster_checks += 1)
                    .filter(|(index, d)| {
                        let visible = !renderer.culling
                            || renderer.frustum_visible(
                                renderer.mesh_for(&d.object).bounds,
                                *matrix * d.object.model,
                            );
                        accepted[*index] = visible;
                        visible
                    })
                    .map(|(_, d)| shadows::ShadowCaster::new(d))
                    .collect();
                let mut stable = vec![false; draws.len()];
                if let Some(old) = self.retained[slot]
                    .as_ref()
                    .filter(|old| old.len() == casters.len())
                {
                    for (prior, (index, draw)) in old
                        .iter()
                        .zip(draws.iter().enumerate().filter(|(i, _)| accepted[*i]))
                    {
                        stable[index] = draw.deformation == 0 && prior.matches(draw);
                    }
                }
                (!renderer.state_caching || self.retained[slot].as_ref() != Some(&casters))
                    .then_some(CasterUpdate {
                        casters,
                        accepted,
                        stable,
                        static_certificate: Default::default(),
                    })
            })
            .collect();
        Changes {
            updates,
            caster_checks,
            reused_maps,
        }
    }
    pub fn invalidate_changes(&mut self, changes: &[Option<CasterUpdate>]) {
        for (slot, change) in changes.iter().enumerate() {
            if change.is_some() {
                self.retained[slot] = None;
            }
        }
    }
    pub fn finish(&mut self, changes: Vec<Option<CasterUpdate>>) {
        for (slot, change) in changes.into_iter().enumerate() {
            if let Some(change) = change {
                if let Some(certificate) = change.static_certificate.into_inner() {
                    self.static_layers.get_mut()[slot]
                        .finish_retained(&self.casters[slot].row, certificate);
                }
                self.retained[slot] = Some(change.casters);
            }
        }
    }
    pub fn draw(
        &self,
        renderer: &SceneRenderer,
        encoder: &mut crate::profiling::Encoder,
        draws: &[PreparedDraw],
        batches: &[instancing::Batch],
        point: bool,
        changes: &[Option<CasterUpdate>],
    ) -> (usize, u64) {
        let mut counts = (0, 0);
        let mut caches = self.static_layers.borrow_mut();
        let mut ranges = self.range_layers.borrow_mut();
        let caching = renderer.state_caching && renderer.shadow_preparation_cache;
        let mut cached_bytes = 0;
        // Existing sources keep deterministic map priority. Unchanged faces
        // have a bounded idle grace. Bypassed or over-budget sources release
        // handles before admission; required working maps remain intact.
        for (slot, cache) in caches.iter_mut().enumerate() {
            if caching && changes[slot].is_none() {
                cache.age_unused();
            }
            if let Some(total) = retained_depth_bytes(
                cached_bytes,
                cache.bytes(),
                self.static_depth_budget,
                caching,
            ) {
                cached_bytes = total;
            } else {
                cache.clear();
            }
        }
        for (slot, _) in self.matrices.iter().enumerate() {
            let Some(change) = &changes[slot] else {
                continue;
            };
            let work = draws
                .iter()
                .enumerate()
                .filter(|(i, _)| change.accepted[*i])
                .fold((0u64, 0u64), |(a, b), (i, d)| {
                    let triangles = u64::from(renderer.mesh_for(&d.object).count / 3);
                    if change.stable[i] {
                        (a + triangles, b)
                    } else {
                        (a, b + triangles)
                    }
                });
            let previous_bytes = caches[slot].bytes();
            let available = self
                .static_depth_budget
                .saturating_sub(cached_bytes - previous_bytes);
            let plan = if caching {
                // A local cache contains only this light's accepted stable
                // casters. Its retained key list uses the same subset.
                let static_mask = change
                    .stable
                    .iter()
                    .zip(&change.accepted)
                    .map(|(stable, accepted)| *stable && *accepted)
                    .collect();
                caches[slot].prepare_with_budget(
                    &self.device,
                    draws,
                    batches,
                    static_mask,
                    &self.casters[slot].row,
                    self.resolution,
                    work,
                    available,
                    renderer.caster_serials(),
                )
            } else {
                None
            };
            cached_bytes = cached_bytes - previous_bytes + caches[slot].bytes();
            debug_assert!(cached_bytes <= self.static_depth_budget);
            if let Some(plan) = &plan
                && plan.rebuild
            {
                let compact = if self.range_enabled
                    && renderer.instancing.shadow_batching()
                    && !renderer.native_shadows_active()
                {
                    Some(ranges[slot][0].prepare(
                        renderer,
                        &self.device,
                        &self.queue,
                        batches,
                        &plan.static_mask,
                    ))
                } else {
                    None
                };
                if let Some(compact) = &compact {
                    self.range_draws_saved
                        .set(self.range_draws_saved.get() + compact.saved);
                    self.range_write_bytes
                        .set(self.range_write_bytes.get() + compact.bytes);
                }
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("local static shadow casters"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: caches[slot].depth(),
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_bind_group(1, &self.casters[slot].binding, &[]);
                let work = renderer.draw_shadow_casters_with_bindings(
                    &mut pass,
                    draws,
                    compact.as_ref().map_or(batches, |p| p.batches.as_slice()),
                    None,
                    point,
                    Some(&plan.static_mask),
                    compact.as_ref().map(|p| &p.bindings),
                );
                counts.0 += work.0;
                counts.1 += work.1;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("local shadow casters"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.layers[slot],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            let dynamic;
            let accepted = if let Some(plan) = &plan {
                // Always restore an immutable source. Loading and modifying that
                // source itself would leave stale depth when a mover departs.
                caches[slot].copy(&mut pass);
                self.static_depth_copies
                    .set(self.static_depth_copies.get() + 1);
                if !plan.rebuild {
                    self.static_triangles_skipped
                        .set(self.static_triangles_skipped.get() + work.0);
                }
                counts.0 += 1;
                counts.1 += 1;
                dynamic = plan
                    .dynamic_mask
                    .iter()
                    .zip(&change.accepted)
                    .map(|(dynamic, accepted)| *dynamic && *accepted)
                    .collect::<Vec<_>>();
                &dynamic
            } else {
                &change.accepted
            };
            pass.set_bind_group(1, &self.casters[slot].binding, &[]);
            let compact = if self.range_enabled
                && renderer.instancing.shadow_batching()
                && !renderer.native_shadows_active()
            {
                Some(ranges[slot][1].prepare(
                    renderer,
                    &self.device,
                    &self.queue,
                    batches,
                    accepted,
                ))
            } else {
                None
            };
            if let Some(compact) = &compact {
                self.range_draws_saved
                    .set(self.range_draws_saved.get() + compact.saved);
                self.range_write_bytes
                    .set(self.range_write_bytes.get() + compact.bytes);
            }
            let (submitted_draws, triangles) = renderer.draw_shadow_casters_with_bindings(
                &mut pass,
                draws,
                compact.as_ref().map_or(batches, |p| p.batches.as_slice()),
                None,
                point,
                Some(accepted),
                compact.as_ref().map(|p| &p.bindings),
            );
            counts.0 += submitted_draws;
            counts.1 += triangles;
            drop(pass);
            if let Some(plan) = &plan
                && plan.rebuild
            {
                *change.static_certificate.borrow_mut() = Some(sun_cache::Certificate::new(
                    &plan.static_mask,
                    renderer.caster_serials(),
                ));
            }
        }
        counts
    }
}

impl SceneRenderer {
    pub fn set_shadow_spatial_culling_enabled(&mut self, enabled: bool) {
        self.shadows.spots.set_spatial_enabled(enabled);
        self.shadows.points.set_spatial_enabled(enabled);
    }
    pub fn set_shadow_range_compaction_enabled(&mut self, enabled: bool) {
        self.shadows.spots.set_range_enabled(enabled);
        self.shadows.points.set_range_enabled(enabled);
        self.shadows.sun_cache.set_range_enabled(enabled);
    }
}

#[cfg(test)]
mod optimization_tests {
    use super::*;
    fn material() -> Material {
        Material {
            metallic: None,
            roughness: None,
            surface_overrides: Default::default(),
            tint: [0.5, 0.7, 0.4],
            uv_scale: [1.; 2],
            texture: TextureKind::White,
            lit: true,
            shader: None,
        }
    }
    fn scene(items: Vec<DrawItem>) -> RenderScene {
        RenderScene {
            skin_poses: Default::default(),
            shader_time: 0.,
            particles: vec![],
            fog: Default::default(),
            gi: None,
            lights: vec![LocalLight {
                directional: false,
                position: [0., 0., 2.],
                direction: [0., 0., -1.],
                color: [1.; 3],
                intensity: 4.,
                range: 20.,
                spot_angles: Some([35., 45.]),
                shadows: Some(Default::default()),
            }],
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
                -4., 4., -4., 4., 0.1, 30.,
            ),
            items,
        }
    }
    fn capture(
        gpu: &Gpu,
        renderer: &mut SceneRenderer,
        scene: &RenderScene,
    ) -> Result<crate::Frame> {
        crate::capture_offscreen(gpu, 160, 160, |target| {
            renderer.draw_linear(gpu, target, [160; 2], scene)
        })
    }
    fn static_bytes(maps: &ShadowMaps) -> u64 {
        maps.static_layers
            .borrow()
            .iter()
            .map(sun_cache::Cache::bytes)
            .sum()
    }
    #[test]
    fn receiver_updates_only_dirty_rows_and_clears_retired_slots() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut maps = ShadowMaps::new(&gpu, &renderer.shadows.caster_layout, 4, 64);
        let mut rows = vec![
            (Mat4::IDENTITY, LocalShadowSettings::default()),
            (
                Mat4::from_translation(Vec3::X),
                LocalShadowSettings::default(),
            ),
        ];
        maps.update(&gpu, &rows)?;
        assert_eq!(maps.receiver_write_bytes, 160);
        assert_eq!(maps.receiver_writes, 1);
        maps.update(&gpu, &rows)?;
        assert_eq!(maps.receiver_write_bytes, 0);
        assert_eq!(maps.receiver_writes, 0);
        rows[1].0 *= Mat4::from_rotation_y(0.2);
        maps.update(&gpu, &rows)?;
        assert_eq!(maps.receiver_write_bytes, 80);
        rows.truncate(1);
        maps.update(&gpu, &rows)?;
        assert_eq!(maps.receiver_write_bytes, 80);
        assert!(maps.receiver_bytes[80..].iter().all(|v| *v == 0));
        rows.clear();
        maps.update(&gpu, &rows)?;
        assert_eq!(maps.receiver_write_bytes, 80);
        println!("local_receiver_proof warm0 edit80 retire80 initial160of320bytes");
        Ok(())
    }
    #[test]
    fn fragmented_local_ranges_compact_after_retained_spatial_order() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut renderers = std::array::from_fn::<_, 2, _>(|_| {
            SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm)
        });
        renderers[0].set_shadow_range_compaction_enabled(false);
        for renderer in &mut renderers {
            renderer.set_occlusion_enabled(false);
            renderer.set_shadow_preparation_caching_enabled(false);
            // Covers portable range compaction; native lists have their own proof.
            renderer.set_native_shadow_lists_enabled(false);
        }
        let mut scene = scene(
            (0..160)
                .map(|index| DrawItem {
                    motion_id: index + 1,
                    model: Mat4::from_translation(Vec3::new(0., 0., -5.)),
                    mesh: MeshKind::Cube,
                    material: material(),
                })
                .collect(),
        );
        assert_eq!(
            capture(&gpu, &mut renderers[0], &scene)?.rgba,
            capture(&gpu, &mut renderers[1], &scene)?.rgba
        );
        for (index, item) in scene.items.iter_mut().enumerate() {
            if index % 2 == 1 {
                item.model = Mat4::from_translation(Vec3::new(20., 0., -5.));
            }
        }
        assert_eq!(
            capture(&gpu, &mut renderers[0], &scene)?.rgba,
            capture(&gpu, &mut renderers[1], &scene)?.rgba
        );
        assert_eq!(
            renderers[0].stats.shadow_triangles,
            renderers[1].stats.shadow_triangles
        );
        assert_eq!(renderers[0].stats.shadow_draws, 80);
        assert_eq!(renderers[1].stats.shadow_draws, 1);
        assert_eq!(renderers[1].shadows.spots.range_draws_saved.get(), 79);
        assert_eq!(renderers[1].shadows.spots.range_write_bytes.get(), 80 * 96);
        assert_eq!(
            renderers[1].stats.local_shadow_caster_checks, 160,
            "one validation scan, no second draw visibility scan"
        );
        println!(
            "local_ranges_proof 80->1draws 960triangles unchanged checks160once packed7680bytes"
        );
        Ok(())
    }
    #[test]
    fn fragmented_sun_layers_compact_and_retain_exact_depth_rows() -> Result<()> {
        let exact = |reference: &crate::Frame, candidate: crate::Frame, context: &str| {
            assert_eq!(reference.rgba.len(), candidate.rgba.len(), "{context}");
            let different = reference
                .rgba
                .chunks_exact(4)
                .zip(candidate.rgba.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(different, 0, "{context}: pixels differ");
        };
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut renderers = std::array::from_fn::<_, 3, _>(|_| {
            SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm)
        });
        renderers[0].set_shadow_preparation_caching_enabled(false);
        renderers[1].set_shadow_range_compaction_enabled(false);
        for renderer in &mut renderers {
            // Covers portable sun range compaction; native lists have their own proof.
            renderer.set_native_shadow_lists_enabled(false);
        }
        let vertices = [
            [-0.12, -0.12, 0., 0., 0., 1., 0., 1.],
            [0.12, -0.12, 0., 0., 0., 1., 1., 1.],
            [0., 0.12, 0., 0., 0., 1., 0.5, 0.],
        ];
        let attributes = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 3];
        for renderer in &mut renderers {
            renderer.set_occlusion_enabled(false);
            renderer.upload_mesh(
                &gpu,
                "sun-range-receiver",
                &[
                    [-3.5, -3.5, 0., 0., 0., 1., 0., 1.],
                    [3.5, -3.5, 0., 0., 0., 1., 1., 1.],
                    [0., 3.5, 0., 0., 0., 1., 0.5, 0.],
                ],
                &(0..40_000).flat_map(|_| [0, 1, 2]).collect::<Vec<_>>(),
            )?;
            renderer.upload_model(
                &gpu,
                "sun-range-triangle",
                &vertices,
                &[0, 1, 2],
                &[ModelPart {
                    source_key: "0000000000000000",
                    start: 0,
                    count: 3,
                    color: [1.; 4],
                    alpha_cutoff: None,
                    image: None,
                    shading: Some(crate::ModelShading {
                        vertex_start: 0,
                        vertices: &attributes,
                        metallic: 0.,
                        roughness: 0.7,
                        normal_scale: 1.,
                        occlusion_strength: 1.,
                        emissive_factor: [0.; 3],
                        double_sided: false,
                        base_color_sampler: Default::default(),
                        normal: None,
                        metallic_roughness: None,
                        occlusion: None,
                        emissive: None,
                    }),
                }],
            )?;
        }
        let mut items = vec![DrawItem {
            motion_id: 1,
            model: Mat4::from_translation(Vec3::new(0., 0., -6.)),
            mesh: MeshKind::Imported("sun-range-receiver".into()),
            material: material(),
        }];
        items.extend((0..26).map(|index| DrawItem {
            motion_id: index + 2,
            model: Mat4::IDENTITY,
            mesh: MeshKind::ModelPart("sun-range-triangle".into(), 0),
            material: material(),
        }));
        // A fixed near-depth bound keeps the fitted sun matrix independent of
        // the movers, while the expensive receiver supplies the far/XY bounds.
        items.push(DrawItem {
            motion_id: 28,
            model: Mat4::from_translation(Vec3::new(3., 2., -1.)),
            mesh: MeshKind::ModelPart("sun-range-triangle".into(), 0),
            material: material(),
        });
        let mut scene = scene(items);
        scene.lights.clear();
        scene.lighting.shadows = true;
        scene.lighting.shadow_resolution = 256;
        scene.lighting.sun_direction = [0.15, 0.1, 1.];
        let update = |scene: &mut RenderScene, tick: usize| {
            for (index, item) in scene.items[1..27].iter_mut().enumerate() {
                let moving = index % 2 == 1;
                let pair = index / 2;
                item.model =
                    Mat4::from_translation(Vec3::new(
                        -1.8 + (pair % 7) as f32 * 0.5 + if moving { 0.2 } else { 0. },
                        -1.8 + (pair / 7) as f32 * 1.
                            + if moving { tick as f32 * 0.01 } else { 0. },
                        -3.,
                    )) * Mat4::from_scale(Vec3::new(if pair % 2 == 0 { 1. } else { -1. }, 1., 1.));
            }
        };
        for tick in 0..4 {
            update(&mut scene, tick);
            let full = capture(&gpu, &mut renderers[0], &scene)?;
            exact(
                &full,
                capture(&gpu, &mut renderers[1], &scene)?,
                "uncompacted sun cache",
            );
            exact(
                &full,
                capture(&gpu, &mut renderers[2], &scene)?,
                "compacted sun cache",
            );
            if tick >= 2 {
                assert!(renderers[1].stats.sun_static_cache_reused);
                assert!(renderers[2].stats.sun_static_cache_reused);
                assert_eq!(renderers[2].stats.sun_dynamic_casters, 13);
                assert_eq!(renderers[1].stats.shadow_draws, 14);
                assert_eq!(renderers[2].stats.shadow_draws, 3);
                assert_eq!(renderers[1].stats.shadow_triangles, 14);
                assert_eq!(renderers[2].stats.shadow_triangles, 14);
                assert_eq!(renderers[2].shadows.sun_cache.range_draws_saved.get(), 11);
                assert_eq!(renderers[2].frame_stats().sun_range_plan_builds, 0);
                assert_eq!(renderers[2].frame_stats().sun_range_plan_reuses, 1);
                assert_eq!(
                    renderers[2].shadows.sun_cache.range_write_bytes.get(),
                    13 * 96
                );
            }
            if tick == 3 {
                let mut shadowless = scene.clone();
                shadowless.lighting.shadows = false;
                assert!(
                    full.rgba != capture(&gpu, &mut renderers[0], &shadowless)?.rgba,
                    "the exact comparison must observe sun shadows"
                );
            }
        }
        let candidate = &mut renderers[2];
        // Inspect the actual retained stream independently of whole-map reuse:
        // fixed accepted members cost zero bytes, one edited model row costs 96.
        let draws = candidate.prepare(&scene);
        let batches = candidate.prepare_shadow_instances(&gpu, &draws)?;
        let accepted = draws
            .iter()
            .map(|draw| draw.object.motion_id >= 2 && draw.object.motion_id % 2 == 1)
            .collect::<Vec<_>>();
        candidate.shadows.sun_cache.reset_work_stats();
        let warm = candidate
            .shadows
            .sun_cache
            .prepare_ranges(candidate, &gpu, &draws, &batches, &accepted, true)
            .unwrap();
        assert_eq!(warm.bytes, 0);
        assert_eq!(warm.saved, 11);
        assert_eq!(candidate.shadows.sun_cache.range_plan_builds.get(), 0);
        assert_eq!(candidate.shadows.sun_cache.range_plan_reuses.get(), 1);
        let mut compacted_coverage = Vec::new();
        for batch in &warm.batches {
            if warm
                .bindings
                .contains_key(&batch.slot.unwrap_or(usize::MAX))
            {
                assert_eq!(batch.first_instance, 0);
                let coverage =
                    candidate.shadow_coverage_at(batch.indices[0], &draws[batch.indices[0]]);
                compacted_coverage.push(coverage);
                assert!(batch.indices.iter().all(|&index| {
                    accepted[index]
                        && candidate.shadow_coverage_at(index, &draws[index]) == coverage
                }));
            }
        }
        assert!(compacted_coverage.contains(&shadows::ShadowCoverage::OpaqueCcw));
        assert!(compacted_coverage.contains(&shadows::ShadowCoverage::OpaqueCw));
        let index = accepted.iter().position(|&v| v).unwrap();
        // A changed population cannot reuse its previous grouping/admission.
        // Returning to the original mask also refreshes the certificate, while
        // later model-row changes retain it and update their packed bytes only.
        let mut reduced = accepted.clone();
        reduced[index] = false;
        candidate.shadows.sun_cache.reset_work_stats();
        let changed = candidate
            .shadows
            .sun_cache
            .prepare_ranges(candidate, &gpu, &draws, &batches, &reduced, true)
            .unwrap();
        assert!(
            changed
                .batches
                .iter()
                .flat_map(|batch| &batch.indices)
                .all(|&index| reduced[index])
        );
        assert_eq!(candidate.shadows.sun_cache.range_plan_builds.get(), 1);
        assert_eq!(candidate.shadows.sun_cache.range_plan_reuses.get(), 0);
        candidate.shadows.sun_cache.reset_work_stats();
        assert_eq!(
            candidate
                .shadows
                .sun_cache
                .prepare_ranges(candidate, &gpu, &draws, &batches, &accepted, true)
                .unwrap()
                .saved,
            11
        );
        assert_eq!(candidate.shadows.sun_cache.range_plan_builds.get(), 1);
        assert_eq!(candidate.shadows.sun_cache.range_plan_reuses.get(), 0);
        candidate.shadows.sun_cache.reset_work_stats();
        let original = *candidate.objects[index].uniform.as_ref().unwrap();
        candidate.objects[index].uniform.as_mut().unwrap()[144..148]
            .copy_from_slice(&0.125f32.to_le_bytes());
        let dirty = candidate
            .shadows
            .sun_cache
            .prepare_ranges(candidate, &gpu, &draws, &batches, &accepted, true)
            .unwrap();
        assert_eq!(dirty.bytes, 96);
        candidate.objects[index].uniform = Some(original);
        assert_eq!(
            candidate
                .shadows
                .sun_cache
                .prepare_ranges(candidate, &gpu, &draws, &batches, &accepted, true,)
                .unwrap()
                .bytes,
            96
        );
        assert_eq!(candidate.shadows.sun_cache.range_plan_builds.get(), 0);
        assert_eq!(candidate.shadows.sun_cache.range_plan_reuses.get(), 2);
        // prepare() lends the retained frame's draws to its caller. Normal
        // rendering returns them after submission; this packing-only inspection
        // must do the same before testing a subsequent renderer frame.
        candidate.surface_preparation.draws = draws;
        // Diagnostic fallbacks preserve nonzero accepted ranges and exact pixels;
        // compact records must never bind against the ordinary color layout.
        candidate.set_shadow_batching_enabled(false);
        update(&mut scene, 4);
        let full = capture(&gpu, &mut renderers[0], &scene)?;
        exact(
            &full,
            capture(&gpu, &mut renderers[2], &scene)?,
            "disabled shadow batching",
        );
        assert_eq!(renderers[2].shadows.sun_cache.range_draws_saved.get(), 0);
        renderers[2].set_shadow_batching_enabled(true);
        renderers[2].set_instancing_enabled(false);
        update(&mut scene, 5);
        let full = capture(&gpu, &mut renderers[0], &scene)?;
        exact(
            &full,
            capture(&gpu, &mut renderers[2], &scene)?,
            "disabled color batching",
        );
        assert_eq!(renderers[2].shadows.sun_cache.range_draws_saved.get(), 0);
        renderers[2].set_instancing_enabled(true);
        scene
            .items
            .retain(|item| item.motion_id == 1 || item.motion_id == 28);
        scene.items.extend((0..1190).map(|index| DrawItem {
            motion_id: 100 + index,
            model: Mat4::IDENTITY,
            mesh: MeshKind::Cube,
            material: material(),
        }));
        let movers = [5, 175, 345, 515, 685, 855, 1025, 1026];
        for tick in 0..4 {
            for (index, item) in scene.items[2..].iter_mut().enumerate() {
                item.model = Mat4::from_translation(Vec3::new(
                    -2.8 + index as f32 * (5.6 / 1189.),
                    -1.4 + if movers.contains(&index) {
                        tick as f32 * 0.01
                    } else {
                        0.
                    },
                    -3.,
                )) * Mat4::from_scale(Vec3::splat(0.02));
            }
            let full = capture(&gpu, &mut renderers[0], &scene)?;
            exact(
                &full,
                capture(&gpu, &mut renderers[1], &scene)?,
                "crosschunk uncompacted sun",
            );
            exact(
                &full,
                capture(&gpu, &mut renderers[2], &scene)?,
                "crosschunk compacted sun",
            );
            if tick >= 2 {
                assert!(renderers[1].stats.sun_static_cache_reused);
                assert!(renderers[2].stats.sun_static_cache_reused);
                assert_eq!(renderers[2].stats.sun_dynamic_casters, 8);
                assert_eq!(renderers[1].stats.shadow_draws, 8);
                assert_eq!(renderers[2].stats.shadow_draws, 2);
                assert_eq!(renderers[1].stats.shadow_triangles, 97);
                assert_eq!(renderers[2].stats.shadow_triangles, 97);
                assert_eq!(renderers[2].shadows.sun_cache.range_draws_saved.get(), 6);
                assert_eq!(renderers[2].frame_stats().sun_range_plan_builds, 0);
                assert_eq!(renderers[2].frame_stats().sun_range_plan_reuses, 1);
                assert_eq!(
                    renderers[2].shadows.sun_cache.range_write_bytes.get(),
                    8 * 96
                );
            }
        }
        println!(
            "sun_ranges_proof fragmented13dynamic CW_CCW immutable_restore14->3draws unchanged14triangles warm0 dirty96bytes ordinary_color_and_shadow_fallback_exact cross7chunks8movers 8->2draws unchanged97triangles retained_subset_warm0regroups mask_change_rebuilds"
        );
        Ok(())
    }
    #[test]
    fn retained_spatial_caster_index_matches_linear_scans_through_refits() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut renderers = std::array::from_fn::<_, 2, _>(|_| {
            SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm)
        });
        renderers[0].set_shadow_spatial_culling_enabled(false);
        for renderer in &mut renderers {
            renderer.set_occlusion_enabled(false);
            renderer.set_shadow_preparation_caching_enabled(false);
        }
        let mut scene = scene(
            (0..4096)
                .map(|index| DrawItem {
                    motion_id: index + 1,
                    model: Mat4::from_translation(Vec3::new(
                        (index % 32) as f32 * 3. - 46.5,
                        ((index / 32) % 16) as f32 * 3. - 22.5,
                        -5. - (index / 512) as f32 * 4.,
                    )) * Mat4::from_scale(Vec3::splat(0.5)),
                    mesh: MeshKind::Cube,
                    material: material(),
                })
                .collect(),
        );
        scene.view_projection =
            glam::camera::rh::proj::directx::orthographic(-10., 10., -10., 10., 0.1, 50.);
        scene.lights[0].position = [-20., 0., 2.];
        scene.lights[0].spot_angles = Some([10., 20.]);
        scene.lights.push(LocalLight {
            directional: false,
            position: [20., 0., -10.],
            direction: [0., 0., -1.],
            color: [1., 0.5, 0.2],
            intensity: 3.,
            range: 10.,
            spot_angles: None,
            shadows: Some(Default::default()),
        });
        for tick in 0..3 {
            if tick > 0 {
                for (index, item) in scene.items.iter_mut().enumerate() {
                    if index % 17 == 0 {
                        item.model *= Mat4::from_translation(Vec3::new(0.3, 0.2, -0.1));
                    }
                }
            }
            assert_eq!(
                capture(&gpu, &mut renderers[0], &scene)?.rgba,
                capture(&gpu, &mut renderers[1], &scene)?.rgba,
                "BVH caster pixels differ tick{tick}"
            );
            assert_eq!(
                renderers[0].stats.shadow_triangles,
                renderers[1].stats.shadow_triangles
            );
            assert!(renderers[1].stats.visible_items < 4096);
            assert!(renderers[1].stats.shadow_triangles > 0);
            assert_eq!(renderers[0].stats.local_shadow_caster_checks, 4096 * 7);
            assert!(
                renderers[1].stats.local_shadow_caster_checks < 4096 * 3,
                "spatial broadphase should substantially shrink checks"
            );
        }
        println!(
            "local_spatial_proof checks{}->{} exactspot_pointfaces_offscreen_and_refits",
            renderers[0].stats.local_shadow_caster_checks,
            renderers[1].stats.local_shadow_caster_checks
        );
        Ok(())
    }
    #[test]
    fn compact_expensive_static_local_layer_restores_immutable_depth() -> Result<()> {
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut renderers = std::array::from_fn::<_, 2, _>(|_| {
            SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm)
        });
        renderers[0].set_shadow_preparation_caching_enabled(false);
        renderers[0].set_opaque_shadow_specialization_enabled(false);
        for renderer in &mut renderers {
            renderer.set_occlusion_enabled(false);
            renderer.upload_mesh(
                &gpu,
                "expensive",
                &[
                    [-3., -3., 0., 0., 0., 1., 0., 1.],
                    [3., -3., 0., 0., 0., 1., 1., 1.],
                    [0., 3., 0., 0., 0., 1., 0.5, 0.],
                ],
                &(0..40_000).flat_map(|_| [0, 1, 2]).collect::<Vec<_>>(),
            )?;
        }
        let mut scene = scene(vec![
            DrawItem {
                motion_id: 1,
                model: Mat4::from_translation(Vec3::new(0., 0., -5.)),
                mesh: MeshKind::Imported("expensive".into()),
                material: material(),
            },
            DrawItem {
                motion_id: 2,
                model: Mat4::from_translation(Vec3::new(0., 0., -3.)),
                mesh: MeshKind::Cube,
                material: material(),
            },
            DrawItem {
                motion_id: 3,
                model: Mat4::from_translation(Vec3::new(40., 0., -3.)),
                mesh: MeshKind::Cube,
                material: material(),
            },
        ]);
        for tick in 0..5 {
            scene.items[1].model = Mat4::from_translation(Vec3::new(
                if tick == 4 { 20. } else { tick as f32 * 0.2 },
                0.,
                -3.,
            ));
            assert_eq!(
                capture(&gpu, &mut renderers[0], &scene)?.rgba,
                capture(&gpu, &mut renderers[1], &scene)?.rgba,
                "static local depth changed on mover tick{tick}"
            );
            if tick == 2 {
                assert_eq!(renderers[1].shadows.spots.static_depth_copies.get(), 1);
                assert_eq!(
                    renderers[1].shadows.spots.static_triangles_skipped.get(),
                    40_000
                );
                assert_eq!(renderers[1].stats.shadow_triangles, 13);
                assert_eq!(renderers[0].stats.shadow_triangles, 40_012);
            }
        }
        let layer_bytes = sun_cache::depth_bytes(spot_shadows::RESOLUTION).unwrap();
        // Departure changes caster membership and conservatively bypasses this
        // static subset. Re-entry warms it again once membership stabilizes.
        assert_eq!(static_bytes(&renderers[1].shadows.spots), 0);
        for x in [0.55, 0.65] {
            scene.items[1].model = Mat4::from_translation(Vec3::new(x, 0., -3.));
            assert_eq!(
                capture(&gpu, &mut renderers[0], &scene)?.rgba,
                capture(&gpu, &mut renderers[1], &scene)?.rgba
            );
        }
        assert_eq!(static_bytes(&renderers[1].shadows.spots), layer_bytes);
        // Admission failure releases an existing source and falls back to the
        // exact full pass without reallocating the required working map.
        renderers[1].shadows.spots.static_depth_budget = 0;
        scene.items[1].model = Mat4::from_translation(Vec3::new(0.75, 0., -3.));
        assert_eq!(
            capture(&gpu, &mut renderers[0], &scene)?.rgba,
            capture(&gpu, &mut renderers[1], &scene)?.rgba
        );
        assert_eq!(static_bytes(&renderers[1].shadows.spots), 0);
        assert_eq!(renderers[1].shadows.spots.static_depth_copies.get(), 0);
        assert_eq!(renderers[1].stats.shadow_triangles, 40_012);
        renderers[1].shadows.spots.static_depth_budget = LOCAL_DEPTH_BUDGET_BYTES;
        for x in [0.8, 0.9] {
            scene.items[1].model = Mat4::from_translation(Vec3::new(x, 0., -3.));
            assert_eq!(
                capture(&gpu, &mut renderers[0], &scene)?.rgba,
                capture(&gpu, &mut renderers[1], &scene)?.rgba
            );
        }
        assert_eq!(static_bytes(&renderers[1].shadows.spots), layer_bytes);
        assert_eq!(
            renderers[1].shadows.spots.static_triangles_skipped.get(),
            40_000
        );
        // Idle grace retains the source for 59 unused updates, then drops its
        // view/binding/pipeline/keys; cached working pixels remain exact.
        for _ in 0..59 {
            renderers[1].shadows.spots.age_unused_static_depth();
        }
        assert_eq!(static_bytes(&renderers[1].shadows.spots), layer_bytes);
        renderers[1].shadows.spots.age_unused_static_depth();
        assert_eq!(static_bytes(&renderers[1].shadows.spots), 0);
        assert_eq!(
            capture(&gpu, &mut renderers[0], &scene)?.rgba,
            capture(&gpu, &mut renderers[1], &scene)?.rgba
        );
        scene.items[1].model = Mat4::from_translation(Vec3::new(1., 0., -3.));
        assert_eq!(
            capture(&gpu, &mut renderers[0], &scene)?.rgba,
            capture(&gpu, &mut renderers[1], &scene)?.rgba
        );
        assert_eq!(static_bytes(&renderers[1].shadows.spots), layer_bytes);
        renderers[1].set_shadow_preparation_caching_enabled(false);
        assert_eq!(static_bytes(&renderers[1].shadows.spots), 0);
        renderers[1].set_shadow_preparation_caching_enabled(true);
        // The same expensive one-group population also activates the revised
        // sun heuristic after batching has collapsed its command count.
        scene.items.pop();
        scene.lights.clear();
        scene.lighting.shadows = true;
        scene.lighting.shadow_resolution = 256;
        scene.lighting.sun_direction = [0., 0., 1.];
        for tick in 0..3 {
            scene.items[1].model = Mat4::from_translation(Vec3::new(tick as f32 * 0.1, 0., -3.));
            assert_eq!(
                capture(&gpu, &mut renderers[0], &scene)?.rgba,
                capture(&gpu, &mut renderers[1], &scene)?.rgba,
                "expensive compact sun cache tick{tick}"
            );
            if tick == 2 {
                assert!(renderers[1].stats.sun_static_cache_reused);
                assert_eq!(renderers[1].stats.sun_static_casters, 1);
                assert_eq!(renderers[1].stats.shadow_triangles, 13);
            }
        }
        assert_eq!(
            renderers[1].shadows.sun_cache.bytes(),
            sun_cache::depth_bytes(256).unwrap()
        );
        // A newly added overlapping expensive mover changes work profitability
        // without changing the fitted projection. The rejected plan must free
        // its previous static source immediately and preserve reference pixels.
        let mut expensive_mover = scene.items[0].clone();
        expensive_mover.motion_id = 4;
        scene.items.push(expensive_mover);
        scene.items[1].model = Mat4::from_translation(Vec3::new(0.3, 0., -3.));
        assert_eq!(
            capture(&gpu, &mut renderers[0], &scene)?.rgba,
            capture(&gpu, &mut renderers[1], &scene)?.rgba
        );
        assert_eq!(renderers[1].shadows.sun_cache.bytes(), 0);
        assert!(!renderers[1].stats.sun_static_cache_reused);
        scene.items.pop();
        // Same-ID source publication invalidates the static layer and bindings.
        for renderer in &mut renderers {
            renderer.upload_mesh(
                &gpu,
                "expensive",
                &[
                    [-2., -1., 0., 0., 0., 1., 0., 1.],
                    [2., -1., 0., 0., 0., 1., 1., 1.],
                    [0., 1., 0., 0., 0., 1., 0.5, 0.],
                ],
                &[0, 1, 2],
            )?;
        }
        assert_eq!(
            capture(&gpu, &mut renderers[0], &scene)?.rgba,
            capture(&gpu, &mut renderers[1], &scene)?.rgba
        );
        println!(
            "local_static_sun_proof 40012->13triangles one_static_group immutable_restore mover_departure sameID_reupload pixels_equal depth_budget_rejects_releases idle59_retains60_releases bypass_releases sun_unprofitable_releases"
        );
        Ok(())
    }
    #[test]
    fn compacted_ranges_rebind_a_replaced_view() -> Result<()> {
        // Compacted entries outlive the frames that drew their casters, so a
        // texture replaced while no caster used it must not stay bound.
        let gpu = pollster::block_on(Gpu::request(
            &crate::instance(crate::Backend::native()),
            None,
            false,
        ))?;
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        renderer.set_occlusion_enabled(false);
        renderer.set_native_shadow_lists_enabled(false);
        renderer.upload_image(&gpu, "mask", 1, 1, &[255; 4])?;
        let scene = scene(
            (0..8)
                .map(|index| DrawItem {
                    motion_id: index + 1,
                    model: Mat4::from_translation(Vec3::new(index as f32 * 0.1, 0., -5.)),
                    mesh: MeshKind::Cube,
                    material: Material {
                        texture: TextureKind::Imported("mask".into()),
                        ..material()
                    },
                })
                .collect(),
        );
        capture(&gpu, &mut renderer, &scene)?;
        let batches = [instancing::Batch {
            indices: (0..8).collect(),
            slot: Some(0),
            first_instance: 0,
        }];
        let accepted: Vec<bool> = (0..8).map(|index| index % 2 == 0).collect();
        let mut cache = compaction::Cache::default();
        let mut bind = |renderer: &SceneRenderer| {
            let plan = cache.prepare(renderer, &gpu.device, &gpu.queue, &batches, &accepted);
            assert_eq!(plan.saved, 3);
            plan.bindings[&0].clone()
        };
        let first = bind(&renderer);
        assert!(
            bind(&renderer) == first,
            "an unchanged view keeps its binding"
        );
        renderer.upload_image(&gpu, "mask", 1, 1, &[255, 0, 0, 255])?;
        assert!(bind(&renderer) != first, "a replaced view is rebound");
        Ok(())
    }
}
