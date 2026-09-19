use super::*;

pub(super) fn checks(gpu: &Gpu) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let item = |position: Vec3| DrawItem {
        motion_id: 0,
        model: Mat4::from_translation(position),
        mesh: MeshKind::Cube,
        material: Material {
            metallic: None,
            roughness: None,
            surface_overrides: Default::default(),
            tint: [0.3, 0.8, 0.7],
            texture: TextureKind::White,
            uv_scale: [1.; 2],
            lit: true,
            shader: None,
        },
    };
    let scene = RenderScene {
        skin_poses: Default::default(),
        shader_time: 0.,
        particles: Vec::new(),
        fog: Default::default(),
        gi: None,
        lights: Vec::new(),
        display: Default::default(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        lighting: Default::default(),
        view_projection: glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 20.),
        items: vec![
            item(Vec3::new(0., 0., -4.)),
            item(Vec3::new(100., 0., -4.)),
            item(Vec3::new(0., 0., 5.)),
            DrawItem {
                motion_id: 0,
                model: Mat4::from_translation(Vec3::new(1.8, 0., -4.))
                    * Mat4::from_rotation_z(0.6)
                    * Mat4::from_scale(Vec3::new(-2., 0.2, 1.)),
                ..item(Vec3::ZERO)
            },
        ],
    };
    renderer.set_culling_enabled(false);
    renderer.set_state_caching_enabled(false);
    renderer.set_instancing_enabled(false);
    let reference = capture(gpu, &mut renderer, &scene, [128, 128])?;
    let all = renderer.frame_stats();
    renderer.set_culling_enabled(true);
    renderer.set_state_caching_enabled(true);
    renderer.set_instancing_enabled(true);
    let optimized = capture(gpu, &mut renderer, &scene, [128, 128])?;
    let reduced = renderer.frame_stats();
    ensure!(
        reference.rgba == optimized.rgba,
        "frustum culling or state caching changed pixels"
    );
    ensure!(
        all.visible_surfaces == 4 && reduced.visible_surfaces == 2 && reduced.culled_surfaces == 2,
        "incorrect surface counters: {all:?} {reduced:?}"
    );
    ensure!(
        reduced.pipeline_binds == 1
            && all.pipeline_binds == 4
            && all.color_draws == 4
            && reduced.color_draws == 2
            && reduced.shadow_draws == all.shadow_draws,
        "batching or shadow counters incorrect: {all:?} {reduced:?}"
    );
    let mut shadow_scene = RenderScene {
        skin_poses: Default::default(),
        shader_time: 0.,
        particles: Vec::new(),
        fog: Default::default(),
        gi: None,
        lights: Vec::new(),
        display: Default::default(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        lighting: bozzard_render::Lighting {
            sun_direction: [3., 0., 1.],
            sun_intensity: 3.,
            ambient_intensity: 0.1,
            shadow_resolution: 512,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.)),
        items: vec![
            DrawItem {
                motion_id: 0,
                model: Mat4::from_scale(Vec3::new(4., 4., 1.)),
                mesh: MeshKind::Quad,
                ..item(Vec3::ZERO)
            },
            DrawItem {
                motion_id: 0,
                model: Mat4::from_translation(Vec3::new(2.5, 0., 1.))
                    * Mat4::from_scale(Vec3::splat(0.6)),
                mesh: MeshKind::Quad,
                ..item(Vec3::ZERO)
            },
        ],
    };
    let with = capture(gpu, &mut renderer, &shadow_scene, [128, 128])?;
    let stats = renderer.frame_stats();
    ensure!(
        stats.visible_surfaces == 1 && stats.shadow_draws == 2,
        "offscreen shadow caster incorrectly culled"
    );
    shadow_scene.lighting.shadows = false;
    let without = capture(gpu, &mut renderer, &shadow_scene, [128, 128])?;
    ensure!(
        with.rgba
            .chunks_exact(4)
            .zip(without.rgba.chunks_exact(4))
            .filter(|(a, b)| a[1] + 20 < b[1])
            .count()
            > 100,
        "offscreen caster did not shadow visible geometry"
    );
    shadow_cache_checks(gpu, &mut renderer, shadow_scene.clone())?;
    auxiliary_target_checks(gpu, shadow_scene)?;
    println!(
        "visibility_gpu_ok exact_reference_pixels mirrored_crossing_bounds offscreen_shadow_casters pipeline_cache counters"
    );
    Ok(())
}

