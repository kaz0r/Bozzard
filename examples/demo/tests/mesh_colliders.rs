use bozzard_demo::SceneDemo;
use bozzard_scene::blueprint::{NodeKind as K, Value};
use bozzard_scene::{
    BoxCollider, GameplayInput, Gravity, GravityState, MeshCollider, Scene, Transform, TriangleMesh,
};

fn scene() -> Scene {
    let mut scene = Scene::from_json(r#"{"version":1,"name":"Mesh collision","views":{},"objects":[{"id":"floor","name":"Floor","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},{"id":"body","name":"Body","transform":{"translation":[0,3,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#).unwrap();
    scene.objects[0].mesh_collider = Some(MeshCollider {
        enabled: true,
        mesh: TriangleMesh::new(vec![
            [[-5., 0., -5.], [5., 0., -5.], [5., 0., 5.]],
            [[-5., 0., -5.], [5., 0., 5.], [-5., 0., 5.]],
        ])
        .unwrap(),
    });
    scene.objects[1].collider = Some(BoxCollider::default());
    scene
}
fn position(demo: &SceneDemo) -> [f32; 3] {
    demo.app
        .world
        .get::<Transform>(demo.instance().entity("body").unwrap())
        .unwrap()
        .translation
}
#[test]
fn triangle_sweeps_ground_slide_disable_and_roundtrip() {
    let scene = scene();
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    let mut demo = SceneDemo::new(&scene).unwrap();
    let hit = demo
        .with_instance(|i, w| i.move_box(w, "body", [0., -100., 0.].into()))
        .unwrap();
    assert_eq!(hit.contacts, ["floor"]);
    assert!((position(&demo)[1] - 0.5).abs() < 0.001);
    assert!(hit.contact_normals.iter().any(|n| n.y > 0.99));
    demo.with_instance(|i, w| i.move_box(w, "body", [2., 0., 0.].into()))
        .unwrap();
    assert!((position(&demo)[0] - 2.).abs() < 0.001);
    let body = demo.instance().entity("body").unwrap();
    demo.app.world.insert(body, Gravity::default()).unwrap();
    for _ in 0..120 {
        demo.app.step();
        demo.check_simulation().unwrap();
    }
    assert!(demo.app.world.get::<GravityState>(body).unwrap().grounded);
    let floor = demo.instance().entity("floor").unwrap();
    demo.app
        .world
        .get_mut::<MeshCollider>(floor)
        .unwrap()
        .enabled = false;
    for _ in 0..60 {
        demo.app.step();
        demo.check_simulation().unwrap();
    }
    assert!(position(&demo)[1] < 0.);
    assert!(
        !demo.instance().capture(&demo.app.world).unwrap().objects[0]
            .mesh_collider
            .as_ref()
            .unwrap()
            .enabled
    );
}
#[test]
fn actual_triangles_leave_holes_and_block_both_sides() {
    let mut scene = scene();
    scene.objects[0].mesh_collider.as_mut().unwrap().mesh = TriangleMesh::new(vec![
        [[-5., 0., -5.], [-2., 0., 5.], [-5., 0., 5.]],
        [[5., 0., -5.], [5., 0., 5.], [2., 0., 5.]],
    ])
    .unwrap();
    let mut demo = SceneDemo::new(&scene).unwrap();
    assert!(
        demo.with_instance(|i, w| i.move_box(w, "body", [0., -10., 0.].into()))
            .unwrap()
            .contacts
            .is_empty()
    );
    scene = self::scene();
    scene.objects[1].transform.translation[1] = -3.;
    let mut demo = SceneDemo::new(&scene).unwrap();
    let hit = demo
        .with_instance(|i, w| i.move_box(w, "body", [0., 10., 0.].into()))
        .unwrap();
    assert!(hit.contact_normals.iter().any(|n| n.y < -0.99));
    assert!((position(&demo)[1] + 0.5).abs() < 0.001);
}
#[test]
fn ramps_parent_transforms_recovery_and_invalid_data() {
    let mut scene = scene();
    scene.objects[0].transform.rotation_degrees[2] = 20.;
    scene.objects[0].transform.scale = [-2., 1., 0.7];
    let mut demo = SceneDemo::new(&scene).unwrap();
    let hit = demo
        .with_instance(|i, w| i.move_box(w, "body", [0., -3., 0.].into()))
        .unwrap();
    assert_eq!(hit.contacts, ["floor"]);
    assert!(hit.contact_normals.iter().any(|n| n.y > 0.9 && n.x < -0.3));
    assert!(position(&demo)[0] < 0.);
    scene.objects[0].transform = Transform::default();
    scene.objects[1].transform.translation[1] = 0.25;
    let mut demo = SceneDemo::new(&scene).unwrap();
    assert_eq!(
        demo.instance()
            .collisions(&demo.app.world)
            .unwrap()
            .overlaps,
        [("body".into(), "floor".into())]
    );
    demo.with_instance(|i, w| i.move_box(w, "body", [0., 0., 0.].into()))
        .unwrap();
    assert!(position(&demo)[1] >= 0.5);
    let camera = demo
        .instance()
        .obstructed_camera(
            &demo.app.world,
            "body",
            [0., 2., 0.].into(),
            [0., -2., 0.].into(),
            0.2,
        )
        .unwrap();
    assert!(camera.y > 0.19);
    let mut parented = self::scene();
    let mut parent = parented.objects[0].clone();
    parent.id = "parent".into();
    parent.mesh_collider = None;
    parent.transform.translation = [3., 1., -2.];
    parent.transform.rotation_degrees = [15., 20., 10.];
    parent.transform.scale = [2., 1., 0.6];
    parented.objects[0].parent = Some(parent.id.clone());
    parented.objects[0].transform.rotation_degrees[2] = 20.;
    parented.objects[0].transform.scale[0] = -1.;
    parented.objects.push(parent);
    let matrix = parented.global_transforms().unwrap()["floor"];
    let normal = matrix
        .inverse()
        .transpose()
        .transform_vector3([0., 1., 0.].into())
        .normalize();
    let center = matrix.transform_point3([0.; 3].into());
    parented.objects[1].transform.translation = (center + normal * 3.).to_array();
    let mut transformed = SceneDemo::new(&parented).unwrap();
    let hit = transformed
        .with_instance(|i, w| i.move_box(w, "body", -normal * 5.))
        .unwrap();
    assert_eq!(hit.contacts, ["floor"]);
    assert!(hit.contact_normals.iter().any(|n| n.dot(normal) > 0.999));
    let mut compound = self::scene();
    compound.objects[0].parent = Some("body".into());
    let mut compound = SceneDemo::new(&compound).unwrap();
    let before = position(&compound);
    assert!(
        compound
            .with_instance(|i, w| i.move_box(w, "body", [0., 1., 0.].into()))
            .is_err()
    );
    assert_eq!(position(&compound), before);
    scene.objects[0].gravity = Some(Gravity::default());
    assert!(scene.validate().is_err());
    assert!(TriangleMesh::new(vec![[[f32::NAN; 3]; 3]]).is_err());
    assert!(TriangleMesh::new(vec![[[0.; 3]; 3]]).is_err());
    assert!(TriangleMesh::new(vec![[[0.; 3]; 3]; 100001]).is_err());
}
#[test]
fn mesh_blueprint_contacts_and_prefab_lifecycle_are_independent() {
    use bozzard_scene::blueprint::{Blueprint, Node, Socket, Wire};
    let mut scene = scene();
    scene.objects[1].transform.translation[1] = 0.25;
    let mut graph = Blueprint {
        nodes: vec![
            Node::new(1, K::BodyEnter, [0.; 2]),
            Node::new(2, K::SetPosition, [300., 0.]),
            Node::new(3, K::BodyExit, [0., 200.]),
            Node::new(4, K::SetPosition, [300., 200.]),
        ],
        wires: vec![
            Wire {
                from: Socket { node: 1, port: 0 },
                to: Socket { node: 2, port: 0 },
            },
            Wire {
                from: Socket { node: 3, port: 0 },
                to: Socket { node: 4, port: 0 },
            },
        ],
        ..Default::default()
    };
    graph.nodes[1].inputs[1] = Value::Vector([0., 10., 0.]);
    graph.nodes[3].inputs[1] = Value::Vector([0., 20., 0.]);
    scene.assets.insert(
        "floor-template".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Prefab,
            path: "floor.prefab.json".into(),
        },
    );
    scene.objects[0]
        .blueprints
        .push(bozzard_scene::BlueprintAttachment {
            enabled: true,
            graph,
        });
    let mut demo = SceneDemo::new(&scene).unwrap();
    demo.app.step();
    demo.check_simulation().unwrap();
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(demo.instance().entity("floor").unwrap())
            .unwrap()
            .translation[1],
        10.
    );
    demo.app.step();
    demo.check_simulation().unwrap();
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(demo.instance().entity("floor").unwrap())
            .unwrap()
            .translation[1],
        20.
    );
    let prefab = bozzard_scene::Prefab {
        version: 1,
        name: "Floor".into(),
        root: "floor".into(),
        objects: vec![scene.objects[0].clone()],
        assets: Default::default(),
    };
    demo.with_instance(|i, _| i.register_prefab("floor-template".into(), prefab))
        .unwrap();
    let a = demo
        .with_instance(|i, w| i.spawn_prefab(w, "floor-template", [20., 0., 0.]))
        .unwrap();
    let b = demo
        .with_instance(|i, w| i.spawn_prefab(w, "floor-template", [40., 0., 0.]))
        .unwrap();
    let a_entity = demo.instance().entity(&a).unwrap();
    let b_entity = demo.instance().entity(&b).unwrap();
    demo.app
        .world
        .get_mut::<MeshCollider>(a_entity)
        .unwrap()
        .enabled = false;
    assert!(
        demo.app
            .world
            .get::<MeshCollider>(b_entity)
            .unwrap()
            .enabled
    );
    demo.with_instance(|i, w| i.destroy_prefab(w, &a)).unwrap();
    assert!(demo.app.world.get::<MeshCollider>(a_entity).is_none());
    assert!(demo.instance().entity(&b).is_some());
    demo.set_gameplay_input(GameplayInput::default());
}

#[test]
fn player_spawn_validation_includes_mesh_surfaces() {
    let mut level = Scene::from_json(include_str!("../scenes/first-trail.json")).unwrap();
    let player = level
        .objects
        .iter()
        .find(|o| o.player_controller.is_some())
        .unwrap()
        .clone();
    let mut floor = scene().objects.remove(0);
    floor.id = "unsafe-mesh".into();
    floor.transform.translation = player.transform.translation;
    level.objects.push(floor);
    assert!(format!("{:#}", level.validate().unwrap_err()).contains("spawn point"));
}
