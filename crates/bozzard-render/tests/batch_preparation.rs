//! Exact parity and work-count regressions for retained batching preparation.
use bozzard_render::*;
use glam::{Mat4, Vec3};

fn scene() -> RenderScene {
    RenderScene {
        view_projection: camera(0.),
        items: (0..1024)
            .map(|index| {
                let group = index / 64;
                let local = index % 64;
                DrawItem {
                    motion_id: index as u64 + 1,
                    model: Mat4::from_translation(Vec3::new(
                        if group % 2 == 0 { -12. } else { 12. } + (local % 8) as f32 * 0.3 - 1.05,
                        (group / 2) as f32 * 3. - 10.5 + (local / 8) as f32 * 0.3 - 1.05,
                        -5.,
                    )) * Mat4::from_scale(Vec3::splat(0.2)),
                    mesh: MeshKind::Cube,
                    material: Material {
                        texture: TextureKind::Imported(format!("group-{group:02}")),
                        tint: [0.3 + (local % 5) as f32 * 0.1, 0.6, 0.4],
                        lit: true,
                        metallic: Some(0.2),
                        roughness: Some(0.6),
                        uv_scale: [1.; 2],
                        surface_overrides: Default::default(),
                        shader: None,
                    },
                }
            })
            .collect(),
        lighting: Lighting {
            shadows: false,
            ..Default::default()
        },
        environment: EnvironmentSettings::disabled(),
        display: DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        },
        skin_poses: Default::default(),
        particles: vec![],
        lights: vec![],
        fog: Default::default(),
        gi: None,
        shader_time: 0.,
    }
}
fn camera(x: f32) -> Mat4 {
    glam::camera::rh::proj::directx::orthographic(-16., 16., -16., 16., 0.1, 30.)
        * Mat4::from_translation(Vec3::X * -x)
}
fn setup() -> anyhow::Result<(Gpu, [SceneRenderer; 2])> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_batch_preparation_caching_enabled(false);
    for renderer in &mut renderers {
        renderer.set_occlusion_enabled(false);
        for group in 0..16 {
            renderer.upload_image(
                &gpu,
                &format!("group-{group:02}"),
                1,
                1,
                &[30 + group as u8 * 10, 150, 220, 255],
            )?;
        }
    }
    Ok((gpu, renderers))
}
fn compare(
    gpu: &Gpu,
    renderers: &mut [SceneRenderer; 2],
    scene: &RenderScene,
) -> anyhow::Result<[FrameStats; 2]> {
    let mut pixels = None;
    for renderer in renderers.iter_mut() {
        let capture = capture_offscreen(gpu, 320, 320, |target| {
            renderer.draw(gpu, target, [320; 2], scene)
        })?;
        if let Some(pixels) = &pixels {
            assert_eq!(
                capture.rgba, *pixels,
                "retained batch preparation changed pixels"
            );
        } else {
            pixels = Some(capture.rgba);
        }
    }
    let stats = renderers.each_ref().map(|renderer| renderer.frame_stats());
    for frame in &stats {
        let stages = frame.preparation_stages();
        assert!(stages.iter().all(|(_, ms)| ms.is_finite() && *ms >= 0.));
        let sum: f64 = stages.iter().map(|(_, ms)| ms).sum();
        assert!(
            (sum - frame.prepare_ms).abs() <= frame.prepare_ms.max(1.) * 1e-9,
            "overlapping or missing preparation stages: {sum} vs {}",
            frame.prepare_ms
        );
        assert!(frame.batch_plan_ms + frame.instance_prepare_ms <= frame.batch_prepare_ms + 1e-6);
        assert!(
            frame.shadow_batch_plan_ms + frame.shadow_instance_prepare_ms
                <= frame.shadow_prepare_ms + 1e-6
        );
        assert_eq!(frame.object_uniform_source_checks, frame.surfaces);
        assert!(frame.object_matrix_checks <= frame.object_uniform_source_checks);
        assert_eq!(
            frame.light_mask_checks,
            frame.light_mask_cache_hits + frame.light_mask_builds
        );
        assert!(frame.object_uniform_writes <= frame.individual_uniform_candidates);
    }
    assert_eq!(stats[0].batching, stats[1].batching);
    assert_eq!(stats[0].color_draws, stats[1].color_draws);
    assert_eq!(stats[0].color_triangles, stats[1].color_triangles);
    assert_eq!(stats[0].shadow_draws, stats[1].shadow_draws);
    assert_eq!(stats[0].shadow_triangles, stats[1].shadow_triangles);
    assert!(stats[1].instance_buffer_bytes <= (stats[1].instanced_draws + 8) * 16 * 1024);
    Ok(stats)
}

