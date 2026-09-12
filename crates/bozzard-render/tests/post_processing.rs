use bozzard_render::*;
use glam::{Mat4, Vec3};
fn object(
    mesh: MeshKind,
    scale: [f32; 3],
    translation: [f32; 3],
    tint: [f32; 3],
    lit: bool,
) -> DrawItem {
    DrawItem {
        motion_id: 0,
        mesh,
        model: Mat4::from_translation(translation.into()) * Mat4::from_scale(scale.into()),
        material: Material {
            metallic: None,
            roughness: None,
            tint,
            lit,
            texture: TextureKind::White,
            uv_scale: [1.; 2],
            surface_overrides: Default::default(),
        },
    }
}
fn scene() -> RenderScene {
    RenderScene {
        particles: Vec::new(),
        fog: Default::default(),
        gi: None,
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: Lighting {
            shadows: false,
            sun_direction: [0., 0., 1.],
            sun_intensity: 20.,
            ambient_intensity: 0.,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::perspective(0.85, 1., 0.1, 50.)
            * glam::camera::rh::view::look_at_mat4(
                Vec3::new(3., 2.5, 4.),
                Vec3::new(0., 0.4, 0.),
                Vec3::Y,
            ),
        items: vec![
            object(
                MeshKind::Cube,
                [8., 0.1, 8.],
                [0., -0.05, 0.],
                [0.4; 3],
                false,
            ),
            object(
                MeshKind::Cube,
                [1.; 3],
                [0., 0.5, 0.],
                [0.55, 0.3, 0.15],
                false,
            ),
            object(
                MeshKind::Quad,
                [0.5; 3],
                [0.8, 0.5, 0.6],
                [1., 0.3, 0.05],
                true,
            ),
        ],
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
fn changed(a: &Frame, b: &Frame) -> usize {
    a.rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > 2))
        .count()
}
#[test]
fn gpu_post_processing_color_depth_resize_bypass_and_encoding() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let size = [129, 97];
    let baseline = capture(&gpu, &mut renderer, &scene, size, false)?;
    let raw = capture(&gpu, &mut renderer, &scene, size, true)?;
    scene.display.ambient_occlusion.enabled = true;
    scene.display.ambient_occlusion.intensity = 2.;
    let ao = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        changed(&baseline, &ao) > 30,
        "AO must ground the box/floor contact"
    );
    // FXAA may redistribute edge brightness after AO changes contrast; total energy must fall.
    let energy = |frame: &Frame| {
        frame
            .rgba
            .chunks_exact(4)
            .map(|p| p[..3].iter().map(|v| *v as u64).sum::<u64>())
            .sum::<u64>()
    };
    assert!(
        energy(&ao) < energy(&baseline),
        "AO must reduce scene brightness"
    );
    diagnostic("ao-before", &baseline)?;
    diagnostic("ao-after", &ao)?;
    scene.display.ambient_occlusion.intensity = 0.;
    scene.display.heat_distortion.enabled = true;
    scene.display.heat_distortion.strength = 0.;
    assert_eq!(
        capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
        baseline.rgba,
        "zero strength must bypass depth passes"
    );
    scene.display.tone_mapper = ToneMapper::Filmic;
    let filmic = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(changed(&baseline, &filmic) > 100);
    scene.display.color_grading.saturation = 0.;
    let mono = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        mono.rgba
            .chunks_exact(4)
            .all(|p| p[0].abs_diff(p[1]) <= 1 && p[1].abs_diff(p[2]) <= 1),
        "zero saturation must be monochrome"
    );
    scene.display.color_grading.saturation = 1.1;
    scene.display.color_grading.temperature = 0.3;
    scene.display.bloom = BloomSettings {
        enabled: true,
        intensity: 0.4,
        anamorphic: 0.8,
        ..Default::default()
    };
    scene.display.ambient_occlusion.intensity = 1.4;
    scene.display.heat_distortion.strength = 20.;
    scene.display.grain.intensity = 0.15;
    scene.display.vignette.intensity = 0.6;
    scene.display.time_seconds = 1.25;
    let combined = capture(&gpu, &mut renderer, &scene, size, false)?;
    diagnostic("combined", &combined)?;
    assert!(combined.rgba.chunks_exact(4).all(|p| p[3] == 255));
    assert_eq!(
        capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
        combined.rgba,
        "same visual time must be deterministic"
    );
    assert_eq!(
        capture(&gpu, &mut renderer, &scene, size, true)?.rgba,
        raw.rgba,
        "raw diagnostics must bypass every effect"
    );
    let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("post sRGB parity"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut hardware = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
    hardware.draw(&gpu, &output.create_view(&Default::default()), size, &scene)?;
    let encoded = read_texture(&gpu, &output, size[0], size[1])?;
    assert!(
        encoded
            .rgba
            .iter()
            .zip(&combined.rgba)
            .all(|(a, b)| a.abs_diff(*b) <= 2),
        "software/hardware sRGB mismatch"
    );
    scene.display.time_seconds += 0.5;
    assert!(
        changed(
            &combined,
            &capture(&gpu, &mut renderer, &scene, size, false)?
        ) > 30,
        "grain must animate"
    );
    for size in [[1, 1], [1, 19], [27, 1], [47, 31], [129, 97]] {
        let full = capture(&gpu, &mut renderer, &scene, size, false)?;
        let mut fresh = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(
            full.rgba,
            capture(&gpu, &mut fresh, &scene, size, false)?.rgba,
            "resize must rebind every effect texture"
        );
        scene.display.heat_distortion.enabled = false;
        scene.display.ambient_occlusion.enabled = false;
        let _ = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.display.heat_distortion.enabled = true;
        scene.display.ambient_occlusion.enabled = true;
        assert_eq!(
            full.rgba,
            capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
            "toggle must rebind bloom source"
        );
    }
    // Completely planar geometry must not self-occlude.
    scene.items = vec![object(MeshKind::Quad, [20.; 3], [0.; 3], [0.4; 3], false)];
    scene.display = Default::default();
    let flat = capture(&gpu, &mut renderer, &scene, [64, 64], false)?;
    scene.display.ambient_occlusion.enabled = true;
    assert_eq!(
        changed(
            &flat,
            &capture(&gpu, &mut renderer, &scene, [64, 64], false)?
        ),
        0
    );
    // Filmic grading must retain detail in dark scenes rather than clipping it away.
    scene.display = Default::default();
    scene.items[0].material.tint = [0.01; 3];
    let dark = capture(&gpu, &mut renderer, &scene, [64, 64], false)?;
    scene.display.tone_mapper = ToneMapper::Filmic;
    scene.display.color_grading.contrast = 1.05;
    let graded_dark = capture(&gpu, &mut renderer, &scene, [64, 64], false)?;
    let center = (32 * 64 + 32) * 4;
    assert!(
        graded_dark.rgba[center] as f32 >= dark.rgba[center] as f32 * 0.75,
        "filmic contrast crushed shadow detail"
    );
    // Extreme but valid controls must still produce finite bounded display values.
    scene.display.exposure_ev = 16.;
    scene.display.color_grading.gain = [4.; 3];
    scene.display.color_grading.gamma = [0.25; 3];
    let _ = capture(&gpu, &mut renderer, &scene, [8, 8], false)?;
    Ok(())
}
#[test]
fn gpu_anamorphic_bloom_spreads_horizontally_and_heat_is_local() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    scene.view_projection =
        glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.));
    scene.items = vec![object(MeshKind::Quad, [0.1; 3], [0.; 3], [1.; 3], true)];
    let size = [128, 128];
    let baseline = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.bloom = BloomSettings {
        enabled: true,
        intensity: 1.,
        anamorphic: 1.,
        ..Default::default()
    };
    let streaks = capture(&gpu, &mut renderer, &scene, size, false)?;
    diagnostic("anamorphic", &streaks)?;
    let mut moments = [0.; 2];
    for y in 0..128 {
        for x in 0..128 {
            let i = (y * 128 + x) * 4;
            let weight = streaks.rgba[i].saturating_sub(baseline.rgba[i]) as f64;
            moments[0] += weight * (x as f64 - 63.5).powi(2);
            moments[1] += weight * (y as f64 - 63.5).powi(2);
        }
    }
    assert!(
        moments[0] > moments[1] * 1.25,
        "bloom not stretched: {moments:?}"
    );
    scene.display.bloom.enabled = false;
    scene.items[0].model =
        Mat4::from_translation(Vec3::new(0., -0.4, 0.5)) * Mat4::from_scale(Vec3::splat(0.5));
    let mut background = object(MeshKind::Quad, [4.; 3], [0., 0., -0.5], [0.4; 3], false);
    background.material.texture = TextureKind::ProceduralChecker;
    background.material.uv_scale = [4.; 2];
    scene.items.push(background);
    let off = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.heat_distortion = HeatDistortion {
        enabled: true,
        strength: 30.,
        rise: 0.3,
        threshold: 1.,
        ..Default::default()
    };
    let heat0 = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.time_seconds = 0.5;
    let heat1 = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        changed(&off, &heat0) > 5,
        "heat must distort bright-source neighborhood"
    );
    assert!(
        changed(&heat0, &heat1) > 5,
        "heat must animate without grain"
    );
    // The top quarter lies beyond the authored plume, so its pixels must be identical.
    assert_eq!(&off.rgba[..128 * 24 * 4], &heat1.rgba[..128 * 24 * 4]);
    scene.items.remove(0);
    let cold = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.heat_distortion.enabled = false;
    assert_eq!(
        cold.rgba,
        capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
        "no HDR source means no distortion"
    );
    Ok(())
}

fn diagnostic(name: &str, frame: &Frame) -> anyhow::Result<()> {
    if let Some(directory) = std::env::var_os("BOZZARD_POST_CAPTURE_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory)?;
        frame.write_ppm(&directory.join(format!("{name}.ppm")))?;
    }
    Ok(())
}
