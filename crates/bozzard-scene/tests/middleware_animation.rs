use bozzard_ecs::World;
use bozzard_scene::{
    Scene, Transform,
    middleware::{
        animation::{
            Animator, BlendSample, Comparison, Control, Motion, RootMotion, Runtime,
            StateDefinition, Transition,
            data::{Binding, Channel, Clip, Joint, Pose, Property, Rig},
        },
        curve::{Curve, Repeat},
        registry,
        signals::{Kind, Signals},
        timeline::Marker,
    },
};
use glam::{Mat4, Vec3};
use std::sync::Arc;
fn animator() -> Animator {
    let rig = Rig {
        nodes: vec![
            Joint {
                name: "Root".into(),
                parent: None,
                rest: Pose::default(),
            },
            Joint {
                name: "Tip".into(),
                parent: Some(0),
                rest: Pose {
                    translation: [0., 1., 0.],
                    ..Default::default()
                },
            },
        ],
        bindings: vec![
            Binding {
                node: 0,
                inverse_bind: Mat4::IDENTITY.to_cols_array(),
            },
            Binding {
                node: 1,
                inverse_bind: Mat4::from_translation(Vec3::new(0., -1., 0.)).to_cols_array(),
            },
        ],
        clips: vec![
            Clip {
                name: "Idle".into(),
                duration: 1.,
                channels: vec![],
                events: vec![],
            },
            Clip {
                name: "Walk".into(),
                duration: 1.,
                channels: vec![Channel {
                    node: 0,
                    property: Property::Translation,
                    curves: vec![
                        Curve::linear(0., 2., 1.),
                        Curve::constant(0.),
                        Curve::constant(0.),
                    ],
                }],
                events: vec![Marker {
                    time: 0.5,
                    name: "Step".into(),
                }],
            },
        ],
    };
    let mut animator = Animator::from_rig(String::new(), Arc::new(rig));
    animator.parameters.insert("Speed".into(), 0.);
    animator.states = Arc::new(vec![StateDefinition {
        name: "Move".into(),
        repeat: Repeat::Loop,
        motion: Motion::Blend1d {
            parameter: "Speed".into(),
            samples: vec![
                BlendSample {
                    threshold: 0.,
                    clip: 0,
                },
                BlendSample {
                    threshold: 1.,
                    clip: 1,
                },
            ],
        },
    }]);
    animator.initial = "Move".into();
    animator
}
fn setup(animator: &Animator) -> (Scene, World, bozzard_scene::SceneInstance) {
    let mut scene=Scene::from_json(r#"{"version":1,"name":"animation","views":{},"objects":[{"id":"actor","name":"Actor","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#).unwrap();
    registry::set(&mut scene.objects[0], animator).unwrap();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    (scene, world, instance)
}
#[test]
fn blend_tree_events_root_motion_and_checkpoint_resume_without_teleports() {
    let mut animator = animator();
    animator.parameters.insert("Speed".into(), 1.);
    animator.root_motion = Some(RootMotion {
        node: 0,
        translation: [true, false, false],
        yaw: false,
    });
    let (_, mut world, mut instance) = setup(&animator);
    instance.step_animations(&mut world, 0.75).unwrap();
    assert_eq!(
        world
            .get::<Transform>(instance.entity("actor").unwrap())
            .unwrap()
            .translation[0],
        1.5
    );
    assert_eq!(
        world.resource::<Runtime>().unwrap().players["actor"].pose[0].translation[0],
        0.,
        "root motion applied both to the object and its skeleton"
    );
    assert_eq!(
        world
            .resource::<Signals>()
            .unwrap()
            .for_owner("actor", Kind::Animation)
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["Step"]
    );
    let save = instance.save_game_json(&world).unwrap();
    instance.step_animations(&mut world, 0.75).unwrap();
    let x = world
        .get::<Transform>(instance.entity("actor").unwrap())
        .unwrap()
        .translation[0];
    assert_eq!(x, 3.);
    instance.load_game_json(&mut world, &save).unwrap();
    instance.step_animations(&mut world, 0.75).unwrap();
    assert_eq!(
        world
            .get::<Transform>(instance.entity("actor").unwrap())
            .unwrap()
            .translation[0],
        x
    );
    instance
        .control_animation(
            &mut world,
            "actor",
            Control::Parameter {
                name: "Speed".into(),
                value: 0.5,
            },
        )
        .unwrap();
    instance.step_animations(&mut world, 0.25).unwrap();
    assert_eq!(
        world
            .get::<Transform>(instance.entity("actor").unwrap())
            .unwrap()
            .translation[0],
        3.25
    );
    instance
        .control_animation(&mut world, "actor", Control::Pause)
        .unwrap();
    instance.step_animations(&mut world, 1.).unwrap();
    assert_eq!(
        world
            .get::<Transform>(instance.entity("actor").unwrap())
            .unwrap()
            .translation[0],
        3.25
    );
    assert!(
        instance
            .control_animation(
                &mut world,
                "actor",
                Control::Parameter {
                    name: "Missing".into(),
                    value: 1.
                }
            )
            .is_err()
    );
}
#[test]
fn state_transition_fades_and_invalid_rigs_fail_before_spawn() {
    let mut a = animator();
    a.states = Arc::new(vec![
        StateDefinition {
            name: "Idle".into(),
            repeat: Repeat::Loop,
            motion: Motion::Clip { clip: 0 },
        },
        StateDefinition {
            name: "Walk".into(),
            repeat: Repeat::Loop,
            motion: Motion::Clip { clip: 1 },
        },
    ]);
    a.initial = "Idle".into();
    a.transitions = Arc::new(vec![Transition {
        from: "Idle".into(),
        to: "Walk".into(),
        parameter: "Speed".into(),
        comparison: Comparison::Above,
        threshold: 0.5,
        fade: 1.,
        exit_time: None,
    }]);
    let (_, mut world, instance) = setup(&a);
    instance.step_animations(&mut world, 0.).unwrap();
    instance
        .control_animation(
            &mut world,
            "actor",
            Control::Parameter {
                name: "Speed".into(),
                value: 1.,
            },
        )
        .unwrap();
    instance.step_animations(&mut world, 0.5).unwrap();
    let player = &world.resource::<Runtime>().unwrap().players["actor"];
    assert_eq!(player.state, 1);
    assert_eq!(player.pose[0].translation[0], 0.5);
    assert!(player.fade.is_some());
    instance.step_animations(&mut world, 0.5).unwrap();
    assert!(
        world.resource::<Runtime>().unwrap().players["actor"]
            .fade
            .is_none()
    );
    Arc::make_mut(&mut a.rig).nodes[0].parent = Some(1);
    let mut scene =
        Scene::from_json(r#"{"version":1,"name":"invalid","views":{},"objects":[]}"#).unwrap();
    let (good, _, _) = setup(&animator());
    scene.objects = good.objects;
    scene.objects[0]
        .extras
        .insert("animator".into(), serde_json::to_value(a).unwrap());
    let mut fresh = World::default();
    assert!(scene.spawn(&mut fresh).is_err());
    assert_eq!(fresh.query::<Transform>().count(), 0);
}
#[test]
fn quaternion_sampling_uses_shortest_arc_and_cubic_normalization() {
    use bozzard_scene::middleware::curve::{Interpolation, Key};
    let mut rig = animator().rig.as_ref().clone();
    let q = glam::Quat::from_rotation_z(120f32.to_radians()).to_array();
    let start = glam::Quat::IDENTITY.to_array();
    rig.clips[0].channels = vec![Channel {
        node: 1,
        property: Property::Rotation,
        curves: (0..4).map(|i| Curve::linear(start[i], q[i], 1.)).collect(),
    }];
    rig.validate().unwrap();
    let pose = rig.sample(0, 0.5).unwrap();
    let vector = glam::Quat::from_array(pose[1].rotation) * Vec3::X;
    assert!((vector.x - 0.5).abs() < 1e-5);
    for curve in &mut rig.clips[0].channels[0].curves {
        curve.interpolation = Interpolation::Cubic;
        curve.keys[0] = Key {
            outgoing: 1.,
            ..curve.keys[0]
        };
    }
    rig.validate().unwrap();
    rig.sample(0, 0.5).unwrap()[1].validate().unwrap();
}

#[test]
fn blueprint_transport_and_delayed_animation_events_keep_their_typed_payload() {
    use bozzard_scene::{
        Blueprint, BlueprintAttachment, BlueprintRuntime, GameplayInput,
        blueprint::{
            BlackboardValue, Node, NodeKind as K, PinType, Socket, Value, VariableScope, Wire,
        },
    };
    let mut animator = animator();
    animator.autoplay = false;
    animator.parameters.insert("Speed".into(), 1.);
    let (mut scene, _, _) = setup(&animator);
    scene.objects[0].blackboard.insert(
        "event".into(),
        BlackboardValue::Scalar(Value::Text(String::new())),
    );
    let mut play = Node::new(3, K::PlayAnimation, [0.; 2]);
    play.inputs[1] = Value::Text("Move".into());
    let event = Node::new(4, K::AnimationEvent, [0.; 2]);
    let mut delay = Node::new(5, K::Delay, [0.; 2]);
    delay.inputs[1] = Value::Number(0.25);
    let mut store = Node::new(6, K::SetVariable, [0.; 2]);
    store.scope = VariableScope::Object;
    store.value_type = PinType::Text;
    store.variable = "event".into();
    store.inputs[1] = Value::Text(String::new());
    let mut graph = Blueprint::default();
    graph.nodes.extend([play, event, delay, store]);
    for (from, port, to, input) in [(1, 0, 3, 0), (4, 0, 5, 0), (5, 0, 6, 0), (4, 1, 6, 1)] {
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
    scene.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance
        .step_blueprints(&mut world, 0.01, GameplayInput::default())
        .unwrap();
    instance.step_animations(&mut world, 0.5).unwrap();
    instance
        .step_blueprints(&mut world, 0.5, GameplayInput::default())
        .unwrap();
    assert_eq!(
        world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .object_blackboard("actor")
            .unwrap()["event"],
        BlackboardValue::Scalar(Value::Text(String::new()))
    );
    instance.step_animations(&mut world, 0.25).unwrap();
    instance
        .step_blueprints(&mut world, 0.25, GameplayInput::default())
        .unwrap();
    assert_eq!(
        world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .object_blackboard("actor")
            .unwrap()["event"],
        BlackboardValue::Scalar(Value::Text("Step".into()))
    );
}

#[test]
fn root_yaw_crosses_pi_without_turning_backwards() {
    let mut a = animator();
    a.states = Arc::new(vec![StateDefinition {
        name: "Turn".into(),
        repeat: Repeat::Loop,
        motion: Motion::Clip { clip: 0 },
    }]);
    a.initial = "Turn".into();
    let start = glam::Quat::from_rotation_y(170f32.to_radians()).to_array();
    let end = glam::Quat::from_rotation_y(190f32.to_radians()).to_array();
    Arc::make_mut(&mut a.rig).clips[0].channels = vec![Channel {
        node: 0,
        property: Property::Rotation,
        curves: (0..4)
            .map(|i| Curve::linear(start[i], end[i], 1.))
            .collect(),
    }];
    a.root_motion = Some(RootMotion {
        node: 0,
        translation: [false; 3],
        yaw: true,
    });
    let (_, mut world, instance) = setup(&a);
    instance.step_animations(&mut world, 0.75).unwrap();
    instance.step_animations(&mut world, 0.75).unwrap();
    let yaw = world
        .get::<Transform>(instance.entity("actor").unwrap())
        .unwrap()
        .rotation_degrees[1];
    assert!(
        (yaw - 30.).abs() < 0.001,
        "yaw jumped at pi or loop boundary: {yaw}"
    );
}
