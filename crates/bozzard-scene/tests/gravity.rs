use bozzard_ecs::World;
use bozzard_scene::{Gravity, GravityState, Scene, Transform};
use glam::Vec3;

const EPSILON: f32 = 2e-4;

fn scene(json: &str) -> Scene {
    Scene::from_json(json).unwrap()
}

fn collision_scene(objects: &str) -> Scene {
    scene(&format!(
        r#"{{"version":1,"name":"gravity test","views":{{}},"objects":[{objects}]}}"#
    ))
}

fn center(instance: &bozzard_scene::SceneInstance, world: &World, id: &str) -> Vec3 {
    instance.global_transforms(world).unwrap()[id].transform_point3(Vec3::ZERO)
}

fn state<'a>(
    instance: &bozzard_scene::SceneInstance,
    world: &'a World,
    id: &str,
) -> &'a GravityState {
    world
        .get::<GravityState>(instance.entity(id).unwrap())
        .unwrap()
}

#[test]
fn gravity_free_fall_accelerates_downward_and_caps_speed() {
    assert_eq!(
        Gravity::default(),
        Gravity {
            enabled: true,
            acceleration: 9.81,
            max_speed: 50.0,
        }
    );
    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[0,10,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[1,1,1],"enabled":true},
          "gravity":{"enabled":true,"acceleration":10,"max_speed":3}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    instance.step_gravity(&mut world, 0.1).unwrap();
    assert!((state(&instance, &world, "mover").vertical_velocity + 1.0).abs() <= EPSILON);
    assert!(!state(&instance, &world, "mover").grounded);
    assert!((center(&instance, &world, "mover").y - 9.9).abs() <= EPSILON);

    instance.step_gravity(&mut world, 1.0).unwrap();
    assert!((state(&instance, &world, "mover").vertical_velocity + 3.0).abs() <= EPSILON);
    assert!(!state(&instance, &world, "mover").grounded);
}

#[test]
fn gravity_lands_and_stays_grounded_without_drifting() {
    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[0,2,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true},
          "gravity":{"enabled":true,"acceleration":10,"max_speed":50}
        },{
          "id":"floor", "name":"floor",
          "transform":{"translation":[0,-1,0],"rotation_degrees":[0,0,0],"scale":[20,1,20]},
          "collider":{"center":[0,0,0],"size":[1,2,1],"enabled":true}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    instance.step_gravity(&mut world, 1.0).unwrap();
    let landed_y = center(&instance, &world, "mover").y;
    assert!((landed_y - 1.0).abs() <= EPSILON);
    assert!(state(&instance, &world, "mover").grounded);
    assert_eq!(state(&instance, &world, "mover").vertical_velocity, 0.0);

    for _ in 0..20 {
        instance.step_gravity(&mut world, 1.0 / 60.0).unwrap();
        assert!(state(&instance, &world, "mover").grounded);
        assert_eq!(state(&instance, &world, "mover").vertical_velocity, 0.0);
        assert!((center(&instance, &world, "mover").y - landed_y).abs() <= EPSILON);
    }
}

#[test]
fn gravity_resumes_falling_after_the_object_moves_off_a_ledge() {
    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[0,1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true},
          "gravity":{"enabled":true,"acceleration":10,"max_speed":50}
        },{
          "id":"ledge", "name":"ledge",
          "transform":{"translation":[0,-1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    instance.step_gravity(&mut world, 0.1).unwrap();
    assert!(state(&instance, &world, "mover").grounded);
    instance
        .move_box(&mut world, "mover", Vec3::new(4.0, 0.0, 0.0))
        .unwrap();
    let before = center(&instance, &world, "mover").y;

    instance.step_gravity(&mut world, 0.1).unwrap();
    assert!(center(&instance, &world, "mover").y < before - EPSILON);
    assert!(state(&instance, &world, "mover").vertical_velocity < 0.0);
    assert!(!state(&instance, &world, "mover").grounded);
}

#[test]
fn disabled_gravity_leaves_the_transform_and_state_unchanged() {
    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[0,7,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[1,1,1],"enabled":true},
          "gravity":{"enabled":false,"acceleration":10,"max_speed":50}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    let before = center(&instance, &world, "mover");

    instance.step_gravity(&mut world, 1.0).unwrap();

    assert_eq!(center(&instance, &world, "mover"), before);
    assert_eq!(state(&instance, &world, "mover"), &GravityState::default());
}

#[test]
fn disabled_collider_pauses_gravity() {
    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[0,7,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[1,1,1],"enabled":false},
          "gravity":{"enabled":true,"acceleration":10,"max_speed":50}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    let before = center(&instance, &world, "mover");

    instance.step_gravity(&mut world, 1.0).unwrap();

    assert_eq!(center(&instance, &world, "mover"), before);
    assert_eq!(state(&instance, &world, "mover"), &GravityState::default());
}

#[test]
fn gravity_config_captures_but_runtime_velocity_starts_fresh_after_spawning() {
    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[0,10,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[1,1,1],"enabled":true},
          "gravity":{"enabled":true,"acceleration":12.5,"max_speed":7}
        }"#,
    );
    let mut first_world = World::new();
    let first = scene.spawn(&mut first_world).unwrap();
    first.step_gravity(&mut first_world, 0.1).unwrap();
    assert!(state(&first, &first_world, "mover").vertical_velocity < 0.0);

    let saved = first.capture(&first_world).unwrap();
    assert_eq!(
        saved.objects[0].gravity,
        Some(Gravity {
            enabled: true,
            acceleration: 12.5,
            max_speed: 7.0,
        })
    );
    let mut second_world = World::new();
    let second = saved.spawn(&mut second_world).unwrap();
    assert_eq!(
        state(&second, &second_world, "mover"),
        &GravityState::default()
    );
}

#[test]
fn gravity_rejects_invalid_configuration_and_step_delta() {
    for invalid in [
        r#"{"enabled":true,"acceleration":0,"max_speed":50}"#,
        r#"{"enabled":true,"acceleration":-1,"max_speed":50}"#,
        r#"{"enabled":true,"acceleration":10,"max_speed":0}"#,
    ] {
        let json = format!(
            r#"{{"version":1,"name":"bad","views":{{}},"objects":[{{"id":"mover","name":"mover","transform":{{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},"collider":{{"center":[0,0,0],"size":[1,1,1],"enabled":true}},"gravity":{invalid}}}]}}"#
        );
        assert!(Scene::from_json(&json).is_err());
    }
    assert!(
        Scene::from_json(
            r#"{
          "version":1,"name":"missing collider","views":{},"objects":[{
            "id":"mover","name":"mover",
            "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "gravity":{"enabled":true,"acceleration":10,"max_speed":50}
          }]
        }"#,
        )
        .is_err()
    );

    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[0,1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[1,1,1],"enabled":true},
          "gravity":{"enabled":true,"acceleration":10,"max_speed":50}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    let before = *world
        .get::<Transform>(instance.entity("mover").unwrap())
        .unwrap();
    for dt in [0.0, -0.1, f32::NAN] {
        assert!(instance.step_gravity(&mut world, dt).is_err());
        assert_eq!(
            *world
                .get::<Transform>(instance.entity("mover").unwrap())
                .unwrap(),
            before
        );
        assert_eq!(state(&instance, &world, "mover"), &GravityState::default());
    }
}
