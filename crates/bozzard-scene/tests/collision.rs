use bozzard_ecs::World;
use bozzard_scene::{BoxCollider, Scene, Transform};
use glam::Vec3;

fn scene(json: &str) -> Scene {
    Scene::from_json(json).unwrap()
}

fn collision_scene(objects: &str) -> Scene {
    scene(&format!(
        r#"{{"version":1,"name":"collision test","views":{{}},"objects":[{objects}]}}"#
    ))
}

fn ids(snapshot: &bozzard_scene::CollisionSnapshot) -> Vec<(String, String)> {
    snapshot.overlaps.clone()
}

fn intervals_overlap(a: &[Vec3; 8], b: &[Vec3; 8], axis: Vec3) -> bool {
    let (a_min, a_max) = a
        .iter()
        .map(|corner| corner.dot(axis))
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), value| {
            (min.min(value), max.max(value))
        });
    let (b_min, b_max) = b
        .iter()
        .map(|corner| corner.dot(axis))
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), value| {
            (min.min(value), max.max(value))
        });
    a_max >= b_min && b_max >= a_min
}

fn face_normals(corners: &[Vec3; 8]) -> [Vec3; 3] {
    let x = corners[1] - corners[0];
    let y = corners[2] - corners[0];
    let z = corners[4] - corners[0];
    [y.cross(z), z.cross(x), x.cross(y)]
}

#[test]
fn scenes_without_colliders_remain_compatible() {
    let scene = scene(
        r#"{
          "version": 1,
          "name": "old scene",
          "views": {},
          "objects": [{
            "id": "legacy",
            "name": "Legacy object",
            "transform": {
              "translation": [0.0, 0.0, 0.0],
              "rotation_degrees": [0.0, 0.0, 0.0],
              "scale": [1.0, 1.0, 1.0]
            }
          }]
        }"#,
    );
    assert!(scene.objects[0].collider.is_none());

    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    assert!(instance.collisions(&world).unwrap().boxes.is_empty());
    assert_eq!(instance.capture(&world).unwrap(), scene);
}

#[test]
fn collider_json_spawns_and_capture_roundtrips_live_ecs_data() {
    assert_eq!(
        BoxCollider::default(),
        BoxCollider {
            center: [0.0; 3],
            size: [1.0; 3],
            enabled: true,
        }
    );
    let scene = collision_scene(
        r#"{
          "id":"box", "name":"Box",
          "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0.25,-0.5,1.0],"size":[2.0,3.0,4.0]}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    let entity = instance.entity("box").unwrap();
    assert_eq!(
        world.get::<BoxCollider>(entity),
        Some(&BoxCollider {
            center: [0.25, -0.5, 1.0],
            size: [2.0, 3.0, 4.0],
            enabled: true,
        })
    );

    world.get_mut::<BoxCollider>(entity).unwrap().enabled = false;
    let saved = instance.capture(&world).unwrap();
    assert!(!saved.objects[0].collider.as_ref().unwrap().enabled);
    let loaded = Scene::from_json(&saved.to_json().unwrap()).unwrap();
    let mut second_world = World::new();
    let second = loaded.spawn(&mut second_world).unwrap();
    assert!(second.collisions(&second_world).unwrap().boxes.is_empty());
}

#[test]
fn invalid_colliders_fail_validation_before_any_entities_spawn() {
    for kind in 0..3 {
        let mut scene = collision_scene(
            r#"{
              "id":"bad", "name":"Bad",
              "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
              "collider":{"center":[0,0,0],"size":[1,1,1],"enabled":true}
            }"#,
        );
        let collider = scene.objects[0].collider.as_mut().unwrap();
        match kind {
            0 => collider.center[0] = f32::NAN,
            1 => collider.size[1] = 0.0,
            _ => collider.size[2] = 0.000_09,
        }
        let mut world = World::new();
        assert!(scene.spawn(&mut world).is_err());
        assert_eq!(world.len(), 0);
    }
}

