use bozzard_app::job::{Job, Progress};
use bozzard_ecs::World;
use bozzard_scene::{
    GameplayInput, GameplayState, Object, PlayerMotion, Scene, SceneInstance, Transform,
    blueprint::{BlackboardValue, Value},
    middleware::{
        registry,
        sprite::{Atlas, Clip, Runtime, Sprite},
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn empty() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"Loading","views":{},"objects":[]}"#).unwrap()
}
fn object(id: &str) -> Object {
    Object {
        id: id.into(),
        name: id.into(),
        ..Default::default()
    }
}
fn setup() -> (World, SceneInstance) {
    let mut main = empty();
    main.objects.push(object("existing"));
    let mut level = empty();
    level.objects.push(object("root"));
    level.objects.push(Object {
        parent: Some("root".into()),
        ..object("child")
    });
    main.runtime_scenes
        .insert("addition".into(), Arc::new(level));
    let mut world = World::default();
    let instance = main.spawn(&mut world).unwrap();
    (world, instance)
}
fn wait<T: Send + 'static>(job: &Job<T>) -> anyhow::Result<T> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "scene worker timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn additive_worker_publishes_without_resetting_live_components_or_resources() {
    let (mut world, mut instance) = setup();
    let entity = instance.entity("existing").unwrap();
    let job = instance
        .prepare_scene_load("addition", true)
        .unwrap()
        .start()
        .unwrap();
    let prepared = wait(&job).unwrap();
    assert_eq!(job.fraction(), 1.);
    assert_eq!(prepared.object_count(), 2);
    assert!(prepared.additive());
    assert_eq!(prepared.name(), "addition");
    assert_eq!(world.len(), 1);
    world.get_mut::<Transform>(entity).unwrap().translation = [7., 8., 9.];
    world.insert_resource(PlayerMotion {
        desired: glam::Vec3::X,
    });
    world.insert_resource(GameplayInput {
        jump: true,
        ..Default::default()
    });
    world.insert_resource(123_u64);
    assert_eq!(
        instance.accept_scene_load(&mut world, prepared).unwrap(),
        "scene-1"
    );
    assert_eq!(instance.entity("existing"), Some(entity));
    assert_eq!(
        world.get::<Transform>(entity).unwrap().translation,
        [7., 8., 9.]
    );
    assert_eq!(
        world.resource::<PlayerMotion>().unwrap().desired,
        glam::Vec3::X
    );
    assert!(world.resource::<GameplayInput>().unwrap().jump);
    assert_eq!(world.resource::<u64>(), Some(&123));
    assert_eq!(
        instance.document().objects[2].parent.as_deref(),
        Some("scene-1-root")
    );
    assert_eq!(world.len(), 3);
    instance
        .load_runtime_scene(&mut world, "addition", true)
        .unwrap();
    assert!(instance.entity("scene-2-child").is_some());
    assert_eq!(world.len(), 5);
}