fn auxiliary_target_checks(gpu: &Gpu, mut scene: RenderScene) -> Result<()> {
    let mut optimized = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut reference = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    reference.set_state_caching_enabled(false);
    scene.items[0].motion_id = 1;
    scene.items[1].motion_id = 2;
    let model = scene.items[1].model;
    // Compare independent histories, including toggling effects after their inputs
    // were discarded. A cache populated by the reference must not mask a stale read.
    for mask in [0, 1, 0, 2, 0, 4, 0, 7, 0] {
        scene.display.temporal_aa.enabled = mask & 1 != 0;
        scene.display.motion_blur.enabled = mask & 2 != 0;
        scene.display.reflections.enabled = mask & 4 != 0;
        for step in 0..4 {
            scene.shader_time += 1. / 60.;
            scene.display.time_seconds = scene.shader_time;
            scene.items[1].model =
                model * Mat4::from_translation(Vec3::new(step as f32 * 0.08, 0., 0.));
            let draw = |renderer: &mut SceneRenderer| {
                capture_offscreen(gpu, 128, 128, |view| {
                    renderer.draw(gpu, view, [128, 128], &scene)
                })
            };
            let actual = draw(&mut optimized)?;
            let expected = draw(&mut reference)?;
            ensure!(
                actual.rgba == expected.rgba,
                "auxiliary discard changed pixels, effects={mask}, frame={step}"
            );
            let targets = if mask == 0 {
                0
            } else if mask == 7 {
                3
            } else {
                2
            };
            ensure!(
                optimized.frame_stats().geometry_store_bytes == targets * 128 * 128 * 8,
                "unexpected auxiliary stores"
            );
            ensure!(
                reference.frame_stats().geometry_store_bytes == 3 * 128 * 128 * 8,
                "reference did not preserve all auxiliary targets"
            );
        }
    }
    println!(
        "geometry_store_gpu_ok exact_reference_pixels independent_histories taa_blur_reflection_toggle"
    );
    Ok(())
}