#[test]
fn preparation_profile_counts_distinguish_warm_checks_camera_validation_edits_and_empty_frames()
-> anyhow::Result<()> {
    let (gpu, mut renderers) = setup()?;
    let mut scene = scene();
    scene.lights.push(LocalLight {
        directional: false,
        position: [0.; 3],
        direction: [0., -1., 0.],
        color: [1.; 3],
        intensity: 10.,
        range: 40.,
        spot_angles: None,
        shadows: None,
    });
    let cold = compare(&gpu, &mut renderers, &scene)?[1];
    assert_eq!(cold.object_binding_allocations, 1024);
    assert_eq!(cold.object_uniform_builds, 1024);
    assert_eq!(cold.light_mask_builds, 1024);
    assert_eq!(cold.object_matrix_checks, 1024);
    compare(&gpu, &mut renderers, &scene)?;
    let warm = compare(&gpu, &mut renderers, &scene)?[1];
    assert_eq!(warm.object_binding_allocations, 0);
    assert_eq!(warm.mesh_validation_checks, 0); // Imported textures, but primitive meshes.
    assert_eq!(warm.graph_source_checks, 1024);
    assert_eq!(warm.graph_instance_checks, 16);
    assert_eq!(warm.object_uniform_source_checks, 1024);
    assert_eq!(warm.object_matrix_checks, 0);
    assert_eq!(warm.object_uniform_builds, 0);
    assert_eq!(warm.light_mask_cache_hits, 1024);
    assert_eq!(warm.light_mask_builds, 0);
    assert_eq!(warm.individual_uniform_candidates, 0);
    assert_eq!(
        warm.scratch_bounds_bytes,
        1024 * std::mem::size_of::<[Vec3; 2]>()
    );
    assert_eq!(warm.scratch_visibility_bytes, 1024);
    assert_eq!(warm.scratch_visible_items_bytes, 1024);
    assert_eq!(warm.scratch_individual_bytes, 1024);
    assert_eq!(warm.scratch_transparent_bytes, 0);

    scene.view_projection = camera(8.);
    let camera = compare(&gpu, &mut renderers, &scene)?[1];
    assert_eq!(camera.object_uniform_builds, 0);
    assert_eq!(camera.object_matrix_checks, 1024); // Cannot skip combined-matrix validation.
    assert_eq!(camera.light_mask_cache_hits, 1024);
    scene.items[0].material.tint[0] += 0.01; // An invisible surface still gets checked.
    let edit = compare(&gpu, &mut renderers, &scene)?[1];
    assert_eq!(edit.object_uniform_source_checks, 1024);
    assert_eq!(edit.object_uniform_builds, 1);
    assert_eq!(edit.object_matrix_checks, 1);
    assert_eq!(edit.light_mask_cache_hits, 1024);

    scene.items.clear();
    let empty = compare(&gpu, &mut renderers, &scene)?[1];
    assert_eq!(empty.graph_source_checks, 0);
    assert_eq!(empty.graph_instance_checks, 0);
    assert_eq!(empty.object_uniform_source_checks, 0);
    assert_eq!(empty.object_matrix_checks, 0);
    assert_eq!(empty.light_mask_checks, 0);
    assert_eq!(empty.object_binding_allocations, 0);
    assert_eq!(empty.individual_uniform_candidates, 0);
    assert_eq!(empty.preparation_scratch_bytes(), 0);
    Ok(())
}

#[test]
fn whole_group_culling_preserves_residency_and_bounds_spares_through_empty_frames()
-> anyhow::Result<()> {
    let (gpu, mut renderers) = setup()?;
    let mut scene = scene();
    let mut uploads = [0; 2];
    for (frame, x) in [0., 8., 0., 8., 0., 8., 0., 8.].into_iter().enumerate() {
        scene.view_projection = camera(x);
        let stats = compare(&gpu, &mut renderers, &scene)?;
        if frame >= 4 {
            assert!(stats[1].batch_plan_reused);
            assert_eq!(
                stats[1].instance_uniform_bytes, 0,
                "unchanged whole groups moved between buffers"
            );
            assert_eq!(stats[1].instance_buffer_allocations, 0);
            for mode in 0..2 {
                uploads[mode] += stats[mode].instance_uniform_bytes;
            }
        }
    }
    assert!(
        uploads[0] > 0,
        "fixture must exercise the old positional-buffer churn"
    );
    assert_eq!(uploads[1], 0);
    println!(
        "whole_group_culling_upload_bytes: positional={} retained={}",
        uploads[0], uploads[1]
    );
    scene.view_projection = camera(100.);
    let stats = compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(stats[1].color_draws, 0);
    assert!(stats[1].instance_buffer_bytes <= 8 * 16 * 1024);
    scene.view_projection = camera(0.);
    let stats = compare(&gpu, &mut renderers, &scene)?;
    assert!(stats[1].batch_plan_reused);
    assert!(stats[1].instance_uniform_bytes > 0); // Released records must re-upload.
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().instance_uniform_bytes, 0);
    // Singleton output still uses the normal individual binding, not a stale pool slot.
    scene.view_projection =
        glam::camera::rh::proj::directx::orthographic(-0.04, 0.04, -0.04, 0.04, 0.1, 30.)
            * Mat4::from_translation(Vec3::new(13.05, 11.55, 0.));
    let stats = compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(stats[1].visible_surfaces, 1);
    assert_eq!(stats[1].instanced_draws, 0);
    Ok(())
}

