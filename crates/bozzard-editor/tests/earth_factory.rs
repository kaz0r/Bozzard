use bozzard_editor::Editor;
use bozzard_scene::{BlueprintRuntime, Layer, Mesh, blueprint::BlackboardValue};

#[test]
fn threaded_frames_preserve_factory_state_input_menus_and_stop_restart() -> anyhow::Result<()> {
    use bozzard_scene::{GameplayInput, blueprint::Value, keys, middleware::ui::Input};
    use std::time::Duration;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut scene = bozzard_demo::load_document(Some(&path))?;
    scene
        .blackboard
        .insert("seed".into(), BlackboardValue::Scalar(Value::Number(4.)));
    scene.blackboard.insert(
        "demo_mode".into(),
        BlackboardValue::Scalar(Value::Bool(true)),
    );
    let mut serial = Editor::new(scene.clone(), &path)?;
    let mut threaded = Editor::new(scene, &path)?;
    serial.start_play()?;
    threaded.start_play()?;
    for frame in 0..240 {
        // Travel across chunk seams, rotate the camera, and open/close modal UI.
        let held = if frame < 80 && frame % 2 == 0 {
            keys::bit("D")
        } else if frame == 82 {
            keys::bit("Ctrl") | keys::bit("R")
        } else if frame == 140 || frame == 160 {
            keys::bit("J")
        } else if frame == 164 || frame == 175 {
            keys::bit("M")
        } else if frame >= 180 && frame % 2 == 0 {
            keys::bit("A")
        } else {
            0
        };
        for (editor, worker) in [(&mut serial, false), (&mut threaded, true)] {
            let play = editor.play.as_mut().unwrap();
            play.set_gameplay_input(GameplayInput {
                keys: held,
                ..Default::default()
            });
            if frame == 100 || frame == 120 {
                play.ui_input(Layer::ThreeD, [1080., 600.], Input::Key("Escape".into()))?;
            }
            let before = play.app.ticks();
            // Include sub-tick and catch-up frames, as real presentation does.
            let delta = Duration::from_millis(if frame % 7 == 0 { 35 } else { 16 });
            editor.prepare_simulation_frame(delta, worker)?;
            assert_eq!(editor.play.as_ref().unwrap().app.ticks(), before);
            if frame % 9 == 0 {
                // Hidden viewport: end-of-frame fallback consumes exactly once.
                editor.finish_simulation_frame()?;
            } else {
                let result = editor.render_with_simulation(|| {
                    if frame == 90 {
                        Err("draw failure")
                    } else {
                        Ok(())
                    }
                })?;
                assert_eq!(result.is_err(), frame == 90);
            }
            let after = editor.play.as_ref().unwrap().app.ticks();
            editor.finish_simulation_frame()?;
            assert_eq!(editor.play.as_ref().unwrap().app.ticks(), after);
        }
        let (a, b) = (
            serial.play.as_ref().unwrap(),
            threaded.play.as_ref().unwrap(),
        );
        assert_eq!(a.app.ticks(), b.app.ticks());
        let a = a.app.world.resource::<BlueprintRuntime>().unwrap();
        let b = b.app.world.resource::<BlueprintRuntime>().unwrap();
        assert_eq!(a.scene_blackboard(), b.scene_blackboard(), "frame {frame}");
    }
    let metrics = threaded
        .play
        .as_ref()
        .unwrap()
        .app
        .world
        .resource::<bozzard_diagnostics::SimulationMetrics>()
        .unwrap();
    assert!(metrics.threaded && metrics.available);
    assert!(metrics.cpu_ms >= 0. && metrics.wait_ms >= 0.);
    // A stopped/restarted scene receives no elapsed time or commands from its predecessor.
    threaded.prepare_simulation_frame(Duration::from_secs(1), true)?;
    threaded.stop_play();
    threaded.start_play()?;
    threaded.finish_simulation_frame()?;
    assert_eq!(threaded.play.as_ref().unwrap().app.ticks(), 0);
    threaded.prepare_simulation_frame(Duration::from_millis(20), true)?;
    threaded.finish_simulation_frame()?;
    assert_eq!(threaded.play.as_ref().unwrap().app.ticks(), 1);
    threaded
        .play
        .as_mut()
        .unwrap()
        .app
        .add_system(|world, _, _| {
            world.remove_resource::<bozzard_scene::SceneInstance>();
            panic!("incomplete scene update");
        });
    threaded.prepare_simulation_frame(Duration::from_millis(20), true)?;
    assert!(threaded.finish_simulation_frame().is_err());
    assert!(
        threaded.play.is_none(),
        "a worker panic must stop Play before world queries"
    );
    threaded.stop_play();
    Ok(())
}

