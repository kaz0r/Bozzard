use bozzard_ecs::World;
use bozzard_scene::BoxCollider;
use bozzard_scene::{
    AssetKind, AssetSource, GravityState, MeshCollider, Prefab, Scene, Transform, TriangleMesh,
};
use glam::{Mat4, Vec3};
fn scene() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"Rigid bodies","views":{},"objects":[
        {"id":"body","name":"Body","transform":{"translation":[0,4,0],"rotation_degrees":[24,17,32],"scale":[1,1,1]},"collider":{},"gravity":{}},
        {"id":"floor","name":"Floor","transform":{"translation":[0,-0.5,0],"rotation_degrees":[0,0,0],"scale":[30,1,30]},"collider":{}}
    ]}"#).unwrap()
}
fn corners(matrix: Mat4) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        matrix.transform_point3(Vec3::new(
            if i & 1 == 0 { -0.5 } else { 0.5 },
            if i & 2 == 0 { -0.5 } else { 0.5 },
            if i & 4 == 0 { -0.5 } else { 0.5 },
        ))
    })
}
#[test]
fn dynamic_hulls_are_solid_for_box_queries_and_recover_contained_movers() {
    let mut s = scene();
    s.objects[0].collider = None;
    s.objects[0].mesh_collider = Some(MeshCollider {
        enabled: true,
        mesh: cube(),
    });
    s.objects[0].transform = Transform {
        scale: [3.; 3],
        ..Default::default()
    };
    s.objects[1].transform = Transform::default();
    s.objects[1].collider = Some(BoxCollider::default());
    let mut world = World::new();
    let instance = s.spawn(&mut world).unwrap();
    assert_eq!(
        instance.collisions(&world).unwrap().overlaps,
        vec![("body".into(), "floor".into())]
    );
    let movement = instance.move_box(&mut world, "floor", Vec3::ZERO).unwrap();
    assert!(movement.applied.length() > 1.9);
    assert!(instance.collisions(&world).unwrap().overlaps.is_empty());
}
fn cube() -> TriangleMesh {
    let p = corners(Mat4::IDENTITY);
    let indices = [
        [0, 2, 3],
        [0, 3, 1],
        [4, 5, 7],
        [4, 7, 6],
        [0, 1, 5],
        [0, 5, 4],
        [2, 6, 7],
        [2, 7, 3],
        [0, 4, 6],
        [0, 6, 2],
        [1, 3, 7],
        [1, 7, 5],
    ];
    TriangleMesh::new(indices.map(|i| i.map(|j| p[j].to_array())).to_vec()).unwrap()
}
#[test]
fn tilted_boxes_and_convex_meshes_topple_and_settle_without_penetrating() {
    for mesh in [false, true] {
        let mut scene = scene();
        if mesh {
            scene.objects[0].collider = None;
            scene.objects[0].mesh_collider = Some(MeshCollider {
                enabled: true,
                mesh: cube(),
            });
        }
        let mut world = World::new();
        let instance = scene.spawn(&mut world).unwrap();
        for _ in 0..600 {
            instance.step_gravity(&mut world, 1. / 60.).unwrap();
        }
        let matrix = instance.global_transforms(&world).unwrap()["body"];
        let corners = corners(matrix);
        let low = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let high = corners
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        eprintln!(
            "mesh={mesh} low={low} high={high} pose={:?}",
            world.get::<Transform>(instance.entity("body").unwrap())
        );
        assert!(low > -0.005 && low < 0.005);
        assert!(
            (high - low - 1.).abs() < 0.01,
            "box should settle onto a face, not freeze on its tilted corner"
        );
        let before = matrix;
        for _ in 0..120 {
            instance.step_gravity(&mut world, 1. / 60.).unwrap();
        }
        assert!(instance.global_transforms(&world).unwrap()["body"].abs_diff_eq(before, 0.001));
    }
}
#[test]
fn dynamic_mesh_hulls_are_shared_and_flat_meshes_are_rejected() {
    let mut scene = scene();
    scene.objects[0].collider = None;
    scene.objects[0].mesh_collider = Some(MeshCollider {
        enabled: true,
        mesh: TriangleMesh::new(vec![[[0., 0., 0.], [1., 0., 0.], [0., 0., 1.]]]).unwrap(),
    });
    assert!(scene.validate().is_err());
    scene.objects[0].mesh_collider.as_mut().unwrap().mesh = cube();
    scene.validate().unwrap();
    assert_eq!(cube().convex_hull().unwrap().triangles().len(), 12);
}
#[test]
fn root_scale_and_uniform_parent_rotation_preserve_geometry() {
    let mut s = scene();
    let mut parent = s.objects[1].clone();
    parent.id = "parent".into();
    parent.collider = None;
    parent.transform.translation = [0.; 3];
    parent.transform.rotation_degrees = [0., 35., 0.];
    parent.transform.scale = [2.; 3];
    s.objects.push(parent);
    s.objects[0].parent = Some("parent".into());
    s.objects[0].transform.scale = [-0.5, 0.5, 0.5];
    let mut world = World::new();
    let i = s.spawn(&mut world).unwrap();
    for _ in 0..600 {
        i.step_gravity(&mut world, 1. / 60.).unwrap();
    }
    let corners = corners(i.global_transforms(&world).unwrap()["body"]);
    assert!(corners.iter().all(|v| v.y > -0.005 && v.y < 1.01));
    s.objects[2].transform.scale = [2., 1., 1.];
    assert!(s.validate().is_err());
}

