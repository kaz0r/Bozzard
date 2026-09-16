//! Authored joints between two Rigidbodies.
use bozzard_ecs::World;
use bozzard_scene::{Joint, JointKind, Scene};
use glam::Vec3;

fn scene(objects: &str) -> Scene {
    Scene::from_json(&format!(
        r#"{{"version":1,"name":"joints","views":{{}},"objects":[{objects}]}}"#
    ))
    .unwrap()
}

fn anchor() -> &'static str {
    r#"{"id":"anchor","name":"anchor",
        "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
        "collider":{"size":[1,1,1]},"gravity":{"enabled":false}}"#
}

fn body(translation: &str, joint: &str) -> String {
    format!(
        r#"{{"id":"body","name":"body",
            "transform":{{"translation":{translation},"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
            "collider":{{"size":[1,1,1]}},"gravity":{{"enabled":true,"acceleration":10}},
            "joint":{joint}}}"#
    )
}

fn center(instance: &bozzard_scene::SceneInstance, world: &World, id: &str) -> Vec3 {
    instance.global_transforms(world).unwrap()[id].transform_point3(Vec3::ZERO)
}

fn settle(scene: &Scene, steps: usize) -> (World, bozzard_scene::SceneInstance) {
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    for _ in 0..steps {
        instance.step_gravity(&mut world, 1. / 60.).unwrap();
    }
    (world, instance)
}

#[test]
fn a_fixed_joint_holds_a_dynamic_body_in_place() {
    let held = scene(&format!(
        "{},{}",
        body("[2,3,0]", r#"{"other":"anchor","kind":"fixed"}"#),
        anchor()
    ));
    let (world, instance) = settle(&held, 180);
    let position = center(&instance, &world, "body");
    assert!(
        (position - Vec3::new(2.0, 3.0, 0.0)).length() < 0.05,
        "the fixed joint should hold the body at its authored pose, got {position:?}"
    );

    // Control: without the joint the same body falls to the floor.
    let free = scene(&format!(
        r#"{{"id":"body","name":"body",
            "transform":{{"translation":[2,3,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
            "collider":{{"size":[1,1,1]}},"gravity":{{"enabled":true,"acceleration":10}}}},
          {}"#,
        anchor()
    ));
    let (world, instance) = settle(&free, 180);
    assert!(
        center(&instance, &world, "body").y < 1.0,
        "the control body should fall"
    );
}

#[test]
fn a_hinge_keeps_the_bodies_linked_while_they_swing() {
    let hinge = scene(&format!(
        "{},{}",
        body(
            "[2,0,0]",
            r#"{"other":"anchor","kind":"revolute","anchor":[-2,0,0],"axis":[0,0,1],"other_axis":[0,0,1]}"#
        ),
        anchor()
    ));
    let (world, instance) = settle(&hinge, 180);
    let position = center(&instance, &world, "body");
    assert!(
        (position.length() - 2.0).abs() < 0.15,
        "the hinge should hold the 2 m link, got {position:?}"
    );
    assert!(
        position.y < -0.5,
        "gravity should swing the link down, got {position:?}"
    );
}

#[test]
fn a_slider_locks_every_axis_but_its_own() {
    let slider = scene(&format!(
        "{},{}",
        body(
            "[2,0,0]",
            r#"{"other":"anchor","kind":"prismatic","axis":[1,0,0],"other_axis":[1,0,0]}"#
        ),
        anchor()
    ));
    let (world, instance) = settle(&slider, 180);
    let position = center(&instance, &world, "body");
    assert!(
        position.y.abs() < 0.05 && position.z.abs() < 0.05,
        "a slider along X must not fall, got {position:?}"
    );
}

#[test]
fn a_rope_stops_a_falling_body_at_its_max_distance() {
    let rope = scene(&format!(
        "{},{}",
        body(
            "[0,3,0]",
            r#"{"other":"anchor","kind":"rope","max_limit":2}"#
        ),
        anchor()
    ));
    let (world, instance) = settle(&rope, 300);
    let position = center(&instance, &world, "body");
    assert!(
        position.length() < 2.2,
        "the rope should stop the fall at 2 m, got {position:?}"
    );
    assert!(
        position.length() > 1.8,
        "gravity should pull the rope taut, got {position:?}"
    );
}

#[test]
fn joint_authoring_errors_fail_loudly() {
    let raw = |joint: &str| {
        format!(
            r#"{{"version":1,"name":"joints","views":{{}},"objects":[{},{}]}}"#,
            body("[2,3,0]", joint),
            anchor()
        )
    };
    assert!(Scene::from_json(&raw(r#"{"other":"nope","kind":"fixed"}"#)).is_err());
    assert!(Scene::from_json(&raw(r#"{"other":"body","kind":"fixed"}"#)).is_err());

    // A hinge needs a direction.
    let mut joint = Joint {
        other: "anchor".into(),
        kind: JointKind::Revolute,
        axis: [0.0; 3],
        ..Default::default()
    };
    assert!(joint.validate().is_err());
    joint.axis = [0.0, 1.0, 0.0];
    assert!(joint.validate().is_ok());
}

#[test]
fn a_disabled_joint_does_not_constrain() {
    let disabled = scene(&format!(
        "{},{}",
        body(
            "[2,3,0]",
            r#"{"enabled":false,"other":"anchor","kind":"fixed"}"#
        ),
        anchor()
    ));
    let (world, instance) = settle(&disabled, 180);
    assert!(
        center(&instance, &world, "body").y < 1.0,
        "a disabled joint should let the body fall"
    );
}
