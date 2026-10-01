use bozzard_render::*;
use glam::{Mat4, Vec3};

fn scene(count: usize) -> RenderScene {
    RenderScene {
        skin_poses: Default::default(),
        shader_time: 0.,
        particles: vec![],
        view_projection: glam::camera::rh::proj::directx::orthographic(
            -17., 17., -17., 17., 0.1, 30.,
        ),
        items: (0..count)
            .map(|i| DrawItem {
                motion_id: i as u64 + 1,
                model: Mat4::from_translation(Vec3::new(
                    (i % 32) as f32 - 15.5,
                    (i / 32) as f32 - 15.5,
                    -5.,
                )) * Mat4::from_scale(Vec3::splat(0.7)),
                mesh: MeshKind::Cube,
                material: Material {
                    metallic: Some(0.2),
                    roughness: Some(0.6),
                    tint: [0.2 + (i % 7) as f32 * 0.1, 0.5, 0.3],
                    lit: true,
                    texture: TextureKind::Checker,
                    uv_scale: [1.; 2],
                    surface_overrides: Default::default(),
                    shader: None,
                },
            })
            .collect(),
        lighting: Lighting {
            shadows: false,
            ..Default::default()
        },
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        fog: Default::default(),
        gi: None,
        display: DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        },
    }
}
fn capture(gpu: &Gpu, renderer: &mut SceneRenderer, scene: &RenderScene) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, 320, 320, |target| {
        renderer.draw(gpu, target, [320; 2], scene)
    })
}
fn compare(
    gpu: &Gpu,
    renderers: &mut [SceneRenderer; 2],
    scene: &RenderScene,
) -> anyhow::Result<()> {
    let a = capture(gpu, &mut renderers[0], scene)?;
    let b = capture(gpu, &mut renderers[1], scene)?;
    assert_eq!(a.rgba, b.rgba, "instanced and reference pixels differ");
    assert_eq!(
        renderers[0].frame_stats().color_triangles,
        renderers[1].frame_stats().color_triangles
    );
    assert_eq!(
        renderers[0].frame_stats().visible_surfaces,
        renderers[1].frame_stats().visible_surfaces
    );
    Ok(())
}
fn upload(gpu: &Gpu, renderer: &mut SceneRenderer, alpha: u8) -> anyhow::Result<()> {
    let vertices = [
        [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
        [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
        [0., 0.5, 0., 0., 0., 1., 0.5, 0.],
    ];
    let attrs = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 3];
    renderer.upload_model(
        gpu,
        "model",
        &vertices,
        &[0, 1, 2],
        &[ModelPart {
            source_key: "0000000000000000",
            start: 0,
            count: 3,
            color: [1.; 4],
            alpha_cutoff: Some(0.5),
            image: Some(ModelImage {
                width: 1,
                height: 1,
                rgba: &[alpha, 255, 255, alpha],
            }),
            shading: Some(ModelShading {
                vertex_start: 0,
                vertices: &attrs,
                metallic: 0.3,
                roughness: 0.6,
                normal_scale: 1.,
                occlusion_strength: 1.,
                emissive_factor: [0.; 3],
                double_sided: false,
                base_color_sampler: Default::default(),
                normal: None,
                metallic_roughness: None,
                occlusion: None,
                emissive: None,
            }),
        }],
    )
}

#[test]
fn opaque_runs_match_reference_through_edits_temporal_shadows_and_reuploads() -> anyhow::Result<()>
{
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_instancing_enabled(false);
    renderers[0].set_state_caching_enabled(false);
    renderers[0].set_local_light_culling_enabled(false);
    let mut scene = scene(1024);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[0].frame_stats().color_draws, 1024);
    assert_eq!(renderers[1].frame_stats().color_draws, 16);
    assert_eq!(renderers[1].frame_stats().object_uniform_writes, 0);
    assert_eq!(renderers[1].frame_stats().instanced_surfaces, 1024);
    assert_eq!(renderers[0].frame_stats().visible_items, 1024);
    assert_eq!(renderers[1].frame_stats().visible_items, 1024);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
    // Culled objects, a singleton tail, mirrored/nonuniform scales and material edits.
    scene.items.truncate(67);
    scene.items[8].model = Mat4::from_translation(Vec3::new(100., 0., -4.));
    scene.items[9].model *= Mat4::from_scale(Vec3::new(-1., 2., 0.7));
    scene.items[12].material.texture = TextureKind::Normals;
    scene.items[13].material.uv_scale = [2., 3.];
    scene.lighting.shadows = true;
    scene.lighting.shadow_resolution = 256;
    scene.display.temporal_aa.enabled = true;
    scene.display.motion_blur.enabled = true;
    for tick in 0..4 {
        scene.display.time_seconds = tick as f32 / 60.;
        scene.items[3].model *= Mat4::from_translation(Vec3::new(0.1, 0., 0.));
        compare(&gpu, &mut renderers, &scene)?;
        assert_eq!(renderers[1].frame_stats().visible_items, 66);
        assert_eq!(
            renderers[0].frame_stats().shadow_triangles,
            renderers[1].frame_stats().shadow_triangles
        );
        assert!(renderers[1].frame_stats().shadow_draws < renderers[0].frame_stats().shadow_draws);
    }
    // Local light frusta can split a color batch; unlit objects in that batch must never cast.
    scene.items[15].material.lit = false;
    scene.lights = vec![
        LocalLight {
            directional: false,
            position: [0., -14., 1.],
            direction: [0., 0., -1.],
            color: [1., 0.6, 0.3],
            intensity: 3.,
            range: 40.,
            spot_angles: Some([35., 55.]),
            shadows: Some(Default::default()),
        },
        LocalLight {
            directional: false,
            position: [0., -14., -3.],
            direction: [0., 0., -1.],
            color: [0.3, 0.6, 1.],
            intensity: 2.,
            range: 30.,
            spot_angles: None,
            shadows: Some(Default::default()),
        },
    ];
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(
        renderers[0].frame_stats().shadow_triangles,
        renderers[1].frame_stats().shadow_triangles
    );
    assert!(renderers[1].frame_stats().shadow_draws < renderers[0].frame_stats().shadow_draws);
    // Transparent textures stay sorted and single-draw even between opaque runs.
    for r in &mut renderers {
        r.upload_image(&gpu, "glass", 1, 1, &[255, 20, 0, 128])?;
    }
    for i in [10, 11, 60] {
        scene.items[i].material.texture = TextureKind::Imported("glass".into());
    }
    compare(&gpu, &mut renderers, &scene)?;
    // Imported PBR surface identity, alpha masks, and same-ID replacement invalidate bindings.
    for item in &mut scene.items {
        item.mesh = MeshKind::Imported("model".into());
        item.material.texture = TextureKind::White;
    }
    for alpha in [255, 0, 255] {
        for r in &mut renderers {
            upload(&gpu, r, alpha)?;
        }
        compare(&gpu, &mut renderers, &scene)?;
        assert!(renderers[1].frame_stats().instanced_surfaces > 32);
    }
    scene.items.clear();
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().color_draws, 0);
    Ok(())
}