#[test]
fn collision_snapshot_reports_enabled_touching_and_overlapping_boxes_in_id_order() {
    let scene = collision_scene(
        r#"{
          "id":"anchor", "name":"Anchor",
          "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        },{
          "id":"touch", "name":"Touch",
          "transform":{"translation":[2,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        },{
          "id":"overlap", "name":"Overlap",
          "transform":{"translation":[0.5,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        },{
          "id":"apart", "name":"Apart",
          "transform":{"translation":[8,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        },{
          "id":"disabled", "name":"Disabled",
          "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":false}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    let snapshot = instance.collisions(&world).unwrap();

    assert_eq!(
        snapshot
            .boxes
            .iter()
            .map(|b| b.id.as_str())
            .collect::<Vec<_>>(),
        ["anchor", "apart", "overlap", "touch"]
    );
    assert_eq!(
        ids(&snapshot),
        vec![
            ("anchor".into(), "overlap".into()),
            ("anchor".into(), "touch".into()),
            ("overlap".into(), "touch".into()),
        ]
    );
}

#[test]
fn collisions_follow_runtime_rotation_parent_shear_and_mirrored_scale() {
    let scene = collision_scene(
        r#"{
          "id":"axis", "name":"Axis",
          "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        },{
          "id":"rotated", "name":"Rotated",
          "transform":{"translation":[2.2,0,2.2],"rotation_degrees":[0,45,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[2,2,2],"enabled":true}
        },{
          "id":"parent", "name":"Parent",
          "transform":{"translation":[5,0,0],"rotation_degrees":[0,30,0],"scale":[-2,1,0.5]}
        },{
          "id":"sheared", "name":"Sheared", "parent":"parent",
          "transform":{"translation":[0.5,0,0],"rotation_degrees":[0,25,0],"scale":[1,1,1]},
          "collider":{"center":[0.25,0,0],"size":[1,1,1],"enabled":true}
        },{
          "id":"probe", "name":"Probe",
          "transform":{"translation":[20,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[0.2,0.2,0.2],"enabled":true}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();

    // Their axis-aligned bounding boxes overlap, but these rotated cubes do not.
    assert!(
        !ids(&instance.collisions(&world).unwrap()).contains(&("axis".into(), "rotated".into()))
    );

    world
        .get_mut::<Transform>(instance.entity("rotated").unwrap())
        .unwrap()
        .translation = [1.2, 0.0, 1.2];
    let sheared_center = instance.global_transforms(&world).unwrap()["sheared"]
        .transform_point3(Vec3::new(0.25, 0.0, 0.0));
    world
        .get_mut::<Transform>(instance.entity("probe").unwrap())
        .unwrap()
        .translation = sheared_center.to_array();
    let snapshot = instance.collisions(&world).unwrap();
    assert!(ids(&snapshot).contains(&("axis".into(), "rotated".into())));
    assert!(ids(&snapshot).contains(&("probe".into(), "sheared".into())));

    let sheared = snapshot.boxes.iter().find(|b| b.id == "sheared").unwrap();
    let corner_average = sheared.corners.iter().copied().sum::<Vec3>() / 8.0;
    assert!((corner_average - sheared_center).length() < 1e-5);
}

#[test]
fn edge_cross_axes_reject_boxes_when_all_face_normal_intervals_overlap() {
    // This fixed pair has no separating face normal. Its only separating direction
    // comes from crossing one edge direction from each independently rotated box.
    let scene = collision_scene(
        r#"{
          "id":"a", "name":"A",
          "transform":{"translation":[0,0,0],"rotation_degrees":[-72.12188,5.497219,56.72914],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[3.7207024,1.9022856,3.620713],"enabled":true}
        },{
          "id":"b", "name":"B",
          "transform":{"translation":[1.6964042,-2.885569,3.8067462],"rotation_degrees":[66.27773,18.25118,65.60451],"scale":[1,1,1]},
          "collider":{"center":[0,0,0],"size":[3.8078852,3.7719324,3.451929],"enabled":true}
        }"#,
    );
    let mut world = World::new();
    let instance = scene.spawn(&mut world).unwrap();
    let snapshot = instance.collisions(&world).unwrap();
    assert!(snapshot.overlaps.is_empty());

    let a = &snapshot.boxes[0].corners;
    let b = &snapshot.boxes[1].corners;
    for axis in face_normals(a).into_iter().chain(face_normals(b)) {
        assert!(intervals_overlap(a, b, axis.normalize()));
    }
}