#[test]
fn earth_factory_opens_and_generates_a_world_in_editor_play() -> anyhow::Result<()> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    let authored = editor.scene().clone();
    assert_eq!(
        authored.views.get(&Layer::ThreeD).map(String::as_str),
        Some("camera")
    );
    assert_eq!(
        authored
            .objects
            .iter()
            .filter(|object| object.id.starts_with("ground-"))
            .count(),
        0
    );
    assert!(
        authored
            .objects
            .iter()
            .filter(|object| !object.extras.contains_key("ui_widget")
                && !object.extras.contains_key("ui_canvas"))
            .count()
            < 50,
        "the ground must be one scene object"
    );
    assert!(matches!(
        authored.objects.iter().find(|object| object.id == "ground").unwrap().drawable.as_ref().unwrap().mesh,
        Mesh::Asset(ref id) if id == "earth-ground"
    ));
    editor.assets.require_ready()?;
    let ground = editor
        .assets
        .entries()
        .find(|entry| entry.id == "earth-ground")
        .unwrap();
    let Some(bozzard_assets::AssetData::Mesh(mesh)) = ground.data() else {
        panic!("the baked ground did not import as a mesh");
    };
    assert_eq!(mesh.parts.len(), 5, "four grass shades and the earth cliff");
    assert!(mesh.parts.iter().all(|part| part.count > 0));
    for asset in ["machine-miner", "machine-smelter", "node-iron"] {
        let entry = editor
            .assets
            .entries()
            .find(|entry| entry.id == asset)
            .unwrap();
        let Some(bozzard_assets::AssetData::Prefab(prefab)) = entry.data() else {
            panic!("{asset} did not import as a prefab");
        };
        let root = prefab
            .objects
            .iter()
            .find(|object| object.id == "root")
            .unwrap();
        assert_eq!(
            root.transform.scale, [1.0; 3],
            "{asset} pivot flattens its children"
        );
        let height = prefab
            .objects
            .iter()
            .filter(|object| object.drawable.is_some())
            .map(|object| object.transform.translation[1] + object.transform.scale[1] * 0.5)
            .fold(0.0f32, f32::max);
        assert!(
            height > 0.7,
            "{asset} must read as a 3D object, not a flat tile"
        );
    }

    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    play.app.step();
    play.check_simulation()?;

    let board = play.app.world.resource::<BlueprintRuntime>().unwrap();
    let BlackboardValue::List { values, .. } = board.scene_blackboard().get("nodes").unwrap()
    else {
        panic!("the generated nodes must be stored in editor Play state");
    };
    assert_eq!(values.len(), 225);
    let render = play
        .instance()
        .view(&play.app.world, Layer::ThreeD, 16.0 / 9.0)?;
    assert!(
        render.objects.len()
            > authored
                .objects
                .iter()
                .filter(|o| o.drawable.is_some())
                .count(),
        "Play adds the script-spawned nodes and machines to the authored ground"
    );

    editor.stop_play();
    assert_eq!(
        editor.scene(),
        &authored,
        "Play must not edit the authored scene"
    );
    Ok(())
}

