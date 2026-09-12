use bozzard_demo::SceneDemo;
use bozzard_scene::{
    Blueprint, BlueprintAttachment, BoxCollider, Scene, Transform, Trigger, TriggerAction,
    blueprint::{Node, NodeKind as K, ObjectRef, Socket, Value, Wire},
};
fn link(g: &mut Blueprint, a: u32, p: usize, b: u32, q: usize) {
    g.connect(Wire {
        from: Socket { node: a, port: p },
        to: Socket { node: b, port: q },
    })
    .unwrap();
}
fn action(event: K, kind: K, target: &str, value: Value) -> Blueprint {
    let mut g = Blueprint {
        nodes: vec![Node::new(1, event, [0.; 2]), Node::new(2, kind, [300., 0.])],
        ..Default::default()
    };
    g.nodes[1].inputs[1] = value;
    g.nodes[1].inputs[2] = Value::Object(ObjectRef::Id(target.into()));
    link(&mut g, 1, 0, 2, 0);
    g
}
fn scene(graph: Blueprint) -> Scene {
    let mut s=Scene::from_json(r#"{"version":1,"name":"Objects","views":{},"objects":[{"id":"plate","name":"Plate","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#).unwrap();
    let mut door = s.objects[0].clone();
    door.id = "door".into();
    s.objects.push(door);
    s.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
    s
}
fn step(d: &mut SceneDemo) {
    d.app.step();
    d.check_simulation().unwrap();
}
fn position(d: &SceneDemo, id: &str) -> [f32; 3] {
    d.app
        .world
        .get::<Transform>(d.instance().entity(id).unwrap())
        .unwrap()
        .translation
}
fn move_to(d: &mut SceneDemo, id: &str, p: [f32; 3]) {
    let e = d.instance().entity(id).unwrap();
    d.app.world.get_mut::<Transform>(e).unwrap().translation = p;
}
#[test]
fn explicit_targets_reads_and_legacy_defaults_survive_roundtrip() {
    let mut g = action(
        K::Start,
        K::SetPosition,
        "door",
        Value::Vector([3., 4., 5.]),
    );
    g.nodes.extend([
        Node::new(3, K::Position, [0.; 2]),
        Node::new(4, K::SetPosition, [0.; 2]),
    ]);
    g.nodes[2].inputs[0] = Value::Object(ObjectRef::Id("door".into()));
    link(&mut g, 2, 0, 4, 0);
    link(&mut g, 3, 0, 4, 1);
    let s = scene(g);
    let s = Scene::from_json(&s.to_json().unwrap()).unwrap();
    let mut d = SceneDemo::new(&s).unwrap();
    step(&mut d);
    assert_eq!(position(&d, "door"), [3., 4., 5.]);
    assert_eq!(position(&d, "plate"), [3., 4., 5.]);
    let old = r#"{"version":1,"name":"Old","nodes":[{"id":1,"position":[0,0],"kind":"start","inputs":[]},{"id":2,"position":[0,0],"kind":"translate","inputs":["exec",{"vector":[1,0,0]}]}],"wires":[{"from":{"node":1,"port":0},"to":{"node":2,"port":0}}]}"#;
    let g = Blueprint::from_json(old).unwrap();
    assert_eq!(g.nodes[1].inputs[2], Value::Object(ObjectRef::SelfObject));
    let mut d = SceneDemo::new(&scene(g)).unwrap();
    step(&mut d);
    assert_eq!(position(&d, "plate"), [1., 0., 0.]);
    assert_eq!(position(&d, "door"), [0.; 3]);
}
#[test]
fn per_body_events_identify_each_collider_and_count_remaining_occupants() {
    let mut enter = action(
        K::BodyEnter,
        K::Translate,
        "door",
        Value::Vector([0., 1., 0.]),
    );
    // Target the entering collider through the event's second output.
    link(&mut enter, 1, 1, 2, 2);
    let mut s = scene(enter);
    s.objects[0].trigger = Some(Trigger {
        action: TriggerAction::Goal,
        ..Default::default()
    });
    let mut exit = action(K::BodyExit, K::SetPosition, "door", Value::Vector([0.; 3]));
    exit.nodes.extend([
        Node::new(3, K::OverlapCount, [0.; 2]),
        Node::new(4, K::MakeVector, [0.; 2]),
    ]);
    link(&mut exit, 3, 0, 4, 0);
    link(&mut exit, 4, 0, 2, 1);
    s.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph: exit,
    });
    for id in ["a", "b"] {
        let mut b = s.objects[1].clone();
        b.id = id.into();
        b.collider = Some(BoxCollider::default());
        s.objects.push(b);
    }
    let mut d = SceneDemo::new(&s).unwrap();
    step(&mut d);
    assert_eq!(position(&d, "a"), [0., 1., 0.]);
    assert_eq!(position(&d, "b"), [0., 1., 0.]);
    // Put both back within the trigger. No second entry while contact remains.
    move_to(&mut d, "a", [0.; 3]);
    move_to(&mut d, "b", [0.; 3]);
    step(&mut d);
    assert_eq!(position(&d, "a"), [0.; 3]);
    move_to(&mut d, "a", [10., 0., 0.]);
    step(&mut d);
    assert_eq!(position(&d, "door"), [1., 0., 0.]);
    let e = d.instance().entity("b").unwrap();
    d.app.world.get_mut::<BoxCollider>(e).unwrap().enabled = false;
    step(&mut d);
    assert_eq!(position(&d, "door"), [0.; 3]);
}
#[test]
fn invalid_targets_fail_clearly_and_other_is_scoped_to_its_event() {
    let mut g = action(K::Start, K::SetPosition, "missing", Value::Vector([0.; 3]));
    assert!(
        scene(g.clone())
            .validate()
            .unwrap_err()
            .to_string()
            .contains("missing object")
    );
    g.nodes[1].inputs[2] = Value::Object(ObjectRef::None);
    let mut d = SceneDemo::new(&scene(g)).unwrap();
    d.app.step();
    assert!(format!("{:#}", d.check_simulation().unwrap_err()).contains("None"));
    let mut g = action(K::Start, K::SetPosition, "door", Value::Vector([1.; 3]));
    g.nodes.extend([
        Node::new(3, K::BodyEnter, [0.; 2]),
        Node::new(4, K::IsValidObject, [0.; 2]),
        Node::new(5, K::Branch, [0.; 2]),
    ]);
    g.wires.clear();
    link(&mut g, 1, 0, 5, 0);
    link(&mut g, 3, 1, 4, 0);
    link(&mut g, 4, 0, 5, 1);
    link(&mut g, 5, 0, 2, 0);
    assert!(
        g.connect(Wire {
            from: Socket { node: 3, port: 1 },
            to: Socket { node: 2, port: 1 }
        })
        .is_err()
    );
    let mut d = SceneDemo::new(&scene(g)).unwrap();
    step(&mut d);
    assert_eq!(position(&d, "door"), [0.; 3]);
}

