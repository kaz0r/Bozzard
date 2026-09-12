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
        },
    };
    let scene = RenderScene {
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
    let reference = capture(gpu, &mut renderer, &scene, [128, 128])?;
    let all = renderer.frame_stats();
    renderer.set_culling_enabled(true);
    renderer.set_state_caching_enabled(true);
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
            && reduced.shadow_draws == all.shadow_draws,
        "batching or shadow counters incorrect"
    );
    let mut shadow_scene = RenderScene {
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
    println!(
        "visibility_gpu_ok exact_reference_pixels mirrored_crossing_bounds offscreen_shadow_casters pipeline_cache counters"
    );
    Ok(())
}
