//! Capture an authored scene through the editor's production renderer.
use anyhow::{Context, Result, ensure};
use bozzard_render::{Gpu, SceneRenderer, wgpu};
use bozzard_scene::Layer;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .context("usage: capture_scene SCENE OUTPUT.ppm [WIDTH HEIGHT]")?;
    let output = args.next().context("missing output path")?;
    let width: u32 = args.next().map(|v| v.parse()).transpose()?.unwrap_or(1600);
    let height: u32 = args.next().map(|v| v.parse()).transpose()?.unwrap_or(1000);
    ensure!(
        (1..=4096).contains(&width) && (1..=4096).contains(&height),
        "invalid capture size"
    );
    let editor = bozzard_editor::Editor::open(std::path::Path::new(&path))?;
    editor.assets.require_ready()?;
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
    let scene = editor.render(Layer::ThreeD, width as f32 / height as f32)?;
    let frame = bozzard_render::capture_offscreen(&gpu, width, height, |target| {
        renderer.draw(&gpu, target, [width, height], &scene)
    })?;
    frame.write_ppm(std::path::Path::new(&output))?;
    println!(
        "Captured {} objects at {width}x{height}: {output}",
        editor.scene().objects.len()
    );
    Ok(())
}
