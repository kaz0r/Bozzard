use super::*;

/// Why a color batch plan could not be retained. This is CPU preparation, not GPU timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchPlanRebuildReason {
    Cold,
    Membership,
    Visibility,
    Bounds,
    Metadata,
    IncrementalDisabled,
    UnsupportedProjection,
    NonAffineModel,
    UnboundedGeometry,
    OrderingCapacity,
    OrderingConflict,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct FrameStats {
    /// Monotonic identity for matching asynchronous GPU results with their rendered frame.
    pub frame_id: u64,
    pub viewport_size: [u32; 2],
    pub particles: usize,
    /// Simulation/sort/gather/bucket compute dispatches; zero for an unchanged paused frame.
    pub particle_compute_dispatches: u32,
    /// Particle descriptors uploaded this frame, excluding small camera/sort uniforms.
    pub particle_descriptor_bytes: usize,
    pub particle_triangles: u64,
    pub scene_items: usize,
    /// Distinct world draw items with a frustum-visible surface. Excludes screen HUD;
    /// GPU occlusion can reject additional objects after this CPU-side count.
    pub visible_items: usize,
    pub surfaces: usize,
    pub visible_surfaces: usize,
    pub culled_surfaces: usize,
    /// Submitted upper bound; subtract completed occlusion savings only when
    /// the result's frame_id matches the frame being inspected. Cached occlusion
    /// already omits hidden commands, so do not subtract its savings again.
    pub color_triangles: u64,
    /// Color-pass mesh draw commands, excluding particles, sky, HUD and post-processing.
    /// An indirect command can skip its instances; completed GPU savings are in
    /// `occlusion_result`, with that result's own frame identity.
    pub color_draws: usize,
    pub occlusion_depth_draws: usize,
    pub occlusion_depth_triangles: u64,
    pub occlusion_candidates: usize,
    /// Identical depth inputs and bounds reused completed visibility on the CPU.
    pub occlusion_cache_hit: bool,
    pub occlusion_bytes: u64,
    pub occlusion_result: Option<OcclusionResult>,
    pub instanced_draws: usize,
    /// Visible surfaces represented by draws with more than one instance.
    pub instanced_surfaces: usize,
    pub batching: BatchingStats,
    pub batch_plan_reused: bool,
    pub batch_plan_rebuilds: usize,
    /// Ordering renewed after an envelope escape, without regrouping surfaces.
    pub batch_plan_recertifications: usize,
    pub batch_plan_rebuild_reason: Option<BatchPlanRebuildReason>,
    /// CPU plan validation/construction, excluding instance-buffer preparation.
    pub batch_plan_ms: f64,
    /// CPU singleton classification/histogram collection; included in batch_plan_ms.
    pub batch_diagnostics_ms: f64,
    pub batch_diagnostics_reused: bool,
    /// CPU packed color-buffer comparison, binding creation/rebinding and writes.
    /// Excludes shader pipeline compilation and GPU execution.
    pub instance_prepare_ms: f64,
    /// CPU independent shadow membership validation/grouping, excluding buffer preparation.
    pub shadow_batch_plan_ms: f64,
    pub shadow_batch_plan_reused: bool,
    pub shadow_batch_plan_rebuilds: usize,
    pub shadow_instance_prepare_ms: f64,
    /// Logical packed color-buffer bytes, including at most eight inactive buffers.
    pub instance_buffer_bytes: usize,
    pub shadow_instance_buffer_bytes: usize,
    /// CPU camera-frustum checks, excluding occlusion and GPU execution.
    pub visibility_ms: f64,
    /// CPU occlusion preparation/encoding; not the depth/compute GPU duration.
    pub occlusion_prepare_ms: f64,
    /// World/projected bounds updated while retaining an existing ordering plan.
    pub batch_bounds_updates: usize,
    pub batch_order_checks: usize,
    /// Additional packed instance uniform bytes uploaded this frame.
    pub instance_uniform_bytes: usize,
    /// New instance-buffer allocations; texture rebinding reuses the allocation.
    pub instance_buffer_allocations: usize,
    /// Additional packed shadow-caster bytes, independent of camera batches.
    pub shadow_instance_uniform_bytes: usize,
    pub shadow_instance_buffer_allocations: usize,
    /// Shared camera, lighting, fog and graph-clock bytes uploaded once per frame.
    pub frame_uniform_bytes: usize,
    pub shadow_draws: usize,
    /// Depth passes encoded this frame, including clears of empty maps.
    pub shadow_maps_rendered: usize,
    /// Opaque lit surfaces inspected while validating local shadow map contents.
    pub local_shadow_caster_checks: usize,
    /// Local maps reused after frame edits without another per-map caster scan.
    pub local_shadow_maps_reused_without_scan: usize,
    /// Exact opaque inputs and fitted sun uniforms reused the whole sun depth map.
    pub sun_shadow_fit_reused: bool,
    /// Static depth reused while dynamic casters were rendered over a depth copy.
    pub sun_static_cache_reused: bool,
    pub sun_static_casters: usize,
    pub sun_dynamic_casters: usize,
    /// Full-map depth copies, included in shadow draw/triangle and GPU pass counts.
    pub sun_depth_copies: usize,
    /// Per-caster light-space bounds reused/recomputed while fitting the sun map.
    pub sun_bounds_reused: usize,
    pub sun_bounds_recomputed: usize,
    /// A non-finite corner required the original whole-scene reduction.
    pub sun_bounds_fallback: bool,
    /// Retained light-space bounds allocation, excluding the inline cache header.
    pub sun_bounds_cache_bytes: usize,
    /// CPU time spent computing/validating fitted sun bounds in this frame.
    pub sun_fit_ms: f64,
    /// CPU snapshot comparison/classification/construction/retirement time.
    pub shadow_state_ms: f64,
    pub shadow_metadata_built_casters: usize,
    pub shadow_metadata_updated_casters: usize,
    pub shadow_metadata_reused_casters: usize,
    /// Mesh/texture clone calls; primitive keys may not allocate.
    pub shadow_metadata_key_clones: usize,
    pub graph_compilations: usize,
    /// Lazily compiled opaque instanced graph host variants this frame.
    pub graph_instanced_compilations: usize,
    /// All active graphs plus at most eight recently absent graphs.
    pub resident_graphs: usize,
    pub shadow_triangles: u64,
    /// True when every shadow map was reused without another depth pass.
    pub shadow_cache_hit: bool,
    pub object_uniform_writes: usize,
    /// Object uniforms recomputed on the CPU, before byte comparison/upload.
    pub object_uniform_builds: usize,
    /// Conservative object/light pairs retained for visible lit surfaces.
    pub local_light_candidates: usize,
    /// Visible lit surfaces multiplied by the scene's local-light count.
    pub local_light_slots: usize,
    pub light_mask_builds: usize,
    pub auxiliary_targets: usize,
    /// Logical size of allocated auxiliary textures; excludes other effect/history targets.
    pub geometry_allocated_bytes: u64,
    /// Logical bytes retained by the opaque pass's three RGBA16F auxiliary targets.
    /// Not measured memory traffic.
    pub geometry_store_bytes: u64,
    /// Color-pass mesh pipeline binds; excludes sky, shadow and display passes.
    pub pipeline_binds: usize,
    /// CPU work only, including command submission. Not GPU execution or FPS.
    pub cpu_ms: f64,
    /// Total CPU preparation before encoding. See preparation_stages for disjoint children.
    pub prepare_ms: f64,
    /// Render dimensions, camera and initial material/override/fog/display validation.
    pub scene_validation_ms: f64,
    /// Frame targets, environment/display/GI/lights/text/skinning/sprites and encoder setup.
    pub renderer_setup_ms: f64,
    /// Active graph discovery, cache retirement and missing basic/PBR pipeline compilation.
    pub graph_prepare_ms: f64,
    pub graph_source_checks: usize,
    /// Per-surface object binding creation/rebinding and uploaded-mesh validation.
    pub object_binding_prepare_ms: f64,
    pub object_binding_allocations: usize,
    pub mesh_validation_checks: usize,
    /// Per-frame static surface certificates reused/built, or rows resolved without retention.
    pub resource_metadata_hits: usize,
    pub resource_metadata_builds: usize,
    pub resource_metadata_bypasses: usize,
    /// Actual mesh_for calls for local bounds during CPU preparation (not encode lookups).
    pub resource_bounds_lookups: usize,
    /// Logical populated inline metadata bytes, already included in surface_preparation_bytes.
    /// Not additional allocated memory or vector capacity; zero after a failed frame.
    pub resource_metadata_bytes: usize,
    /// Local mesh-bounds lookup and temporary bounds-vector allocation/fill.
    pub bounds_collect_ms: f64,
    /// Revision comparison/refresh for local-light selection, excluding per-object masks.
    pub light_selection_ms: f64,
    /// Visible item/surface counting and temporary item-mask allocation/fill.
    pub visibility_bookkeeping_ms: f64,
    /// Shared frame constants, byte comparison and optional queue write.
    pub frame_uniform_prepare_ms: f64,
    /// One coarse timer around the object source/mask/validation/build loop.
    /// Includes light-cache checks; never reads the clock per object.
    pub object_uniform_prepare_ms: f64,
    pub object_uniform_source_checks: usize,
    /// Conditional combined view/model finite checks, including changed-camera frames.
    pub object_matrix_checks: usize,
    pub light_mask_checks: usize,
    pub light_mask_cache_hits: usize,
    /// Complete color batch preparation, including nested plan/buffer timers and cold pipelines.
    pub batch_prepare_ms: f64,
    /// Instanced graph discovery/cache checks and missing variant compilation.
    pub graph_instanced_prepare_ms: f64,
    pub graph_instance_checks: usize,
    /// Complete shadow preparation before encoding. Includes nested fitting/group/buffer timers,
    /// but excludes the successful-frame metadata refresh/retirement after submission.
    pub shadow_prepare_ms: f64,
    /// Temporary individual-upload mask allocation and required-object marking.
    pub individual_prepare_ms: f64,
    /// Scan of that mask and conditional dirty individual-object queue writes.
    pub individual_upload_ms: f64,
    pub individual_uniform_candidates: usize,
    /// Optional particle-transparent index-vector allocation/filtering; not particle preparation.
    pub transparent_prepare_ms: f64,
    pub particle_prepare_ms: f64,
    /// Total prepare_ms less the disjoint measured stages (clock/bookkeeping gaps).
    pub prepare_unaccounted_ms: f64,
    /// Logical temporary vector capacities in bytes, not allocator traffic or peak memory.
    /// These include allocation/fill work already timed in the corresponding stage.
    pub scratch_bounds_bytes: usize,
    pub scratch_visibility_bytes: usize,
    pub scratch_visible_items_bytes: usize,
    pub scratch_individual_bytes: usize,
    pub scratch_transparent_bytes: usize,
    /// CPU comparison, retained-surface refresh, expansion and ordering, before GPU encoding.
    pub surface_prepare_ms: f64,
    pub surface_source_checks: usize,
    pub surface_items_rebuilt: usize,
    pub surface_items_reused: usize,
    pub surface_records_built: usize,
    pub surface_records_reused: usize,
    pub surface_model_updates: usize,
    pub surface_depth_updates: usize,
    /// Existing ordering retained without sorting; source-order ties remain stable.
    pub surface_order_reused: bool,
    /// Retained vector capacities, excluding shared Arc storage, owned key strings and GPU data.
    pub surface_preparation_bytes: usize,
    pub encode_ms: f64,
    pub submit_ms: f64,
}
impl FrameStats {
    /// Disjoint CPU stages included in prepare_ms on successful frames, in execution order.
    /// Failed frames can have partial stage/counter results without a completed total. Do not add
    /// batch_plan_ms/instance_prepare_ms or shadow fit/state/group/buffer children
    /// again. Cold graph/pipeline creation is included, GPU execution is not.
    pub fn preparation_stages(&self) -> [(&'static str, f64); 20] {
        [
            ("scene_validation", self.scene_validation_ms),
            ("renderer_setup", self.renderer_setup_ms),
            ("surface_prepare", self.surface_prepare_ms),
            ("graph_prepare", self.graph_prepare_ms),
            ("object_binding_prepare", self.object_binding_prepare_ms),
            ("bounds_collect", self.bounds_collect_ms),
            ("visibility", self.visibility_ms),
            ("light_selection", self.light_selection_ms),
            ("visibility_bookkeeping", self.visibility_bookkeeping_ms),
            ("frame_uniform_prepare", self.frame_uniform_prepare_ms),
            ("object_uniform_prepare", self.object_uniform_prepare_ms),
            ("batch_prepare", self.batch_prepare_ms),
            ("graph_instanced_prepare", self.graph_instanced_prepare_ms),
            ("occlusion_prepare", self.occlusion_prepare_ms),
            ("shadow_prepare", self.shadow_prepare_ms),
            ("individual_prepare", self.individual_prepare_ms),
            ("individual_upload", self.individual_upload_ms),
            ("transparent_prepare", self.transparent_prepare_ms),
            ("particle_prepare", self.particle_prepare_ms),
            ("unaccounted", self.prepare_unaccounted_ms),
        ]
    }

    /// Sum of five observed temporary vector capacities, not simultaneous peak
    /// memory, retained cache storage, allocator instrumentation or upload bytes.
    pub fn preparation_scratch_bytes(&self) -> usize {
        self.scratch_bounds_bytes
            + self.scratch_visibility_bytes
            + self.scratch_visible_items_bytes
            + self.scratch_individual_bytes
            + self.scratch_transparent_bytes
    }
}

/// Reject only when every transformed AABB corner is outside the same homogeneous
/// clip plane. No perspective divide: handles near-plane crossings and negative w.
pub(super) fn visible(bounds: [Vec3; 2], mvp: Mat4) -> bool {
    // One accepted corner already rules out every common rejection plane.
    // Keep the remainder out of this small fast path so its seven-corner
    // scratch storage does not burden objects accepted by the first corner.
    let first = mvp * bounds[0].extend(1.);
    let tolerance = first.abs().max_element().max(1.) * 1e-6;
    if let Some(plane) = (0..6).find(|&p| clip_distance(first, p) < -tolerance) {
        visible_remaining(bounds, mvp, first, tolerance, plane)
    } else {
        true
    }
}

#[inline]
fn clip_distance(q: glam::Vec4, plane: usize) -> f32 {
    match plane {
        0 => q.w + q.x,
        1 => q.w - q.x,
        2 => q.w + q.y,
        3 => q.w - q.y,
        4 => q.z,
        _ => q.w - q.z,
    }
}

#[inline(never)]
fn visible_remaining(
    bounds: [Vec3; 2],
    mvp: Mat4,
    first: glam::Vec4,
    tolerance: f32,
    first_plane: usize,
) -> bool {
    let corners: [glam::Vec4; 7] = std::array::from_fn(|index| {
        let i = index + 1;
        mvp * Vec3::new(
            bounds[i & 1].x,
            bounds[(i >> 1) & 1].y,
            bounds[(i >> 2) & 1].z,
        )
        .extend(1.)
    });
    !(first_plane..6).any(|plane| {
        (plane == first_plane || clip_distance(first, plane) < -tolerance)
            && corners.iter().all(|q| {
                let tolerance = q.abs().max_element().max(1.) * 1e-6;
                clip_distance(*q, plane) < -tolerance
            })
    })
}

fn visible_reference(bounds: [Vec3; 2], mvp: Mat4) -> bool {
    let corners: [glam::Vec4; 8] = std::array::from_fn(|i| {
        mvp * Vec3::new(
            bounds[i & 1].x,
            bounds[(i >> 1) & 1].y,
            bounds[(i >> 2) & 1].z,
        )
        .extend(1.)
    });
    !(0..6).any(|plane| {
        corners.iter().all(|q| {
            let distance = match plane {
                0 => q.w + q.x,
                1 => q.w - q.x,
                2 => q.w + q.y,
                3 => q.w - q.y,
                4 => q.z,
                _ => q.w - q.z,
            };
            let tolerance = q.abs().max_element().max(1.) * 1e-6;
            distance < -tolerance
        })
    })
}
impl SceneRenderer {
    pub fn frame_stats(&self) -> FrameStats {
        self.stats
    }
    /// Diagnostic switches permit output/performance comparison with the reference path.
    pub fn set_culling_enabled(&mut self, enabled: bool) {
        self.culling = enabled;
    }
    /// Compare early corner acceptance with the original eight-corner predicate.
    pub fn set_frustum_early_acceptance_enabled(&mut self, enabled: bool) {
        self.early_frustum_acceptance = enabled;
    }
    /// Compare fitted/static sun and local-caster reuse with full shadow preparation.
    pub fn set_shadow_preparation_caching_enabled(&mut self, enabled: bool) {
        self.shadow_preparation_cache = enabled;
        if !enabled {
            self.shadows.sun_cache.clear();
        }
    }
    /// Compare retained light-space bounds with the original sun-fitting loop.
    pub fn set_sun_fit_caching_enabled(&mut self, enabled: bool) {
        self.sun_fit_caching = enabled;
        if !enabled {
            self.shadows.sun_fit.clear();
        }
    }
    /// Compare retained snapshot storage/classification with full snapshot rebuilding.
    pub fn set_shadow_metadata_reuse_enabled(&mut self, enabled: bool) {
        self.shadow_metadata_reuse = enabled;
    }
    pub(super) fn frustum_visible(&self, bounds: [Vec3; 2], mvp: Mat4) -> bool {
        if self.early_frustum_acceptance {
            visible(bounds, mvp)
        } else {
            visible_reference(bounds, mvp)
        }
    }
    /// Disabling releases retained preparation; stats continue to describe the last draw.
    pub fn set_state_caching_enabled(&mut self, enabled: bool) {
        self.state_caching = enabled;
        if !enabled {
            self.instancing.invalidate_preparation();
            self.surface_preparation.clear();
        }
    }
    /// Compare conservative surface light masks with the full light loop.
    pub fn set_local_light_culling_enabled(&mut self, enabled: bool) {
        self.light_selection.set_enabled(enabled);
    }
    /// Compare dedicated shadow batches with the former color-batch fallback.
    pub fn set_shadow_batching_enabled(&mut self, enabled: bool) {
        if self.instancing.shadow_batches_enabled != enabled {
            self.instancing.shadow_batches_enabled = enabled;
            self.instancing.shadow_plan = None;
            if !enabled {
                self.instancing.shadow_bindings.clear();
            }
            self.shadow_frame = None;
            self.shadows.spots.invalidate();
            self.shadows.points.invalidate();
        }
    }
    pub(super) fn visibility(
        &self,
        scene: &RenderScene,
        draws: &[PreparedDraw],
        bounds: &[[Vec3; 2]],
    ) -> Vec<bool> {
        draws
            .iter()
            .zip(bounds)
            .map(|(d, &bounds)| {
                !self.culling
                    || self.frustum_visible(bounds, scene.view_projection * d.object.model)
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "release-mode CPU frustum profile; run without competing CPU/GPU work"]
    fn frustum_predicate_benchmark() {
        use std::{hint::black_box, time::Instant};
        let bounds = [Vec3::splat(-0.4), Vec3::splat(0.4)];
        let ortho = glam::camera::rh::proj::directx::orthographic(-10., 10., -10., 10., 0.1, 30.);
        let perspective = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 30.);
        for workload in ["inside", "outside", "near-crossing", "mixed"] {
            let matrices: Vec<_> = (0..1024)
                .map(|i| {
                    let x = (i % 32) as f32 * 0.5 - 7.75;
                    let y = (i / 32) as f32 * 0.5 - 7.75;
                    match workload {
                        "inside" => ortho * Mat4::from_translation(Vec3::new(x, y, -5.)),
                        "outside" => ortho * Mat4::from_translation(Vec3::new(x - 30., y, -5.)),
                        "near-crossing" => {
                            perspective
                                * Mat4::from_translation(Vec3::new(x * 0.01, y * 0.01, -0.2))
                        }
                        _ => {
                            perspective
                                * Mat4::from_translation(Vec3::new(x, y, -0.1 - (i % 13) as f32))
                        }
                    }
                })
                .collect();
            let predicates: [fn([Vec3; 2], Mat4) -> bool; 2] = [visible_reference, visible];
            let mut samples: [Vec<f64>; 2] = Default::default();
            let mut accepted = [0; 2];
            for tick in 0..70 {
                for mode in if tick % 2 == 0 { [0, 1] } else { [1, 0] } {
                    let predicate = black_box(predicates[mode]);
                    let start = Instant::now();
                    let mut count = 0;
                    for _ in 0..16 {
                        for &matrix in &matrices {
                            count += usize::from(predicate(black_box(bounds), black_box(matrix)));
                        }
                    }
                    black_box(count);
                    accepted[mode] = count;
                    if tick >= 10 {
                        samples[mode]
                            .push(start.elapsed().as_nanos() as f64 / (16 * matrices.len()) as f64);
                    }
                }
            }
            assert_eq!(accepted[0], accepted[1]);
            for (mode, sample) in samples.iter_mut().enumerate() {
                sample.sort_by(f64::total_cmp);
                println!(
                    "workload={workload} mode={} ns_per_predicate_median={:.3} p95={:.3} accepted={}/{}",
                    ["eight-corner-reference", "early-corner-acceptance"][mode],
                    sample[30],
                    sample[57],
                    accepted[mode],
                    16 * matrices.len()
                );
            }
        }
    }
    #[test]
    fn early_corner_acceptance_matches_reference_for_projective_and_uncertain_bounds() {
        let mut state = 0x38c9_73a2u32;
        let mut value = || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f64 / u32::MAX as f64 * 2. - 1.) as f32
        };
        let ortho = glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.);
        let perspective = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 10.);
        for i in 0..50_000 {
            let center = Vec3::new(value(), value(), value()) * 2.;
            let extent = Vec3::new(value(), value(), value()).abs();
            let bounds = [center - extent, center + extent];
            let mut matrix = if i % 3 == 0 {
                Mat4::from_cols_array(&std::array::from_fn(|_| value() * 3.))
            } else {
                let transform = Mat4::from_translation(Vec3::new(value(), value(), value()) * 15.)
                    * Mat4::from_rotation_y(value() * 4.)
                    * Mat4::from_scale(Vec3::new(value(), value(), value()) * 3.);
                if i % 3 == 1 {
                    ortho * transform
                } else {
                    perspective * transform
                }
            };
            if i % 127 == 0 {
                matrix *= 1e6;
            }
            if i % 251 == 0 {
                matrix *= 1e-8;
            }
            assert_eq!(
                visible(bounds, matrix),
                visible_reference(bounds, matrix),
                "case {i}"
            );
        }
        let bounds = [Vec3::splat(-0.5), Vec3::splat(0.5)];
        for special in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.] {
            for column in 0..16 {
                let mut values = Mat4::IDENTITY.to_cols_array();
                values[column] = special;
                let matrix = Mat4::from_cols_array(&values);
                assert_eq!(visible(bounds, matrix), visible_reference(bounds, matrix));
            }
        }
        // Exactly tangent and either side of the relative clip tolerance.
        for plane in 0..6 {
            for offset in [-2e-6, -1e-6, 0., 1e-6, 2e-6] {
                let point = match plane {
                    0 => Vec3::new(-1. + offset, 0., 0.5),
                    1 => Vec3::new(1. + offset, 0., 0.5),
                    2 => Vec3::new(0., -1. + offset, 0.5),
                    3 => Vec3::new(0., 1. + offset, 0.5),
                    4 => Vec3::new(0., 0., offset),
                    _ => Vec3::new(0., 0., 1. + offset),
                };
                assert_eq!(
                    visible([point; 2], Mat4::IDENTITY),
                    visible_reference([point; 2], Mat4::IDENTITY)
                );
            }
        }
    }
    #[test]
    fn conservative_clip_planes_transform_and_camera_crossings() {
        let b = [Vec3::splat(-0.5), Vec3::splat(0.5)];
        let ortho = glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.);
        assert!(visible(
            b,
            ortho * Mat4::from_translation(Vec3::new(0., 0., -2.))
        ));
        for p in [
            Vec3::new(2., 0., -2.),
            Vec3::new(-2., 0., -2.),
            Vec3::new(0., 2., -2.),
            Vec3::new(0., -2., -2.),
            Vec3::new(0., 0., 1.),
            Vec3::new(0., 0., -11.),
        ] {
            assert!(!visible(b, ortho * Mat4::from_translation(p)), "{p:?}");
        }
        assert!(visible(
            b,
            ortho * Mat4::from_translation(Vec3::new(1.5, 0., -2.))
        ));
        assert!(visible(
            b,
            ortho
                * Mat4::from_translation(Vec3::new(1.2, 0., -2.))
                * Mat4::from_rotation_z(0.6)
                * Mat4::from_scale(Vec3::new(-2., 0.2, 1.))
        ));
        let perspective = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 10.);
        assert!(visible(
            b,
            perspective * Mat4::from_translation(Vec3::new(0., 0., -0.2))
        ));
        assert!(!visible(
            b,
            perspective * Mat4::from_translation(Vec3::new(0., 0., 2.))
        ));
        assert!(!visible(
            b,
            perspective * Mat4::from_translation(Vec3::new(20., 0., -2.))
        ));
    }
}
