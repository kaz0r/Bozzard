use bozzard_demo::SceneDemo;
use bozzard_scene::{
    Blueprint, BlueprintAttachment, GameplayInput, Scene, Transform,
    blueprint::{Node, NodeKind as K, Socket, Value, Wire},
};

fn link(graph: &mut Blueprint, from: u32, port: usize, to: u32, input: usize) {
    graph
        .connect(Wire {
            from: Socket { node: from, port },
            to: Socket {
                node: to,
                port: input,
            },
        })
        .unwrap();
}
fn action(event: K, kind: K, value: Value) -> Blueprint {
    let mut g = Blueprint {
        nodes: vec![
            Node::new(1, event, [0., 0.]),
            Node::new(2, kind, [300., 0.]),
        ],
        ..Blueprint::default()
    };
    g.nodes[1].inputs[1] = value;
    link(&mut g, 1, 0, 2, 0);
    g
}
fn scene(graphs: Vec<Blueprint>) -> Scene {
    let mut s = Scene::from_json(r#"{"version":1,"name":"Blueprint test","views":{},"objects":[{"id":"owner","name":"Owner","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}}]}"#).unwrap();
    s.objects[0].blueprints = graphs
        .into_iter()
        .map(|graph| BlueprintAttachment {
            graph,
            enabled: true,
        })
        .collect();
    s
}
fn transform(demo: &SceneDemo) -> Transform {
    *demo
        .app
        .world
        .get(demo.instance().entity("owner").unwrap())
        .unwrap()
}

#[test]
fn demo_coral_cube_bounces_only_on_z_without_drift() {
    let scene = Scene::from_json(include_str!("../scenes/blueprint-lab.json")).unwrap();
    let mut demo = SceneDemo::new(&scene).unwrap();
    let entity = demo.instance().entity("coral-cube").unwrap();
    let initial = *demo.app.world.get::<Transform>(entity).unwrap();
    for tick in 1..=240 {
        demo.app.step();
        demo.check_simulation().unwrap();
        let actual = demo.app.world.get::<Transform>(entity).unwrap();
        assert_eq!(&actual.translation[..2], &initial.translation[..2]);
        assert_eq!(actual.rotation_degrees, initial.rotation_degrees);
        assert_eq!(actual.scale, initial.scale);
        assert!((-2.0001..=0.0001).contains(&actual.translation[2]));
        if tick % 30 == 0 {
            let expected_z = [-1., 0., -1., -2.][(tick / 30) % 4];
            assert!(
                (actual.translation[2] - expected_z).abs() < 0.0001,
                "tick {tick}: {:?}",
                actual.translation
            );
        }
    }
    assert_eq!(demo.instance().document(), &scene);
}

#[test]
fn graphs_execute_in_order_with_independent_state_and_coded_behavior() {
    let start = action(K::Start, K::Translate, Value::Vector([1., 0., 0.]));
    let mut counter = action(K::Update, K::SetVariable, Value::Number(0.));
    counter.nodes.extend([
        Node::new(3, K::GetVariable, [0., 0.]),
        Node::new(4, K::Add, [0., 0.]),
        Node::new(5, K::MakeVector, [0., 0.]),
        Node::new(6, K::Translate, [0., 0.]),
    ]);
    counter.nodes[3].inputs[1] = Value::Number(1.);
    link(&mut counter, 3, 0, 4, 0);
    link(&mut counter, 4, 0, 2, 1);
    link(&mut counter, 2, 0, 6, 0);
    link(&mut counter, 3, 0, 5, 0);
    link(&mut counter, 5, 0, 6, 1);
    let mut s = scene(vec![start, counter.clone(), counter, Blueprint::spinning()]);
    s.objects[0].spin = Some(bozzard_scene::Spin([0., 15., 0.]));
    let mut sibling = s.objects[0].clone();
    sibling.id = "sibling".into();
    sibling.blueprints[1].enabled = false;
    s.objects.push(sibling);
    let mut demo = SceneDemo::new(&s).unwrap();
    for _ in 0..3 {
        demo.app.step();
        demo.check_simulation().unwrap();
    }
    // Each private counter adds 1+2+3, Start runs only once. Spin and Blueprint rotate coexist.
    assert_eq!(transform(&demo).translation, [13., 0., 0.]);
    assert!((transform(&demo).rotation_degrees[1] - 3.).abs() < 0.001);
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(demo.instance().entity("sibling").unwrap())
            .unwrap()
            .translation,
        [7., 0., 0.]
    );
    assert_eq!(demo.instance().document(), &s);
    let captured = demo.instance().capture(&demo.app.world).unwrap();
    assert_eq!(captured.objects[0].blueprints, s.objects[0].blueprints);
    let restarted = SceneDemo::new(&s).unwrap();
    assert_eq!(transform(&restarted).translation, [0.; 3]);
    let ordered = scene(vec![
        action(K::Start, K::SetPosition, Value::Vector([10., 0., 0.])),
        action(K::Start, K::Translate, Value::Vector([2., 0., 0.])),
    ]);
    let mut demo = SceneDemo::new(&ordered).unwrap();
    demo.app.step();
    assert_eq!(transform(&demo).translation[0], 12.);
}

