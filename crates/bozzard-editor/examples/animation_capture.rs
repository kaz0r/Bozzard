//! Capture the character lab through production simulation, GPU skinning and HUD rendering.
use anyhow::Result;
use bozzard_render::{Gpu, SceneRenderer, wgpu};
use bozzard_scene::Layer;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let output = std::env::args().nth(1).map_or_else(
        || std::env::temp_dir().join("bozzard-animation-capture"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&output)?;
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/animation-lab.json");
    let gpu = pollster::block_on(Gpu::request(
        &bozzard_render::instance(bozzard_render::Backend::native()),
        None,
        false,
    ))?;
    let mut editor = bozzard_editor::Editor::open(&path)?;
    editor.start_play()?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    for entry in editor.assets.entries() {
        if let Some(data) = entry.data() {
            bozzard_render_assets::upload(&gpu, &mut renderer, &entry.id, data)?;
        }
    }
    let play = editor.play.as_mut().unwrap();
    for frame in 0..120 {
        for _ in 0..3 {
            play.app.step();
            play.check_simulation()?;
        }
        let mut scene = bozzard_editor::extract(play, &editor.assets, Layer::ThreeD, 16. / 9.)?;
        let hud = play
            .instance()
            .ui_frame(&play.app.world, Layer::ThreeD, [1280., 720.])?;
        scene
            .items
            .extend(bozzard_render_assets::widget_items(&hud, &editor.assets)?);
        bozzard_render::capture_offscreen(&gpu, 1280, 720, |target| {
            renderer.draw(&gpu, target, [1280, 720], &scene)
        })?
        .write_ppm(&output.join(format!("frame-{frame:03}.ppm")))?;
    }
    println!(
        "animation_capture_ok frames=120 output={}",
        output.display()
    );
    Ok(())
}
