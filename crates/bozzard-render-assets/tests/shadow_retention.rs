use anyhow::{Result, ensure};
use bozzard_render::*;
use glam::{Mat4, Vec3};
#[path = "../examples/support/scene.rs"]
mod support;

#[test]
fn independently_animated_shadow_maps_match_uncached_frames() -> Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut reference = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    reference.set_state_caching_enabled(false);
    let mut cached = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let initial = support::fixture();
    let mut scene = initial.clone();
    for step in 0..20 {
        match step {
            2 => scene.lights[0].position[0] += 0.5,
            3 => scene.lights[1].direction[0] = 0.1,
            4 => scene.items[0].model *= Mat4::from_translation(Vec3::X),
            5 => scene.items[0].model = Mat4::from_translation(Vec3::new(20., 0., -7.)),
            6 => scene.items[0].model *= Mat4::from_scale(Vec3::new(-1., 1., 1.)),
            7 => scene.lights[0].shadows = None,
            8 => scene.lights.swap(1, 3),
            9 => scene.lights[2].range *= 0.5,
            10 => scene.items.clear(),
            11 => scene = initial.clone(),
            12 => scene.items[0].material.texture = TextureKind::Imported("alpha".into()),
            13 => {} // Replace the same asset ID after retained maps have been populated.
            14 => scene.lights[1].shadows.as_mut().unwrap().normal_bias += 0.02,
            15 => scene.display.temporal_aa.enabled = true,
            16 => scene.display.reflections.enabled = true,
            17 => {
                scene.display.temporal_aa.enabled = false;
                scene.display.motion_blur.enabled = true;
            }
            18 => scene = initial.clone(),
            19 => scene.items[0].mesh = MeshKind::Sphere,
            _ => {}
        }
        if step == 12 || step == 13 {
            for renderer in [&mut cached, &mut reference] {
                renderer.upload_image(
                    &gpu,
                    "alpha",
                    1,
                    1,
                    &[255, 255, 255, if step == 12 { 255 } else { 0 }],
                )?;
            }
        }
        // Distinct renderers maintain separate histories, including animated frames.
        let before = capture_offscreen(&gpu, 160, 100, |target| {
            reference.draw(&gpu, target, [160, 100], &scene)
        })?;
        let after = capture_offscreen(&gpu, 160, 100, |target| {
            cached.draw(&gpu, target, [160, 100], &scene)
        })?;
        ensure!(
            before.rgba == after.rgba,
            "retained map mismatch at step {step}"
        );
        let stats = cached.frame_stats();
        if step == 0 {
            ensure!(
                stats.auxiliary_targets == 0 && stats.geometry_allocated_bytes == 0,
                "unused buffers were allocated"
            );
        }
        if step == 15 {
            ensure!(
                stats.auxiliary_targets == 3 && stats.geometry_allocated_bytes == 160 * 100 * 24,
                "TAA buffers were not allocated"
            );
        }
        if step == 18 {
            ensure!(
                stats.auxiliary_targets == 0,
                "disabled effects still use auxiliary attachments"
            );
        }
        if step == 1 {
            ensure!(stats.shadow_maps_rendered == 0, "idle maps were redrawn");
        }
        if step == 2 {
            ensure!(
                stats.shadow_maps_rendered == 6,
                "moving one point light must only redraw its six faces: {stats:?}"
            );
        }
        if step == 3 {
            ensure!(
                stats.shadow_maps_rendered == 1,
                "moving one spot must only redraw one map: {stats:?}"
            );
        }
        if step == 4 {
            ensure!(
                stats.shadow_maps_rendered < reference.frame_stats().shadow_maps_rendered,
                "moving one caster invalidated every map"
            );
        }
    }
    Ok(())
}

#[test]
fn sphere_shadow_uses_the_rendered_sphere_geometry() -> Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = support::fixture();
    scene.items.truncate(1);
    scene.items[0].mesh = MeshKind::Sphere;
    scene.lights.clear();
    capture_offscreen(&gpu, 80, 50, |target| {
        renderer.draw(&gpu, target, [80, 50], &scene)
    })?;
    let stats = renderer.frame_stats();
    ensure!(
        stats.color_triangles > 12 && stats.shadow_triangles == stats.color_triangles,
        "sphere casts different geometry than its visible surface: {stats:?}"
    );
    Ok(())
}
