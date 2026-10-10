//! Rapier owns non-player body velocities, contacts, inertia and sleeping.
//! ECS transforms remain the public pose: external edits are teleports on the next tick.
//!
//! A body is an object with a `Gravity` component (a Rigidbody) plus every colliding descendant
//! that does not start a Rigidbody of its own. Those descendants are compound shapes: contact
//! events still name the object that owns the shape, but the solver moves one body.
use super::*;
use crate::middleware::sprite::{Tilemap, collision_boxes};
use glam::Mat3;
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::math::Pose;
use rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, FixedJointBuilder, GenericJoint, Group, ImpulseJointHandle,
    InteractionGroups, JointAxis, PhysicsWorld, PrismaticJointBuilder, QueryFilter,
    RevoluteJointBuilder, RigidBodyBuilder, RigidBodyHandle, RopeJointBuilder, SharedShape,
    SphericalJointBuilder,
};

/// One collider of a body: the Rigidbody itself or one of its compound children.
#[derive(Clone, PartialEq)]
struct PartKey {
    /// Object that owns this shape. Contact events report this ID, not the body root.
    object: String,
    entity: Entity,
    /// Shape scale, without rotation: the offset pose carries the rotation.
    linear: Mat3,
    collider: Option<BoxCollider>,
    mesh: Option<TriangleMesh>,
    /// A tilemap's merged solid rectangles, as one compound shape.
    tiles: Option<Tilemap>,
    /// A Player Controller's capsule, as (half cylinder height, radius), already scaled.
    capsule: Option<(f32, f32)>,
    dynamic: bool,
    /// Rapier interaction groups, from the authored `layers` and `mask`.
    groups: (u32, u32),
    /// Shape pose relative to the rigid body.
    offset: Pose,
}
impl PartKey {
    /// Whether the object that owns this shape still offers it. The step prunes and rebuilds
    /// bodies from the live ECS; this guards callers that read contacts before the next step.
    fn is_live(&self, world: &World) -> bool {
        if self.capsule.is_some() {
            return world.get::<PlayerController>(self.entity).is_some();
        }
        if self.tiles.is_some() {
            return world
                .get::<Tilemap>(self.entity)
                .is_some_and(|map| map.enabled && !map.solid.is_empty());
        }
        world
            .get::<BoxCollider>(self.entity)
            .is_some_and(|c| c.enabled)
            || world
                .get::<MeshCollider>(self.entity)
                .is_some_and(|c| c.enabled)
    }
    fn cook(&self) -> Result<SharedShape> {
        if let Some(tiles) = &self.tiles {
            let shapes = tiles
                .solid_boxes()
                .iter()
                .map(|collider| -> Result<_> {
                    let (_, _, corners) = collider.geometry(Mat4::from_mat3(self.linear))?;
                    Ok((
                        Pose::IDENTITY,
                        SharedShape::convex_hull(&corners).context("invalid tile collider hull")?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            ensure!(!shapes.is_empty(), "solid tilemap has no shapes");
            return Ok(SharedShape::compound(shapes));
        }
        if let Some((half_height, radius)) = self.capsule {
            ensure!(
                half_height.is_finite() && radius.is_finite() && radius > 0.0,
                "invalid character capsule on '{}'",
                self.object
            );
            return Ok(SharedShape::capsule_y(half_height, radius));
        }
        if let Some(collider) = self.collider {
            let (_, _, corners) = collider.geometry(Mat4::from_mat3(self.linear))?;
            return SharedShape::convex_hull(&corners)
                .with_context(|| format!("invalid box hull on '{}'", self.object));
        }
        let mesh = self
            .mesh
            .as_ref()
            .context("Rigidbody needs an enabled collider")?;
        let points: Vec<_> = mesh
            .triangles()
            .iter()
            .flatten()
            .map(|p| self.linear * Vec3::from_array(*p))
            .collect();
        ensure!(
            points.iter().all(|p| p.is_finite()),
            "physics mesh transform overflow"
        );
        if self.dynamic {
            SharedShape::convex_hull(&points)
                .with_context(|| format!("invalid dynamic mesh hull on '{}'", self.object))
        } else {
            let indices = (0..points.len() as u32)
                .step_by(3)
                .map(|i| [i, i + 1, i + 2])
                .collect();
            Ok(SharedShape::trimesh(points, indices)?)
        }
    }
}
struct Body {
    /// Entity of the Rigidbody root.
    entity: Entity,
    handle: RigidBodyHandle,
    /// Collider handles, parallel to `keys`.
    handles: Vec<ColliderHandle>,
    keys: Vec<PartKey>,
    dynamic: bool,
    /// A Player Controller: moved by the Rapier character controller, not the solver.
    player: bool,
    config: Gravity,
    pose: Mat4,
    spin: Option<Spin>,
    /// Local transform and parent pose of the root and each part when `keys` were built.
    inputs: Vec<(Transform, Mat4)>,
    /// Body rotation those inputs produced.
    orientation: Quat,
}
/// The component revisions and hierarchy that decide which objects form which bodies.
/// Physics lives in the world while hierarchy revisions count per scene instance, so the
/// instance is part of the key too.
#[derive(Clone, Copy, PartialEq)]
struct LayoutKey {
    instance: u64,
    hierarchy: u64,
    components: [bozzard_ecs::ComponentRevision; 5],
}
impl LayoutKey {
    fn of(instance: &SceneInstance, world: &World) -> Self {
        Self {
            instance: instance.instance_id,
            hierarchy: instance.hierarchy_revision,
            components: [
                world.component_revision::<BoxCollider>(),
                world.component_revision::<MeshCollider>(),
                world.component_revision::<Tilemap>(),
                world.component_revision::<PlayerController>(),
                world.component_revision::<Gravity>(),
            ],
        }
    }
}
/// Colliding objects grouped under their Rigidbodies. While its key holds, a body's
/// shapes change only with the transforms its `inputs` name.
struct Layout {
    key: LayoutKey,
    roots: BTreeMap<String, RootLayout>,
    /// Gravity owners that are disabled or own no shape, in ID order.
    resets: Vec<Entity>,
}
#[derive(Default)]
struct RootLayout {
    parts: Vec<String>,
    /// The root and then each part: its entity and its parent's document index.
    inputs: Vec<(Entity, Option<usize>)>,
}
impl Layout {
    fn new(
        instance: &SceneInstance,
        world: &World,
        objects: &BTreeMap<&str, &Object>,
        key: LayoutKey,
    ) -> Result<Self> {
        // Group every colliding object under the Rigidbody that owns it.
        let mut roots: BTreeMap<String, RootLayout> = BTreeMap::new();
        for (id, &entity) in &instance.entities {
            if let Some(gravity) = world.get::<Gravity>(entity) {
                gravity.validate()?;
            }
            let colliding = world.get::<BoxCollider>(entity).is_some_and(|c| c.enabled)
                || world.get::<MeshCollider>(entity).is_some_and(|c| c.enabled)
                || world
                    .get::<Tilemap>(entity)
                    .is_some_and(|t| t.enabled && !collision_boxes(world, id, t).is_empty());
            if colliding {
                roots
                    .entry(compound_root(objects, id))
                    .or_default()
                    .parts
                    .push(id.clone());
            }
        }
        // A Rigidbody with no colliding shape anywhere is not a body; its runtime state resets.
        let resets = instance
            .entities
            .iter()
            .filter(|&(id, &entity)| {
                world
                    .get::<Gravity>(entity)
                    .is_some_and(|gravity| !gravity.enabled || !roots.contains_key(id))
            })
            .map(|(_, &entity)| entity)
            .collect();
        for (root, layout) in &mut roots {
            layout.inputs = std::iter::once(root)
                .chain(&layout.parts)
                .map(|id| {
                    let parent = objects[id.as_str()]
                        .parent
                        .as_ref()
                        .map(|parent| instance.object_indices[&instance.entities[parent]]);
                    (instance.entities[id], parent)
                })
                .collect();
        }
        Ok(Self { key, roots, resets })
    }
}
fn object_map(instance: &SceneInstance) -> BTreeMap<&str, &Object> {
    instance
        .document
        .objects
        .iter()
        .map(|o| (o.id.as_str(), o))
        .collect()
}
/// Authored joint values plus the resolved bodies, so a change rebuilds exactly that joint.
#[derive(Clone, PartialEq)]
struct JointKey {
    owner: String,
    other: String,
    kind: JointKind,
    anchor: [f32; 3],
    other_anchor: [f32; 3],
    axis: [f32; 3],
    other_axis: [f32; 3],
    limits: bool,
    min_limit: f32,
    max_limit: f32,
}
struct JointRecord {
    handle: ImpulseJointHandle,
    key: JointKey,
}
#[derive(Default)]
pub(crate) struct Physics {
    simulation: PhysicsWorld,
    bodies: BTreeMap<String, Body>,
    /// Joint records keyed by the object that authors them.
    joints: BTreeMap<String, JointRecord>,
    /// Each player's last ground collider and its world position, for moving-platform rides.
    player_ground: BTreeMap<String, (RigidBodyHandle, Vec3)>,
    // Spawned bodies are created on the next physics step; remember their launch velocity until then.
    pending: BTreeMap<String, Vec3>,
    restored: BTreeMap<String, BodySave>,
    layout: Option<Layout>,
    /// Bodies whose shapes a step reused without rebuilding their part keys.
    #[cfg(test)]
    reused_bodies: usize,
}

/// A rotating child can only keep its authored scale under a rigid/uniformly scaled parent.
pub(crate) fn parent_pose(matrix: Mat4) -> Result<(Quat, f32)> {
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    let reconstructed = Mat4::from_scale_rotation_translation(scale, rotation, translation);
    ensure!(
        scale.min_element() > 0.0
            && (scale - Vec3::splat(scale.x)).abs().max_element() <= scale.x * 1e-5
            && matrix
                .to_cols_array()
                .iter()
                .zip(reconstructed.to_cols_array())
                .all(|(a, b)| (a - b).abs() <= 1e-5 * a.abs().max(1.0)),
        "Rigidbody needs a non-sheared, positive uniformly scaled parent; move it under a rigid transform parent"
    );
    Ok((rotation, scale.x))
}
/// The world rotation and scale-only shape matrix of one collider object. A dynamic body fails
/// loudly on a sheared parent; a fixed one bakes the shear into the shape, which is the existing
/// escape hatch for authored static geometry.
fn object_frame(
    world: &World,
    objects: &BTreeMap<&str, &Object>,
    matrices: &crate::transforms::Matrices<'_>,
    entities: &BTreeMap<String, Entity>,
    id: &str,
    dynamic: bool,
) -> Result<(Quat, Mat3)> {
    let object = objects[id];
    let parent = object
        .parent
        .as_ref()
        .map(|p| matrices[p])
        .unwrap_or(Mat4::IDENTITY);
    let local = world
        .get::<Transform>(entities[id])
        .context("missing physics transform")?;
    match parent_pose(parent) {
        Ok((orientation, scale)) => Ok((
            orientation * rotation(local),
            Mat3::from_diagonal(Vec3::from_array(local.scale) * scale),
        )),
        Err(error) if dynamic => Err(error.context(format!("Rigidbody '{id}'"))),
        Err(_) => Ok((Quat::IDENTITY, Mat3::from_mat4(matrices[id]))),
    }
}
fn rotation(transform: &Transform) -> Quat {
    let [x, y, z] = transform.rotation_degrees.map(f32::to_radians);
    Quat::from_euler(EulerRot::YXZ, y, x, z)
}
/// Engine convention: an object faces its local -Z axis.
pub(crate) fn forward(transform: &Transform) -> Vec3 {
    rotation(transform) * Vec3::NEG_Z
}

/// The Rigidbody that owns an object's collider: the object itself or its nearest ancestor that
/// has a `Gravity` component and is not a Player Controller. Without one, the object is its own body.
fn compound_root(objects: &BTreeMap<&str, &Object>, id: &str) -> String {
    let mut current = Some(id);
    while let Some(c) = current {
        let object = objects[c];
        if object.player_controller.is_none() && object.gravity.is_some() {
            return c.to_owned();
        }
        current = object.parent.as_deref();
    }
    id.to_owned()
}

/// The Rapier constraint for an authored joint. Revolute angles are degrees in the scene file.
/// Jointed bodies do not collide with each other, matching the usual authoring expectation.
fn build_joint(joint: &Joint, frame2: Option<Pose>) -> GenericJoint {
    let mut data: GenericJoint = match joint.kind {
        JointKind::Fixed => FixedJointBuilder::new().build().into(),
        JointKind::Revolute => RevoluteJointBuilder::new(Vec3::Y).build().into(),
        JointKind::Spherical => SphericalJointBuilder::new().build().into(),
        JointKind::Prismatic => PrismaticJointBuilder::new(Vec3::Y).build().into(),
        JointKind::Rope => RopeJointBuilder::new(joint.max_limit.max(0.0))
            .build()
            .into(),
    };
    data.set_contacts_enabled(false);
    data.set_local_anchor1(Vec3::from(joint.anchor));
    data.set_local_anchor2(Vec3::from(joint.other_anchor));
    match joint.kind {
        JointKind::Revolute => {
            data.set_local_axis1(Vec3::from(joint.axis));
            data.set_local_axis2(Vec3::from(joint.other_axis));
            if joint.limits {
                data.set_limits(
                    JointAxis::AngX,
                    [joint.min_limit.to_radians(), joint.max_limit.to_radians()],
                );
            }
        }
        JointKind::Prismatic => {
            data.set_local_axis1(Vec3::from(joint.axis));
            data.set_local_axis2(Vec3::from(joint.other_axis));
            if joint.limits {
                data.set_limits(JointAxis::LinX, [joint.min_limit, joint.max_limit]);
            }
        }
        JointKind::Fixed | JointKind::Spherical | JointKind::Rope => {}
    }
    // Last: the frame carries the pose-preserving translation, so an anchor write must not reset it.
    if let Some(frame) = frame2 {
        data.set_local_frame2(frame);
    }
    data
}

/// Teleport a body whose world pose changed outside the solver and apply a new spin.
fn place(
    simulation: &mut PhysicsWorld,
    entry: &mut Body,
    dynamic: bool,
    global: Mat4,
    pose: Pose,
    spin: Option<Spin>,
) {
    let body = &mut simulation.bodies[entry.handle];
    if dynamic {
        if entry.pose != global {
            body.set_position(pose, true);
        }
        if entry.spin != spin {
            body.set_angvel(
                Vec3::from_array(spin.map_or([0.; 3], |s| s.0)).map(f32::to_radians),
                true,
            );
        }
    } else if entry.pose != global {
        // Static edits and player respawns are teleports, not high-speed kinematic launches.
        body.set_position(pose, true);
    }
    entry.spin = spin;
}

impl Physics {
    /// Collider handle to owning object ID for every live body part still matching its pose.
    fn part_ids<'a>(
        &'a self,
        world: &World,
        matrices: &crate::transforms::Matrices<'_>,
    ) -> std::collections::HashMap<ColliderHandle, &'a str> {
        self.bodies
            .iter()
            .filter(|(id, b)| matrices.get(id) == Some(&b.pose))
            .flat_map(|(_, b)| {
                b.handles
                    .iter()
                    .zip(&b.keys)
                    .filter(|(_, key)| key.is_live(world))
                    .map(|(handle, key)| (*handle, key.object.as_str()))
            })
            .collect()
    }
    pub(crate) fn contacts(
        &self,
        world: &World,
        matrices: &crate::transforms::Matrices<'_>,
    ) -> Vec<(String, String)> {
        let ids = self.part_ids(world, matrices);
        self.simulation
            .narrow_phase
            .contact_pairs()
            .filter(|p| {
                p.has_any_active_contact()
                    || p.manifolds
                        .iter()
                        .any(|m| m.points.iter().any(|p| p.dist <= 0.001))
            })
            .filter_map(|p| {
                let (a, b) = (*ids.get(&p.collider1)?, *ids.get(&p.collider2)?);
                Some(if a < b {
                    (a.to_owned(), b.to_owned())
                } else {
                    (b.to_owned(), a.to_owned())
                })
            })
            .collect()
    }
    pub(crate) fn remove_entity(&mut self, entity: Entity) {
        let mut removed = Vec::new();
        for (id, body) in &mut self.bodies {
            if body.entity == entity {
                removed.push(id.clone());
                continue;
            }
            // A removed compound child drops its shape; the body keeps the rest.
            if let Some(index) = body.keys.iter().position(|key| key.entity == entity) {
                self.simulation.remove_collider(body.handles[index]);
                body.handles.remove(index);
                body.keys.remove(index);
            }
        }
        for id in removed {
            let body = self.bodies.remove(&id).unwrap();
            self.simulation.remove_body(body.handle);
            self.player_ground.remove(&id);
        }
    }
    fn body_of(&self, entity: Entity) -> Option<RigidBodyHandle> {
        self.bodies
            .values()
            .find(|b| b.entity == entity || b.keys.iter().any(|key| key.entity == entity))
            .map(|b| b.handle)
    }
    pub(crate) fn launch(&mut self, entity: Entity, speed: f32) {
        if let Some(handle) = self.body_of(entity) {
            let body = &mut self.simulation.bodies[handle];
            let mut velocity = body.linvel();
            velocity.y = speed;
            body.set_linvel(velocity, true);
        }
    }
    /// Applied immediately to a live body, otherwise queued until that body is created.
    pub(crate) fn set_velocity(&mut self, id: &str, entity: Entity, velocity: Vec3) {
        match self.body_of(entity) {
            Some(handle) => {
                self.pending.remove(id);
                self.simulation.bodies[handle].set_linvel(velocity, true);
            }
            None => {
                self.pending.insert(id.into(), velocity);
            }
        }
    }
    /// Build the compound shape list for one Rigidbody root.
    #[allow(clippy::too_many_arguments)]
    fn part_keys(
        &self,
        world: &World,
        objects: &BTreeMap<&str, &Object>,
        entities: &BTreeMap<String, Entity>,
        matrices: &crate::transforms::Matrices<'_>,
        root: &str,
        parts: &[String],
        dynamic: bool,
    ) -> Result<(Quat, Vec<PartKey>)> {
        let (root_orientation, linear_root) =
            object_frame(world, objects, matrices, entities, root, dynamic)?;
        let origin = matrices[root].w_axis.truncate();
        let mut keys = Vec::new();
        for id in parts {
            let entity = entities[id];
            let player = world.get::<PlayerController>(entity).cloned();
            let box_collider = world
                .get::<BoxCollider>(entity)
                .copied()
                .filter(|c| c.enabled);
            let mesh = world
                .get::<MeshCollider>(entity)
                .filter(|c| c.enabled)
                .cloned();
            let tiles = world
                .get::<Tilemap>(entity)
                .filter(|t| t.enabled && !collision_boxes(world, id, t).is_empty())
                .cloned();
            ensure!(
                box_collider.is_none() || mesh.is_none(),
                "choose one collider on '{id}'"
            );
            let groups = box_collider
                .map(|c| (c.layers, c.mask))
                .or_else(|| mesh.as_ref().map(|m| (m.layers, m.mask)))
                .unwrap_or((DEFAULT_LAYERS, DEFAULT_MASK));
            // A Player Controller's collider is the authored capsule, not its Box Collider. The box
            // stays for the CPU queries (triggers, respawn, Blueprints) and for camera clearance.
            // A tilemap's merged solid rectangles take the place of a box or mesh collider.
            let (collider, mesh, tiles, capsule) = if let Some(config) = &player {
                (
                    None,
                    None,
                    None,
                    // The controller is authored in world units, independent of the visual scale.
                    Some((config.capsule_half_height(), config.capsule_radius)),
                )
            } else if tiles.is_some() {
                (None, None, tiles, None)
            } else {
                (box_collider, mesh, None, None)
            };
            // The world frame of the shape, and its pose relative to the body. The body pose
            // already carries the root's rotation, so a child cancels it out again.
            let (linear, offset) = if id == root {
                (linear_root, Pose::IDENTITY)
            } else {
                let (orientation, linear) =
                    object_frame(world, objects, matrices, entities, id, dynamic)?;
                let inverse = root_orientation.inverse();
                (
                    linear,
                    Pose::from_parts(
                        inverse * (matrices[id].w_axis.truncate() - origin),
                        inverse * orientation,
                    ),
                )
            };
            let mesh = mesh
                .map(|m| {
                    if dynamic {
                        m.mesh.convex_hull()
                    } else {
                        Ok(m.mesh.clone())
                    }
                })
                .transpose()?;
            keys.push(PartKey {
                object: id.clone(),
                entity,
                linear,
                collider,
                mesh,
                tiles,
                capsule,
                dynamic,
                groups,
                offset,
            });
        }
        Ok((root_orientation, keys))
    }
    /// Create, refresh and drop the Rapier joints the scene authors. A joint whose endpoint body
    /// is missing at runtime (a destroyed prefab) is dropped rather than failing the step.
    fn reconcile_joints(
        &mut self,
        instance: &SceneInstance,
        objects: &BTreeMap<&str, &Object>,
    ) -> Result<()> {
        let mut live = BTreeSet::new();
        for object in &instance.document.objects {
            let Some(joint) = &object.joint else { continue };
            if !joint.enabled {
                continue;
            }
            let owner = compound_root(objects, &object.id);
            let other = compound_root(objects, &joint.other);
            if owner == other
                || !self.bodies.contains_key(&owner)
                || !self.bodies.contains_key(&other)
            {
                continue;
            }
            let key = JointKey {
                owner,
                other,
                kind: joint.kind,
                anchor: joint.anchor,
                other_anchor: joint.other_anchor,
                axis: joint.axis,
                other_axis: joint.other_axis,
                limits: joint.limits,
                min_limit: joint.min_limit,
                max_limit: joint.max_limit,
            };
            live.insert(object.id.clone());
            if let Some(record) = self.joints.get(&object.id)
                && record.key == key
                && self
                    .simulation
                    .impulse_joints()
                    .any(|(handle, _)| handle == record.handle)
            {
                continue;
            }
            if let Some(record) = self.joints.remove(&object.id) {
                self.simulation.remove_impulse_joint(record.handle);
            }
            let body1 = self.bodies[&key.owner].handle;
            let body2 = self.bodies[&key.other].handle;
            // A zero-anchor Fixed joint keeps the pose the bodies have right now.
            let frame2 = (joint.kind == JointKind::Fixed
                && joint.anchor == [0.0; 3]
                && joint.other_anchor == [0.0; 3])
                .then(|| {
                    let p1 = *self.simulation.bodies[body1].position();
                    let p2 = *self.simulation.bodies[body2].position();
                    p2.inverse() * p1
                });
            let handle =
                self.simulation
                    .insert_impulse_joint(body1, body2, build_joint(joint, frame2));
            self.joints
                .insert(object.id.clone(), JointRecord { handle, key });
        }
        let stale: Vec<_> = self
            .joints
            .keys()
            .filter(|id| !live.contains(*id))
            .cloned()
            .collect();
        for id in stale {
            if let Some(record) = self.joints.remove(&id) {
                self.simulation.remove_impulse_joint(record.handle);
            }
        }
        Ok(())
    }
    /// Move every Player Controller with Rapier's kinematic character controller: slides on walls,
    /// climbs steps and slopes, snaps to ground, sweeps (no tunnelling) and rides moving platforms.
    fn drive_players(&mut self, world: &mut World, dt: f32) -> Result<()> {
        if !self.bodies.values().any(|b| b.player) {
            return Ok(());
        }
        let motion = world.remove_resource::<PlayerMotion>().unwrap_or_default();
        let players: Vec<_> = self
            .bodies
            .iter()
            .filter(|(_, b)| b.player)
            .map(|(id, b)| (id.clone(), b.entity, b.handle, b.handles.first().copied()))
            .collect();
        for (id, entity, handle, capsule) in players {
            let Some(capsule) = capsule else { continue };
            let config = world
                .get::<PlayerController>(entity)
                .cloned()
                .context("Player Controller removed")?;
            let transform = *world
                .get::<Transform>(entity)
                .context("player transform missing")?;
            let shape = SharedShape::capsule_y(config.capsule_half_height(), config.capsule_radius);
            let shape: &dyn rapier3d::prelude::Shape = shape.as_ref();
            let pose = *self.simulation.bodies[handle].position();
            // Ride a platform: whatever the ground collider's body moved this tick is added to the
            // requested translation, whether it is a dynamic body or a teleported static one.
            let platform = self
                .player_ground
                .get(&id)
                .and_then(|(ground, last)| {
                    self.simulation
                        .bodies
                        .get(*ground)
                        .map(|b| b.position().translation - last)
                })
                .unwrap_or(Vec3::ZERO);
            let desired = motion.desired + platform;
            let controller = KinematicCharacterController {
                offset: CharacterLength::Absolute(0.01),
                slide: true,
                autostep: (config.step_height > 0.0).then_some(CharacterAutostep {
                    max_height: CharacterLength::Absolute(config.step_height),
                    min_width: CharacterLength::Absolute(config.capsule_radius),
                    include_dynamic_bodies: true,
                }),
                max_slope_climb_angle: config.slope_limit_degrees.to_radians(),
                min_slope_slide_angle: config.slope_limit_degrees.to_radians(),
                snap_to_ground: config
                    .snap_to_ground
                    .then(|| CharacterLength::Absolute(config.step_height.max(0.01))),
                ..Default::default()
            };
            let mut ground = None;
            let movement = {
                let queries = self
                    .simulation
                    .query_pipeline_with_filter(QueryFilter::default().exclude_collider(capsule));
                controller.move_shape(dt, &queries, shape, &pose, desired, |hit| {
                    if hit.hit.normal1.dot(Vec3::Y) >= 0.5 {
                        ground = Some(hit.handle);
                    }
                })
            };
            let translation = pose.translation + movement.translation;
            let mut next = transform;
            next.translation = [translation.x, translation.y, translation.z];
            next.validate()?;
            *world.get_mut::<Transform>(entity).unwrap() = next;
            let mut state = world
                .get::<GravityState>(entity)
                .copied()
                .unwrap_or_default();
            // A rising controller is never grounded: the ground probe still sees the floor just
            // below, which would otherwise cancel a jump on its first tick. A blocked ascent
            // cancels the remaining rise (a ceiling).
            let rising = state.vertical_velocity > 0.0;
            if rising && movement.translation.y < desired.y * 0.5 {
                state.vertical_velocity = 0.0;
            }
            state.grounded = movement.grounded && !rising;
            if state.grounded {
                state.vertical_velocity = 0.0;
            }
            world.insert(entity, state)?;
            let pose = Pose::from_parts(translation, pose.rotation);
            self.simulation.bodies[handle].set_position(pose, true);
            if let Some(body) = self.bodies.get_mut(&id) {
                body.pose = next.matrix();
            }
            // The controller's grounded probe does not always emit a collision, so an existing
            // ground record is refreshed whenever the controller is still grounded. Only losing
            // the ground clears it.
            let on_ground = ground
                .and_then(|handle| {
                    self.simulation
                        .colliders
                        .get(handle)
                        .and_then(|c| c.parent())
                })
                .map(|ground| (ground, true))
                .or_else(|| {
                    let record = self.player_ground.get(&id).map(|(ground, _)| *ground);
                    record
                        .filter(|_| movement.grounded)
                        .map(|ground| (ground, true))
                });
            match on_ground {
                Some((ground, _)) => {
                    let position = self.simulation.bodies[ground].position().translation;
                    self.player_ground.insert(id, (ground, position));
                }
                None => {
                    self.player_ground.remove(&id);
                }
            }
        }
        Ok(())
    }
    fn step(&mut self, instance: &SceneInstance, world: &mut World, dt: f32) -> Result<()> {
        let matrices = instance.live_matrices(world)?;
        let mut objects = None;
        let key = LayoutKey::of(instance, world);
        // A step that fails while bodies are refreshed drops the layout, so the next
        // step rebuilds every body's shapes.
        let (layout, rebuilt) = match self.layout.take() {
            Some(layout) if layout.key == key => (layout, false),
            _ => {
                let objects = objects.get_or_insert_with(|| object_map(instance));
                (Layout::new(instance, world, objects, key)?, true)
            }
        };
        for &entity in &layout.resets {
            world.insert(entity, GravityState::default())?;
        }
        let roots = &layout.roots;
        // Prune bodies whose root vanished; Rapier removes attached colliders and joints too.
        self.bodies.retain(|id, b| {
            let keep = roots.contains_key(id) && instance.entity(id) == Some(b.entity);
            if !keep {
                self.simulation.remove_body(b.handle);
            }
            keep
        });
        // Drop launches aimed at objects that were removed before their body was built.
        self.pending.retain(|id, _| instance.entity(id).is_some());
        for (
            root,
            RootLayout {
                parts,
                inputs: slots,
            },
        ) in roots
        {
            let entity = slots[0].0;
            let gravity = world.get::<Gravity>(entity).copied();
            let config = gravity.unwrap_or_default();
            let dynamic = gravity.is_some_and(|g| g.enabled)
                && world.get::<PlayerController>(entity).is_none();
            let player = world.get::<PlayerController>(entity).is_some();
            let spin = world.get::<Spin>(entity).copied();
            let global = *matrices.entity(entity).context("scene object missing")?;
            let input = |&(entity, parent): &(Entity, Option<usize>)| {
                let local = *world.get::<Transform>(entity)?;
                Some((local, parent.map_or(Mat4::IDENTITY, |p| matrices.at(p))))
            };
            // Unchanged components, local transforms and parent poses give the same shapes.
            if let Some(entry) = self.bodies.get_mut(root).filter(|b| {
                !rebuilt
                    && b.config == config
                    && b.dynamic == dynamic
                    && b.player == player
                    && b.inputs.len() == slots.len()
                    && slots.iter().zip(&b.inputs).all(|(slot, recorded)| {
                        input(slot).is_some_and(|(local, parent)| {
                            super::render_extraction::transform_equal(local, recorded.0)
                                && super::render_extraction::floats_equal(
                                    &parent.to_cols_array(),
                                    &recorded.1.to_cols_array(),
                                )
                        })
                    })
            }) {
                let pose = Pose::from_parts(global.w_axis.truncate(), entry.orientation);
                place(&mut self.simulation, entry, dynamic, global, pose, spin);
                #[cfg(test)]
                {
                    self.reused_bodies += 1;
                }
                continue;
            }
            let inputs: Option<Vec<_>> = slots.iter().map(input).collect();
            let (orientation, keys) = self.part_keys(
                world,
                objects.get_or_insert_with(|| object_map(instance)),
                &instance.entities,
                &matrices,
                root,
                parts,
                dynamic,
            )?;
            let pose = Pose::from_parts(global.w_axis.truncate(), orientation);
            let mut previous = None;
            if self.bodies.get(root).is_some_and(|b| {
                b.keys != keys || b.config != config || b.dynamic != dynamic || b.player != player
            }) {
                let b = self.bodies.remove(root).unwrap();
                if b.dynamic {
                    let body = &self.simulation.bodies[b.handle];
                    previous = Some((body.linvel(), body.angvel(), b.spin));
                }
                self.simulation.remove_body(b.handle);
            }
            if let Some(entry) = self.bodies.get_mut(root) {
                // Same shapes from new inputs: remember them so the next step can skip.
                entry.orientation = orientation;
                entry.inputs = inputs.unwrap_or_default();
            } else {
                let body = if dynamic {
                    RigidBodyBuilder::dynamic()
                } else {
                    RigidBodyBuilder::fixed()
                };
                let body = body
                    .pose(pose)
                    .ccd_enabled(dynamic)
                    .linear_damping(config.linear_damping)
                    .gravity_scale(config.rapier_gravity_scale())
                    .angular_damping(config.angular_damping);
                let handle = self.simulation.insert_body(body);
                // ponytail: split the authored mass equally across compound shapes; use
                // per-shape density if a reference game needs a real inertia tensor.
                let mass = config.mass / keys.len().max(1) as f32;
                let mut handles = Vec::new();
                for key in &keys {
                    let shape = key
                        .cook()
                        .with_context(|| format!("physics shape '{}'", key.object))?;
                    let collider = ColliderBuilder::new(shape)
                        .mass(mass)
                        .friction(config.friction)
                        .restitution(config.restitution)
                        .position(key.offset)
                        .collision_groups(InteractionGroups::new(
                            Group::from_bits_truncate(key.groups.0),
                            Group::from_bits_truncate(key.groups.1),
                            rapier3d::geometry::InteractionTestMode::And,
                        ));
                    handles.push(self.simulation.insert_collider(collider, Some(handle)));
                }
                if dynamic {
                    let state = world
                        .get::<GravityState>(entity)
                        .copied()
                        .unwrap_or_default();
                    ensure!(
                        state.vertical_velocity.is_finite(),
                        "invalid fall velocity on '{root}'"
                    );
                    let velocity = self.pending.remove(root).unwrap_or_else(|| {
                        previous.map_or(Vec3::Y * state.vertical_velocity, |v| v.0)
                    });
                    let angular = previous.filter(|v| v.2 == spin).map_or_else(
                        || Vec3::from_array(spin.map_or([0.; 3], |s| s.0)).map(f32::to_radians),
                        |v| v.1,
                    );
                    self.simulation.bodies[handle].set_linvel(velocity, true);
                    self.simulation.bodies[handle].set_angvel(angular, true);
                    if let Some(saved) = self.restored.remove(root) {
                        let body = &mut self.simulation.bodies[handle];
                        body.set_linvel(Vec3::from(saved.linear), true);
                        body.set_angvel(Vec3::from(saved.angular), true);
                        if saved.sleeping {
                            body.sleep();
                        }
                    }
                }
                self.bodies.insert(
                    root.clone(),
                    Body {
                        entity,
                        handle,
                        handles,
                        keys,
                        dynamic,
                        player,
                        config,
                        pose: global,
                        spin,
                        inputs: inputs.unwrap_or_default(),
                        orientation,
                    },
                );
            }
            let entry = self.bodies.get_mut(root).unwrap();
            place(&mut self.simulation, entry, dynamic, global, pose, spin);
        }
        self.layout = Some(layout);
        // A teleported static body is not an active island, so Rapier would leave its colliders
        // (and therefore the character queries and contacts) at the old pose.
        self.simulation
            .bodies
            .propagate_modified_body_positions_to_colliders(&mut self.simulation.colliders);
        if !self.joints.is_empty() || instance.document.objects.iter().any(|o| o.joint.is_some()) {
            let objects = objects.get_or_insert_with(|| object_map(instance));
            self.reconcile_joints(instance, objects)?;
        }
        // ponytail: bounded substeps for public callers; the app already supplies 60 Hz ticks.
        let steps = (dt * 60.0).ceil() as u32;
        self.simulation.integration_parameters.dt = dt / steps as f32;
        self.simulation
            .integration_parameters
            .normalized_allowed_linear_error = 0.00001;
        self.simulation.integration_parameters.max_ccd_substeps = 4;
        self.simulation.integration_parameters.num_solver_iterations = 8;
        self.simulation
            .integration_parameters
            .contact_softness
            .natural_frequency = 60.0;
        self.simulation
            .integration_parameters
            .static_contact_softness
            .natural_frequency = 120.0;
        for _ in 0..steps {
            self.simulation.step();
            ensure!(
                self.simulation.quarantine().is_empty(),
                "physics solver rejected non-finite body state"
            );
            for entry in self.bodies.values().filter(|b| b.dynamic) {
                let b = &mut self.simulation.bodies[entry.handle];
                let mut v = b.linvel();
                if v.y < -entry.config.max_speed {
                    v.y = -entry.config.max_speed;
                    b.set_linvel(v, false);
                }
            }
        }
        // Run after the solver so the character queries the world at this tick's positions.
        self.drive_players(world, dt)?;
        let mut updates = Vec::new();
        for (id, entry) in &self.bodies {
            if !entry.dynamic {
                continue;
            }
            let b = &self.simulation.bodies[entry.handle];
            ensure!(
                b.translation().is_finite()
                    && b.rotation().is_finite()
                    && b.linvel().is_finite()
                    && b.angvel().is_finite(),
                "invalid physics pose '{id}'"
            );
            let object = &instance.document.objects[instance.object_indices[&entry.entity]];
            let parent = object
                .parent
                .as_ref()
                .map(|p| matrices[p])
                .unwrap_or(Mat4::IDENTITY);
            let q = parent_pose(parent)?.0.inverse() * b.rotation();
            let (y, x, z) = q.to_euler(EulerRot::YXZ);
            let old = *world.get::<Transform>(entry.entity).unwrap();
            let mut transform = old;
            transform.translation = parent
                .inverse()
                .transform_point3(b.translation())
                .to_array();
            transform.rotation_degrees = [x, y, z].map(f32::to_degrees);
            transform.validate()?;
            // Any compound shape touching the ground grounds the body.
            let grounded = entry.handles.iter().any(|&handle| {
                self.simulation
                    .narrow_phase
                    .contact_pairs_with(handle)
                    .any(|pair| {
                        pair.manifolds.iter().any(|m| {
                            let normal = if pair.collider1 == handle {
                                -m.data.normal
                            } else {
                                m.data.normal
                            };
                            normal.y >= 0.5
                                && (!m.data.solver_contacts.is_empty()
                                    || m.points.iter().any(|p| p.dist <= 0.001))
                        })
                    })
            });
            let velocity = b.linvel().y;
            let state = GravityState {
                vertical_velocity: if velocity.abs() < 0.001 {
                    0.0
                } else {
                    velocity
                },
                grounded,
            };
            updates.push((id, entry.entity, old, transform, state));
        }
        for (_, e, _, t, _) in &updates {
            world.insert(*e, *t)?;
        }
        let matrices = match instance.live_matrices(world) {
            Ok(matrices) => matrices,
            Err(error) => {
                for (_, e, t, _, _) in updates {
                    world.insert(e, t)?;
                }
                return Err(error);
            }
        };
        for (_, e, _, _, state) in updates {
            world.insert(e, state)?;
        }
        for b in self.bodies.values_mut() {
            b.pose = *matrices.entity(b.entity).context("scene object missing")?;
        }
        Ok(())
    }
}
impl SceneInstance {
    /// Launch from a graph: blueprint targets are document IDs, bodies are ECS entities.
    pub(crate) fn set_velocity(
        &self,
        world: &mut World,
        id: &str,
        entity: Entity,
        velocity: Vec3,
    ) -> Result<()> {
        ensure!(
            velocity.is_finite() && velocity.length() <= 1000.0,
            "Set Velocity on '{id}' must be finite and at most 1000 units/second"
        );
        ensure!(
            world.get::<Gravity>(entity).is_some_and(|g| g.enabled)
                && world.get::<PlayerController>(entity).is_none(),
            "Set Velocity needs a Gravity rigidbody that is not a Player Controller"
        );
        let mut physics = world.remove_resource::<Physics>().unwrap_or_default();
        physics.set_velocity(id, entity, velocity);
        world.insert_resource(physics);
        Ok(())
    }
    pub(crate) fn step_bodies(&self, world: &mut World, dt: f32) -> Result<()> {
        // The character controller needs the world even when no dynamic body exists.
        let needs_world = !self
            .component_entities::<PlayerController>(world)
            .is_empty()
            || self
                .component_entities::<Gravity>(world)
                .into_iter()
                .any(|(_, &e)| world.get::<Gravity>(e).is_some_and(|g| g.enabled));
        if !needs_world {
            world.remove_resource::<Physics>();
            return Ok(());
        }
        let mut physics = world.remove_resource::<Physics>().unwrap_or_default();
        physics.step(self, world, dt)?;
        world.insert_resource(physics);
        Ok(())
    }
    pub fn physics_body_count(&self, world: &World) -> usize {
        world.resource::<Physics>().map_or(0, |p| {
            debug_assert_eq!(
                p.simulation.colliders.len(),
                p.bodies.values().map(|b| b.handles.len()).sum::<usize>()
            );
            p.bodies.len()
        })
    }
}

impl Physics {
    pub(crate) fn blueprint_contacts(
        &self,
        world: &World,
        matrices: &crate::transforms::Matrices<'_>,
    ) -> BTreeMap<String, Vec<collision::Contact>> {
        let ids = self.part_ids(world, matrices);
        let mut result: BTreeMap<String, Vec<collision::Contact>> = BTreeMap::new();
        for pair in self.simulation.narrow_phase.contact_pairs() {
            let (Some(a), Some(b)) = (ids.get(&pair.collider1), ids.get(&pair.collider2)) else {
                continue;
            };
            let mut normal = Vec3::ZERO;
            let mut impulse = 0.;
            let mut found = false;
            for manifold in &pair.manifolds {
                if !manifold.data.solver_contacts.is_empty()
                    || manifold.points.iter().any(|p| p.dist <= 0.001)
                {
                    if !found {
                        normal = manifold.data.normal;
                        found = true;
                    }
                    impulse += manifold.points.iter().map(|p| p.data.impulse).sum::<f32>();
                }
            }
            if found {
                result
                    .entry((*a).to_owned())
                    .or_default()
                    .push(collision::Contact {
                        other: (*b).to_owned(),
                        normal: -normal,
                        impulse,
                    });
                result
                    .entry((*b).to_owned())
                    .or_default()
                    .push(collision::Contact {
                        other: (*a).to_owned(),
                        normal,
                        impulse,
                    });
            }
        }
        for contacts in result.values_mut() {
            contacts.sort_by(|a, b| a.other.cmp(&b.other));
        }
        result
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BodySave {
    id: String,
    linear: [f32; 3],
    angular: [f32; 3],
    sleeping: bool,
}
impl Physics {
    pub(crate) fn save(&self) -> Vec<BodySave> {
        let mut result: BTreeMap<_, _> = self
            .bodies
            .iter()
            .filter(|(_, b)| b.dynamic)
            .map(|(id, b)| {
                let body = &self.simulation.bodies[b.handle];
                (
                    id.clone(),
                    BodySave {
                        id: id.clone(),
                        linear: body.linvel().to_array(),
                        angular: body.angvel().to_array(),
                        sleeping: body.is_sleeping(),
                    },
                )
            })
            .collect();
        for (id, saved) in &self.restored {
            result.insert(id.clone(), saved.clone());
        }
        for (id, v) in &self.pending {
            result.insert(
                id.clone(),
                BodySave {
                    id: id.clone(),
                    linear: v.to_array(),
                    angular: [0.; 3],
                    sleeping: false,
                },
            );
        }
        result.into_values().collect()
    }
    pub(crate) fn restore(saved: &[BodySave], scene: &Scene) -> Result<Self> {
        let mut restored = BTreeMap::new();
        for body in saved {
            ensure!(
                scene
                    .objects
                    .iter()
                    .any(|o| o.id == body.id && o.gravity.is_some_and(|g| g.enabled))
                    && body
                        .linear
                        .iter()
                        .chain(&body.angular)
                        .all(|v| v.is_finite()),
                "invalid saved body"
            );
            ensure!(
                restored.insert(body.id.clone(), body.clone()).is_none(),
                "duplicate saved body"
            );
        }
        Ok(Self {
            restored,
            ..Self::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(id: &str, translation: [f32; 3], extra: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"{id}",
                "transform":{{"translation":{translation:?},"rotation_degrees":[0,0,0],"scale":[1,1,1]}}{extra}}}"#
        )
    }

    fn cube() -> TriangleMesh {
        let p: [[f32; 3]; 8] = std::array::from_fn(|i| {
            [0, 1, 2].map(|axis| if i >> axis & 1 == 0 { -0.5 } else { 0.5 })
        });
        let faces = [
            [0, 2, 3],
            [0, 3, 1],
            [4, 5, 7],
            [4, 7, 6],
            [0, 1, 5],
            [0, 5, 4],
            [2, 6, 7],
            [2, 7, 3],
            [0, 4, 6],
            [0, 6, 2],
            [1, 3, 7],
            [1, 7, 5],
        ];
        TriangleMesh::new(faces.iter().map(|f| f.map(|i| p[i])).collect()).unwrap()
    }

    /// Static shapes under a moving ancestry and a sheared parent, a compound and a mesh
    /// body, a wall and a Player Controller: every kind of input a body's shapes read.
    /// `marker` precedes the floor's parent, so removing it shifts document indices.
    fn scene() -> Scene {
        let objects = [
            object(
                "camera",
                [0., 4., 8.],
                r#","camera":{"projection":"perspective","vertical_fov_degrees":55,"near":0.1,"far":100}"#,
            ),
            object(
                "player",
                [6., 1., 0.],
                r#","collider":{"size":[0.8,1.2,0.8]},"gravity":{"enabled":true,"acceleration":10},
                "player_controller":{"camera":"camera","capsule_radius":0.4,"capsule_height":1.2,"fall_height":-20}"#,
            ),
            object("marker", [0., 0., 0.], ""),
            object("base", [0., 0., 0.], ""),
            object("stand", [0., 0., 0.], r#","parent":"base""#),
            object("wall", [3., 1., 0.], r#","collider":{"size":[0.5,2,4]}"#),
            object(
                "floor",
                [0., -0.5, 0.],
                r#","parent":"stand","collider":{"size":[30,1,30]}"#,
            ),
            object(
                "crate",
                [0., 3., 0.],
                r#","collider":{"size":[1,1,1]},"gravity":{"enabled":true,"acceleration":10}"#,
            ),
            object(
                "arm",
                [0., -1., 0.],
                r#","parent":"crate","collider":{"size":[1,1,1]}"#,
            ),
            object(
                "ball",
                [-3., 2., 0.],
                r#","collider":{},"gravity":{"enabled":true,"acceleration":10}"#,
            ),
            r#"{"id":"skew","name":"skew",
                "transform":{"translation":[-6,0.5,0],"rotation_degrees":[0,30,0],"scale":[2,1,1]}}"#
                .into(),
            r#"{"id":"slab","name":"slab","parent":"skew",
                "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,20],"scale":[1,1,1]},
                "collider":{"size":[2,0.5,2]}}"#
                .into(),
            object(
                "pebble",
                [-6., 3., 0.],
                r#","collider":{"size":[0.5,0.5,0.5]},"gravity":{"enabled":true,"acceleration":10}"#,
            ),
        ]
        .join(",");
        let mut scene = Scene::from_json(&format!(
            r#"{{"version":1,"name":"shape reuse","views":{{"3d":"camera"}},"objects":[{objects}]}}"#
        ))
        .unwrap();
        let ball = scene.objects.iter_mut().find(|o| o.id == "ball").unwrap();
        ball.collider = None;
        ball.mesh_collider = Some(MeshCollider {
            enabled: true,
            layers: DEFAULT_LAYERS,
            mask: DEFAULT_MASK,
            mesh: cube(),
        });
        scene
    }

    /// Outside edits at fixed ticks: each changes one input of some body's shapes or layout.
    fn edit(instance: &mut SceneInstance, world: &mut World, tick: usize) {
        let entity = |id: &str| instance.entity(id).unwrap();
        match tick {
            30 => {
                world
                    .get_mut::<Transform>(entity("base"))
                    .unwrap()
                    .translation = [0.5, 0., 0.]
            }
            60 => {
                world
                    .get_mut::<Transform>(entity("stand"))
                    .unwrap()
                    .rotation_degrees = [0., 0., 2.]
            }
            90 => world.get_mut::<BoxCollider>(entity("floor")).unwrap().size = [30., 1.5, 30.],
            120 => {
                world
                    .get_mut::<Transform>(entity("arm"))
                    .unwrap()
                    .translation = [0.3, -1., 0.]
            }
            150 => world.get_mut::<Gravity>(entity("crate")).unwrap().friction = 0.2,
            180 => {
                world
                    .get_mut::<BoxCollider>(entity("wall"))
                    .unwrap()
                    .enabled = false
            }
            200 => instance
                .remove_objects_raw(world, &BTreeSet::from(["marker".to_owned()]), false)
                .unwrap(),
            210 => {
                let mut wall = world.get_mut::<BoxCollider>(entity("wall")).unwrap();
                wall.enabled = true;
                wall.layers = 2;
            }
            225 => {
                world
                    .get_mut::<Transform>(entity("base"))
                    .unwrap()
                    .translation = [-0.5, 0., 0.]
            }
            240 => {
                world
                    .get_mut::<Transform>(entity("skew"))
                    .unwrap()
                    .rotation_degrees = [0., 45., 0.]
            }
            270 => world.get_mut::<MeshCollider>(entity("ball")).unwrap().mask = 1,
            300 => {
                world
                    .get_mut::<PlayerController>(entity("player"))
                    .unwrap()
                    .capsule_radius = 0.3
            }
            320 => world.get_mut::<Gravity>(entity("crate")).unwrap().enabled = false,
            340 => world.get_mut::<Gravity>(entity("crate")).unwrap().enabled = true,
            _ => {}
        }
    }

    type Shape = (String, Vec<PartKey>, [u32; 4], bool, bool, usize);
    type Root = (String, Vec<String>, Vec<(String, Option<usize>)>);
    type State = (
        Vec<(String, [u32; 16], Option<[u32; 2]>)>,
        Vec<Shape>,
        Vec<Root>,
        Vec<String>,
    );

    /// Poses, fall states, body shapes and layout by object ID, bit for bit.
    /// `anchor` replaces world-local entity handles so two worlds compare.
    fn state(instance: &SceneInstance, world: &World, anchor: Entity) -> State {
        let id = |entity: &Entity| {
            instance.document.objects[instance.object_indices[entity]]
                .id
                .clone()
        };
        let poses = instance
            .global_transforms(world)
            .unwrap()
            .into_iter()
            .map(|(id, matrix)| {
                let fall = world
                    .get::<GravityState>(instance.entity(&id).unwrap())
                    .map(|s| [s.vertical_velocity.to_bits(), u32::from(s.grounded)]);
                (id, matrix.to_cols_array().map(f32::to_bits), fall)
            })
            .collect();
        let physics = world.resource::<Physics>().unwrap();
        let shapes = physics
            .bodies
            .iter()
            .map(|(root, body)| {
                let keys = body
                    .keys
                    .iter()
                    .map(|key| PartKey {
                        entity: anchor,
                        ..key.clone()
                    })
                    .collect();
                let orientation = body.orientation.to_array().map(f32::to_bits);
                let parts = body.handles.len();
                (
                    root.clone(),
                    keys,
                    orientation,
                    body.dynamic,
                    body.player,
                    parts,
                )
            })
            .collect();
        let layout = physics.layout.as_ref().unwrap();
        let roots = layout
            .roots
            .iter()
            .map(|(root, layout)| {
                let inputs = layout.inputs.iter().map(|(e, parent)| (id(e), *parent));
                (root.clone(), layout.parts.clone(), inputs.collect())
            })
            .collect();
        let resets = layout.resets.iter().map(id).collect();
        (poses, shapes, roots, resets)
    }

    #[test]
    fn reused_shapes_match_rebuilding_every_body_each_step() {
        let scene = scene();
        let (mut cached, mut rebuilt) = (World::new(), World::new());
        let mut reusing = scene.spawn(&mut cached).unwrap();
        let mut rebuilding = scene.spawn(&mut rebuilt).unwrap();
        let anchor = World::new().spawn();
        for tick in 0..360 {
            edit(&mut reusing, &mut cached, tick);
            edit(&mut rebuilding, &mut rebuilt, tick);
            // Without a layout every body takes the full path, as before shapes were reused.
            if let Some(physics) = rebuilt.resource_mut::<Physics>() {
                physics.layout = None;
            }
            reusing.step_gravity(&mut cached, 1. / 60.).unwrap();
            rebuilding.step_gravity(&mut rebuilt, 1. / 60.).unwrap();
            assert!(
                state(&reusing, &cached, anchor) == state(&rebuilding, &rebuilt, anchor),
                "reused shapes diverged from rebuilt ones at tick {tick}"
            );
        }
        let reused = |world: &World| world.resource::<Physics>().unwrap().reused_bodies;
        assert_eq!(reused(&rebuilt), 0);
        assert!(
            reused(&cached) > 360,
            "static bodies should skip their part keys, reused {}",
            reused(&cached)
        );
        let crate_y = cached
            .get::<Transform>(reusing.entity("crate").unwrap())
            .unwrap()
            .translation[1];
        assert!(
            crate_y < 2.5,
            "the crate should have fallen, not stay at {crate_y}"
        );
    }

    #[test]
    fn instances_sharing_a_world_do_not_reuse_each_others_layout() {
        let scene = Scene::from_json(&format!(
            r#"{{"version":1,"name":"shared","views":{{}},"objects":[{},{}]}}"#,
            object(
                "body",
                [0., 3., 0.],
                r#","collider":{},"gravity":{"enabled":true,"acceleration":10}"#
            ),
            object("floor", [0., -0.5, 0.], r#","collider":{"size":[10,1,10]}"#),
        ))
        .unwrap();
        let mut world = World::new();
        let first = scene.spawn(&mut world).unwrap();
        let second = scene.spawn(&mut world).unwrap();
        for _ in 0..30 {
            first.step_gravity(&mut world, 1. / 60.).unwrap();
            second.step_gravity(&mut world, 1. / 60.).unwrap();
        }
        for instance in [&first, &second] {
            let y = world
                .get::<Transform>(instance.entity("body").unwrap())
                .unwrap()
                .translation[1];
            assert!(y < 3., "each instance's body should fall, not stay at {y}");
        }
    }
}
