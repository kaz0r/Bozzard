//! Semantic contracts for humanoid animation, independent of authored demo clips or a GPU.
use bozzard_ecs::World;
use bozzard_scene::{
    Object, Scene, Transform,
    middleware::{
        animation::{
            AnimationLayer, Animator, BlendPoint, BoneMask, Control, FootPlacement, IkConstraint,
            IkTarget, LayerBlend, Motion, MotionWarp, RootMotion, Runtime, StateDefinition,
            WarpGoal, WarpTarget,
            data::{Binding, Channel, Clip, Joint, Pose, Property, Rig},
            retarget::RetargetMap,
        },
        curve::{Curve, Repeat},
        registry,
    },
};
use glam::{Mat4, Quat, Vec3};
use std::sync::Arc;

fn rig() -> Rig {
    let nodes = [
        ("Root", None, [0., 2., 0.]),
        ("Knee", Some(0), [0., -1., 0.]),
        ("Foot", Some(1), [0., -1., 0.]),
    ]
    .into_iter()
    .map(|(name, parent, translation)| Joint {
        name: name.into(),
        parent,
        rest: Pose {
            translation,
            ..Default::default()
        },
    })
    .collect();
    Rig {
        nodes,
        bindings: (0..3)
            .map(|node| Binding {
                node,
                inverse_bind: Mat4::from_translation(Vec3::new(0., node as f32 - 2., 0.))
                    .to_cols_array(),
            })
            .collect(),
        clips: vec![Clip {
            name: "Idle".into(),
            duration: 1.,
            channels: vec![],
            events: vec![],
        }],
    }
}
fn constant_translation(name: &str, node: u32, value: [f32; 3]) -> Clip {
    Clip {
        name: name.into(),
        duration: 1.,
        events: vec![],
        channels: vec![Channel {
            node,
            property: Property::Translation,
            curves: value.map(Curve::constant).to_vec(),
        }],
    }
}
fn setup(
    animator: Animator,
    ground: bool,
) -> anyhow::Result<(Scene, World, bozzard_scene::SceneInstance)> {
    let mut scene =
        Scene::from_json(r#"{"version":1,"name":"character test","views":{},"objects":[]}"#)?;
    let mut actor = Object {
        id: "actor".into(),
        name: "Actor".into(),
        ..Default::default()
    };
    registry::set(&mut actor, &animator)?;
    scene.objects.push(actor);
    if ground {
        scene.objects.push(Object {
            id: "ground".into(),
            name: "Ground".into(),
            transform: Transform {
                translation: [0., -0.1, 0.],
                ..Default::default()
            },
            collider: Some(bozzard_scene::BoxCollider {
                size: [10., 0.2, 10.],
                ..Default::default()
            }),
            ..Default::default()
        });
    }
    let mut world = World::default();
    let instance = scene.spawn(&mut world)?;
    Ok((scene, world, instance))
}
fn pose(world: &World) -> &[Pose] {
    &world.resource::<Runtime>().unwrap().players["actor"].pose
}
fn near(a: Vec3, b: Vec3) {
    assert!(a.distance(b) < 1e-4, "{a:?} != {b:?}");
}

#[test]
fn root_motion_turning_paths_agree_across_tick_sizes_and_loop_boundaries() -> anyhow::Result<()> {
    let mut r = rig();
    let end = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array();
    r.clips[0].channels = vec![
        Channel {
            node: 0,
            property: Property::Translation,
            curves: vec![
                Curve::linear(0., 1., 1.),
                Curve::constant(2.),
                Curve::constant(0.),
            ],
        },
        Channel {
            node: 0,
            property: Property::Rotation,
            curves: (0..4)
                .map(|i| Curve::linear(Quat::IDENTITY.to_array()[i], end[i], 1.))
                .collect(),
        },
    ];
    let mut a = Animator::from_rig(String::new(), Arc::new(r));
    a.root_motion = Some(RootMotion {
        node: 0,
        translation: [true, false, true],
        yaw: true,
    });
    for (repeat, duration, expected) in
        [(Repeat::Once, 1., Vec3::X), (Repeat::Loop, 4., Vec3::ZERO)]
    {
        Arc::make_mut(&mut a.states)[0].repeat = repeat;
        let (_, mut large, instance) = setup(a.clone(), false)?;
        let (_, mut small, small_instance) = setup(a.clone(), false)?;
        instance.step_animations(&mut large, duration)?;
        for _ in 0..(duration * 4.) as usize {
            small_instance.step_animations(&mut small, 0.25)?;
        }
        let big = large
            .get::<Transform>(instance.entity("actor").unwrap())
            .unwrap();
        let little = small
            .get::<Transform>(small_instance.entity("actor").unwrap())
            .unwrap();
        near(Vec3::from_array(big.translation), expected);
        near(Vec3::from_array(little.translation), expected);
        assert!(
            big.matrix()
                .to_scale_rotation_translation()
                .1
                .dot(little.matrix().to_scale_rotation_translation().1)
                .abs()
                > 0.99999
        );
    }
    Ok(())
}

#[test]
fn root_motion_uses_imported_bone_basis_and_actor_parent_transform() -> anyhow::Result<()> {
    let mut r = rig();
    r.nodes = vec![
        Joint {
            name: "Imported basis".into(),
            parent: None,
            rest: Pose {
                rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2).to_array(),
                ..Default::default()
            },
        },
        Joint {
            name: "Root".into(),
            parent: Some(0),
            rest: Pose::default(),
        },
    ];
    r.bindings = vec![Binding {
        node: 1,
        inverse_bind: Mat4::IDENTITY.to_cols_array(),
    }];
    r.clips[0].channels = vec![Channel {
        node: 1,
        property: Property::Translation,
        curves: vec![
            Curve::constant(0.),
            Curve::linear(0., 1., 1.),
            Curve::constant(0.),
        ],
    }];
    let mut a = Animator::from_rig(String::new(), Arc::new(r));
    Arc::make_mut(&mut a.states)[0].repeat = Repeat::Once;
    a.root_motion = Some(RootMotion {
        node: 1,
        translation: [false, false, true],
        yaw: false,
    });
    let (mut scene, _, _) = setup(a, false)?;
    scene.objects[0].parent = Some("parent".into());
    scene.objects.push(Object {
        id: "parent".into(),
        name: "Parent".into(),
        transform: Transform {
            translation: [3., 0., 0.],
            rotation_degrees: [0., 90., 0.],
            ..Default::default()
        },
        ..Default::default()
    });
    let mut world = World::default();
    let instance = scene.spawn(&mut world)?;
    instance.step_animations(&mut world, 1.)?;
    near(
        Vec3::from_array(
            world
                .get::<Transform>(instance.entity("actor").unwrap())
                .unwrap()
                .translation,
        ),
        Vec3::Z,
    );
    near(
        instance
            .animation_bone_transform(&world, "actor", 1)?
            .w_axis
            .truncate(),
        Vec3::new(4., 0., 0.),
    );
    near(Vec3::from_array(pose(&world)[1].translation), Vec3::ZERO);
    Ok(())
}

