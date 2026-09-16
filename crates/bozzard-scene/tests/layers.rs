//! Collision layers and masks gate the CPU sweeps, overlap reporting and the Rapier solver.
use bozzard_ecs::World;
use bozzard_scene::{BoxCollider, Scene};
use glam::Vec3;

fn collision_scene(objects: &str) -> Scene {
    Scene::from_json(&format!(
        r#"{{"version":1,"name":"layers","views":{{}},"objects":[{objects}]}}"#
    ))
    .unwrap()
}

fn boxed(id: &str, x: f32, layers: u32, mask: u32) -> String {
    format!(
        r#"{{"id":"{id}","name":"{id}",
            "transform":{{"translation":[{x},0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
            "collider":{{"size":[1,1,1],"layers":{layers},"mask":{mask}}}}}"#
    )
}

#[test]
fn a_scene_without_layer_fields_keeps_the_default_layer_and_full_mask() {
    let plain = collision_scene(
        r#"{"id":"box","name":"box",
            "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "collider":{"size":[1,1,1]}}"#,
    );
    assert_eq!(
        plain.objects[0].collider,
        Some(BoxCollider {
            size: [1.0; 3],
            ..Default::default()
        })
    );
    let mut world = World::new();
    let instance = plain.spawn(&mut world).unwrap();
    // The defaults survive spawn and capture, so an older scene reloads unchanged.
    assert_eq!(
        instance.capture(&world).unwrap().objects[0].collider,
        plain.objects[0].collider
    );
}

#[test]
fn non_matching_layers_report_no_overlap() {
    // Player (bit 0) meets Environment (bit 1) unless a mask excludes it.
    let interacting = collision_scene(&format!(
        "{},{}",
        boxed("a", 0.0, 1, 2),
        boxed("b", 0.5, 2, 1)
    ));
    let mut world = World::new();
    let instance = interacting.spawn(&mut world).unwrap();
    assert_eq!(
        instance.collisions(&world).unwrap().overlaps,
        vec![("a".to_string(), "b".to_string())]
    );

    // One side refusing is enough, exactly like `InteractionGroups`.
    for (a_mask, b_mask) in [(0, u32::MAX), (u32::MAX, 0)] {
        let scene = collision_scene(&format!(
            "{},{}",
            boxed("a", 0.0, 1, a_mask),
            boxed("b", 0.5, 2, b_mask)
        ));
        let mut world = World::new();
        let instance = scene.spawn(&mut world).unwrap();
        assert!(
            instance.collisions(&world).unwrap().overlaps.is_empty(),
            "mask {a_mask}/{b_mask} should not meet"
        );
    }
}

#[test]
fn a_swept_mover_passes_through_layers_it_does_not_collide_with() {
    let clear = collision_scene(&format!(
        "{},{}",
        boxed("mover", 0.0, 1, 1),
        boxed("wall", 2.0, 2, u32::MAX)
    ));
    let mut world = World::new();
    let instance = clear.spawn(&mut world).unwrap();
    let moved = instance
        .move_box(&mut world, "mover", Vec3::X * 3.5)
        .unwrap();
    assert!(
        (moved.applied.x - 3.5).abs() < 1e-4,
        "mover was blocked by a non-interacting layer: {moved:?}"
    );
    assert!(moved.contacts.is_empty());

    let blocked = collision_scene(&format!(
        "{},{}",
        boxed("mover", 0.0, 1, 3),
        boxed("wall", 2.0, 2, u32::MAX)
    ));
    let mut world = World::new();
    let instance = blocked.spawn(&mut world).unwrap();
    let moved = instance
        .move_box(&mut world, "mover", Vec3::X * 3.5)
        .unwrap();
    assert!(moved.applied.x < 1.01, "mover should stop at the wall");
    assert_eq!(moved.contacts, vec!["wall".to_string()]);
}

#[test]
fn rapier_bodies_on_non_matching_layers_pass_through_each_other() {
    let falling = |body_mask: u32| {
        collision_scene(&format!(
            r#"{{"id":"body","name":"body",
                "transform":{{"translation":[0,3,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
                "collider":{{"size":[1,1,1],"layers":1,"mask":{body_mask}}},
                "gravity":{{"enabled":true,"acceleration":10}}}},
              {{"id":"floor","name":"floor",
                "transform":{{"translation":[0,-0.5,0],"rotation_degrees":[0,0,0],"scale":[20,1,20]}},
                "collider":{{"size":[1,1,1],"layers":2,"mask":1}}}}"#
        ))
    };
    let rest = |body_mask: u32| {
        let mut world = World::new();
        let instance = falling(body_mask).spawn(&mut world).unwrap();
        for _ in 0..180 {
            instance.step_gravity(&mut world, 1. / 60.).unwrap();
        }
        instance.global_transforms(&world).unwrap()["body"]
            .transform_point3(Vec3::ZERO)
            .y
    };
    assert!(
        (rest(2) - 0.5).abs() < 0.05,
        "an interacting body should rest on the floor"
    );
    assert!(
        rest(1) < -4.0,
        "a body that does not collide with the floor layer should keep falling"
    );
}