#[test]
fn stationary_uniforms_are_reused_and_render_edits_match_uncached_output() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_state_caching_enabled(false);
    let mut scene = scene(67);
    for _ in 0..3 {
        compare(&gpu, &mut renderers, &scene)?;
    }
    assert_eq!(renderers[1].frame_stats().object_uniform_builds, 0);
    for change in 0..9 {
        match change {
            0 => scene.items[0].model *= Mat4::from_translation(Vec3::X * 0.2),
            1 => scene.items[1].material.tint = [1., 0., 0.],
            2 => scene.items[2].material.uv_scale = [2., 3.],
            3 => scene.items[3].material.texture = TextureKind::Normals,
            4 => scene.items[4].material.roughness = Some(0.1),
            5 => scene.items[5].material.lit = false,
            6 => scene.lighting.sun_color = [0.3, 1., 0.4],
            7 => scene.view_projection *= Mat4::from_translation(Vec3::X * 0.25),
            _ => {
                scene.fog.enabled = true;
                scene.fog.distance_density = 0.08;
            }
        }
        compare(&gpu, &mut renderers, &scene)?;
        let builds = renderers[1].frame_stats().object_uniform_builds;
        if change < 6 {
            assert!(builds > 0);
            assert!(
                builds <= 2,
                "only edited/moving objects should rebuild: {builds}"
            );
            if change != 3 && change != 5 {
                assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 256);
            }
        } else {
            assert_eq!(builds, 0, "frame changes must not rebuild object uniforms");
            assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
            assert_eq!(renderers[1].frame_stats().object_uniform_writes, 0);
            assert_eq!(renderers[1].frame_stats().frame_uniform_bytes, 320);
        }
    }
    Ok(())
}