#[test]
fn blueprint_layer_restart_and_warp_controls_use_typed_inputs() -> anyhow::Result<()> {
    use bozzard_scene::{
        Blueprint, BlueprintAttachment, GameplayInput,
        blueprint::{Node, NodeKind as K, Socket, Value, Wire},
    };
    let mut a = warped_animator();
    a.layers = Arc::new(vec![AnimationLayer {
        name: "Gesture".into(),
        ..Default::default()
    }]);
    let (mut scene, _, _) = setup(a, false)?;
    let mut warp = Node::new(3, K::SetAnimationWarpTarget, [0.; 2]);
    warp.inputs[1] = Value::Text("Reach".into());
    warp.inputs[2] = Value::Vector([9., 0., 0.]);
    warp.inputs[3] = Value::Number(20.);
    let mut restart = Node::new(4, K::RestartAnimationLayer, [0.; 2]);
    restart.inputs[1] = Value::Text("Gesture".into());
    let mut delay = Node::new(5, K::Delay, [0.; 2]);
    delay.inputs[1] = Value::Number(0.1);
    let mut clear = Node::new(6, K::ClearAnimationWarpTarget, [0.; 2]);
    clear.inputs[1] = Value::Text("Reach".into());
    let mut graph = Blueprint::default();
    graph.nodes.extend([warp, restart, delay, clear]);
    for (from, to) in [(1, 3), (3, 4), (4, 5), (5, 6)] {
        graph.connect(Wire {
            from: Socket {
                node: from,
                port: 0,
            },
            to: Socket { node: to, port: 0 },
        })?;
    }
    scene.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world)?;
    instance.step_animations(&mut world, 0.4)?;
    instance.step_blueprints(&mut world, 0.01, GameplayInput::default())?;
    let player = &world.resource::<Runtime>().unwrap().players["actor"];
    assert_eq!(player.warp_targets["Reach"].position, [9., 0., 0.]);
    assert_eq!(player.warp_targets["Reach"].yaw_degrees, 20.);
    assert_eq!(player.layers[0].clock.elapsed, 0.);
    instance.step_blueprints(&mut world, 0.2, GameplayInput::default())?;
    assert!(
        world.resource::<Runtime>().unwrap().players["actor"]
            .warp_targets
            .is_empty()
    );
    Ok(())
}

