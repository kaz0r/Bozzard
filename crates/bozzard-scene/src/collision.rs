//! Box overlap queries and swept single-box translation against static colliders.
use super::*;
use glam::DVec3;
mod broad_phase;
mod mesh;
mod queries;
mod response;
pub use mesh::{CollisionMesh, MeshCollider, TriangleMesh};
pub use queries::{Contact, QueryHit};
pub use response::MoveResult;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BoxCollider {
    pub center: [f32; 3],
    /// Full local dimensions, independent of the rendered mesh.
    pub size: [f32; 3],
    pub enabled: bool,
    /// Layers this collider belongs to. Bit 0 is the default layer; see [`LAYER_NAMES`].
    pub layers: u32,
    /// Layers this collider interacts with. Two colliders meet only when each one's `layers`
    /// intersects the other's `mask`, so an exclusion on either side is enough.
    pub mask: u32,
}
impl Default for BoxCollider {
    fn default() -> Self {
        Self {
            center: [0.0; 3],
            size: [1.0; 3],
            enabled: true,
            layers: DEFAULT_LAYERS,
            mask: DEFAULT_MASK,
        }
    }
}

/// The authored layer names. Bits without a name stay reserved and are preserved on save.
pub const LAYER_NAMES: &[&str] = &[
    "Default",
    "Player",
    "Environment",
    "Gameplay",
    "Projectile",
    "Character",
    "Sensor",
    "Reserved 7",
];
/// A new collider belongs to the default layer.
pub const DEFAULT_LAYERS: u32 = 1;
/// A new collider interacts with every layer, so existing scenes keep their behaviour.
pub const DEFAULT_MASK: u32 = u32::MAX;