#[test]
fn interleaved_meshes_batch_globally_without_changing_coplanar_or_transparent_pixels()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_global_batching_enabled(false);
    let mut scene = scene(1024);
    for (i, item) in scene.items.iter_mut().enumerate() {
        if i % 2 != 0 {
            item.mesh = MeshKind::Sphere;
        }
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[0].frame_stats().color_draws, 1024);
    assert_eq!(renderers[1].frame_stats().color_draws, 16);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
    // Exercise invalidation, hidden members and a moving camera.
    for tick in 0..3 {
        scene.items[1].model *= Mat4::from_translation(Vec3::X * 0.03);
        scene.items[5].model = Mat4::from_translation(Vec3::new(100., 0., -5.));
        scene.view_projection *= Mat4::from_translation(Vec3::Y * 0.01);
        scene.items[2].material.tint[tick] = 0.9;
        compare(&gpu, &mut renderers, &scene)?;
    }
    // Equal-depth surfaces separated by a different mesh/material must preserve
    // their original winner, even when that prevents combining matching quads.
    scene.items.truncate(3);
    for (i, item) in scene.items.iter_mut().enumerate() {
        item.mesh = MeshKind::Quad;
        item.model =
            Mat4::from_translation(Vec3::new(0., 0., -5.)) * Mat4::from_scale(Vec3::splat(16.));
        item.material.texture = if i == 1 {
            TextureKind::Checker
        } else {
            TextureKind::White
        };
        item.material.tint = [i as f32 * 0.3, 0.9 - i as f32 * 0.3, 0.4];
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().color_draws, 3);
    // Near-plane-crossing bounds cannot safely participate in opaque reordering.
    scene.items[1].mesh = MeshKind::Cube;
    scene.items[1].model = Mat4::from_scale(Vec3::splat(4.));
    scene.view_projection = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 30.);
    compare(&gpu, &mut renderers, &scene)?;
    for renderer in &mut renderers {
        renderer.upload_image(&gpu, "glass", 1, 1, &[200, 30, 0, 128])?;
    }
    scene.items[0].material.texture = TextureKind::Imported("glass".into());
    scene.items[2].material.texture = TextureKind::Imported("glass".into());
    compare(&gpu, &mut renderers, &scene)?;
    Ok(())
}

#[test]
fn sparse_edits_and_batch_regrowth_reuse_instance_buffers() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_instancing_enabled(false);
    let mut scene = scene(131);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_buffer_allocations, 3);
    for index in [0, 1, 7, 63, 64, 127, 130] {
        scene.items[index].material.tint = [0.9, 0.2, 0.1];
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 7 * 256);
    assert_eq!(renderers[1].frame_stats().instance_buffer_allocations, 0);
    let items = scene.items.clone();
    scene.items.truncate(2);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
    scene.items = items;
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 62 * 256);
    assert_eq!(renderers[1].frame_stats().instance_buffer_allocations, 0);
    for item in &mut scene.items {
        item.material.texture = TextureKind::White;
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
    assert_eq!(renderers[1].frame_stats().instance_buffer_allocations, 0);
    scene.lighting.shadows = true;
    scene.lighting.shadow_resolution = 256;
    compare(&gpu, &mut renderers, &scene)?;
    compare(&gpu, &mut renderers, &scene)?;
    assert!(renderers[1].frame_stats().shadow_cache_hit);
    scene.items[0].material.lit = false;
    compare(&gpu, &mut renderers, &scene)?;
    assert!(!renderers[1].frame_stats().shadow_cache_hit);
    assert!(renderers[1].frame_stats().shadow_maps_rendered > 0);
    assert_eq!(
        renderers[0].frame_stats().shadow_triangles,
        renderers[1].frame_stats().shadow_triangles
    );
    Ok(())
}

