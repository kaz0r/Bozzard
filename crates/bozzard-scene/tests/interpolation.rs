use bozzard_ecs::World;
use bozzard_scene::{Layer, Prefab, Scene, SceneInstance, Transform};
use glam::{Mat4, Quat, Vec3};
use std::collections::BTreeMap;

fn scene() -> Scene {
    Scene::from_json(r#"{
        "version":1,"name":"Interpolation regression","views":{"3d":"camera"},
        "objects":[
            {"id":"root","name":"Root","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[2,1,0.5]}},
            {"id":"cube","name":"Cube","parent":"root","transform":{"translation":[1,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,0.5,0.2],"uv_scale":[1,1]}},
            {"id":"lamp","name":"Lamp","parent":"cube","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"light":{"kind":"point"}},
            {"id":"camera","name":"Camera","parent":"root","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "camera":{"projection":"perspective","vertical_fov_degrees":60,"near":0.1,"far":100}}
        ]
    }"#).unwrap()
}

fn setup() -> (SceneInstance, World) {
    let mut world = World::new();
    let instance = scene().spawn(&mut world).unwrap();
    instance.set_render_interpolation(&mut world, true).unwrap();
    (instance, world)
}

fn assert_matrix(actual: Mat4, expected: Mat4) {
    for (actual, expected) in actual
        .to_cols_array()
        .into_iter()
        .zip(expected.to_cols_array())
    {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }
}

fn position(instance: &SceneInstance, world: &World, id: &str, alpha: f32) -> Vec3 {
    instance.interpolated_transforms(world, alpha).unwrap()[id].transform_point3(Vec3::ZERO)
}

#[test]
fn local_poses_preserve_hierarchies_shear_and_camera_light_alignment() {
    let (instance, mut world) = setup();
    world.advance_change_tick();
    let root = instance.entity("root").unwrap();
    world.get_mut::<Transform>(root).unwrap().translation = [10., 0., 0.];
    world.get_mut::<Transform>(root).unwrap().rotation_degrees = [0., 90., 0.];
    world
        .get_mut::<Transform>(instance.entity("cube").unwrap())
        .unwrap()
        .rotation_degrees = [0., 0., 90.];
    instance.capture_render_transforms(&mut world).unwrap();
    let before = instance.capture(&world).unwrap();
    let change_tick = world.changed_tick::<Transform>(root);
    let expected_root = Mat4::from_scale_rotation_translation(
        Vec3::new(2., 1., 0.5),
        Quat::from_rotation_y(45_f32.to_radians()),
        Vec3::new(5., 0., 0.),
    );
    let expected_cube = expected_root
        * Mat4::from_rotation_translation(Quat::from_rotation_z(45_f32.to_radians()), Vec3::X);
    let view = instance
        .view_interpolated_from_camera(&world, Layer::ThreeD, 1., None, 0.5)
        .unwrap();
    assert_matrix(view.objects[0].0, expected_cube);
    assert!(
        Vec3::from(view.lights[0].position).distance(expected_cube.transform_point3(Vec3::ZERO))
            < 0.0001
    );
    let projection = world
        .get::<bozzard_scene::Camera>(instance.entity("camera").unwrap())
        .unwrap()
        .projection(1.)
        .unwrap();
    assert_matrix(
        view.view_projection,
        projection * (expected_root * Mat4::from_translation(Vec3::Z * 10.)).inverse(),
    );
    let exact = instance.global_transforms(&world).unwrap();
    assert_matrix(
        instance.interpolated_transforms(&world, 1.).unwrap()["cube"],
        exact["cube"],
    );
    // Alternating extraction and CPU queries cannot contaminate either transform cache.
    assert_matrix(
        instance.interpolated_transforms(&world, 0.5).unwrap()["cube"],
        expected_cube,
    );
    assert_eq!(instance.global_transforms(&world).unwrap(), exact);
    assert_eq!(instance.capture(&world).unwrap(), before);
    assert_eq!(world.changed_tick::<Transform>(root), change_tick);
}

#[test]
fn rotation_wraps_follow_the_shortest_arc_and_reflections_snap() {
    let (instance, mut world) = setup();
    let root = instance.entity("root").unwrap();
    world.get_mut::<Transform>(root).unwrap().rotation_degrees[1] = 359.;
    instance
        .set_render_interpolation(&mut world, false)
        .unwrap();
    instance.set_render_interpolation(&mut world, true).unwrap();
    world.advance_change_tick();
    world.get_mut::<Transform>(root).unwrap().rotation_degrees[1] = 1.;
    instance.capture_render_transforms(&mut world).unwrap();
    assert_matrix(
        instance.interpolated_transforms(&world, 0.5).unwrap()["root"],
        Mat4::from_scale(Vec3::new(2., 1., 0.5)),
    );
    world.advance_change_tick();
    world.get_mut::<Transform>(root).unwrap().scale[0] = -2.;
    instance.capture_render_transforms(&mut world).unwrap();
    for alpha in [0., 0.25, 0.5, 0.75, 1.] {
        assert_eq!(
            instance.interpolated_transforms(&world, alpha).unwrap()["root"],
            instance.global_transforms(&world).unwrap()["root"]
        );
    }
    for invalid in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        assert!(instance.interpolated_transforms(&world, invalid).is_err());
    }
}

#[test]
fn interpolation_keeps_large_finite_endpoints_and_positive_scale_invertible() {
    let mut source = scene();
    source.objects[0].transform.translation[0] = -f32::MAX * 0.75;
    source.objects[0].transform.scale = [1.; 3];
    let mut world = World::new();
    let instance = source.spawn(&mut world).unwrap();
    instance.set_render_interpolation(&mut world, true).unwrap();
    world.advance_change_tick();
    let root = instance.entity("root").unwrap();
    world.get_mut::<Transform>(root).unwrap().translation[0] = f32::MAX * 0.75;
    instance.capture_render_transforms(&mut world).unwrap();
    let midpoint = instance.interpolated_transforms(&world, 0.5).unwrap()["root"];
    assert!(midpoint.is_finite() && midpoint.inverse().is_finite());
    assert_eq!(midpoint.transform_point3(Vec3::ZERO), Vec3::ZERO);
    world.advance_change_tick();
    {
        let mut transform = world.get_mut::<Transform>(root).unwrap();
        transform.translation[0] = 0.;
        transform.scale = [3., 0.5, 2.];
    }
    instance.capture_render_transforms(&mut world).unwrap();
    assert_matrix(
        instance.interpolated_transforms(&world, 0.5).unwrap()["root"],
        Mat4::from_scale_rotation_translation(
            Vec3::new(2., 0.75, 1.5),
            Quat::IDENTITY,
            Vec3::X * (f32::MAX * 0.375),
        ),
    );
}

#[test]
fn two_dimensional_sprites_and_world_text_use_the_same_local_pose_as_meshes() {
    use bozzard_scene::{
        TextRendering,
        middleware::{registry, sprite::Sprite},
    };
    let mut source = scene();
    source.views.insert(Layer::TwoD, "camera".into());
    source.assets.insert(
        "atlas".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Image,
            path: "atlas.png".into(),
        },
    );
    source.objects[1].text_rendering = Some(TextRendering {
        layer: Layer::TwoD,
        ..Default::default()
    });
    registry::set(
        &mut source.objects[1],
        &Sprite {
            image: "atlas".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut world = World::new();
    let instance = source.spawn(&mut world).unwrap();
    instance.set_render_interpolation(&mut world, true).unwrap();
    world.advance_change_tick();
    world
        .get_mut::<Transform>(instance.entity("root").unwrap())
        .unwrap()
        .translation[0] = 10.;
    instance.capture_render_transforms(&mut world).unwrap();
    let view = instance
        .view_interpolated_from_camera(&world, Layer::TwoD, 1., None, 0.5)
        .unwrap();
    let expected =
        Mat4::from_scale_rotation_translation(Vec3::new(2., 1., 0.5), Quat::IDENTITY, Vec3::X * 7.);
    assert_matrix(view.texts[0].0, expected);
    assert_matrix(view.sprites[0].model, expected);
}

#[test]
fn additive_scene_changes_keep_existing_motion_and_snap_new_members() {
    let mut source = scene();
    let mut addition = scene();
    addition.views.clear();
    addition.objects[0].transform.translation[0] = 30.;
    source
        .runtime_scenes
        .insert("addition".into(), std::sync::Arc::new(addition));
    let mut world = World::new();
    let mut instance = source.spawn(&mut world).unwrap();
    instance.set_render_interpolation(&mut world, true).unwrap();
    world.advance_change_tick();
    world
        .get_mut::<Transform>(instance.entity("root").unwrap())
        .unwrap()
        .translation[0] = 10.;
    instance
        .load_runtime_scene(&mut world, "addition", true)
        .unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "root", 0.5).x, 5.);
    assert_eq!(position(&instance, &world, "scene-1-root", 0.).x, 30.);
    world.advance_change_tick();
    instance
        .unload_runtime_scene(&mut world, "scene-1")
        .unwrap();
    world
        .get_mut::<Transform>(instance.entity("root").unwrap())
        .unwrap()
        .translation[0] = 20.;
    instance.capture_render_transforms(&mut world).unwrap();
    let matrices = instance.interpolated_transforms(&world, 0.5).unwrap();
    assert_eq!(matrices["root"].transform_point3(Vec3::ZERO).x, 15.);
    assert_eq!(matrices.len(), source.objects.len());
}

#[test]
fn repeated_frames_stationary_ticks_and_outside_tick_writes_never_replay_motion() {
    let (instance, mut world) = setup();
    let root = instance.entity("root").unwrap();
    world.advance_change_tick();
    world.get_mut::<Transform>(root).unwrap().translation[0] = 10.;
    instance.capture_render_transforms(&mut world).unwrap();
    for alpha in [0., 0.25, 0.5, 0.75, 1.] {
        assert!((position(&instance, &world, "root", alpha).x - 10. * alpha).abs() < 0.0001);
        assert!((position(&instance, &world, "root", alpha).x - 10. * alpha).abs() < 0.0001);
    }
    // A native/debug-camera edit between ticks has no matching snapshot and snaps immediately.
    world.get_mut::<Transform>(root).unwrap().translation[0] = 30.;
    assert_eq!(position(&instance, &world, "root", 0.5).x, 30.);
    world.advance_change_tick();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "root", 0.5).x, 30.);
    world.advance_change_tick();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "root", 0.).x, 30.);
}

