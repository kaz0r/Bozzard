use bozzard_editor::Editor;
use bozzard_scene::{BlueprintRuntime, Layer, Mesh, blueprint::BlackboardValue};

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
fn capture_earth_factory_ui() -> anyhow::Result<()> {
    use bozzard_render::{Gpu, SceneRenderer, wgpu};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut editor = Editor::open(&path)?;
    let mut scene = editor.scene().clone();
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
