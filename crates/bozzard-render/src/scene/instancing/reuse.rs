use super::*;
mod spatial;
pub(super) use spatial::Index as BoundsIndex;
use std::collections::HashSet;

type Bounds = [Vec3; 2];

fn expanded(bounds: Bounds, margin: Vec3) -> Bounds {
    [bounds[0] - margin, bounds[1] + margin]
}
fn overlaps(a: Bounds, b: Bounds) -> bool {
    !(a[1].cmplt(b[0]).any() || b[1].cmplt(a[0]).any())
}
fn contains(outer: Bounds, inner: Bounds) -> bool {
    inner[0].cmpge(outer[0]).all() && inner[1].cmple(outer[1]).all()
}
fn padding(camera: Mat4) -> Option<Vec3> {
    let inverse = camera.inverse();
    (camera.x_axis.w == 0.
        && camera.y_axis.w == 0.
        && camera.z_axis.w == 0.
        && camera.w_axis.w > 1e-6
        && inverse.is_finite())
    .then(|| {
        (inverse.x_axis.truncate().abs()
            + inverse.y_axis.truncate().abs()
            + inverse.z_axis.truncate().abs())
            * (2e-5 / inverse.w_axis.w.abs())
    })
    .filter(|margin| margin.is_finite())
}
pub(super) fn orthographic(camera: Mat4) -> bool {
    padding(camera).is_some()
}
fn affine(model: Mat4) -> bool {
    model.x_axis.w == 0. && model.y_axis.w == 0. && model.z_axis.w == 0. && model.w_axis.w == 1.
}

pub(super) struct Ordering {
    original: bool,
    screen_space: bool,
    world: Vec<Bounds>,
    envelopes: Vec<Bounds>,
    projected: Vec<Bounds>,
    current: Vec<bool>,
    pairs: Vec<(usize, usize)>,
    neighbors: Vec<Vec<usize>>,
    checked: Vec<bool>,
    ranks: Vec<usize>,
    seen: Vec<bool>,
    spatial: spatial::Index,
    pair_set: HashSet<(usize, usize)>,
    candidates: Vec<usize>,
    escaped: Vec<usize>,
}

impl Ordering {
    pub(super) fn new(
        inputs: &[Input],
        batches: &[Batch],
        camera: Mat4,
        projected: Vec<Bounds>,
    ) -> std::result::Result<Self, BatchPlanRebuildReason> {
        let mut ranks = vec![usize::MAX; inputs.len()];
        let order: Vec<_> = batches
            .iter()
            .flat_map(|batch| &batch.indices)
            .copied()
            .filter(|&i| !inputs[i].transparent)
            .collect();
        let original = order.windows(2).all(|pair| pair[0] < pair[1]);
        let mut result = Self {
            original,
            screen_space: false,
            world: vec![],
            envelopes: vec![],
            projected,
            current: vec![true; inputs.len()],
            pairs: vec![],
            neighbors: vec![],
            checked: vec![],
            ranks: vec![],
            seen: vec![],
            spatial: spatial::Index::default(),
            pair_set: HashSet::new(),
            candidates: Vec::new(),
            escaped: Vec::new(),
        };
        // An order that never moved opaque draws is safe for any camera or motion.
        if original {
            return Ok(result);
        }
        result.neighbors.resize_with(inputs.len(), Vec::new);
        let world_margin = padding(camera);
        result.screen_space = world_margin.is_none();
        let margin = world_margin.unwrap_or(Vec3::ZERO);
        result.world.resize(inputs.len(), [Vec3::ZERO; 2]);
        result.envelopes.resize(inputs.len(), [Vec3::ZERO; 2]);
        for (rank, &index) in order.iter().enumerate() {
            let input = &inputs[index];
            if !affine(input.model) {
                return Err(BatchPlanRebuildReason::NonAffineModel);
            }
            let bounds = if result.screen_space {
                result.projected[index]
            } else {
                projected_bounds(input.bounds, input.model)
            };
            if !bounds[0].is_finite() || !bounds[1].is_finite() {
                return Err(BatchPlanRebuildReason::UnboundedGeometry);
            }
            result.world[index] = bounds;
            // Room for modest rotations/motion. Envelopes are only certificates;
            // they never change culling, occlusion bounds or actual rendering.
            let slack = (bounds[1] - bounds[0]) * 0.125 + Vec3::splat(0.0001);
            result.envelopes[index] = expanded(bounds, margin + slack);
            ranks[index] = rank;
        }
        // Sweep source order, activating only preceding objects. Spatial/rank
        // aggregates now describe the same eligible leaves, avoiding false
        // candidates from unrelated earlier-index and later-rank objects.
        let mut discovery = spatial::InversionIndex::new(&result.envelopes, &ranks);
        let hierarchy_nodes = discovery.len();
        let mut work = 0usize;
        let mut query_visits = 0usize;
        let mut activation_updates = 0usize;
        let work_budget = inputs.len().saturating_mul(64).min(MAX_PLAN_CANDIDATES);
        let diagnostic = |queries: usize, updates: usize, pairs: usize, reason: &str| {
            if std::env::var_os("BOZZARD_BATCH_PLAN_DIAGNOSTICS").is_some() {
                eprintln!(
                    "batch_ordering population={} hierarchy_nodes={} node_visits={queries} activation_updates={updates} total_work={} work_budget={} pairs={} pair_budget={} result={reason}",
                    inputs.len(),
                    hierarchy_nodes,
                    queries + updates,
                    work_budget,
                    pairs,
                    (inputs.len() * 8).min(32_768),
                );
            }
        };
        // Constructor partitioning is bounded O(N log N) work, as before.
        // Every query visit and activation update consumes this single cap.
        for (index, &rank) in ranks.iter().enumerate() {
            if rank == usize::MAX {
                continue;
            }
            let preceding_work = work;
            let complete = discovery.query(
                index,
                rank,
                result.envelopes[index],
                &mut result.candidates,
                &mut work,
                work_budget,
            );
            query_visits += work - preceding_work;
            if !complete {
                diagnostic(
                    query_visits,
                    activation_updates,
                    result.pairs.len(),
                    "candidate_work_capacity",
                );
                return Err(BatchPlanRebuildReason::OrderingCapacity);
            }
            for &before in &result.candidates {
                // The hierarchy returns only complete envelope overlaps with
                // an earlier original index and a later emitted rank.
                if result.pairs.len() >= (inputs.len() * 8).min(32_768) {
                    diagnostic(
                        query_visits,
                        activation_updates,
                        result.pairs.len(),
                        "retained_pair_capacity",
                    );
                    return Err(BatchPlanRebuildReason::OrderingCapacity);
                }
                let pair = result.pairs.len();
                result.pairs.push((before, index));
                result.pair_set.insert((before, index));
                result.neighbors[before].push(pair);
                result.neighbors[index].push(pair);
            }
            let preceding_work = work;
            let complete =
                discovery.activate(index, rank, result.envelopes[index], &mut work, work_budget);
            activation_updates += work - preceding_work;
            if !complete {
                diagnostic(
                    query_visits,
                    activation_updates,
                    result.pairs.len(),
                    "activation_work_capacity",
                );
                return Err(BatchPlanRebuildReason::OrderingCapacity);
            }
        }
        diagnostic(
            query_visits,
            activation_updates,
            result.pairs.len(),
            "certified",
        );
        // Keep only the existing bounded mutable grid for envelope renewal;
        // initial discovery needs at most 2N-1 temporary hierarchy nodes.
        drop(discovery);
        result.spatial = spatial::Index::new(&result.envelopes, &ranks);
        result.checked.resize(result.pairs.len(), false);
        result.ranks = ranks;
        result.seen.resize(inputs.len(), false);
        Ok(result)
    }

