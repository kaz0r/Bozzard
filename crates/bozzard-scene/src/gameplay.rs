//! Headless, fixed-step single-player kinematic gameplay. Authored settings are separate
//! from input and progress; spawning a new world resets the entire run.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerController {
    pub camera: String,
    pub move_speed: f32,
    pub jump_speed: f32,
    pub camera_distance: f32,
    pub camera_height: f32,
    pub camera_radius: f32,
    pub orbit_sensitivity: f32,
    pub fall_height: f32,
}
impl Default for PlayerController {
    fn default() -> Self {
        Self {
            camera: String::new(),
            move_speed: 4.0,
            jump_speed: 6.0,
            camera_distance: 6.0,
            camera_height: 1.0,
            camera_radius: 0.3,
            orbit_sensitivity: 0.2,
            fall_height: -10.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerAction {
    /// Reports Blueprint contacts without built-in gameplay effects.
    Sensor,
    Collectible,
    Checkpoint {
        respawn: [f32; 3],
    },
    Goal,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    #[serde(default)]
    pub volume: BoxCollider,
    pub action: TriggerAction,
}
impl Default for Trigger {
    fn default() -> Self {
        Self {
            volume: BoxCollider::default(),
            action: TriggerAction::Collectible,
        }
    }
}
/// Movement is right/forward in camera yaw space. Jump/orbit are queued edges/deltas,
/// consumed once even when a render frame advances multiple simulation ticks.
#[derive(Clone, Copy, Debug, Default)]
pub struct GameplayInput {
    pub movement: [f32; 2],
    pub jump: bool,
    pub orbit: [f32; 2],
}
#[derive(Clone, Debug)]
pub struct GameplayState {
    pub player: String,
    pub collected: BTreeSet<String>,
    pub total: usize,
    pub checkpoint: Option<String>,
    pub respawn: [f32; 3],
    pub respawns: u32,
    pub won: bool,
    pub yaw: f32,
    pub pitch: f32,
}
impl GameplayState {
    pub fn feedback(&self) -> String {
        format!(
            "{} · {}/{} collected · checkpoint: {} · respawns: {}",
            if self.won {
                "YOU WIN! R: restart (editor: Stop / Play)"
            } else {
                "Collect all gold, then reach the green goal"
            },
            self.collected.len(),
            self.total,
            self.checkpoint.as_deref().unwrap_or("start"),
            self.respawns
        )
    }
}

pub(super) fn validate(scene: &Scene) -> Result<()> {
    let players: Vec<_> = scene
        .objects
        .iter()
        .filter(|o| o.player_controller.is_some())
        .collect();
    ensure!(
        players.len() <= 1,
        "only one Player Controller is supported per scene"
    );
    for object in &scene.objects {
        if let Some(trigger) = &object.trigger {
            trigger.volume.validate()?;
            ensure!(
                object.collider.is_none()
                    && object.gravity.is_none()
                    && object.player_controller.is_none(),
                "trigger '{}' must not also be a solid collider, gravity body or player",
                object.id
            );
            if let TriggerAction::Checkpoint { respawn } = trigger.action {
                ensure!(
                    respawn.iter().all(|v| v.is_finite()),
                    "checkpoint respawn must be finite"
                );
                if let Some(player) = players.first() {
                    ensure!(
                        respawn[1] > player.player_controller.as_ref().unwrap().fall_height,
                        "checkpoint respawn must be above the player's fall height"
                    );
                }
            }
        }
    }
    for player in players {
        let config = player.player_controller.as_ref().unwrap();
        for (name, value) in [
            ("move speed", config.move_speed),
            ("jump speed", config.jump_speed),
            ("camera distance", config.camera_distance),
            ("camera radius", config.camera_radius),
            ("orbit sensitivity", config.orbit_sensitivity),
        ] {
            ensure!(
                value.is_finite() && (0.001..=1000.0).contains(&value),
                "player {name} must be in 0.001..=1000"
            );
        }
        ensure!(
            config.camera_height.is_finite() && (0.0..=1000.0).contains(&config.camera_height),
            "camera height must be in 0..=1000"
        );
        ensure!(
            config.fall_height.is_finite() && player.transform.translation[1] > config.fall_height,
            "player start must be above a finite fall height"
        );
        ensure!(
            player.parent.is_none() && player.spin.is_none(),
            "Player Controller needs a root object without Spin"
        );
        ensure!(
            player.collider.is_some_and(|c| c.enabled) && player.gravity.is_some_and(|g| g.enabled),
            "Player Controller needs enabled Box collider and Gravity"
        );
        let camera = scene
            .objects
            .iter()
            .find(|o| o.id == config.camera)
            .context("Player Controller camera reference is missing")?;
        ensure!(
            camera.id != player.id
                && camera.parent.is_none()
                && camera.spin.is_none()
                && camera.gravity.is_none()
                && camera.collider.is_none()
                && camera.trigger.is_none()
                && matches!(camera.camera, Some(Camera::Perspective { .. })),
            "follow camera needs a separate root perspective camera without Spin/Gravity/collider/trigger"
        );
        ensure!(
            scene.views.get(&Layer::ThreeD) == Some(&config.camera),
            "follow camera must be the active 3D camera"
        );
        for object in &scene.objects {
            let mut parent = object.parent.as_deref();
            // The hierarchy is validated separately; bound traversal even for invalid documents.
            for _ in 0..scene.objects.len() {
                let Some(id) = parent else { break };
                ensure!(
                    id != player.id || !object.collider.is_some_and(|c| c.enabled),
                    "Player Controller cannot carry child colliders"
                );
                parent = scene
                    .objects
                    .iter()
                    .find(|o| o.id == id)
                    .and_then(|o| o.parent.as_deref());
            }
        }
    }
    Ok(())
}
/// Reject obviously unsafe authored spawn points before Play; runtime-moving obstacles
/// can still change their surroundings, so respawn retains normal collision recovery.
pub(super) fn validate_respawns(scene: &Scene, matrices: &BTreeMap<&str, Mat4>) -> Result<()> {
    let Some(player) = scene.objects.iter().find(|o| o.player_controller.is_some()) else {
        return Ok(());
    };
    // Geometry only: entity handles are not used by the SAT query.
    let mut world = World::new();
    let entity = world.spawn();
    let make_box = |id: &str, collider: BoxCollider, matrix: Mat4| -> Result<CollisionBox> {
        let (center, edges, corners) = collider.geometry(matrix)?;
        Ok(CollisionBox {
            id: id.into(),
            entity,
            center,
            edges,
            corners,
        })
    };
    let solids: Vec<_> = scene
        .objects
        .iter()
        .filter(|o| o.id != player.id)
        .filter_map(|o| {
            o.collider
                .filter(|c| c.enabled)
                .map(|c| make_box(&o.id, c, matrices[o.id.as_str()]))
        })
        .collect::<Result<_>>()?;
    let mut points = vec![(player.id.as_str(), player.transform.translation)];
    for object in &scene.objects {
        if let Some(Trigger {
            volume,
            action: TriggerAction::Checkpoint { respawn },
        }) = &object.trigger
            && volume.enabled
        {
            points.push((object.id.as_str(), *respawn));
        }
    }
    for (id, point) in points {
        let mut transform = player.transform;
        transform.translation = point;
        let bounds = make_box(&player.id, player.collider.unwrap(), transform.matrix())?;
        ensure!(
            !solids.iter().any(|solid| bounds.penetrates(solid)),
            "spawn point '{id}' intersects a solid collider; move it to a clear position"
        );
    }
    Ok(())
}

impl SceneInstance {
    pub fn initialize_gameplay(&self, world: &mut World) {
        world.insert_resource(GameplayInput::default());
        if let Some(player) = self
            .document
            .objects
            .iter()
            .find(|o| o.player_controller.is_some())
        {
            let config = player.player_controller.as_ref().unwrap();
            let camera = self
                .document
                .objects
                .iter()
                .find(|o| o.id == config.camera)
                .unwrap();
            world.insert_resource(GameplayState {
                player: player.id.clone(),
                collected: BTreeSet::new(),
                total: self
                    .document
                    .objects
                    .iter()
                    .filter(|o| {
                        o.trigger.as_ref().is_some_and(|t| {
                            t.volume.enabled && matches!(t.action, TriggerAction::Collectible)
                        })
                    })
                    .count(),
                checkpoint: None,
                respawn: player.transform.translation,
                respawns: 0,
                won: false,
                yaw: camera.transform.rotation_degrees[1],
                pitch: camera.transform.rotation_degrees[0].clamp(-70.0, 10.0),
            });
        }
    }
    pub fn gameplay_motion(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt > 0.0, "invalid gameplay timestep");
        let Some(mut state) = world.resource::<GameplayState>().cloned() else {
            return Ok(());
        };
        let input = world
            .resource::<GameplayInput>()
            .copied()
            .unwrap_or_default();
        ensure!(
            input
                .movement
                .iter()
                .chain(&input.orbit)
                .all(|v| v.is_finite()),
            "gameplay input must be finite"
        );
        world.insert_resource(GameplayInput {
            movement: input.movement,
            ..Default::default()
        });
        let entity = self.entity(&state.player).context("player missing")?;
        let config = world
            .get::<PlayerController>(entity)
            .context("Player Controller removed")?
            .clone();
        state.yaw = (state.yaw - input.orbit[0] * config.orbit_sensitivity).rem_euclid(360.0);
        state.pitch = (state.pitch - input.orbit[1] * config.orbit_sensitivity).clamp(-70.0, 10.0);
        if !state.won {
            let direction =
                Vec3::new(input.movement[0], 0.0, -input.movement[1]).clamp_length_max(1.0);
            let delta =
                Quat::from_rotation_y(state.yaw.to_radians()) * direction * config.move_speed * dt;
            if delta != Vec3::ZERO {
                self.move_box(world, &state.player, delta)?;
            }
            if input.jump {
                self.jump_box(world, &state.player, config.jump_speed)?;
            }
        }
        world.insert_resource(state);
        Ok(())
    }
    pub fn gameplay_interactions(&self, world: &mut World) -> Result<()> {
        let Some(mut state) = world.resource::<GameplayState>().cloned() else {
            return Ok(());
        };
        let entity = self.entity(&state.player).context("player missing")?;
        let config = world
            .get::<PlayerController>(entity)
            .context("Player Controller removed")?
            .clone();
        if world
            .get::<Transform>(entity)
            .context("player transform missing")?
            .translation[1]
            < config.fall_height
        {
            world.get_mut::<Transform>(entity).unwrap().translation = state.respawn;
            world.insert(entity, GravityState::default())?;
            state.respawns = state.respawns.saturating_add(1);
        }
        let matrices = self.global_transforms(world)?;
        let player_box = self
            .collisions(world)?
            .boxes
            .into_iter()
            .find(|b| b.id == state.player)
            .context("player collider missing")?;
        let mut at_goal = false;
        if !state.won {
            for (id, &trigger_entity) in &self.entities {
                let Some(trigger) = world.get::<Trigger>(trigger_entity) else {
                    continue;
                };
                if !trigger.volume.enabled {
                    continue;
                }
                let (center, edges, corners) = trigger.volume.geometry(matrices[id])?;
                let volume = CollisionBox {
                    id: id.clone(),
                    entity: trigger_entity,
                    center,
                    edges,
                    corners,
                };
                if !player_box.intersects(&volume) {
                    continue;
                }
                match &trigger.action {
                    TriggerAction::Collectible => {
                        state.collected.insert(id.clone());
                    }
                    TriggerAction::Checkpoint { respawn } => {
                        state.checkpoint = Some(id.clone());
                        state.respawn = *respawn;
                    }
                    TriggerAction::Goal => at_goal = true,
                    TriggerAction::Sensor => {}
                }
            }
            state.won = at_goal && state.collected.len() == state.total;
        }
        let target =
            matrices[&state.player].transform_point3(Vec3::ZERO) + Vec3::Y * config.camera_height;
        let rotation = Quat::from_euler(
            EulerRot::YXZ,
            state.yaw.to_radians(),
            state.pitch.to_radians(),
            0.0,
        );
        let desired = target + rotation * Vec3::Z * config.camera_distance;
        let camera = self
            .entity(&config.camera)
            .context("follow camera missing")?;
        let near_radius = match world
            .get::<Camera>(camera)
            .context("follow camera component removed")?
        {
            Camera::Perspective {
                near,
                vertical_fov_degrees,
                ..
            } => near * (vertical_fov_degrees.to_radians() * 0.5).tan() * 2.5 + near,
            _ => config.camera_radius,
        };
        let position = self.obstructed_camera(
            world,
            &state.player,
            target,
            desired,
            config.camera_radius.max(near_radius),
        )?;
        let transform = world
            .get_mut::<Transform>(camera)
            .context("follow camera transform missing")?;
        transform.translation = position.to_array();
        transform.rotation_degrees = [state.pitch, state.yaw, 0.0];
        transform.scale = [1.0; 3];
        world.insert_resource(state);
        Ok(())
    }
    /// Conservative sphere-vs-transformed-box segment test. Expanded local slabs also
    /// handle nonuniform scale/shear; corner clearance may pull in earlier than necessary.
    pub fn obstructed_camera(
        &self,
        world: &World,
        player: &str,
        target: Vec3,
        desired: Vec3,
        radius: f32,
    ) -> Result<Vec3> {
        ensure!(
            target.is_finite() && desired.is_finite() && radius.is_finite() && radius > 0.0,
            "invalid camera probe"
        );
        let matrices = self.global_transforms(world)?;
        let mut fraction = 1.0_f32;
        for (id, &entity) in &self.entities {
            if id == player {
                continue;
            }
            let Some(collider) = world.get::<BoxCollider>(entity).filter(|c| c.enabled) else {
                continue;
            };
            let inverse = matrices[id].inverse();
            let origin = inverse.transform_point3(target) - Vec3::from(collider.center);
            let delta = inverse.transform_vector3(desired - target);
            let padding = Vec3::new(
                inverse.row(0).truncate().length(),
                inverse.row(1).truncate().length(),
                inverse.row(2).truncate().length(),
            ) * radius;
            let extent = Vec3::from(collider.size) * 0.5 + padding;
            let (mut enter, mut exit) = (0.0_f32, 1.0_f32);
            for axis in 0..3 {
                if delta[axis].abs() < 1e-8 {
                    if origin[axis].abs() > extent[axis] {
                        exit = -1.0;
                        break;
                    }
                } else {
                    let a = (-extent[axis] - origin[axis]) / delta[axis];
                    let b = (extent[axis] - origin[axis]) / delta[axis];
                    enter = enter.max(a.min(b));
                    exit = exit.min(a.max(b));
                }
            }
            if enter <= exit {
                fraction = fraction.min((enter - 0.001).max(0.0));
            }
        }
        Ok(target.lerp(desired, fraction))
    }
}