fn shadow_cache_checks(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    mut scene: RenderScene,
) -> Result<()> {
    fn compare(
        gpu: &Gpu,
        renderer: &mut SceneRenderer,
        scene: &RenderScene,
        hit: bool,
        label: &str,
    ) -> Result<()> {
        let actual = capture(gpu, renderer, scene, [128, 128])?;
        let stats = renderer.frame_stats();
        ensure!(
            stats.shadow_cache_hit == hit,
            "{label}: unexpected shadow cache state {stats:?}"
        );
        if hit {
            ensure!(
                stats.shadow_draws == 0,
                "{label}: reused shadows were redrawn"
            );
        }
        renderer.set_state_caching_enabled(false);
        let reference = capture(gpu, renderer, scene, [128, 128])?;
        renderer.set_state_caching_enabled(true);
        ensure!(
            actual.rgba == reference.rgba,
            "{label}: shadow/uniform cache changed pixels"
        );
        Ok(())
    }
    scene.lighting.shadows = true;
    let light = bozzard_render::LocalLight {
        directional: false,
        position: [2., 2., 3.],
        direction: [-0.4, -0.4, -1.],
        color: [1., 0.5, 0.2],
        intensity: 30.,
        range: 10.,
        spot_angles: None,
        shadows: Some(Default::default()),
    };
    scene.lights = vec![
        light,
        bozzard_render::LocalLight {
            spot_angles: Some([25., 50.]),
            ..light
        },
    ];
    compare(gpu, renderer, &scene, false, "initial sun/point/spot")?;
    compare(gpu, renderer, &scene, true, "unchanged")?;
    scene.view_projection *= Mat4::from_translation(Vec3::new(0.1, 0., 0.));
    compare(gpu, renderer, &scene, true, "camera pan")?;
    scene.lights[0].intensity *= 0.5;
    scene.lights[1].color = [0.1, 0.8, 1.];
    scene.lighting.sun_color = [1., 0.4, 0.2];
    compare(gpu, renderer, &scene, true, "light color and intensity")?;
    scene.items[0].material.tint = [0.2, 0.4, 0.9];
    scene.items[0].material.roughness = Some(0.7);
    compare(gpu, renderer, &scene, true, "surface color and roughness")?;
    scene.items[1].model *= Mat4::from_translation(Vec3::new(-0.5, 0., 0.));
    compare(gpu, renderer, &scene, false, "moving caster")?;
    scene.items[1].model *= Mat4::from_scale(Vec3::new(-1., 1., 1.));
    compare(gpu, renderer, &scene, false, "mirrored caster")?;
    scene.lights[0].position[0] += 0.3;
    compare(gpu, renderer, &scene, false, "moving point light")?;
    scene.lights[1].direction[0] -= 0.2;
    compare(gpu, renderer, &scene, false, "rotating spotlight")?;
    scene.lights[0].range = 7.;
    compare(gpu, renderer, &scene, false, "point range")?;
    scene.lights[1].shadows.as_mut().unwrap().normal_bias = 0.03;
    compare(gpu, renderer, &scene, false, "receiver bias")?;
    scene.lighting.shadow_resolution = 256;
    compare(gpu, renderer, &scene, false, "shadow resize")?;
    scene.items[1].material.texture = TextureKind::Imported("shadow-cache-alpha".into());
    renderer.upload_image(
        gpu,
        "shadow-cache-alpha",
        2,
        1,
        &[255, 255, 255, 255, 255, 255, 255, 0],
    )?;
    compare(gpu, renderer, &scene, false, "new transparent texture")?;
    scene.items[1].model *= Mat4::from_translation(Vec3::new(-0.3, 0.5, 0.));
    compare(
        gpu,
        renderer,
        &scene,
        false,
        "transparent receiver changes sun fit",
    )?;
    renderer.upload_image(
        gpu,
        "shadow-cache-alpha",
        2,
        1,
        &[255, 255, 255, 255, 255, 255, 255, 255],
    )?;
    compare(gpu, renderer, &scene, false, "texture made opaque")?;
    scene.items[1].material.uv_scale = [2., 3.];
    compare(gpu, renderer, &scene, false, "caster UV")?;
    scene.items[1].mesh = MeshKind::Imported("shadow-cache-mesh".into());
    let mut vertices = [
        [-0.5, -0.5, 0., 0., 0., 1., 0., 0.],
        [0.5, -0.5, 0., 0., 0., 1., 1., 0.],
        [0.0, 0.5, 0., 0., 0., 1., 0., 1.],
    ];
    renderer.upload_mesh(gpu, "shadow-cache-mesh", &vertices, &[0, 1, 2])?;
    compare(gpu, renderer, &scene, false, "new mesh")?;
    vertices[2][1] = 1.5;
    renderer.upload_mesh(gpu, "shadow-cache-mesh", &vertices, &[0, 1, 2])?;
    compare(gpu, renderer, &scene, false, "same-ID mesh reload")?;
    scene.items.pop();
    renderer.remove_asset("shadow-cache-mesh");
    compare(gpu, renderer, &scene, false, "caster removal")?;
    scene.lighting.shadows = false;
    scene.lights.clear();
    compare(gpu, renderer, &scene, false, "disable all shadows")?;
    compare(gpu, renderer, &scene, true, "disabled shadows remain idle")?;
    scene.lighting.shadows = true;
    scene.lights = vec![light];
    compare(gpu, renderer, &scene, false, "reenable shadows")?;
    println!(
        "shadow_cache_gpu_ok exact_reference_pixels camera material light caster reload resize enable_disable"
    );
    Ok(())
}
