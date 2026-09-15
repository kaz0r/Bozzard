use bozzard_ecs::World;
use bozzard_scene::{BoxCollider, MeshCollider, Object, Scene, Transform, TriangleMesh};
use glam::Vec3;
fn scene() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"queries","views":{},"objects":[]}"#).unwrap()
}
#[test]
fn sphere_narrow_phase_rejects_box_corners_and_mesh_holes() {
    let mut s = scene();
    s.objects.push(Object {
        id: "box".into(),
        name: "box".into(),
        collider: Some(BoxCollider::default()),
        ..Default::default()
    });
    let mut world = World::default();
    let i = s.spawn(&mut world).unwrap();
    let q = i.query_geometry(&world).unwrap();
    assert!(
        q.overlap_sphere(Vec3::splat(0.9), 0.5, None, 8)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        q.overlap_sphere(Vec3::new(0.9, 0., 0.), 0.5, None, 8)
            .unwrap(),
        ["box"]
    );
    s.objects[0].collider = None;
    s.objects[0].mesh_collider = Some(MeshCollider {
        enabled: true,
        layers: 1,
        mask: u32::MAX,
        mesh: TriangleMesh::new(vec![
            [[-2., 0., 0.], [-1., 0., 0.], [-2., 1., 0.]],
            [[1., 0., 0.], [2., 0., 0.], [2., 1., 0.]],
        ])
        .unwrap(),
    });
    let mut world = World::default();
    let i = s.spawn(&mut world).unwrap();
    let q = i.query_geometry(&world).unwrap();
    assert!(
        q.raycast(Vec3::new(0., 0.2, 1.), Vec3::NEG_Z, 2., None)
            .unwrap()
            .is_none()
    );
    assert!(
        q.overlap_sphere(Vec3::new(0., 0.2, 0.), 0.2, None, 8)
            .unwrap()
            .is_empty()
    );
    assert!(
        q.raycast(Vec3::new(-1.8, 0.2, 1.), Vec3::NEG_Z, 2., None)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        q.overlap_sphere(Vec3::new(-1.8, 0.2, 0.), 0.2, None, 8)
            .unwrap(),
        ["box"]
    );
}
#[test]
fn transformed_colliders_have_world_distances_normals_and_stable_ties() {
    let mut s = scene();
    s.objects.push(Object {
        id: "a".into(),
        name: "a".into(),
        collider: Some(BoxCollider::default()),
        transform: Transform {
            translation: [0., 0., -5.],
            rotation_degrees: [0., 45., 0.],
            scale: [2., 1., 2.],
        },
        ..Default::default()
    });
    let mut b = s.objects[0].clone();
    b.id = "b".into();
    s.objects.push(b);
    let mut world = World::default();
    let i = s.spawn(&mut world).unwrap();
    let q = i.query_geometry(&world).unwrap();
    let h = q
        .raycast(Vec3::ZERO, Vec3::NEG_Z * 5., 10., None)
        .unwrap()
        .unwrap();
    assert_eq!(h.object, "a");
    assert!((h.distance - (5. - 2f32.sqrt())).abs() < 1e-5);
    assert!((h.normal.length() - 1.).abs() < 1e-5 && h.normal.z > 0.7);
    assert_eq!(
        q.raycast(Vec3::ZERO, Vec3::NEG_Z, 10., Some("a"))
            .unwrap()
            .unwrap()
            .object,
        "b"
    );
    assert!(q.overlap_sphere(Vec3::ZERO, 20., None, 1).is_err());
}
