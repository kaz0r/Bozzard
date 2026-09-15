//! Capture both middleware samples using the production extraction and native GPU paths.
use anyhow::Result;
use bozzard_render::{Gpu, SceneRenderer, wgpu};
use bozzard_scene::{Layer, middleware::ui::Input};
fn main() -> Result<()> {
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    for (name, layer) in [
        ("middleware-lab", Layer::ThreeD),
        ("ui-2d-lab", Layer::TwoD),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../examples/demo/scenes/{name}.json"));
        let mut editor = bozzard_editor::Editor::open(&path)?;
        editor.start_play()?;
        let play = editor.play.as_mut().unwrap();
        play.with_instance(|instance, _| instance.set_gpu_particles(true));
        if layer == Layer::TwoD {
            play.ui_input(layer, [1280., 720.], Input::Key("Enter".into()))?;
        }
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        for entry in editor.assets.entries() {
            if let Some(data) = entry.data() {
                bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
            }
        }
        let mut capture = None;
        for _ in 0..120 {
            play.app.step();
            play.check_simulation()?;
            let mut scene = bozzard_editor::extract(play, &editor.assets, layer, 16. / 9.)?;
            let frame = play
                .instance()
                .ui_frame(&play.app.world, layer, [1280., 720.])?;
            scene
                .items
                .extend(bozzard_render_assets::widget_items(&frame, &editor.assets)?);
            capture = Some(bozzard_render::capture_offscreen(
                &gpu,
                1280,
                720,
                |target| renderer.draw(&gpu, target, [1280, 720], &scene),
            )?);
        }
        let output = std::env::temp_dir().join(format!("{name}.ppm"));
        capture.unwrap().write_ppm(&output)?;
        println!("{}", output.display());
    }
    Ok(())
}
