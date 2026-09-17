use anyhow::Result;
use bozzard_editor::Editor;
use bozzard_render::{Backend, Gpu, SceneRenderer, TextureKind, capture_offscreen, instance, wgpu};
use bozzard_render_assets::ComputeBridge;
use bozzard_scene::Layer;
use std::path::Path;

fn open(name: &str) -> Result<Editor> {
    Editor::open(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../examples/demo/scenes/{name}.json")),
    )
}
#[test]
fn authored_compute_examples_render_reuse_pause_restart_and_read_back() -> Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut bridge = ComputeBridge::new(&gpu);
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut editor = open("compute-waves")?;
    let authored = editor.scene().clone();
    editor.assets.require_ready()?;
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    play.with_instance(|instance, _| bridge.prepare(instance));
    play.app.step();
    play.check_simulation()?;
    assert!(bridge.submit(&gpu, play.instance())?);
    bridge.sync_renderer(&mut renderer);
    let first_scene = editor.render(Layer::ThreeD, 2.)?;
    assert!(
        first_scene
            .items
            .iter()
            .any(|item| matches!(item.material.texture, TextureKind::Generated(_)))
    );
    let first = capture_offscreen(&gpu, 256, 128, |target| {
        renderer.draw(&gpu, target, [256, 128], &first_scene)
    })?;
    assert!(
        first
            .rgba
            .chunks_exact(4)
            .filter(|p| p[2] > p[0].saturating_add(15))
            .count()
            > 5000,
        "the authored material displays the blue GPU texture"
    );
    // Generated base color also flows through the shader graph's existing Texture Sample node
    // and an imported model surface, without a second texture namespace in either shader path.
    use bozzard_scene::shader_graph::{Node, NodeKind, ShaderGraph, Socket, Wire};
    let mut graph = ShaderGraph::default();
    graph.nodes.extend([
        Node::new(2, NodeKind::UV, [0.; 2]),
        Node::new(3, NodeKind::TextureSample, [0.; 2]),
    ]);
    graph.connect(Wire {
        from: Socket { node: 2, port: 0 },
        to: Socket { node: 3, port: 0 },
    })?;
    graph.connect(Wire {
        from: Socket { node: 3, port: 0 },
        to: Socket { node: 1, port: 0 },
    })?;
    let mut graphed = first_scene.clone();
    graphed.items[0].material.shader = Some(bozzard_render_assets::shader_source(&graph)?);
    let sampled = capture_offscreen(&gpu, 256, 128, |target| {
        renderer.draw(&gpu, target, [256, 128], &graphed)
    })?;
    assert!(
        sampled
            .rgba
            .chunks_exact(4)
            .filter(|p| p[2] > p[0].saturating_add(15))
            .count()
            > 5000
    );
    renderer.upload_model(
        &gpu,
        "compute-model",
        &[
            [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
            [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
            [0.5, 0.5, 0., 0., 0., 1., 1., 0.],
            [-0.5, 0.5, 0., 0., 0., 1., 0., 0.],
        ],
        &[0, 1, 2, 0, 2, 3],
        &[bozzard_render::ModelPart {
            source_key: "test",
            shading: None,
            start: 0,
            count: 6,
            color: [1.; 4],
            alpha_cutoff: None,
            image: None,
        }],
    )?;
    graphed.items[0].mesh = bozzard_render::MeshKind::Imported("compute-model".into());
    let imported = capture_offscreen(&gpu, 256, 128, |target| {
        renderer.draw(&gpu, target, [256, 128], &graphed)
    })?;
    assert!(
        imported
            .rgba
            .chunks_exact(4)
            .filter(|p| p[2] > p[0].saturating_add(15))
            .count()
            > 5000
    );
    bridge.poll(&gpu)?;
    let warm = bridge.executor.statistics();
    let play = editor.play.as_mut().unwrap();
    for _ in 0..20 {
        play.app.step();
        play.check_simulation()?;
    }
    assert!(bridge.submit(&gpu, play.instance())?);
    assert!(
        !bridge.submit(&gpu, play.instance())?,
        "multiple viewports consume a shared world once"
    );
    bridge.sync_renderer(&mut renderer);
    let next_scene = editor.render(Layer::ThreeD, 2.)?;
    let next = capture_offscreen(&gpu, 256, 128, |target| {
        renderer.draw(&gpu, target, [256, 128], &next_scene)
    })?;
    assert!(
        first
            .rgba
            .chunks_exact(4)
            .zip(next.rgba.chunks_exact(4))
            .filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 3))
            .count()
            > 1000,
        "later script ticks update rendered pixels without CPU readback"
    );
    let after = bridge.executor.statistics();
    assert_eq!(after.pipeline_compilations, warm.pipeline_compilations);
    assert_eq!(after.bind_group_creations, warm.bind_group_creations);
    assert_eq!(after.resource_creations, warm.resource_creations);
    // No tick means no new dispatch, whether the editor repaints or a surface is zero-sized.
    let paused = after.submissions;
    for _ in 0..3 {
        bridge.poll(&gpu)?;
        assert!(!bridge.submit(&gpu, editor.play.as_ref().unwrap().instance())?);
    }
    assert_eq!(bridge.executor.statistics().submissions, paused);
    editor.stop_play();
    bridge.stop();
    bridge.sync_renderer(&mut renderer);
    assert_eq!(editor.scene(), &authored);
    assert_eq!(bridge.executor.statistics().allocations, 0);

    let mut editor = open("compute-numbers")?;
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    play.with_instance(|instance, _| bridge.prepare(instance));
    play.app.step();
    play.check_simulation()?;
    assert!(bridge.submit(&gpu, play.instance())?);
    gpu.wait()?;
    bridge.poll(&gpu)?;
    play.app.step();
    play.check_simulation()?;
    let messages: Vec<_> = play
        .app
        .world
        .resource::<bozzard_scene::ScriptRuntime>()
        .unwrap()
        .messages()
        .collect();
    assert!(
        messages
            .iter()
            .any(|line| line.contains("Compute result:") && line.contains("31")),
        "{messages:?}"
    );
    assert_eq!(
        play.instance()
            .compute()
            .runtime
            .statistics()
            .pending_readbacks,
        0
    );
    Ok(())
}