#[test]
fn two_dimensional_blend_interpolates_inside_and_clamps_outside_the_hull() -> anyhow::Result<()> {
    let mut rig = rig();
    rig.clips = vec![
        constant_translation("Origin", 0, [0., 2., 0.]),
        constant_translation("Right", 0, [2., 2., 0.]),
        constant_translation("Forward", 0, [4., 2., 0.]),
    ];
    let mut animator = Animator::from_rig(String::new(), Arc::new(rig));
    animator
        .parameters
        .extend([("X".into(), 0.25), ("Y".into(), 0.25)]);
    animator.initial = "Move".into();
    animator.states = Arc::new(vec![StateDefinition {
        name: "Move".into(),
        repeat: Repeat::Loop,
        motion: Motion::Blend2d {
            parameters: ["X".into(), "Y".into()],
            samples: vec![
                BlendPoint {
                    position: [0., 0.],
                    clip: 0,
                },
                BlendPoint {
                    position: [1., 0.],
                    clip: 1,
                },
                BlendPoint {
                    position: [0., 1.],
                    clip: 2,
                },
            ],
        },
    }]);
    let (_, mut world, instance) = setup(animator, false)?;
    instance.step_animations(&mut world, 0.)?;
    near(
        Vec3::from_array(pose(&world)[0].translation),
        Vec3::new(1.5, 2., 0.),
    );
    for name in ["X", "Y"] {
        instance.control_animation(
            &mut world,
            "actor",
            Control::Parameter {
                name: name.into(),
                value: 3.,
            },
        )?;
    }
    instance.step_animations(&mut world, 0.)?;
    near(
        Vec3::from_array(pose(&world)[0].translation),
        Vec3::new(3., 2., 0.),
    );
    Ok(())
}

#[test]
fn directional_gaits_keep_phase_when_the_blend_parameters_change() -> anyhow::Result<()> {
    let mut rig = rig();
    for (name, duration) in [("Forward", 1.), ("Left", 2.), ("Right", 0.5)] {
        let mut clip = constant_translation(name, 0, [0., 2., 0.]);
        clip.duration = duration;
        clip.channels[0].curves[0] = Curve::linear(0., 1., duration);
        rig.clips.push(clip);
    }
    let mut animator = Animator::from_rig(String::new(), Arc::new(rig));
    animator
        .parameters
        .extend([("X".into(), 0.), ("Y".into(), 1.)]);
    animator.initial = "Move".into();
    animator.states = Arc::new(vec![StateDefinition {
        name: "Move".into(),
        repeat: Repeat::Loop,
        motion: Motion::Blend2d {
            parameters: ["X".into(), "Y".into()],
            samples: vec![
                BlendPoint {
                    position: [0., 1.],
                    clip: 1,
                },
                BlendPoint {
                    position: [-1., 0.],
                    clip: 2,
                },
                BlendPoint {
                    position: [1., 0.],
                    clip: 3,
                },
            ],
        },
    }]);
    let (_, mut world, instance) = setup(animator, false)?;
    instance.step_animations(&mut world, 0.25)?;
    let before = pose(&world)[0].translation[0];
    instance.control_animation(
        &mut world,
        "actor",
        Control::Parameter {
            name: "X".into(),
            value: -1.,
        },
    )?;
    instance.control_animation(
        &mut world,
        "actor",
        Control::Parameter {
            name: "Y".into(),
            value: 0.,
        },
    )?;
    instance.step_animations(&mut world, 0.)?;
    assert!(
        (pose(&world)[0].translation[0] - before).abs() < 1e-6,
        "gait change restarted its phase"
    );
    Ok(())
}