    fn renew(
        &mut self,
        escaped: &[usize],
        margin: Vec3,
    ) -> std::result::Result<(), BatchPlanRebuildReason> {
        // Recenter all escaped envelopes first: two simultaneously moving
        // surfaces must discover each other at their *new* locations.
        for &index in escaped {
            let bounds = self.world[index];
            let slack = (bounds[1] - bounds[0]) * 0.125 + Vec3::splat(0.0001);
            let envelope = expanded(bounds, margin + slack);
            if !envelope[0].is_finite() || !envelope[1].is_finite() {
                return Err(BatchPlanRebuildReason::UnboundedGeometry);
            }
            self.envelopes[index] = envelope;
            self.spatial.update(index, envelope);
        }
        // Remove dependencies whose complete movement envelopes are disjoint.
        // A later envelope update queries the new spatial neighborhood again.
        self.pairs
            .retain(|&(a, b)| overlaps(self.envelopes[a], self.envelopes[b]));
        self.pair_set.clear();
        for neighbors in &mut self.neighbors {
            neighbors.clear();
        }
        for (pair, &(a, b)) in self.pairs.iter().enumerate() {
            self.pair_set.insert((a, b));
            self.neighbors[a].push(pair);
            self.neighbors[b].push(pair);
        }
        self.checked.clear();
        self.checked.resize(self.pairs.len(), false);
        let mut examined = 0usize;
        let candidate_budget = self.ranks.len().saturating_mul(64).min(MAX_PLAN_CANDIDATES);
        for &index in escaped {
            self.spatial
                .query(self.envelopes[index], &mut self.candidates);
            for &other in &self.candidates {
                examined += 1;
                if examined > candidate_budget {
                    return Err(BatchPlanRebuildReason::OrderingCapacity);
                }
                if other == index || self.ranks[other] == usize::MAX {
                    continue;
                }
                let (before, after) = (index.min(other), index.max(other));
                if self.ranks[before] < self.ranks[after]
                    || self.pair_set.contains(&(before, after))
                    || !overlaps(self.envelopes[index], self.envelopes[other])
                {
                    continue;
                }
                // Coincident surfaces can still exceed this fixed budget.
                if self.pairs.len() >= (self.ranks.len() * 8).min(32_768) {
                    return Err(BatchPlanRebuildReason::OrderingCapacity);
                }
                let pair = self.pairs.len();
                self.pairs.push((before, after));
                self.pair_set.insert((before, after));
                self.neighbors[before].push(pair);
                self.neighbors[after].push(pair);
                self.checked.push(false);
            }
        }
        Ok(())
    }

    fn check(
        &mut self,
        pair: usize,
        draws: &[PreparedDraw],
        inputs: &[Input],
        camera: Mat4,
        margin: Vec3,
        stats: &mut Checks,
    ) -> bool {
        if self.checked[pair] {
            return true;
        }
        self.checked[pair] = true;
        stats.pairs += 1;
        let (a, b) = self.pairs[pair];
        if self.screen_space {
            return !overlaps(self.world[a], self.world[b]);
        }
        if !overlaps(
            expanded(self.world[a], margin),
            expanded(self.world[b], margin),
        ) {
            return true;
        }
        for index in [a, b] {
            if !self.current[index] {
                self.projected[index] =
                    projected_bounds(inputs[index].bounds, camera * draws[index].object.model);
                self.current[index] = true;
                stats.bounds += 1;
            }
        }
        !overlaps(self.projected[a], self.projected[b])
    }