#[test]
fn input_edges_and_focus_clear_work_without_a_coded_player() {
    let s = scene(vec![action(
        K::InputPressed,
        K::Translate,
        Value::Vector([1., 0., 0.]),
    )]);
    let mut demo = SceneDemo::new(&s).unwrap();
    assert!(demo.gameplay().is_none() && demo.accepts_gameplay_input());
    demo.set_gameplay_input(GameplayInput {
        jump: true,
        ..Default::default()
    });
    demo.set_gameplay_input(GameplayInput::default());
    for _ in 0..4 {
        demo.app.step();
    }
    assert_eq!(transform(&demo).translation[0], 1.);
    demo.set_gameplay_input(GameplayInput {
        jump: true,
        ..Default::default()
    });
    demo.clear_gameplay_input();
    demo.app.step();
    assert_eq!(transform(&demo).translation[0], 1.);
    demo.set_gameplay_input(GameplayInput {
        jump: true,
        ..Default::default()
    });
    demo.app.step();
    assert_eq!(transform(&demo).translation[0], 2.);
}

#[test]
fn overlaps_branch_visibility_and_light_actions_are_real_runtime_changes() {
    let mut enter = action(K::TriggerEnter, K::SetColor, Value::Vector([0., 1., 0.]));
    enter.nodes.push(Node::new(3, K::Branch, [0., 0.]));
    enter.nodes[2].inputs[1] = Value::Bool(true);
    enter.wires.clear();
    link(&mut enter, 1, 0, 3, 0);
    link(&mut enter, 3, 0, 2, 0);
    let mut s = scene(vec![
        enter,
        action(K::TriggerExit, K::SetColor, Value::Vector([1., 0., 0.])),
        action(K::Start, K::SetVisible, Value::Bool(false)),
        action(K::Start, K::SetLightIntensity, Value::Number(42.)),
    ]);
    s.objects[0].trigger = Some(bozzard_scene::Trigger {
        action: bozzard_scene::TriggerAction::Goal,
        ..Default::default()
    });
    s.objects[0].light = Some(Default::default());
    let mut solid = s.objects[0].clone();
    solid.id = "solid".into();
    solid.trigger = None;
    solid.light = None;
    solid.blueprints.clear();
    solid.collider = Some(Default::default());
    s.objects.push(solid);
    let mut demo = SceneDemo::new(&s).unwrap();
    demo.app.step();
    demo.check_simulation().unwrap();
    let e = demo.instance().entity("owner").unwrap();
    assert_eq!(
        demo.app
            .world
            .get::<bozzard_scene::Drawable>(e)
            .unwrap()
            .color,
        [0., 1., 0.]
    );
    assert!(
        demo.app
            .world
            .get::<bozzard_scene::BlueprintHidden>(e)
            .unwrap()
            .0
    );
    assert_eq!(
        demo.app
            .world
            .get::<bozzard_scene::Light>(e)
            .unwrap()
            .intensity,
        42.
    );
    demo.app.world.get_mut::<Transform>(e).unwrap().translation[0] = 5.;
    demo.app.step();
    demo.check_simulation().unwrap();
    assert_eq!(
        demo.app
            .world
            .get::<bozzard_scene::Drawable>(e)
            .unwrap()
            .color,
        [1., 0., 0.]
    );
}

#[test]
fn transform_writes_reject_singular_roots_and_composed_overflow_atomically() {
    for case in 0..3 {
        let mut s = scene(vec![action(
            K::Start,
            K::SetScale,
            Value::Vector(if case == 0 {
                [f32::MAX; 3]
            } else {
                [1e20, 1., 1.]
            }),
        )]);
        if case > 0 {
            let mut related = s.objects[0].clone();
            related.id = "related".into();
            related.blueprints.clear();
            related.transform.scale = [1e20, 1., 1.];
            if case == 1 {
                s.objects[0].parent = Some(related.id.clone());
            } else {
                related.parent = Some("owner".into());
            }
            s.objects.push(related);
        }
        let mut demo = SceneDemo::new(&s).unwrap();
        let before = demo.instance().global_transforms(&demo.app.world).unwrap();
        demo.app.step();
        assert!(demo.check_simulation().is_err(), "case {case}");
        assert_eq!(
            demo.instance().global_transforms(&demo.app.world).unwrap(),
            before
        );
        assert_eq!(transform(&demo).scale, [1.; 3]);
    }
}