#[test]
fn masked_override_and_additive_layers_preserve_the_unselected_body() -> anyhow::Result<()> {
    for blend in [LayerBlend::Override, LayerBlend::Additive] {
        let mut rig = rig();
        let mut overlay = constant_translation("Upper", 0, [10., 2., 0.]);
        overlay.channels.push(Channel {
            node: 1,
            property: Property::Rotation,
            curves: Quat::from_rotation_z(0.5)
                .to_array()
                .map(Curve::constant)
                .to_vec(),
        });
        rig.clips.push(overlay);
        let mut animator = Animator::from_rig(String::new(), Arc::new(rig));
        animator.layers = Arc::new(vec![AnimationLayer {
            name: "Upper".into(),
            motion: Motion::Clip { clip: 1 },
            blend,
            mask: BoneMask {
                root: Some(1),
                ..Default::default()
            },
            ..Default::default()
        }]);
        let (_, mut world, instance) = setup(animator, false)?;
        instance.step_animations(&mut world, 0.1)?;
        near(
            Vec3::from_array(pose(&world)[0].translation),
            Vec3::new(0., 2., 0.),
        );
        assert!(
            Quat::from_array(pose(&world)[1].rotation)
                .abs_diff_eq(Quat::from_rotation_z(0.5), 1e-5)
        );
    }
    Ok(())
}

#[test]
fn two_bone_ik_reaches_a_target_and_clamps_unreachable_goals_without_nan() -> anyhow::Result<()> {
    let mut animator = Animator::from_rig(String::new(), Arc::new(rig()));
    animator.ik = Arc::new(vec![IkConstraint {
        name: "Leg".into(),
        root: 0,
        middle: 1,
        tip: 2,
        pole: [0., 0., 1.],
        target: IkTarget::Point {
            position: [0.5, 0.3, 0.],
        },
        smoothing: 0.,
        ..Default::default()
    }]);
    let (_, mut world, instance) = setup(animator.clone(), false)?;
    instance.step_animations(&mut world, 0.1)?;
    near(
        instance
            .animation_bone_transform(&world, "actor", 2)?
            .w_axis
            .truncate(),
        Vec3::new(0.5, 0.3, 0.),
    );
    Arc::make_mut(&mut animator.ik)[0].target = IkTarget::Point {
        position: [100., 0., 0.],
    };
    let (_, mut world, instance) = setup(animator, false)?;
    instance.step_animations(&mut world, 0.1)?;
    let end = instance
        .animation_bone_transform(&world, "actor", 2)?
        .w_axis
        .truncate();
    assert!(end.is_finite() && end.distance(Vec3::new(0., 2., 0.)) <= 2.0001);
    Ok(())
}

