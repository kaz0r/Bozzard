//! Compound colliders and per-body drag / gravity scale.
use bozzard_ecs::World;
use bozzard_scene::{GravityState, Scene};
use glam::Vec3;

fn scene(objects: &str) -> Scene {
    Scene::from_json(&format!(
        r#"{{"version":1,"name":"compound","views":{{}},"objects":[{objects}]}}"#
    ))
    .unwrap()
}

fn center(instance: &bozzard_scene::SceneInstance, world: &World, id: &str) -> Vec3 {
    instance.global_transforms(world).unwrap()[id].transform_point3(Vec3::ZERO)
}

#[test]
fn a_rigidbody_carries_its_child_colliders_as_one_compound_body() {
    // Only the child's shape can reach the floor, so the body rests if and only if the solver used it.
    let scene = scene(
        r#"{"id":"body","name":"body",
            "transform":{"translation":[0,4,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "collider":{"center":[0,1,0],"size":[1,1,1]},
            "gravity":{"enabled":true,"acceleration":10}},
          {"id":"arm","name":"arm","parent":"body",
            "transform":{"translation":[0,-1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "collider":{"center":[0,0,0],"size":[1,1,1]}},
          {"id":"floor","name":"floor",
            "transform":{"translation":[0,-0.5,0],"rotation_degrees":[0,0,0],"scale":[20,1,20]},
            "collider":{"size":[1,1,1]}}"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    for _ in 0..240 {
        instance.step_gravity(&mut world, 1. / 60.).unwrap();
    }
    assert_eq!(
        instance.physics_body_count(&world),
        2,
        "one body for the Rigidbody and its child, plus the static floor"
    );
    let settled = center(&instance, &world, "body").y;
    assert!(
        (settled - 1.5).abs() < 0.05,
        "the child shape should hold the body at 1.5, not {settled}"
    );
    assert!(
        world
            .get::<GravityState>(instance.entity("body").unwrap())
            .unwrap()
            .grounded
    );
}

#[test]
fn a_child_rigidbody_stays_its_own_body() {
    let scene = scene(
        r#"{"id":"body","name":"body",
            "transform":{"translation":[0,4,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "collider":{"size":[1,1,1]},
            "gravity":{"enabled":false}},
          {"id":"tip","name":"tip","parent":"body",
            "transform":{"translation":[0,-1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "collider":{"size":[1,1,1]},
            "gravity":{"enabled":true,"acceleration":10}},
          {"id":"floor","name":"floor",
            "transform":{"translation":[0,-0.5,0],"rotation_degrees":[0,0,0],"scale":[20,1,20]},
            "collider":{"size":[1,1,1]}}"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    for _ in 0..240 {
        instance.step_gravity(&mut world, 1. / 60.).unwrap();
    }
    assert_eq!(
        instance.physics_body_count(&world),
        3,
        "a nested Rigidbody is a separate body: root, tip and the floor"
    );
}

#[test]
fn gravity_scale_zero_floats_and_linear_drag_slows_the_fall() {
    let scene = scene(
        r#"{"id":"float","name":"float",
            "transform":{"translation":[0,3,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "collider":{"size":[1,1,1]},
            "gravity":{"enabled":true,"acceleration":10,"gravity_scale":0}},
          {"id":"damped","name":"damped",
            "transform":{"translation":[4,3,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "collider":{"size":[1,1,1]},
            "gravity":{"enabled":true,"acceleration":10,"linear_damping":20}},
          {"id":"free","name":"free",
            "transform":{"translation":[8,3,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "collider":{"size":[1,1,1]},
            "gravity":{"enabled":true,"acceleration":10}}"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    for _ in 0..30 {
        instance.step_gravity(&mut world, 1. / 60.).unwrap();
    }
    assert!(
        (center(&instance, &world, "float").y - 3.0).abs() < 0.01,
        "gravity scale 0 should not fall"
    );
    let damped = center(&instance, &world, "damped").y;
    let free = center(&instance, &world, "free").y;
    assert!(
        damped > free + 0.1,
        "linear drag should slow the fall: damped {damped}, free {free}"
    );
}
