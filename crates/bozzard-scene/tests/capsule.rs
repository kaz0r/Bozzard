//! The Player Controller is a Rapier capsule character controller.
use bozzard_ecs::World;
use bozzard_scene::{GameplayInput, GravityState, Scene, SceneInstance, Transform};
use glam::Vec3;

const PLAYER: &str = r#"{"id":"player","name":"player",
    "transform":{"translation":[0,1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
    "collider":{"size":[0.8,1.2,0.8]},
    "gravity":{"enabled":true,"acceleration":10},
    "player_controller":{"camera":"camera","capsule_radius":0.4,"capsule_height":1.2,
        "move_speed":4,"jump_speed":6,"step_height":0.35,"slope_limit_degrees":40,"fall_height":-20}}"#;

const CAMERA: &str = r#"{"id":"camera","name":"camera",
    "transform":{"translation":[0,4,6],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
    "camera":{"projection":"perspective","vertical_fov_degrees":55,"near":0.1,"far":100}}"#;

const FLOOR: &str = r#"{"id":"floor","name":"floor",
    "transform":{"translation":[0,-0.5,0],"rotation_degrees":[0,0,0],"scale":[40,1,40]},
    "collider":{"size":[1,1,1]}}"#;

fn scene(extra: &[&str]) -> Scene {
    scene_with_player(PLAYER, extra)
}

fn scene_with_player(player: &str, extra: &[&str]) -> Scene {
    let objects = [player, CAMERA, FLOOR]
        .into_iter()
        .chain(extra.iter().copied())
        .collect::<Vec<_>>()
        .join(",");
    Scene::from_json(&format!(
        r#"{{"version":1,"name":"capsule","views":{{"3d":"camera"}},"objects":[{objects}]}}"#
    ))
    .unwrap()
}

fn position(instance: &SceneInstance, world: &World, id: &str) -> Vec3 {
    Vec3::from(
        world
            .get::<Transform>(instance.entity(id).unwrap())
            .unwrap()
            .translation,
    )
}

fn run(scene: &Scene, ticks: usize, input: GameplayInput) -> (World, SceneInstance) {
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    for _ in 0..ticks {
        world.insert_resource(input);
        instance.gameplay_motion(&mut world, 1.0 / 60.0).unwrap();
        instance.step_gravity(&mut world, 1.0 / 60.0).unwrap();
    }
    (world, instance)
}

fn forward() -> GameplayInput {
    GameplayInput {
        movement: [0.0, 1.0],
        ..Default::default()
    }
}

#[test]
fn the_capsule_steps_over_a_low_obstacle_without_jumping() {
    let step = r#"{"id":"step","name":"step",
        "transform":{"translation":[0,0.15,-2],"rotation_degrees":[0,0,0],"scale":[3,0.3,2]},
        "collider":{"size":[1,1,1]}}"#;
    let (world, instance) = run(&scene(&[step]), 45, forward());
    let position = position(&instance, &world, "player");
    assert!(
        position.y > 0.85 && position.z < -1.0 && position.z > -3.5,
        "the capsule should stand on the 0.3 m step, got {position:?}"
    );
    assert!(
        world
            .get::<GravityState>(instance.entity("player").unwrap())
            .unwrap()
            .grounded
    );
}

#[test]
fn a_wall_taller_than_the_step_height_blocks_the_capsule() {
    let wall = r#"{"id":"wall","name":"wall",
        "transform":{"translation":[0,1,-2],"rotation_degrees":[0,0,0],"scale":[3,2,0.4]},
        "collider":{"size":[1,1,1]}}"#;
    let (world, instance) = run(&scene(&[wall]), 150, forward());
    let position = position(&instance, &world, "player");
    assert!(
        position.z > -1.4,
        "the capsule should stop at the wall, got {position:?}"
    );
    assert!(position.y < 0.7, "the capsule should stay on the floor");
}

#[test]
fn a_jump_clears_an_obstacle_the_step_cannot() {
    let wall = r#"{"id":"wall","name":"wall",
        "transform":{"translation":[0,0.5,-2],"rotation_degrees":[0,0,0],"scale":[3,1,0.4]},
        "collider":{"size":[1,1,1]}}"#;
    let mut world = World::new();
    let instance = scene(&[wall]).spawn(&mut world).unwrap();
    for tick in 0..200 {
        world.insert_resource(GameplayInput {
            movement: [0.0, 1.0],
            jump: tick == 20,
            ..Default::default()
        });
        instance.gameplay_motion(&mut world, 1.0 / 60.0).unwrap();
        instance.step_gravity(&mut world, 1.0 / 60.0).unwrap();
    }
    assert!(
        position(&instance, &world, "player").z < -2.5,
        "one jump should carry the capsule over the 1 m wall, got {:?}",
        position(&instance, &world, "player")
    );
}

#[test]
fn the_capsule_rides_a_teleported_platform() {
    // A raised platform the player spawns on, above the floor so it is the only ground contact.
    let platform = r#"{"id":"platform","name":"platform",
        "transform":{"translation":[0,1,0],"rotation_degrees":[0,0,0],"scale":[4,1,4]},
        "collider":{"size":[1,1,1]}}"#;
    let document = scene_with_player(&PLAYER.replace("[0,1,0]", "[0,2.2,0]"), &[platform]);
    let mut world = World::new();
    let instance = document.spawn(&mut world).unwrap();
    for _ in 0..30 {
        world.insert_resource(GameplayInput::default());
        instance.gameplay_motion(&mut world, 1.0 / 60.0).unwrap();
        instance.step_gravity(&mut world, 1.0 / 60.0).unwrap();
    }
    assert!(
        world
            .get::<GravityState>(instance.entity("player").unwrap())
            .unwrap()
            .grounded
    );
    let start = position(&instance, &world, "player").y;
    for _ in 0..60 {
        world
            .get_mut::<Transform>(instance.entity("platform").unwrap())
            .unwrap()
            .translation[1] += 0.01;
        world.insert_resource(GameplayInput::default());
        instance.gameplay_motion(&mut world, 1.0 / 60.0).unwrap();
        instance.step_gravity(&mut world, 1.0 / 60.0).unwrap();
    }
    let lifted = position(&instance, &world, "player").y;
    assert!(
        lifted > start + 0.4,
        "the player should ride the rising platform: {start} -> {lifted}"
    );
}
