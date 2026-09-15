//! Immutable, balanced triangle BVH. Built with the asset on its loading worker.
use super::{MeshData, job::Progress};
use anyhow::{Context, Result, ensure};
pub(super) use bozzard_scene::spatial::box_entry;
use glam::Vec3;
use std::time::Instant;

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

use bozzard_scene::bvh::{BvhNode, TriangleBvh};

pub(super) struct MeshIndex {
    tree: TriangleBvh,
    build_ms: f64,
}
impl MeshIndex {
    pub(super) fn build(mesh: &MeshData, progress: &Progress) -> Result<Self> {
        let started = Instant::now();
        ensure!(
            mesh.indices.len().is_multiple_of(3) && mesh.indices.len() <= 3_000_000,
            "invalid picking index count"
        );
        let mut triangles = Vec::with_capacity(mesh.indices.len() / 3);
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
            triangles.push(bounds);
        }
        Ok(Self {
            tree: TriangleBvh::build(triangles, &|| progress.check())?,
            build_ms: started.elapsed().as_secs_f64() * 1000.0,
        })
    }
    pub(super) fn bounds(&self) -> Option<[Vec3; 2]> {
        self.tree.nodes.first().map(|node| node.bounds)
    }
    pub(super) fn stats(&self) -> MeshPickStats {
        MeshPickStats {
            triangles: self.tree.triangle_order.len(),
            nodes: self.tree.nodes.len(),
            bytes: self.tree.nodes.capacity() * std::mem::size_of::<BvhNode>()
                + self.tree.triangle_order.capacity() * std::mem::size_of::<u32>(),
            build_ms: self.build_ms,
        }
    }
    pub(super) fn cast(&self, mesh: &MeshData, origin: Vec3, direction: Vec3) -> Option<MeshHit> {
        self.cast_filtered(mesh, origin, direction, &|_| true)
    }
    pub(super) fn cast_filtered(
        &self,
        mesh: &MeshData,
        origin: Vec3,
        direction: Vec3,
        accept: &impl Fn(u32) -> bool,
    ) -> Option<MeshHit> {
        if self.tree.nodes.is_empty() || !valid_ray(origin, direction) {
            return None;
        }
        self.tree
            .raycast(origin, direction, f32::INFINITY, &mut |triangle| {
                accept(triangle)
                    .then(|| triangle_hit(mesh, triangle, origin, direction))
                    .flatten()
            })
            .map(|(triangle, distance)| MeshHit { distance, triangle })
    }
}

fn valid_ray(origin: Vec3, direction: Vec3) -> bool {
    origin.is_finite() && direction.is_finite() && direction != Vec3::ZERO
}

/// Linear oracle preserves the original editor's triangle test and first-hit tie order.
pub(super) fn cast_linear(mesh: &MeshData, origin: Vec3, direction: Vec3) -> Option<MeshHit> {
    cast_linear_filtered(mesh, origin, direction, &|_| true)
}
pub(super) fn cast_linear_filtered(
    mesh: &MeshData,
    origin: Vec3,
    direction: Vec3,
    accept: &impl Fn(u32) -> bool,
) -> Option<MeshHit> {
    if !valid_ray(origin, direction) {
        return None;
    }
    (0..mesh.indices.len() / 3)
        .filter(|triangle| accept(*triangle as u32))
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
    bozzard_scene::spatial::triangle_hit([a, b, c], o, d)
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
            skin: None,
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