#[test]
fn cancelled_and_stale_results_cannot_publish_even_after_poll() {
    let (mut world, mut instance) = setup();
    let original = instance.document().clone();
    let job = instance
        .prepare_scene_load("addition", true)
        .unwrap()
        .start()
        .unwrap();
    let prepared = wait(&job).unwrap();
    job.cancel();
    assert!(
        instance
            .accept_scene_load(&mut world, prepared)
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert_eq!(instance.document(), &original);
    assert_eq!(world.len(), 1);
    let prepared = instance
        .prepare_scene_load("addition", true)
        .unwrap()
        .prepare(&Progress::default())
        .unwrap();
    instance.restart_runtime_scene(&mut world).unwrap();
    assert!(
        instance
            .accept_scene_load(&mut world, prepared)
            .unwrap_err()
            .to_string()
            .contains("changed")
    );
    assert_eq!(instance.document(), &original);
    assert_eq!(world.len(), 1);
}

#[test]
fn conflicting_defaults_fail_before_touching_live_world() {
    let mut scene = empty();
    scene.objects.push(object("existing"));
    scene
        .blackboard
        .insert("score".into(), BlackboardValue::Scalar(Value::Number(1.)));
    let mut level = empty();
    level.objects.push(object("new"));
    level
        .blackboard
        .insert("score".into(), BlackboardValue::Scalar(Value::Number(2.)));
    scene
        .runtime_scenes
        .insert("addition".into(), Arc::new(level));
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let entity = instance.entity("existing").unwrap();
    let job = instance
        .prepare_scene_load("addition", true)
        .unwrap()
        .start()
        .unwrap();
    assert!(
        wait(&job)
            .err()
            .unwrap()
            .to_string()
            .contains("blackboard conflict")
    );
    assert_eq!(instance.document(), &scene);
    assert_eq!(world.len(), 1);
    assert!(world.contains(entity));
}

#[test]
fn replacement_discards_old_handles_and_motion_but_keeps_host_resources() {
    let (mut world, mut instance) = setup();
    let old = instance.entity("existing").unwrap();
    world.insert_resource(PlayerMotion {
        desired: glam::Vec3::X,
    });
    world.insert_resource(123_u64);
    let prepared = instance
        .prepare_scene_load("addition", false)
        .unwrap()
        .prepare(&Progress::default())
        .unwrap();
    instance.accept_scene_load(&mut world, prepared).unwrap();
    assert!(!world.contains(old));
    assert!(instance.entity("root").is_some());
    assert_eq!(world.len(), 2);
    assert_eq!(
        world.resource::<PlayerMotion>().unwrap().desired,
        glam::Vec3::ZERO
    );
    assert_eq!(world.resource::<u64>(), Some(&123));
    instance.restart_runtime_scene(&mut world).unwrap();
    assert!(instance.entity("root").is_some());
}

#[test]
fn prepared_sprite_state_is_moved_without_restarting_existing_animation() {
    let mut scene = empty();
    scene.assets.insert(
        "atlas".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Image,
            path: "atlas.png".into(),
        },
    );
    let mut sprite = object("sprite");
    registry::set(
        &mut sprite,
        &Sprite {
            image: "atlas".into(),
            atlas: Atlas {
                columns: 4,
                rows: 1,
            },
            initial: "walk".into(),
            clips: Arc::new(vec![Clip {
                name: "walk".into(),
                fps: 4.,
                frames: vec![0, 1, 2, 3],
                ..Default::default()
            }]),
            ..Default::default()
        },
    )
    .unwrap();
    scene.objects.push(sprite);
    let level = scene.clone();
    scene
        .runtime_scenes
        .insert("addition".into(), Arc::new(level));
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    let job = instance
        .prepare_scene_load("addition", true)
        .unwrap()
        .start()
        .unwrap();
    instance.step_sprites(&mut world, 0.6).unwrap();
    let prepared = wait(&job).unwrap();
    instance.accept_scene_load(&mut world, prepared).unwrap();
    let runtime = world.resource::<Runtime>().unwrap();
    assert_eq!(runtime.players["sprite"].frame, 2);
    assert_eq!(runtime.players["scene-1-sprite"].frame, 0);
    instance.step_sprites(&mut world, 0.25).unwrap();
    let runtime = world.resource::<Runtime>().unwrap();
    assert_eq!(runtime.players["sprite"].frame, 3);
    assert_eq!(runtime.players["scene-1-sprite"].frame, 1);
}

#[test]
fn additive_player_adopts_missing_view_and_remaps_follow_camera() {
    let mut level = empty();
    let mut player = object("player");
    player.collider = Some(Default::default());
    player.gravity = Some(Default::default());
    player.player_controller = Some(bozzard_scene::PlayerController {
        camera: "camera".into(),
        ..Default::default()
    });
    let mut camera = object("camera");
    camera.camera = Some(bozzard_scene::Camera::Perspective {
        vertical_fov_degrees: 60.,
        near: 0.1,
        far: 100.,
    });
    level
        .views
        .insert(bozzard_scene::Layer::ThreeD, "camera".into());
    level.objects.extend([player, camera]);
    let mut main = empty();
    main.runtime_scenes.insert("player".into(), Arc::new(level));
    let mut world = World::default();
    let mut instance = main.spawn(&mut world).unwrap();
    instance
        .load_runtime_scene(&mut world, "player", true)
        .unwrap();
    assert_eq!(
        instance.document().views[&bozzard_scene::Layer::ThreeD],
        "scene-1-camera"
    );
    assert_eq!(
        world.resource::<GameplayState>().unwrap().player,
        "scene-1-player"
    );
    assert_eq!(
        instance.document().objects[0]
            .player_controller
            .as_ref()
            .unwrap()
            .camera,
        "scene-1-camera"
    );
}