    fn retain_screen(
        &mut self,
        changed: &[usize],
        camera_changed: bool,
        draws: &[PreparedDraw],
        inputs: &[Input],
        camera: Mat4,
        stats: &mut Checks,
    ) -> std::result::Result<(), BatchPlanRebuildReason> {
        let mut escaped = std::mem::take(&mut self.escaped);
        escaped.clear();
        self.seen.fill(false);
        for &index in changed {
            self.seen[index] = true;
        }
        for (index, input) in inputs.iter().enumerate() {
            if !input.visible || input.transparent || !camera_changed && !self.seen[index] {
                continue;
            }
            if !affine(draws[index].object.model) {
                return Err(BatchPlanRebuildReason::NonAffineModel);
            }
            let bounds = projected_bounds(input.bounds, camera * draws[index].object.model);
            if !bounds[0].is_finite() || !bounds[1].is_finite() {
                return Err(BatchPlanRebuildReason::UnboundedGeometry);
            }
            self.world[index] = bounds;
            self.projected[index] = bounds;
            stats.bounds += 1;
            if !contains(self.envelopes[index], bounds) {
                escaped.push(index);
            }
        }
        self.checked.fill(false);
        if !escaped.is_empty() {
            self.renew(&escaped, Vec3::ZERO)?;
            stats.recertified = true;
        }
        if camera_changed {
            for pair in 0..self.pairs.len() {
                if !self.check(pair, draws, inputs, camera, Vec3::ZERO, stats) {
                    return Err(BatchPlanRebuildReason::OrderingConflict);
                }
            }
        } else {
            for &index in changed.iter().chain(&escaped) {
                for neighbor in 0..self.neighbors[index].len() {
                    let pair = self.neighbors[index][neighbor];
                    if !self.check(pair, draws, inputs, camera, Vec3::ZERO, stats) {
                        return Err(BatchPlanRebuildReason::OrderingConflict);
                    }
                }
            }
        }
        self.escaped = escaped;
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Checks {
    pub bounds: usize,
    pub pairs: usize,
    pub recertified: bool,
}

pub(super) fn retain(
    plan: &mut Plan,
    draws: &[PreparedDraw],
    visible: &[bool],
    camera: Mat4,
    incremental: bool,
    bounds: &[Bounds],
) -> std::result::Result<Checks, BatchPlanRebuildReason> {
    if plan.inputs.len() != draws.len() {
        return Err(BatchPlanRebuildReason::Membership);
    }
    let mut changed = std::mem::take(&mut plan.changed);
    changed.clear();
    let original = plan
        .ordering
        .as_ref()
        .is_ok_and(|ordering| ordering.original);
    for (index, ((input, draw), &visible)) in
        plan.inputs.iter_mut().zip(draws).zip(visible).enumerate()
    {
        let visible = plan.all_surfaces || visible;
        if input.visible != visible {
            return Err(BatchPlanRebuildReason::Visibility);
        }
        if input.bounds != bounds[index] {
            if !original {
                return Err(BatchPlanRebuildReason::Bounds);
            }
            // Original order remains safe through deformation and its changing
            // bounds. Culling still uses the current bounds supplied by caller.
            input.bounds = bounds[index];
        }
        if !input.matches_metadata(draw, bounds[index], visible) {
            return Err(BatchPlanRebuildReason::Metadata);
        }
        if input.model != draw.object.model {
            changed.push(index);
        }
    }
    let camera_changed = plan.camera != camera;
    let mut stats = Checks::default();
    if !camera_changed && changed.is_empty() {
        plan.changed = changed;
        return Ok(stats);
    }
    if !incremental {
        return Err(BatchPlanRebuildReason::IncrementalDisabled);
    }
    let ordering = plan.ordering.as_mut().map_err(|reason| *reason)?;
    if !ordering.original {
        if ordering.screen_space {
            ordering.retain_screen(
                &changed,
                camera_changed,
                draws,
                &plan.inputs,
                camera,
                &mut stats,
            )?;
        } else {
            let margin = padding(camera).ok_or(BatchPlanRebuildReason::UnsupportedProjection)?;
            let mut escaped = std::mem::take(&mut ordering.escaped);
            escaped.clear();
            ordering.seen.fill(false);
            for &index in &changed {
                let input = &plan.inputs[index];
                if !input.visible || input.transparent {
                    continue;
                }
                let model = draws[index].object.model;
                if !affine(model) {
                    return Err(BatchPlanRebuildReason::NonAffineModel);
                }
                ordering.world[index] = projected_bounds(input.bounds, model);
                ordering.current[index] = false;
                stats.bounds += 1;
                if !contains(
                    ordering.envelopes[index],
                    expanded(ordering.world[index], margin),
                ) {
                    escaped.push(index);
                    ordering.seen[index] = true;
                }
            }
            ordering.checked.fill(false);
            if camera_changed {
                ordering.current.fill(false);
                for (index, input) in plan.inputs.iter().enumerate() {
                    if input.visible
                        && !input.transparent
                        && !contains(
                            ordering.envelopes[index],
                            expanded(ordering.world[index], margin),
                        )
                        && !ordering.seen[index]
                    {
                        escaped.push(index);
                        ordering.seen[index] = true;
                    }
                }
            }
            if !escaped.is_empty() {
                ordering.renew(&escaped, margin)?;
                stats.recertified = true;
            }
            if camera_changed {
                for pair in 0..ordering.pairs.len() {
                    if !ordering.check(pair, draws, &plan.inputs, camera, margin, &mut stats) {
                        return Err(BatchPlanRebuildReason::OrderingConflict);
                    }
                }
            } else {
                for &index in changed.iter().chain(&escaped) {
                    for neighbor in 0..ordering.neighbors[index].len() {
                        let pair = ordering.neighbors[index][neighbor];
                        if !ordering.check(pair, draws, &plan.inputs, camera, margin, &mut stats) {
                            return Err(BatchPlanRebuildReason::OrderingConflict);
                        }
                    }
                }
            }
            ordering.escaped = escaped;
        }
    }
    for &index in &changed {
        plan.inputs[index].model = draws[index].object.model;
    }
    plan.camera = camera;
    plan.changed = changed;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Mat4 {
        glam::camera::rh::proj::directx::orthographic(-30., 30., -30., 30., 0.1, 100.)
    }

    fn draws(count: usize) -> Vec<PreparedDraw> {
        (0..count)
            .map(|index| PreparedDraw {
                preparation: Default::default(),
                source_item: index,
                deformation: 0,
                shared_geometry: None,
                world_geometry_units: None,
                pbr_override: [-1.; 2],
                shader: None,
                pbr: false,
                raster: 0,
                opacity: 1.,
                cutoff: 0.,
                transparent: false,
                depth: 0.,
                object: DrawItem {
                    motion_id: index as u64 + 1,
                    model: Mat4::from_translation(Vec3::new(
                        (index % 16) as f32 * 2. - 15.,
                        (index / 16) as f32 * 2. - 7.,
                        -5.,
                    )),
                    mesh: MeshKind::Quad,
                    material: Material {
                        metallic: None,
                        roughness: None,
                        surface_overrides: Default::default(),
                        tint: [1.; 3],
                        uv_scale: [1.; 2],
                        texture: match index % 3 {
                            0 => TextureKind::White,
                            1 => TextureKind::Checker,
                            _ => TextureKind::Normals,
                        },
                        lit: false,
                        shader: None,
                    },
                },
            })
            .collect()
    }

    fn plan(draws: &[PreparedDraw], camera: Mat4, bounds: &[[Vec3; 2]]) -> Plan {
        let inputs = draws
            .iter()
            .zip(bounds)
            .map(|(draw, &bounds)| Input {
                mesh: draw.object.mesh.clone(),
                texture: draw.object.material.texture.clone(),
                model: draw.object.model,
                bounds,
                shader: draw.shader,
                deformation: draw.deformation,
                shared_geometry: draw.shared_geometry.clone(),
                world_geometry_units: draw.world_geometry_units,
                pbr: draw.pbr,
                lit: draw.object.material.lit,
                transparent: draw.transparent,
                visible: true,
                raster: draw.raster,
            })
            .collect::<Vec<_>>();
        let (batches, projected, _) = global_batches(draws, &inputs, camera, true, false);
        let ordering = Ordering::new(&inputs, &batches, camera, projected);
        let batch_of = batch_membership(draws.len(), &batches);
        Plan {
            camera,
            all_surfaces: true,
            inputs,
            batches,
            ordering,
            changed: Vec::new(),
            batch_of,
        }
    }

    #[test]
    fn optimal_original_order_skips_every_projection_and_overlap_edge() {
        let mut draws = draws(10_000);
        for draw in &mut draws {
            draw.object.material.texture = TextureKind::White;
            draw.object.model = Mat4::IDENTITY;
        }
        let bounds = vec![[Vec3::splat(-1.), Vec3::splat(1.)]; draws.len()];
        let plan = plan(&draws, camera(), &bounds);
        assert_eq!(plan.batches.len(), draws.len().div_ceil(MAX_INSTANCES));
        let ordering = plan.ordering.unwrap();
        assert!(ordering.original);
        assert!(ordering.projected.is_empty());
        assert!(ordering.pairs.is_empty());
        assert!(ordering.neighbors.is_empty());
    }

    #[test]
    fn dense_multi_key_construction_falls_back_before_quadratic_storage() {
        let mut draws = draws(1024);
        for draw in &mut draws {
            draw.object.model = Mat4::IDENTITY;
        }
        let bounds = vec![[Vec3::splat(-1.), Vec3::splat(1.)]; draws.len()];
        let plan = plan(&draws, camera(), &bounds);
        let (batches, projected, limited) =
            global_batches(&draws, &plan.inputs, camera(), true, false);
        assert!(limited);
        assert!(projected.is_empty());
        assert_eq!(batches.len(), draws.len());
        assert_eq!(
            batches
                .iter()
                .flat_map(|b| &b.indices)
                .copied()
                .collect::<Vec<_>>(),
            (0..draws.len()).collect::<Vec<_>>()
        );
    }

    fn options(capacity: usize, all_surfaces: bool) -> PlanOptions {
        PlanOptions {
            graphs: true,
            transparent_runs: false,
            capacity,
            text_limit: 0,
            incremental: true,
            all_surfaces,
        }
    }

    #[test]
    fn hidden_construction_fallback_is_rejected_through_camera_and_visibility_churn() {
        for capacity in [MAX_INSTANCES, arena::MAX_NATIVE_INSTANCES] {
            // Exactly the production admission ratio: one quarter visible.
            // Visible copies are disjoint, but the hidden mixed-key population
            // exhausts the bounded DAG. Its source order still certifies.
            const VISIBLE: usize = 160;
            let mut draws = draws(4 * VISIBLE);
            for (index, draw) in draws.iter_mut().enumerate() {
                draw.object.material.texture = if index % 2 == 0 {
                    TextureKind::White
                } else {
                    TextureKind::Checker
                };
                if index >= VISIBLE {
                    draw.object.model = Mat4::from_translation(Vec3::new(1000., 0., -5.));
                }
            }
            let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
            let mut visible = vec![false; draws.len()];
            visible[..VISIBLE].fill(true);
            let full = build_plan(
                &draws,
                &bounds,
                &vec![true; draws.len()],
                camera(),
                options(capacity, false),
            );
            assert!(full.construction_limited);
            assert!(full.plan.ordering.as_ref().unwrap().original);
            assert_eq!(
                visible_batches(&full.plan, &visible, Vec::new()).len(),
                VISIBLE
            );

            let mut policy = SupersetPolicy::default();
            assert!(policy.request(None, BatchPlanRebuildReason::Visibility));
            let admitted = build_plan(&draws, &bounds, &visible, camera(), options(capacity, true));
            assert!(admitted.rejected_superset);
            assert!(!admitted.construction_limited);
            assert!(!admitted.plan.all_surfaces);
            let expected = 2 * (VISIBLE / 2).div_ceil(capacity);
            assert_eq!(admitted.plan.batches.len(), expected);
            policy.rejected = admitted.rejected_superset;
            let mut retained = admitted.plan;

            for frame in 1..=24 {
                // Camera motion and object motion accompany the changing
                // frustum membership; none can admit the poor hidden plan.
                visible[..VISIBLE].fill(true);
                visible[frame % VISIBLE] = false;
                draws[0].object.model *= Mat4::from_translation(Vec3::new(0.01, 0., 0.));
                let view = camera() * Mat4::from_rotation_z(frame as f32 * 0.003);
                let reason = retain(&mut retained, &draws, &visible, view, true, &bounds)
                    .err()
                    .unwrap();
                assert_eq!(reason, BatchPlanRebuildReason::Visibility);
                let promote = policy.request(Some(&retained), reason);
                assert!(
                    !promote,
                    "known dense hidden graph was retried at frame {frame}"
                );
                let built = build_plan(&draws, &bounds, &visible, view, options(capacity, promote));
                assert!(!built.rejected_superset);
                assert!(!built.construction_limited);
                assert!(!built.plan.all_surfaces);
                assert_eq!(built.plan.batches.len(), expected);
                retained = built.plan;
                assert_safe(&retained, &draws, view, &visible);
                // The recovered visible certificate also survives motion when
                // membership is unchanged, rather than rebuilding every frame.
                let moved_view = view * Mat4::from_translation(Vec3::new(0.001, 0., 0.));
                retain(&mut retained, &draws, &visible, moved_view, true, &bounds).unwrap();
                assert_safe(&retained, &draws, moved_view, &visible);
            }
            // Structural changes can make a previously rejected population
            // useful. They reset the bounded admission policy for later churn.
            for reason in [
                BatchPlanRebuildReason::Membership,
                BatchPlanRebuildReason::Metadata,
                BatchPlanRebuildReason::Bounds,
            ] {
                assert!(!policy.request(Some(&retained), reason));
                assert!(policy.request(Some(&retained), BatchPlanRebuildReason::Visibility));
                policy.rejected = true;
            }
        }
    }

    #[test]
    fn original_order_cliques_do_not_exhaust_inversion_discovery_work() {
        const COUNT: usize = 4096;
        let mut draws = draws(COUNT);
        for draw in &mut draws {
            draw.object.model = Mat4::from_translation(Vec3::new(0., 0., -5.));
            draw.object.material.texture = TextureKind::White;
        }
        draws[COUNT - 2].object.model = Mat4::from_translation(Vec3::new(-1000., 0., -5.));
        draws[COUNT - 1].object.model = Mat4::from_translation(Vec3::new(1000., 0., -5.));
        let bounds = vec![[Vec3::splat(-100.), Vec3::splat(100.)]; COUNT];
        let inputs = plan(&draws, camera(), &bounds).inputs;
        let mut indices: Vec<_> = (0..COUNT).collect();
        indices.swap(COUNT - 2, COUNT - 1);
        let batches: Vec<_> = indices
            .chunks(MAX_INSTANCES)
            .map(|indices| Batch {
                indices: indices.to_vec(),
                slot: None,
                first_instance: 0,
            })
            .collect();
        let projected = inputs
            .iter()
            .map(|input| projected_bounds(input.bounds, camera() * input.model))
            .collect();
        let ordering = Ordering::new(&inputs, &batches, camera(), projected).unwrap();
        assert!(!ordering.original);
        assert!(ordering.pairs.is_empty());
        let mut grid = spatial::Index::new(&ordering.envelopes, &ordering.ranks);
        let mut neighbors = Vec::new();
        grid.query(ordering.envelopes[0], &mut neighbors);
        assert!(neighbors.len() >= COUNT - 2);
        // The old all-neighbor enumeration exceeds the work budget even though
        // this dense clique preserves every winner and needs no inversion pair.
        assert!(inputs.len() * neighbors.len() > inputs.len() * 64);
        let mut discovery = spatial::InversionIndex::new(&ordering.envelopes, &ordering.ranks);
        let mut work = 0;
        for index in 0..COUNT {
            assert!(discovery.query(
                index,
                ordering.ranks[index],
                ordering.envelopes[index],
                &mut neighbors,
                &mut work,
                COUNT * 64
            ));
            assert!(neighbors.is_empty());
            assert!(discovery.activate(
                index,
                ordering.ranks[index],
                ordering.envelopes[index],
                &mut work,
                COUNT * 64
            ));
        }
        let mut retained = Plan {
            camera: camera(),
            all_surfaces: true,
            inputs,
            batch_of: batch_membership(COUNT, &batches),
            batches,
            ordering: Ok(ordering),
            changed: Vec::new(),
        };
        let visible = vec![true; COUNT];
        let view = camera() * Mat4::from_rotation_z(0.01);
        let checks = retain(&mut retained, &draws, &visible, view, true, &bounds).unwrap();
        assert_eq!(checks.bounds, 0);
        assert_eq!(checks.pairs, 0);
        let mut sample = vec![false; COUNT];
        for index in (0..COUNT).step_by(128).chain([COUNT - 2, COUNT - 1]) {
            sample[index] = true;
        }
        assert_safe(&retained, &draws, view, &sample);
        println!(
            "ordering_rank_prune_proof population={COUNT} total_query_activation_work={work} budget={} retained_pairs=0 dense_source_order_winners_preserved=true disjoint_tail_reorder_camera_reused=true",
            COUNT * 64
        );
    }

    #[test]
    fn initial_dense_inversions_reach_pair_capacity_before_work_capacity() {
        let mut draws = draws(128);
        for draw in &mut draws {
            draw.object.model = Mat4::from_translation(Vec3::new(0., 0., -5.));
            draw.object.material.texture = TextureKind::White;
        }
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let inputs = plan(&draws, camera(), &bounds).inputs;
        let count = inputs.len();
        // Source-order activation of a reversed emitted schedule produces real
        // overlapping inversions. Count actual queries and updates, proving the
        // retained-pair cap is reached before their shared traversal-work cap.
        let batches = [Batch {
            indices: (0..count).rev().collect(),
            slot: None,
            first_instance: 0,
        }];
        let projected = inputs
            .iter()
            .map(|input| projected_bounds(input.bounds, camera() * input.model))
            .collect();
        let ranks: Vec<_> = (0..count).rev().collect();
        let mut discovery = spatial::InversionIndex::new(&bounds, &ranks);
        let mut work = 0;
        let mut pairs = 0;
        let mut candidates = Vec::new();
        for index in 0..count {
            assert!(discovery.query(
                index,
                ranks[index],
                bounds[index],
                &mut candidates,
                &mut work,
                count * 64
            ));
            pairs += candidates.len();
            if pairs > count * 8 {
                break;
            }
            assert!(discovery.activate(index, ranks[index], bounds[index], &mut work, count * 64));
        }
        assert!(pairs > count * 8);
        assert!(work < count * 64);
        assert_eq!(
            Ordering::new(&inputs, &batches, camera(), projected).err(),
            Some(BatchPlanRebuildReason::OrderingCapacity)
        );
    }

    #[test]
    fn sparse_plane_admits_a_quality_superset_and_retains_camera_visibility_churn() {
        const SIDE: usize = 128;
        const COUNT: usize = SIDE * SIDE;
        const KEYS: usize = COUNT / 32;
        // Even the best single-axis sweep examines over one million pairs on
        // this disjoint plane, beyond the unchanged 524,288-candidate ceiling.
        // Complete spatial neighborhoods stay small despite the long plane rows.
        const {
            assert!(SIDE * (SIDE * (SIDE - 1) / 2) > MAX_PLAN_CANDIDATES);
        }
        let mut draws = draws(COUNT);
        for (index, draw) in draws.iter_mut().enumerate() {
            draw.object.model = Mat4::from_translation(Vec3::new(
                (index % SIDE) as f32 * 2. - SIDE as f32,
                (index / SIDE) as f32 * 2. - SIDE as f32,
                -5.,
            ));
            // 32 peers/key fit one group on both portable/native paths. Keys
            // are interleaved, so the original-order fast path cannot mask a
            // construction or certificate broadphase regression.
            draw.object.material.texture =
                TextureKind::Imported(format!("sparse-key-{}", index % KEYS));
        }
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; COUNT];
        let mut visible: Vec<_> = (0..COUNT).map(|index| index % 4 == 0).collect();
        let view =
            glam::camera::rh::proj::directx::orthographic(-200., 200., -200., 200., 0.1, 100.);
        for capacity in [MAX_INSTANCES, arena::MAX_NATIVE_INSTANCES] {
            let built = build_plan(&draws, &bounds, &visible, view, options(capacity, true));
            assert!(!built.construction_limited);
            assert!(!built.rejected_superset);
            assert!(built.plan.all_surfaces);
            assert_eq!(built.plan.batches.len(), KEYS);
            let mut plan = built.plan;
            let ordering = plan.ordering.as_ref().unwrap();
            assert!(!ordering.original);
            assert!(ordering.pairs.is_empty());
            for frame in 1..=24 {
                for (index, value) in visible.iter_mut().enumerate() {
                    *value = index % 4 == frame % 4;
                }
                let camera = view * Mat4::from_rotation_z(frame as f32 * 0.01);
                let checks = retain(&mut plan, &draws, &visible, camera, true, &bounds).unwrap();
                assert_eq!(checks.bounds, 0);
                assert_eq!(checks.pairs, 0);
                assert!(!checks.recertified);
                let output = visible_batches(&plan, &visible, Vec::new());
                assert_eq!(output.len(), KEYS / 4);
                assert_eq!(
                    output
                        .iter()
                        .map(|batch| batch.indices.len())
                        .sum::<usize>(),
                    COUNT / 4
                );
                assert!(
                    output
                        .iter()
                        .flat_map(|batch| &batch.indices)
                        .all(|&index| visible[index])
                );
                // Exhaustively check a dispersed subset of 64 newly admitted
                // boxes, covering inversions across widely separated rows.
                let sample: Vec<_> = (0..COUNT).map(|index| index % 256 == frame % 4).collect();
                assert_safe(&plan, &draws, camera, &sample);
            }
            visible
                .iter_mut()
                .enumerate()
                .for_each(|(index, value)| *value = index % 4 == 0);
        }
    }

    #[test]
    fn certified_hidden_bridge_cannot_degrade_visible_grouping() {
        let mut draws = draws(3);
        for (index, draw) in draws.iter_mut().enumerate() {
            draw.object.model = Mat4::from_translation(Vec3::new(index as f32 - 1., 0., -5.));
        }
        draws[2].object.material.texture = draws[0].object.material.texture.clone();
        let bounds = [
            [Vec3::splat(-0.4), Vec3::splat(0.4)],
            [Vec3::splat(-1.8), Vec3::splat(1.8)],
            [Vec3::splat(-0.4), Vec3::splat(0.4)],
        ];
        let visible = [true, false, true];
        for capacity in [MAX_INSTANCES, arena::MAX_NATIVE_INSTANCES] {
            let full = build_plan(
                &draws,
                &bounds,
                &[true; 3],
                camera(),
                options(capacity, false),
            );
            assert!(!full.construction_limited);
            assert!(full.plan.ordering.is_ok());
            assert_eq!(visible_batches(&full.plan, &visible, Vec::new()).len(), 2);
            let built = build_plan(&draws, &bounds, &visible, camera(), options(capacity, true));
            assert!(built.rejected_superset);
            assert!(!built.plan.all_surfaces);
            assert_eq!(built.plan.batches[0].indices, [0, 2]);
            assert_eq!(built.plan.batches.len(), 1);
            assert_safe(&built.plan, &draws, camera(), &visible);
        }
    }

    #[test]
    fn useful_hidden_plan_is_admitted_and_reuses_visibility_changes() {
        let mut draws = draws(128);
        for (index, draw) in draws.iter_mut().enumerate() {
            draw.object.material.texture = if index % 2 == 0 {
                TextureKind::White
            } else {
                TextureKind::Checker
            };
        }
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut visible = vec![false; draws.len()];
        visible[..64].fill(true);
        for capacity in [MAX_INSTANCES, arena::MAX_NATIVE_INSTANCES] {
            let built = build_plan(&draws, &bounds, &visible, camera(), options(capacity, true));
            assert!(!built.rejected_superset);
            assert!(!built.construction_limited);
            assert!(built.plan.all_surfaces);
            let mut plan = built.plan;
            for index in 64..128 {
                visible[index] = true;
                retain(&mut plan, &draws, &visible, camera(), true, &bounds).unwrap();
                assert_eq!(visible_batches(&plan, &visible, Vec::new()).len(), 2);
            }
            assert_safe(&plan, &draws, camera(), &visible);
            visible[64..].fill(false);
        }
    }

    #[test]
    fn lookahead_unlocks_a_compatible_ready_peer_without_relaxing_edges() {
        let mut draws = draws(3);
        draws[2].object.material.texture = draws[0].object.material.texture.clone();
        draws[0].object.model = Mat4::from_translation(Vec3::new(-10., 0., -5.));
        draws[1].object.model = Mat4::from_translation(Vec3::new(10., 0., -5.));
        draws[2].object.model = draws[1].object.model;
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let plan = plan(&draws, camera(), &bounds);
        assert_eq!(
            plan.batches
                .iter()
                .map(|b| b.indices.clone())
                .collect::<Vec<_>>(),
            [vec![1], vec![0, 2]]
        );
        assert_safe(&plan, &draws, camera(), &[true; 3]);
    }

    #[test]
    fn perspective_motion_reuses_a_safe_order_and_rejects_new_overlap() {
        let mut draws = draws(9);
        let view = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 100.);
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut plan = plan(&draws, view, &bounds);
        assert!(plan.ordering.as_ref().unwrap().screen_space);
        draws[3].object.model *= Mat4::from_translation(Vec3::new(0.01, 0., 0.));
        let checks = retain(&mut plan, &draws, &[true; 9], view, true, &bounds).unwrap();
        assert_eq!(checks.bounds, 1);
        assert_safe(&plan, &draws, view, &[true; 9]);
        let moved_view = view * Mat4::from_translation(Vec3::new(-0.01, 0., 0.));
        retain(&mut plan, &draws, &[true; 9], moved_view, true, &bounds).unwrap();
        assert_safe(&plan, &draws, moved_view, &[true; 9]);
        let order: Vec<_> = plan
            .batches
            .iter()
            .flat_map(|b| b.indices.iter().copied())
            .collect();
        let mut ranks = vec![0; draws.len()];
        for (rank, index) in order.into_iter().enumerate() {
            ranks[index] = rank;
        }
        let (before, after) = (0..draws.len())
            .find_map(|before| {
                (before + 1..draws.len())
                    .find(|&after| ranks[before] > ranks[after])
                    .map(|after| (before, after))
            })
            .expect("fixture must contain a reordered pair");
        draws[after].object.model = draws[before].object.model;
        assert_eq!(
            retain(&mut plan, &draws, &[true; 9], moved_view, true, &bounds).err(),
            Some(BatchPlanRebuildReason::OrderingConflict)
        );
    }