#[test]
fn ground_contacts_follow_moving_support_and_survive_checkpoint_restore() -> anyhow::Result<()> {
    let mut animator = Animator::from_rig(String::new(), Arc::new(rig()));
    animator.ik = Arc::new(vec![IkConstraint {
        root: 0,
        middle: 1,
        tip: 2,
        pole: [0., 0., 1.],
        target: IkTarget::Ground {
            sole_height: 0.05,
            ray_up: 0.6,
            ray_down: 0.6,
            layers: u32::MAX,
            release_height: 0.2,
            plant: true,
            align_normal: true,
        },
        smoothing: 0.,
        ..Default::default()
    }]);
    let (_, mut world, mut instance) = setup(animator, true)?;
    instance.step_animations(&mut world, 1. / 60.)?;
    assert!(world.resource::<Runtime>().unwrap().players["actor"].ik[0].planted);
    let save = instance.save_game_json(&world)?;
    let ground = instance.entity("ground").unwrap();
    world.insert(
        ground,
        Transform {
            translation: [0., 0.1, 0.],
            ..Default::default()
        },
    )?;
    instance.step_animations(&mut world, 1. / 60.)?;
    near(
        instance
            .animation_bone_transform(&world, "actor", 2)?
            .w_axis
            .truncate(),
        Vec3::new(0., 0.25, 0.),
    );
    instance.load_game_json(&mut world, &save)?;
    instance.step_animations(&mut world, 1. / 60.)?;
    near(
        instance
            .animation_bone_transform(&world, "actor", 2)?
            .w_axis
            .truncate(),
        Vec3::new(0., 0.05, 0.),
    );
    Ok(())
}

#[test]
fn retargeting_preserves_target_limb_lengths_and_scales_root_translation() -> anyhow::Result<()> {
    let mut source = rig();
    source
        .clips
        .push(constant_translation("Travel", 0, [1., 2., 0.]));
    let mut target = rig();
    target.nodes[1].rest.translation[1] = -1.5;
    target.nodes[2].rest.translation[1] = -1.5;
    let mut mapping = RetargetMap::by_name(&source, &target);
    mapping.translation_scale = 2.;
    let baked = mapping.bake_clip(&source, &target, 1, "Reused travel".into(), 30.)?;
    target.clips.push(baked);
    target.validate()?;
    let pose = target.sample(1, 0.5)?;
    near(Vec3::from_array(pose[0].translation), Vec3::new(2., 2., 0.));
    assert_eq!(pose[1].translation[1], -1.5);
    assert_eq!(pose[2].translation[1], -1.5);
    Ok(())
}

fn warped_animator() -> Animator {
    let mut rig = rig();
    rig.clips[0].channels = vec![Channel {
        node: 0,
        property: Property::Translation,
        curves: vec![
            Curve::linear(0., 2., 1.),
            Curve::constant(2.),
            Curve::constant(0.),
        ],
    }];
    let mut animator = Animator::from_rig(String::new(), Arc::new(rig));
    Arc::make_mut(&mut animator.states)[0].repeat = Repeat::Once;
    animator.root_motion = Some(RootMotion {
        node: 0,
        translation: [true, false, false],
        yaw: false,
    });
    animator.warps = Arc::new(vec![MotionWarp {
        name: "Reach".into(),
        state: "Idle".into(),
        start: 0.25,
        end: 0.75,
        translation: [true, false, false],
        yaw: false,
        target: WarpTarget::Point {
            position: [5., 0., 0.],
            yaw_degrees: 0.,
        },
    }]);
    animator
}
#[test]
fn motion_warp_reaches_the_goal_even_when_a_tick_crosses_the_window_boundary() -> anyhow::Result<()>
{
    let (_, mut world, instance) = setup(warped_animator(), false)?;
    instance.step_animations(&mut world, 0.5)?;
    instance.step_animations(&mut world, 0.25)?;
    near(
        world
            .get::<Transform>(instance.entity("actor").unwrap())
            .unwrap()
            .translation
            .into(),
        Vec3::new(5., 0., 0.),
    );
    instance.step_animations(&mut world, 0.25)?;
    near(
        world
            .get::<Transform>(instance.entity("actor").unwrap())
            .unwrap()
            .translation
            .into(),
        Vec3::new(5.5, 0., 0.),
    );
    Ok(())
}
#[test]
fn runtime_warp_targets_and_layer_clocks_round_trip_without_restarting() -> anyhow::Result<()> {
    let mut animator = warped_animator();
    animator.layers = Arc::new(vec![AnimationLayer {
        name: "Gesture".into(),
        ..Default::default()
    }]);
    let (_, mut world, mut instance) = setup(animator, false)?;
    instance.control_animation(
        &mut world,
        "actor",
        Control::WarpTarget {
            name: "Reach".into(),
            goal: WarpGoal {
                position: [7., 0., 0.],
                yaw_degrees: 0.,
            },
        },
    )?;
    instance.step_animations(&mut world, 0.4)?;
    let save = instance.save_game_json(&world)?;
    instance.step_animations(&mut world, 0.35)?;
    let expected = *world
        .get::<Transform>(instance.entity("actor").unwrap())
        .unwrap();
    let layer_time = world.resource::<Runtime>().unwrap().players["actor"].layers[0]
        .clock
        .elapsed;
    instance.load_game_json(&mut world, &save)?;
    instance.step_animations(&mut world, 0.35)?;
    near(
        world
            .get::<Transform>(instance.entity("actor").unwrap())
            .unwrap()
            .translation
            .into(),
        expected.translation.into(),
    );
    assert!(
        (world.resource::<Runtime>().unwrap().players["actor"].layers[0]
            .clock
            .elapsed
            - layer_time)
            .abs()
            < 1e-6
    );
    Ok(())
}