#[test]
fn unload_releases_only_owned_state_and_survives_checkpoint_restore() {
    let (mut world, mut instance) = setup();
    instance
        .load_runtime_scene(&mut world, "addition", true)
        .unwrap();
    instance
        .load_runtime_scene(&mut world, "addition", true)
        .unwrap();
    let existing = instance.entity("existing").unwrap();
    let other = instance.entity("scene-2-root").unwrap();
    let removed = instance.entity("scene-1-root").unwrap();
    world.get_mut::<Transform>(other).unwrap().translation[0] = 17.;
    let mut sprites = Runtime::default();
    sprites
        .players
        .insert("scene-1-root".into(), Default::default());
    sprites
        .players
        .insert("scene-2-root".into(), Default::default());
    world.insert_resource(sprites);
    let mut audio = bozzard_scene::middleware::audio::Runtime::default();
    audio.buses[0] = Some(0.3);
    world.insert_resource(audio);
    instance
        .unload_runtime_scene(&mut world, "scene-1")
        .unwrap();
    assert!(!world.contains(removed));
    assert!(world.contains(existing));
    assert!(world.contains(other));
    assert_eq!(world.get::<Transform>(other).unwrap().translation[0], 17.);
    assert_eq!(world.resource::<Runtime>().unwrap().players.len(), 1);
    assert_eq!(
        world
            .resource::<bozzard_scene::middleware::audio::Runtime>()
            .unwrap()
            .buses[0],
        Some(0.3)
    );
    assert_eq!(instance.loaded_scenes().len(), 1);
    // The artificial sprite states have no authored Sprite; remove that test fixture before save.
    world.remove_resource::<Runtime>();
    let checkpoint = instance.save_game_json(&world).unwrap();
    instance
        .unload_runtime_scene(&mut world, "scene-2")
        .unwrap();
    assert_eq!(world.len(), 1);
    instance.load_game_json(&mut world, &checkpoint).unwrap();
    assert_eq!(instance.loaded_scenes()["scene-2"].members.len(), 2);
    instance
        .unload_runtime_scene(&mut world, "scene-2")
        .unwrap();
    assert_eq!(world.len(), 1);
    assert!(instance.loaded_scenes().is_empty());
    instance
        .load_runtime_scene(&mut world, "addition", true)
        .unwrap();
    assert!(instance.entity("scene-3-root").is_some());
}

#[test]
fn invalid_saved_ownership_is_rejected_without_replacing_live_state() {
    let (mut world, mut instance) = setup();
    instance
        .load_runtime_scene(&mut world, "addition", true)
        .unwrap();
    let entity = instance.entity("existing").unwrap();
    let mut json: serde_json::Value =
        serde_json::from_str(&instance.save_game_json(&world).unwrap()).unwrap();
    json["additive_scenes"]["scene-1"]["members"] = serde_json::json!(["missing"]);
    assert!(
        instance
            .load_game_json(&mut world, &json.to_string())
            .is_err()
    );
    assert_eq!(instance.entity("existing"), Some(entity));
    assert_eq!(world.len(), 3);
}

#[test]
fn owner_spawned_prefabs_unload_but_persistent_spawns_survive() {
    let mut main = empty();
    main.objects.push(object("existing"));
    main.assets.insert(
        "crate".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Prefab,
            path: "crate.prefab.json".into(),
        },
    );
    let mut level = empty();
    level.objects.push(object("spawner"));
    main.runtime_scenes
        .insert("addition".into(), Arc::new(level));
    let mut world = World::default();
    let mut instance = main.spawn(&mut world).unwrap();
    instance
        .register_prefab(
            "crate".into(),
            bozzard_scene::Prefab {
                nested: Default::default(),
                base: None,
                version: 1,
                name: "Crate".into(),
                root: "crate".into(),
                objects: vec![object("crate")],
                assets: Default::default(),
            },
        )
        .unwrap();
    instance
        .load_runtime_scene(&mut world, "addition", true)
        .unwrap();
    let owned = instance
        .spawn_prefab_for(&mut world, "scene-1-spawner", "crate", [1., 0., 0.])
        .unwrap();
    let persistent = instance
        .spawn_prefab(&mut world, "crate", [2., 0., 0.])
        .unwrap();
    assert!(instance.loaded_scenes()["scene-1"].members.contains(&owned));
    let checkpoint = instance.save_game_json(&world).unwrap();
    instance.load_game_json(&mut world, &checkpoint).unwrap();
    instance
        .unload_runtime_scene(&mut world, "scene-1")
        .unwrap();
    assert!(instance.entity(&owned).is_none());
    assert!(instance.entity(&persistent).is_some());
    assert_eq!(world.len(), 2);
}

