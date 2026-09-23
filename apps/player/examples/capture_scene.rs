//! Capture a scene using the player renderer, including a bloom-off comparison.
use anyhow::{Context, Result, ensure};
use bozzard_assets::AssetStore;
use bozzard_demo::SceneDemo;
use bozzard_render::{Backend, Gpu, SceneRenderer, capture_offscreen, instance, wgpu};
use bozzard_scene::{Layer, Scene};
use std::path::PathBuf;

#[path = "../src/presentation.rs"]
mod presentation;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(
        args.next()
            .context("usage: capture_scene SCENE OUTPUT_DIR")?,
    );
    let out = PathBuf::from(args.next().context("missing output directory")?);
    std::fs::create_dir_all(&out)?;
    let document = Scene::from_json(&std::fs::read_to_string(&path)?)?;
    let mut assets = AssetStore::new(path.parent().unwrap(), &document.assets)?;
    assets.load_pending()?;
    assets.require_ready()?;
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    for id in document.assets.keys() {
        let data = assets
            .get(assets.handle(id).unwrap())
            .unwrap()
            .shared_data()
            .context("asset not ready")?;
        bozzard_render_assets::upload(&gpu, &mut renderer, id, &data)?;
    }
    let demo = SceneDemo::new_with_prefabs(&document, Some(&path))?;
    let mut scene = presentation::extract(&demo, &assets, Layer::ThreeD, 1.25)?;
    let size = [1200, 960];
    let with = capture_offscreen(&gpu, size[0], size[1], |view| {
        renderer.draw(&gpu, view, size, &scene)
    })?;
    with.write_ppm(&out.join("room-bloom.ppm"))?;
    scene.display.bloom.enabled = false;
    renderer.reset_display_history();
    let without = capture_offscreen(&gpu, size[0], size[1], |view| {
        renderer.draw(&gpu, view, size, &scene)
    })?;
    without.write_ppm(&out.join("room-no-bloom.ppm"))?;
    let changed = with
        .rgba
        .chunks_exact(4)
        .zip(without.rgba.chunks_exact(4))
        .filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > 3))
        .count();
    if document.display.bloom.enabled {
        ensure!(
            changed > 100,
            "authored bloom did not produce a visible halo"
        );
    }
    println!(
        "scene_capture_ok objects={} bloom_changed_pixels={changed}",
        document.objects.len()
    );
    Ok(())
}