/// Collision filtering, shared by Rapier (`InteractionGroups`) and the CPU sweeps and queries.
/// Two colliders meet only when each one's membership intersects the other's filter.
pub fn layers_interact(a_layers: u32, a_mask: u32, b_layers: u32, b_mask: u32) -> bool {
    a_layers & b_mask != 0 && b_layers & a_mask != 0
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

#[derive(Clone, Debug)]
pub struct CollisionBox {
    pub id: String,
    pub entity: Entity,
    /// Bit-indexed corners: X=bit0, Y=bit1, Z=bit2. Bit set selects the positive extent.
    pub corners: [Vec3; 8],
    /// Collision filtering, copied from the authored collider. A query shape uses all bits.
    pub layers: u32,
    pub mask: u32,
    pub(super) center: DVec3,
    pub(super) edges: [DVec3; 3],
}
impl CollisionBox {
    pub(super) fn penetrates(&self, other: &Self) -> bool {
        response::penetration(self, other).is_some_and(|(depth, _)| depth > 1e-5)
    }

    /// SAT for transformed boxes, including shear from rotated/nonuniformly scaled parents.
    /// Touching counts as overlap; a relative 1e-6 tolerance handles transform roundoff.
    pub fn intersects(&self, other: &Self) -> bool {
        let a = self.edges;
        let b = other.edges;
        let faces = |e: [DVec3; 3]| [e[1].cross(e[2]), e[2].cross(e[0]), e[0].cross(e[1])];
        // Evaluate separating axes lazily. Distant pairs usually stop on a face
        // normal, without allocating or computing the nine edge cross products.
        let axes = faces(a).into_iter().chain(faces(b)).chain(
            a.into_iter()
                .flat_map(|u| b.into_iter().map(move |v| u.cross(v))),
        );
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
#[derive(Clone, Debug, Default)]
pub struct CollisionSnapshot {
    /// Enabled colliders in stable object-ID order.
    pub boxes: Vec<CollisionBox>,
    /// Static triangle surfaces; mesh/mesh overlap is not queried.
    pub meshes: Vec<CollisionMesh>,
    /// Unique, sorted object-ID pairs. Re-query after changing runtime transforms/components.
    pub overlaps: Vec<(String, String)>,
}
impl SceneInstance {
    pub(crate) fn has_collision_geometry(&self, world: &World) -> bool {
        !self.component_entities::<BoxCollider>(world).is_empty()
            || !self.component_entities::<MeshCollider>(world).is_empty()
            || self
                .component_entities::<crate::middleware::sprite::Tilemap>(world)
                .into_iter()
                .any(|(_, &e)| {
                    world
                        .get::<crate::middleware::sprite::Tilemap>(e)
                        .is_some_and(|m| m.enabled && !m.solid.is_empty())
                })
    }
    // Movement needs validated geometry, but has no use for all scene overlaps.
    pub(crate) fn collision_geometry(
        &self,
        world: &World,
    ) -> Result<(CollisionSnapshot, BTreeMap<String, Mat4>)> {
        let matrices = self.global_transforms(world)?;
        let mut snapshot = CollisionSnapshot::default();
        for (id, &entity) in &self.entities {
            if let Some(map) = world
                .get::<crate::middleware::sprite::Tilemap>(entity)
                .filter(|m| m.enabled && !m.solid.is_empty())
            {
                for collider in crate::middleware::sprite::collision_boxes(world, id, map).iter() {
                    let (center, edges, corners) = collider.geometry(matrices[id])?;
                    snapshot.boxes.push(CollisionBox {
                        id: id.clone(),
                        entity,
                        center,
                        edges,
                        corners,
                        layers: collider.layers,
                        mask: collider.mask,
                    });
                }
            }
            if let Some(collider) = world.get::<MeshCollider>(entity).filter(|c| c.enabled) {
                ensure!(
                    world.get::<BoxCollider>(entity).is_none(),
                    "Mesh Collider cannot also be a Box Collider"
                );
                collider.geometry(matrices[id])?;
                snapshot.meshes.push(CollisionMesh {
                    id: id.clone(),
                    entity,
                    mesh: if world.get::<Gravity>(entity).is_some_and(|g| g.enabled) {
                        collider.mesh.convex_hull()?
                    } else {
                        collider.mesh.clone()
                    },
                    matrix: matrices[id],
                    solid: world.get::<Gravity>(entity).is_some_and(|g| g.enabled),
                    layers: collider.layers,
                    mask: collider.mask,
                });
            }
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
                    layers: collider.layers,
                    mask: collider.mask,
                });
            }
        }
        Ok((snapshot, matrices))
    }

    pub fn collisions(&self, world: &World) -> Result<CollisionSnapshot> {
        if !self.has_collision_geometry(world) {
            return Ok(CollisionSnapshot::default());
        }
        Ok(self.collision_snapshot(world)?.0)
    }
    pub(crate) fn collision_snapshot(
        &self,
        world: &World,
    ) -> Result<(CollisionSnapshot, BTreeMap<String, Mat4>)> {
        let (mut snapshot, matrices) = self.collision_geometry(world)?;
        broad_phase::overlaps(&snapshot.boxes, &mut snapshot.overlaps);
        // Layers gate overlap reporting exactly as they gate the solver, so a gameplay volume on
        // its own layer stops firing On Overlap/On Collision events against everything.
        let layers: BTreeMap<_, _> = snapshot
            .boxes
            .iter()
            .map(|b| (b.id.clone(), (b.layers, b.mask)))
            .chain(
                snapshot
                    .meshes
                    .iter()
                    .map(|m| (m.id.clone(), (m.layers, m.mask))),
            )
            .collect();
        let interact = |a: &str, b: &str| {
            layers.get(a).zip(layers.get(b)).is_some_and(
                |(&(a_layers, a_mask), &(b_layers, b_mask))| {
                    layers_interact(a_layers, a_mask, b_layers, b_mask)
                },
            )
        };
        snapshot.overlaps.retain(|(a, b)| interact(a, b));
        let extra: Vec<_> = snapshot
            .boxes
            .iter()
            .flat_map(|a| {
                snapshot.meshes.iter().filter_map(move |b| {
                    if interact(&a.id, &b.id) && b.intersects(a) {
                        Some(if a.id < b.id {
                            (a.id.clone(), b.id.clone())
                        } else {
                            (b.id.clone(), a.id.clone())
                        })
                    } else {
                        None
                    }
                })
            })
            .collect();
        snapshot.overlaps.extend(extra);
        if let Some(physics) = world.resource::<crate::physics::Physics>() {
            snapshot.overlaps.extend(physics.contacts(world, &matrices));
        }
        snapshot.overlaps.sort();
        snapshot.overlaps.dedup();
        Ok((snapshot, matrices))
    }
}
