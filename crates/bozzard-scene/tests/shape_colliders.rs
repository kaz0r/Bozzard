use bozzard_ecs::World;
use bozzard_scene::{ColliderAxis, ColliderShape, Scene, SceneInstance, ShapeCollider, Transform};
use glam::Vec3;

fn scene(objects: &str) -> Scene {
    Scene::from_json(&format!(
        r#"{{"version":1,"name":"Shapes","views":{{}},"objects":[{objects}]}}"#
    ))
    .unwrap()
}
const FLOOR: &str = r#"{"id":"floor","name":"Floor","transform":{"translation":[0,-0.5,0],"rotation_degrees":[0,0,0],"scale":[40,1,40]},"collider":{}}"#;

fn run(instance: &SceneInstance, world: &mut World, seconds: f32) {
    for _ in 0..(seconds * 60.0) as usize {
        instance.step_gravity(world, 1. / 60.).unwrap();
    }
}
fn transform(instance: &SceneInstance, world: &World, id: &str) -> Transform {
    *world
        .get::<Transform>(instance.entity(id).unwrap())
        .unwrap()
}

#[test]
fn a_ball_rolls_down_a_slope_instead_of_sliding() {
    let s = scene(
        r#"{"id":"ball","name":"Ball","transform":{"translation":[0,2.2,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"shape_collider":{"radius":0.5},"gravity":{}},
           {"id":"slope","name":"Slope","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,-15],"scale":[40,1,8]},"collider":{}}"#,
    );
    let mut world = World::new();
    let instance = s.spawn(&mut world).unwrap();
    run(&instance, &mut world, 3.0);
    let ball = transform(&instance, &world, "ball");
    // Down the slope is +X; a rolling ball turns about -Z as it goes.
    assert!(ball.translation[0] > 2.0, "ball stayed put: {ball:?}");
    let travelled = ball.translation[0];
    let turned = ball
        .rotation_degrees
        .iter()
        .map(|d| d.abs())
        .fold(0.0, f32::max);
    assert!(
        turned > 20.0,
        "ball slid {travelled} m without turning: {ball:?}"
    );
}

#[test]
fn shapes_rest_on_the_floor_at_their_true_extent() {
    let s = scene(&format!(
        r#"{FLOOR},
           {{"id":"ball","name":"Ball","transform":{{"translation":[0,3,0],"rotation_degrees":[0,0,0],"scale":[2,2,2]}},"shape_collider":{{"radius":0.5}},"gravity":{{}}}},
           {{"id":"drum","name":"Drum","transform":{{"translation":[4,3,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},"shape_collider":{{"shape":"cylinder","radius":0.6,"height":1.4}},"gravity":{{}}}},
           {{"id":"pill","name":"Pill","transform":{{"translation":[-4,3,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},"shape_collider":{{"shape":"capsule","radius":0.3,"height":2,"axis":"x"}},"gravity":{{}}}}"#
    ));
    let mut world = World::new();
    let instance = s.spawn(&mut world).unwrap();
    run(&instance, &mut world, 4.0);
    // A scaled sphere rests on its scaled radius, a standing cylinder on half its height
    // and a capsule lying along X on its radius.
    for (id, height) in [("ball", 1.0), ("drum", 0.7), ("pill", 0.3)] {
        let y = transform(&instance, &world, id).translation[1];
        assert!(
            (y - height).abs() < 0.03,
            "{id} rests at {y}, expected {height}"
        );
    }
}

#[test]
fn queries_see_the_round_surface_not_its_bounds() {
    let s = scene(
        r#"{"id":"orb","name":"Orb","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[2,2,2]},"shape_collider":{"radius":1}}"#,
    );
    let mut world = World::new();
    let instance = s.spawn(&mut world).unwrap();
    let snapshot = instance.collisions(&world).unwrap();
    // Radius 2 after scale. Faces sit under 1% inside the true sphere.
    let hit = snapshot
        .raycast(Vec3::new(-10., 0., 0.), Vec3::X, 20., None)
        .unwrap()
        .expect("ray through the centre");
    assert_eq!(hit.object, "orb");
    assert!((hit.distance - 8.0).abs() < 0.03, "{hit:?}");
    let grazing = snapshot
        .raycast(Vec3::new(-10., 1.5, 0.), Vec3::X, 20., None)
        .unwrap()
        .expect("ray through the upper half");
    let expected = 10.0 - (4.0f32 - 1.5 * 1.5).sqrt();
    assert!((grazing.distance - expected).abs() < 0.06, "{grazing:?}");
    assert!(
        snapshot
            .raycast(Vec3::new(-10., 2.1, 0.), Vec3::X, 20., None)
            .unwrap()
            .is_none(),
        "a ray above the sphere hits nothing"
    );
    // Inside the sphere counts; the corner of its bounding box does not.
    let small = Vec3::splat(0.1);
    assert_eq!(
        snapshot.overlap_box(Vec3::ZERO, small, None, 4).unwrap(),
        ["orb"]
    );
    assert!(
        snapshot
            .overlap_box(Vec3::new(1.7, 1.7, 0.), small, None, 4)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn box_movers_stop_at_a_cylinder_and_report_the_contact() {
    let s = scene(
        r#"{"id":"post","name":"Post","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"shape_collider":{"shape":"cylinder","radius":1,"height":4}},
           {"id":"crate","name":"Crate","transform":{"translation":[-4,0,0],"rotation_degrees":[0,0,0],"scale":[0.5,0.5,0.5]},"collider":{}}"#,
    );
    let mut world = World::new();
    let instance = s.spawn(&mut world).unwrap();
    let movement = instance
        .move_box(&mut world, "crate", Vec3::new(6., 0., 0.))
        .unwrap();
    assert_eq!(movement.contacts, ["post"]);
    let x = transform(&instance, &world, "crate").translation[0];
    // The crate's near face stops at the cylinder wall, x = -1.
    assert!((x + 1.25).abs() < 0.02, "crate stopped at {x}");
}

#[test]
fn overlaps_follow_the_round_wall_not_the_bounding_square() {
    // Both crates sit inside the cylinder's bounding square; only the first reaches the wall.
    let s = scene(
        r#"{"id":"post","name":"Post","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"shape_collider":{"shape":"cylinder","radius":1,"height":4}},
           {"id":"inside","name":"Inside","transform":{"translation":[-1.1,0,0],"rotation_degrees":[0,0,0],"scale":[0.5,0.5,0.5]},"collider":{}},
           {"id":"corner","name":"Corner","transform":{"translation":[1,0,1],"rotation_degrees":[0,0,0],"scale":[0.5,0.5,0.5]},"collider":{}}"#,
    );
    let mut world = World::new();
    let instance = s.spawn(&mut world).unwrap();
    assert_eq!(
        instance.collisions(&world).unwrap().overlaps,
        vec![("inside".to_owned(), "post".to_owned())]
    );
}

#[test]
fn scene_files_round_trip_and_one_collider_kind_per_object() {
    let s = scene(
        r#"{"id":"pill","name":"Pill","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"shape_collider":{"shape":"capsule","radius":0.3,"height":1.5,"axis":"z","layers":4}}"#,
    );
    let collider = s.objects[0].shape_collider.unwrap();
    assert_eq!(
        collider,
        ShapeCollider {
            shape: ColliderShape::Capsule,
            radius: 0.3,
            height: 1.5,
            axis: ColliderAxis::Z,
            layers: 4,
            ..Default::default()
        }
    );
    assert_eq!(Scene::from_json(&s.to_json().unwrap()).unwrap(), s);
    for (extra, message) in [
        (r#""collider":{}"#, "Shape Collider cannot also"),
        (
            r#""trigger":{"action":{"kind":"sensor"}}"#,
            "must not also be a solid collider",
        ),
    ] {
        let error = Scene::from_json(&format!(
            r#"{{"version":1,"name":"Bad","views":{{}},"objects":[{{"id":"a","name":"A","transform":{{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},"shape_collider":{{}},{extra}}}]}}"#
        ))
        .unwrap_err();
        assert!(format!("{error:#}").contains(message), "{extra}: {error:#}");
    }
    let broken = Scene::from_json(
        r#"{"version":1,"name":"Bad","views":{},"objects":[{"id":"a","name":"A","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"shape_collider":{"radius":-1}}]}"#,
    );
    assert!(broken.is_err());
}
