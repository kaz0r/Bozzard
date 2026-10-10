//! Player settings reach games through Blueprint nodes and script functions alike.
use bozzard_ecs::World;
use bozzard_scene::{
    GameplayInput, Layer, Scene,
    blueprint::{Blueprint, BlueprintAttachment, Node, NodeKind as N, Socket, Value, Wire},
    middleware::{audio::AudioMixer, registry},
    player_settings::{
        PlayerSettings, Quality, SETTINGS_FILE, SettingsStore, VolumeChannel, WindowMode,
    },
};

fn scene() -> Scene {
    let mut scene = Scene::from_json(
        r#"{"version":1,"name":"settings","views":{},
            "assets":{"menu":{"kind":"script","path":"menu.rs"}},
            "objects":[{"id":"menu","name":"Menu","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#,
    )
    .unwrap();
    registry::set(
        &mut scene.objects[0],
        &AudioMixer {
            master: 0.8,
            ..Default::default()
        },
    )
    .unwrap();
    scene
}

fn wire(from: (u32, usize), to: (u32, usize)) -> Wire {
    Wire {
        from: Socket {
            node: from.0,
            port: from.1,
        },
        to: Socket {
            node: to.0,
            port: to.1,
        },
    }
}

fn node(id: u32, kind: N, inputs: &[(usize, Value)]) -> Node {
    let mut node = Node::new(id, kind, [id as f32 * 100., 0.]);
    for (port, value) in inputs {
        node.inputs[*port] = value.clone();
    }
    node
}

#[test]
fn blueprint_nodes_edit_query_apply_and_save_settings_and_audio_follows() {
    let text = |s: &str| Value::Text(s.into());
    let number = Value::Number;
    let mut scene = scene();
    let graph = Blueprint {
        name: "Settings menu".into(),
        nodes: vec![
            node(1, N::Start, &[]),
            node(
                2,
                N::SetVolumeSetting,
                &[(1, text("music")), (2, number(0.25))],
            ),
            node(3, N::SetVsyncSetting, &[(1, Value::Bool(false))]),
            node(
                4,
                N::SetWindowSizeSetting,
                &[(1, number(1280.)), (2, number(720.))],
            ),
            node(5, N::SetQualitySetting, &[(1, text("Low"))]),
            node(6, N::SetWindowModeSetting, &[(1, text("borderless"))]),
            // Queries read the edited values: copy Music to UI.
            node(7, N::VolumeSetting, &[(0, text("music"))]),
            node(8, N::SetVolumeSetting, &[(1, text("ui"))]),
            // Apply only if the size query reports the edit.
            node(9, N::WindowSizeSetting, &[]),
            node(10, N::Equal, &[(1, number(1280.))]),
            node(11, N::Branch, &[]),
            node(12, N::ApplySettings, &[]),
            node(13, N::SaveSettings, &[]),
        ],
        wires: vec![
            wire((1, 0), (2, 0)),
            wire((2, 0), (3, 0)),
            wire((3, 0), (4, 0)),
            wire((4, 0), (5, 0)),
            wire((5, 0), (6, 0)),
            wire((6, 0), (8, 0)),
            wire((7, 0), (8, 2)),
            wire((8, 0), (11, 0)),
            wire((9, 0), (10, 0)),
            wire((10, 0), (11, 1)),
            wire((11, 0), (12, 0)),
            wire((12, 0), (13, 0)),
        ],
        ..Default::default()
    };
    graph.validate().unwrap();
    scene.objects[0].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    let before = instance.audio_frame(&world, Layer::ThreeD).unwrap();
    assert_eq!((before.master, before.buses), (0.8, [1.; 4]));

    instance
        .step_blueprints(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let store = world.resource::<SettingsStore>().unwrap();
    let expected = PlayerSettings {
        window_mode: WindowMode::Borderless,
        window_size: [1280, 720],
        vsync: false,
        quality: Quality::Low,
        music_volume: 0.25,
        ui_volume: 0.25,
        ..Default::default()
    };
    assert_eq!(*store.applied(), expected);
    assert_eq!(
        store.saved(),
        Some(&expected),
        "saved in memory without a host path"
    );
    assert_eq!(store.revision(), 1);
    let after = instance.audio_frame(&world, Layer::ThreeD).unwrap();
    assert_eq!(after.master, 0.8);
    assert_eq!(after.buses, [1., 0.25, 0.25, 1.]);

    // Settings live outside game checkpoints: loading a save leaves them alone.
    let save = instance.save_game_json(&world).unwrap();
    assert!(!save.contains("music_volume"));
    world
        .resource_mut::<SettingsStore>()
        .unwrap()
        .request(bozzard_scene::player_settings::Request::Reset)
        .unwrap();
    instance.load_game_json(&mut world, &save).unwrap();
    let store = world.resource::<SettingsStore>().unwrap();
    assert_eq!(*store.edited(), PlayerSettings::default());
    assert_eq!(*store.applied(), expected);
}

#[test]
fn invalid_settings_node_inputs_fail_the_graph_loudly() {
    for (kind, inputs, reason) in [
        (
            N::SetWindowModeSetting,
            vec![(1, Value::Text("huge".into()))],
            "unknown window mode",
        ),
        (
            N::SetWindowSizeSetting,
            vec![(1, Value::Number(100.)), (2, Value::Number(720.))],
            "window size must be an integer",
        ),
        (
            N::SetVolumeSetting,
            vec![(1, Value::Text("voice".into())), (2, Value::Number(0.5))],
            "unknown volume channel",
        ),
        (
            N::SetVolumeSetting,
            vec![(1, Value::Text("music".into())), (2, Value::Number(1.5))],
            "within 0–1",
        ),
        (
            N::SetQualitySetting,
            vec![(1, Value::Text("ultra".into()))],
            "unknown quality preset",
        ),
    ] {
        let mut scene = scene();
        scene.objects[0].blueprints.push(BlueprintAttachment {
            enabled: true,
            graph: Blueprint {
                nodes: vec![node(1, N::Start, &[]), node(2, kind, &inputs)],
                wires: vec![wire((1, 0), (2, 0))],
                ..Default::default()
            },
        });
        let mut world = World::default();
        let mut instance = scene.spawn(&mut world).unwrap();
        let error = instance
            .step_blueprints(&mut world, 1. / 60., GameplayInput::default())
            .unwrap_err();
        assert!(format!("{error:#}").contains(reason), "{kind:?}: {error:#}");
        assert_eq!(
            bozzard_scene::player_settings::current(&world),
            PlayerSettings::default()
        );
    }
}

#[test]
fn script_functions_mirror_the_nodes_and_save_to_the_host_file() {
    let directory =
        std::env::temp_dir().join(format!("bozzard-script-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let path = directory.join(SETTINGS_FILE);
    let mut scene = scene();
    scene.objects[0].script_manager = Some(bozzard_scene::ScriptManager {
        scripts: vec![bozzard_scene::ScriptAttachment {
            enabled: true,
            script: "menu".into(),
        }],
    });
    let mut world = World::default();
    world.insert_resource(SettingsStore::new(Some(path.clone())));
    let mut instance = scene.spawn(&mut world).unwrap();
    instance
        .register_script(
            "menu".into(),
            r#"
            fn on_start(me) {
                set_volume_setting("sfx", 0.5);
                if get_volume_setting("SFX") != 0.5 { throw "edits are visible in the same hook"; }
                set_window_mode_setting("fullscreen");
                set_window_size_setting(800, 600);
                set_quality_setting("medium");
                set_vsync_setting(false);
                let all = player_settings();
                if all.window_mode != "fullscreen" || all.window_size[0] != 800 { throw "map"; }
                if get_window_size_setting()[1] != 600 { throw "size"; }
                if get_quality_setting() != "medium" || get_vsync_setting() { throw "values"; }
                apply_settings();
                save_settings();
                reset_settings();
                if get_window_mode_setting() != "windowed" { throw "reset"; }
            }
            "#
            .into(),
        )
        .unwrap();
    instance
        .step_scripts(&mut world, 1. / 60., GameplayInput::default())
        .unwrap();
    let store = world.resource::<SettingsStore>().unwrap();
    let expected = PlayerSettings {
        window_mode: WindowMode::Fullscreen,
        window_size: [800, 600],
        vsync: false,
        quality: Quality::Medium,
        sfx_volume: 0.5,
        ..Default::default()
    };
    assert_eq!(*store.applied(), expected);
    assert_eq!(*store.edited(), PlayerSettings::default());
    assert_eq!(PlayerSettings::load(&path).unwrap(), Some(expected));
    let frame = instance.audio_frame(&world, Layer::ThreeD).unwrap();
    assert_eq!(frame.buses, [0.5, 1., 1., 0.5], "Ambience follows SFX");
    assert_eq!(expected.volume(VolumeChannel::Sfx), 0.5);

    for (call, reason) in [
        ("set_window_size_setting(10, 10)", "window size"),
        ("set_quality_setting(\"ultra\")", "unknown quality"),
        ("set_volume_setting(\"music\", 2.0)", "0–1"),
        ("get_volume_setting(\"voice\")", "unknown volume channel"),
    ] {
        instance
            .register_script("menu".into(), format!("fn on_update(me, dt) {{ {call}; }}"))
            .unwrap();
        let error = instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap_err();
        assert!(format!("{error:#}").contains(reason), "{call}: {error:#}");
    }
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn script_catalog_lists_every_settings_function() {
    let functions = bozzard_scene::script_function_descriptions().join("\n");
    for name in [
        "player_settings(",
        "get_window_mode_setting(",
        "set_window_mode_setting(",
        "get_window_size_setting(",
        "set_window_size_setting(",
        "get_vsync_setting(",
        "set_vsync_setting(",
        "get_quality_setting(",
        "set_quality_setting(",
        "get_volume_setting(",
        "set_volume_setting(",
        "apply_settings(",
        "save_settings(",
        "reset_settings(",
    ] {
        assert!(functions.contains(name), "missing {name}");
    }
}