#[test]
fn hot_reload_retains_working_sources_and_requires_recreation_for_changed_interfaces() -> Result<()>
{
    struct Files(std::path::PathBuf);
    impl Drop for Files {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let files = Files(std::env::temp_dir().join(format!(
            "bozzard-compute-reload-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        )));
    std::fs::create_dir_all(files.0.join("assets/compute"))?;
    let path = files.0.join("scene.json");
    let shader = files.0.join("assets/compute/waves.compute.wgsl");
    let original = include_str!("../../../examples/demo/scenes/assets/compute/waves.compute.wgsl");
    std::fs::write(
        &path,
        include_str!("../../../examples/demo/scenes/compute-waves.json"),
    )?;
    std::fs::write(&shader, original)?;
    std::fs::write(
        files.0.join("assets/compute/waves.rs"),
        include_str!("../../../examples/demo/scenes/assets/compute/waves.rs"),
    )?;
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut bridge = ComputeBridge::new(&gpu);
    let mut editor = Editor::open(&path)?;
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    play.with_instance(|instance, _| {
        bridge.prepare(instance);
        bridge.refresh(&gpu, instance, &editor.assets)
    })?;
    let old = play.instance().compute_kernels()["waves"].id();
    play.app.step();
    play.check_simulation()?;
    let changed = original.replace("0.08, 0.62, 0.68", "0.8, 0.12, 0.08");
    std::fs::write(&shader, &changed)?;
    editor.assets.refresh();
    editor.assets.require_ready()?;
    play.with_instance(|instance, _| bridge.refresh(&gpu, instance, &editor.assets))?;
    let working = play.instance().compute_kernels()["waves"].id();
    assert_ne!(old, working);
    play.app.step();
    play.check_simulation()?;
    let mut revisions = Vec::new();
    play.instance().compute().runtime.submit_with(|batch, _| {
        for request in batch.requests {
            if let bozzard_scene::compute::Command::Dispatch { kernel, .. } = &request.command {
                revisions.push(kernel.id());
            }
        }
        bozzard_scene::compute::Submission::Deferred
    })?;
    assert_eq!(
        revisions,
        [old, working],
        "accepted work pins its original source revision"
    );
    assert!(bridge.submit(&gpu, play.instance())?);
    gpu.wait()?;
    bridge.poll(&gpu)?;

    std::fs::write(&shader, "@compute fn broken this is not WGSL")?;
    editor.assets.refresh();
    assert!(editor.assets.require_ready().is_err());
    play.with_instance(|instance, _| bridge.refresh(&gpu, instance, &editor.assets))?;
    assert_eq!(play.instance().compute_kernels()["waves"].id(), working);
    assert!(bridge.diagnostics().contains_key("waves"));
    play.app.step();
    play.check_simulation()?;

    std::fs::write(
        &shader,
        changed.replace("amplitude: f32", "amplitude: f32, extra: f32"),
    )?;
    editor.assets.refresh();
    editor.assets.require_ready()?;
    play.with_instance(|instance, _| bridge.refresh(&gpu, instance, &editor.assets))?;
    assert_eq!(play.instance().compute_kernels()["waves"].id(), working);
    assert!(bridge.diagnostics()["waves"].contains("interface changed"));
    play.app.step();
    play.check_simulation()?;

    // CPU-valid WGSL can still exceed the limits actually enabled on the graphics device.
    let oversized = changed.replace("workgroup_size(8, 8)", "workgroup_size(1024, 8)");
    std::fs::write(&shader, oversized)?;
    editor.assets.refresh();
    editor.assets.require_ready()?;
    play.with_instance(|instance, _| bridge.refresh(&gpu, instance, &editor.assets))?;
    assert_eq!(play.instance().compute_kernels()["waves"].id(), working);
    assert!(bridge.diagnostics()["waves"].contains("enabled device limits"));
    let compilations = bridge.executor.statistics().pipeline_compilations;
    play.with_instance(|instance, _| bridge.refresh(&gpu, instance, &editor.assets))?;
    assert_eq!(
        bridge.executor.statistics().pipeline_compilations,
        compilations,
        "a failed revision is not retried every frame"
    );
    std::fs::write(&shader, original)?;
    editor.assets.refresh();
    editor.assets.require_ready()?;
    play.with_instance(|instance, _| bridge.refresh(&gpu, instance, &editor.assets))?;
    assert!(bridge.diagnostics().is_empty());
    assert_ne!(play.instance().compute_kernels()["waves"].id(), working);
    assert!(bridge.submit(&gpu, play.instance())?);
    gpu.wait()?;
    bridge.poll(&gpu)?;
    Ok(())
}