    #[test]
    fn visibility_churn_preserves_later_group_buffer_slots() {
        let draws = draws(9);
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut plan = plan(&draws, camera(), &bounds);
        for (slot, batch) in plan.batches.iter_mut().enumerate() {
            batch.slot = Some(slot + 5);
        }
        let expected = plan.batches[1].slot;
        let mut visible = vec![true; draws.len()];
        for &index in &plan.batches[0].indices {
            visible[index] = false;
        }
        let output = visible_batches(&plan, &visible, Vec::new());
        assert_eq!(output[0].slot, expected);
        assert_eq!(plan.batches[1].slot, expected);
    }

    #[test]
    fn original_order_accepts_changing_deformation_bounds() {
        let mut draws = draws(4);
        for draw in &mut draws {
            draw.object.material.texture = TextureKind::White;
        }
        let mut bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut plan = plan(&draws, camera(), &bounds);
        assert!(plan.ordering.as_ref().unwrap().original);
        bounds[0][1] = Vec3::splat(2.);
        retain(&mut plan, &draws, &[true; 4], camera(), true, &bounds).unwrap();
        assert_eq!(plan.inputs[0].bounds, bounds[0]);
        assert_safe(&plan, &draws, camera(), &[true; 4]);
    }

    #[test]
    fn animated_groups_require_exact_current_previous_and_tangent_streams() -> anyhow::Result<()> {
        let gpu = pollster::block_on(Gpu::request_prefer_software(&crate::instance(
            crate::Backend::native(),
        )))?;
        let buffer = || {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("animated compatibility test stream"),
                size: 64,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            })
        };
        let shared = GeometryKey {
            vertices: buffer(),
            previous: Some(buffer()),
            tangents: Some(buffer()),
        };
        let mut draws = draws(4);
        for (index, draw) in draws.iter_mut().enumerate() {
            draw.object.material.texture = TextureKind::White;
            draw.deformation = index as u64 + 1;
            draw.shared_geometry = Some(shared.clone());
        }
        assert!(compatible(&draws[0], &draws[1], true, false));
        draws[2].shared_geometry.as_mut().unwrap().previous = Some(buffer());
        draws[3].shared_geometry.as_mut().unwrap().tangents = Some(buffer());
        assert!(!compatible(&draws[0], &draws[2], true, false));
        assert!(!compatible(&draws[0], &draws[3], true, false));
        let mut bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut plan = plan(&draws, camera(), &bounds);
        assert_eq!(
            plan.batches
                .iter()
                .map(|b| b.indices.len())
                .collect::<Vec<_>>(),
            [2, 1, 1]
        );
        assert!(plan.ordering.as_ref().unwrap().original);
        // A dispatch updates buffer contents/revision without changing command
        // compatibility; bounds changes are also safe in this original order.
        draws[0].deformation += 10;
        bounds[0][1] = Vec3::splat(2.);
        retain(&mut plan, &draws, &[true; 4], camera(), true, &bounds).unwrap();
        draws[0].shared_geometry = None;
        assert!(key(&draws[0], true).is_none());
        assert_eq!(
            retain(&mut plan, &draws, &[true; 4], camera(), true, &bounds).err(),
            Some(BatchPlanRebuildReason::Metadata)
        );
        Ok(())
    }

    #[test]
    fn heterogeneous_text_requires_native_ordered_runs_and_preserves_state_boundaries() {
        let mut draws = draws(5);
        for (index, draw) in draws.iter_mut().enumerate() {
            draw.transparent = true;
            draw.world_geometry_units = Some(11);
            draw.object.material.texture = TextureKind::Text;
            draw.object.mesh = MeshKind::SharedText(std::sync::Arc::new(TextMesh {
                text: format!("glyph run {index}"),
                ..Default::default()
            }));
        }
        draws[2].object.material.texture = TextureKind::White;
        let sizes = |capacity, allowed| {
            batches_with_capacity(&draws, &[true; 5], true, true, allowed, capacity, 16 * 1024)
                .iter()
                .map(|batch| batch.indices.len())
                .collect::<Vec<_>>()
        };
        assert_eq!(sizes(MAX_INSTANCES, true), [1; 5]);
        assert_eq!(sizes(arena::MAX_NATIVE_INSTANCES, true), [2, 1, 2]);
        assert_eq!(sizes(arena::MAX_NATIVE_INSTANCES, false), [1; 5]);
        assert_eq!(
            batches_with_capacity(
                &draws,
                &[true; 5],
                true,
                true,
                true,
                arena::MAX_NATIVE_INSTANCES,
                11
            )
            .iter()
            .map(|b| b.indices.len())
            .collect::<Vec<_>>(),
            [1; 5]
        );
        for draw in &mut draws {
            draw.object.material.texture = TextureKind::Text;
            draw.world_geometry_units = Some(32);
            draw.object.mesh = MeshKind::SharedText(std::sync::Arc::new(TextMesh {
                text: "x".repeat(32),
                ..Default::default()
            }));
        }
        // Identical over-budget geometry remains one ordinary instance group;
        // it does not require per-vertex geometry copies or larger ID buffers.
        assert_eq!(
            batches_with_capacity(
                &draws,
                &[true; 5],
                true,
                true,
                true,
                arena::MAX_NATIVE_INSTANCES,
                11
            )
            .iter()
            .map(|b| b.indices.len())
            .collect::<Vec<_>>(),
            [5]
        );
    }

    fn assert_safe(plan: &Plan, draws: &[PreparedDraw], camera: Mat4, visible: &[bool]) {
        let output = visible_batches(plan, visible, Vec::new());
        let order: Vec<_> = output
            .iter()
            .flat_map(|batch| &batch.indices)
            .copied()
            .collect();
        let mut ranks = vec![usize::MAX; draws.len()];
        for (rank, &index) in order.iter().enumerate() {
            assert_eq!(ranks[index], usize::MAX, "duplicate surface");
            ranks[index] = rank;
        }
        for (index, &visible) in visible.iter().enumerate() {
            assert_eq!(ranks[index] != usize::MAX, visible);
        }
        let margin = padding(camera);
        let visible_indices: Vec<_> = visible
            .iter()
            .enumerate()
            .filter_map(|(index, &visible)| visible.then_some(index))
            .collect();
        for (position, &a) in visible_indices.iter().enumerate() {
            for &b in &visible_indices[position + 1..] {
                if ranks[a] < ranks[b] {
                    continue;
                }
                // Independent, exhaustive check of each reordered opaque pair.
                let bounds_a = plan.inputs[a].bounds;
                let bounds_b = plan.inputs[b].bounds;
                let world_a = expanded(
                    projected_bounds(bounds_a, draws[a].object.model),
                    margin.unwrap_or(Vec3::ZERO),
                );
                let world_b = expanded(
                    projected_bounds(bounds_b, draws[b].object.model),
                    margin.unwrap_or(Vec3::ZERO),
                );
                let screen_a = projected_bounds(bounds_a, camera * draws[a].object.model);
                let screen_b = projected_bounds(bounds_b, camera * draws[b].object.model);
                assert!(
                    margin.is_some() && !overlaps(world_a, world_b)
                        || !overlaps(screen_a, screen_b),
                    "unsafe inversion of surfaces {a} and {b}"
                );
            }
        }
    }

    #[test]
    fn filtering_reuses_index_storage_without_mutating_the_certificate() {
        let draws = draws(9);
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let plan = plan(&draws, camera(), &bounds);
        let mut output = visible_batches(&plan, &[true; 9], Vec::new());
        let pointers: Vec<_> = output.iter().map(|batch| batch.indices.as_ptr()).collect();
        for batch in &mut output {
            batch.slot = Some(7);
        }
        let output = visible_batches(&plan, &[true; 9], output);
        assert_eq!(
            pointers,
            output
                .iter()
                .map(|batch| batch.indices.as_ptr())
                .collect::<Vec<_>>()
        );
        assert!(output.iter().all(|batch| batch.slot.is_none()));
        let hidden = visible_batches(&plan, &[false; 9], output);
        assert!(hidden.is_empty());
        assert_eq!(
            plan.batches
                .iter()
                .map(|batch| batch.indices.len())
                .sum::<usize>(),
            9
        );
        assert_safe(
            &plan,
            &draws,
            camera(),
            &[true, false, true, false, true, false, true, false, true],
        );
    }

    #[test]
    fn safe_envelope_escape_renews_without_regrouping() {
        let mut draws = draws(9);
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut plan = plan(&draws, camera(), &bounds);
        let old_order: Vec<_> = plan
            .batches
            .iter()
            .flat_map(|batch| &batch.indices)
            .copied()
            .collect();
        draws[3].object.model = Mat4::from_translation(Vec3::new(8., 8., -5.));
        draws[1].object.model = Mat4::from_translation(Vec3::new(8.86, 8., -5.));
        let checks = retain(&mut plan, &draws, &[true; 9], camera(), true, &bounds).unwrap();
        assert!(checks.recertified);
        assert_eq!(
            old_order,
            plan.batches
                .iter()
                .flat_map(|batch| &batch.indices)
                .copied()
                .collect::<Vec<_>>()
        );
        assert_safe(&plan, &draws, camera(), &[true; 9]);
        // A second edit must consult the renewed neighborhood, not the old one.
        draws[3].object.model = Mat4::from_translation(Vec3::new(8.08, 8., -5.));
        let result = retain(&mut plan, &draws, &[true; 9], camera(), true, &bounds);
        assert_eq!(result.err(), Some(BatchPlanRebuildReason::OrderingConflict));
    }

    #[test]
    fn simultaneous_escapes_discover_conflicts_at_both_new_positions() {
        let mut draws = draws(4);
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut plan = plan(&draws, camera(), &bounds);
        // Surface 3 is grouped with 0 ahead of surface 1. Both leave their old
        // disjoint neighborhoods and collide; checking either old location is wrong.
        draws[1].object.model = Mat4::from_translation(Vec3::new(8., 8., -5.));
        draws[3].object.model = draws[1].object.model;
        // Hidden surfaces are still certified before they can re-enter the frustum.
        let result = retain(
            &mut plan,
            &draws,
            &[true, false, false, false],
            camera(),
            true,
            &bounds,
        );
        assert_eq!(result.err(), Some(BatchPlanRebuildReason::OrderingConflict));
    }

    #[test]
    fn uncertain_projection_models_and_changed_mesh_bounds_keep_the_fallback() {
        let mut draws = draws(4);
        let mut bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut cached = plan(&draws, camera(), &bounds);
        let perspective = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 100.);
        assert_eq!(
            retain(&mut cached, &draws, &[true; 4], perspective, true, &bounds).err(),
            Some(BatchPlanRebuildReason::UnsupportedProjection)
        );
        let mut cached = plan(&draws, camera(), &bounds);
        draws[3].object.model.x_axis.w = 0.01;
        assert_eq!(
            retain(&mut cached, &draws, &[true; 4], camera(), true, &bounds).err(),
            Some(BatchPlanRebuildReason::NonAffineModel)
        );
        draws[3].object.model.x_axis.w = 0.;
        let mut cached = plan(&draws, camera(), &bounds);
        bounds[2][0].x -= 1.;
        assert_eq!(
            retain(&mut cached, &draws, &[true; 4], camera(), true, &bounds).err(),
            Some(BatchPlanRebuildReason::Bounds)
        );
    }

    #[test]
    fn renewal_pair_storage_is_bounded() {
        let mut draws = draws(128);
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut plan = plan(&draws, camera(), &bounds);
        for draw in &mut draws {
            draw.object.model = Mat4::from_translation(Vec3::new(0., 0., -5.));
        }
        let result = retain(&mut plan, &draws, &[true; 128], camera(), true, &bounds);
        assert_eq!(result.err(), Some(BatchPlanRebuildReason::OrderingCapacity));
        assert_eq!(plan.ordering.as_ref().unwrap().pairs.len(), draws.len() * 8);
    }

    #[test]
    fn randomized_motion_camera_and_visibility_reuse_preserves_every_dependency() {
        let mut random = 0x0062_7ad1_u64;
        let mut next = || {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            (random >> 32) as u32
        };
        let mut draws = draws(48);
        let bounds = vec![[Vec3::splat(-0.4), Vec3::splat(0.4)]; draws.len()];
        let mut view = camera();
        let mut plan = plan(&draws, view, &bounds);
        let mut reused = 0;
        let mut renewed = 0;
        let mut conflicts = 0;
        for frame in 0..300 {
            for _ in 0..2 {
                let index = next() as usize % draws.len();
                let x = (next() % 200) as f32 * 0.1 - 10.;
                let y = (next() % 160) as f32 * 0.1 - 8.;
                draws[index].object.model = Mat4::from_translation(Vec3::new(x, y, -5.))
                    * Mat4::from_rotation_z((next() % 60) as f32 * 0.01);
            }
            if frame % 3 == 0 {
                view = camera() * Mat4::from_rotation_y(frame as f32 * 0.02);
            }
            let visible: Vec<_> = (0..draws.len()).map(|_| next() % 4 != 0).collect();
            match retain(&mut plan, &draws, &visible, view, true, &bounds) {
                Ok(checks) => {
                    reused += 1;
                    renewed += usize::from(checks.recertified);
                }
                Err(reason) => {
                    conflicts += usize::from(reason == BatchPlanRebuildReason::OrderingConflict);
                    plan = self::plan(&draws, view, &bounds);
                }
            }
            assert_safe(&plan, &draws, view, &visible);
        }
        assert!(
            reused > 100 && renewed > 100 && conflicts > 0,
            "test must exercise reuse, renewal and fallback: {reused}/{renewed}/{conflicts}"
        );
    }
    #[test]
    fn native_capacity_reduces_homogeneous_submission_without_graph_construction() {
        let mut draws = draws(10_000);
        for draw in &mut draws {
            draw.object.material.texture = TextureKind::White;
        }
        let bounds = vec![[Vec3::splat(-0.5), Vec3::splat(0.5)]; draws.len()];
        let inputs = plan(&draws, Mat4::IDENTITY, &bounds).inputs;
        let (native, projected, limited) = global_batches_with_capacity(
            &draws,
            &inputs,
            Mat4::IDENTITY,
            true,
            false,
            arena::MAX_NATIVE_INSTANCES,
            16 * 1024,
        );
        assert_eq!(native.len(), 10);
        assert!(projected.is_empty());
        assert!(!limited);
        let (baseline, _, _) = global_batches(&draws, &inputs, Mat4::IDENTITY, true, false);
        assert_eq!(baseline.len(), 157);
    }
    #[test]
    fn long_thin_bounds_select_separating_axis_and_avoid_capacity_fallback() {
        let mut draws = draws(4096);
        for (i, draw) in draws.iter_mut().enumerate() {
            draw.object.model = Mat4::from_translation(Vec3::new(0., i as f32 * 3., -5.));
        }
        let bounds =
            vec![[Vec3::new(-10000., -0.4, -0.4), Vec3::new(10000., 0.4, 0.4)]; draws.len()];
        let plan = plan(&draws, Mat4::IDENTITY, &bounds);
        let (batches, _, limited) =
            global_batches(&draws, &plan.inputs, Mat4::IDENTITY, true, false);
        assert!(!limited);
        assert_eq!(batches.len(), 66);
    }
}
