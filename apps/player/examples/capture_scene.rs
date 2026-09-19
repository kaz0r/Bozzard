//! Capture the real player renderer without editor chrome or image processing.
use anyhow::{Context, Result, ensure};
use bozzard_assets::AssetStore;
use bozzard_demo::{SceneDemo, load_document};
use bozzard_render::{Backend, Gpu, SceneRenderer, capture_offscreen, instance, wgpu};
use bozzard_scene::Layer;
use std::path::{Path, PathBuf};

#[path = "../src/presentation.rs"]
mod presentation;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: capture_scene SCENE.json OUTPUT.ppm [WIDTH HEIGHT TICKS CAMERA]")?,
    );
    let output = PathBuf::from(args.next().context("missing output PPM path")?);
    let width = args.next().map(|n| n.parse()).transpose()?.unwrap_or(1600);
    let height = args.next().map(|n| n.parse()).transpose()?.unwrap_or(1000);
    let ticks: u32 = args.next().map(|n| n.parse()).transpose()?.unwrap_or(0);
    let camera = args.next();
    ensure!(args.next().is_none(), "unexpected argument");
    ensure!(
        (1..=4096).contains(&width) && (1..=4096).contains(&height),
        "dimensions must be in 1..4096"
    );
    ensure!(
        ticks <= 36_000,
        "capture supports at most ten minutes of simulation"
    );
    let mut document = load_document(Some(&source))?;
    if let Some(camera) = camera {
        document.views.insert(Layer::ThreeD, camera);
    }
    let mut demo = SceneDemo::new_with_prefabs(&document, Some(&source))?;
    let root = source
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut assets = AssetStore::new(root, &demo.instance().document().assets)?;
    assets.refresh();
    assets.require_ready()?;
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    bozzard_render_assets::Residency::default().sync(&gpu, &mut renderer, &assets)?;
    for _ in 0..ticks {
        demo.app.step();
        demo.check_simulation()?;
    }
    let scene = presentation::extract(&demo, &assets, Layer::ThreeD, width as f32 / height as f32)?;
    let frame = capture_offscreen(&gpu, width, height, |view| {
        renderer.draw(&gpu, view, [width, height], &scene)
    })?;
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    frame.write_ppm(&output)?;
    println!(
        "capture_ok size={width}x{height} ticks={ticks} draws={} particles={} gi={} output={}",
        scene.items.len(),
        scene.particles.len(),
        scene.gi.is_some(),
        output.display()
    );
    Ok(())
}
