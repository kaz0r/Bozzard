//! Conservative caster broad phase shared by all faces in one map set. Every
//! surviving leaf still runs the original local-space homogeneous predicate.
use super::*;

struct Row {
    index: usize,
    bounds: [Vec3; 2],
}
struct Node {
    bounds: [Vec3; 2],
    range: std::ops::Range<usize>,
    children: Option<[usize; 2]>,
}
pub(super) struct CasterIndex {
    rows: Vec<Row>,
    nodes: Vec<Node>,
    uncertain: Vec<usize>,
    sources: Vec<([Vec3; 2], Mat4, bool)>,
    row_by_draw: Vec<Option<usize>>,
}

impl CasterIndex {
    pub fn new(renderer: &SceneRenderer, draws: &[PreparedDraw]) -> Self {
        let mut result = Self {
            rows: Vec::new(),
            nodes: Vec::new(),
            uncertain: Vec::new(),
            sources: Vec::new(),
            row_by_draw: vec![None; draws.len()],
        };
        for (index, draw) in draws.iter().enumerate() {
            let casts = !draw.transparent && draw.object.material.lit;
            let local = renderer.mesh_for(&draw.object).bounds;
            result.sources.push((local, draw.object.model, casts));
            if !casts {
                continue;
            }
            if let Some(bounds) = world_bounds(local, draw.object.model) {
                result.rows.push(Row { index, bounds });
            } else {
                result.uncertain.push(index);
            }
        }
        if !result.rows.is_empty() {
            result.build(0..result.rows.len());
        }
        for (row, value) in result.rows.iter().enumerate() {
            result.row_by_draw[value.index] = Some(row);
        }
        result
    }
    /// Retain the tree topology for movers. Refit ancestors only when exact
    /// model/bounds inputs change; caster/affine membership changes rebuild it.
    pub fn refresh(&mut self, renderer: &SceneRenderer, draws: &[PreparedDraw]) {
        if draws.len() != self.sources.len() {
            *self = Self::new(renderer, draws);
            return;
        }
        let mut changed = false;
        for (index, draw) in draws.iter().enumerate() {
            let source = (
                renderer.mesh_for(&draw.object).bounds,
                draw.object.model,
                !draw.transparent && draw.object.material.lit,
            );
            if self.sources[index] == source {
                continue;
            }
            let world = source.2.then(|| world_bounds(source.0, source.1)).flatten();
            if source.2 != self.sources[index].2
                || (source.2 && world.is_some() != self.row_by_draw[index].is_some())
            {
                *self = Self::new(renderer, draws);
                return;
            }
            if let Some(row) = self.row_by_draw[index] {
                self.rows[row].bounds = world.unwrap();
                changed = true;
            }
            self.sources[index] = source;
        }
        if changed {
            self.refit();
        }
    }
    fn refit(&mut self) {
        for index in (0..self.nodes.len()).rev() {
            let bounds = if let Some([left, right]) = self.nodes[index].children {
                [
                    self.nodes[left].bounds[0].min(self.nodes[right].bounds[0]),
                    self.nodes[left].bounds[1].max(self.nodes[right].bounds[1]),
                ]
            } else {
                let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
                for row in &self.rows[self.nodes[index].range.clone()] {
                    bounds[0] = bounds[0].min(row.bounds[0]);
                    bounds[1] = bounds[1].max(row.bounds[1]);
                }
                bounds
            };
            self.nodes[index].bounds = bounds;
        }
    }
    fn build(&mut self, range: std::ops::Range<usize>) -> usize {
        let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
        for row in &self.rows[range.clone()] {
            bounds[0] = bounds[0].min(row.bounds[0]);
            bounds[1] = bounds[1].max(row.bounds[1]);
        }
        let index = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            range: range.clone(),
            children: None,
        });
        if range.len() > 8 {
            let extent = bounds[1] - bounds[0];
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            let middle = range.start + range.len() / 2;
            self.rows[range.clone()].select_nth_unstable_by(range.len() / 2, |a, b| {
                (a.bounds[0][axis] * 0.5 + a.bounds[1][axis] * 0.5)
                    .total_cmp(&(b.bounds[0][axis] * 0.5 + b.bounds[1][axis] * 0.5))
                    .then(a.index.cmp(&b.index))
            });
            let left = self.build(range.start..middle);
            let right = self.build(middle..range.end);
            self.nodes[index].children = Some([left, right]);
        }
        index
    }
    pub fn query(&self, projection: Mat4) -> Vec<usize> {
        let mut result = self.uncertain.clone();
        if !self.nodes.is_empty() {
            self.visit(0, projection, &mut result);
        }
        result.sort_unstable();
        result
    }
    fn visit(&self, index: usize, projection: Mat4, result: &mut Vec<usize>) {
        let node = &self.nodes[index];
        if !visibility::visible(node.bounds, projection) {
            return;
        }
        if let Some([left, right]) = node.children {
            self.visit(left, projection, result);
            self.visit(right, projection, result);
        } else {
            result.extend(self.rows[node.range.clone()].iter().map(|row| row.index));
        }
    }
}