#[test]
fn pressure_plate_fixture_opens_only_its_own_door_and_resets_without_winning() {
    let s = Scene::from_json(include_str!("../scenes/pressure-plate-lab.json")).unwrap();
    let mut d = SceneDemo::new(&s).unwrap();
    for _ in 0..10 {
        step(&mut d);
    }
    assert_eq!(position(&d, "door-1")[1], 1.5);
    assert_eq!(position(&d, "door-2")[1], 1.5);
    move_to(&mut d, "player", [-3., 0.65, 1.]);
    step(&mut d);
    assert_eq!(position(&d, "door-1")[1], 4.5);
    assert_eq!(position(&d, "door-2")[1], 1.5);
    assert!(
        !d.app
            .world
            .resource::<bozzard_scene::GameplayState>()
            .unwrap()
            .won
    );
    move_to(&mut d, "player", [3., 0.65, 1.]);
    step(&mut d);
    assert_eq!(position(&d, "door-1")[1], 1.5);
    assert_eq!(position(&d, "door-2")[1], 4.5);
    move_to(&mut d, "player", [0., 0.65, 4.]);
    step(&mut d);
    assert_eq!(position(&d, "door-1")[1], 1.5);
    assert_eq!(position(&d, "door-2")[1], 1.5);
    assert_eq!(d.instance().document(), &s);
    let fresh = SceneDemo::new(&s).unwrap();
    assert_eq!(position(&fresh, "door-2")[1], 1.5);
}