#[test]
fn explicit_teleports_snap_descendants_and_resume_smoothing_on_the_next_tick() {
    let (instance, mut world) = setup();
    let root = instance.entity("root").unwrap();
    let cube = instance.entity("cube").unwrap();
    world.advance_change_tick();
    world.get_mut::<Transform>(root).unwrap().translation[0] = 100.;
    world.get_mut::<Transform>(cube).unwrap().translation[0] = 2.;
    instance
        .reset_render_interpolation(&mut world, "root")
        .unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "cube", 0.).x, 104.);
    world.advance_change_tick();
    world.get_mut::<Transform>(root).unwrap().translation[0] = 110.;
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "cube", 0.5).x, 109.);
    assert!(
        instance
            .reset_render_interpolation(&mut world, "missing")
            .is_err()
    );
}

#[test]
fn teleporting_a_child_snaps_its_world_pose_without_snapping_siblings() {
    let (instance, mut world) = setup();
    world.advance_change_tick();
    world
        .get_mut::<Transform>(instance.entity("root").unwrap())
        .unwrap()
        .translation[0] = 10.;
    world
        .get_mut::<Transform>(instance.entity("cube").unwrap())
        .unwrap()
        .translation[0] = 2.;
    instance
        .reset_render_interpolation(&mut world, "cube")
        .unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    let matrices = instance.interpolated_transforms(&world, 0.5).unwrap();
    assert_eq!(matrices["root"].transform_point3(Vec3::ZERO).x, 5.);
    assert_eq!(matrices["cube"].transform_point3(Vec3::ZERO).x, 14.);
    assert_eq!(matrices["lamp"].transform_point3(Vec3::ZERO).x, 14.);
    assert_eq!(matrices["camera"].transform_point3(Vec3::ZERO).x, 5.);
}