fn world_bounds(local: [Vec3; 2], model: Mat4) -> Option<[Vec3; 2]> {
    if model.x_axis.w != 0. || model.y_axis.w != 0. || model.z_axis.w != 0. || model.w_axis.w != 1.
    {
        return None;
    }
    let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
    for corner in shadows::corners(local) {
        let p = model.transform_point3(corner);
        bounds[0] = bounds[0].min(p);
        bounds[1] = bounds[1].max(p);
    }
    let arithmetic = model
        .to_cols_array()
        .into_iter()
        .map(f32::abs)
        .fold(0., f32::max)
        * local[0].abs().max(local[1].abs()).max_element().max(1.)
        * 4.;
    let margin = (arithmetic + bounds[0].abs().max(bounds[1].abs()).max_element() + 1.) * 1e-4;
    bounds[0] -= Vec3::splat(margin);
    bounds[1] += Vec3::splat(margin);
    bounds.iter().all(|p| p.is_finite()).then_some(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn broad_phase_keeps_every_original_frustum_acceptance() {
        let local = [Vec3::splat(-0.5), Vec3::splat(0.5)];
        let mut index = CasterIndex {
            rows: Vec::new(),
            nodes: Vec::new(),
            uncertain: Vec::new(),
            sources: vec![],
            row_by_draw: vec![],
        };
        let mut models: Vec<_> = (0..4096)
            .map(|i| {
                let position = Vec3::new(
                    (i % 32) as f32 - 15.5,
                    ((i / 32) % 16) as f32 - 7.5,
                    -((i / 512) as f32 * 4. + 0.05),
                );
                Mat4::from_translation(position)
                    * Mat4::from_rotation_y(i as f32 * 0.37)
                    * Mat4::from_scale(Vec3::new(if i % 2 == 0 { -0.7 } else { 0.7 }, 1.2, 0.4))
            })
            .collect();
        for (i, model) in models.iter().enumerate() {
            index.rows.push(Row {
                index: i,
                bounds: world_bounds(local, *model).unwrap(),
            });
        }
        index.build(0..index.rows.len());
        for row in &mut index.rows {
            if row.index % 11 == 0 {
                models[row.index] *= Mat4::from_translation(Vec3::new(10., -3., 2.));
                row.bounds = world_bounds(local, models[row.index]).unwrap();
            }
        }
        index.refit();
        for angle in [0.1_f32, 10., 45., 89.9] {
            for direction in [Vec3::NEG_Z, Vec3::new(1., 0.2, -1.).normalize()] {
                let projection =
                    glam::camera::rh::proj::directx::perspective(
                        angle.to_radians() * 2.,
                        1.,
                        0.01,
                        20.,
                    ) * glam::camera::rh::view::look_to_mat4(Vec3::ZERO, direction, Vec3::Y);
                let candidates = index.query(projection);
                for (i, model) in models.iter().enumerate() {
                    if visibility::visible(local, projection * *model) {
                        assert!(
                            candidates.binary_search(&i).is_ok(),
                            "{i} {angle} {direction:?}"
                        );
                    }
                }
                assert!(
                    candidates.len() < models.len(),
                    "broad phase must actually prune"
                );
            }
        }
    }
}
