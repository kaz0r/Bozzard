use bozzard_ecs::World;
use bozzard_scene::{Scene, Transform};
use glam::{Vec3, Vec3Swizzles};

const EPSILON: f32 = 2e-4;

fn scene(json: &str) -> Scene {
    Scene::from_json(json).unwrap()
}

fn collision_scene(objects: &str) -> Scene {
    scene(&format!(
        r#"{{"version":1,"name":"response test","views":{{}},"objects":[{objects}]}}"#
    ))
}

fn box_object(id: &str, translation: [f32; 3]) -> String {
    box_object_with_size(id, translation, [2.0, 2.0, 2.0])
}

fn box_object_with_size(id: &str, translation: [f32; 3], size: [f32; 3]) -> String {
    format!(
        r#"{{"id":"{id}","name":"{id}","transform":{{"translation":{translation:?},"rotation_degrees":[0,0,0],"scale":[1,1,1]}},"collider":{{"center":[0,0,0],"size":{size:?},"enabled":true}}}}"#
    )
}

fn center(instance: &bozzard_scene::SceneInstance, world: &World, id: &str) -> Vec3 {
    instance.global_transforms(world).unwrap()[id].transform_point3(Vec3::ZERO)
}

fn assert_vec3_near(actual: Vec3, expected: Vec3) {
    assert!(
        (actual - expected).abs().max_element() <= EPSILON,
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn move_box_applies_unobstructed_world_motion() {
    let mover = box_object("mover", [1.0, 2.0, 3.0]);
    let scene = collision_scene(&mover);
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    let result = instance
        .move_box(&mut world, "mover", Vec3::new(2.5, -1.0, 4.0))
        .unwrap();

    assert_vec3_near(result.requested, Vec3::new(2.5, -1.0, 4.0));
    assert_vec3_near(result.applied, Vec3::new(2.5, -1.0, 4.0));
    assert!(result.contacts.is_empty());
    assert_vec3_near(center(&instance, &world, "mover"), Vec3::new(3.5, 1.0, 7.0));
}

#[test]
fn move_box_stops_before_a_wall_even_at_high_speed() {
    let scene = collision_scene(&format!(
        "{},{}",
        box_object("mover", [0.0, 0.0, 0.0]),
        box_object("wall", [5.0, 0.0, 0.0])
    ));
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    let result = instance
        .move_box(&mut world, "mover", Vec3::new(100.0, 0.0, 0.0))
        .unwrap();

    assert_vec3_near(result.requested, Vec3::new(100.0, 0.0, 0.0));
    assert!((result.applied.x - 3.0).abs() <= EPSILON);
    assert_vec3_near(result.applied.yz().extend(0.0), Vec3::ZERO);
    assert_eq!(result.contacts, ["wall"]);
    assert!((center(&instance, &world, "mover").x - 3.0).abs() <= EPSILON);
}

#[test]
fn move_box_blocks_downward_floor_motion_but_allows_motion_away_from_it() {
    let scene = collision_scene(&format!(
        "{},{}",
        box_object("mover", [0.0, 0.0, 0.0]),
        box_object("floor", [0.0, -3.0, 0.0])
    ));
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    let down = instance
        .move_box(&mut world, "mover", Vec3::new(0.0, -50.0, 0.0))
        .unwrap();
    assert!((down.applied.y + 1.0).abs() <= EPSILON);
    assert_vec3_near(down.applied.xz().extend(0.0), Vec3::ZERO);
    assert_eq!(down.contacts, ["floor"]);

    let up = instance
        .move_box(&mut world, "mover", Vec3::new(0.0, 4.0, 0.0))
        .unwrap();
    assert_vec3_near(up.applied, Vec3::new(0.0, 4.0, 0.0));
    assert!(up.contacts.is_empty());
    assert!((center(&instance, &world, "mover").y - 3.0).abs() <= EPSILON);
}

#[test]
fn move_box_slides_along_a_wall() {
    let scene = collision_scene(&format!(
        "{},{}",
        box_object("mover", [0.0, 0.0, 0.0]),
        box_object("wall", [5.0, 0.0, 0.0])
    ));
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    let result = instance
        .move_box(&mut world, "mover", Vec3::new(10.0, 0.0, 4.0))
        .unwrap();

    assert!((result.applied.x - 3.0).abs() <= EPSILON);
    assert!((result.applied.z - 4.0).abs() <= EPSILON);
    assert_eq!(result.contacts, ["wall"]);
    assert_vec3_near(center(&instance, &world, "mover"), Vec3::new(3.0, 0.0, 4.0));
}

#[test]
fn move_box_stops_at_a_corner() {
    let scene = collision_scene(&format!(
        "{},{},{}",
        box_object("mover", [0.0, 0.0, 0.0]),
        box_object_with_size("wall_x", [5.0, 0.0, 0.0], [2.0, 2.0, 100.0]),
        box_object_with_size("wall_z", [0.0, 0.0, 5.0], [100.0, 2.0, 2.0])
    ));
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    let result = instance
        .move_box(&mut world, "mover", Vec3::new(10.0, 0.0, 10.0))
        .unwrap();

    assert!((result.applied.x - 3.0).abs() <= EPSILON);
    assert!((result.applied.z - 3.0).abs() <= EPSILON);
    assert_eq!(result.contacts, ["wall_x", "wall_z"]);
    assert_vec3_near(center(&instance, &world, "mover"), Vec3::new(3.0, 0.0, 3.0));
}

#[test]
fn move_box_converts_world_motion_through_a_scaled_rotated_parent() {
    let scene = collision_scene(
        r#"{
          "id":"parent", "name":"parent",
          "transform":{"translation":[4,1,-2],"rotation_degrees":[0,90,0],"scale":[2,3,0.5]}
        },{
          "id":"mover", "name":"mover", "parent":"parent",
          "transform":{"translation":[1,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[1,1,1],"enabled":true}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    let before = center(&instance, &world, "mover");
    let requested = Vec3::new(3.0, -2.0, 5.0);

    let result = instance.move_box(&mut world, "mover", requested).unwrap();

    assert_vec3_near(result.requested, requested);
    assert_vec3_near(result.applied, requested);
    assert!(result.contacts.is_empty());
    assert_vec3_near(center(&instance, &world, "mover"), before + requested);
    assert_vec3_near(
        Vec3::from(
            world
                .get::<Transform>(instance.entity("mover").unwrap())
                .unwrap()
                .translation,
        ),
        Vec3::new(-1.5, -2.0 / 3.0, 6.0),
    );
}

#[test]
fn move_box_ignores_disabled_colliders() {
    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        },{
          "id":"disabled", "name":"disabled",
          "transform":{"translation":[5,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":false}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    let result = instance
        .move_box(&mut world, "mover", Vec3::new(10.0, 0.0, 0.0))
        .unwrap();

    assert_vec3_near(result.applied, Vec3::new(10.0, 0.0, 0.0));
    assert!(result.contacts.is_empty());
}

#[test]
fn move_box_recovers_an_initial_penetration_before_applying_motion() {
    let scene = collision_scene(&format!(
        "{},{}",
        box_object("mover", [0.0, 0.0, 0.0]),
        box_object("blocker", [0.5, 0.0, 0.0])
    ));
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    let result = instance.move_box(&mut world, "mover", Vec3::ZERO).unwrap();

    assert_eq!(result.contacts, ["blocker"]);
    assert!(result.applied.length() > 1.0);
    assert!(instance.collisions(&world).unwrap().overlaps.is_empty());
}

#[test]
fn move_box_sweeps_against_a_rotated_wall_at_high_speed() {
    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        },{
          "id":"rotated_wall", "name":"rotated_wall",
          "transform":{"translation":[6,0,0],"rotation_degrees":[0,45,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,8],"enabled":true}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    let result = instance
        .move_box(&mut world, "mover", Vec3::new(100.0, 0.0, 0.0))
        .unwrap();

    assert!(result.applied.x > 0.0 && result.applied.x < 100.0);
    assert!(result.applied.z > 1.0, "expected a slide, got {result:?}");
    assert_eq!(result.contacts, ["rotated_wall"]);
    assert!(instance.collisions(&world).unwrap().overlaps.is_empty());
}

#[test]
fn move_box_errors_transactionally_for_invalid_delta_or_missing_collider() {
    let scene = collision_scene(
        r#"{
          "id":"mover", "name":"mover",
          "transform":{"translation":[1,2,3],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        },{
          "id":"plain", "name":"plain",
          "transform":{"translation":[4,5,6],"rotation_degrees":[0,0,0],"scale":[1,1,1]}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    let mover = instance.entity("mover").unwrap();
    let plain = instance.entity("plain").unwrap();
    let mover_before = *world.get::<Transform>(mover).unwrap();
    let plain_before = *world.get::<Transform>(plain).unwrap();

    assert!(
        instance
            .move_box(&mut world, "mover", Vec3::new(f32::NAN, 0.0, 0.0))
            .is_err()
    );
    assert_eq!(*world.get::<Transform>(mover).unwrap(), mover_before);

    assert!(instance.move_box(&mut world, "plain", Vec3::X).is_err());
    assert_eq!(*world.get::<Transform>(plain).unwrap(), plain_before);
    assert_eq!(*world.get::<Transform>(mover).unwrap(), mover_before);
}
