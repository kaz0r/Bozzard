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
    let mut scene = scene(1024);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[0].frame_stats().color_draws, 1024);
    assert_eq!(renderers[1].frame_stats().color_draws, 32);
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
        assert!(builds > 0);
        if change < 6 {
            assert!(
                builds <= 2,
                "only edited/moving objects should rebuild: {builds}"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "release-mode synchronized CPU/wall benchmark; run explicitly"]
fn scale_benchmark() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers: [SceneRenderer; 2] =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_instancing_enabled(false);
    let mut scene = scene(1024);
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
        let mut samples: [Vec<(f64, f64)>; 2] = Default::default();
        for frame in 0..110 {
            if moving {
                for item in &mut scene.items {
                    item.model *= Mat4::from_rotation_y(0.003);
                }
            }
            for mode in if frame % 2 == 0 { [0, 1] } else { [1, 0] } {
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
                "moving={moving} instancing={} draws={} cpu_ms={:.3} synchronized_ms={:.3}",
                mode == 1,
                renderers[mode].frame_stats().color_draws,
                median(false),
                median(true)
            );
        }
    }
    Ok(())
}