#[test]
fn graph_clock_is_shared_without_rebuilding_individual_objects() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene(1);
    scene.items[0].model =
        Mat4::from_translation(Vec3::new(0., 0., -5.)) * Mat4::from_scale(Vec3::splat(12.));
    scene.items[0].material.lit = false;
    scene.items[0].material.shader = Some(std::sync::Arc::new(ShaderSource {
        id: 987654321,
        surface: "fn graph_material_surface(uv:vec2<f32>,normal_uv:vec2<f32>,mr_uv:vec2<f32>,ao_uv:vec2<f32>,emissive_uv:vec2<f32>,world_normal:vec3<f32>,tangent:vec4<f32>,world:vec3<f32>,view:vec3<f32>,front:bool,time:f32)->SurfaceParams { return SurfaceParams(vec3<f32>(time,0.2,0.1),0.0,1.0,vec3<f32>(0),1.0,world_normal,1.0); }".into(),
    }));
    let a = capture(&gpu, &mut renderer, &scene)?;
    scene.shader_time = 0.75;
    let b = capture(&gpu, &mut renderer, &scene)?;
    assert_ne!(
        a.rgba, b.rgba,
        "graph Time must still change the visible surface"
    );
    assert_eq!(renderer.frame_stats().object_uniform_builds, 0);
    assert_eq!(renderer.frame_stats().object_uniform_writes, 0);
    assert_eq!(renderer.frame_stats().frame_uniform_bytes, 320);
    assert_eq!(b.rgba, capture(&gpu, &mut renderer, &scene)?.rgba);
    assert_eq!(renderer.frame_stats().frame_uniform_bytes, 0);
    Ok(())
}

#[test]
fn camera_validation_errors_do_not_poison_shared_uniform_cache() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene(1);
    scene.items[0].model =
        Mat4::from_translation(Vec3::new(0., 0., -5.)) * Mat4::from_scale(Vec3::new(1e10, 1., 1.));
    let good = capture(&gpu, &mut renderer, &scene)?;
    let camera = scene.view_projection;
    scene.view_projection = Mat4::from_scale(Vec3::new(1e30, 1., 1.)) * camera;
    for _ in 0..2 {
        let error = capture(&gpu, &mut renderer, &scene)
            .err()
            .expect("overflowing combined transform");
        assert!(
            error.to_string().contains("invalid object matrix"),
            "{error}"
        );
    }
    scene.view_projection = camera;
    assert_eq!(good.rgba, capture(&gpu, &mut renderer, &scene)?.rgba);
    assert_eq!(renderer.frame_stats().frame_uniform_bytes, 320);
    Ok(())
}

