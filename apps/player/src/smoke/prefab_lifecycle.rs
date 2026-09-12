use super::*;

#[test]
#[ignore = "hardware GPU soak; cargo test -p bozzard-player bonfire_gpu_soak -- --ignored --nocapture"]
fn bonfire_gpu_soak() -> Result<()> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/bonfire-lab.json");
    let document = load_document(Some(&path))?;
    let static_draws = document
        .objects
        .iter()
        .filter(|o| o.drawable.is_some())
        .count();
    let mut demo = SceneDemo::new_with_prefabs(&document, Some(&path))?;
    let instance = instance(bozzard_render::Backend::native());
    let gpu = pollster::block_on(Gpu::request(&instance, None, false))?;
    gpu.require_hardware()?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut assets = assets::Assets::load(demo.instance().document(), Some(&path))?;
    assets.upload(&gpu, &mut renderer)?;
    let mut baseline = None;
    // One minute of simulation, sampled once a second at low resolution. No window.
    for tick in 1..=3600 {
        demo.app.step();
        demo.check_simulation()?;
        if tick % 60 != 0 {
            continue;
        }
        let count = demo.instance().document().prefabs.len();
        ensure!(count <= 16, "ember population grew: {count}");
        let scene = extract(&demo, assets.store(), Layer::ThreeD, 1.6)?;
        let _frame = capture_display(&gpu, &mut renderer, &scene, [320, 200])?;
        ensure!(
            renderer.frame_stats().surfaces == static_draws + count,
            "dead ember retained by renderer"
        );
        gpu.wait()?;
        let report = instance
            .generate_report()
            .context("GPU resource reporting unavailable")?;
        let resources = [
            report.hub.buffers.num_allocated,
            report.hub.textures.num_allocated,
            report.hub.bind_groups.num_allocated,
        ];
        if tick == 300 {
            baseline = Some(resources);
        }
        if let Some(baseline) = baseline {
            ensure!(
                resources
                    .iter()
                    .zip(baseline)
                    .all(|(n, base)| *n <= base + 4),
                "GPU resources accumulating: baseline={baseline:?}, current={resources:?}"
            );
        }
        if tick % 600 == 0 {
            println!(
                "bonfire_gpu_soak ticks={tick} embers={count} buffers/textures/bind_groups={resources:?}"
            );
        }
    }
    demo.set_gameplay_input(bozzard_scene::GameplayInput {
        jump: true,
        ..Default::default()
    });
    for _ in 0..180 {
        demo.app.step();
        demo.check_simulation()?;
    }
    ensure!(
        demo.instance().document().prefabs.is_empty(),
        "embers did not drain"
    );
    let scene = extract(&demo, assets.store(), Layer::ThreeD, 1.6)?;
    let _frame = capture_display(&gpu, &mut renderer, &scene, [320, 200])?;
    ensure!(
        renderer.frame_stats().surfaces == static_draws,
        "destroyed embers still drawn"
    );
    println!("bonfire_gpu_soak_ok bounded_entities_and_gpu_resources drain_removed_draws");
    Ok(())
}
