//! Conservative heightfield navmesh: each walkable cell is a pair of triangles, with clearance
//! eroded by the agent radius. Suitable for ground navigation; separate surfaces model stacked floors.
use super::*;
use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    hash::{Hash, Hasher},
};
pub const MAX_CELLS: usize = 16384;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BakeSettings {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub cell: f32,
    pub radius: f32,
    pub height: f32,
    pub climb: f32,
    pub slope_degrees: f32,
}
impl Default for BakeSettings {
    fn default() -> Self {
        Self {
            min: [-10., -2., -10.],
            max: [10., 4., 10.],
            cell: 0.5,
            radius: 0.25,
            height: 1.8,
            climb: 0.3,
            slope_degrees: 45.,
        }
    }
}
impl BakeSettings {
    pub fn dimensions(&self) -> Result<[usize; 2]> {
        ensure!(
            self.min
                .iter()
                .chain(&self.max)
                .chain([
                    &self.cell,
                    &self.radius,
                    &self.height,
                    &self.climb,
                    &self.slope_degrees
                ])
                .all(|v| v.is_finite()),
            "non-finite navigation bake setting"
        );
        let span = Vec3::from(self.max) - Vec3::from(self.min);
        ensure!(
            span.min_element() > 0.
                && span.max_element() <= 10000.
                && self.cell >= 0.05
                && self.cell <= 100.
                && self.radius >= 0.01
                && self.radius <= 10.
                && self.height >= 0.1
                && self.height <= 100.
                && self.climb >= 0.
                && self.climb < self.height
                && (0.0..=80.).contains(&self.slope_degrees),
            "invalid navigation bounds, resolution or agent dimensions"
        );
        let dims = [
            (span.x / self.cell).ceil() as usize,
            (span.z / self.cell).ceil() as usize,
        ];
        ensure!(
            dims[0] * dims[1] <= MAX_CELLS,
            "navmesh exceeds 16384 cells; increase cell size or reduce bounds"
        );
        Ok(dims)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cell {
    pub y: f32,
    pub normal: [f32; 3],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavData {
    pub settings: BakeSettings,
    pub dimensions: [usize; 2],
    pub cells: Vec<Option<Cell>>,
    pub geometry_signature: u64,
}
impl NavData {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.dimensions == self.settings.dimensions()?
                && self.cells.len() == self.dimensions[0] * self.dimensions[1],
            "navmesh dimensions do not match bake settings"
        );
        for cell in self.cells.iter().flatten() {
            let normal = Vec3::from(cell.normal);
            ensure!(
                cell.y.is_finite()
                    && (self.settings.min[1]..=self.settings.max[1]).contains(&cell.y)
                    && normal.is_finite()
                    && (normal.length_squared() - 1.).abs() < 1e-3
                    && normal.y >= self.settings.slope_degrees.to_radians().cos() - 1e-4,
                "invalid baked navigation cell"
            );
        }
        Ok(())
    }
    pub fn point(&self, index: usize) -> Option<Vec3> {
        let cell = self.cells.get(index)?.as_ref()?;
        Some(Vec3::new(
            self.settings.min[0]
                + (index % self.dimensions[0]) as f32 * self.settings.cell
                + self.settings.cell * 0.5,
            cell.y,
            self.settings.min[2]
                + (index / self.dimensions[0]) as f32 * self.settings.cell
                + self.settings.cell * 0.5,
        ))
    }
    pub fn triangles(&self) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
        self.cells
            .iter()
            .enumerate()
            .filter_map(|(i, cell)| cell.as_ref().map(|c| (i, c)))
            .flat_map(|(i, cell)| {
                let p = self.point(i).unwrap();
                let h = self.settings.cell * 0.5;
                let n = Vec3::from(cell.normal);
                let points = [[-h, -h], [-h, h], [h, h], [h, -h]]
                    .map(|[x, z]| (p + Vec3::new(x, -(n.x * x + n.z * z) / n.y, z)).to_array());
                [
                    [points[0], points[1], points[2]],
                    [points[0], points[2], points[3]],
                ]
            })
    }
    /// A small neighborhood snaps endpoints without teleporting across distant disconnected islands.
    pub fn nearest(&self, position: Vec3) -> Option<usize> {
        if !position.is_finite() {
            return None;
        }
        let x = ((position.x - self.settings.min[0]) / self.settings.cell).floor() as i32;
        let z = ((position.z - self.settings.min[2]) / self.settings.cell).floor() as i32;
        let mut best = None;
        let mut distance = f32::INFINITY;
        for dz in -2..=2 {
            for dx in -2..=2 {
                let (x, z) = (x.saturating_add(dx), z.saturating_add(dz));
                if x < 0
                    || z < 0
                    || x >= self.dimensions[0] as i32
                    || z >= self.dimensions[1] as i32
                {
                    continue;
                }
                let index = z as usize * self.dimensions[0] + x as usize;
                if let Some(p) = self.point(index) {
                    let d = p.distance_squared(position);
                    if d < distance
                        && (p.y - position.y).abs() <= self.settings.height + self.settings.climb
                    {
                        best = Some(index);
                        distance = d;
                    }
                }
            }
        }
        best
    }
    fn neighbors(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let x = index % self.dimensions[0];
        let z = index / self.dimensions[0];
        [
            (x > 0).then(|| index - 1),
            (x + 1 < self.dimensions[0]).then_some(index + 1),
            (z > 0).then(|| index - self.dimensions[0]),
            (z + 1 < self.dimensions[1]).then_some(index + self.dimensions[0]),
        ]
        .into_iter()
        .flatten()
        .filter(move |&next| {
            self.cells[next].as_ref().is_some_and(|c| {
                (c.y - self.cells[index].as_ref().unwrap().y).abs() <= self.settings.climb + 1e-4
            })
        })
    }
    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let start = self.nearest(from)?;
        let end = self.nearest(to)?;
        let end_point = self.point(end)?;
        let mut costs = vec![f32::INFINITY; self.cells.len()];
        let mut previous = vec![usize::MAX; self.cells.len()];
        let mut open = BinaryHeap::new();
        costs[start] = 0.;
        open.push(Candidate {
            cost: 0.,
            index: start,
        });
        let mut expanded = 0;
        while let Some(Candidate { index, cost }) = open.pop() {
            if index == end {
                let mut path = vec![end_point];
                let mut node = end;
                while node != start {
                    node = previous[node];
                    path.push(self.point(node)?);
                }
                path.reverse();
                return Some(path);
            }
            if cost > costs[index] + self.point(index)?.distance(end_point) + 1e-4 {
                continue;
            }
            expanded += 1;
            if expanded > self.cells.len() * 4 {
                return None;
            }
            let p = self.point(index)?;
            for next in self.neighbors(index) {
                let q = self.point(next)?;
                let next_cost = costs[index] + p.distance(q);
                if next_cost < costs[next] {
                    costs[next] = next_cost;
                    previous[next] = index;
                    open.push(Candidate {
                        index: next,
                        cost: next_cost + q.distance(end_point),
                    });
                }
            }
        }
        None
    }
}
#[derive(PartialEq)]
struct Candidate {
    cost: f32,
    index: usize,
}
impl Eq for Candidate {}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.index.cmp(&self.index))
    }
}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn geometry_signature(geometry: &crate::CollisionSnapshot) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for b in &geometry.boxes {
        b.id.hash(&mut h);
        for v in &b.corners {
            for n in v.to_array() {
                n.to_bits().hash(&mut h);
            }
        }
    }
    for m in &geometry.meshes {
        m.id.hash(&mut h);
        for n in m.matrix.to_cols_array() {
            n.to_bits().hash(&mut h);
        }
        for t in m.mesh.triangles() {
            for n in t.iter().flatten() {
                n.to_bits().hash(&mut h);
            }
        }
    }
    h.finish()
}
impl SceneInstance {
    pub fn navigation_geometry(&self, world: &World) -> Result<crate::CollisionSnapshot> {
        let mut geometry = self.query_geometry(world)?;
        let static_entity = |e| {
            world.get::<crate::Gravity>(e).is_none_or(|v| !v.enabled)
                && world.get::<crate::PlayerController>(e).is_none()
                && world.get::<crate::Trigger>(e).is_none()
                && world.get::<NavAgent>(e).is_none()
        };
        geometry.boxes.retain(|b| static_entity(b.entity));
        geometry.meshes.retain(|m| static_entity(m.entity));
        Ok(geometry)
    }
    pub fn bake_navigation(
        &self,
        world: &World,
        settings: &BakeSettings,
        mut progress: impl FnMut(usize, usize) -> Result<()>,
    ) -> Result<NavData> {
        let dimensions = settings.dimensions()?;
        let count = dimensions[0] * dimensions[1];
        let geometry = self.navigation_geometry(world)?;
        let signature = geometry_signature(&geometry);
        let mut cells = Vec::with_capacity(count);
        let mut budget = 100_000_000;
        let slope = settings.slope_degrees.to_radians().cos();
        let half = settings.cell * 0.5 + settings.radius;
        for i in 0..count {
            if i % 64 == 0 {
                progress(i, count)?;
            }
            let center = Vec3::new(
                settings.min[0] + (i % dimensions[0]) as f32 * settings.cell + settings.cell * 0.5,
                settings.max[1],
                settings.min[2] + (i / dimensions[0]) as f32 * settings.cell + settings.cell * 0.5,
            );
            let mut samples = Vec::with_capacity(5);
            for [x, z] in [
                [0., 0.],
                [-half, -half],
                [-half, half],
                [half, -half],
                [half, half],
            ] {
                if let Some(hit) = geometry.raycast_budget(
                    center + Vec3::new(x, 0., z),
                    -Vec3::Y,
                    settings.max[1] - settings.min[1],
                    None,
                    u32::MAX,
                    &mut budget,
                )? && hit.normal.y >= slope
                {
                    samples.push(hit);
                }
            }
            let cell = if samples.len() == 5 {
                let min = samples
                    .iter()
                    .map(|h| h.position.y)
                    .fold(f32::INFINITY, f32::min);
                let max = samples
                    .iter()
                    .map(|h| h.position.y)
                    .fold(f32::NEG_INFINITY, f32::max);
                let slope_rise =
                    half * 2f32.sqrt() * 2. * settings.slope_degrees.to_radians().tan();
                if max - min <= settings.climb.max(slope_rise) + 1e-4
                    && geometry
                        .overlap_box_budget(
                            Vec3::new(center.x, max + 0.02 + settings.height * 0.5, center.z),
                            Vec3::new(half * 2., settings.height, half * 2.),
                            None,
                            u32::MAX,
                            geometry.boxes.len() + geometry.meshes.len(),
                            &mut budget,
                        )?
                        .is_empty()
                {
                    Some(Cell {
                        y: samples[0].position.y,
                        normal: samples[0].normal.to_array(),
                    })
                } else {
                    None
                }
            } else {
                None
            };
            cells.push(cell);
        }
        progress(count, count)?;
        let data = NavData {
            settings: settings.clone(),
            dimensions,
            cells,
            geometry_signature: signature,
        };
        data.validate()?;
        Ok(data)
    }
}