#[test]
fn prefab_removal_reindexes_history_without_ghosts_or_reused_entity_poses() {
    let mut source = scene();
    source.assets.insert(
        "piece".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Prefab,
            path: "piece.prefab.json".into(),
        },
    );
    let mut world = World::new();
    let mut instance = source.spawn(&mut world).unwrap();
    instance.set_render_interpolation(&mut world, true).unwrap();
    let mut object = scene().objects[1].clone();
    object.id = "piece-root".into();
    object.parent = None;
    let prefab = Prefab {
        version: 1,
        name: "Piece".into(),
        root: object.id.clone(),
        objects: vec![object],
        assets: BTreeMap::new(),
        nested: BTreeMap::new(),
        base: None,
    };
    instance.register_prefab("piece".into(), prefab).unwrap();
    world.advance_change_tick();
    let first = instance
        .spawn_prefab(&mut world, "piece", [20., 0., 0.])
        .unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    let old_entity = instance.entity(&first).unwrap();
    assert_eq!(position(&instance, &world, &first, 0.).x, 20.);
    world.advance_change_tick();
    instance.destroy_prefab(&mut world, &first).unwrap();
    let second = instance
        .spawn_prefab(&mut world, "piece", [40., 0., 0.])
        .unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_ne!(instance.entity(&second).unwrap(), old_entity);
    let poses = instance.interpolated_transforms(&world, 0.5).unwrap();
    assert!(!poses.contains_key(&first));
    assert_eq!(poses[&second].transform_point3(Vec3::ZERO).x, 40.);
    assert_eq!(poses.len(), instance.document().objects.len());
}