#[test]
fn diagnostic_and_shadow_membership_reuse_preserves_edits_publication_switches_and_retry()
-> anyhow::Result<()> {
    let (gpu, mut renderers) = setup()?;
    let mut scene = scene();
    scene.lighting.shadows = true;
    scene.lighting.shadow_resolution = 256;
    for renderer in &mut renderers {
        renderer.set_shadow_preparation_caching_enabled(false);
    }
    compare(&gpu, &mut renderers, &scene)?;
    for frame in 0..5 {
        scene.items[0].model *= Mat4::from_rotation_y(0.05);
        scene.items[1].material.tint[frame % 3] = 0.4 + frame as f32 * 0.1;
        let stats = compare(&gpu, &mut renderers, &scene)?;
        assert!(stats[1].batch_diagnostics_reused);
        assert!(stats[1].shadow_batch_plan_reused);
        assert_eq!(stats[1].shadow_batch_plan_rebuilds, 0);
        assert_eq!(stats[0].shadow_batch_plan_rebuilds, 1);
    }
    for edit in 0..7 {
        match edit {
            0 => scene.items[0].material.lit = false,
            1 => scene.items[2].material.texture = TextureKind::White,
            2 => {
                scene.items.remove(3);
            }
            3 => scene.items.swap(0, 64),
            4 => scene.items[4].mesh = MeshKind::Sphere,
            5 => {
                for renderer in &mut renderers {
                    renderer.upload_image(&gpu, "group-00", 1, 1, &[220, 40, 30, 128])?;
                }
            }
            _ => {
                for renderer in &mut renderers {
                    renderer.upload_image(&gpu, "group-00", 1, 1, &[80, 40, 220, 255])?;
                }
            }
        }
        let stats = compare(&gpu, &mut renderers, &scene)?;
        assert_eq!(
            stats[1].shadow_batch_plan_rebuilds, 1,
            "edit {edit} did not invalidate membership"
        );
        scene.items[7].model *= Mat4::from_rotation_y(0.02);
        compare(&gpu, &mut renderers, &scene)?;
        assert!(renderers[1].frame_stats().shadow_batch_plan_reused);
    }
    // Failure cannot publish a half-prepared grouping or a reused histogram.
    let texture = scene.items[10].material.texture.clone();
    scene.items[10].material.texture = TextureKind::Imported("missing".into());
    for renderer in &mut renderers {
        let result = capture_offscreen(&gpu, 320, 320, |target| {
            renderer.draw(&gpu, target, [320; 2], &scene)
        });
        assert!(result.is_err());
    }
    scene.items[10].material.texture = texture;
    let stats = compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(stats[1].shadow_batch_plan_rebuilds, 1);
    assert!(!stats[1].batch_diagnostics_reused);
    for enabled in [false, true, false, true] {
        renderers[1].set_batch_preparation_caching_enabled(enabled);
        scene.items[7].model *= Mat4::from_rotation_y(0.02);
        compare(&gpu, &mut renderers, &scene)?;
        scene.items[7].model *= Mat4::from_rotation_y(0.02);
        compare(&gpu, &mut renderers, &scene)?;
        assert_eq!(renderers[1].frame_stats().shadow_batch_plan_reused, enabled);
    }
    for switch in 0..5 {
        for enabled in [false, true] {
            for renderer in &mut renderers {
                match switch {
                    0 => renderer.set_shadow_batching_enabled(enabled),
                    1 => renderer.set_instancing_enabled(enabled),
                    2 => renderer.set_global_batching_enabled(enabled),
                    3 => renderer.set_state_caching_enabled(enabled),
                    _ => renderer.set_shader_graph_instancing_enabled(enabled),
                }
            }
            scene.items[7].model *= Mat4::from_rotation_y(0.02);
            compare(&gpu, &mut renderers, &scene)?;
            compare(&gpu, &mut renderers, &scene)?;
        }
    }
    // Disabled shadows must not pin a historical scene's group allocations.
    let original = scene.clone();
    scene.lighting.shadows = false;
    scene.items.truncate(2);
    compare(&gpu, &mut renderers, &scene)?;
    assert!(renderers[1].frame_stats().shadow_instance_buffer_bytes <= 8 * 16 * 1024);
    scene = original;
    scene.items[7].model *= Mat4::from_rotation_y(0.02);
    let stats = compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(stats[1].shadow_batch_plan_rebuilds, 1);
    Ok(())
}
