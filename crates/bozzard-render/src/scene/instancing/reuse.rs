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
}

impl Ordering {
    pub(super) fn new(
        inputs: &[Input],
        batches: &[Batch],
        camera: Mat4,
        projected: Vec<Bounds>,
    ) -> Option<Self> {
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
        };
        // An order that never moved opaque draws is safe for any camera or motion.
        if original {
            return Some(result);
        }
        let margin = padding(camera)?;
        result.world.resize(inputs.len(), [Vec3::ZERO; 2]);
        result.envelopes.resize(inputs.len(), [Vec3::ZERO; 2]);
        for (rank, &index) in order.iter().enumerate() {
            let input = &inputs[index];
            if !affine(input.model) {
                return None;
            }
            let bounds = projected_bounds(input.bounds, input.model);
            if !bounds[0].is_finite() || !bounds[1].is_finite() {
                return None;
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
                    return None;
                }
                let pair = result.pairs.len();
                result.pairs.push((before, after));
                result.neighbors[before].push(pair);
                result.neighbors[after].push(pair);
            }
            active.push(index);
        }
        result.checked.resize(result.pairs.len(), false);
        Some(result)
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
}

pub(super) fn retain(
    plan: &mut Plan,
    draws: &[PreparedDraw],
    visible: &[bool],
    camera: Mat4,
    incremental: bool,
    bounds: &[Bounds],
) -> Option<Checks> {
    if plan.inputs.len() != draws.len() {
        return None;
    }
    let mut changed = Vec::new();
    for (index, ((input, draw), &visible)) in plan.inputs.iter().zip(draws).zip(visible).enumerate()
    {
        if !input.matches_metadata(draw, bounds[index], visible) {
            return None;
        }
        if input.model != draw.object.model {
            changed.push(index);
        }
    }
    let camera_changed = plan.camera != camera;
    let mut stats = Checks::default();
    if !camera_changed && changed.is_empty() {
        return Some(stats);
    }
    if !incremental {
        return None;
    }
    let ordering = plan.ordering.as_mut()?;
    if !ordering.original {
        let margin = padding(camera)?;
        for &index in &changed {
            let input = &plan.inputs[index];
            if !input.visible || input.transparent {
                continue;
            }
            let model = draws[index].object.model;
            if !affine(model) {
                return None;
            }
            ordering.world[index] = projected_bounds(input.bounds, model);
            ordering.current[index] = false;
            stats.bounds += 1;
            if !contains(
                ordering.envelopes[index],
                expanded(ordering.world[index], margin),
            ) {
                return None;
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
                {
                    return None;
                }
            }
            for pair in 0..ordering.pairs.len() {
                if !ordering.check(pair, draws, &plan.inputs, camera, margin, &mut stats) {
                    return None;
                }
            }
        } else {
            for &index in &changed {
                for neighbor in 0..ordering.neighbors[index].len() {
                    let pair = ordering.neighbors[index][neighbor];
                    if !ordering.check(pair, draws, &plan.inputs, camera, margin, &mut stats) {
                        return None;
                    }
                }
            }
        }
    }
    for index in changed {
        plan.inputs[index].model = draws[index].object.model;
    }
    plan.camera = camera;
    Some(stats)
}