#[test]
fn nonconsecutive_ik_chains_are_rejected_before_spawn() {
    let mut animator = Animator::from_rig(String::new(), Arc::new(rig()));
    animator.ik = Arc::new(vec![IkConstraint {
        root: 0,
        middle: 2,
        tip: 1,
        ..Default::default()
    }]);
    assert!(setup(animator, false).is_err());
}

#[test]
fn cocircular_blend_points_interpolate_a_plane_without_gaps() -> anyhow::Result<()> {
    let mut source = rig();
    source.clips = [0., 1., 3., 2.]
        .into_iter()
        .enumerate()
        .map(|(i, x)| constant_translation(&format!("C{i}"), 0, [x, 2., 0.]))
        .collect();
    let mut animator = Animator::from_rig(String::new(), Arc::new(source));
    animator
        .parameters
        .extend([("X".into(), 0.), ("Y".into(), 0.)]);
    Arc::make_mut(&mut animator.states)[0].motion = Motion::Blend2d {
        parameters: ["X".into(), "Y".into()],
        samples: [[0., 0.], [1., 0.], [1., 1.], [0., 1.]]
            .into_iter()
            .enumerate()
            .map(|(clip, position)| BlendPoint { clip, position })
            .collect(),
    };
    let (_, mut world, instance) = setup(animator, false)?;
    for x in [0., 0.1, 0.5, 0.9, 1.] {
        for y in [0., 0.1, 0.5, 0.9, 1.] {
            for (name, value) in [("X", x), ("Y", y)] {
                instance.control_animation(
                    &mut world,
                    "actor",
                    Control::Parameter {
                        name: name.into(),
                        value,
                    },
                )?;
            }
            instance.step_animations(&mut world, 0.)?;
            assert!((pose(&world)[0].translation[0] - x - 2. * y).abs() < 1e-5);
        }
    }
    Ok(())
}

#[test]
fn collinear_and_duplicate_directional_samples_fail_validation() {
    for points in [
        [[0., 0.], [1., 0.], [2., 0.]],
        [[0., 0.], [0., 0.], [1., 1.]],
    ] {
        let mut a = Animator::from_rig(String::new(), Arc::new(rig()));
        a.parameters.extend([("X".into(), 0.), ("Y".into(), 0.)]);
        Arc::make_mut(&mut a.states)[0].motion = Motion::Blend2d {
            parameters: ["X".into(), "Y".into()],
            samples: points
                .map(|position| BlendPoint { position, clip: 0 })
                .to_vec(),
        };
        assert!(setup(a, false).is_err());
    }
}

fn layered_animator() -> Animator {
    let mut source = rig();
    let mut clip = constant_translation("Overlay", 0, [0., 2., 0.]);
    clip.channels.push(Channel {
        node: 1,
        property: Property::Rotation,
        curves: Quat::from_rotation_z(0.6)
            .to_array()
            .map(Curve::constant)
            .to_vec(),
    });
    source.clips.push(clip);
    let mut a = Animator::from_rig(String::new(), Arc::new(source));
    a.parameters.insert("Gesture".into(), 1.);
    a.layers = Arc::new(vec![AnimationLayer {
        name: "Gesture".into(),
        motion: Motion::Clip { clip: 1 },
        blend: LayerBlend::Additive,
        mask: BoneMask {
            root: Some(1),
            ..Default::default()
        },
        weight_parameter: Some("Gesture".into()),
        ..Default::default()
    }]);
    a
}