#[test]
fn incremental_plans_retain_camera_and_rotor_edits_but_rebuild_for_new_conflicts()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_incremental_batch_planning_enabled(false);
    let mut scene = scene(132);
    for (i, item) in scene.items.iter_mut().enumerate() {
        if i % 2 != 0 {
            item.mesh = MeshKind::Sphere;
        }
    }
    compare(&gpu, &mut renderers, &scene)?;
    for _ in 0..8 {
        scene.view_projection *= Mat4::from_translation(Vec3::X * 0.006);
        scene.items[0].model *= Mat4::from_rotation_y(0.015);
        compare(&gpu, &mut renderers, &scene)?;
        assert!(renderers[1].frame_stats().batch_plan_reused);
        assert_eq!(renderers[1].frame_stats().batch_plan_rebuilds, 0);
        assert!(renderers[1].frame_stats().batch_bounds_updates <= 2);
    }
    scene.view_projection = Mat4::from_rotation_z(0.04) * scene.view_projection;
    compare(&gpu, &mut renderers, &scene)?;
    assert!(renderers[1].frame_stats().batch_plan_reused);
    // A large relocation escapes the certified bounds and must build a new plan.
    scene.items[0].model = scene.items[3].model;
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().batch_plan_rebuilds, 1);

    scene.items.truncate(3);
    scene.view_projection =
        glam::camera::rh::proj::directx::orthographic(-17., 17., -17., 17., 0.1, 30.);
    for (i, item) in scene.items.iter_mut().enumerate() {
        item.mesh = MeshKind::Quad;
        item.model = Mat4::from_translation(Vec3::new([-6., 0., 4.04][i], 0., -5.))
            * Mat4::from_scale(Vec3::splat(4.));
        item.material.lit = false;
        item.material.texture = if i == 1 {
            TextureKind::Checker
        } else {
            TextureKind::White
        };
        item.material.tint = if i == 1 {
            [0.1, 0.9, 0.2]
        } else {
            [0.9, 0.1, 0.1]
        };
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().color_draws, 2);
    // This small edit stays inside the envelope but creates an inverted coplanar
    // overlap. Retaining the former two-draw plan would change the visible winner.
    scene.items[2].model =
        Mat4::from_translation(Vec3::new(3.92, 0., -5.)) * Mat4::from_scale(Vec3::splat(4.));
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().batch_plan_rebuilds, 1);
    assert_eq!(renderers[1].frame_stats().color_draws, 3);
    // Metadata and visibility changes keep their full rebuild path.
    scene.items[1].material.texture = TextureKind::Normals;
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().batch_plan_rebuilds, 1);
    scene.items[2].model = Mat4::from_translation(Vec3::new(100., 0., -5.));
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().batch_plan_rebuilds, 1);
    Ok(())
}

#[test]
#[ignore = "release-mode synchronized CPU/wall benchmark; run explicitly"]
fn scale_benchmark() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers: [SceneRenderer; 3] =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_instancing_enabled(false);
    renderers[1].set_global_batching_enabled(false);
    let mut scene = scene(1024);
    for (i, item) in scene.items.iter_mut().enumerate() {
        if i % 2 != 0 {
            item.mesh = MeshKind::Sphere;
        }
    }
    let target = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("scale benchmark target"),
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
    for moving in [false, true] {
        let mut samples: [Vec<(f64, f64)>; 3] = Default::default();
        for frame in 0..110 {
            if moving {
                for item in &mut scene.items {
                    item.model *= Mat4::from_rotation_y(0.003);
                }
            }
            for offset in 0..3 {
                let mode = (frame + offset) % 3;
                let start = std::time::Instant::now();
                renderers[mode].draw(&gpu, &target, [320; 2], &scene)?;
                gpu.wait()?;
                if frame >= 10 {
                    samples[mode].push((
                        renderers[mode].frame_stats().cpu_ms,
                        start.elapsed().as_secs_f64() * 1000.,
                    ));
                }
            }
        }
        for (mode, values) in samples.iter().enumerate() {
            let median = |wall: bool| {
                let mut v: Vec<_> = values
                    .iter()
                    .map(|(cpu, total)| if wall { *total } else { *cpu })
                    .collect();
                v.sort_by(f64::total_cmp);
                (v[49] + v[50]) * 0.5
            };
            println!(
                "moving={moving} mode={} draws={} cpu_ms={:.3} synchronized_ms={:.3}",
                ["individual", "consecutive", "global"][mode],
                renderers[mode].frame_stats().color_draws,
                median(false),
                median(true)
            );
        }
    }
    Ok(())
}