#[test]
fn malformed_graphs_reject_atomically_and_bad_runtime_values_freeze_safely() {
    let graph = Blueprint::spinning();
    assert_eq!(
        Blueprint::from_json(&graph.to_json().unwrap()).unwrap(),
        graph
    );
    for case in 0..6 {
        let mut bad = graph.clone();
        match case {
            0 => bad.nodes[1].id = bad.nodes[0].id,
            1 => bad.wires[0].to.node = 999,
            2 => bad.wires[0].to.port = 1,
            3 => bad.nodes[2].inputs[0] = Value::Number(2.),
            4 => bad.nodes[0].position[0] = f32::NAN,
            _ => bad.version = 2,
        }
        assert!(bad.validate().is_err(), "case {case}");
    }
    let mut cycle = Blueprint {
        nodes: vec![
            Node::new(1, K::Add, [0., 0.]),
            Node::new(2, K::Add, [0., 0.]),
        ],
        ..Blueprint::default()
    };
    link(&mut cycle, 1, 0, 2, 0);
    let before = cycle.clone();
    assert!(
        cycle
            .connect(Wire {
                from: Socket { node: 2, port: 0 },
                to: Socket { node: 1, port: 0 }
            })
            .is_err()
    );
    assert_eq!(cycle, before);
    let s = scene(vec![action(K::Update, K::SetScale, Value::Vector([0.; 3]))]);
    let mut demo = SceneDemo::new(&s).unwrap();
    demo.app.step();
    assert!(demo.check_simulation().is_err());
    assert_eq!(transform(&demo).scale, [1.; 3]);
    let before = transform(&demo);
    demo.app.step();
    assert_eq!(transform(&demo), before);
}

#[test]
fn post_processing_graph_controls_animate_without_changing_authored_settings() {
    use bozzard_scene::{Layer, PostProcessVolume};
    let graphs = vec![
        action(K::Start, K::SetExposure, Value::Number(1.5)),
        action(K::Start, K::SetBloomIntensity, Value::Number(0.6)),
        action(K::Start, K::SetSaturation, Value::Number(0.4)),
        action(K::Start, K::SetHeatStrength, Value::Number(5.)),
        action(K::Start, K::SetGrainIntensity, Value::Number(0.1)),
        action(K::Start, K::SetVignetteIntensity, Value::Number(0.7)),
    ];
    let mut authored = scene(graphs);
    authored.post_process_volumes = vec![PostProcessVolume::default()];
    let mut demo = SceneDemo::new(&authored).unwrap();
    demo.app.step();
    demo.check_simulation().unwrap();
    let display = demo.instance().display_at([0.; 3].into(), Layer::ThreeD);
    assert_eq!(display.exposure_ev, 1.5);
    assert_eq!(display.bloom.intensity, 0.6);
    assert_eq!(display.color_grading.saturation, 0.4);
    assert_eq!(display.heat_distortion.strength, 5.);
    assert_eq!(display.grain.intensity, 0.1);
    assert_eq!(display.vignette.intensity, 0.7);
    assert!(
        !demo
            .instance()
            .display_at([0.; 3].into(), Layer::TwoD)
            .heat_distortion
            .enabled
    );
    assert_eq!(demo.instance().capture(&demo.app.world).unwrap(), authored);
    let reset = SceneDemo::new(&authored).unwrap();
    assert_eq!(
        reset.instance().display_at([0.; 3].into(), Layer::ThreeD),
        authored.display_at([0.; 3].into())
    );
}

#[test]
fn post_processing_graph_rejects_out_of_range_without_partial_write() {
    let authored = scene(vec![action(
        K::Start,
        K::SetHeatStrength,
        Value::Number(31.),
    )]);
    let mut demo = SceneDemo::new(&authored).unwrap();
    demo.app.step();
    assert!(demo.check_simulation().is_err());
    assert_eq!(
        demo.instance()
            .display_at([0.; 3].into(), bozzard_scene::Layer::ThreeD),
        authored.display
    );
}
