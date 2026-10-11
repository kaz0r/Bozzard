//! `--capture FILE`: run a scene for a number of fixed ticks without opening a window, then
//! render the next frame offscreen, HUD and UI widgets included, and write it as a binary PPM.
//! Useful for screenshots and visual checks on machines where a window would be in the way.
use super::*;
use bozzard_render::capture_offscreen;

pub(crate) fn run(document: &Scene, options: &Options, path: &Path) -> Result<()> {
    let (mut demo, mut assets) = start_session(document, options)?;
    settings::load(&mut demo, options);
    demo.set_threaded_simulation(false)?;
    for _ in 0..options.capture_ticks {
        demo.app.step();
        demo.check_simulation()?;
    }
    let gpu = pollster::block_on(Gpu::request(
        &instance(options.backend),
        None,
        options.software,
    ))?;
    if options.hardware {
        gpu.require_hardware()?;
    }
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    renderer.set_hud_scale(1.);
    let [width, height] = options.capture_size;
    let layer = options.layer;
    let cache = bozzard_render_assets::RenderSceneCache::default();
    let mut scene = presentation::extract_frame(
        &demo,
        assets.store(),
        &cache,
        layer,
        width as f32 / height as f32,
    )?;
    settings::apply_quality_to_frame(settings::applied(&demo).0.quality, &mut scene);
    let ui = demo
        .instance()
        .ui_frame(&demo.app.world, layer, [width as f32, height as f32])?;
    scene.append_items(bozzard_render_assets::widget_items(&ui, assets.store())?);
    assets.upload_required(&gpu, &mut renderer, &scene, options.gpu_memory_mib << 20)?;
    let frame = capture_offscreen(&gpu, width, height, |view| {
        renderer.draw(&gpu, view, [width, height], &scene)
    })?;
    frame.write_ppm(path)?;
    println!(
        "capture_ok path={} ticks={} size={width}x{height}",
        path.display(),
        options.capture_ticks
    );
    Ok(())
}
