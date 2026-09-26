//! Compare the two native frame pipelines on a real graphics adapter.
use bozzard_editor::Editor;
use bozzard_render::{Gpu, SceneRenderer, wgpu};
use bozzard_scene::{
    GameplayInput, Layer,
    blueprint::{BlackboardValue, Value},
    keys,
};

#[test]
#[ignore = "requires a native graphics adapter"]
fn simulation_worker_preserves_rendered_world() -> anyhow::Result<()> {
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
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    let mut reference = Vec::new();
    for threaded in [false, true] {
        let mut editor = Editor::new(scene.clone(), &path)?;
        editor.start_play()?;
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        for entry in editor.assets.entries() {
            if let Some(data) = entry.data() {
                bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
            }
        }
        let mut captures = 0;
        for frame in 0..80 {
            let held = if frame < 55 && frame % 2 == 0 {
                keys::bit("D")
            } else if frame == 56 {
                keys::bit("Ctrl") | keys::bit("R")
            } else {
                0
            };
            editor
                .play
                .as_mut()
                .unwrap()
                .set_gameplay_input(GameplayInput {
                    keys: held,
                    ..Default::default()
                });
            editor.prepare_simulation_frame(
                std::time::Duration::from_secs_f64(1.0 / 60.0),
                threaded,
            )?;
            if [1, 30, 55, 79].contains(&frame) {
                let size = if frame == 79 { [1280, 600] } else { [900, 700] };
                // HUD timing text intentionally differs. Compare actual world pixels,
                // including camera motion, sunlight, prefab spawning and shadows.
                let render = editor.render(Layer::ThreeD, size[0] as f32 / size[1] as f32)?;
                let capture =
                    bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
                        editor
                            .render_with_simulation(|| renderer.draw(&gpu, target, size, &render))?
                    })?;
                if threaded {
                    assert!(
                        reference[captures] == capture.rgba,
                        "world pixels changed at frame {frame}"
                    );
                } else {
                    reference.push(capture.rgba);
                }
                captures += 1;
            }
            editor.finish_simulation_frame()?;
        }
        assert_eq!(captures, 4);
        assert_eq!(editor.play.as_ref().unwrap().app.ticks(), 80);
        editor.stop_play();
    }
    Ok(())
}
