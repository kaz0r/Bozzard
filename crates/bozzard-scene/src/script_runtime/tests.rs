use super::*;

#[test]
fn check_script_sources_resolves_imports_and_hook_arity() {
    let progress = bozzard_app::job::Progress::default();
    let catalog = |entries: &[(&str, &str)]| {
        entries
            .iter()
            .map(|(id, source)| ((*id).to_owned(), (*source).to_owned()))
            .collect::<BTreeMap<_, _>>()
    };
    check_script_sources(
        catalog(&[
            ("kit/math", "fn twice(x) { x * 2.0 }"),
            (
                "kit/player",
                "import \"kit/math\" as math;\nfn network_input(key) { math::twice(1.0) > 1.0 }",
            ),
        ]),
        &progress,
    )
    .unwrap();
    let missing = check_script_sources(
        catalog(&[(
            "kit/player",
            "import \"kit/absent\" as other;\nfn f() { 1 }",
        )]),
        &progress,
    );
    assert!(missing.unwrap_err().to_string().contains("kit/absent"));
    let arity = check_script_sources(
        catalog(&[("kit/player", "fn network_input(key, extra) { true }")]),
        &progress,
    );
    assert!(arity.unwrap_err().to_string().contains("network_input"));
}

#[test]
fn network_requests_are_local_bounded_and_disabled_without_a_session() {
    let (mut instance, mut world) = demo(
        r#"
        fn on_update(me,dt) {
            let queued = network_send("factory.move", #{x:1,z:0});
            if queued != network_active() { throw "wrong session acceptance"; }
        }
    "#,
    );
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert!(world.resource::<NetworkOutbox>().is_none());
    world.insert_resource(NetworkFrame {
        active: true,
        ..Default::default()
    });
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let requests: Vec<_> = world
        .resource_mut::<NetworkOutbox>()
        .unwrap()
        .drain()
        .collect();
    assert_eq!(
        requests,
        vec![NetworkRequest {
            owner: "thing".into(),
            kind: "factory.move".into(),
            payload: serde_json::json!({"x":1,"z":0}),
        }]
    );
    assert!(world.resource::<NetworkOutbox>().unwrap().is_empty());
    instance
        .register_script(
            "drift".into(),
            r#"
        fn on_update(me,dt) {
            for i in 0..64 { network_send("factory.move", #{x:1,z:0}); }
        }
    "#
            .into(),
        )
        .unwrap();
    for _ in 0..3 {
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
    }
    assert_eq!(
        world.resource::<NetworkOutbox>().unwrap().len(),
        module::MAX_NETWORK_REQUESTS
    );
    world.insert_resource(NetworkFrame::default());
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert!(
        world.resource::<NetworkOutbox>().unwrap().is_empty(),
        "disconnect must discard unsent intent"
    );
    world.insert_resource(NetworkFrame {
        active: true,
        ..Default::default()
    });
    world
        .resource_mut::<NetworkOutbox>()
        .unwrap()
        .drain()
        .for_each(drop);
    for source in [
        r#"fn on_update(me,dt) { network_send("bad kind", #{}); }"#,
        r#"fn on_update(me,dt) { let text=""; for i in 0..1000 { text+="12345678"; } network_send("factory.move", #{text:text}); }"#,
    ] {
        instance
            .register_script("drift".into(), source.into())
            .unwrap();
        assert!(
            instance
                .step_scripts(&mut world, 1. / 60., GameplayInput::default())
                .is_err()
        );
        assert!(world.resource::<NetworkOutbox>().unwrap().is_empty());
    }
}

#[test]
fn borrowed_boards_restore_failed_hooks_and_observe_external_writes_and_destruction() {
    let scene = Scene::from_json(r#"{"version":1,"name":"Borrowed boards","views":{},
        "blackboard":{"fail":{"scalar":{"bool":false}},"number":{"scalar":{"number":1}},
            "numbers":{"list":{"element":"number","capacity":4,"values":[{"number":1}]}}},
        "assets":{"read":{"kind":"script","path":"read.rhai"}},
        "objects":[{"id":"reader","name":"Reader","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "blackboard":{"labels":{"list":{"element":"text","capacity":3,"values":[{"text":"first"}]}},"number":{"scalar":{"number":1}}},
            "script_manager":{"scripts":[{"enabled":true,"script":"read"}]}}]}"#).unwrap();
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance.register_script("read".into(), r#"
        fn on_update(me,dt) {
            let n=get_scene_list_item("numbers",0)+1.0;
            set_scene_list("numbers",[0.0]); set_scene_list("numbers",[n]);
            set_scene_variable("number",0.0); set_scene_variable("number",n);
            set_object_variable("number",0.0); set_object_variable("number",n);
            set_object_list("labels",["pending"]);
            set_object_list(me,"labels",[n.to_string()]);
            if get_object_list_item("labels",0)!=n.to_string() || get_scene_list_item("numbers",0)!=n {
                throw "pending writes must be readable";
            }
            if get_scene_variable("fail") {throw "intentional rollback";}
        }
        fn on_destroy(me) { print("destroyed " + get_object_list_item("labels",0)); }
    "#.into()).unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let runtime = world.resource_mut::<BlueprintRuntime>().unwrap();
    runtime
        .patch_blackboards(
            &[
                ("fail".into(), B::Scalar(Value::Bool(true))),
                (
                    "numbers".into(),
                    B::List {
                        element: blueprint::PinType::Number,
                        capacity: 4,
                        values: vec![Value::Number(99.)],
                    },
                ),
            ]
            .into(),
            &Default::default(),
        )
        .unwrap();
    let before_scene = runtime.scene_blackboard().clone();
    let before_object = runtime.object_blackboard("reader").unwrap().clone();
    let error = instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap_err();
    assert!(error.to_string().contains("intentional rollback"));
    let runtime = world.resource_mut::<BlueprintRuntime>().unwrap();
    assert_eq!(runtime.scene_blackboard(), &before_scene);
    assert_eq!(runtime.object_blackboard("reader").unwrap(), &before_object);
    runtime
        .patch_blackboards(
            &[("fail".into(), B::Scalar(Value::Bool(false)))].into(),
            &Default::default(),
        )
        .unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let runtime = world.resource::<BlueprintRuntime>().unwrap();
    assert_eq!(
        runtime.scene_blackboard()["numbers"].values(),
        &[Value::Number(100.)]
    );
    assert_eq!(
        runtime.scene_blackboard()["number"],
        B::Scalar(Value::Number(100.))
    );
    assert_eq!(
        runtime.object_blackboard("reader").unwrap()["number"],
        B::Scalar(Value::Number(100.))
    );
    instance.scene_script_destroy_events(&mut world).unwrap();
    assert!(
        world
            .resource::<ScriptRuntime>()
            .unwrap()
            .messages()
            .any(|message| message.contains("destroyed 100"))
    );
}

#[test]
fn indexed_list_reads_share_pending_writes_and_validate_actual_length() {
    let scene=Scene::from_json(r#"{"version":1,"name":"Indexed lists","views":{},
        "blackboard":{"numbers":{"list":{"element":"number","capacity":4,"values":[{"number":1},{"number":2}]}}},
        "assets":{"read":{"kind":"script","path":"read.rhai"}},
        "objects":[{"id":"reader","name":"Reader","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "blackboard":{"labels":{"list":{"element":"text","capacity":3,"values":[{"text":"first"}]}},"scalar":{"scalar":{"number":1}}},
            "script_manager":{"scripts":[{"enabled":true,"script":"read"}]}}]}"#).unwrap();
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance.register_script("read".into(),r#"
        fn on_update(me,dt) {
            if get_scene_list_item("numbers",1)!=2.0 || get_object_list_item("labels",0)!="first" { throw "wrong indexed value"; }
            set_scene_list("numbers",[3.0,4.0]); set_object_list("labels",["second"]);
            if get_scene_list_item("numbers",0)!=3.0 || get_object_list_item("labels",0)!="second" { throw "stale indexed value"; }
        }
    "#.into()).unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    for (call, expected) in [
        ("get_scene_list_item(\"numbers\",-1)", "out of bounds"),
        ("get_scene_list_item(\"numbers\",2)", "out of bounds"),
        ("get_object_list_item(\"labels\",1)", "out of bounds"),
        ("get_object_list_item(\"scalar\",0)", "is a scalar"),
        ("get_scene_list_item(\"missing\",0)", "unknown variable"),
    ] {
        instance
            .register_script("read".into(), format!("fn on_update(me,dt) {{ {call}; }}"))
            .unwrap();
        let error = instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn list_comparison_observes_pending_writes_without_requiring_matching_capacities() {
    let scene=Scene::from_json(r#"{"version":1,"name":"List comparison","views":{},
        "assets":{"writer":{"kind":"script","path":"writer.rhai"}},
        "objects":[{"id":"writer","name":"Writer","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "blackboard":{"rows":{"list":{"element":"text","capacity":4,"values":[{"text":"one"},{"text":"two"}]}},"scalar":{"scalar":{"number":1}}},
            "script_manager":{"scripts":[{"enabled":true,"script":"writer"}]}},
        {"id":"buffer","name":"Buffer","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "blackboard":{"rows":{"list":{"element":"text","capacity":2,"values":[{"text":"one"},{"text":"two"}]}}}}]}"#).unwrap();
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance.register_script("writer".into(),r#"fn on_update(me,dt) {
        if !object_lists_equal(me,"rows","buffer","rows") {throw "equal values with different capacities";}
        set_object_list("buffer","rows",["two","one"]);
        if object_lists_equal(me,"rows","buffer","rows") {throw "stale write or incorrect ordering";}
        set_object_list("rows",["two","one"]);
        if !object_lists_equal(me,"rows","buffer","rows") {throw "pending writes should match";}
        set_object_list("buffer","rows",[]);
        if object_lists_equal(me,"rows","buffer","rows") {throw "different lengths should not match";}
    }"#.into()).unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert!(
        world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .object_blackboard("buffer")
            .unwrap()["rows"]
            .values()
            .is_empty()
    );
    for call in [
        r#"object_lists_equal("missing","rows","buffer","rows")"#,
        r#"object_lists_equal(me,"missing","buffer","rows")"#,
        r#"object_lists_equal(me,"scalar","buffer","rows")"#,
    ] {
        instance
            .register_script("writer".into(), format!("fn on_update(me,dt) {{{call};}}"))
            .unwrap();
        assert!(
            instance
                .step_scripts(&mut world, 1. / 60., GameplayInput::default())
                .is_err()
        );
    }
}

#[test]
fn targeted_object_lists_validate_and_publish_to_the_target_board() {
    let scene=Scene::from_json(r#"{"version":1,"name":"Targeted lists","views":{},
        "assets":{"writer":{"kind":"script","path":"writer.rhai"}},
        "objects":[{"id":"writer","name":"Writer","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"script_manager":{"scripts":[{"enabled":true,"script":"writer"}]}},
        {"id":"buffer","name":"Buffer","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"blackboard":{"rows":{"list":{"element":"number","capacity":2,"values":[]}}}}]}"#).unwrap();
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance
        .register_script(
            "writer".into(),
            r#"fn on_update(me,dt) {
        set_object_list("buffer","rows",[3.0,4.0]);
        if get_object_list("buffer","rows")!=[3.0,4.0] {throw "stale target board";}
    }"#
            .into(),
        )
        .unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(
        world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .object_blackboard("buffer")
            .unwrap()["rows"]
            .values(),
        &[Value::Number(3.), Value::Number(4.)]
    );
    for call in [
        r#"get_object_list("missing","rows")"#,
        r#"set_object_list("buffer","missing",[])"#,
        r#"set_object_list("buffer","rows",[1.0,2.0,3.0])"#,
        r#"set_object_list("buffer","rows",["wrong type"])"#,
    ] {
        instance
            .register_script(
                "writer".into(),
                format!("fn on_update(me,dt) {{ {call}; }}"),
            )
            .unwrap();
        assert!(
            instance
                .step_scripts(&mut world, 1. / 60., GameplayInput::default())
                .is_err()
        );
        assert_eq!(
            world
                .resource::<BlueprintRuntime>()
                .unwrap()
                .object_blackboard("buffer")
                .unwrap()["rows"]
                .values(),
            &[Value::Number(3.), Value::Number(4.)]
        );
    }
}

#[test]
fn screen_picking_round_trips_orthographic_perspective_and_parented_cameras() {
    let mut scene = Scene::from_json(r#"{"version":1,"name":"Screen picking","views":{"3d":"camera"},
        "assets":{"pick":{"kind":"script","path":"pick.rhai"}},"objects":[
            {"id":"rig","name":"Rig","transform":{"translation":[3,1,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
            {"id":"camera","name":"Camera","parent":"rig","transform":{"translation":[0,0,10],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "camera":{"projection":"orthographic","vertical_size":10,"near":0.1,"far":100}},
            {"id":"observer","name":"Observer","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "script_manager":{"scripts":[{"enabled":true,"script":"pick"}]}}
        ]}"#).unwrap();
    for perspective in [false, true] {
        for viewport in [[1080., 600.], [600., 1080.]] {
            scene
                .objects
                .iter_mut()
                .find(|o| o.id == "camera")
                .unwrap()
                .camera = Some(if perspective {
                Camera::Perspective {
                    vertical_fov_degrees: 60.,
                    near: 0.1,
                    far: 100.,
                }
            } else {
                Camera::Orthographic {
                    vertical_size: 10.,
                    near: 0.1,
                    far: 100.,
                }
            });
            let mut world = World::default();
            let mut instance = scene.spawn(&mut world).unwrap();
            world.insert_resource(middleware::ui::Runtime {
                viewport,
                ..Default::default()
            });
            instance.register_script("pick".into(),r#"
                fn on_update(me,dt) {
                    let center=world_to_screen([3.0,1.0,0.0]);
                    if abs(center[0]-0.5)>0.001 || abs(center[1]-0.5)>0.001 { throw "parented camera projection"; }
                    for point in [[3.0,1.0,0.0],[2.0,2.0,0.0],[4.0,0.0,0.0]] {
                        let screen=world_to_screen(point); let ray=screen_ray(screen[0],screen[1]);
                        if !ray.valid { throw "valid point rejected"; }
                        let t=-ray.origin[2]/ray.direction[2];
                        for axis in 0..3 {
                            if abs(ray.origin[axis]+t*ray.direction[axis]-point[axis])>0.002 { throw "ray missed projected point"; }
                        }
                    }
                    if screen_ray(-0.1,0.5).valid || screen_ray(0.5,1.1).valid { throw "outside viewport accepted"; }
                }
            "#.into()).unwrap();
            instance
                .step_scripts(&mut world, 1. / 60., GameplayInput::default())
                .unwrap();
        }
    }
}

#[test]
fn overlap_count_matches_with_and_without_overlap_hooks() {
    let scene = Scene::from_json(
        r#"{"version":1,"name":"overlap count","views":{},
            "assets":{"count":{"kind":"script","path":"count.rhai"}},
            "objects":[
              {"id":"probe","name":"Probe","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
               "collider":{"size":[1,1,1]},
               "script_manager":{"scripts":[{"enabled":true,"script":"count"}]}},
              {"id":"near","name":"Near","transform":{"translation":[0.5,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
               "collider":{"size":[1,1,1]}},
              {"id":"ghost","name":"Ghost","transform":{"translation":[0,0.5,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
               "collider":{"size":[1,1,1],"layers":2,"mask":2}},
              {"id":"far","name":"Far","transform":{"translation":[9,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
               "collider":{"size":[1,1,1]}}]}"#,
    )
    .unwrap();
    let count = r#"fn on_update(me, dt) {
        let counts = [overlap_count(me), overlap_count("near"), overlap_count("far")];
        if counts != [1.0, 1.0, 0.0] { throw `overlap counts ${counts}`; }
    }"#;
    // The first script never needs overlap sets; the second listens, which computes them eagerly.
    for source in [
        count.to_owned(),
        format!("{count}\nfn on_overlap_enter(me) {{}}"),
    ] {
        let mut world = World::new();
        let mut instance = scene.spawn(&mut world).unwrap();
        instance.register_script("count".into(), source).unwrap();
        for _ in 0..2 {
            instance
                .step_scripts(&mut world, 1. / 60., GameplayInput::default())
                .unwrap();
        }
    }
}

#[test]
fn collisionless_script_queries_observe_a_collider_added_to_the_live_world() {
    let scene = Scene::from_json(
        r#"{"version":1,"name":"spatial script","views":{},
            "assets":{"look":{"kind":"script","path":"look.rs"}},
            "objects":[
              {"id":"observer","name":"Observer","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
               "script_manager":{"scripts":[{"enabled":true,"script":"look"}]}},
              {"id":"target","name":"Target","transform":{"translation":[3,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#,
    )
    .unwrap();
    let mut world = World::new();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance
        .register_script(
            "look".into(),
            r#"fn on_update(me, dt) {
                let hit = raycast([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 10.0, me);
                set_position(me, if hit.hit { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 0.0] });
            }"#
            .into(),
        )
        .unwrap();

    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let observer = instance.entity("observer").unwrap();
    assert_eq!(world.get::<Transform>(observer).unwrap().translation[0], 0.);

    let target = instance.entity("target").unwrap();
    world.insert(target, BoxCollider::default()).unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(world.get::<Transform>(observer).unwrap().translation[0], 1.);
}

#[test]
fn unchanged_script_transform_preserves_the_ecs_change_tick() {
    let (mut instance, mut world) =
        demo("fn on_update(me, dt) { set_position(me, [0.0, 0.0, 0.0]); }");
    let entity = instance.entity("thing").unwrap();
    let before = world.changed_tick::<Transform>(entity).unwrap();
    world.advance_change_tick();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(world.changed_tick::<Transform>(entity), Some(before));

    instance
        .register_script(
            "drift".into(),
            "fn on_update(me, dt) { set_position(me, [1.0, 0.0, 0.0]); }".into(),
        )
        .unwrap();
    let tick = world.advance_change_tick();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(world.get::<Transform>(entity).unwrap().translation[0], 1.);
    assert_eq!(world.changed_tick::<Transform>(entity), Some(tick));
}

/// Swapping a registered mesh preserves the entity and ignores unchanged writes.
#[test]
fn script_mesh_swaps_reuse_entities_and_validate_assets() {
    let (mut instance, mut world) = demo(r#"fn on_update(me,dt) { set_mesh(me,"ingot"); }"#);
    instance.document.assets.insert(
        "ingot".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: "ingot.glb".into(),
        },
    );
    let entity = instance.entity("thing").unwrap();
    let transform = *world.get::<Transform>(entity).unwrap();
    let count = instance.document.objects.len();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(
        world.get::<Drawable>(entity).unwrap().mesh,
        Mesh::Asset("ingot".into())
    );
    assert_eq!(*world.get::<Transform>(entity).unwrap(), transform);
    assert_eq!(instance.document.objects.len(), count);
    let tick = world.changed_tick::<Drawable>(entity).unwrap();
    world.advance_change_tick();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(world.changed_tick::<Drawable>(entity), Some(tick));
    for asset in ["missing", "drift"] {
        instance
            .register_script(
                "drift".into(),
                format!(r#"fn on_update(me,dt) {{ set_mesh(me,"{asset}"); }}"#),
            )
            .unwrap();
        assert!(
            instance
                .step_scripts(&mut world, 1. / 60., GameplayInput::default())
                .is_err()
        );
        assert_eq!(
            world.get::<Drawable>(entity).unwrap().mesh,
            Mesh::Asset("ingot".into())
        );
    }
}

/// A scene with one drawable object that runs one script.
fn demo(source: &str) -> (SceneInstance, World) {
    let scene = Scene::from_json(
        r#"{"version":1,"name":"scripts","views":{},
            "assets":{"drift":{"kind":"script","path":"drift.rs"}},
            "objects":[
              {"id":"thing","name":"thing","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]},
                "collider":{"size":[1,1,1]},
                "gravity":{"enabled":true},
                "script_manager":{"scripts":[{"enabled":true,"script":"drift"}]}}]}"#,
    )
    .unwrap();
    let mut world = World::default();
    world.insert_resource(crate::GameSession {
        phase: crate::GamePhase::Playing,
        message: String::new(),
    });
    let mut instance = scene.spawn(&mut world).unwrap();
    instance
        .register_script("drift".into(), source.into())
        .unwrap();
    (instance, world)
}

fn scenery_demo(source: &str) -> (SceneInstance, World) {
    let (mut instance, world) = demo(source);
    instance.document.assets.insert(
        "scenery".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "scenery.prefab.json".into(),
        },
    );
    let mut root = instance.document.objects[0].clone();
    root.id = "root".into();
    root.script_manager = None;
    root.gravity = None;
    root.collider = None;
    let mut child = root.clone();
    child.id = "leaf".into();
    child.parent = Some("root".into());
    child.transform.translation = [0., 2., 0.];
    instance
        .register_prefab(
            "scenery".into(),
            Prefab {
                nested: Default::default(),
                base: None,
                version: 1,
                name: "Scenery".into(),
                root: "root".into(),
                objects: vec![child, root],
                assets: BTreeMap::new(),
            },
        )
        .unwrap();
    (instance, world)
}

#[test]
fn batched_spawns_resolve_tokens_hierarchies_and_duplicate_removals() {
    let (mut instance, mut world) = scenery_demo(
        r#"
        let roots = [];
        fn on_start(me) {
            roots.push(spawn_prefab("scenery", [1.0, 0.0, 0.0]));
            roots.push(spawn_prefab("scenery", [2.0, 0.0, 0.0]));
            set_position(roots[1], [3.0, 0.0, 0.0]);
        }
        fn on_update(me, dt) {
            if input_pressed("x") {
                for root in roots { destroy_prefab(root); destroy_prefab(root); }
            }
        }
    "#,
    );
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(instance.document.prefabs.len(), 2);
    let transforms = instance.global_transforms(&world).unwrap();
    for (link, x) in instance.document.prefabs.values().zip([1., 3.]) {
        assert_eq!(
            transforms[&link.members["leaf"]]
                .w_axis
                .truncate()
                .to_array(),
            [x, 2., 0.]
        );
    }
    instance.capture(&world).unwrap().validate().unwrap();
    instance
        .step_scripts(
            &mut world,
            1. / 60.,
            GameplayInput {
                keys: crate::keys::bit("x"),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(instance.document.prefabs.is_empty());
    assert_eq!(instance.document.objects.len(), 1);
    assert_eq!(instance.global_transforms(&world).unwrap().len(), 1);
}

#[test]
fn a_failed_spawn_batch_keeps_scene_and_entities_unchanged() {
    let (mut instance, mut world) = scenery_demo("fn on_update(me, dt) {}");
    let before = instance.capture(&world).unwrap();
    assert!(
        instance
            .spawn_prefab_batch(&mut world, &[("scenery", [0.; 3]), ("missing", [0.; 3])])
            .is_err()
    );
    assert_eq!(instance.capture(&world).unwrap(), before);
    assert_eq!(world.query::<Transform>().count(), 1);
    assert!(
        instance
            .spawn_prefab_batch(&mut world, &[("scenery", [f32::NAN, 0., 0.])])
            .is_err()
    );
    assert_eq!(instance.capture(&world).unwrap(), before);
}

#[test]
fn passive_removal_batches_preserve_scripted_destroy_hook_order() {
    let (mut instance, mut world) = scenery_demo(
        r#"
        fn on_start(me) {
            let a = spawn_prefab("scenery", [1.0, 0.0, 0.0]);
            let b = spawn_prefab("hooked", [2.0, 0.0, 0.0]);
            let c = spawn_prefab("scenery", [3.0, 0.0, 0.0]);
            destroy_prefab(a); destroy_prefab(b); destroy_prefab(c);
        }
    "#,
    );
    instance.document.assets.insert(
        "hooked".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "hooked.prefab.json".into(),
        },
    );
    instance.document.assets.insert(
        "hook".into(),
        AssetSource {
            kind: AssetKind::Script,
            path: "hook.rs".into(),
        },
    );
    let mut prefab = instance.templates["scenery"].clone();
    let manager = instance.document.objects[0].script_manager.clone().unwrap();
    let root = prefab.objects.iter_mut().find(|o| o.id == "root").unwrap();
    root.script_manager = Some(manager);
    root.script_manager.as_mut().unwrap().scripts[0].script = "hook".into();
    prefab
        .assets
        .insert("hook".into(), instance.document.assets["hook"].clone());
    instance.register_prefab("hooked".into(), prefab).unwrap();
    instance
        .register_script(
            "hook".into(),
            "fn on_destroy(me) { throw \"destroy hook reached\"; }".into(),
        )
        .unwrap();
    let error = instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap_err();
    assert!(format!("{error:#}").contains("destroy hook reached"));
    let xs: Vec<_> = instance
        .document
        .prefabs
        .keys()
        .map(|id| {
            world
                .get::<Transform>(instance.entity(id).unwrap())
                .unwrap()
                .translation[0]
        })
        .collect();
    assert_eq!(
        xs,
        [2., 3.],
        "earlier passive removals flush before hooks; later removals wait"
    );
}

#[test]
fn prefab_destroy_hooks_apply_cleanup_commands_before_the_next_destroy() {
    let (mut instance, mut world) = scenery_demo(
        r#"let roots=[];
        fn on_start(me) {
            roots.push(spawn_prefab("scenery",[1.0,0.0,0.0]));
            roots.push(spawn_prefab("scenery",[2.0,0.0,0.0]));
        }
        fn on_update(me,dt) {
            if input_pressed("x") {for root in roots {destroy_prefab(root);}}
        }"#,
    );
    instance.document.assets.insert(
        "cleanup".into(),
        AssetSource {
            kind: AssetKind::Script,
            path: "cleanup.rhai".into(),
        },
    );
    let mut prefab = instance.templates["scenery"].clone();
    let root = prefab.objects.iter_mut().find(|o| o.id == "root").unwrap();
    let mut manager = instance.document.objects[0].script_manager.clone().unwrap();
    manager.scripts[0].script = "cleanup".into();
    root.script_manager = Some(manager);
    prefab.assets.insert(
        "cleanup".into(),
        instance.document.assets["cleanup"].clone(),
    );
    instance.register_prefab("scenery".into(), prefab).unwrap();
    instance
        .register_script(
            "cleanup".into(),
            r#"
        fn on_destroy(me) {
            let previous=get_position("thing");
            set_position("thing",[previous[0]+1.0,0.0,0.0]);
            set_position(me,[0.0,1.0,0.0]);
            print("cleanup "+get_position("thing")[0].to_string());
        }"#
            .into(),
        )
        .unwrap();
    instance
        .step_scripts(&mut world, 1.0 / 60.0, GameplayInput::default())
        .unwrap();
    instance
        .step_scripts(
            &mut world,
            1.0 / 60.0,
            GameplayInput {
                keys: crate::keys::bit("x"),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(instance.document.prefabs.is_empty());
    assert_eq!(
        world
            .get::<Transform>(instance.entity("thing").unwrap())
            .unwrap()
            .translation,
        [2.0, 0.0, 0.0]
    );
    assert_eq!(
        world
            .resource::<ScriptRuntime>()
            .unwrap()
            .messages
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        ["cleanup 1.0", "cleanup 2.0"]
    );
}

#[test]
fn forward_vector_observes_queued_rotations_and_subsequent_ticks() {
    let (mut instance, mut world) = demo(
        r#"
        fn on_update(me,dt) {
            set_rotation(me,[0.0,90.0,0.0]);
            let facing=forward_vector(me);
            if abs(facing[0]+1.0)>0.0001 || abs(facing[2])>0.0001 {throw "stale direction";}
        }
    "#,
    );
    for _ in 0..2 {
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
    }
}

#[test]
fn a_script_moves_its_object_through_the_same_actions_blueprints_use() {
    let (mut instance, mut world) = demo(
        r#"
        fn on_start(me) { print("starting"); }
        fn on_update(me, dt) {
            set_position(me, [1.0, 2.0, 3.0]);
            // A queued write is visible to later reads in the same tick.
            if get_position(me)[1] < 2.0 { set_position(me, [0.0, 0.0, 0.0]); }
            set_velocity(me, [0.0, 0.0, 0.0]);
        }
        "#,
    );
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let entity = instance.entity("thing").unwrap();
    assert_eq!(
        world.get::<Transform>(entity).unwrap().translation,
        [1., 2., 3.]
    );
    let runtime = world.resource::<ScriptRuntime>().unwrap();
    assert_eq!(runtime.messages().collect::<Vec<_>>(), ["starting"]);
    assert_eq!(
        runtime.stats.hooks, 2,
        "on_start and on_update ran once each"
    );
    assert_eq!(runtime.stats.commands, 3);
    assert_eq!(
        runtime.stats.attachments.get(&("thing".into(), 0)),
        Some(&ScriptAttachmentStats {
            hooks: 2,
            commands: 3
        })
    );
}

fn finish_reload(request: ScriptReloadRequest) -> Result<ScriptReloadCandidate> {
    let job = request.start()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "script compile timed out"
        );
        std::thread::yield_now();
    }
}

fn imported_demo() -> (SceneInstance, World) {
    let (mut instance, world) = demo("");
    for id in ["math", "motion"] {
        instance.document.assets.insert(
            id.into(),
            AssetSource {
                kind: AssetKind::Script,
                path: format!("{id}.rhai"),
            },
        );
    }
    instance.register_scripts(BTreeMap::from([
        ("math".into(), "const STEP = 2.0; fn step() { global::STEP }".into()),
        ("motion".into(), "import \"math\" as math; fn move_object(me) { translate(me, [math::step(), 0.0, 0.0]); } fn step() { import \"math\" as local_math; local_math::step() }".into()),
        ("drift".into(), r#"import "motion" as motion;
            let count = 0;
            fn on_start(me) { rotate(me, [0.0, 90.0, 0.0]); }
            fn on_update(me, dt) { count += 1; motion::move_object(me); set_position(me, [get_position(me)[0], count.to_float(), 0.0]); }
            fn step() { motion::step() }
        "#.into()),
    ])).unwrap();
    (instance, world)
}

#[test]
fn asset_imports_use_live_host_preserve_globals_and_fingerprint_transitive_code() {
    let (mut instance, mut world) = imported_demo();
    let old_replay = instance.script_module("drift").unwrap();
    assert_eq!(old_replay.call::<f32>("step", vec![]).unwrap(), 2.);
    for _ in 0..2 {
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
    }
    let entity = instance.entity("thing").unwrap();
    assert_eq!(
        world.get::<Transform>(entity).unwrap().translation,
        [4., 2., 0.]
    );
    let edit = finish_reload(
        instance
            .request_script_reload(
                "math",
                "const STEP = 3.0; fn step() { global::STEP }".into(),
            )
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        instance.publish_script_reload(&mut world, edit).unwrap(),
        ScriptReloadStatus::Applied { .. }
    ));
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    // Imported edits reset the consumer's local scope, but never restart its lifecycle/world.
    let transform = world.get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation, [7., 1., 0.]);
    assert_eq!(transform.rotation_degrees, [0., 90., 0.]);
    let replay = instance.script_module("drift").unwrap();
    assert_ne!(replay.fingerprint(), old_replay.fingerprint());
    assert_eq!(replay.call::<f32>("step", vec![]).unwrap(), 3.);
    assert_eq!(old_replay.call::<f32>("step", vec![]).unwrap(), 2.);
}

#[test]
fn invalid_imports_fail_atomically_before_publication() {
    let (mut instance, mut world) = imported_demo();
    let fingerprint = instance.script_module("drift").unwrap().fingerprint();
    for (source, expected) in [
        (
            "import \"missing\" as x; fn step() { 1.0 }",
            "was not loaded",
        ),
        ("import \"motion\" as x; fn step() { 1.0 }", "cyclic"),
        (
            "fn step() { import get_object_variable(\"module\") as x; 1.0 }",
            "literal script asset IDs",
        ),
        ("print(\"side effect\"); fn step() { 1.0 }", "top-level"),
        ("let shared_value = 1; fn step() { 1.0 }", "top-level"),
        ("fn step( {", "math"),
    ] {
        let error = finish_reload(
            instance
                .request_script_reload("math", source.into())
                .unwrap(),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains(expected), "{error:#}");
        assert_eq!(
            instance.script_module("drift").unwrap().fingerprint(),
            fingerprint
        );
    }
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(
        world
            .get::<Transform>(instance.entity("thing").unwrap())
            .unwrap()
            .translation,
        [2., 1., 0.]
    );
}

#[test]
fn overlapping_import_edits_reject_stale_consumer_snapshots() {
    let (mut instance, mut world) = imported_demo();
    let old = finish_reload(
        instance
            .request_script_reload("math", "fn step() { 5.0 }".into())
            .unwrap(),
    )
    .unwrap();
    // An edit to a consumer must not be overwritten by an older dependency compile.
    let newer = finish_reload(
        instance
            .request_script_reload(
                "motion",
                "import \"math\" as math; fn step() { math::step() + 1.0 } fn move_object(me) {}"
                    .into(),
            )
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        instance.publish_script_reload(&mut world, newer).unwrap(),
        ScriptReloadStatus::Applied { .. }
    ));
    assert!(matches!(
        instance.publish_script_reload(&mut world, old).unwrap(),
        ScriptReloadStatus::Stale { .. }
    ));
    assert_eq!(
        instance
            .script_module("drift")
            .unwrap()
            .call::<f32>("step", vec![])
            .unwrap(),
        3.
    );
}

#[test]
fn new_import_consumer_invalidates_an_inflight_dependency_reload() {
    let (mut instance, mut world) = imported_demo();
    let candidate = finish_reload(
        instance
            .request_script_reload("math", "fn step() { 5.0 }".into())
            .unwrap(),
    )
    .unwrap();
    instance.document.assets.insert(
        "late-consumer".into(),
        AssetSource {
            kind: AssetKind::Script,
            path: "late.rhai".into(),
        },
    );
    instance
        .register_script(
            "late-consumer".into(),
            "import \"math\" as math; fn step() { math::step() }".into(),
        )
        .unwrap();
    assert!(matches!(
        instance
            .publish_script_reload(&mut world, candidate)
            .unwrap(),
        ScriptReloadStatus::Stale { .. }
    ));
    assert_eq!(
        instance
            .script_module("late-consumer")
            .unwrap()
            .call::<f32>("step", vec![])
            .unwrap(),
        2.
    );
}

#[test]
fn live_reload_is_atomic_and_keeps_world_state_without_restarting_hooks() {
    let (mut instance, mut world) = demo(
        "fn on_start(me) { rotate(me, [0.0, 90.0, 0.0]); }\nfn on_update(me, dt) { rotate(me, [0.0, 1.0, 0.0]); }",
    );
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let entity = instance.entity("thing").unwrap();
    let rotation = world.get::<Transform>(entity).unwrap().rotation_degrees;
    assert_eq!(rotation, [0., 91., 0.]);
    let bad = instance
        .request_script_reload("drift", "fn on_update(me) {}".into())
        .unwrap();
    assert!(
        finish_reload(bad)
            .err()
            .unwrap()
            .to_string()
            .contains("drift")
    );
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(
        world.get::<Transform>(entity).unwrap().rotation_degrees,
        [0., 92., 0.]
    );
    let good = instance.request_script_reload("drift",
        "fn on_start(me) { rotate(me, [0.0, 100.0, 0.0]); }\nfn on_update(me, dt) { rotate(me, [0.0, 2.0, 0.0]); }".into()).unwrap();
    let candidate = finish_reload(good).unwrap();
    assert_eq!(
        instance
            .publish_script_reload(&mut world, candidate)
            .unwrap(),
        ScriptReloadStatus::Applied {
            asset: "drift".into(),
            revision: 3
        }
    );
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(
        world.get::<Transform>(entity).unwrap().rotation_degrees,
        [0., 94., 0.]
    );
    assert_eq!(
        world
            .resource::<ScriptRuntime>()
            .unwrap()
            .stats
            .attachments
            .get(&("thing".into(), 0))
            .unwrap()
            .hooks,
        1
    );
}

#[test]
fn stale_reload_cannot_replace_a_newer_edit_or_restarted_scene() {
    let (mut instance, mut world) = demo("fn on_update(me, dt) {}");
    let older = finish_reload(
        instance
            .request_script_reload("drift", "fn on_update(me, dt) {}".into())
            .unwrap(),
    )
    .unwrap();
    let _newer = instance
        .request_script_reload("drift", "fn on_update(me, dt) {}".into())
        .unwrap();
    assert!(matches!(
        instance.publish_script_reload(&mut world, older).unwrap(),
        ScriptReloadStatus::Stale { .. }
    ));
    let before_restart = finish_reload(
        instance
            .request_script_reload("drift", "fn on_update(me, dt) {}".into())
            .unwrap(),
    )
    .unwrap();
    instance.restart_runtime_scene(&mut world).unwrap();
    assert!(matches!(
        instance
            .publish_script_reload(&mut world, before_restart)
            .unwrap(),
        ScriptReloadStatus::Stale { .. }
    ));
    let removed = finish_reload(
        instance
            .request_script_reload("drift", "fn on_update(me, dt) {}".into())
            .unwrap(),
    )
    .unwrap();
    instance.document.objects[0]
        .script_manager
        .as_mut()
        .unwrap()
        .scripts
        .clear();
    assert!(matches!(
        instance.publish_script_reload(&mut world, removed).unwrap(),
        ScriptReloadStatus::Stale { .. }
    ));
}

#[test]
fn script_scope_is_reinitialized_after_replacement() {
    let (mut instance, mut world) =
        demo("let speed = 1.0; fn on_update(me, dt) { rotate(me, [0.0, speed, 0.0]); }");
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let entity = instance.entity("thing").unwrap();
    assert_eq!(
        world.get::<Transform>(entity).unwrap().rotation_degrees[1],
        1.
    );
    let candidate = finish_reload(
        instance
            .request_script_reload(
                "drift",
                "let speed = 2.0; fn on_update(me, dt) { rotate(me, [0.0, speed, 0.0]); }".into(),
            )
            .unwrap(),
    )
    .unwrap();
    instance
        .publish_script_reload(&mut world, candidate)
        .unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(
        world.get::<Transform>(entity).unwrap().rotation_degrees[1],
        3.
    );
}

#[test]
fn prefab_spawned_during_reload_gets_the_new_script_scope() {
    let old = "let speed = 1.0; fn on_update(me, dt) { rotate(me, [0.0, speed, 0.0]); }";
    let new = "let speed = 2.0; fn on_update(me, dt) { rotate(me, [0.0, speed, 0.0]); }";
    let (mut instance, mut world) = demo(old);
    instance.document.assets.insert(
        "copy".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "copy.prefab.json".into(),
        },
    );
    let mut child = instance.document.objects[0].clone();
    child.id = "child".into();
    instance
        .register_prefab(
            "copy".into(),
            Prefab {
                nested: Default::default(),
                base: None,
                version: 1,
                name: "Scripted copy".into(),
                root: "child".into(),
                objects: vec![child],
                assets: BTreeMap::from([(
                    "drift".into(),
                    instance.document.assets["drift"].clone(),
                )]),
            },
        )
        .unwrap();
    let request = instance.request_script_reload("drift", new.into()).unwrap();
    let spawned = instance
        .spawn_prefab(&mut world, "copy", [0., 0., 0.])
        .unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let child = instance.entity(&spawned).unwrap();
    assert_eq!(
        world.get::<Transform>(child).unwrap().rotation_degrees[1],
        1.
    );

    instance
        .publish_script_reload(&mut world, finish_reload(request).unwrap())
        .unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(
        world.get::<Transform>(child).unwrap().rotation_degrees[1],
        3.,
        "the spawned attachment must initialize the new top-level speed"
    );
}

/// Restarting or loading a scene respawns the world. Script sources are runtime state the
/// document cannot carry, so a replacement that dropped them left every attachment unbound.
#[test]
fn a_replaced_scene_keeps_its_scripts_and_starts_their_state_over() {
    let (mut instance, mut world) = demo(
        r#"
        fn on_start(me) { rotate(me, [0.0, 90.0, 0.0]); }
        fn on_update(me, dt) { rotate(me, [0.0, 1.0, 0.0]); }
        "#,
    );
    let rotation = |instance: &SceneInstance, world: &World| {
        world
            .get::<Transform>(instance.entity("thing").unwrap())
            .unwrap()
            .rotation_degrees
    };
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(rotation(&instance, &world), [0., 91., 0.]);

    instance.restart_runtime_scene(&mut world).unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    assert_eq!(
        rotation(&instance, &world),
        [0., 91., 0.],
        "the restarted scene must run its script again from the new world's state"
    );
}

#[test]
fn a_wrong_hook_signature_fails_at_load_and_a_throwing_script_stops_the_tick() {
    let scene = Scene::from_json(
        r#"{"version":1,"name":"scripts","views":{},
            "assets":{"drift":{"kind":"script","path":"drift.rs"}},
            "objects":[{"id":"thing","name":"thing","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                "script_manager":{"scripts":[{"enabled":true,"script":"drift"}]}}]}"#,
    )
    .unwrap();
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    assert!(instance.has_scripts());
    let error = instance
        .register_script("drift".into(), "fn on_update(me) {}".into())
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("on_update takes 2"),
        "{error:#}"
    );

    let (mut instance, mut world) = demo("fn on_update(me, dt) { jump(me, 0.0); }");
    let error = instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap_err();
    assert!(format!("{error:#}").contains("jump speed"), "{error:#}");
}

/// A scene whose attachment names an asset the catalog does not hold as a script cannot run:
/// the loader says so when the scene opens instead of the simulation stopping mid-run.
#[test]
fn registering_sources_reports_an_attachment_with_no_source() {
    let json = r#"{"version":1,"name":"scripts","views":{},
        "assets":{"drift":{"kind":"script","path":"drift.rs"}},
        "objects":[{"id":"thing","name":"thing","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "script_manager":{"scripts":[{"enabled":true,"script":"drift"}]}}]}"#;
    let mut world = World::default();
    let mut instance = Scene::from_json(json).unwrap().spawn(&mut world).unwrap();
    // Nothing registered: the attachment has no source.
    let error = format!(
        "{:#}",
        instance.register_scripts(BTreeMap::new()).unwrap_err()
    );
    assert!(
        error.contains("script 'drift' on 'thing' (attachment 0)")
            && error.contains("was not loaded"),
        "{error}"
    );
    // With its source it compiles, and the check passes.
    instance
        .register_scripts(BTreeMap::from([(
            "drift".into(),
            "fn on_update(me, dt) {}".into(),
        )]))
        .unwrap();
}

/// A script names the prefab it spawns in source, which the loader cannot read, so the scene
/// catalog is the declaration and a scripted scene preloads its prefabs.
#[test]
fn a_scene_with_scripts_preloads_the_prefabs_its_scripts_can_spawn() {
    let scene = |object: &str| {
        format!(
            r#"{{"version":1,"name":"spawner","views":{{}},
                "assets":{{"shot":{{"kind":"prefab","path":"assets/shot.prefab.json"}}}},
                "objects":[{object}]}}"#
        )
    };
    let transform =
        r#""transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}"#;
    // A graph names its prefab on the node, so exactly that prefab is loaded.
    let node = Scene::from_json(&scene(&format!(
        r#"{{"id":"gun","name":"gun",{transform},
            "blueprints":[{{"enabled":true,"graph":{{"version":1,"name":"fire",
                "nodes":[{{"id":1,"position":[0,0],"kind":"spawn_prefab","prefab":"shot",
                    "inputs":["exec",{{"vector":[0,0,0]}}]}}],"wires":[]}}}}]}}"#
    )))
    .unwrap();
    assert_eq!(node.spawn_asset_ids(), BTreeSet::from(["shot".into()]));

    // A script cannot, so every prefab in the catalog stays ready to be spawned by name.
    let script = Scene::from_json(&format!(
        r#"{{"version":1,"name":"spawner","views":{{}},
            "assets":{{"shot":{{"kind":"prefab","path":"assets/shot.prefab.json"}},
                "fire":{{"kind":"script","path":"fire.rs"}}}},
            "objects":[{{"id":"gun","name":"gun",{transform},
                "script_manager":{{"scripts":[{{"enabled":true,"script":"fire"}}]}}}}]}}"#
    ))
    .unwrap();
    assert!(script.has_scripts());
    assert_eq!(script.spawn_asset_ids(), BTreeSet::from(["shot".into()]));

    // Without either authoring path there is nothing to preload.
    let empty = Scene::from_json(&scene(&format!(
        r#"{{"id":"gun","name":"gun",{transform}}}"#
    )))
    .unwrap();
    assert!(empty.spawn_asset_ids().is_empty());
}
