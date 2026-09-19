use super::*;

fn scene() -> RenderScene {
    let material = Material {
        metallic: None,
        roughness: None,
        surface_overrides: Default::default(),
        tint: [0.3, 0.65, 0.8],
        uv_scale: [1.; 2],
        texture: TextureKind::White,
        lit: true,
        shader: None,
    };
    let mut items = vec![DrawItem {
        motion_id: 1,
        model: Mat4::from_translation(Vec3::new(0., 0., -3.))
            * Mat4::from_scale(Vec3::new(4., 4., 1.)),
        mesh: MeshKind::Quad,
        material: material.clone(),
    }];
    for y in 0..32 {
        for x in 0..32 {
            items.push(DrawItem {
                motion_id: items.len() as u64 + 1,
                model: Mat4::from_translation(Vec3::new(
                    (x as f32 - 15.5) * 0.15,
                    (y as f32 - 15.5) * 0.15,
                    -12.,
                )) * Mat4::from_scale(Vec3::splat(0.12)),
                mesh: MeshKind::Sphere,
                material: material.clone(),
            });
        }
    }
    RenderScene {
        skin_poses: Default::default(),
        particles: Vec::new(),
        fog: Default::default(),
        gi: None,
        lights: Vec::new(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: bozzard_render::Lighting {
            shadows: false,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 100.),
        items,
        shader_time: 0.,
    }
}
fn compare(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    scene: &RenderScene,
    size: [u32; 2],
    label: &str,
) -> Result<()> {
    renderer.set_occlusion_enabled(false);
    let reference = capture(gpu, renderer, scene, size)?;
    renderer.set_occlusion_enabled(true);
    let optimized = capture(gpu, renderer, scene, size)?;
    ensure!(
        reference.rgba == optimized.rgba,
        "occlusion changed pixels: {label}"
    );
    let cached = capture(gpu, renderer, scene, size)?;
    ensure!(
        reference.rgba == cached.rgba,
        "cached occlusion changed pixels: {label}"
    );
    gpu.wait()?;
    Ok(())
}
pub(super) fn checks(gpu: &Gpu) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    compare(gpu, &mut renderer, &scene, [160, 144], "opaque wall")?;
    capture(gpu, &mut renderer, &scene, [160, 144])?;
    ensure!(
        renderer.frame_stats().occlusion_cache_hit,
        "unchanged visibility was not reused"
    );
    ensure!(
        renderer.frame_stats().color_draws == 1,
        "cached visibility did not omit hidden commands"
    );
    ensure!(
        renderer.frame_stats().occlusion_depth_draws == 0,
        "cached visibility repeated depth work"
    );
    let savings = renderer
        .occlusion_result()
        .context("occlusion diagnostic readback missing")?;
    ensure!(
        savings.culled_surfaces >= 900 && savings.skipped_triangles > 1_000_000,
        "occlusion failed to skip hidden geometry: {savings:?}"
    );
    // Resize and non-power-of-two depth tiles must include every real pixel and
    // treat padded/background pixels conservatively.
    compare(gpu, &mut renderer, &scene, [157, 139], "partial edge tiles")?;
    let wall = scene.items[0].model;
    for x in [1.3, 9., -1.3, 0.] {
        scene.items[0].model = Mat4::from_translation(Vec3::new(x, 0., 0.)) * wall;
        compare(gpu, &mut renderer, &scene, [157, 139], "moving occluder")?;
    }
    let camera = scene.view_projection;
    for angle in [-0.1, 0.1, 0.] {
        scene.view_projection = camera * Mat4::from_rotation_y(angle);
        compare(gpu, &mut renderer, &scene, [157, 139], "moving camera")?;
    }
    scene.items[1].model =
        Mat4::from_translation(Vec3::new(0., 0., -0.12)) * Mat4::from_scale(Vec3::splat(0.2));
    compare(
        gpu,
        &mut renderer,
        &scene,
        [157, 139],
        "near-plane crossing",
    )?;
    scene.items[1].model = Mat4::from_translation(Vec3::new(0., 0., -2.))
        * Mat4::from_scale(Vec3::new(-0.4, 0.4, 0.4));
    compare(
        gpu,
        &mut renderer,
        &scene,
        [157, 139],
        "visible mirrored foreground",
    )?;
    // A large alpha-masked surface must never fill its holes in the depth prepass.
    scene.items[1].model = Mat4::from_translation(Vec3::new(-2.325, -2.325, -12.))
        * Mat4::from_scale(Vec3::splat(0.12));
    let vertices = [
        [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
        [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
        [0.5, 0.5, 0., 0., 0., 1., 1., 0.],
        [-0.5, 0.5, 0., 0., 0., 1., 0., 0.],
    ];
    let cutout = [
        255, 255, 255, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0,
    ];
    renderer.upload_model(
        gpu,
        "occlusion-cutout",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[bozzard_render::ModelPart {
            source_key: "",
            start: 0,
            count: 6,
            color: [1.; 4],
            alpha_cutoff: Some(0.5),
            image: Some(bozzard_render::ModelImage {
                width: 2,
                height: 2,
                rgba: &cutout,
            }),
            shading: None,
        }],
    )?;
    scene.items[0].mesh = MeshKind::Imported("occlusion-cutout".into());
    compare(gpu, &mut renderer, &scene, [157, 139], "alpha holes")?;
    ensure!(
        renderer.frame_stats().occlusion_depth_draws == 0,
        "cutout used as opaque occluder"
    );
    // Replace geometry under the same asset identity with the same bounds and
    // index count. Half the wall disappears: cached depth must be invalidated.
    for (label, indices) in [
        ("replacement opaque wall", [0, 1, 2, 0, 2, 3]),
        ("replacement half wall", [0, 1, 2, 0, 1, 2]),
    ] {
        renderer.upload_model(
            gpu,
            "occlusion-cutout",
            &vertices,
            &indices,
            &[bozzard_render::ModelPart {
                source_key: "",
                start: 0,
                count: 6,
                color: [1.; 4],
                alpha_cutoff: None,
                image: None,
                shading: None,
            }],
        )?;
        compare(gpu, &mut renderer, &scene, [157, 139], label)?;
    }
    renderer.set_state_caching_enabled(false);
    compare(
        gpu,
        &mut renderer,
        &scene,
        [157, 139],
        "state caching disabled",
    )?;
    ensure!(
        !renderer.frame_stats().occlusion_cache_hit,
        "reference switch reused visibility"
    );
    println!(
        "occlusion_gpu_ok exact_reference_pixels cached_visibility movement resize near_plane mirrored alpha_holes asset_replacement hidden_surfaces={} skipped_triangles={}",
        savings.culled_surfaces, savings.skipped_triangles
    );
    Ok(())
}

#[cfg(test)]
#[test]
#[ignore = "native GPU verification; run explicitly on a graphics host"]
fn native_occlusion() -> Result<()> {
    let instance = instance(bozzard_render::Backend::native());
    let gpu = pollster::block_on(Gpu::request(&instance, None, false))?;
    gpu.require_hardware()?;
    checks(&gpu)?;
    let mut scene = scene();
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let target = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("occlusion benchmark"),
            size: wgpu::Extent3d {
                width: 320,
                height: 320,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    for (label, mesh, moving, covered) in [
        ("spheres_static", MeshKind::Sphere, false, true),
        ("spheres_moving", MeshKind::Sphere, true, true),
        ("cubes", MeshKind::Cube, false, true),
        ("open_spheres", MeshKind::Sphere, false, false),
    ] {
        for item in &mut scene.items[1..] {
            item.mesh = mesh.clone();
        }
        scene.items[0].model =
            Mat4::from_translation(Vec3::new(if covered { 0. } else { 9. }, 0., -3.))
                * Mat4::from_scale(Vec3::new(4., 4., 1.));
        let mut wall = [Vec::new(), Vec::new()];
        let mut cpu = wall.clone();
        for frame in 0..25 {
            if moving {
                scene.items[0].model =
                    Mat4::from_translation(Vec3::new((frame as f32 * 0.1).sin() * 0.1, 0., -3.))
                        * Mat4::from_scale(Vec3::new(4., 4., 1.));
            }
            for enabled in [false, true] {
                renderer.set_occlusion_enabled(enabled);
                let start = Instant::now();
                renderer.draw_linear(&gpu, &target, [320, 320], &scene)?;
                gpu.wait()?;
                if frame >= 3 {
                    wall[usize::from(enabled)].push(start.elapsed().as_secs_f64() * 1000.);
                    cpu[usize::from(enabled)].push(renderer.frame_stats().cpu_ms);
                }
            }
        }
        let median = |values: &mut Vec<f64>| {
            values.sort_by(f64::total_cmp);
            values[values.len() / 2]
        };
        let stats = renderer.frame_stats();
        println!(
            "occlusion_benchmark case={label} reference_wall_ms={:.3} culled_wall_ms={:.3} reference_cpu_ms={:.3} culled_cpu_ms={:.3} candidates={} cached={} commands={} last_gpu_result={:?}",
            median(&mut wall[0]),
            median(&mut wall[1]),
            median(&mut cpu[0]),
            median(&mut cpu[1]),
            stats.occlusion_candidates,
            stats.occlusion_cache_hit,
            stats.color_draws,
            (stats.occlusion_candidates > 0)
                .then(|| renderer.occlusion_result())
                .flatten()
        );
    }
    Ok(())
}