#[test]
fn prefab_removal_releases_solver_bodies_and_fresh_instances_do_not_inherit_velocity() {
    let mut s = scene();
    let mut body = s.objects.remove(0);
    body.collider = None;
    body.mesh_collider = Some(MeshCollider {
        enabled: true,
        mesh: cube(),
    });
    s.assets.insert(
        "body-template".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "unused.prefab.json".into(),
        },
    );
    let prefab = Prefab {
        version: 1,
        name: "Body".into(),
        root: body.id.clone(),
        objects: vec![body],
        assets: Default::default(),
    };
    let mut world = World::new();
    let mut instance = s.spawn(&mut world).unwrap();
    instance
        .register_prefab("body-template".into(), prefab)
        .unwrap();
    for _ in 0..64 {
        let id = instance
            .spawn_prefab(&mut world, "body-template", [0., 4., 0.])
            .unwrap();
        assert_eq!(
            *world
                .get::<GravityState>(instance.entity(&id).unwrap())
                .unwrap(),
            GravityState::default()
        );
        for _ in 0..5 {
            instance.step_gravity(&mut world, 1. / 60.).unwrap();
        }
        assert_eq!(instance.physics_body_count(&world), 2);
        instance.destroy_prefab(&mut world, &id).unwrap();
        assert_eq!(instance.physics_body_count(&world), 1);
        instance.step_gravity(&mut world, 1. / 60.).unwrap();
        assert_eq!(instance.physics_body_count(&world), 0);
    }
    assert_eq!(world.len(), 1);
}

#[test]
fn body_impacts_transfer_motion_and_teleports_keep_scale() {
    let mut s = scene();
    s.objects[0].transform.translation = [-0.65, 3., 0.];
    s.objects[0].transform.rotation_degrees = [0., 0., 35.];
    let mut other = s.objects[0].clone();
    other.id = "other".into();
    other.transform.translation = [0.25, 0.5, 0.];
    other.transform.rotation_degrees = [0.; 3];
    s.objects.push(other);
    let mut world = World::new();
    let instance = s.spawn(&mut world).unwrap();
    for _ in 0..300 {
        instance.step_gravity(&mut world, 1. / 60.).unwrap();
    }
    let entity = instance.entity("other").unwrap();
    assert!(
        world.get::<Transform>(entity).unwrap().translation[0] > 0.26,
        "impact should push the other dynamic body: {:?}",
        world.get::<Transform>(entity)
    );
    let t = world.get_mut::<Transform>(entity).unwrap();
    t.translation = [5., 5., 5.];
    t.scale = [2., 1., 0.5];
    instance.step_gravity(&mut world, 1. / 60.).unwrap();
    let t = world.get::<Transform>(entity).unwrap();
    assert_eq!(t.scale, [2., 1., 0.5]);
    assert!(t.translation[1] > 4.9 && t.translation[0] > 4.9);
    let saved = instance.capture(&world).unwrap();
    assert_eq!(Scene::from_json(&saved.to_json().unwrap()).unwrap(), saved);
}
