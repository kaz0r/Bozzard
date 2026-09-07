//! Discrete 3D box overlap queries. No integration, contact solver, or collision response.
use super::*;
use glam::DVec3;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BoxCollider {
    pub center: [f32; 3],
    /// Full local dimensions, independent of the rendered mesh.
    pub size: [f32; 3],
    pub enabled: bool,
}
impl Default for BoxCollider {
    fn default() -> Self {
        Self {
            center: [0.0; 3],
            size: [1.0; 3],
            enabled: true,
        }
    }
}
impl BoxCollider {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.center.iter().all(|v| v.is_finite()),
            "collider center must be finite"
        );
        ensure!(
            self.size.iter().all(|v| v.is_finite() && *v >= 0.0001),
            "collider size must be finite and at least 0.0001 on every axis"
        );
        Ok(())
    }
    pub(super) fn geometry(&self, matrix: Mat4) -> Result<(DVec3, [DVec3; 3], [Vec3; 8])> {
        self.validate()?;
        let matrix = matrix.as_dmat4();
        let center = matrix.transform_point3(DVec3::from_array(self.center.map(f64::from)));
        let edges = std::array::from_fn(|axis| {
            matrix.transform_vector3(
                [DVec3::X, DVec3::Y, DVec3::Z][axis] * f64::from(self.size[axis]) * 0.5,
            )
        });
        let corners = std::array::from_fn(|i| {
            (center
                + edges[0] * if i & 1 == 0 { -1.0 } else { 1.0 }
                + edges[1] * if i & 2 == 0 { -1.0 } else { 1.0 }
                + edges[2] * if i & 4 == 0 { -1.0 } else { 1.0 })
            .as_vec3()
        });
        ensure!(
            corners.iter().all(|v| v.is_finite()),
            "collider world bounds overflow"
        );
        Ok((center, edges, corners))
    }
}

#[derive(Debug)]
pub struct CollisionBox {
    pub id: String,
    pub entity: Entity,
    /// Bit-indexed corners: X=bit0, Y=bit1, Z=bit2. Bit set selects the positive extent.
    pub corners: [Vec3; 8],
    center: DVec3,
    edges: [DVec3; 3],
}
impl CollisionBox {
    /// SAT for transformed boxes, including shear from rotated/nonuniformly scaled parents.
    /// Touching counts as overlap; a relative 1e-6 tolerance handles transform roundoff.
    pub fn intersects(&self, other: &Self) -> bool {
        let a = self.edges;
        let b = other.edges;
        let faces = |e: [DVec3; 3]| [e[1].cross(e[2]), e[2].cross(e[0]), e[0].cross(e[1])];
        let mut axes = Vec::with_capacity(15);
        axes.extend(faces(a));
        axes.extend(faces(b));
        for u in a {
            for v in b {
                axes.push(u.cross(v));
            }
        }
        for axis in axes {
            let length = axis.length();
            if length == 0.0 {
                continue;
            }
            let axis = axis / length;
            let radius = a.iter().chain(&b).map(|e| e.dot(axis).abs()).sum::<f64>();
            let distance = (other.center - self.center).dot(axis).abs();
            if distance > radius + radius.max(1e-6) * 1e-6 {
                return false;
            }
        }
        true
    }
}
#[derive(Debug, Default)]
pub struct CollisionSnapshot {
    /// Enabled colliders in stable object-ID order.
    pub boxes: Vec<CollisionBox>,
    /// Unique, sorted object-ID pairs. Re-query after changing runtime transforms/components.
    pub overlaps: Vec<(String, String)>,
}
impl SceneInstance {
    pub fn collisions(&self, world: &World) -> Result<CollisionSnapshot> {
        let matrices = self.global_transforms(world)?;
        let mut snapshot = CollisionSnapshot::default();
        for (id, &entity) in &self.entities {
            if let Some(collider) = world.get::<BoxCollider>(entity) {
                collider.validate()?;
                if !collider.enabled {
                    continue;
                }
                let (center, edges, corners) = collider.geometry(matrices[id])?;
                snapshot.boxes.push(CollisionBox {
                    id: id.clone(),
                    entity,
                    center,
                    edges,
                    corners,
                });
            }
        }
        // Deliberately simple all-pairs broad phase for this first detection milestone.
        for (i, a) in snapshot.boxes.iter().enumerate() {
            for b in &snapshot.boxes[i + 1..] {
                if a.intersects(b) {
                    snapshot.overlaps.push((a.id.clone(), b.id.clone()));
                }
            }
        }
        Ok(snapshot)
    }
}