#[test]
fn additive_layers_are_not_applied_twice_during_a_state_fade() -> anyhow::Result<()> {
    let (_, mut world, instance) = setup(layered_animator(), false)?;
    instance.step_animations(&mut world, 0.1)?;
    instance.control_animation(
        &mut world,
        "actor",
        Control::Play {
            state: "Idle".into(),
            fade: 0.5,
        },
    )?;
    instance.step_animations(&mut world, 0.1)?;
    assert!(
        Quat::from_array(pose(&world)[1].rotation).abs_diff_eq(Quat::from_rotation_z(0.6), 1e-5)
    );
    Ok(())
}

#[test]
fn layer_fades_pause_and_resume_from_a_checkpoint() -> anyhow::Result<()> {
    let mut a = layered_animator();
    Arc::make_mut(&mut a.layers)[0].fade = 0.5;
    let (_, mut world, mut instance) = setup(a, false)?;
    instance.step_animations(&mut world, 0.25)?;
    assert_eq!(
        world.resource::<Runtime>().unwrap().players["actor"].layers[0].weight,
        0.5
    );
    let saved = instance.save_game_json(&world)?;
    instance.control_animation(&mut world, "actor", Control::Pause)?;
    instance.step_animations(&mut world, 0.25)?;
    assert_eq!(
        world.resource::<Runtime>().unwrap().players["actor"].layers[0].weight,
        0.5
    );
    instance.load_game_json(&mut world, &saved)?;
    instance.step_animations(&mut world, 0.25)?;
    assert_eq!(
        world.resource::<Runtime>().unwrap().players["actor"].layers[0].weight,
        1.
    );
    instance.control_animation(
        &mut world,
        "actor",
        Control::Parameter {
            name: "Gesture".into(),
            value: 0.,
        },
    )?;
    instance.step_animations(&mut world, 0.25)?;
    assert_eq!(
        world.resource::<Runtime>().unwrap().players["actor"].layers[0].weight,
        0.5
    );
    Ok(())
}

#[test]
fn synchronized_layer_events_follow_the_base_cycle_and_do_not_fire_when_paused()
-> anyhow::Result<()> {
    use bozzard_scene::middleware::{
        signals::{Kind, Signals},
        timeline::Marker,
    };
    let mut a = layered_animator();
    Arc::make_mut(&mut a.layers)[0].synchronized = true;
    Arc::make_mut(&mut a.rig).clips[1].events.push(Marker {
        name: "Beat".into(),
        time: 0.5,
    });
    let (_, mut world, instance) = setup(a, false)?;
    instance.step_animations(&mut world, 0.6)?;
    assert_eq!(
        world
            .resource::<Signals>()
            .unwrap()
            .for_owner("actor", Kind::Animation)
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["Gesture/Beat"]
    );
    instance.control_animation(&mut world, "actor", Control::Pause)?;
    instance.step_animations(&mut world, 0.8)?;
    assert_eq!(
        world
            .resource::<Signals>()
            .unwrap()
            .for_owner("actor", Kind::Animation)
            .count(),
        0
    );
    Ok(())
}

#[test]
fn pelvis_adjustment_keeps_feet_on_lower_ground_and_survives_save_restore() -> anyhow::Result<()> {
    let mut a = Animator::from_rig(String::new(), Arc::new(rig()));
    a.ik = Arc::new(vec![IkConstraint {
        root: 0,
        middle: 1,
        tip: 2,
        smoothing: 0.,
        target: IkTarget::Ground {
            sole_height: 0.05,
            ray_up: 0.5,
            ray_down: 0.8,
            layers: u32::MAX,
            release_height: 0.2,
            plant: true,
            align_normal: true,
        },
        ..Default::default()
    }]);
    a.foot_placement = Some(FootPlacement {
        pelvis: 0,
        smoothing: 0.,
        ..Default::default()
    });
    let (_, mut world, mut instance) = setup(a, true)?;
    world.insert(
        instance.entity("ground").unwrap(),
        Transform {
            translation: [0., -0.3, 0.],
            ..Default::default()
        },
    )?;
    instance.step_animations(&mut world, 1. / 60.)?;
    near(
        instance
            .animation_bone_transform(&world, "actor", 2)?
            .w_axis
            .truncate(),
        Vec3::new(0., -0.15, 0.),
    );
    let saved = instance.save_game_json(&world)?;
    instance.load_game_json(&mut world, &saved)?;
    assert!(
        (world.resource::<Runtime>().unwrap().players["actor"].pelvis_offset + 0.15).abs() < 1e-5
    );
    Ok(())
}