#[test]
fn restart_and_checkpoint_load_replace_history_without_serializing_it() {
    let (mut instance, mut world) = setup();
    world.advance_change_tick();
    world
        .get_mut::<Transform>(instance.entity("root").unwrap())
        .unwrap()
        .translation[0] = 10.;
    instance.capture_render_transforms(&mut world).unwrap();
    let saved = instance.save_game_json(&world).unwrap();
    instance.restart_runtime_scene(&mut world).unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "root", 0.5).x, 0.);
    instance.load_game_json(&mut world, &saved).unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "root", 0.5).x, 10.);
    instance
        .set_render_interpolation(&mut world, false)
        .unwrap();
    assert!(!SceneInstance::render_interpolation_enabled(&world));
    assert_eq!(instance.save_game_json(&world).unwrap(), saved);
}

#[test]
fn tween_seeks_and_timeline_camera_cuts_do_not_blend_across_discontinuities() {
    use bozzard_scene::middleware::{
        curve::Curve,
        registry,
        timeline::{CameraCut, Timeline},
        tween::{Control, Property, Track, Tween},
    };
    use std::sync::Arc;
    let mut source = scene();
    let mut track = Track::new(Property::Translation);
    track.channels[0] = Curve::linear(0., 10., 2.);
    registry::set(
        &mut source.objects[0],
        &Tween {
            autoplay: true,
            duration: 2.,
            tracks: Arc::new(vec![track]),
            ..Default::default()
        },
    )
    .unwrap();
    let mut cut_camera = source.objects[3].clone();
    cut_camera.id = "cut-camera".into();
    cut_camera.parent = Some("root".into());
    cut_camera.transform.translation[0] = 40.;
    source.objects.push(cut_camera);
    registry::set(
        &mut source.objects[1],
        &Timeline {
            motion: Tween {
                autoplay: true,
                duration: 2.,
                ..Default::default()
            },
            cameras: Arc::new(vec![CameraCut {
                time: 1.,
                camera: "cut-camera".into(),
                layer: Layer::ThreeD,
            }]),
            ..Default::default()
        },
    )
    .unwrap();
    let mut world = World::new();
    let instance = source.spawn(&mut world).unwrap();
    instance.set_render_interpolation(&mut world, true).unwrap();
    world.advance_change_tick();
    instance.step_tweens(&mut world, 0.5).unwrap();
    instance.step_timelines(&mut world, 0.5).unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "root", 0.5).x, 1.25);
    world.advance_change_tick();
    instance
        .control_tween(&mut world, "root", Control::Seek(1.5))
        .unwrap();
    instance.step_tweens(&mut world, 0.).unwrap();
    world
        .get_mut::<Transform>(instance.entity("cut-camera").unwrap())
        .unwrap()
        .translation[0] = 60.;
    instance.step_timelines(&mut world, 1.).unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "root", 0.5).x, 7.5);
    let camera = instance.entity("cut-camera").unwrap();
    let projection = world
        .get::<bozzard_scene::Camera>(camera)
        .unwrap()
        .projection(1.)
        .unwrap();
    let expected = projection * instance.global_transforms(&world).unwrap()["cut-camera"].inverse();
    assert_matrix(
        instance
            .view_interpolated_from_camera(&world, Layer::ThreeD, 1., None, 0.)
            .unwrap()
            .view_projection,
        expected,
    );
}

