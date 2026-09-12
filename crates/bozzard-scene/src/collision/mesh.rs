//! Cooked, two-sided triangle surfaces. Box movers retain the existing kinematic response.
use super::*;
use crate::bvh::TriangleBvh;
use std::sync::Arc;

/// Immutable geometry/BVH shared by scene snapshots, prefab instances and the ECS.
#[derive(Clone, Debug)]
pub struct TriangleMesh(Arc<MeshData>);
#[derive(Debug)]
struct MeshData {
    triangles: Vec<[[f32; 3]; 3]>,
    tree: TriangleBvh,
}
impl PartialEq for TriangleMesh {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.triangles == other.0.triangles
    }
}
impl Serialize for TriangleMesh {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.0.triangles.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for TriangleMesh {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Triangles;
        impl<'de> serde::de::Visitor<'de> for Triangles {
            type Value = TriangleMesh;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("at most 100000 collision triangles")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut triangles = Vec::new();
                while let Some(triangle) = seq.next_element()? {
                    if triangles.len() == 100_000 {
                        return Err(serde::de::Error::custom(
                            "Mesh Collider exceeds 100000 triangles",
                        ));
                    }
                    triangles.push(triangle);
                }
                TriangleMesh::new(triangles).map_err(serde::de::Error::custom)
            }
        }
        deserializer.deserialize_seq(Triangles)
    }
}
impl TriangleMesh {
    pub fn new(mut triangles: Vec<[[f32; 3]; 3]>) -> Result<Self> {
        ensure!(
            triangles.len() <= 100_000,
            "Mesh Collider limit: 100000 triangles; use a simplified collision mesh or separate surfaces"
        );
        ensure!(
            triangles.iter().flatten().flatten().all(|v| v.is_finite()),
            "non-finite Mesh Collider vertex"
        );
        triangles.retain(|t| {
            let [a, b, c] = t.map(|p| Vec3::from_array(p).as_dvec3());
            (b - a).cross(c - a).length_squared() > 0.0
        });
        ensure!(
            !triangles.is_empty(),
            "Mesh Collider needs nondegenerate triangles"
        );
        let bounds = triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(Vec3::from_array);
                [a.min(b).min(c), a.max(b).max(c)]
            })
            .collect();
        let tree = TriangleBvh::build(bounds, &|| Ok(()))?;
        Ok(Self(Arc::new(MeshData { triangles, tree })))
    }
    pub fn triangles(&self) -> &[[[f32; 3]; 3]] {
        &self.0.triangles
    }
    pub fn bounds(&self) -> [Vec3; 2] {
        self.0.tree.nodes[0].bounds
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshCollider {
    #[serde(default = "crate::default_true")]
    pub enabled: bool,
    /// Cooked local-space triangles. Independent of the renderer and source files.
    pub mesh: TriangleMesh,
}
impl MeshCollider {
    pub fn geometry(&self, matrix: Mat4) -> Result<[Vec3; 8]> {
        let bounds = self.mesh.bounds();
        let corners = std::array::from_fn(|i| {
            matrix.transform_point3(Vec3::new(
                bounds[i & 1].x,
                bounds[(i >> 1) & 1].y,
                bounds[(i >> 2) & 1].z,
            ))
        });
        ensure!(
            matrix.is_finite()
                && matrix.inverse().is_finite()
                && corners.iter().all(|p| p.is_finite()),
            "Mesh Collider world bounds overflow"
        );
        Ok(corners)
    }
}
#[derive(Debug)]
pub struct CollisionMesh {
    pub id: String,
    pub entity: Entity,
    pub mesh: TriangleMesh,
    pub matrix: Mat4,
}
impl CollisionMesh {
    /// Broad phase in local space, narrow phase in f64 world space (including scale/shear).
    fn candidates(&self, a: &CollisionBox, delta: DVec3, mut visit: impl FnMut([DVec3; 3])) {
        let inverse = self.matrix.as_dmat4().inverse();
        let center = inverse.transform_point3(a.center);
        let extent: DVec3 = a
            .edges
            .iter()
            .map(|e| inverse.transform_vector3(*e).abs())
            .sum();
        let end = center + inverse.transform_vector3(delta);
        let lo = center.min(end) - extent;
        let hi = center.max(end) + extent;
        let pad = lo.abs().max(hi.abs()).max(DVec3::ONE) * (8.0 * f64::from(f32::EPSILON));
        self.mesh
            .0
            .tree
            .query([(lo - pad).as_vec3(), (hi + pad).as_vec3()], &mut |i| {
                visit(self.mesh.0.triangles[i as usize].map(|p| {
                    self.matrix
                        .as_dmat4()
                        .transform_point3(Vec3::from_array(p).as_dvec3())
                }));
            });
    }
    pub fn intersects(&self, a: &CollisionBox) -> bool {
        let mut hit = false;
        self.candidates(a, DVec3::ZERO, |t| {
            if triangle_axes(a, t).all(|axis| {
                let (lo, hi) = interval(a, t, axis);
                let pad = (hi - lo).max(1e-6) * 1e-6;
                lo <= pad && hi >= -pad
            }) {
                hit = true;
            }
        });
        hit
    }
    pub(crate) fn penetration(&self, a: &CollisionBox) -> Option<(f64, DVec3)> {
        let mut deepest = None;
        self.candidates(a, DVec3::ZERO, |t| {
            let mut best = (f64::INFINITY, DVec3::ZERO);
            for axis in triangle_axes(a, t) {
                let (lo, hi) = interval(a, t, axis);
                if lo >= -1e-9 || hi <= 1e-9 {
                    return;
                }
                let hit = if hi < -lo { (hi, axis) } else { (-lo, -axis) };
                if hit.0 < best.0 {
                    best = hit;
                }
            }
            if deepest.is_none_or(|(depth, _)| best.0 > depth) {
                deepest = Some(best);
            }
        });
        deepest
    }
    pub(crate) fn sweep(&self, a: &CollisionBox, delta: DVec3) -> Option<(f64, DVec3)> {
        let mut first = None;
        self.candidates(a, delta, |t| {
            let (mut enter, mut exit) = (f64::NEG_INFINITY, f64::INFINITY);
            let mut normal = DVec3::ZERO;
            for axis in triangle_axes(a, t) {
                let (lo, hi) = interval(a, t, axis);
                let speed = delta.dot(axis);
                if speed.abs() <= 1e-14 {
                    if lo >= -1e-9 || hi <= 1e-9 {
                        return;
                    }
                    continue;
                }
                let (a, b) = (lo / speed, hi / speed);
                let near = a.min(b);
                if near > enter {
                    enter = near;
                    normal = if speed > 0.0 { -axis } else { axis };
                }
                exit = exit.min(a.max(b));
                if enter > exit {
                    return;
                }
            }
            if (-1e-9..=1.0).contains(&enter)
                && exit >= 0.0
                && normal.dot(delta) < -1e-14
                && first.is_none_or(|(time, _)| enter.max(0.0) < time)
            {
                first = Some((enter.max(0.0), normal));
            }
        });
        first
    }
}
fn triangle_axes(a: &CollisionBox, t: [DVec3; 3]) -> impl Iterator<Item = DVec3> {
    let e = a.edges;
    let edges = [t[1] - t[0], t[2] - t[1], t[0] - t[2]];
    [
        edges[0].cross(edges[1]),
        e[1].cross(e[2]),
        e[2].cross(e[0]),
        e[0].cross(e[1]),
    ]
    .into_iter()
    .chain(e.into_iter().flat_map(move |u| edges.map(|v| u.cross(v))))
    .filter_map(|axis| {
        let length = axis.length();
        (length > 0.0).then(|| axis / length)
    })
}
/// Interval of box-center displacements that overlap this triangle along an SAT axis.
fn interval(a: &CollisionBox, t: [DVec3; 3], axis: DVec3) -> (f64, f64) {
    let p = t.map(|p| (p - a.center).dot(axis));
    let r = a.edges.iter().map(|e| e.dot(axis).abs()).sum::<f64>();
    (p[0].min(p[1]).min(p[2]) - r, p[0].max(p[1]).max(p[2]) + r)
}
