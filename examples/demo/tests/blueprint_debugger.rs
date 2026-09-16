use bozzard_demo::SceneDemo;
use bozzard_scene::{
    Blueprint, BlueprintAttachment, BlueprintDebugger, BlueprintRuntime, BoxCollider, Breakpoint,
    DebugCommand, DebugPause, GameplayInput, MeshCollider, Object, Scene, Spin, Transform,
    TriangleMesh, Trigger, TriggerAction,
    blueprint::{
        BlackboardValue as B, Node, NodeKind as K, ObjectRef, PinType, Socket, Value as V,
        VariableScope as Scope, Wire,
    },
};
use std::time::Duration;
fn node(id: u32, kind: K) -> Node {
    Node::new(id, kind, [id as f32 * 300., 0.])
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
fn scene(graphs: Vec<Blueprint>) -> Scene {
    let mut scene = Scene::from_json(r#"{"version":1,"name":"debug test","views":{},"objects":[{"id":"owner","name":"Owner","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#).unwrap();
    scene.objects[0].blueprints = graphs
        .into_iter()
        .map(|graph| BlueprintAttachment {
            graph,
            enabled: true,
        })
        .collect();
    scene
}
fn translate(id: u32, x: f32) -> Node {
    let mut n = node(id, K::Translate);
    n.inputs[1] = V::Vector([x, 0., 0.]);
    n
}
fn position(d: &SceneDemo) -> Transform {
    *d.app
        .world
        .get::<Transform>(d.instance().entity("owner").unwrap())
        .unwrap()
}
fn enable(d: &mut SceneDemo, breakpoints: &[(usize, u32)]) {
    d.app
        .world
        .insert_resource(BlueprintDebugger::new(breakpoints.iter().map(
            |&(attachment, node)| Breakpoint {
                scene: "debug test".into(),
                object: "owner".into(),
                attachment,
                node,
            },
        )));
}
#[test]
fn breakpoints_pause_before_actions_and_steps_do_not_repeat_earlier_systems() {
    let graph = graph(
        vec![node(1, K::Start), translate(2, 1.), translate(3, 10.)],
        &[(1, 0, 2, 0), (2, 0, 3, 0)],
    );
    let mut document = scene(vec![graph]);
    document.objects[0].spin = Some(Spin([0., 60., 0.]));
    let mut d = SceneDemo::new(&document).unwrap();
    enable(&mut d, &[(0, 2)]);
    d.app.step();
    assert!(d.app.is_paused());
    assert_eq!(d.app.ticks(), 0);
    assert!(
        d.instance()
            .save_game_json(&d.app.world)
            .unwrap_err()
            .to_string()
            .contains("suspended")
    );
    let paused = position(&d);
    assert_eq!(paused.translation, [0.; 3]);
    assert!(paused.rotation_degrees[1] > 0.);
    let change_tick = d.app.world.change_tick();
    for _ in 0..4 {
        d.app.advance(Duration::from_secs(10));
    }
    assert_eq!(position(&d), paused);
    d.debug_command(DebugCommand::StepNode).unwrap();
    assert_eq!(position(&d).translation, [1., 0., 0.]);
    assert_eq!(position(&d).rotation_degrees, paused.rotation_degrees);
    assert_eq!(d.app.ticks(), 0);
    assert_eq!(d.app.world.change_tick(), change_tick);
    let debugger = d.app.world.resource::<BlueprintDebugger>().unwrap();
    assert_eq!(debugger.paused, Some(DebugPause::NodeStep));
    assert_eq!(debugger.current.as_ref().unwrap().location.node, 3);
    let watch = d
        .instance()
        .inspect_blueprint(&d.app.world, "owner", 0, Some(3))
        .unwrap();
    assert!(
        watch
            .node
            .unwrap()
            .pins
            .iter()
            .any(|p| p.name == "Value" && p.value == "[10, 0, 0]")
    );
    d.debug_command(DebugCommand::StepTick).unwrap();
    assert!(d.app.is_paused());
    assert_eq!(d.app.ticks(), 1);
    assert_eq!(position(&d).translation, [11., 0., 0.]);
    assert_eq!(position(&d).rotation_degrees, paused.rotation_degrees);
    d.debug_command(DebugCommand::Continue).unwrap();
    d.app.step();
    assert!(!d.app.is_paused());
    assert_eq!(d.app.ticks(), 2);
    assert_eq!(position(&d).translation, [11., 0., 0.]);
    assert_eq!(d.instance().document(), &document);
}
#[test]
fn fully_stepped_execution_matches_normal_order_branch_delay_and_shared_state() {
    let mut delay = node(4, K::Delay);
    delay.inputs[1] = V::Number(0.03);
    let mut branch = node(2, K::Branch);
    branch.inputs[1] = V::Bool(false);
    let g = graph(
        vec![
            node(1, K::Start),
            branch,
            translate(3, 100.),
            delay,
            translate(5, 7.),
            translate(6, 2.),
        ],
        &[
            (1, 0, 2, 0),
            (2, 0, 3, 0),
            (2, 1, 4, 0),
            (4, 0, 5, 0),
            (1, 0, 6, 0),
        ],
    );
    let mut set = node(2, K::SetVariable);
    set.scope = Scope::Scene;
    set.variable = "count".into();
    set.inputs[1] = V::Number(3.);
    let g2 = graph(vec![node(1, K::Update), set], &[(1, 0, 2, 0)]);
    let mut document = scene(vec![g, g2]);
    document
        .blackboard
        .insert("count".into(), B::Scalar(V::Number(0.)));
    let mut baseline = SceneDemo::new(&document).unwrap();
    let mut stepped = SceneDemo::new(&document).unwrap();
    enable(&mut stepped, &[]);
    stepped.debug_command(DebugCommand::Pause).unwrap();
    for tick in 1..=8 {
        baseline.app.step();
        baseline.check_simulation().unwrap();
        for _ in 0..100 {
            stepped.debug_command(DebugCommand::StepNode).unwrap();
            if stepped.app.ticks() == tick {
                break;
            }
        }
        assert_eq!(stepped.app.ticks(), tick);
        assert!(stepped.app.is_paused());
        assert_eq!(position(&baseline), position(&stepped));
        assert_eq!(
            baseline
                .instance()
                .save_game_json(&baseline.app.world)
                .unwrap(),
            stepped
                .instance()
                .save_game_json(&stepped.app.world)
                .unwrap()
        );
    }
    assert_eq!(position(&stepped).translation, [9., 0., 0.]);
    let trace = &stepped
        .app
        .world
        .resource::<BlueprintDebugger>()
        .unwrap()
        .trace;
    assert!(
        !trace
            .iter()
            .any(|n| n.location.attachment == 0 && n.location.node == 3)
    );
    assert_eq!(
        trace
            .iter()
            .filter(|n| n.location.attachment == 0 && n.location.node == 5)
            .count(),
        1
    );
    let watch = stepped
        .instance()
        .inspect_blueprint(&stepped.app.world, "owner", 1, Some(2))
        .unwrap();
    assert!(
        watch
            .variables
            .iter()
            .any(|v| v.scope == "Scene" && v.name == "count" && v.value == "3")
    );
}
#[test]
fn event_breakpoint_continue_skips_once_and_trace_is_bounded() {
    let document = scene(vec![graph(
        vec![node(1, K::Update), translate(2, 1.)],
        &[(1, 0, 2, 0)],
    )]);
    let mut d = SceneDemo::new(&document).unwrap();
    enable(&mut d, &[(0, 1)]);
    d.app.step();
    assert!(d.app.is_paused());
    assert_eq!(position(&d).translation[0], 0.);
    d.debug_command(DebugCommand::Continue).unwrap();
    d.app.step();
    assert!(!d.app.is_paused());
    assert_eq!(position(&d).translation[0], 1.);
    d.app.step();
    assert!(d.app.is_paused());
    assert_eq!(position(&d).translation[0], 1.);
    d.app
        .world
        .resource_mut::<BlueprintDebugger>()
        .unwrap()
        .breakpoints
        .clear();
    d.debug_command(DebugCommand::Continue).unwrap();
    for _ in 0..300 {
        d.app.step();
    }
    let debugger = d.app.world.resource::<BlueprintDebugger>().unwrap();
    assert_eq!(debugger.trace.len(), 256);
    assert!(debugger.discarded > 0);
}
#[test]
fn failures_keep_the_origin_and_require_stop_instead_of_replaying_the_action() {
    let mut bad = node(2, K::SetText);
    bad.inputs[1] = V::Text("no text component".into());
    let document = scene(vec![graph(vec![node(1, K::Start), bad], &[(1, 0, 2, 0)])]);
    let mut d = SceneDemo::new(&document).unwrap();
    enable(&mut d, &[]);
    d.app.step();
    assert!(d.app.is_paused());
    assert!(d.check_simulation().is_err());
    let debugger = d.app.world.resource::<BlueprintDebugger>().unwrap();
    assert_eq!(debugger.paused, Some(DebugPause::Error));
    assert_eq!(debugger.current.as_ref().unwrap().location.node, 2);
    d.debug_command(DebugCommand::Continue).unwrap();
    assert!(d.app.is_paused());
    assert!(
        !d.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .suspended()
    );
}
#[test]
fn immediate_ui_dispatch_can_pause_and_resume_without_advancing_time() {
    use bozzard_scene::middleware::signals::{Kind, Signal, Signals};
    let document = scene(vec![graph(
        vec![node(1, K::UiEvent), translate(2, 4.)],
        &[(1, 0, 2, 0)],
    )]);
    let mut d = SceneDemo::new(&document).unwrap();
    enable(&mut d, &[(0, 2)]);
    let mut signals = Signals::default();
    signals
        .emit(
            "owner",
            Signal {
                kind: Kind::Ui,
                name: "pressed".into(),
                value: 0.,
                other: None,
            },
        )
        .unwrap();
    d.app.world.insert_resource(signals);
    d.with_instance(|i, w| i.dispatch_ui_blueprints(w)).unwrap();
    assert!(d.app.is_paused());
    assert_eq!(d.app.ticks(), 0);
    assert_eq!(position(&d).translation[0], 0.);
    d.debug_command(DebugCommand::StepNode).unwrap();
    assert!(d.app.is_paused());
    assert_eq!(d.app.ticks(), 0);
    assert_eq!(position(&d).translation[0], 4.);
    d.with_instance(|i, w| i.step_blueprints(w, 0.1, GameplayInput::default()))
        .unwrap();
    assert_eq!(position(&d).translation[0], 4.);
}

#[test]
fn spawned_instance_destroy_handlers_suspend_before_removal_and_keep_their_watches() {
    use bozzard_scene::{AssetKind, AssetSource, Object, Prefab};
    let mut set = node(2, K::SetVariable);
    set.scope = Scope::Scene;
    set.variable = "destroyed".into();
    set.inputs[1] = V::Number(1.);
    let prefab = Prefab {
        version: 1,
        name: "part".into(),
        root: "root".into(),
        assets: Default::default(),
        objects: vec![Object {
            id: "root".into(),
            name: "Part".into(),
            blueprints: vec![BlueprintAttachment {
                enabled: true,
                graph: graph(vec![node(1, K::Destroy), set], &[(1, 0, 2, 0)]),
            }],
            ..Default::default()
        }],
    };
    let mut spawn = node(2, K::SpawnPrefab);
    spawn.prefab = "part".into();
    let mut delay = node(3, K::Delay);
    delay.inputs[1] = V::Number(0.);
    let mut document = scene(vec![graph(
        vec![node(1, K::Start), spawn, delay, node(4, K::DestroyPrefab)],
        &[(1, 0, 2, 0), (2, 0, 3, 0), (3, 0, 4, 0), (2, 1, 4, 1)],
    )]);
    document
        .blackboard
        .insert("destroyed".into(), B::Scalar(V::Number(0.)));
    document.assets.insert(
        "part".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "part.prefab.json".into(),
        },
    );
    let mut d = SceneDemo::new(&document).unwrap();
    d.with_instance(|i, _| i.register_prefab("part".into(), prefab))
        .unwrap();
    enable(&mut d, &[]);
    d.app.step();
    let id = d
        .instance()
        .document()
        .prefabs
        .values()
        .next()
        .unwrap()
        .members
        .values()
        .next()
        .unwrap()
        .clone();
    d.app
        .world
        .resource_mut::<BlueprintDebugger>()
        .unwrap()
        .breakpoints
        .insert(Breakpoint {
            scene: "debug test".into(),
            object: id.clone(),
            attachment: 0,
            node: 2,
        });
    d.app.step();
    assert!(d.app.is_paused());
    assert_eq!(d.app.ticks(), 1);
    assert!(d.instance().entity(&id).is_some());
    let watch = d
        .instance()
        .inspect_blueprint(&d.app.world, &id, 0, Some(2))
        .unwrap();
    assert!(
        watch
            .variables
            .iter()
            .any(|v| v.name == "destroyed" && v.value == "0")
    );
    d.debug_command(DebugCommand::StepNode).unwrap();
    assert_eq!(d.app.ticks(), 2);
    assert!(d.instance().entity(&id).is_none());
    assert_eq!(
        d.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .scene_blackboard()["destroyed"],
        B::Scalar(V::Number(1.))
    );
    let trace = &d.app.world.resource::<BlueprintDebugger>().unwrap().trace;
    assert_eq!(
        trace
            .iter()
            .filter(|s| s.location.object == id && s.location.node == 2)
            .count(),
        1
    );
    let next = d
        .with_instance(|i, w| i.spawn_prefab(w, "part", [0.; 3]))
        .unwrap();
    d.debug_command(DebugCommand::Continue).unwrap();
    d.app.step();
    let bp = Breakpoint {
        scene: "debug test".into(),
        object: next.clone(),
        attachment: 0,
        node: 2,
    };
    d.app
        .world
        .resource_mut::<BlueprintDebugger>()
        .unwrap()
        .breakpoints
        .insert(bp);
    d.with_instance(|i, w| i.destroy_prefab(w, &next)).unwrap();
    assert!(!d.app.is_paused(), "direct host destruction is atomic");
    assert!(
        d.app
            .world
            .resource::<BlueprintDebugger>()
            .unwrap()
            .trace
            .iter()
            .any(|s| s.atomic && s.location.object == next && s.location.node == 2)
    );
    assert!(
        d.app
            .world
            .resource::<bozzard_diagnostics::Diagnostics>()
            .unwrap()
            .console
            .events
            .iter()
            .any(|e| e.message.contains("atomic teardown"))
    );
}

#[test]
fn scene_scoped_breakpoints_do_not_hit_an_unrelated_graph_after_loading() {
    let mut load = node(2, K::LoadScene);
    load.inputs[1] = V::Text("next".into());
    let mut document = scene(vec![graph(vec![node(1, K::Start), load], &[(1, 0, 2, 0)])]);
    let mut next = scene(vec![graph(
        vec![node(1, K::Start), translate(2, 10.)],
        &[(1, 0, 2, 0)],
    )]);
    next.name = "next".into();
    document
        .runtime_scenes
        .insert("next".into(), std::sync::Arc::new(next));
    let mut d = SceneDemo::new(&document).unwrap();
    enable(&mut d, &[(0, 2)]);
    d.app.step();
    assert!(d.app.is_paused());
    d.debug_command(DebugCommand::Continue).unwrap();
    d.app.step();
    d.check_simulation().unwrap();
    d.app.step();
    d.check_simulation().unwrap();
    assert!(!d.app.is_paused());
    assert_eq!(position(&d).translation[0], 10.);
    assert_eq!(d.instance().document().name, "next");
}

#[test]
fn paused_input_is_discarded_and_watch_previews_are_utf8_safe_and_bounded() {
    let mut log = node(2, K::LogInfo);
    log.inputs[1] = V::Text("🙂".repeat(1024));
    let document = scene(vec![graph(vec![node(1, K::Start), log], &[(1, 0, 2, 0)])]);
    let mut d = SceneDemo::new(&document).unwrap();
    enable(&mut d, &[(0, 2)]);
    d.app.step();
    assert!(d.app.is_paused());
    d.set_gameplay_input(GameplayInput {
        movement: [1., 0.],
        ..Default::default()
    });
    assert_eq!(
        d.app.world.resource::<GameplayInput>().unwrap().movement,
        [0.; 2]
    );
    let watch = d
        .instance()
        .inspect_blueprint(&d.app.world, "owner", 0, Some(2))
        .unwrap();
    let pin = watch
        .node
        .unwrap()
        .pins
        .into_iter()
        .find(|p| p.kind == bozzard_scene::blueprint::PinType::Text)
        .unwrap();
    assert!(pin.value.len() <= 515);
    assert!(pin.value.ends_with('…'));
}

#[test]
fn traced_middleware_simulation_matches_the_normal_interpreter() {
    let document = Scene::from_json(include_str!("../scenes/middleware-lab.json")).unwrap();
    let mut normal = SceneDemo::new(&document).unwrap();
    let mut traced = SceneDemo::new(&document).unwrap();
    traced.app.world.insert_resource(BlueprintDebugger::new([]));
    for tick in 0..120 {
        normal.app.step();
        traced.app.step();
        normal.check_simulation().unwrap();
        traced.check_simulation().unwrap();
        if tick % 10 == 0 {
            assert_eq!(
                normal.instance().save_game_json(&normal.app.world).unwrap(),
                traced.instance().save_game_json(&traced.app.world).unwrap(),
                "tick {tick}"
            );
        }
    }
}

#[test]
fn watches_keep_event_inputs_after_a_tick_and_stale_breakpoint_guards_do_not_pause() {
    let document = scene(vec![Blueprint::spinning()]);
    let mut d = SceneDemo::new(&document).unwrap();
    enable(&mut d, &[(0, 4)]);
    let location = d
        .app
        .world
        .resource::<BlueprintDebugger>()
        .unwrap()
        .breakpoints
        .first()
        .unwrap()
        .clone();
    d.app
        .world
        .resource_mut::<BlueprintDebugger>()
        .unwrap()
        .breakpoint_guards
        .insert(location, ("Another graph".into(), K::Rotate));
    d.app.step();
    assert!(!d.app.is_paused());
    d.debug_command(DebugCommand::Pause).unwrap();
    let watch = d
        .instance()
        .inspect_blueprint(&d.app.world, "owner", 0, Some(4))
        .unwrap()
        .node
        .unwrap();
    assert_eq!(watch.event, "On Update");
    assert!(
        watch
            .pins
            .iter()
            .any(|p| p.name == "Value" && p.value.starts_with("[0, 0.75"))
    );
    d.debug_command(DebugCommand::StepTick).unwrap();
    assert_eq!(d.app.ticks(), 2);
}

#[test]
fn stepped_trigger_events_respect_both_collision_masks_for_boxes_and_meshes() {
    for mesh in [false, true] {
        for (sensor_mask, body_mask, expected_x) in [(2, 1, 1.), (0, 1, 0.), (2, 0, 0.)] {
            let mut document = scene(vec![graph(
                vec![node(1, K::TriggerEnter), translate(2, 1.)],
                &[(1, 0, 2, 0)],
            )]);
            document.objects[0].trigger = Some(Trigger {
                volume: BoxCollider {
                    layers: 1,
                    mask: sensor_mask,
                    ..Default::default()
                },
                action: TriggerAction::Sensor,
            });
            let mut body = Object {
                id: "body".into(),
                name: "Body".into(),
                ..Default::default()
            };
            if mesh {
                body.mesh_collider = Some(MeshCollider {
                    enabled: true,
                    layers: 2,
                    mask: body_mask,
                    mesh: TriangleMesh::new(vec![[[-1., 0., -1.], [1., 0., -1.], [0., 0., 1.]]])
                        .unwrap(),
                });
            } else {
                body.collider = Some(BoxCollider {
                    layers: 2,
                    mask: body_mask,
                    ..Default::default()
                });
            }
            document.objects.push(body);
            let mut normal = SceneDemo::new(&document).unwrap();
            let mut stepped = SceneDemo::new(&document).unwrap();
            enable(&mut stepped, &[]);
            stepped.debug_command(DebugCommand::Pause).unwrap();
            normal.app.step();
            normal.check_simulation().unwrap();
            for _ in 0..4 {
                stepped.debug_command(DebugCommand::StepNode).unwrap();
                if stepped.app.ticks() == 1 {
                    break;
                }
            }
            assert_eq!(stepped.app.ticks(), 1);
            assert_eq!(position(&normal).translation[0], expected_x);
            assert_eq!(position(&stepped), position(&normal));
            assert_eq!(
                normal.instance().save_game_json(&normal.app.world).unwrap(),
                stepped
                    .instance()
                    .save_game_json(&stepped.app.world)
                    .unwrap(),
                "mesh={mesh}, masks={sensor_mask}/{body_mask}"
            );
        }
    }
}

#[test]
fn stepped_spatial_queries_keep_all_layers_and_reuse_geometry() {
    let mut ray = node(2, K::Raycast);
    ray.inputs[2] = V::Vector([0., 0., -1.]);
    ray.inputs[3] = V::Number(10.);
    let mut hit = node(3, K::SetVariable);
    hit.scope = Scope::Scene;
    hit.variable = "hit".into();
    hit.value_type = PinType::Object;
    hit.reset_inputs();
    let overlap = |id, kind, variable: &str| {
        let mut n = node(id, kind);
        n.scope = Scope::Scene;
        n.variable = variable.into();
        n.inputs[1] = V::Vector([0., 0., -3.]);
        n.inputs[2] = if kind == K::SphereOverlap {
            V::Number(1.)
        } else {
            V::Vector([2.; 3])
        };
        n
    };
    let mut los = node(6, K::LineOfSight);
    los.inputs[2] = V::Vector([0., 0., -5.]);
    let mut visible = node(7, K::SetVariable);
    visible.scope = Scope::Scene;
    visible.variable = "visible".into();
    visible.value_type = PinType::Bool;
    visible.reset_inputs();
    let mut document = scene(vec![graph(
        vec![
            node(1, K::Start),
            ray,
            hit,
            overlap(4, K::SphereOverlap, "sphere"),
            overlap(5, K::BoxOverlap, "box"),
            los,
            visible,
        ],
        &[
            (1, 0, 2, 0),
            (2, 0, 3, 0),
            (2, 2, 3, 1),
            (3, 0, 4, 0),
            (4, 0, 5, 0),
            (5, 0, 6, 0),
            (6, 0, 7, 0),
            (6, 1, 7, 1),
        ],
    )]);
    document.objects.push(Object {
        id: "body".into(),
        name: "Body".into(),
        transform: Transform {
            translation: [0., 0., -3.],
            ..Default::default()
        },
        collider: Some(BoxCollider {
            layers: 1 << 7,
            ..Default::default()
        }),
        ..Default::default()
    });
    document
        .blackboard
        .insert("hit".into(), B::Scalar(V::Object(ObjectRef::None)));
    document
        .blackboard
        .insert("visible".into(), B::Scalar(V::Bool(true)));
    for variable in ["sphere", "box"] {
        document.blackboard.insert(
            variable.into(),
            B::List {
                element: PinType::Object,
                capacity: 8,
                values: vec![],
            },
        );
    }
    let mut normal = SceneDemo::new(&document).unwrap();
    let mut stepped = SceneDemo::new(&document).unwrap();
    enable(&mut stepped, &[]);
    stepped.debug_command(DebugCommand::Pause).unwrap();
    normal.app.step();
    normal.check_simulation().unwrap();
    for _ in 0..10 {
        stepped.debug_command(DebugCommand::StepNode).unwrap();
        if stepped.app.ticks() == 1 {
            break;
        }
    }
    assert_eq!(stepped.app.ticks(), 1);
    assert_eq!(
        normal.instance().save_game_json(&normal.app.world).unwrap(),
        stepped
            .instance()
            .save_game_json(&stepped.app.world)
            .unwrap()
    );
    for d in [&normal, &stepped] {
        let runtime = d.app.world.resource::<BlueprintRuntime>().unwrap();
        let board = runtime.scene_blackboard();
        assert_eq!(
            board["hit"],
            B::Scalar(V::Object(ObjectRef::Id("body".into())))
        );
        assert_eq!(board["visible"], B::Scalar(V::Bool(false)));
        for variable in ["sphere", "box"] {
            let B::List { values, .. } = &board[variable] else {
                panic!("overlap result must remain a list")
            };
            assert_eq!(values, &[V::Object(ObjectRef::Id("body".into()))]);
        }
        assert_eq!(runtime.stats.query_geometry_builds, 1);
        assert_eq!(runtime.stats.actions, 6);
    }
}