#[test]
fn scripts_and_blueprints_can_reset_interpolation_without_changing_transform_semantics() {
    use bozzard_scene::{
        ScriptAttachment, ScriptManager,
        blueprint::{
            Blueprint, BlueprintAttachment, Node, NodeKind, ObjectRef, Socket, Value, Wire,
        },
    };
    let mut source = scene();
    source.assets.insert(
        "motion".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Script,
            path: "motion.rhai".into(),
        },
    );
    source.objects[0].script_manager = Some(ScriptManager {
        scripts: vec![ScriptAttachment {
            enabled: true,
            script: "motion".into(),
        }],
    });
    let mut graph = Blueprint::default();
    let start = Node::new(1, NodeKind::Update, [0., 0.]);
    let mut reset = Node::new(2, NodeKind::ResetInterpolation, [200., 0.]);
    reset.inputs[1] = Value::Object(ObjectRef::Id("cube".into()));
    graph.nodes = vec![start, reset];
    graph.wires = vec![Wire {
        from: Socket { node: 1, port: 0 },
        to: Socket { node: 2, port: 0 },
    }];
    source.objects[1].blueprints = vec![BlueprintAttachment {
        enabled: true,
        graph,
    }];
    let mut world = World::new();
    let mut instance = source.spawn(&mut world).unwrap();
    instance
        .register_scripts(BTreeMap::from([(
            "motion".into(),
            r#"
                fn on_update(me, dt) {
                    set_position(me, [50.0, 0.0, 0.0]);
                    reset_interpolation(me);
                    set_position("cube", [3.0, 0.0, 0.0]);
                }
            "#
            .into(),
        )]))
        .unwrap();
    instance.set_render_interpolation(&mut world, true).unwrap();
    world.advance_change_tick();
    instance
        .step_scripts(
            &mut world,
            1. / 60.,
            bozzard_scene::GameplayInput::default(),
        )
        .unwrap();
    instance
        .step_blueprints(
            &mut world,
            1. / 60.,
            bozzard_scene::GameplayInput::default(),
        )
        .unwrap();
    instance.capture_render_transforms(&mut world).unwrap();
    assert_eq!(position(&instance, &world, "root", 0.).x, 50.);
    assert_eq!(position(&instance, &world, "cube", 0.).x, 56.);
}