#[test]
fn local_light_masks_match_full_loops_through_geometry_light_and_material_edits()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_instancing_enabled(false);
    renderers[0].set_local_light_culling_enabled(false);
    let mut scene = scene(132);
    scene.lights = (0..32)
        .map(|i| LocalLight {
            directional: i == 31,
            position: [(i % 8) as f32 * 4. - 14., (i / 8) as f32 * 8. - 12., -3.],
            direction: [0., 0., -1.],
            color: [0.5, 0.3, 0.8],
            intensity: 3.,
            range: 4.,
            spot_angles: (i % 3 == 0 && i != 31).then_some([35., 35.]),
            shadows: None,
        })
        .collect();
    scene.items[2].model *= Mat4::from_scale(Vec3::new(-1., 2., 0.7));
    scene.items[4].material.metallic = None;
    scene.items[4].material.roughness = None; // Standard diffuse host, too.
    for renderer in &mut renderers {
        renderer.upload_image(&gpu, "glass", 1, 1, &[180, 220, 255, 128])?;
        upload(&gpu, renderer, 255)?;
    }
    scene.items[8].material.texture = TextureKind::Imported("glass".into());
    scene.items[9].mesh = MeshKind::Imported("model".into());
    scene.items[9].material.texture = TextureKind::White;
    compare(&gpu, &mut renderers, &scene)?;
    let stats = renderers[1].frame_stats();
    assert!(stats.local_light_candidates < stats.local_light_slots / 4);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().light_mask_builds, 0);
    // Positive radiance edits use the shared light buffer, preserving the masks.
    scene.lights[1].intensity *= 2.;
    scene.lights[1].color = [0.2, 0.8, 0.4];
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().light_mask_builds, 0);
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
    // A boundary/tangent light, zero radiance, changing ranges, light order and
    // the high mask bit all retain the same ascending-index accumulation.
    let center = scene.items[0].model.w_axis.truncate();
    scene.lights[0].position = (center + Vec3::new(0.35, 0., 0.351)).to_array();
    scene.lights[0].range = 0.001;
    scene.lights[0].spot_angles = None;
    scene.lights[2].intensity = 0.;
    scene.lights[3].color = [0.; 3];
    compare(&gpu, &mut renderers, &scene)?;
    for tick in 0..4 {
        scene.lights[0].range = 2. + tick as f32;
        scene.lights[1].position[0] -= 1.;
        scene.items[0].model *= Mat4::from_rotation_y(0.07);
        scene.items[6].material.lit = tick % 2 == 0;
        scene.lights.swap(5, 31);
        compare(&gpu, &mut renderers, &scene)?;
    }
    // Shadow-light slots do not change when distant lights are omitted.
    scene.lighting.shadow_resolution = 256;
    scene.lights[0].range = 8.;
    scene.lights[0].shadows = Some(Default::default());
    scene.lights[1].spot_angles = Some([30., 60.]);
    scene.lights[1].shadows = Some(Default::default());
    compare(&gpu, &mut renderers, &scene)?;
    // A late invalid object must not publish stale light revisions for a retry.
    scene.lights[0].position[0] += 2.;
    let saved = scene.items[100].model;
    scene.items[100].model = Mat4::ZERO;
    for renderer in &mut renderers {
        assert!(capture(&gpu, renderer, &scene).is_err());
    }
    scene.items[100].model = saved;
    compare(&gpu, &mut renderers, &scene)?;
    // Conservative rounding at large world coordinates, plus the projective
    // model fallback (surface lighting uses undivided world coordinates).
    let translation = Vec3::new(10000., -10000., 0.);
    for item in &mut scene.items {
        item.model = Mat4::from_translation(translation) * item.model;
    }
    for light in &mut scene.lights {
        light.position = (Vec3::from(light.position) + translation).to_array();
    }
    scene.view_projection *= Mat4::from_translation(-translation);
    compare(&gpu, &mut renderers, &scene)?;
    scene.items[2].model.x_axis.w = 0.0001;
    compare(&gpu, &mut renderers, &scene)?;
    let lights = std::mem::take(&mut scene.lights);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().light_mask_builds, 0);
    scene.lights = lights;
    compare(&gpu, &mut renderers, &scene)?;
    Ok(())
}

