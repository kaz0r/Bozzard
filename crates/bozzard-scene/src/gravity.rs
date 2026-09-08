//! Fixed-step world-down acceleration for individual swept box movers.
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
}
impl Default for Gravity {
    fn default() -> Self {
        Self {
            enabled: true,
            acceleration: 9.81,
            max_speed: 50.0,
            jump_speed: 5.0,
        }
    }
}
impl Gravity {
    pub fn validate(&self) -> Result<()> {
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
            || !world.get::<BoxCollider>(entity).is_some_and(|c| c.enabled)
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
        Ok(true)
    }

    /// Sequential kinematic gravity in stable object-ID order. No dynamic impulses.
    /// Disabled gravity/colliders reset velocity. Errors leave the failing body's move unapplied;
    /// previously stepped bodies in the same tick are not rolled back.
    pub fn step_gravity(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(
            dt.is_finite() && dt > 0.0,
            "gravity timestep must be positive and finite"
        );
        for (id, &entity) in &self.entities {
            let Some(gravity) = world.get::<Gravity>(entity).copied() else {
                if world.get::<GravityState>(entity).is_some() {
                    world.insert(entity, GravityState::default())?;
                }
                continue;
            };
            gravity.validate()?;
            if !gravity.enabled || world.get::<BoxCollider>(entity).is_some_and(|c| !c.enabled) {
                world.insert(entity, GravityState::default())?;
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
        Ok(())
    }
}
