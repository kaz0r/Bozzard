//! Rapier owns non-player body velocities, contacts, inertia and sleeping.
//! ECS transforms remain the public pose: external edits are teleports on the next tick.
use super::*;
use glam::Mat3;
use rapier3d::math::Pose;
use rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, PhysicsWorld, RigidBodyBuilder, RigidBodyHandle, SharedShape,
};

#[derive(Clone, PartialEq)]
struct ShapeKey {
    linear: Mat3,
    collider: Option<BoxCollider>,
    mesh: Option<TriangleMesh>,
    dynamic: bool,
}
struct Body {
    entity: Entity,
    handle: RigidBodyHandle,
    collider: ColliderHandle,
    shape: ShapeKey,
    config: Gravity,
    pose: Mat4,
    spin: Option<Spin>,
}
#[derive(Default)]
pub(crate) struct Physics {
    simulation: PhysicsWorld,
    bodies: BTreeMap<String, Body>,
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
fn rotation(transform: &Transform) -> Quat {
    let [x, y, z] = transform.rotation_degrees.map(f32::to_radians);
    Quat::from_euler(EulerRot::YXZ, y, x, z)
}
impl ShapeKey {
    fn cook(&self) -> Result<SharedShape> {
        if let Some(collider) = self.collider {
            let (_, _, corners) = collider.geometry(Mat4::from_mat3(self.linear))?;
            return SharedShape::convex_hull(&corners).context("invalid Rigidbody box hull");
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
            SharedShape::convex_hull(&points).context("invalid dynamic mesh hull")
        } else {
            let indices = (0..points.len() as u32)
                .step_by(3)
                .map(|i| [i, i + 1, i + 2])
                .collect();
            Ok(SharedShape::trimesh(points, indices)?)
        }
    }
}
impl Physics {
    pub(crate) fn contacts(
        &self,
        world: &World,
        matrices: &BTreeMap<String, Mat4>,
    ) -> Vec<(String, String)> {
        let ids: std::collections::HashMap<_, _> = self
            .bodies
            .iter()
            .filter(|(id, b)| {
                matrices.get(*id) == Some(&b.pose)
                    && (world
                        .get::<BoxCollider>(b.entity)
                        .is_some_and(|c| c.enabled)
                        || world
                            .get::<MeshCollider>(b.entity)
                            .is_some_and(|c| c.enabled))
            })
            .map(|(id, b)| (b.collider, id))
            .collect();
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
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                })
            })
            .collect()
    }
    pub(crate) fn remove_entity(&mut self, entity: Entity) {
        self.bodies.retain(|_, body| {
            if body.entity == entity {
                self.simulation.remove_body(body.handle);
                false
            } else {
                true
            }
        });
    }
    pub(crate) fn launch(&mut self, entity: Entity, speed: f32) {
        if let Some(body) = self.bodies.values().find(|b| b.entity == entity) {
            let body = &mut self.simulation.bodies[body.handle];
            let mut velocity = body.linvel();
            velocity.y = speed;
            body.set_linvel(velocity, true);
        }
    }
    fn step(&mut self, instance: &SceneInstance, world: &mut World, dt: f32) -> Result<()> {
        let matrices = instance.global_transforms(world)?;
        let objects: BTreeMap<_, _> = instance
            .document
            .objects
            .iter()
            .map(|o| (o.id.as_str(), o))
            .collect();
        // Prune removed/disabled shapes before queries; Rapier removes attached contacts and joints too.
        self.bodies.retain(|id, b| {
            let keep = instance.entity(id) == Some(b.entity)
                && (world
                    .get::<BoxCollider>(b.entity)
                    .is_some_and(|c| c.enabled)
                    || world
                        .get::<MeshCollider>(b.entity)
                        .is_some_and(|c| c.enabled));
            if !keep {
                self.simulation.remove_body(b.handle);
            }
            keep
        });
        for (id, &entity) in &instance.entities {
            let gravity = world.get::<Gravity>(entity).copied();
            let player = world.get::<PlayerController>(entity).is_some();
            let collider = world
                .get::<BoxCollider>(entity)
                .copied()
                .filter(|c| c.enabled);
            let mesh = world
                .get::<MeshCollider>(entity)
                .filter(|c| c.enabled)
                .cloned();
            if let Some(g) = gravity {
                g.validate()?;
            }
            if gravity.is_some()
                && (!gravity.unwrap().enabled || (collider.is_none() && mesh.is_none()))
            {
                world.insert(entity, GravityState::default())?;
            }
            if collider.is_none() && mesh.is_none() {
                continue;
            }
            ensure!(
                collider.is_none() || mesh.is_none(),
                "choose one collider on '{id}'"
            );
            let dynamic = gravity.is_some_and(|g| g.enabled) && !player;
            let object = objects[id.as_str()];
            let parent = object
                .parent
                .as_ref()
                .map(|p| matrices[p])
                .unwrap_or(Mat4::IDENTITY);
            let global = matrices[id];
            let local = world
                .get::<Transform>(entity)
                .context("missing physics transform")?;
            let (orientation, linear) = match parent_pose(parent) {
                Ok((q, s)) => (
                    q * rotation(local),
                    Mat3::from_diagonal(Vec3::from_array(local.scale) * s),
                ),
                Err(error) if dynamic => return Err(error.context(format!("Rigidbody '{id}'"))),
                Err(_) => (Quat::IDENTITY, Mat3::from_mat4(global)),
            };
            // Compound body ownership is not implicit: colliding descendants would otherwise fight their parent.
            let mut ancestor = object.parent.as_deref();
            while let Some(a) = ancestor {
                let e = instance.entities[a];
                ensure!(
                    !world.get::<Gravity>(e).is_some_and(|g| g.enabled),
                    "Rigidbody '{a}' cannot carry a colliding descendant '{id}'; use one collider per body"
                );
                ancestor = objects[a].parent.as_deref();
            }
            let mesh = mesh
                .map(|m| {
                    if dynamic {
                        m.mesh.convex_hull()
                    } else {
                        Ok(m.mesh.clone())
                    }
                })
                .transpose()?;
            let key = ShapeKey {
                linear,
                collider,
                mesh,
                dynamic,
            };
            let config = gravity.unwrap_or_default();
            let spin = world.get::<Spin>(entity).copied();
            let pose = Pose::from_parts(global.w_axis.truncate(), orientation);
            let mut previous = None;
            if self
                .bodies
                .get(id)
                .is_some_and(|b| b.shape != key || b.config != config)
            {
                let b = self.bodies.remove(id).unwrap();
                if b.shape.dynamic {
                    let body = &self.simulation.bodies[b.handle];
                    previous = Some((body.linvel(), body.angvel(), b.spin));
                }
                self.simulation.remove_body(b.handle);
            }
            if !self.bodies.contains_key(id) {
                let shape = key
                    .cook()
                    .with_context(|| format!("physics shape '{id}'"))?;
                let body = if dynamic {
                    RigidBodyBuilder::dynamic()
                } else {
                    RigidBodyBuilder::fixed()
                };
                let body = body
                    .pose(pose)
                    .ccd_enabled(dynamic)
                    .gravity_scale(config.acceleration / 9.81)
                    .angular_damping(config.angular_damping);
                let (handle, collider) = self.simulation.insert(
                    body,
                    ColliderBuilder::new(shape)
                        .mass(config.mass)
                        .friction(config.friction)
                        .restitution(config.restitution),
                );
                if dynamic {
                    let state = world
                        .get::<GravityState>(entity)
                        .copied()
                        .unwrap_or_default();
                    ensure!(
                        state.vertical_velocity.is_finite(),
                        "invalid fall velocity on '{id}'"
                    );
                    let velocity = previous.map_or(Vec3::Y * state.vertical_velocity, |v| v.0);
                    let angular = previous.filter(|v| v.2 == spin).map_or_else(
                        || Vec3::from_array(spin.map_or([0.; 3], |s| s.0)).map(f32::to_radians),
                        |v| v.1,
                    );
                    self.simulation.bodies[handle].set_linvel(velocity, true);
                    self.simulation.bodies[handle].set_angvel(angular, true);
                }
                self.bodies.insert(
                    id.clone(),
                    Body {
                        entity,
                        handle,
                        collider,
                        shape: key,
                        config,
                        pose: global,
                        spin,
                    },
                );
            }
            let entry = self.bodies.get_mut(id).unwrap();
            let body = &mut self.simulation.bodies[entry.handle];
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
            for entry in self.bodies.values().filter(|b| b.shape.dynamic) {
                let b = &mut self.simulation.bodies[entry.handle];
                let mut v = b.linvel();
                if v.y < -entry.config.max_speed {
                    v.y = -entry.config.max_speed;
                    b.set_linvel(v, false);
                }
            }
        }
        let mut updates = Vec::new();
        for (id, entry) in &self.bodies {
            if !entry.shape.dynamic {
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
            let grounded = self
                .simulation
                .narrow_phase
                .contact_pairs_with(entry.collider)
                .any(|pair| {
                    pair.manifolds.iter().any(|m| {
                        let normal = if pair.collider1 == entry.collider {
                            -m.data.normal
                        } else {
                            m.data.normal
                        };
                        normal.y >= 0.5
                            && (!m.data.solver_contacts.is_empty()
                                || m.points.iter().any(|p| p.dist <= 0.001))
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
    pub(crate) fn step_bodies(&self, world: &mut World, dt: f32) -> Result<()> {
        // Don't allocate a second collision world for scenes with only kinematic players/static scenery.
        if !self.entities.values().any(|&e| {
            world.get::<Gravity>(e).is_some_and(|g| g.enabled)
                && world.get::<PlayerController>(e).is_none()
        }) {
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
            debug_assert_eq!(p.simulation.colliders.len(), p.bodies.len());
            p.simulation.bodies.len()
        })
    }
}