#[test]
#[ignore = "renders a preview image for visual inspection"]
fn capture_earth_factory_debug_hud() -> anyhow::Result<()> {
    use bozzard_render::{Gpu, SceneRenderer, wgpu};
    use bozzard_scene::{GameplayInput, blueprint::Value, keys};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    let mut scene = editor.scene().clone();
    scene
        .blackboard
        .insert("seed".into(), BlackboardValue::Scalar(Value::Number(1.)));
    editor.apply("Reproducible HUD preview", scene)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    for entry in editor.assets.entries() {
        if let Some(data) = entry.data() {
            bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
        }
    }
    let mut menu_open = false;
    for (size, label, menu, scroll) in [
        ([1280, 800], "debug", false, 0.),
        ([900, 700], "debug", false, 0.),
        ([1280, 800], "menu", true, 0.),
        ([900, 700], "menu", true, 0.),
        ([1280, 800], "zoom", false, -160.),
        ([1280, 800], "explored", false, 160.),
    ] {
        use bozzard_scene::middleware::ui::Input;
        let play = editor.play.as_mut().unwrap();
        if label == "explored" {
            for _ in 0..15 {
                for key in [keys::bit("D"), 0] {
                    play.set_gameplay_input(GameplayInput {
                        keys: key,
                        ..Default::default()
                    });
                    play.app.step();
                    play.check_simulation()?;
                }
            }
        }
        if menu != menu_open {
            play.ui_input(Layer::ThreeD, [1080., 600.], Input::Key("Escape".into()))?;
            play.app.step();
            play.check_simulation()?;
            menu_open = menu;
        }
        if scroll != 0. {
            play.ui_input(
                Layer::ThreeD,
                [1080., 600.],
                Input::ScrollAt {
                    point: [700., 300.],
                    delta: scroll,
                },
            )?;
        }
        // Warm the font atlas and exercise the real renderer -> script -> HUD path.
        for i in 0..32 {
            editor
                .prepare_simulation_frame(std::time::Duration::from_secs_f64(1.0 / 60.0), true)?;
            let mut render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
            let ui = editor.ui_frame(Layer::ThreeD, size.map(|v| v as f32))?;
            render
                .items
                .extend(bozzard_render_assets::widget_items(&ui, &editor.assets)?);
            let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                editor.render_with_simulation(|| renderer.draw(&gpu, target, size, &render))?
            })?;
            let stats = renderer.frame_stats();
            assert!(stats.visible_items > 0);
            assert!(
                stats.visible_items < stats.scene_items,
                "HUD is excluded from visible entities"
            );
            assert!(stats.cpu_ms > 0.);
            bozzard_render_assets::publish_frame(
                &mut editor.play.as_mut().unwrap().app.world,
                stats,
            );
            if i == 31 {
                if label == "debug" {
                    assert_eq!(
                        ui.element("nearby-tooltip").unwrap().text,
                        "Landing pod  [J] Journal"
                    );
                } else if label == "explored" {
                    assert!(ui.element("nearby-tooltip").is_none());
                }
                assert!(!ui.element("debug-entities").unwrap().text.contains("--"));
                assert!(
                    ui.element("debug-simulation")
                        .unwrap()
                        .text
                        .starts_with("Sim worker")
                );
                assert!(
                    !ui.element("debug-timing")
                        .unwrap()
                        .text
                        .contains("CPU draw --")
                );
                capture.write_ppm(
                    &std::env::temp_dir().join(format!("earth-factory-{label}-{}.ppm", size[0])),
                )?;
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "renders a preview image for visual inspection"]
fn capture_earth_factory_ui() -> anyhow::Result<()> {
    use bozzard_render::{Gpu, SceneRenderer, wgpu};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    let mut scene = editor.scene().clone();
    scene.blackboard.insert(
        "demo_mode".into(),
        BlackboardValue::Scalar(bozzard_scene::blueprint::Value::Bool(true)),
    );
    scene.blackboard.insert(
        "seed".into(),
        BlackboardValue::Scalar(bozzard_scene::blueprint::Value::Number(4.0)),
    );
    editor.apply("Preview layout", scene)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    for _ in 0..620 {
        play.app.step();
        play.check_simulation()?;
    }
    let gpu = pollster::block_on(Gpu::request_prefer_software(&bozzard_render::instance(
        bozzard_render::Backend::native(),
    )))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    for entry in editor.assets.entries() {
        if let Some(data) = entry.data() {
            bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
        }
    }
    let mut capture = |editor: &Editor, size: [u32; 2], label: &str| -> anyhow::Result<()> {
        let mut render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
        let frame = editor.ui_frame(Layer::ThreeD, size.map(|v| v as f32))?;
        render
            .items
            .extend(bozzard_render_assets::widget_items(&frame, &editor.assets)?);
        let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
            renderer.draw(&gpu, target, size, &render)
        })?;
        capture.write_ppm(&std::env::temp_dir().join(format!("earth-factory-ui-{label}.ppm")))?;
        Ok(())
    };
    for (size, orbit_ticks, label) in [
        ([1280, 800], 0, "1280"),
        ([900, 700], 0, "900"),
        ([1100, 450], 0, "1100"),
        ([1280, 800], 17, "camera-mid"),
        ([1280, 800], 20, "camera-end"),
    ] {
        if orbit_ticks > 0 {
            let play = editor.play.as_mut().unwrap();
            play.set_gameplay_input(bozzard_scene::GameplayInput {
                keys: if label == "camera-mid" {
                    bozzard_scene::keys::bit("Ctrl") | bozzard_scene::keys::bit("R")
                } else {
                    0
                },
                ..Default::default()
            });
            for _ in 0..orbit_ticks {
                play.app.step();
                play.check_simulation()?;
            }
        }
        capture(&editor, size, label)?;
    }
    // Open the real container with gameplay input after the camera quarter turn.
    use bozzard_scene::{GameplayInput, blueprint::Value, keys, middleware::ui::Input};
    let play = editor.play.as_mut().unwrap();
    let board = play
        .app
        .world
        .resource::<BlueprintRuntime>()
        .unwrap()
        .scene_blackboard();
    let BlackboardValue::List { values, .. } = &board["builds"] else {
        panic!("builds");
    };
    let cell = values.iter().position(|v| *v == Value::Number(4.)).unwrap() as i32;
    let number = |name: &str| {
        let BlackboardValue::Scalar(Value::Number(v)) = &board[name] else {
            panic!("number");
        };
        *v as i32
    };
    let dx = cell % 15 - 7 - number("cursor_x");
    let dz = cell / 15 - 7 - number("cursor_z");
    for (delta, positive, negative) in [(dx, "S", "W"), (dz, "A", "D")] {
        for _ in 0..delta.abs() {
            for key in [Some(if delta > 0 { positive } else { negative }), None] {
                play.set_gameplay_input(GameplayInput {
                    keys: key.map_or(0, keys::bit),
                    ..Default::default()
                });
                play.app.step();
                play.check_simulation()?;
            }
        }
    }
    play.set_gameplay_input(GameplayInput {
        keys: keys::bit("E"),
        ..Default::default()
    });
    play.app.step();
    play.check_simulation()?;
    play.set_gameplay_input(GameplayInput::default());
    for _ in 0..3 {
        play.app.step();
        play.check_simulation()?;
    }
    capture(&editor, [1280, 800], "storage-opening")?;
    let play = editor.play.as_mut().unwrap();
    for _ in 0..14 {
        play.app.step();
        play.check_simulation()?;
    }
    capture(&editor, [1280, 800], "storage")?;
    capture(&editor, [900, 700], "storage-small")?;
    let frame = editor.ui_frame(Layer::ThreeD, [1280., 800.])?;
    let slot = frame.element("inventory-slot-0").unwrap().rect;
    let play = editor.play.as_mut().unwrap();
    play.ui_input(
        Layer::ThreeD,
        [1280., 800.],
        Input::SecondaryDown([
            slot.min[0] + slot.size[0] * 0.5,
            slot.min[1] + slot.size[1] * 0.5,
        ]),
    )?;
    for _ in 0..9 {
        play.app.step();
        play.check_simulation()?;
    }
    capture(&editor, [1280, 800], "storage-menu")?;
    let play = editor.play.as_mut().unwrap();
    play.ui_input(Layer::ThreeD, [1280., 800.], Input::Key("E".into()))?;
    for _ in 0..5 {
        play.app.step();
        play.check_simulation()?;
    }
    capture(&editor, [1280, 800], "storage-closing")?;
    let play = editor.play.as_mut().unwrap();
    play.set_gameplay_input(GameplayInput {
        keys: keys::bit("J"),
        ..Default::default()
    });
    play.app.step();
    play.check_simulation()?;
    play.set_gameplay_input(GameplayInput::default());
    for _ in 0..15 {
        play.app.step();
        play.check_simulation()?;
    }
    for page in 1..=3 {
        capture(&editor, [1280, 800], &format!("journal-{page}"))?;
        capture(&editor, [900, 700], &format!("journal-{page}-small"))?;
        let play = editor.play.as_mut().unwrap();
        play.set_gameplay_input(GameplayInput {
            keys: keys::bit("ArrowRight"),
            ..Default::default()
        });
        play.app.step();
        play.check_simulation()?;
        play.set_gameplay_input(GameplayInput::default());
        play.app.step();
        play.check_simulation()?;
    }
    // Also inspect the default locked progression and actual neighboring terrain.
    let mut journey = Editor::open(&path)?;
    journey.assets.require_ready()?;
    journey.start_play()?;
    {
        let play = journey.play.as_mut().unwrap();
        play.app.step();
        play.check_simulation()?;
    }
    capture(&journey, [1280, 800], "new-game")?;
    {
        let play = journey.play.as_mut().unwrap();
        play.set_gameplay_input(GameplayInput {
            keys: keys::bit("J"),
            ..Default::default()
        });
        play.app.step();
        play.set_gameplay_input(GameplayInput::default());
        for _ in 0..15 {
            play.app.step();
            play.check_simulation()?;
        }
    }
    capture(&journey, [1280, 800], "journal-locked")?;
    capture(&journey, [900, 700], "journal-locked-small")?;
    {
        let play = journey.play.as_mut().unwrap();
        play.set_gameplay_input(GameplayInput {
            keys: keys::bit("J"),
            ..Default::default()
        });
        play.app.step();
        play.set_gameplay_input(GameplayInput::default());
        for _ in 0..12 {
            play.app.step();
            play.check_simulation()?;
        }
        for _ in 0..9 {
            for key in [keys::bit("D"), 0] {
                play.set_gameplay_input(GameplayInput {
                    keys: key,
                    ..Default::default()
                });
                play.app.step();
                play.check_simulation()?;
            }
        }
    }
    capture(&journey, [1280, 800], "chunks")?;
    Ok(())
}

