use bozzard_render::*;
use glam::{Mat4, Vec3};
fn scene() -> RenderScene {
    RenderScene {
        particles: Vec::new(),
        fog: Default::default(),
        gi: None,
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        display: DisplaySettings {
            tone_mapping: false,
            volumetric_fog: VolumetricFog {
                enabled: true,
                density: 0.1,
                albedo: [1.; 3],
                anisotropy: 0.,
                height_falloff: 0.,
                start_distance: 0.,
                max_distance: 10.,
                noise_amount: 0.,
                ambient: 0.,
                ..Default::default()
            },
            ..Default::default()
        },
        lighting: Lighting {
            shadows: false,
            sun_direction: [0., 0., -1.],
            sun_color: [1.; 3],
            sun_intensity: 4. * std::f32::consts::PI,
            ambient_intensity: 0.,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-4., 4., -3., 3., 0.1, 10.1),
        items: vec![],
    }
}
fn object(mesh: MeshKind, scale: [f32; 3], position: [f32; 3]) -> DrawItem {
    DrawItem {
        motion_id: 0,
        mesh,
        model: Mat4::from_translation(position.into()) * Mat4::from_scale(scale.into()),
        material: Material {
            metallic: None,
            roughness: None,
            surface_overrides: Default::default(),
            tint: [0.15; 3],
            uv_scale: [1.; 2],
            texture: TextureKind::White,
            lit: false,
        },
    }
}
fn capture(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    scene: &RenderScene,
    size: [u32; 2],
    raw: bool,
) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, size[0], size[1], |target| {
        if raw {
            renderer.draw_linear(gpu, target, size, scene)
        } else {
            renderer.draw(gpu, target, size, scene)
        }
    })
}
fn srgb(x: f32) -> u8 {
    (255.
        * if x <= 0.0031308 {
            x * 12.92
        } else {
            1.055 * x.powf(1. / 2.4) - 0.055
        })
    .round()
    .clamp(0., 255.) as u8
}
fn center(frame: &Frame) -> &[u8] {
    let i = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
    &frame.rgba[i..i + 4]
}
fn difference(a: &Frame, b: &Frame) -> usize {
    a.rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > 2))
        .count()
}
// Count shadow changes in the medium where the underlying surface is unchanged.
fn scattering_shadow_difference(
    open: &Frame,
    blocked: &Frame,
    surface_open: &Frame,
    surface_blocked: &Frame,
) -> usize {
    open.rgba
        .chunks_exact(4)
        .zip(blocked.rgba.chunks_exact(4))
        .zip(
            surface_open
                .rgba
                .chunks_exact(4)
                .zip(surface_blocked.rgba.chunks_exact(4)),
        )
        .filter(|((a, b), (c, d))| {
            (0..3).any(|i| a[i].abs_diff(b[i]) > 2) && (0..3).all(|i| c[i].abs_diff(d[i]) <= 2)
        })
        .count()
}
fn diagnostic(name: &str, frame: &Frame) -> anyhow::Result<()> {
    if let Some(dir) = std::env::var_os("BOZZARD_VOLUME_CAPTURE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        frame.write_ppm(&dir.join(format!("{name}.ppm")))?;
    }
    Ok(())
}
#[test]
fn volumetric_gpu_matches_homogeneous_transport_and_respects_depth() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let clear = [0.018, 0.025, 0.04];
    for (distance, density) in [(10_f32, 0.1_f32), (4., 0.25), (10., 0.001)] {
        scene.display.volumetric_fog.max_distance = distance;
        scene.display.volumetric_fog.density = density;
        for steps in [16, 48, 96] {
            scene.display.volumetric_fog.steps = steps;
            let frame = capture(&gpu, &mut renderer, &scene, [33, 25], false)?;
            let transmittance = (-distance * density).exp();
            let expected = clear.map(|value| srgb(value * transmittance + 1. - transmittance));
            assert!(
                center(&frame)[..3]
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 2),
                "homogeneous transport: {:?} vs {expected:?}",
                center(&frame)
            );
        }
    }
    scene.display.volumetric_fog.density = 0.1;
    scene.display.volumetric_fog.max_distance = 10.;
    scene.lighting.sun_intensity = 0.;
    let absorption = capture(&gpu, &mut renderer, &scene, [33, 25], false)?;
    let expected = clear.map(|v| srgb(v * (-1_f32).exp()));
    assert!(
        center(&absorption)[..3]
            .iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 2)
    );
    scene.lighting.sun_intensity = 4. * std::f32::consts::PI;
    scene.display.volumetric_fog.albedo = [0.; 3];
    assert_eq!(
        absorption.rgba,
        capture(&gpu, &mut renderer, &scene, [33, 25], false)?.rgba,
        "zero albedo absorbs without scattering"
    );
    scene.display.volumetric_fog.albedo = [1.; 3];
    // Thin foregrounds leave some background pixels without a matching half-resolution
    // depth. Their scattering must still match the analytic result, without dark outlines.
    scene.items = vec![object(MeshKind::Cube, [0.7, 2.7, 0.2], [0.13, 0.17, -2.])];
    scene.display.volumetric_fog.enabled = false;
    let silhouette_source = capture(&gpu, &mut renderer, &scene, [53, 39], false)?;
    scene.display.volumetric_fog.enabled = true;
    let silhouette = capture(&gpu, &mut renderer, &scene, [53, 39], false)?;
    let clear_srgb = clear.map(srgb);
    let expected = clear.map(|v| srgb(v * (-1_f32).exp() + 1. - (-1_f32).exp()));
    for (source, fogged) in silhouette_source
        .rgba
        .chunks_exact(4)
        .zip(silhouette.rgba.chunks_exact(4))
    {
        if source[..3] == clear_srgb {
            assert!(
                (0..3).all(|i| fogged[i].abs_diff(expected[i]) <= 2),
                "background silhouette lost scattering: {fogged:?} vs {expected:?}"
            );
        }
    }
    scene.items = vec![object(MeshKind::Quad, [20.; 3], [0., 0., -2.])];
    let geometry = capture(&gpu, &mut renderer, &scene, [33, 25], false)?;
    let t = (-0.1_f32 * 1.9).exp();
    let expected = srgb(0.15 * t + 1. - t);
    assert!(
        center(&geometry)[..3]
            .iter()
            .all(|v| v.abs_diff(expected) <= 2),
        "fog marched behind opaque depth: {:?}",
        center(&geometry)
    );
    scene.items[0].model =
        Mat4::from_translation(Vec3::new(0., 0., -0.2)) * Mat4::from_scale(Vec3::splat(20.));
    scene.display.volumetric_fog.start_distance = 0.25;
    let near = capture(&gpu, &mut renderer, &scene, [33, 25], false)?;
    scene.display.volumetric_fog.enabled = false;
    assert_eq!(
        near.rgba,
        capture(&gpu, &mut renderer, &scene, [33, 25], false)?.rgba,
        "fog must not leak through close foregrounds"
    );
    let raw = capture(&gpu, &mut renderer, &scene, [33, 25], true)?;
    scene.display.volumetric_fog.enabled = true;
    assert_eq!(
        raw.rgba,
        capture(&gpu, &mut renderer, &scene, [33, 25], true)?.rgba,
        "raw diagnostics bypass volumetrics"
    );
    Ok(())
}
#[test]
fn volumetric_gpu_shadowed_sun_point_spot_and_live_rebinding() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    scene.display.tone_mapping = true;
    scene.display.volumetric_fog.density = 0.12;
    scene.items = vec![
        object(MeshKind::Quad, [16.; 3], [0., 0., -7.]),
        object(MeshKind::Cube, [0.6, 5., 0.6], [0., 0., -3.]),
    ];
    // Only lit opaque geometry casts shadows, matching the surface renderer.
    for item in &mut scene.items {
        item.material.lit = true;
    }
    scene.lighting.sun_direction = [1., 0., 0.];
    let size = [97, 73];
    let unshadowed = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.lighting.shadows = true;
    scene.lighting.shadow_resolution = 512;
    let sun = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.volumetric_fog.enabled = false;
    let surface_blocked = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.lighting.shadows = false;
    let surface_open = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.lighting.shadows = true;
    scene.display.volumetric_fog.enabled = true;
    assert!(
        scattering_shadow_difference(&unshadowed, &sun, &surface_open, &surface_blocked) > 50,
        "sun occluder did not carve light shafts"
    );
    diagnostic("sun-unshadowed", &unshadowed)?;
    diagnostic("sun-shafts", &sun)?;
    scene.lighting.sun_intensity = 0.;
    let light = LocalLight {
        directional: false,
        position: [2., 0., -3.],
        direction: [-1., 0., 0.],
        color: [1., 0.35, 0.08],
        intensity: 80.,
        range: 9.,
        spot_angles: None,
        shadows: None,
    };
    for spot in [false, true] {
        scene.lights = vec![LocalLight {
            spot_angles: spot.then_some([25., 50.]),
            ..light
        }];
        let open = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.lights[0].shadows = Some(Default::default());
        let blocked = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.display.volumetric_fog.enabled = false;
        let surface_blocked = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.lights[0].shadows = None;
        let surface_open = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.lights[0].shadows = Some(Default::default());
        scene.display.volumetric_fog.enabled = true;
        assert!(
            scattering_shadow_difference(&open, &blocked, &surface_open, &surface_blocked) > 20,
            "local shadow did not block scattering (spot={spot})"
        );
        diagnostic(if spot { "spot-shafts" } else { "point-shafts" }, &blocked)?;
        // Zero-strength/removed lights and shadow-array resizing must not retain the previous light.
        scene.lights[0].intensity = 0.;
        let zero = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.lights.clear();
        assert_eq!(
            zero.rgba,
            capture(&gpu, &mut renderer, &scene, size, false)?.rgba
        );
    }
    scene.lights = vec![
        LocalLight {
            shadows: Some(Default::default()),
            ..light
        },
        LocalLight {
            position: [-2., 1., -4.],
            color: [0.1, 0.4, 1.],
            spot_angles: Some([15., 35.]),
            shadows: Some(Default::default()),
            direction: [0., 0., 1.],
            ..light
        },
    ];
    let original = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.lights.reverse();
    let reordered = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        original
            .rgba
            .iter()
            .zip(&reordered.rgba)
            .all(|(a, b)| a.abs_diff(*b) <= 1)
    );
    scene.display.volumetric_fog.noise_amount = 1.;
    scene.display.time_seconds = 2.;
    let noise = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert_eq!(
        noise.rgba,
        capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
        "same fog time must be deterministic"
    );
    scene.display.time_seconds = 6.;
    assert!(
        difference(&noise, &capture(&gpu, &mut renderer, &scene, size, false)?) > 30,
        "wind did not animate density"
    );
    for size in [[1, 1], [1, 17], [19, 1], [47, 31], [97, 73]] {
        scene.display.bloom.enabled = true;
        scene.display.ambient_occlusion.enabled = true;
        let full = capture(&gpu, &mut renderer, &scene, size, false)?;
        let mut fresh = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(
            full.rgba,
            capture(&gpu, &mut fresh, &scene, size, false)?.rgba,
            "resize left stale volumetric targets"
        );
        scene.display.ambient_occlusion.enabled = false;
        let _ = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.display.ambient_occlusion.enabled = true;
        assert_eq!(
            full.rgba,
            capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
            "upstream effect toggle left stale fog source"
        );
        scene.display.volumetric_fog.enabled = false;
        let off = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.display.volumetric_fog.enabled = true;
        scene.display.volumetric_fog.density = 0.;
        assert_eq!(
            off.rgba,
            capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
            "zero density must bypass"
        );
        scene.display.volumetric_fog.density = 0.12;
        assert_eq!(
            full.rgba,
            capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
            "fog toggle left stale bloom source"
        );
    }
    Ok(())
}
