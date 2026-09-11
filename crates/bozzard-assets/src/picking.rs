//! Immutable, balanced triangle BVH. Built with the asset on its loading worker.
use super::{MeshData, job::Progress};
use anyhow::{Context, Result, ensure};
use glam::Vec3;
use std::time::Instant;

const LEAF_TRIANGLES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshHit {
    /// Ray parameter: direction need not be unit length (preserves transformed-ray distance).
    pub distance: f32,
    /// Original triangle index. The source vertex/index buffers are never reordered.
    pub triangle: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct MeshPickStats {
    pub triangles: usize,
    pub nodes: usize,
    /// Resident node/order arrays; excludes temporary build scratch and source geometry.
    pub bytes: usize,
    pub build_ms: f64,
}

#[derive(Clone, Copy, Default)]
struct Node {
    bounds: [Vec3; 2],
    // Leaf: range in triangle_order. Branch (count == 0): adjacent child nodes.
    first: u32,
    count: u32,
}
struct Primitive {
    bounds: [Vec3; 2],
    center: Vec3,
    triangle: u32,
}

pub(super) struct MeshIndex {
    nodes: Vec<Node>,
    triangle_order: Vec<u32>,
    build_ms: f64,
}
impl MeshIndex {
    pub(super) fn build(mesh: &MeshData, progress: &Progress) -> Result<Self> {
        let started = Instant::now();
        ensure!(
            mesh.indices.len().is_multiple_of(3) && mesh.indices.len() <= 3_000_000,
            "invalid picking index count"
        );
        let mut primitives = Vec::with_capacity(mesh.indices.len() / 3);
        for (triangle, indices) in mesh.indices.chunks_exact(3).enumerate() {
            if triangle.is_multiple_of(1024) {
                progress.check()?;
            }
            let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
            for index in indices {
                let vertex = mesh
                    .vertices
                    .get(*index as usize)
                    .context("picking index exceeds vertex count")?;
                let p = Vec3::from_slice(&vertex[..3]);
                ensure!(p.is_finite(), "non-finite picking vertex");
                bounds = [bounds[0].min(p), bounds[1].max(p)];
            }
            primitives.push(Primitive {
                bounds,
                center: bounds[0] * 0.5 + bounds[1] * 0.5,
                triangle: triangle as u32,
            });
        }
        let mut index = Self {
            nodes: Vec::new(),
            triangle_order: Vec::new(),
            build_ms: 0.0,
        };
        if !primitives.is_empty() {
            index.nodes.push(Node::default());
            index.split(0, 0, &mut primitives, progress)?;
            index.triangle_order = primitives.iter().map(|p| p.triangle).collect();
        }
        progress.check()?;
        index.nodes.shrink_to_fit();
        index.build_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok(index)
    }
    fn split(
        &mut self,
        node: usize,
        start: usize,
        primitives: &mut [Primitive],
        progress: &Progress,
    ) -> Result<()> {
        // Median splits bound recursion by log2(import limit), including coincident triangles.
        progress.check()?;
        let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
        let mut centers = bounds;
        for p in primitives.iter() {
            bounds = [bounds[0].min(p.bounds[0]), bounds[1].max(p.bounds[1])];
            centers = [centers[0].min(p.center), centers[1].max(p.center)];
        }
        if primitives.len() <= LEAF_TRIANGLES {
            self.nodes[node] = Node {
                bounds,
                first: start as u32,
                count: primitives.len() as u32,
            };
            return Ok(());
        }
        let extent = centers[1] - centers[0];
        let axis = (0..3)
            .max_by(|&a, &b| extent[a].total_cmp(&extent[b]))
            .unwrap();
        let mid = primitives.len() / 2;
        primitives.select_nth_unstable_by(mid, |a, b| {
            a.center[axis]
                .total_cmp(&b.center[axis])
                .then(a.triangle.cmp(&b.triangle))
        });
        let child = self.nodes.len();
        self.nodes.extend([Node::default(); 2]);
        self.nodes[node] = Node {
            bounds,
            first: child as u32,
            count: 0,
        };
        let (left, right) = primitives.split_at_mut(mid);
        self.split(child, start, left, progress)?;
        self.split(child + 1, start + mid, right, progress)
    }
    pub(super) fn stats(&self) -> MeshPickStats {
        MeshPickStats {
            triangles: self.triangle_order.len(),
            nodes: self.nodes.len(),
            bytes: self.nodes.capacity() * std::mem::size_of::<Node>()
                + self.triangle_order.capacity() * std::mem::size_of::<u32>(),
            build_ms: self.build_ms,
        }
    }
    pub(super) fn cast(&self, mesh: &MeshData, origin: Vec3, direction: Vec3) -> Option<MeshHit> {
        if self.nodes.is_empty() || !valid_ray(origin, direction) {
            return None;
        }
        let mut best = None;
        self.visit(0, mesh, origin, direction, &mut best);
        best
    }
    fn visit(
        &self,
        node: usize,
        mesh: &MeshData,
        origin: Vec3,
        direction: Vec3,
        best: &mut Option<MeshHit>,
    ) {
        let node = self.nodes[node];
        let limit = best.map_or(f32::INFINITY, |hit| hit.distance);
        if box_entry(node.bounds, origin, direction, limit).is_none() {
            return;
        }
        if node.count > 0 {
            for triangle in
                &self.triangle_order[node.first as usize..(node.first + node.count) as usize]
            {
                if let Some(distance) = triangle_hit(mesh, *triangle, origin, direction) {
                    let hit = MeshHit {
                        distance,
                        triangle: *triangle,
                    };
                    if best.is_none_or(|old| {
                        distance < old.distance
                            || (distance == old.distance && hit.triangle < old.triangle)
                    }) {
                        *best = Some(hit);
                    }
                }
            }
        } else {
            let left = node.first as usize;
            let right = left + 1;
            let a = box_entry(self.nodes[left].bounds, origin, direction, limit);
            let b = box_entry(self.nodes[right].bounds, origin, direction, limit);
            // Visit the nearer box first, then prune against the actual nearest triangle.
            match (a, b) {
                (Some(a), Some(b)) => {
                    let order = if a <= b { [left, right] } else { [right, left] };
                    for child in order {
                        self.visit(child, mesh, origin, direction, best);
                    }
                }
                (Some(_), None) => self.visit(left, mesh, origin, direction, best),
                (None, Some(_)) => self.visit(right, mesh, origin, direction, best),
                (None, None) => {}
            }
        }
    }
}

fn valid_ray(origin: Vec3, direction: Vec3) -> bool {
    origin.is_finite() && direction.is_finite() && direction != Vec3::ZERO
}
fn box_entry(bounds: [Vec3; 2], origin: Vec3, direction: Vec3, limit: f32) -> Option<f64> {
    let mut near = 0.0_f64;
    let mut far = f64::from(limit) * (1.0 + 8.0 * f64::from(f32::EPSILON)) + 1e-6;
    for axis in 0..3 {
        // Conservative padding includes f32 triangle/transform rounding near box edges.
        // f64 slab arithmetic avoids overflow and handles tiny nonzero directions.
        let scale = bounds[0][axis]
            .abs()
            .max(bounds[1][axis].abs())
            .max(origin[axis].abs())
            .max(1.0);
        let pad = f64::from(scale) * 8.0 * f64::from(f32::EPSILON);
        let min = f64::from(bounds[0][axis]) - pad;
        let max = f64::from(bounds[1][axis]) + pad;
        let o = f64::from(origin[axis]);
        let d = f64::from(direction[axis]);
        if d == 0.0 {
            if o < min || o > max {
                return None;
            }
        } else {
            let a = (min - o) / d;
            let b = (max - o) / d;
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    (far >= near && far > 0.0).then_some(near)
}

/// Linear oracle preserves the original editor's triangle test and first-hit tie order.
pub(super) fn cast_linear(mesh: &MeshData, origin: Vec3, direction: Vec3) -> Option<MeshHit> {
    if !valid_ray(origin, direction) {
        return None;
    }
    (0..mesh.indices.len() / 3)
        .filter_map(|triangle| {
            triangle_hit(mesh, triangle as u32, origin, direction).map(|distance| MeshHit {
                distance,
                triangle: triangle as u32,
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}
fn triangle_hit(mesh: &MeshData, triangle: u32, o: Vec3, d: Vec3) -> Option<f32> {
    let first = triangle as usize * 3;
    let [a, b, c] = std::array::from_fn(|i| {
        Vec3::from_slice(&mesh.vertices[mesh.indices[first + i] as usize][..3])
    });
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let t = o - a;
    let u = t.dot(p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(e1);
    let v = d.dot(q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = e2.dot(q) / det;
    (distance > 0.0).then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(triangles: impl IntoIterator<Item = [Vec3; 3]>) -> MeshData {
        let vertices: Vec<_> = triangles
            .into_iter()
            .flatten()
            .map(|p| [p.x, p.y, p.z, 0., 0., 1., 0., 0.])
            .collect();
        MeshData {
            indices: (0..vertices.len() as u32).collect(),
            vertices,
            parts: Vec::new(),
            warnings: Vec::new(),
        }
    }
    fn compare(mesh: &MeshData, rays: impl IntoIterator<Item = (Vec3, Vec3)>) {
        let indices = mesh.indices.clone();
        let vertices = mesh.vertices.clone();
        let index = MeshIndex::build(mesh, &Progress::default()).unwrap();
        for (o, d) in rays {
            assert_eq!(
                index.cast(mesh, o, d),
                cast_linear(mesh, o, d),
                "o={o:?} d={d:?}"
            );
        }
        assert_eq!(mesh.indices, indices);
        assert_eq!(mesh.vertices, vertices);
    }

    #[test]
    fn bvh_matches_linear_hits_misses_edges_and_nonunit_rays() {
        let triangles: Vec<_> = (0..7)
            .flat_map(|z| {
                (0..11).flat_map(move |y| {
                    (0..13).map(move |x| {
                        let p = Vec3::new(x as f32 - 6., y as f32 - 5., z as f32 * 0.4);
                        [p, p + Vec3::new(0.8, 0., 0.), p + Vec3::new(0., 0.8, 0.)]
                    })
                })
            })
            .collect();
        let mesh = mesh(triangles);
        let mut rays = Vec::new();
        for y in -20..=20 {
            for x in -24..=24 {
                let o = Vec3::new(x as f32 * 0.3, y as f32 * 0.3, 4.);
                rays.push((o, Vec3::new(0., 0., -0.25)));
                rays.push((o, Vec3::new(0.2, -0.13, -1.)));
                rays.push((o.with_z(1.3), Vec3::Z * 2.));
            }
        }
        rays.extend([
            (Vec3::new(0., 0., 3.), -Vec3::Z),   // vertex / shared depth layers
            (Vec3::new(0.4, 0.4, 3.), -Vec3::Z), // edge
            (Vec3::new(0., 0., 3.), Vec3::Z),    // behind the ray
            (Vec3::ZERO, Vec3::X),               // coplanar
            (Vec3::new(0., 0., 3.), Vec3::new(1e-12, 0., -1.)),
        ]);
        compare(&mesh, rays);
    }

    #[test]
    fn coincident_degenerate_scaled_geometry_and_invalid_rays() {
        for scale in [0.001, 1., 1_000_000.] {
            let mesh = mesh((0..65).map(|i| {
                if i == 0 {
                    [Vec3::ZERO; 3]
                } else {
                    [
                        Vec3::new(-1., -1., 0.),
                        Vec3::new(1., -1., 0.),
                        Vec3::new(0., 1., 0.),
                    ]
                    .map(|p| p * scale)
                }
            }));
            let index = MeshIndex::build(&mesh, &Progress::default()).unwrap();
            let o = Vec3::Z * scale;
            assert_eq!(
                index.cast(&mesh, o, -Vec3::Z).unwrap().triangle,
                1,
                "equal-distance ties retain the first source triangle"
            );
            compare(
                &mesh,
                [
                    (o, -Vec3::Z),
                    (o, Vec3::new(-0.25, 0.125, -1.)),
                    (Vec3::new(scale, -scale, scale), -Vec3::Z),
                    (Vec3::ZERO, Vec3::ZERO),
                    (Vec3::splat(f32::NAN), Vec3::Z),
                    (Vec3::ZERO, Vec3::splat(f32::INFINITY)),
                ],
            );
        }
        compare(&mesh([]), [(Vec3::ZERO, Vec3::Z)]);
        let mut invalid = mesh([[Vec3::ZERO; 3]]);
        invalid.indices[0] = 999;
        assert!(MeshIndex::build(&invalid, &Progress::default()).is_err());
    }

    #[test]
    fn cancelled_build_is_not_published() {
        let (started, start) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let job = crate::job::Job::start("Index", move |progress| {
            started.send(()).unwrap();
            gate.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            MeshIndex::build(&mesh([[Vec3::ZERO; 3]]), &progress)
        })
        .unwrap();
        start
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        job.cancel();
        release.send(()).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = job.poll() {
                assert!(result.is_err());
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