/// Run the same workload before/after a renderer change. An optional reference
/// directory records raw captures on the first run and compares them thereafter.
#[test]
#[ignore = "release-mode uniform update benchmark; run explicitly"]
fn uniform_update_benchmark() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let target = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("uniform update benchmark target"),
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
    for mode in [
        "stationary",
        "daylight",
        "camera",
        "one_object",
        "all_objects",
    ] {
        let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut scene = scene(1024);
        for (i, item) in scene.items.iter_mut().enumerate() {
            if i % 2 != 0 {
                item.mesh = MeshKind::Sphere;
            }
        }
        let camera = scene.view_projection;
        let mut cpu = Vec::new();
        let mut wall = Vec::new();
        for frame in 0..90 {
            match mode {
                "daylight" => {
                    scene.lighting.sun_color = [0.6 + frame as f32 * 0.002, 0.8, 0.7];
                    scene.fog.enabled = true;
                    scene.fog.distance_density = 0.01 + frame as f32 * 0.0001;
                }
                "camera" => {
                    scene.view_projection =
                        camera * Mat4::from_translation(Vec3::new(frame as f32 * 0.001, 0., 0.))
                }
                "one_object" => scene.items[0].model *= Mat4::from_rotation_y(0.003),
                "all_objects" => {
                    for item in &mut scene.items {
                        item.model *= Mat4::from_rotation_y(0.003);
                    }
                }
                _ => {}
            }
            let start = std::time::Instant::now();
            renderer.draw(&gpu, &target, [320; 2], &scene)?;
            gpu.wait()?;
            if frame >= 10 {
                cpu.push(renderer.frame_stats().cpu_ms);
                wall.push(start.elapsed().as_secs_f64() * 1000.);
            }
        }
        let median = |mut values: Vec<f64>| {
            values.sort_by(f64::total_cmp);
            (values[39] + values[40]) * 0.5
        };
        let stats = renderer.frame_stats();
        println!(
            "uniform_update mode={mode} draws={} builds={} instance_bytes={} frame_bytes={} plan_reused={} bounds={} checks={} cpu_ms={:.3} synchronized_ms={:.3}",
            stats.color_draws,
            stats.object_uniform_builds,
            stats.instance_uniform_bytes,
            stats.frame_uniform_bytes,
            stats.batch_plan_reused,
            stats.batch_bounds_updates,
            stats.batch_order_checks,
            median(cpu),
            median(wall)
        );
        if let Some(directory) = std::env::var_os("BOZZARD_UNIFORM_REFERENCE_DIR") {
            std::fs::create_dir_all(&directory)?;
            let path = std::path::PathBuf::from(directory).join(format!("{mode}.rgba"));
            let frame = capture(&gpu, &mut renderer, &scene)?;
            if path.exists() {
                anyhow::ensure!(
                    std::fs::read(&path)? == frame.rgba,
                    "historical pixels differ for {mode}"
                );
            } else {
                std::fs::write(path, &frame.rgba)?;
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "release-mode local lighting benchmark; run explicitly"]
fn local_lighting_benchmark() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene(1024);
    scene.lights = (0..32)
        .map(|i| LocalLight {
            directional: false,
            position: [(i % 8) as f32 * 4. - 14., (i / 8) as f32 * 8. - 12., -3.],
            direction: [0., 0., -1.],
            color: [0.5, 0.3 + (i % 3) as f32 * 0.2, 0.8],
            intensity: 12.,
            range: 4.,
            spot_angles: None,
            shadows: None,
        })
        .collect();
    let target = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("local lighting benchmark target"),
            size: wgpu::Extent3d {
                width: 1280,
                height: 800,
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
    let mut cpu = Vec::new();
    let mut wall = Vec::new();
    for frame in 0..90 {
        let start = std::time::Instant::now();
        renderer.draw(&gpu, &target, [1280, 800], &scene)?;
        gpu.wait()?;
        if frame >= 10 {
            cpu.push(renderer.frame_stats().cpu_ms);
            wall.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    let median = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        (values[39] + values[40]) * 0.5
    };
    println!(
        "local_lighting draws={} candidates={}/{} cpu_ms={:.3} synchronized_ms={:.3}",
        renderer.frame_stats().color_draws,
        renderer.frame_stats().local_light_candidates,
        renderer.frame_stats().local_light_slots,
        median(cpu),
        median(wall)
    );
    if let Some(directory) = std::env::var_os("BOZZARD_UNIFORM_REFERENCE_DIR") {
        std::fs::create_dir_all(&directory)?;
        let path = std::path::PathBuf::from(directory).join("local_lighting.rgba");
        let frame = capture_offscreen(&gpu, 1280, 800, |target| {
            renderer.draw(&gpu, target, [1280, 800], &scene)
        })?;
        if path.exists() {
            anyhow::ensure!(
                std::fs::read(&path)? == frame.rgba,
                "historical lighting pixels differ"
            );
        } else {
            std::fs::write(path, &frame.rgba)?;
        }
    }
    Ok(())
}
