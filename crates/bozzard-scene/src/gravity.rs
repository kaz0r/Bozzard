//! Rigidbody settings (`gravity` is retained as the scene key for compatibility).
//! Player Controllers remain kinematic; other bodies use Rapier dynamics.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gravity {
    pub enabled: bool,
    /// Positive world-down acceleration, in world units / second squared.
    pub acceleration: f32,
    pub max_speed: f32,
    /// Upward launch speed used by editor Play controls, in world units / second.
    pub jump_speed: f32,
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    pub angular_damping: f32,
}
impl Default for Gravity {
    fn default() -> Self {
        Self {
            enabled: true,
            acceleration: 9.81,
            max_speed: 50.0,
            jump_speed: 5.0,
            mass: 1.0,
            friction: 0.6,
            restitution: 0.0,
            angular_damping: 0.1,
        }
    }
}
impl Gravity {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.mass.is_finite() && (0.0001..=1000000.0).contains(&self.mass),
            "Rigidbody mass must be in 0.0001..=1000000"
        );
        ensure!(
            self.friction.is_finite() && (0.0..=10.0).contains(&self.friction),
            "Rigidbody friction must be in 0..=10"
        );
        ensure!(
            self.restitution.is_finite() && (0.0..=1.0).contains(&self.restitution),
            "Rigidbody restitution must be in 0..=1"
        );
        ensure!(
            self.angular_damping.is_finite() && (0.0..=100.0).contains(&self.angular_damping),
            "Rigidbody angular damping must be in 0..=100"
        );
        ensure!(
            self.acceleration.is_finite() && self.acceleration > 0.0,
            "gravity acceleration must be positive and finite"
        );
        ensure!(
            self.max_speed.is_finite() && self.max_speed > 0.0,
            "gravity max speed must be positive and finite"
        );
        ensure!(
            self.jump_speed.is_finite() && self.jump_speed > 0.0,
            "jump speed must be positive and finite"
        );
        Ok(())
    }
}
/// Runtime-only state. Restarting/spawning a scene resets velocity and grounding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GravityState {
    pub vertical_velocity: f32,
    pub grounded: bool,
}
impl SceneInstance {
    /// Apply an upward launch speed only to a grounded, enabled gravity box.
    /// Returns false when jumping is unavailable; consumes grounding immediately.
    pub fn jump_box(&self, world: &mut World, id: &str, speed: f32) -> Result<bool> {
        ensure!(
            speed.is_finite() && speed > 0.0,
            "jump speed must be positive and finite"
        );
        let entity = self.entity(id).context("unknown jumping object")?;
        if !world.get::<Gravity>(entity).is_some_and(|g| g.enabled)
            || !(world.get::<BoxCollider>(entity).is_some_and(|c| c.enabled)
                || world.get::<MeshCollider>(entity).is_some_and(|c| c.enabled))
            || !world
                .get::<GravityState>(entity)
                .is_some_and(|s| s.grounded)
        {
            return Ok(false);
        }
        world.insert(
            entity,
            GravityState {
                vertical_velocity: speed,
                grounded: false,
            },
        )?;
        if let Some(physics) = world.resource_mut::<crate::physics::Physics>() {
            physics.launch(entity, speed);
        }
        Ok(true)
    }

    /// Step kinematic players followed by dynamic rigid bodies. Disabled bodies reset velocity.
    /// Public timesteps are bounded to one second and internally substepped; use fixed ticks.
    pub fn step_gravity(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(
            dt.is_finite() && dt > 0.0 && dt <= 1.0,
            "gravity timestep must be finite and in (0, 1]"
        );
        for (id, &entity) in &self.entities {
            let Some(gravity) = world.get::<Gravity>(entity).copied() else {
                if world.get::<GravityState>(entity).is_some() {
                    world.insert(entity, GravityState::default())?;
                }
                continue;
            };
            gravity.validate()?;
            if !gravity.enabled
                || !(world.get::<BoxCollider>(entity).is_some_and(|c| c.enabled)
                    || world.get::<MeshCollider>(entity).is_some_and(|c| c.enabled))
            {
                world.insert(entity, GravityState::default())?;
                continue;
            }
            if world.get::<PlayerController>(entity).is_none() {
                continue;
            }
            let mut state = world
                .get::<GravityState>(entity)
                .copied()
                .unwrap_or_default();
            ensure!(
                state.vertical_velocity.is_finite(),
                "invalid fall velocity on '{id}'"
            );
            state.vertical_velocity =
                (state.vertical_velocity - gravity.acceleration * dt).max(-gravity.max_speed);
            let movement = self
                .move_box(world, id, Vec3::new(0.0, state.vertical_velocity * dt, 0.0))
                .with_context(|| format!("gravity on '{id}'"))?;
            state.grounded = movement
                .contact_normals
                .iter()
                .any(|normal| normal.y >= 0.5);
            let hit_ceiling = state.vertical_velocity > 0.0
                && movement
                    .contact_normals
                    .iter()
                    .any(|normal| normal.y <= -0.5);
            if state.grounded || hit_ceiling {
                state.vertical_velocity = 0.0;
            }
            world.insert(entity, state)?;
        }
        self.step_bodies(world, dt)
    }
}
