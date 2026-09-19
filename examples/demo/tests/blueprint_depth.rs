use bozzard_demo::SceneDemo;
use bozzard_scene::{
    Blueprint, BlueprintAttachment, BlueprintRuntime, BoxCollider, GameplayInput, Object, Scene,
    Transform,
    blueprint::{
        BlackboardValue as B, Node, NodeKind as K, ObjectRef, PinType, Socket, Value as V,
        VariableScope as S, Wire,
    },
};
use std::collections::{BTreeMap, BTreeSet};
fn scene() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"depth","views":{},"objects":[{"id":"owner","name":"Owner","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#).unwrap()
}
fn graph(nodes: Vec<Node>, wires: &[(u32, usize, u32, usize)]) -> Blueprint {
    Blueprint {
        nodes,
        wires: wires
            .iter()
            .map(|&(a, p, b, q)| Wire {
                from: Socket { node: a, port: p },
                to: Socket { node: b, port: q },
            })
            .collect(),
        ..Default::default()
    }
}
fn node(id: u32, k: K) -> Node {
    Node::new(id, k, [id as f32 * 300., 0.])
}
fn variable(id: u32, k: K, scope: S, name: &str, t: PinType) -> Node {
    let mut n = node(id, k);
    n.scope = scope;
    n.variable = name.into();
    n.value_type = t;
    n.reset_inputs();
    n
}
fn attach(s: &mut Scene, g: Blueprint) {
    s.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph: g,
    });
}
fn tick(d: &mut SceneDemo, dt: f32) {
    d.with_instance(|i, w| i.step_blueprints(w, dt, GameplayInput::default()))
        .unwrap();
}
fn scalar(d: &SceneDemo, scope: S, name: &str) -> V {
    let r = d.app.world.resource::<BlueprintRuntime>().unwrap();
    let b = if scope == S::Scene {
        r.scene_blackboard()
    } else {
        r.object_blackboard("owner").unwrap()
    };
    let B::Scalar(v) = &b[name] else { panic!() };
    v.clone()
}
#[test]
fn shared_scopes_are_typed_ordered_and_isolated_from_local_numbers() {
    let mut s = scene();
    s.blackboard
        .insert("global".into(), B::Scalar(V::Text("initial".into())));
    s.objects[0]
        .blackboard
        .insert("shared".into(), B::Scalar(V::Number(1.)));
    let mut set = variable(2, K::SetVariable, S::Object, "shared", PinType::Number);
    set.inputs[1] = V::Number(42.);
    attach(&mut s, graph(vec![node(1, K::Start), set], &[(1, 0, 2, 0)]));
    let mut print = node(3, K::Print);
    print.inputs[1] = V::Number(-1.);
    attach(
        &mut s,
        graph(
            vec![
                node(1, K::Update),
                variable(2, K::GetVariable, S::Object, "shared", PinType::Number),
                print,
            ],
            &[(1, 0, 3, 0), (2, 0, 3, 1)],
        ),
    );
    let mut global = variable(2, K::SetVariable, S::Scene, "global", PinType::Text);
    global.inputs[1] = V::Text("shared across objects".into());
    attach(
        &mut s,
        graph(vec![node(1, K::Start), global], &[(1, 0, 2, 0)]),
    );
    let mut other = s.objects[0].clone();
    other.id = "other".into();
    other.blueprints.truncate(0);
    s.objects.push(other);
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    assert_eq!(scalar(&d, S::Object, "shared"), V::Number(42.));
    assert_eq!(
        scalar(&d, S::Scene, "global"),
        V::Text("shared across objects".into())
    );
    let r = d.app.world.resource::<BlueprintRuntime>().unwrap();
    assert_eq!(
        r.object_blackboard("other").unwrap()["shared"],
        B::Scalar(V::Number(1.))
    );
    assert!(r.messages.back().unwrap().ends_with("42"));
    let mut bad = s.clone();
    bad.objects[0]
        .blackboard
        .insert("shared".into(), B::Scalar(V::Bool(false)));
    assert!(bad.validate().is_err());
    assert_eq!(Scene::from_json(&s.to_json().unwrap()).unwrap(), s);
}
#[test]
fn lists_support_growth_indexing_removal_and_fail_without_partial_overflow() {
    let mut s = scene();
    s.blackboard.insert(
        "items".into(),
        B::List {
            element: PinType::Number,
            capacity: 2,
            values: vec![],
        },
    );
    s.blackboard.insert("last".into(), B::Scalar(V::Number(0.)));
    let mut push = variable(2, K::ListPush, S::Scene, "items", PinType::Number);
    push.inputs[1] = V::Number(7.);
    let get = variable(3, K::ListGet, S::Scene, "items", PinType::Number);
    let set = variable(4, K::SetVariable, S::Scene, "last", PinType::Number);
    attach(
        &mut s,
        graph(
            vec![node(1, K::Update), push, get, set],
            &[(1, 0, 2, 0), (2, 0, 4, 0), (3, 0, 4, 1)],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    tick(&mut d, 0.1);
    assert_eq!(scalar(&d, S::Scene, "last"), V::Number(7.));
    assert!(
        d.with_instance(|i, w| i.step_blueprints(w, 0.1, GameplayInput::default()))
            .is_err()
    );
    let B::List { values, .. } = &d
        .app
        .world
        .resource::<BlueprintRuntime>()
        .unwrap()
        .scene_blackboard()["items"]
    else {
        panic!()
    };
    assert_eq!(values.len(), 2);
    let mut bad = s.clone();
    if let B::List { values, .. } = bad.blackboard.get_mut("items").unwrap() {
        values.push(V::Bool(true));
    }
    assert!(bad.validate().is_err());
}
#[test]
fn timers_resume_once_cancel_on_disable_and_lifecycle_edges_do_not_repeat() {
    let mut s = scene();
    for name in ["timer", "enabled", "disabled"] {
        s.blackboard.insert(name.into(), B::Scalar(V::Number(0.)));
    }
    let mut delay = node(2, K::Delay);
    delay.inputs[1] = V::Number(0.2);
    let mut fired = variable(3, K::SetVariable, S::Scene, "timer", PinType::Number);
    fired.inputs[1] = V::Number(9.);
    let mut en = variable(5, K::SetVariable, S::Scene, "enabled", PinType::Number);
    en.inputs[1] = V::Number(1.);
    let mut dis = variable(7, K::SetVariable, S::Scene, "disabled", PinType::Number);
    dis.inputs[1] = V::Number(2.);
    attach(
        &mut s,
        graph(
            vec![
                node(1, K::Start),
                delay,
                fired,
                node(4, K::Enable),
                en,
                node(6, K::Disable),
                dis,
            ],
            &[(1, 0, 2, 0), (2, 0, 3, 0), (4, 0, 5, 0), (6, 0, 7, 0)],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    assert_eq!(scalar(&d, S::Scene, "timer"), V::Number(0.));
    tick(&mut d, 0.1);
    assert_eq!(scalar(&d, S::Scene, "timer"), V::Number(0.));
    tick(&mut d, 0.1);
    assert_eq!(scalar(&d, S::Scene, "timer"), V::Number(9.));
    assert_eq!(
        d.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .pending_timers(),
        0
    );
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    d.with_instance(|i, _| i.set_blueprint_enabled("owner", 0, false))
        .unwrap();
    tick(&mut d, 0.1);
    assert_eq!(scalar(&d, S::Scene, "disabled"), V::Number(2.));
    assert_eq!(
        d.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .pending_timers(),
        0
    );
    d.with_instance(|i, _| i.set_blueprint_enabled("owner", 0, true))
        .unwrap();
    for _ in 0..5 {
        tick(&mut d, 0.1);
    }
    assert_eq!(scalar(&d, S::Scene, "timer"), V::Number(0.));
}
fn math(kind: K, inputs: Vec<V>) -> V {
    let mut s = scene();
    let mut op = node(2, kind);
    op.inputs = inputs;
    let t = op.output_pins()[0].1;
    s.blackboard
        .insert("result".into(), B::Scalar(t.default_value()));
    let set = variable(3, K::SetVariable, S::Scene, "result", t);
    attach(
        &mut s,
        graph(
            vec![node(1, K::Start), op, set],
            &[(1, 0, 3, 0), (2, 0, 3, 1)],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    scalar(&d, S::Scene, "result")
}
#[test]
fn scalar_vector_and_angle_math_have_defined_edge_behavior() {
    for (k, a, b, out) in [
        (K::Min, 2., 3., 2.),
        (K::Max, 2., 3., 3.),
        (K::Modulo, -5., 3., 1.),
        (K::Power, 2., 3., 8.),
        (K::Atan2, 0., 1., 0.),
    ] {
        assert_eq!(math(k, vec![V::Number(a), V::Number(b)]), V::Number(out));
    }
    for (k, a, out) in [
        (K::Abs, -3., 3.),
        (K::Cosine, 0., 1.),
        (K::Tangent, 0., 0.),
        (K::ArcSine, 0., 0.),
        (K::ArcCosine, 1., 0.),
        (K::ToRadians, 180., std::f32::consts::PI),
        (K::ToDegrees, std::f32::consts::PI, 180.),
        (K::Floor, 1.7, 1.),
        (K::Ceil, 1.2, 2.),
        (K::Round, 1.7, 2.),
        (K::Sqrt, 9., 3.),
    ] {
        assert_eq!(math(k, vec![V::Number(a)]), V::Number(out));
    }
    assert_eq!(
        math(
            K::Lerp,
            vec![V::Number(10.), V::Number(20.), V::Number(0.25)]
        ),
        V::Number(12.5)
    );
    assert_eq!(
        math(K::Length, vec![V::Vector([3., 4., 0.])]),
        V::Number(5.)
    );
    assert_eq!(
        math(K::Normalize, vec![V::Vector([0.; 3])]),
        V::Vector([0.; 3])
    );
    assert_eq!(
        math(K::Normalize, vec![V::Vector([0., 2., 0.])]),
        V::Vector([0., 1., 0.])
    );
    assert_eq!(
        math(
            K::Dot,
            vec![V::Vector([1., 0., 0.]), V::Vector([0., 1., 0.])]
        ),
        V::Number(0.)
    );
    assert_eq!(
        math(
            K::Cross,
            vec![V::Vector([1., 0., 0.]), V::Vector([0., 1., 0.])]
        ),
        V::Vector([0., 0., 1.])
    );
    assert_eq!(
        math(
            K::Distance,
            vec![V::Vector([1., 0., 0.]), V::Vector([1., 3., 4.])]
        ),
        V::Number(5.)
    );
    assert_eq!(
        math(
            K::LerpVector,
            vec![V::Vector([0.; 3]), V::Vector([2.; 3]), V::Number(0.5)]
        ),
        V::Vector([1.; 3])
    );
}
#[test]
fn query_nodes_return_typed_hits_sorted_lists_and_clear_los() {
    let mut s = scene();
    s.objects.push(Object {
        id: "box".into(),
        name: "box".into(),
        collider: Some(BoxCollider::default()),
        transform: Transform {
            translation: [0., 0., -3.],
            ..Default::default()
        },
        ..Default::default()
    });
    s.blackboard.insert(
        "hits".into(),
        B::List {
            element: PinType::Object,
            capacity: 8,
            values: vec![],
        },
    );
    s.blackboard
        .insert("hit".into(), B::Scalar(V::Object(ObjectRef::None)));
    s.blackboard
        .insert("visible".into(), B::Scalar(V::Bool(false)));
    let mut ray = node(2, K::Raycast);
    ray.inputs[2] = V::Vector([0., 0., -2.]);
    ray.inputs[3] = V::Number(10.);
    let hit = variable(3, K::SetVariable, S::Scene, "hit", PinType::Object);
    let mut sphere = variable(4, K::SphereOverlap, S::Scene, "hits", PinType::Object);
    sphere.inputs[1] = V::Vector([0., 0., -3.]);
    sphere.inputs[2] = V::Number(1.);
    let mut los = node(5, K::LineOfSight);
    los.inputs[2] = V::Vector([5., 0., 0.]);
    let visible = variable(6, K::SetVariable, S::Scene, "visible", PinType::Bool);
    attach(
        &mut s,
        graph(
            vec![node(1, K::Start), ray, hit, sphere, los, visible],
            &[
                (1, 0, 2, 0),
                (2, 0, 3, 0),
                (2, 2, 3, 1),
                (3, 0, 4, 0),
                (4, 0, 5, 0),
                (5, 0, 6, 0),
                (5, 1, 6, 1),
            ],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    assert_eq!(
        scalar(&d, S::Scene, "hit"),
        V::Object(ObjectRef::Id("box".into()))
    );
    assert_eq!(scalar(&d, S::Scene, "visible"), V::Bool(true));
    let B::List { values, .. } = &d
        .app
        .world
        .resource::<BlueprintRuntime>()
        .unwrap()
        .scene_blackboard()["hits"]
    else {
        panic!()
    };
    assert_eq!(values, &[V::Object(ObjectRef::Id("box".into()))]);
    let q = d.instance().query_geometry(&d.app.world).unwrap();
    assert!(
        q.raycast(glam::Vec3::ZERO, glam::Vec3::ZERO, 1., None)
            .is_err()
    );
    assert_eq!(
        q.overlap_box(glam::Vec3::new(0., 0., -3.), glam::Vec3::ONE, None, 8)
            .unwrap(),
        ["box"]
    );
}
#[test]
fn save_restore_preserves_timer_random_and_shared_state_continuation() {
    let mut s = scene();
    s.blackboard.insert("r".into(), B::Scalar(V::Number(0.)));
    s.blackboard
        .insert("done".into(), B::Scalar(V::Bool(false)));
    let mut random = node(2, K::Random);
    random.inputs[1] = V::Number(-5.);
    random.inputs[2] = V::Number(5.);
    let set = variable(3, K::SetVariable, S::Scene, "r", PinType::Number);
    let mut delay = node(5, K::Delay);
    delay.inputs[1] = V::Number(0.3);
    let mut done = variable(6, K::SetVariable, S::Scene, "done", PinType::Bool);
    done.inputs[1] = V::Bool(true);
    attach(
        &mut s,
        graph(
            vec![
                node(1, K::Update),
                random,
                set,
                node(4, K::Start),
                delay,
                done,
            ],
            &[
                (1, 0, 2, 0),
                (2, 0, 3, 0),
                (2, 1, 3, 1),
                (4, 0, 5, 0),
                (5, 0, 6, 0),
            ],
        ),
    );
    let mut a = SceneDemo::new(&s).unwrap();
    tick(&mut a, 0.1);
    let save = a.instance().save_game_json(&a.app.world).unwrap();
    let mut b = SceneDemo::new(&s).unwrap();
    b.with_instance(|i, w| i.load_game_json(w, &save)).unwrap();
    for _ in 0..6 {
        tick(&mut a, 0.1);
        tick(&mut b, 0.1);
        assert_eq!(scalar(&a, S::Scene, "r"), scalar(&b, S::Scene, "r"));
        assert_eq!(scalar(&a, S::Scene, "done"), scalar(&b, S::Scene, "done"));
    }
    let before = b.instance().save_game_json(&b.app.world).unwrap();
    assert!(b.with_instance(|i, w| i.load_game_json(w, "{}")).is_err());
    assert_eq!(before, b.instance().save_game_json(&b.app.world).unwrap());
}
#[test]
fn scene_load_additive_and_restart_nodes_are_headless_and_remap_objects() {
    let mut s = scene();
    let mut level = scene();
    level.name = "level".into();
    level.objects[0].blackboard.insert(
        "self".into(),
        B::Scalar(V::Object(ObjectRef::Id("owner".into()))),
    );
    level.objects[0].transform.translation = [5., 0., 0.];
    s.runtime_scenes.insert("next".into(), level.into());
    let mut add = node(2, K::AddScene);
    add.inputs[1] = V::Text("next".into());
    attach(&mut s, graph(vec![node(1, K::Start), add], &[(1, 0, 2, 0)]));
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    assert!(d.instance().entity("owner").is_some());
    assert!(d.instance().entity("scene-1-owner").is_some());
    assert_eq!(
        d.instance().document().objects[1].blackboard["self"],
        B::Scalar(V::Object(ObjectRef::Id("scene-1-owner".into())))
    );
    d.with_instance(|i, w| i.load_runtime_scene(w, "next", false))
        .unwrap();
    assert!(d.instance().entity("scene-1-owner").is_none());
    assert_eq!(d.instance().document().name, "level");
    let mut s = scene();
    let restart = node(2, K::RestartScene);
    let mut event = node(1, K::InputPressed);
    event.key = bozzard_scene::blueprint::InputKey::parse("A").unwrap();
    attach(&mut s, graph(vec![event, restart], &[(1, 0, 2, 0)]));
    let mut d = SceneDemo::new(&s).unwrap();
    let e = d.instance().entity("owner").unwrap();
    d.app.world.get_mut::<Transform>(e).unwrap().translation = [9.; 3];
    d.with_instance(|i, w| {
        i.step_blueprints(
            w,
            0.1,
            GameplayInput {
                keys: bozzard_scene::keys::bit("A"),
                ..Default::default()
            },
        )
    })
    .unwrap();
    assert_eq!(
        d.app
            .world
            .get::<Transform>(d.instance().entity("owner").unwrap())
            .unwrap()
            .translation,
        [0.; 3]
    );
}
#[test]
fn copy_paste_reroutes_and_stale_wire_diagnostics_preserve_valid_graphs() {
    let mut reroute = node(2, K::Reroute);
    reroute.value_type = PinType::Exec;
    reroute.reset_inputs();
    let mut comment = node(4, K::Comment);
    comment.comment = "Timer branch".into();
    let mut g = graph(
        vec![node(1, K::Start), reroute, node(3, K::Print), comment],
        &[(1, 0, 2, 0), (2, 0, 3, 0)],
    );
    g.validate().unwrap();
    let copy = g.copy_subgraph(&BTreeSet::from([2, 3, 4])).unwrap();
    assert_eq!(copy.wires.len(), 1);
    let pasted = g.paste_subgraph(&copy, [50., 25.]).unwrap();
    assert_eq!(pasted.len(), 3);
    assert!(pasted.iter().all(|id| *id > 4));
    g.validate().unwrap();
    g.nodes[1].value_type = PinType::Bool;
    g.nodes[1].reset_inputs();
    assert_eq!(g.stale_wires().len(), 2);
    assert!(g.validate().is_err());
    let json = serde_json::to_string(&g).unwrap();
    let err = Blueprint::from_json(&json).unwrap_err().to_string();
    assert!(err.contains("type changed"));
    let mut duplicate = copy.clone();
    duplicate.variables.insert("value".into(), 0.);
    duplicate.blackboard = BTreeMap::from([("value".into(), B::Scalar(V::Bool(false)))]);
    assert!(duplicate.validate().is_err());
}

#[test]
fn collision_entry_reports_solver_impulse_and_delayed_contact_context_once() {
    let mut s = scene();
    s.objects[0].transform.translation = [0., 2., 0.];
    s.objects[0].collider = Some(BoxCollider::default());
    s.objects[0].gravity = Some(bozzard_scene::Gravity::default());
    s.objects.push(Object {
        id: "floor".into(),
        name: "floor".into(),
        collider: Some(BoxCollider {
            size: [20., 1., 20.],
            ..Default::default()
        }),
        transform: Transform {
            translation: [0., -0.5, 0.],
            ..Default::default()
        },
        ..Default::default()
    });
    s.blackboard
        .insert("normal".into(), B::Scalar(V::Vector([0.; 3])));
    s.blackboard
        .insert("impulse".into(), B::Scalar(V::Number(0.)));
    s.blackboard
        .insert("other".into(), B::Scalar(V::Object(ObjectRef::None)));
    let mut delay = node(2, K::Delay);
    delay.inputs[1] = V::Number(0.1);
    attach(
        &mut s,
        graph(
            vec![
                node(1, K::CollisionEnter),
                delay,
                variable(3, K::SetVariable, S::Scene, "normal", PinType::Vector),
                variable(4, K::SetVariable, S::Scene, "impulse", PinType::Number),
                variable(5, K::SetVariable, S::Scene, "other", PinType::Object),
            ],
            &[
                (1, 0, 2, 0),
                (2, 0, 3, 0),
                (1, 2, 3, 1),
                (3, 0, 4, 0),
                (1, 3, 4, 1),
                (4, 0, 5, 0),
                (1, 1, 5, 1),
            ],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    for _ in 0..120 {
        d.app.step();
        d.check_simulation().unwrap();
    }
    let V::Vector(normal) = scalar(&d, S::Scene, "normal") else {
        panic!()
    };
    assert!(normal[1] > 0.99, "{normal:?}");
    let V::Number(impulse) = scalar(&d, S::Scene, "impulse") else {
        panic!()
    };
    assert!(impulse > 0.1, "{impulse}");
    assert_eq!(
        scalar(&d, S::Scene, "other"),
        V::Object(ObjectRef::Id("floor".into()))
    );
    assert_eq!(
        d.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .pending_timers(),
        0
    );
    for _ in 0..60 {
        d.app.step();
        d.check_simulation().unwrap();
    }
    assert_eq!(scalar(&d, S::Scene, "impulse"), V::Number(impulse));
}
#[test]
fn destroy_event_can_read_its_owner_and_write_scene_state_before_removal() {
    let mut s = scene();
    s.blackboard
        .insert("destroyed".into(), B::Scalar(V::Vector([0.; 3])));
    s.assets.insert(
        "part".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Prefab,
            path: "part.prefab.json".into(),
        },
    );
    let mut prefab_object = Object {
        id: "root".into(),
        name: "root".into(),
        ..Default::default()
    };
    prefab_object.blueprints.push(BlueprintAttachment {
        enabled: true,
        graph: graph(
            vec![
                node(1, K::Destroy),
                node(2, K::Position),
                variable(3, K::SetVariable, S::Scene, "destroyed", PinType::Vector),
            ],
            &[(1, 0, 3, 0), (2, 0, 3, 1)],
        ),
    });
    let prefab = bozzard_scene::Prefab {
        nested: Default::default(),
        base: None,
        version: 1,
        name: "part".into(),
        root: "root".into(),
        objects: vec![prefab_object],
        assets: Default::default(),
    };
    prefab.validate().unwrap();
    let mut spawn = node(2, K::SpawnPrefab);
    spawn.prefab = "part".into();
    spawn.inputs[1] = V::Vector([1., 2., 3.]);
    let mut delay = node(3, K::Delay);
    delay.inputs[1] = V::Number(0.1);
    attach(
        &mut s,
        graph(
            vec![node(1, K::Start), spawn, delay, node(4, K::DestroyPrefab)],
            &[(1, 0, 2, 0), (2, 0, 3, 0), (3, 0, 4, 0), (2, 1, 4, 1)],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    d.with_instance(|i, _| i.register_prefab("part".into(), prefab))
        .unwrap();
    tick(&mut d, 0.1);
    let id = d
        .instance()
        .document()
        .prefabs
        .keys()
        .next()
        .unwrap()
        .clone();
    assert!(d.instance().entity(&id).is_some());
    tick(&mut d, 0.1);
    assert!(d.instance().entity(&id).is_none());
    assert_eq!(scalar(&d, S::Scene, "destroyed"), V::Vector([1., 2., 3.]));
    let next = d
        .with_instance(|i, w| i.spawn_prefab(w, "part", [4., 5., 6.]))
        .unwrap();
    d.with_instance(|i, w| i.destroy_prefab(w, &next)).unwrap();
    assert_eq!(scalar(&d, S::Scene, "destroyed"), V::Vector([4., 5., 6.]));
}
#[test]
fn save_and_load_nodes_use_persistent_slots_and_reject_path_traversal() {
    let path = std::env::temp_dir().join(format!("bozzard-depth-save-{}", std::process::id()));
    let mut s = scene();
    s.blackboard
        .insert("value".into(), B::Scalar(V::Number(1.)));
    let mut save = node(2, K::SaveGame);
    save.inputs[1] = V::Text("slot".into());
    let mut load = node(4, K::LoadGame);
    load.inputs[1] = V::Text("slot".into());
    let mut press = node(3, K::InputPressed);
    press.key = bozzard_scene::blueprint::InputKey::parse("A").unwrap();
    attach(
        &mut s,
        graph(
            vec![node(1, K::Start), save, press, load],
            &[(1, 0, 2, 0), (3, 0, 4, 0)],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    d.app
        .world
        .insert_resource(bozzard_scene::scene_control::GameSaves::in_directory(
            path.clone(),
        ));
    tick(&mut d, 0.1);
    assert!(path.join("slot.json").is_file());
    let id = d.instance().entity("owner").unwrap();
    d.app.world.get_mut::<Transform>(id).unwrap().translation = [10.; 3];
    d.with_instance(|i, w| {
        i.step_blueprints(
            w,
            0.1,
            GameplayInput {
                keys: bozzard_scene::keys::bit("A"),
                ..Default::default()
            },
        )
    })
    .unwrap();
    assert_eq!(
        d.app
            .world
            .get::<Transform>(d.instance().entity("owner").unwrap())
            .unwrap()
            .translation,
        [0.; 3]
    );
    let mut bad = s;
    bad.objects[0].blueprints[0].graph.nodes[1].inputs[1] = V::Text("../escape".into());
    let mut d = SceneDemo::new(&bad).unwrap();
    assert!(
        d.with_instance(|i, w| i.step_blueprints(w, 0.1, GameplayInput::default()))
            .is_err()
    );
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn all_list_mutations_and_scalar_types_are_usable_and_validated() {
    for t in PinType::VALUES {
        let mut s = scene();
        let v = match t {
            PinType::Number => V::Number(42.),
            PinType::Bool => V::Bool(true),
            PinType::Text => V::Text("item".into()),
            PinType::Vector => V::Vector([1., 2., 3.]),
            PinType::Object => V::Object(ObjectRef::Id("owner".into())),
            _ => unreachable!(),
        };
        s.blackboard.insert(
            "items".into(),
            B::List {
                element: t,
                capacity: 4,
                values: vec![t.default_value()],
            },
        );
        s.blackboard
            .insert("result".into(), B::Scalar(t.default_value()));
        let mut set = variable(2, K::ListSet, S::Scene, "items", t);
        set.inputs[1] = v.clone();
        attach(
            &mut s,
            graph(
                vec![
                    node(1, K::Start),
                    set,
                    variable(3, K::ListGet, S::Scene, "items", t),
                    variable(4, K::SetVariable, S::Scene, "result", t),
                    variable(5, K::ListRemove, S::Scene, "items", t),
                    variable(6, K::ListClear, S::Scene, "items", t),
                ],
                &[
                    (1, 0, 2, 0),
                    (2, 0, 4, 0),
                    (3, 0, 4, 1),
                    (4, 0, 5, 0),
                    (5, 0, 6, 0),
                ],
            ),
        );
        let mut d = SceneDemo::new(&s).unwrap();
        tick(&mut d, 0.1);
        assert_eq!(scalar(&d, S::Scene, "result"), v);
        let B::List { values, .. } = &d
            .app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .scene_blackboard()["items"]
        else {
            panic!()
        };
        assert!(values.is_empty());
    }
}

#[test]
fn query_geometry_and_compiled_programs_are_reused_until_a_transform_changes() {
    let mut s = scene();
    s.objects.push(Object {
        id: "box".into(),
        name: "box".into(),
        collider: Some(BoxCollider::default()),
        ..Default::default()
    });
    let ray = |id| {
        let mut n = node(id, K::Raycast);
        n.inputs[1] = V::Vector([0., 0., 5.]);
        n.inputs[2] = V::Vector([0., 0., -1.]);
        n.inputs[3] = V::Number(10.);
        n
    };
    attach(
        &mut s,
        graph(
            vec![node(1, K::Update), ray(2), ray(3)],
            &[(1, 0, 2, 0), (2, 0, 3, 0)],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    for _ in 0..4 {
        tick(&mut d, 0.1);
        let stats = &d.app.world.resource::<BlueprintRuntime>().unwrap().stats;
        assert_eq!(stats.compiled_graphs, 1);
        assert_eq!(stats.query_geometry_builds, 1);
        assert_eq!(stats.actions, 2);
    }
    let g = &mut s.objects[0].blueprints[0].graph;
    let mut move_box = node(4, K::SetPosition);
    move_box.inputs[1] = V::Vector([0., 0., -10.]);
    move_box.inputs[2] = V::Object(ObjectRef::Id("box".into()));
    g.nodes.push(move_box);
    g.wires[1].to.node = 4;
    g.wires.push(Wire {
        from: Socket { node: 4, port: 0 },
        to: Socket { node: 3, port: 0 },
    });
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    assert_eq!(
        d.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .stats
            .query_geometry_builds,
        2
    );
}
#[test]
fn zero_delay_is_next_tick_and_invalid_math_never_writes_a_variable() {
    let mut s = scene();
    s.blackboard
        .insert("result".into(), B::Scalar(V::Number(123.)));
    let set = variable(3, K::SetVariable, S::Scene, "result", PinType::Number);
    attach(
        &mut s,
        graph(
            vec![node(1, K::Start), node(2, K::Delay), set],
            &[(1, 0, 2, 0), (2, 0, 3, 0)],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    assert_eq!(scalar(&d, S::Scene, "result"), V::Number(123.));
    tick(&mut d, 0.1);
    assert_eq!(scalar(&d, S::Scene, "result"), V::Number(0.));
    for (kind, inputs) in [
        (K::Modulo, vec![V::Number(1.), V::Number(0.)]),
        (K::Sqrt, vec![V::Number(-1.)]),
        (K::Power, vec![V::Number(1e30), V::Number(20.)]),
        (K::ArcSine, vec![V::Number(2.)]),
    ] {
        let mut s = scene();
        s.blackboard
            .insert("result".into(), B::Scalar(V::Number(123.)));
        let mut op = node(2, kind);
        op.inputs = inputs;
        attach(
            &mut s,
            graph(
                vec![
                    node(1, K::Start),
                    op,
                    variable(3, K::SetVariable, S::Scene, "result", PinType::Number),
                ],
                &[(1, 0, 3, 0), (2, 0, 3, 1)],
            ),
        );
        let mut d = SceneDemo::new(&s).unwrap();
        assert!(
            d.with_instance(|i, w| i.step_blueprints(w, 0.1, GameplayInput::default()))
                .is_err()
        );
        assert_eq!(scalar(&d, S::Scene, "result"), V::Number(123.));
    }
}
#[test]
fn scene_loading_node_replaces_at_tick_boundary_and_bad_level_preserves_state() {
    let mut s = scene();
    let mut next = scene();
    next.name = "destination".into();
    next.objects[0].transform.translation = [1., 2., 3.];
    s.runtime_scenes.insert("next".into(), next.into());
    let mut load = node(2, K::LoadScene);
    load.inputs[1] = V::Text("next".into());
    attach(
        &mut s,
        graph(vec![node(1, K::Start), load], &[(1, 0, 2, 0)]),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    assert_eq!(d.instance().document().name, "destination");
    let before = d.instance().capture(&d.app.world).unwrap();
    assert!(
        d.with_instance(|i, w| i.load_runtime_scene(w, "missing", false))
            .is_err()
    );
    assert_eq!(before, d.instance().capture(&d.app.world).unwrap());
}

#[test]
fn checkpoint_restores_dynamic_velocity_and_menu_restart_keeps_current_level_and_store() {
    let mut s = scene();
    s.objects[0].transform.translation = [0., 20., 0.];
    s.objects[0].collider = Some(BoxCollider::default());
    s.objects[0].gravity = Some(bozzard_scene::Gravity::default());
    s.game_flow = Some(Default::default());
    let mut a = SceneDemo::new(&s).unwrap();
    a.game_action(bozzard_scene::GameAction::Start).unwrap();
    for _ in 0..20 {
        a.app.step();
        a.check_simulation().unwrap();
    }
    let save = a.instance().save_game_json(&a.app.world).unwrap();
    let mut b = SceneDemo::new(&s).unwrap();
    b.with_instance(|i, w| i.load_game_json(w, &save)).unwrap();
    for _ in 0..20 {
        a.app.step();
        b.app.step();
        a.check_simulation().unwrap();
        b.check_simulation().unwrap();
        let y = |d: &SceneDemo| {
            d.app
                .world
                .get::<Transform>(d.instance().entity("owner").unwrap())
                .unwrap()
                .translation[1]
        };
        assert!((y(&a) - y(&b)).abs() < 0.001);
    }
    let mut next = s.clone();
    next.name = "next level".into();
    s.runtime_scenes.insert("next".into(), next.into());
    let mut d = SceneDemo::new(&s).unwrap();
    let path = std::env::temp_dir().join("bozzard-test-save-directory");
    d.app
        .world
        .insert_resource(bozzard_scene::scene_control::GameSaves::in_directory(
            path.clone(),
        ));
    d.with_instance(|i, w| i.load_runtime_scene(w, "next", false))
        .unwrap();
    d.game_action(bozzard_scene::GameAction::Restart).unwrap();
    assert_eq!(d.instance().document().name, "next level");
    assert_eq!(
        d.app
            .world
            .resource::<bozzard_scene::scene_control::GameSaves>()
            .unwrap()
            .directory,
        Some(path)
    );
}

#[test]
fn touching_static_collision_uses_surface_normal_instead_of_center_direction() {
    let mut s = scene();
    s.objects[0].transform.translation = [3., 0.5, 0.];
    s.objects[0].collider = Some(BoxCollider::default());
    s.objects.push(Object {
        id: "floor".into(),
        name: "Floor".into(),
        collider: Some(BoxCollider {
            size: [20., 1., 20.],
            ..Default::default()
        }),
        transform: Transform {
            translation: [0., -0.5, 0.],
            ..Default::default()
        },
        ..Default::default()
    });
    s.blackboard
        .insert("normal".into(), B::Scalar(V::Vector([0.; 3])));
    attach(
        &mut s,
        graph(
            vec![
                node(1, K::CollisionEnter),
                variable(2, K::SetVariable, S::Scene, "normal", PinType::Vector),
            ],
            &[(1, 0, 2, 0), (1, 2, 2, 1)],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    assert_eq!(scalar(&d, S::Scene, "normal"), V::Vector([0., 1., 0.]));
}

#[test]
fn destroy_handler_save_is_applied_at_the_transition_boundary() {
    let mut s = scene();
    let mut level = scene();
    level.name = "next".into();
    let mut press = node(1, K::InputPressed);
    press.key = bozzard_scene::blueprint::InputKey::parse("A").unwrap();
    let mut load = node(2, K::LoadGame);
    load.inputs[1] = V::Text("after-transition".into());
    attach(&mut level, graph(vec![press, load], &[(1, 0, 2, 0)]));
    s.runtime_scenes.insert("next".into(), level.into());
    let mut next = node(2, K::LoadScene);
    next.inputs[1] = V::Text("next".into());
    let mut save = node(4, K::SaveGame);
    save.inputs[1] = V::Text("after-transition".into());
    attach(
        &mut s,
        graph(
            vec![node(1, K::Start), next, node(3, K::Destroy), save],
            &[(1, 0, 2, 0), (3, 0, 4, 0)],
        ),
    );
    let mut d = SceneDemo::new(&s).unwrap();
    tick(&mut d, 0.1);
    let e = d.instance().entity("owner").unwrap();
    d.app.world.get_mut::<Transform>(e).unwrap().translation = [7.; 3];
    d.with_instance(|i, w| {
        i.step_blueprints(
            w,
            0.1,
            GameplayInput {
                keys: bozzard_scene::keys::bit("A"),
                ..Default::default()
            },
        )
    })
    .unwrap();
    assert_eq!(d.instance().document().name, "next");
    assert_eq!(
        d.app
            .world
            .get::<Transform>(d.instance().entity("owner").unwrap())
            .unwrap()
            .translation,
        [0.; 3]
    );
}
