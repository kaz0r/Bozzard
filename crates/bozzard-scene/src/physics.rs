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
    matrices: &BTreeMap<String, Mat4>,
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

impl Physics {
    /// Collider handle to owning object ID for every live body part still matching its pose.
    fn part_ids<'a>(
        &'a self,
        world: &World,
        matrices: &BTreeMap<String, Mat4>,
    ) -> std::collections::HashMap<ColliderHandle, &'a str> {
        self.bodies
            .iter()
            .filter(|(id, b)| matrices.get(*id) == Some(&b.pose))
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
        matrices: &BTreeMap<String, Mat4>,
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
        matrices: &BTreeMap<String, Mat4>,
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
        let matrices = instance.global_transforms(world)?;
        let objects: BTreeMap<_, _> = instance
            .document
            .objects
            .iter()
            .map(|o| (o.id.as_str(), o))
            .collect();
        // Group every colliding object under the Rigidbody that owns it.
        let mut roots: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
                    .entry(compound_root(&objects, id))
                    .or_default()
                    .push(id.clone());
            }
        }
        // A Rigidbody with no colliding shape anywhere is not a body; clear its runtime state.
        for (id, &entity) in &instance.entities {
            let Some(gravity) = world.get::<Gravity>(entity).copied() else {
                continue;
            };
            if !gravity.enabled || !roots.contains_key(id) {
                world.insert(entity, GravityState::default())?;
            }
        }
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
        for (root, parts) in &roots {
            let entity = instance.entities[root];
            let gravity = world.get::<Gravity>(entity).copied();
            let config = gravity.unwrap_or_default();
            let dynamic = gravity.is_some_and(|g| g.enabled)
                && world.get::<PlayerController>(entity).is_none();
            let player = world.get::<PlayerController>(entity).is_some();
            let spin = world.get::<Spin>(entity).copied();
            let global = matrices[root];
            let (keys_orientation, keys) = self.part_keys(
                world,
                &objects,
                &instance.entities,
                &matrices,
                root,
                parts,
                dynamic,
            )?;
            let pose = Pose::from_parts(global.w_axis.truncate(), keys_orientation);
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
            if !self.bodies.contains_key(root) {
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
                    },
                );
            }
            let entry = self.bodies.get_mut(root).unwrap();
            let handle = entry.handle;
            let body = &mut self.simulation.bodies[handle];
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
        // A teleported static body is not an active island, so Rapier would leave its colliders
        // (and therefore the character queries and contacts) at the old pose.
        self.simulation
            .bodies
            .propagate_modified_body_positions_to_colliders(&mut self.simulation.colliders);
        self.reconcile_joints(instance, &objects)?;
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
            let object = objects[id.as_str()];
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
        if let Err(error) = instance.global_transforms(world) {
            for (_, e, t, _, _) in updates {
                world.insert(e, t)?;
            }
            return Err(error);
        }
        let matrices = instance.global_transforms(world)?;
        for (_, e, _, _, state) in updates {
            world.insert(e, state)?;
        }
        for (id, b) in &mut self.bodies {
            b.pose = matrices[id];
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
        matrices: &BTreeMap<String, Mat4>,
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