#[test]
#[ignore = "native GPU comparison of streamed and fully retained chunks"]
fn chunk_streaming_matches_retained_world_rendering() -> anyhow::Result<()> {
    use bozzard_diagnostics::RenderDiagnostics;
    use bozzard_render::{Gpu, SceneRenderer, wgpu};
    use bozzard_scene::blueprint::Value;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let source = std::fs::read_to_string(path.parent().unwrap().join("scripts/earth_factory.rs"))?;
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    for (size, zoom, heading) in [
        ([1280, 800], 19., 0.),
        ([1280, 800], 9., 90.),
        ([1280, 800], 32., 45.),
        ([1920, 600], 32., 0.),
    ] {
        let mut reference = Vec::new();
        let mut reference_entities = 0;
        for streaming in [false, true] {
            let mut editor = Editor::open(&path)?;
            let mut scene = editor.scene().clone();
            scene
                .blackboard
                .insert("seed".into(), BlackboardValue::Scalar(Value::Number(4.)));
            editor.apply("Streaming comparison", scene)?;
            editor.assets.require_ready()?;
            editor.start_play()?;
            let mut script = source.replace("fn on_start(me)", "fn original_start(me)");
            if !streaming {
                script = script.replace(
                    "    update_chunk_residency();",
                    "    // Keep every chunk for the reference.",
                );
            }
            script += &format!(
                r#"
                fn on_start(me) {{
                    original_start(me);
                    for x in 1..9 {{ discover_chunk(x, 0); }}
                    for z in 1..5 {{ discover_chunk(8, z); }}
                    enter_chunk(8, 2);
                    set_object_variable("camera_zoom", {zoom:.1});
                    set_object_variable("camera_zoom_target", {zoom:.1});
                    set_camera_size("camera", {zoom:.1});
                    set_rotation("camera-rig", [0.0, {heading:.1}, 0.0]);
                }}
            "#
            );
            let play = editor.play.as_mut().unwrap();
            play.with_instance(|instance, _| {
                instance.register_script("earth-factory".into(), script)
            })?;
            let mut diagnostics = RenderDiagnostics::default();
            diagnostics.metrics.counters.viewport_aspect = size[0] as f32 / size[1] as f32;
            play.app.world.insert_resource(diagnostics);
            let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
            for entry in editor.assets.entries() {
                if let Some(data) = entry.data() {
                    bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
                }
            }
            // Compare mid-pan as well as settled views: unloading must never cut
            // away terrain beneath the animated camera.
            for tick in 1..=50 {
                let play = editor.play.as_mut().unwrap();
                play.app.step();
                play.check_simulation()?;
                if ![12, 28, 50].contains(&tick) {
                    continue;
                }
                let entities = play.instance().document().objects.len();
                let render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
                let capture =
                    bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                        renderer.draw(&gpu, target, size, &render)
                    })?;
                if streaming {
                    let index = [12, 28, 50].iter().position(|t| *t == tick).unwrap();
                    assert!(
                        reference[index] == capture.rgba,
                        "streaming changed world pixels: tick={tick} size={size:?} zoom={zoom} heading={heading}"
                    );
                    if tick == 50 {
                        assert!(
                            entities < reference_entities,
                            "distant scene objects must be unloaded"
                        );
                        println!(
                            "streaming size={size:?} zoom={zoom} heading={heading} entities={reference_entities}->{entities} mid_pan_and_settled_pixels=true"
                        );
                    }
                } else {
                    reference_entities = entities;
                    reference.push(capture.rgba);
                }
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "manual CPU profile; timing varies by host"]
fn profile_earth_factory_exploration() -> anyhow::Result<()> {
    use bozzard_diagnostics::Diagnostics;
    use bozzard_scene::{GameplayInput, blueprint::Value, keys, middleware::ui::Input};
    use std::{collections::BTreeMap, time::Instant};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    let mut scene = editor.scene().clone();
    scene
        .blackboard
        .insert("seed".into(), BlackboardValue::Scalar(Value::Number(4.)));
    editor.apply("Profile exploration", scene)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    editor.play.as_mut().unwrap().app.step();
    let mut previous = (0_i32, 0_i32);
    for (x, z) in [(0_i32, 0_i32), (2, 0), (5, 0), (8, 2), (8, 4)] {
        let play = editor.play.as_mut().unwrap();
        for (count, key) in [((x - previous.0) * 15, "D"), ((z - previous.1) * 15, "S")] {
            for _ in 0..count {
                for key in [keys::bit(key), 0] {
                    play.set_gameplay_input(GameplayInput {
                        keys: key,
                        ..Default::default()
                    });
                    play.app.step();
                    play.check_simulation()?;
                }
            }
        }
        previous = (x, z);
        let mut samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for _ in 0..60 {
            let play = editor.play.as_mut().unwrap();
            let d = play.app.world.resource_mut::<Diagnostics>().unwrap();
            d.profiler.recording = true;
            d.profiler.begin_frame();
            let started = Instant::now();
            play.app.step();
            play.check_simulation()?;
            samples
                .entry("tick".into())
                .or_default()
                .push(started.elapsed().as_secs_f64() * 1000.);
            for span in &play
                .app
                .world
                .resource::<Diagnostics>()
                .unwrap()
                .profiler
                .spans
            {
                samples
                    .entry(format!("span/{}", span.name))
                    .or_default()
                    .push(span.duration_ms);
            }
            let started = Instant::now();
            let frame = editor.ui_frame(Layer::ThreeD, [1280., 800.])?;
            samples
                .entry("UI layout".into())
                .or_default()
                .push(started.elapsed().as_secs_f64() * 1000.);
            let started = Instant::now();
            editor.play.as_mut().unwrap().ui_input(
                Layer::ThreeD,
                [1280., 800.],
                Input::PointerMove([800., 400.]),
            )?;
            samples
                .entry("pointer".into())
                .or_default()
                .push(started.elapsed().as_secs_f64() * 1000.);
            let started = Instant::now();
            std::hint::black_box(editor.render(Layer::ThreeD, 1.6)?);
            samples
                .entry("extraction".into())
                .or_default()
                .push(started.elapsed().as_secs_f64() * 1000.);
            let started = Instant::now();
            std::hint::black_box(bozzard_render_assets::widget_items(&frame, &editor.assets)?);
            samples
                .entry("UI draws".into())
                .or_default()
                .push(started.elapsed().as_secs_f64() * 1000.);
            let started = Instant::now();
            let play = editor.play.as_ref().unwrap();
            std::hint::black_box(
                play.instance()
                    .audio_frame(&play.app.world, Layer::ThreeD)?,
            );
            samples
                .entry("audio frame".into())
                .or_default()
                .push(started.elapsed().as_secs_f64() * 1000.);
            let started = Instant::now();
            std::hint::black_box(editor.collisions()?);
            samples
                .entry("collision overlay".into())
                .or_default()
                .push(started.elapsed().as_secs_f64() * 1000.);
        }
        let play = editor.play.as_ref().unwrap();
        let board = play.app.world.resource::<BlueprintRuntime>().unwrap();
        let BlackboardValue::List { values, .. } =
            &board.object_blackboard("controller").unwrap()["visited"]
        else {
            panic!("visited");
        };
        let BlackboardValue::List {
            values: resident, ..
        } = &board.object_blackboard("controller").unwrap()["resident"]
        else {
            panic!("resident");
        };
        println!(
            "region={x},{z} discovered={} resident={} entities={}",
            values.iter().filter(|v| **v == Value::Number(1.)).count(),
            resident.iter().filter(|v| **v == Value::Number(1.)).count(),
            play.instance().document().objects.len()
        );
        for (name, mut values) in samples {
            values.sort_by(f64::total_cmp);
            println!(
                "  {name}: median={:.3}ms p95={:.3}ms",
                values[values.len() / 2],
                values[values.len() * 95 / 100]
            );
        }
        editor
            .play
            .as_mut()
            .unwrap()
            .app
            .world
            .resource_mut::<Diagnostics>()
            .unwrap()
            .profiler
            .recording = false;
    }
    Ok(())
}

#[test]
#[ignore = "manual CPU profile; timing varies by host"]
fn profile_earth_factory_cpu() -> anyhow::Result<()> {
    use bozzard_scene::{GameplayInput, blueprint::Value, keys, middleware::ui::Input};
    use std::time::Instant;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    let mut scene = editor.scene().clone();
    scene.blackboard.insert(
        "demo_mode".into(),
        BlackboardValue::Scalar(bozzard_scene::blueprint::Value::Bool(true)),
    );
    scene
        .blackboard
        .insert("seed".into(), BlackboardValue::Scalar(Value::Number(4.)));
    editor.apply("Profile layout", scene)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    for _ in 0..360 {
        play.app.step();
        play.check_simulation()?;
    }
    for open in [false, true] {
        if open {
            let play = editor.play.as_mut().unwrap();
            let board = play
                .app
                .world
                .resource::<BlueprintRuntime>()
                .unwrap()
                .scene_blackboard();
            let BlackboardValue::List { values, .. } = &board["builds"] else {
                panic!("builds");
            };
            let cell = values.iter().position(|v| *v == Value::Number(4.)).unwrap() as i32;
            let number = |name: &str| {
                let BlackboardValue::Scalar(Value::Number(v)) = &board[name] else {
                    panic!("number");
                };
                *v as i32
            };
            let dx = cell % 15 - 7 - number("cursor_x");
            let dz = cell / 15 - 7 - number("cursor_z");
            for (delta, positive, negative) in [(dx, "D", "A"), (dz, "S", "W")] {
                for _ in 0..delta.abs() {
                    for key in [Some(if delta > 0 { positive } else { negative }), None] {
                        play.set_gameplay_input(GameplayInput {
                            keys: key.map_or(0, keys::bit),
                            ..Default::default()
                        });
                        play.app.step();
                        play.check_simulation()?;
                    }
                }
            }
            play.set_gameplay_input(GameplayInput {
                keys: keys::bit("E"),
                ..Default::default()
            });
            play.app.step();
            play.check_simulation()?;
            play.set_gameplay_input(GameplayInput::default());
            for _ in 0..20 {
                play.app.step();
                play.check_simulation()?;
            }
        }
        let mut samples: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::new());
        for _ in 0..100 {
            let start = Instant::now();
            let play = editor.play.as_mut().unwrap();
            play.app.step();
            play.check_simulation()?;
            samples[0].push(start.elapsed().as_secs_f64() * 1000.);
            let start = Instant::now();
            let frame = editor.ui_frame(Layer::ThreeD, [1280., 800.])?;
            samples[1].push(start.elapsed().as_secs_f64() * 1000.);
            let start = Instant::now();
            editor.play.as_mut().unwrap().ui_input(
                Layer::ThreeD,
                [1280., 800.],
                Input::PointerMove([650., 400.]),
            )?;
            samples[2].push(start.elapsed().as_secs_f64() * 1000.);
            let start = Instant::now();
            let _ = std::hint::black_box(editor.render(Layer::ThreeD, 1.6)?);
            samples[3].push(start.elapsed().as_secs_f64() * 1000.);
            let start = Instant::now();
            let _ =
                std::hint::black_box(bozzard_render_assets::widget_items(&frame, &editor.assets)?);
            samples[4].push(start.elapsed().as_secs_f64() * 1000.);
        }
        for (name, mut values) in [
            "simulation",
            "UI layout",
            "pointer event",
            "scene extraction",
            "UI draw list",
        ]
        .into_iter()
        .zip(samples)
        {
            values.sort_by(f64::total_cmp);
            println!(
                "inventory_open={open} {name}: median={:.2}ms p95={:.2}ms",
                values[50], values[95]
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "manual renderer profile; requires a graphics adapter"]
fn profile_earth_factory_render() -> anyhow::Result<()> {
    use bozzard_render::{Gpu, SceneRenderer, wgpu};
    use bozzard_scene::blueprint::Value;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    let mut scene = editor.scene().clone();
    scene.blackboard.insert(
        "demo_mode".into(),
        BlackboardValue::Scalar(bozzard_scene::blueprint::Value::Bool(true)),
    );
    scene
        .blackboard
        .insert("seed".into(), BlackboardValue::Scalar(Value::Number(4.)));
    editor.apply("Profile layout", scene)?;
    editor.assets.require_ready()?;
    editor.start_play()?;
    for _ in 0..360 {
        let play = editor.play.as_mut().unwrap();
        play.app.step();
        play.check_simulation()?;
    }
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    println!("render adapter: {:?}", gpu.adapter.get_info());
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    for entry in editor.assets.entries() {
        if let Some(data) = entry.data() {
            bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
        }
    }
    let size = [1280, 800];
    let target = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("factory renderer profile"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let mut samples: [Vec<f64>; 4] = std::array::from_fn(|_| Vec::new());
    let mut draws = Vec::new();
    for i in 0..72 {
        let play = editor.play.as_mut().unwrap();
        play.app.step();
        play.check_simulation()?;
        let mut scene = editor.render(Layer::ThreeD, 1.6)?;
        let ui = editor.ui_frame(Layer::ThreeD, size.map(|v| v as f32))?;
        scene
            .items
            .extend(bozzard_render_assets::widget_items(&ui, &editor.assets)?);
        renderer.draw(&gpu, &target, size, &scene)?;
        gpu.wait()?;
        let s = renderer.frame_stats();
        if i >= 12 {
            for (samples, value) in
                samples
                    .iter_mut()
                    .zip([s.cpu_ms, s.prepare_ms, s.encode_ms, s.submit_ms])
            {
                samples.push(value);
            }
            draws.push(s.shadow_draws);
        }
        if i == 71 {
            println!("render stats: {s:?}");
        }
    }
    for (name, mut values) in ["total", "prepare", "encode", "submit"]
        .into_iter()
        .zip(samples)
    {
        values.sort_by(f64::total_cmp);
        println!(
            "renderer CPU {name}: median={:.2}ms p95={:.2}ms",
            values[30], values[57]
        );
    }
    draws.sort();
    println!("shadow draw commands median={}", draws[30]);
    Ok(())
}
