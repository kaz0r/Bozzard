use super::*;

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
    world: Vec<Bounds>,
    envelopes: Vec<Bounds>,
    projected: Vec<Bounds>,
    current: Vec<bool>,
    pairs: Vec<(usize, usize)>,
    neighbors: Vec<Vec<usize>>,
    checked: Vec<bool>,
    ranks: Vec<usize>,
    seen: Vec<bool>,
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
            world: vec![],
            envelopes: vec![],
            projected,
            current: vec![true; inputs.len()],
            pairs: vec![],
            neighbors: (0..inputs.len()).map(|_| Vec::new()).collect(),
            checked: vec![],
            ranks: vec![],
            seen: vec![],
        };
        // An order that never moved opaque draws is safe for any camera or motion.
        if original {
            return Ok(result);
        }
        let margin = padding(camera).ok_or(BatchPlanRebuildReason::UnsupportedProjection)?;
        result.world.resize(inputs.len(), [Vec3::ZERO; 2]);
        result.envelopes.resize(inputs.len(), [Vec3::ZERO; 2]);
        for (rank, &index) in order.iter().enumerate() {
            let input = &inputs[index];
            if !affine(input.model) {
                return Err(BatchPlanRebuildReason::NonAffineModel);
            }
            let bounds = projected_bounds(input.bounds, input.model);
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
        let mut sweep = order;
        sweep.sort_by(|&a, &b| {
            result.envelopes[a][0]
                .x
                .total_cmp(&result.envelopes[b][0].x)
        });
        let mut active: Vec<usize> = Vec::new();
        for index in sweep {
            let bounds = result.envelopes[index];
            active.retain(|&other| result.envelopes[other][1].x >= bounds[0].x);
            for &other in &active {
                let (before, after) = (index.min(other), index.max(other));
                if ranks[before] < ranks[after] || !overlaps(bounds, result.envelopes[other]) {
                    continue;
                }
                // Bound retained memory for scenes with many coincident surfaces.
                if result.pairs.len() >= (inputs.len() * 8).min(32_768) {
                    return Err(BatchPlanRebuildReason::OrderingCapacity);
                }
                let pair = result.pairs.len();
                result.pairs.push((before, after));
                result.neighbors[before].push(pair);
                result.neighbors[after].push(pair);
            }
            active.push(index);
        }
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
        }
        for &index in escaped {
            self.seen.fill(false);
            for &pair in &self.neighbors[index] {
                let (a, b) = self.pairs[pair];
                self.seen[if a == index { b } else { a }] = true;
            }
            for other in 0..self.ranks.len() {
                if other == index || self.ranks[other] == usize::MAX || self.seen[other] {
                    continue;
                }
                let (before, after) = (index.min(other), index.max(other));
                if self.ranks[before] < self.ranks[after]
                    || !overlaps(self.envelopes[index], self.envelopes[other])
                {
                    continue;
                }
                // Stale pairs remain conservative; a long journey may reach
                // this budget and rebuild, but can never grow without bound.
                if self.pairs.len() >= (self.ranks.len() * 8).min(32_768) {
                    return Err(BatchPlanRebuildReason::OrderingCapacity);
                }
                let pair = self.pairs.len();
                self.pairs.push((before, after));
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
    let mut changed = Vec::new();
    for (index, ((input, draw), &visible)) in plan.inputs.iter().zip(draws).zip(visible).enumerate()
    {
        let visible = plan.all_surfaces || visible;
        if input.visible != visible {
            return Err(BatchPlanRebuildReason::Visibility);
        }
        if input.bounds != bounds[index] {
            return Err(BatchPlanRebuildReason::Bounds);
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
        return Ok(stats);
    }
    if !incremental {
        return Err(BatchPlanRebuildReason::IncrementalDisabled);
    }
    let ordering = plan.ordering.as_mut().map_err(|reason| *reason)?;
    if !ordering.original {
        let margin = padding(camera).ok_or(BatchPlanRebuildReason::UnsupportedProjection)?;
        let mut escaped = Vec::new();
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
                    && !escaped.contains(&index)
                {
                    escaped.push(index);
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
    }
    for index in changed {
        plan.inputs[index].model = draws[index].object.model;
    }
    plan.camera = camera;
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
                pbr_override: [-1.; 2],
                shader: None,
                pbr: false,
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
                pbr: draw.pbr,
                lit: draw.object.material.lit,
                transparent: draw.transparent,
                visible: true,
            })
            .collect::<Vec<_>>();
        let (batches, projected) = global_batches(draws, &inputs, camera, true);
        let ordering = Ordering::new(&inputs, &batches, camera, projected);
        Plan {
            camera,
            all_surfaces: true,
            inputs,
            batches,
            ordering,
        }
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
        let margin = padding(camera).unwrap();
        for a in 0..draws.len() {
            for b in a + 1..draws.len() {
                if !visible[a] || !visible[b] || ranks[a] < ranks[b] {
                    continue;
                }
                // Independent, exhaustive check of each reordered opaque pair.
                let bounds_a = plan.inputs[a].bounds;
                let bounds_b = plan.inputs[b].bounds;
                let world_a = expanded(projected_bounds(bounds_a, draws[a].object.model), margin);
                let world_b = expanded(projected_bounds(bounds_b, draws[b].object.model), margin);
                let screen_a = projected_bounds(bounds_a, camera * draws[a].object.model);
                let screen_b = projected_bounds(bounds_b, camera * draws[b].object.model);
                assert!(
                    !overlaps(world_a, world_b) || !overlaps(screen_a, screen_b),
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
}
