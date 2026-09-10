use super::*;

pub(super) fn run(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    scene: &RenderScene,
    size: [u32; 2],
    frames: u32,
) -> Result<()> {
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frame benchmark"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let modes = [
        ("reference", false, false),
        ("culling", true, false),
        ("optimized", true, true),
    ];
    let mut reference = None;
    for (_, cull, cache) in modes {
        renderer.set_culling_enabled(cull);
        renderer.set_state_caching_enabled(cache);
        let frame = capture_display(gpu, renderer, scene, size)?;
        if let Some(pixels) = &reference {
            ensure!(
                *pixels == frame.rgba,
                "benchmark optimization changed pixels"
            );
        } else {
            reference = Some(frame.rgba);
        }
    }
    let mut cpu = [Vec::new(), Vec::new(), Vec::new()];
    let mut wall = cpu.clone();
    let mut stats = [bozzard_render::FrameStats::default(); 3];
    // Interleave configurations to reduce warm-up/thermal bias. Synchronize each frame;
    // wall time includes CPU+GPU+wait overhead, not a claim of windowed FPS/GPU timestamps.
    for iteration in 0..frames + 3 {
        for (i, (_, cull, cache)) in modes.iter().enumerate() {
            renderer.set_culling_enabled(*cull);
            renderer.set_state_caching_enabled(*cache);
            let started = Instant::now();
            renderer.draw(gpu, &view, size, scene)?;
            gpu.wait()?;
            stats[i] = renderer.frame_stats();
            if iteration >= 3 {
                cpu[i].push(stats[i].cpu_ms);
                wall[i].push(started.elapsed().as_secs_f64() * 1000.);
            }
        }
    }
    let median = |values: &mut Vec<f64>| {
        values.sort_by(f64::total_cmp);
        values[values.len() / 2]
    };
    for (i, (name, _, _)) in modes.iter().enumerate() {
        println!(
            "frame_benchmark mode={name} frames={frames} size={}x{} cpu_median_ms={:.3} synchronized_wall_median_ms={:.3} surfaces={} visible={} culled={} triangles={} shadow_draws={} shadow_triangles={} pipeline_binds={} exact_pixels=true",
            size[0],
            size[1],
            median(&mut cpu[i]),
            median(&mut wall[i]),
            stats[i].surfaces,
            stats[i].visible_surfaces,
            stats[i].culled_surfaces,
            stats[i].color_triangles,
            stats[i].shadow_draws,
            stats[i].shadow_triangles,
            stats[i].pipeline_binds
        );
    }
    renderer.set_culling_enabled(true);
    renderer.set_state_caching_enabled(true);
    Ok(())
}