#[test]
fn async_lifecycle_is_polled_by_shared_ticks_and_exposes_failure_and_cancellation() {
    use bozzard_scene::scene_loading::LoadPhase;
    let (mut world, mut instance) = setup();
    instance
        .begin_scene_load(&mut world, "addition", true)
        .unwrap();
    assert!(
        instance
            .begin_scene_load(&mut world, "addition", true)
            .is_err()
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while instance.scene_load_status(&world).phase == LoadPhase::Loading {
        instance
            .step_blueprints(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let status = instance.scene_load_status(&world);
    assert_eq!(status.phase, LoadPhase::Loaded);
    assert_eq!(status.progress, 1.);
    assert_eq!(status.handle, "scene-1");
    instance
        .begin_scene_load(&mut world, "addition", true)
        .unwrap();
    instance.cancel_scene_load(&mut world);
    assert_eq!(
        instance.scene_load_status(&world).phase,
        LoadPhase::Cancelling
    );
    while instance
        .begin_scene_load(&mut world, "addition", true)
        .is_err()
    {
        instance.poll_scene_load(&mut world).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    // A restart invalidates even a result prepared from an identical document.
    instance.restart_runtime_scene(&mut world).unwrap();
    while instance.scene_load_status(&world).phase == LoadPhase::Loading {
        instance
            .step_blueprints(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(instance.scene_load_status(&world).phase, LoadPhase::Failed);
    assert!(instance.scene_load_status(&world).error.contains("changed"));
    assert_eq!(world.len(), 1);
}

#[test]
fn scripts_load_in_background_read_status_and_unload_with_destroy_hooks() {
    let mut main = empty();
    main.assets.insert(
        "controller".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Script,
            path: "controller.rs".into(),
        },
    );
    main.assets.insert(
        "worker".into(),
        bozzard_scene::AssetSource {
            kind: bozzard_scene::AssetKind::Script,
            path: "worker.rs".into(),
        },
    );
    main.blackboard.insert(
        "destroyed".into(),
        BlackboardValue::Scalar(Value::Bool(false)),
    );
    main.objects.push(object("controller"));
    main.objects[0].script_manager = Some(
        serde_json::from_value(
            serde_json::json!({"scripts":[{"script":"controller","enabled":true}]}),
        )
        .unwrap(),
    );
    let mut level = empty();
    level.assets = main.assets.clone();
    level.blackboard = main.blackboard.clone();
    level.objects.push(object("worker"));
    level.objects[0].script_manager = Some(
        serde_json::from_value(serde_json::json!({"scripts":[{"script":"worker","enabled":true}]}))
            .unwrap(),
    );
    main.runtime_scenes
        .insert("addition".into(), Arc::new(level));
    let mut world = World::default();
    let mut instance = main.spawn(&mut world).unwrap();
    instance
        .register_script(
            "controller".into(),
            r#"
        fn on_start(me) { add_scene_async("addition"); }
        fn on_update(me, dt) {
            if loaded_scene_handle() != "" && !scene_loading() {
                set_position(me, [scene_load_progress(), 0.0, 0.0]);
            }
        }
    "#
            .into(),
        )
        .unwrap();
    instance
        .register_script(
            "worker".into(),
            r#"
        fn on_destroy(me) { set_scene_variable("destroyed", true); }
    "#
            .into(),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        if world
            .get::<Transform>(instance.entity("controller").unwrap())
            .unwrap()
            .translation[0]
            == 1.
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    instance
        .unload_runtime_scene(&mut world, "scene-1")
        .unwrap();
    assert_eq!(world.len(), 1);
    assert_eq!(
        world
            .resource::<bozzard_scene::BlueprintRuntime>()
            .unwrap()
            .scene_blackboard()["destroyed"],
        BlackboardValue::Scalar(Value::Bool(true))
    );
    let saved = instance.save_game_json(&world).unwrap();
    instance.load_game_json(&mut world, &saved).unwrap();
}

#[test]
fn blueprint_async_actions_and_typed_status_outputs_drive_gameplay() {
    use bozzard_scene::blueprint::{
        Blueprint, BlueprintAttachment, Node, NodeKind as K, Socket, VariableScope, Wire,
    };
    let (_, old) = setup();
    let mut main = old.document().clone();
    main.blackboard.insert(
        "progress".into(),
        BlackboardValue::Scalar(Value::Number(0.)),
    );
    let mut graph = Blueprint::default();
    let mut load = Node::new(3, K::AddSceneAsync, [0.; 2]);
    load.inputs[1] = Value::Text("addition".into());
    let status = Node::new(4, K::SceneLoadStatus, [0.; 2]);
    let mut set = Node::new(5, K::SetVariable, [0.; 2]);
    set.variable = "progress".into();
    set.scope = VariableScope::Scene;
    graph.nodes.extend([load, status, set]);
    for (from, port, to, input) in [(1, 0, 3, 0), (2, 0, 5, 0), (4, 1, 5, 1)] {
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
    main.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
    let mut world = World::default();
    let mut instance = main.spawn(&mut world).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        instance
            .step_blueprints(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        if instance.entity("scene-1-root").is_some()
            && world
                .resource::<bozzard_scene::BlueprintRuntime>()
                .unwrap()
                .scene_blackboard()["progress"]
                == BlackboardValue::Scalar(Value::Number(1.))
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(world.len(), 3);
}

#[test]
fn prepared_replacement_waits_for_a_suspended_debugger_tick() {
    use bozzard_scene::blueprint::{
        Blueprint, BlueprintAttachment, Node, NodeKind as K, Socket, Wire,
    };
    use bozzard_scene::{BlueprintDebugger, BlueprintRuntime, Breakpoint};
    let (_, old) = setup();
    let mut main = old.document().clone();
    let mut graph = Blueprint::default();
    graph.nodes.push(Node::new(3, K::Translate, [0.; 2]));
    graph
        .connect(Wire {
            from: Socket { node: 1, port: 0 },
            to: Socket { node: 3, port: 0 },
        })
        .unwrap();
    main.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
    let mut world = World::default();
    let mut instance = main.spawn(&mut world).unwrap();
    world.insert_resource(BlueprintDebugger::new([Breakpoint {
        scene: main.name,
        object: "existing".into(),
        attachment: 0,
        node: 3,
    }]));
    instance
        .step_blueprints(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert!(world.resource::<BlueprintRuntime>().unwrap().suspended());
    let prepared = instance
        .prepare_scene_load("addition", false)
        .unwrap()
        .prepare(&Progress::default())
        .unwrap();
    assert!(
        instance
            .accept_scene_load(&mut world, prepared)
            .unwrap_err()
            .to_string()
            .contains("suspended")
    );
    assert_eq!(world.len(), 1);
    instance
        .begin_scene_load(&mut world, "addition", false)
        .unwrap();
    assert_eq!(instance.poll_scene_load(&mut world).unwrap(), None);
    assert_eq!(world.len(), 1);
    instance.cancel_scene_load(&mut world);
}

#[test]
fn additive_joint_references_follow_the_new_body_ids() {
    let mut main = empty();
    let mut level = empty();
    let mut a = object("a");
    a.collider = Some(Default::default());
    a.gravity = Some(Default::default());
    a.joint = Some(bozzard_scene::Joint {
        other: "b".into(),
        ..Default::default()
    });
    let mut b = object("b");
    b.collider = Some(Default::default());
    b.transform.translation[0] = 2.;
    level.objects.extend([a, b]);
    main.runtime_scenes.insert("joint".into(), Arc::new(level));
    let mut world = World::default();
    let mut instance = main.spawn(&mut world).unwrap();
    instance
        .load_runtime_scene(&mut world, "joint", true)
        .unwrap();
    assert_eq!(
        world
            .get::<bozzard_scene::Joint>(instance.entity("scene-1-a").unwrap())
            .unwrap()
            .other,
        "scene-1-b"
    );
    instance
        .unload_runtime_scene(&mut world, "scene-1")
        .unwrap();
    assert!(world.is_empty());
}

#[test]
fn async_replacement_applies_destroy_handler_saves_at_the_same_boundary() {
    use bozzard_scene::blueprint::{
        Blueprint, BlueprintAttachment, Node, NodeKind as K, Socket, Wire,
    };
    let (_, old) = setup();
    let mut main = old.document().clone();
    let mut graph = Blueprint::default();
    graph.nodes.push(Node::new(3, K::Destroy, [0.; 2]));
    let mut save = Node::new(4, K::SaveGame, [0.; 2]);
    save.inputs[1] = Value::Text("departure".into());
    graph.nodes.push(save);
    graph
        .connect(Wire {
            from: Socket { node: 3, port: 0 },
            to: Socket { node: 4, port: 0 },
        })
        .unwrap();
    main.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
    let mut world = World::default();
    let mut instance = main.spawn(&mut world).unwrap();
    instance
        .begin_scene_load(&mut world, "addition", false)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while instance.scene_load_status(&world).phase.busy() {
        instance
            .step_blueprints(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        instance.scene_load_status(&world).phase,
        bozzard_scene::scene_loading::LoadPhase::Loaded
    );
    assert!(instance.entity("root").is_some());
    assert!(
        world
            .resource::<bozzard_scene::scene_control::GameSaves>()
            .is_some()
    );
}