#[test]
fn consecutive_warp_windows_work_in_a_single_large_tick() -> anyhow::Result<()> {
    let mut a = warped_animator();
    let mut first = a.warps[0].clone();
    first.start = 0.;
    first.end = 0.4;
    let mut second = first.clone();
    second.name = "Second".into();
    second.start = 0.5;
    second.end = 0.9;
    second.target = WarpTarget::Point {
        position: [8., 0., 0.],
        yaw_degrees: 0.,
    };
    a.warps = Arc::new(vec![second, first]);
    let (_, mut world, instance) = setup(a, false)?;
    instance.step_animations(&mut world, 1.)?;
    near(
        world
            .get::<Transform>(instance.entity("actor").unwrap())
            .unwrap()
            .translation
            .into(),
        Vec3::new(8.2, 0., 0.),
    );
    Ok(())
}

#[test]
fn motion_warp_yaw_changes_the_direction_of_motion_after_its_window() -> anyhow::Result<()> {
    let mut a = warped_animator();
    let window = &mut Arc::make_mut(&mut a.warps)[0];
    window.start = 0.;
    window.end = 0.5;
    window.yaw = true;
    window.target = WarpTarget::Point {
        position: [5., 0., 0.],
        yaw_degrees: 90.,
    };
    let (_, mut world, instance) = setup(a, false)?;
    instance.step_animations(&mut world, 1.)?;
    let actor = world
        .get::<Transform>(instance.entity("actor").unwrap())
        .unwrap();
    near(actor.translation.into(), Vec3::new(5., 0., -1.));
    assert!((actor.rotation_degrees[1] - 90.).abs() < 1e-4);
    Ok(())
}

#[test]
fn rhai_controls_and_queries_share_the_animation_runtime() -> anyhow::Result<()> {
    let mut a = warped_animator();
    a.parameters.insert("Speed".into(), 0.);
    a.layers = Arc::new(vec![AnimationLayer {
        name: "Gesture".into(),
        ..Default::default()
    }]);
    let (mut scene, _, _) = setup(a, false)?;
    scene.assets.insert(
        "script".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Script,
            path: "test.rhai".into(),
        },
    );
    scene.objects[0].script_manager = Some(bozzard_scene::ScriptManager {
        scripts: vec![bozzard_scene::ScriptAttachment {
            enabled: true,
            script: "script".into(),
        }],
    });
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world)?;
    instance.register_script("script".into(), r#"
        fn on_start(me) {
            play_animation(me, "Idle", 0.0);
            set_animation_parameter(me, "Speed", 0.7);
            set_animation_warp_target(me, "Reach", [9.0,0.0,0.0], 20.0);
            restart_animation_layer(me, "Gesture");
        }
        fn on_update(me, dt) {
            if animation_state(me) == "Idle" && animation_playing(me) && animation_progress(me) >= 0.2 {
                pause_animation(me);
                seek_animation(me, 0.6);
                clear_animation_warp_target(me, "Reach");
            }
        }
    "#.into())?;
    instance.step_scripts(&mut world, 0.1, Default::default())?;
    instance.step_animations(&mut world, 0.25)?;
    instance.step_scripts(&mut world, 0.1, Default::default())?;
    let player = &world.resource::<Runtime>().unwrap().players["actor"];
    assert!(!player.clock.playing);
    assert!((player.clock.elapsed - 0.6).abs() < 1e-6);
    assert_eq!(player.parameters["Speed"], 0.7);
    assert!(player.warp_targets.is_empty());
    Ok(())
}
