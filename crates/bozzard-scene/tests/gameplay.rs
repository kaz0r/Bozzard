use bozzard_ecs::World;
use bozzard_scene::{
    GameplayInput, GameplayState, GravityState, Layer, PlayerController, Scene, SceneInstance,
    Transform,
};
use glam::Vec3;

fn scene() -> Scene {
    Scene::from_json(include_str!(
        "../../../examples/demo/scenes/first-trail.json"
    ))
    .unwrap()
}
fn setup() -> (SceneInstance, World) {
    let mut world = World::new();
    let instance = scene().spawn(&mut world).unwrap();
    (instance, world)
}
fn step(instance: &SceneInstance, world: &mut World, input: GameplayInput) {
    world.insert_resource(input);
    instance.gameplay_motion(world, 1.0 / 60.0).unwrap();
    instance.step_gravity(world, 1.0 / 60.0).unwrap();
    instance.gameplay_interactions(world).unwrap();
}
fn position(instance: &SceneInstance, world: &World, id: &str) -> Vec3 {
    Vec3::from(
        world
            .get::<Transform>(instance.entity(id).unwrap())
            .unwrap()
            .translation,
    )
}
fn teleport(instance: &SceneInstance, world: &mut World, pos: [f32; 3]) {
    world
        .get_mut::<Transform>(instance.entity("player").unwrap())
        .unwrap()
        .translation = pos;
}
#[test]
fn serialized_settings_defaults_and_invalid_references_are_transactional() {
    let scene = scene();
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    let minimal: PlayerController = serde_json::from_str(r#"{"camera":"camera"}"#).unwrap();
    assert_eq!(minimal.move_speed, 4.0);
    let old = Scene::from_json(include_str!(
        "../../../examples/demo/scenes/gravity-lab.json"
    ))
    .unwrap();
    assert!(
        old.objects
            .iter()
            .all(|o| o.player_controller.is_none() && o.trigger.is_none())
    );
    for bad in 0..7 {
        let mut candidate = scene.clone();
        let player = candidate
            .objects
            .iter_mut()
            .find(|o| o.id == "player")
            .unwrap();
        match bad {
            0 => player.player_controller.as_mut().unwrap().camera = "missing".into(),
            1 => player.player_controller.as_mut().unwrap().move_speed = f32::NAN,
            2 => player.player_controller.as_mut().unwrap().camera_distance = 0.0,
            3 => player.gravity = None,
            4 => player.parent = Some("trail".into()),
            5 => player.player_controller.as_mut().unwrap().fall_height = 5.0,
            _ => player.player_controller.as_mut().unwrap().camera = "player".into(),
        }
        let mut world = World::new();
        assert!(candidate.spawn(&mut world).is_err(), "case {bad}");
        assert_eq!(world.query::<Transform>().count(), 0);
    }
    let mut duplicate = scene.clone();
    let mut player = duplicate
        .objects
        .iter()
        .find(|o| o.id == "player")
        .unwrap()
        .clone();
    player.id = "second-player".into();
    duplicate.objects.push(player);
    assert!(duplicate.validate().is_err());
}
#[test]
fn camera_relative_motion_normalizes_diagonals_and_jump_is_grounded_edge() {
    let (instance, mut world) = setup();
    for _ in 0..20 {
        step(&instance, &mut world, GameplayInput::default());
    }
    let entity = instance.entity("player").unwrap();
    assert!(world.get::<GravityState>(entity).unwrap().grounded);
    let start = position(&instance, &world, "player");
    step(
        &instance,
        &mut world,
        GameplayInput {
            movement: [1.0, 1.0],
            ..Default::default()
        },
    );
    assert!((position(&instance, &world, "player").distance(start) - 4.0 / 60.0).abs() < 0.0001);
    world.resource_mut::<GameplayState>().unwrap().yaw = 90.0;
    let start = position(&instance, &world, "player");
    step(
        &instance,
        &mut world,
        GameplayInput {
            movement: [0.0, 1.0],
            jump: true,
            ..Default::default()
        },
    );
    let delta = position(&instance, &world, "player") - start;
    assert!(delta.x < -0.06 && delta.z.abs() < 0.0001 && delta.y > 0.0);
    let velocity = world.get::<GravityState>(entity).unwrap().vertical_velocity;
    step(
        &instance,
        &mut world,
        GameplayInput {
            jump: true,
            ..Default::default()
        },
    );
    assert!(world.get::<GravityState>(entity).unwrap().vertical_velocity < velocity);
    assert!(!world.resource::<GameplayInput>().unwrap().jump);
    for _ in 0..120 {
        step(&instance, &mut world, GameplayInput::default());
    }
    assert!(world.get::<GravityState>(entity).unwrap().grounded);
}
#[test]
fn camera_obstruction_clearance_rotation_and_disabled_colliders() {
    let (instance, mut world) = setup();
    let target = Vec3::new(0.0, 2.0, -11.0);
    let desired = Vec3::new(6.0, 2.0, -11.0);
    let hit = instance
        .obstructed_camera(&world, "player", target, desired, 0.3)
        .unwrap();
    assert!(hit.x > 2.8 && hit.x < 2.96, "{hit:?}");
    let entity = instance.entity("wall").unwrap();
    world.get_mut::<Transform>(entity).unwrap().rotation_degrees[1] = 30.0;
    let rotated = instance
        .obstructed_camera(&world, "player", target, desired, 0.3)
        .unwrap();
    assert!(rotated.x > 0.0 && rotated.x < 3.5);
    world
        .get_mut::<bozzard_scene::BoxCollider>(entity)
        .unwrap()
        .enabled = false;
    assert_eq!(
        instance
            .obstructed_camera(&world, "player", target, desired, 0.3)
            .unwrap(),
        desired
    );
    let inside = Vec3::new(0.0, -0.5, 0.0);
    assert_eq!(
        instance
            .obstructed_camera(&world, "player", inside, inside + Vec3::Z, 0.3)
            .unwrap(),
        inside
    );
}
#[test]
fn collection_once_goal_gating_checkpoints_and_fall_reset_velocity_not_progress() {
    let (instance, mut world) = setup();
    teleport(&instance, &mut world, [0.0, 0.65, -18.0]);
    instance.gameplay_interactions(&mut world).unwrap();
    assert!(!world.resource::<GameplayState>().unwrap().won);
    teleport(&instance, &mut world, [0.0, 0.65, 0.0]);
    let visible = instance
        .view(&world, Layer::ThreeD, 1.0)
        .unwrap()
        .objects
        .len();
    for _ in 0..3 {
        instance.gameplay_interactions(&mut world).unwrap();
    }
    assert_eq!(
        world.resource::<GameplayState>().unwrap().collected.len(),
        1
    );
    assert_eq!(
        instance
            .view(&world, Layer::ThreeD, 1.0)
            .unwrap()
            .objects
            .len(),
        visible - 1
    );
    teleport(&instance, &mut world, [0.0, 0.65, -9.0]);
    instance.gameplay_interactions(&mut world).unwrap();
    assert_eq!(
        world
            .resource::<GameplayState>()
            .unwrap()
            .checkpoint
            .as_deref(),
        Some("checkpoint")
    );
    teleport(&instance, &mut world, [0.0, -20.0, 0.0]);
    world
        .insert(
            instance.entity("player").unwrap(),
            GravityState {
                vertical_velocity: -30.0,
                grounded: false,
            },
        )
        .unwrap();
    instance.gameplay_interactions(&mut world).unwrap();
    assert_eq!(
        position(&instance, &world, "player"),
        Vec3::new(0.0, 0.65, -9.0)
    );
    assert_eq!(
        *world
            .get::<GravityState>(instance.entity("player").unwrap())
            .unwrap(),
        GravityState::default()
    );
    let state = world.resource::<GameplayState>().unwrap();
    assert_eq!(state.collected.len(), 1);
    assert_eq!(state.respawns, 1);
    for pos in [[0.0, 1.4, -6.0], [0.0, 0.65, -14.0], [0.0, 0.65, -18.0]] {
        teleport(&instance, &mut world, pos);
        instance.gameplay_interactions(&mut world).unwrap();
    }
    assert!(world.resource::<GameplayState>().unwrap().won);
    // Capturing never serializes run progress or hides authored collectible drawables.
    let captured = instance.capture(&world).unwrap();
    assert!(
        captured
            .objects
            .iter()
            .find(|o| o.id == "gold-1")
            .unwrap()
            .drawable
            .is_some()
    );
    assert!(!captured.to_json().unwrap().contains("collected"));
}
#[test]
fn first_trail_is_reachable_with_only_forward_and_one_grounded_jump() {
    let (instance, mut world) = setup();
    for tick in 0..340 {
        step(
            &instance,
            &mut world,
            GameplayInput {
                movement: [0.0, 1.0],
                jump: tick == 80,
                ..Default::default()
            },
        );
    }
    let state = world.resource::<GameplayState>().unwrap();
    assert!(
        state.won,
        "{} position={:?}",
        state.feedback(),
        position(&instance, &world, "player")
    );
    assert_eq!(state.collected.len(), 3);
    assert_eq!(state.respawns, 0);
    assert_eq!(state.checkpoint.as_deref(), Some("checkpoint"));
}

#[test]
fn unsafe_spawns_and_solid_triggers_are_rejected_disabled_collectibles_are_optional() {
    let mut document = scene();
    let checkpoint = document
        .objects
        .iter_mut()
        .find(|o| o.id == "checkpoint")
        .unwrap();
    checkpoint.trigger.as_mut().unwrap().action = bozzard_scene::TriggerAction::Checkpoint {
        respawn: [3.5, 1.0, -11.0],
    };
    assert!(
        document
            .validate()
            .unwrap_err()
            .to_string()
            .contains("intersects")
    );
    checkpoint_spawn_below_fall_is_invalid();
    let mut document = scene();
    document
        .objects
        .iter_mut()
        .find(|o| o.id == "gold-1")
        .unwrap()
        .collider = Some(Default::default());
    assert!(document.validate().is_err());
    let mut document = scene();
    document
        .objects
        .iter_mut()
        .find(|o| o.id == "gold-1")
        .unwrap()
        .trigger
        .as_mut()
        .unwrap()
        .volume
        .enabled = false;
    let mut world = World::new();
    let instance = document.spawn(&mut world).unwrap();
    assert_eq!(world.resource::<GameplayState>().unwrap().total, 2);
    teleport(&instance, &mut world, [0.0, 0.65, 0.0]);
    instance.gameplay_interactions(&mut world).unwrap();
    assert!(
        world
            .resource::<GameplayState>()
            .unwrap()
            .collected
            .is_empty()
    );
    teleport(&instance, &mut world, [10.0, -9.0, 0.0]);
    instance.gameplay_interactions(&mut world).unwrap();
    assert_eq!(
        position(&instance, &world, "player"),
        Vec3::new(0.0, 0.65, 2.0)
    );
    fn checkpoint_spawn_below_fall_is_invalid() {
        let mut document = scene();
        document
            .objects
            .iter_mut()
            .find(|o| o.id == "checkpoint")
            .unwrap()
            .trigger
            .as_mut()
            .unwrap()
            .action = bozzard_scene::TriggerAction::Checkpoint {
            respawn: [0.0, -20.0, 0.0],
        };
        assert!(document.validate().is_err());
    }
}
