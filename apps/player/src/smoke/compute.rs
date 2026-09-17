//! Packaged compute scenes use the same presentation bridge, without a surface/window.
use super::*;
use bozzard_render_assets::ComputeBridge;

pub(super) fn check_document(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    document: &Scene,
    assets: &bozzard_assets::AssetStore,
    options: &Options,
) -> Result<()> {
    let mut demo = SceneDemo::new_with_prefabs(document, options.scene.as_deref())?;
    let mut bridge = ComputeBridge::new(gpu);
    demo.with_instance(|instance, _| bridge.prepare(instance));
    let layer = if demo.instance().has_view(Layer::ThreeD) {
        Layer::ThreeD
    } else {
        Layer::TwoD
    };
    let mut first = None;
    for tick in 0..120 {
        bridge.poll(gpu)?;
        demo.app.step();
        demo.check_simulation()?;
        bridge.submit(gpu, demo.instance())?;
        // Smoke verification may wait; production presentation only polls asynchronously.
        gpu.wait()?;
        bridge.poll(gpu)?;
        bridge.sync_renderer(renderer);
        if tick == 0 {
            first = Some(capture_display(
                gpu,
                renderer,
                &extract(&demo, assets, layer, 2.)?,
                [512, 256],
            )?);
        }
    }
    demo.app.step();
    demo.check_simulation()?;
    let final_scene = extract(&demo, assets, layer, 2.)?;
    let last = capture_display(gpu, renderer, &final_scene, [512, 256])?;
    let first = first.unwrap();
    first.write_ppm(&options.output.join("compute-initial.ppm"))?;
    last.write_ppm(&options.output.join("compute-final.ppm"))?;
    if document.name.starts_with("Compute Waves") {
        ensure!(
            final_scene
                .items
                .iter()
                .any(|item| matches!(item.material.texture, TextureKind::Generated(_))),
            "compute material missing"
        );
        ensure!(
            last.rgba
                .chunks_exact(4)
                .filter(|p| p[2] > p[0].saturating_add(15))
                .count()
                > 5000,
            "compute texture did not reach the material"
        );
        ensure!(first.rgba != last.rgba, "compute texture did not animate");
    }
    if document.name.starts_with("Compute Numbers") {
        let messages = demo
            .app
            .world
            .resource::<bozzard_scene::ScriptRuntime>()
            .context("script runtime missing")?;
        ensure!(
            messages
                .messages()
                .any(|line| line.contains("[7.0, 13.0, 19.0, 25.0, 31.0]")),
            "numeric compute result did not arrive"
        );
    }
    if let Some(state) = demo.instance().compute_if_initialized() {
        for job in state.runtime.jobs() {
            ensure!(
                !matches!(job.state, bozzard_scene::compute::JobState::Failed(_)),
                "compute job {}: {:?}",
                job.label,
                job.state
            );
        }
    }
    bridge.stop();
    bridge.sync_renderer(renderer);
    println!(
        "compute_scene_ok scene={:?} submissions={} pipelines={} resources_after_stop={}",
        document.name,
        bridge.executor.statistics().submissions,
        bridge.executor.statistics().pipeline_compilations,
        bridge.executor.statistics().allocations
    );
    Ok(())
}
