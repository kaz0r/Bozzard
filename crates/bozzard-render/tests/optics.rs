use bozzard_render::*;
use glam::{Mat4, Vec3};
fn scene() -> RenderScene {
    RenderScene {
        view_projection: glam::camera::rh::proj::directx::orthographic(-4., 4., -3., 3., 0.1, 30.),
        items: vec![],
        lighting: Lighting {
            shadows: false,
            sun_intensity: 0.,
            ambient_intensity: 0.,
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
fn quad(x: f32, z: f32, size: [f32; 2], color: [f32; 3]) -> DrawItem {
    DrawItem {
        mesh: MeshKind::Quad,
        model: Mat4::from_translation(Vec3::new(x, 0., -z))
            * Mat4::from_scale(Vec3::new(size[0], size[1], 1.)),
        material: Material {
            tint: color,
            lit: false,
            texture: TextureKind::White,
            uv_scale: [1.; 2],
            surface_overrides: Default::default(),
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
fn pixel(frame: &Frame, x: u32, y: u32) -> &[u8] {
    let i = ((y * frame.width + x) * 4) as usize;
    &frame.rgba[i..i + 4]
}
fn mean(frame: &Frame) -> f32 {
    frame.rgba.chunks_exact(4).map(|p| p[0] as f32).sum::<f32>()
        / (frame.width * frame.height) as f32
}
fn changed(a: &Frame, b: &Frame) -> usize {
    a.rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > 2))
        .count()
}
fn diagnostic(name: &str, frame: &Frame) -> anyhow::Result<()> {
    if let Some(dir) = std::env::var_os("BOZZARD_OPTICS_CAPTURE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        frame.write_ppm(&dir.join(format!("{name}.ppm")))?;
    }
    Ok(())
}
#[test]
fn auto_exposure_gpu_meter_adaptation_pause_reset_and_bypass() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let size = [65, 49];
    scene.items = vec![quad(0., 5., [20., 20.], [0.18; 3])];
    scene.display.auto_exposure = AutoExposure {
        enabled: true,
        min_ev: -8.,
        max_ev: 8.,
        speed_up: 1.,
        speed_down: 4.,
        center_weight: 0.,
        ..Default::default()
    };
    let gray = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        (mean(&gray) - 118.).abs() < 3.,
        "gray target {}",
        mean(&gray)
    );
    scene.items[0].material.tint = [0.018; 3];
    scene.display.time_seconds = 0.1;
    let start = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        mean(&start) < 65. && mean(&start) > 30.,
        "dark adaptation must start gradually: {}",
        mean(&start)
    );
    assert_eq!(
        start.rgba,
        capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
        "pause must hold exposure"
    );
    scene.display.time_seconds = 0.6;
    let middle = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.time_seconds = 5.;
    let settled = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(mean(&start) < mean(&middle) && mean(&middle) < mean(&settled));
    assert!((mean(&settled) - 118.).abs() < 4.);
    diagnostic("exposure-dark-start", &start)?;
    diagnostic("exposure-dark-settled", &settled)?;
    scene.items[0].material.tint = [0.18; 3];
    scene.display.time_seconds = 5.1;
    let bright = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.time_seconds = 6.;
    let recovered = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(mean(&bright) > mean(&recovered) + 10.);
    assert!((mean(&recovered) - 118.).abs() < 5.);
    renderer.reset_display_history();
    assert_eq!(
        gray.rgba,
        capture(&gpu, &mut renderer, &scene, size, false)?.rgba
    );
    scene.display.time_seconds = 0.;
    scene.display.auto_exposure.min_ev = 1.;
    scene.display.auto_exposure.max_ev = 1.;
    let doubled = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!((mean(&doubled) - 162.).abs() < 3.);
    scene.display.auto_exposure.strength = 0.;
    let zero = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.auto_exposure.enabled = false;
    assert_eq!(
        zero.rgba,
        capture(&gpu, &mut renderer, &scene, size, false)?.rgba
    );
    let raw = capture(&gpu, &mut renderer, &scene, size, true)?;
    scene.display.auto_exposure.enabled = true;
    scene.display.auto_exposure.strength = 1.;
    assert_eq!(
        raw.rgba,
        capture(&gpu, &mut renderer, &scene, size, true)?.rgba
    );
    // Equal elapsed time gives the same adaptation regardless of draw frequency.
    scene.display.auto_exposure = AutoExposure {
        enabled: true,
        min_ev: -8.,
        max_ev: 8.,
        speed_up: 1.,
        ..Default::default()
    };
    scene.display.time_seconds = 0.;
    scene.items[0].material.tint = [0.18; 3];
    let _ = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.items[0].material.tint = [0.018; 3];
    scene.display.time_seconds = 1.;
    let one_step = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.time_seconds = 0.;
    scene.items[0].material.tint = [0.18; 3];
    let _ = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.items[0].material.tint = [0.018; 3];
    for step in 1..=10 {
        scene.display.time_seconds = step as f32 * 0.1;
        let _ = capture(&gpu, &mut renderer, &scene, size, false)?;
    }
    let ten_steps = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        one_step
            .rgba
            .iter()
            .zip(&ten_steps.rgba)
            .all(|(a, b)| a.abs_diff(*b) <= 1)
    );
    scene.items[0].material.tint = [0.18; 3];
    scene.display.time_seconds = 0.;
    // Tiny extreme highlights are trimmed, instead of making the rest of the frame dark.
    scene.display.auto_exposure = AutoExposure {
        enabled: true,
        min_ev: -8.,
        max_ev: 8.,
        center_weight: 0.,
        ..Default::default()
    };
    scene.items.push(quad(0., 4., [0.1, 0.1], [1.; 3]));
    let spark = capture(&gpu, &mut renderer, &scene, [257, 193], false)?;
    assert!(pixel(&spark, 25, 25)[0].abs_diff(pixel(&gray, 5, 5)[0]) <= 2);
    // History survives resizing; a frozen time must not cause a jump.
    scene.display.time_seconds = 1.;
    for size in [[1, 1], [1, 17], [19, 1], [97, 73]] {
        let frame = capture(&gpu, &mut renderer, &scene, size, false)?;
        assert_eq!(
            frame.rgba,
            capture(&gpu, &mut renderer, &scene, size, false)?.rgba
        );
    }
    Ok(())
}
#[test]
fn bokeh_gpu_focus_foreground_background_rebinding_and_encoding() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let size = [321, 241];
    scene.items = vec![
        quad(-2., 5., [0.12, 3.], [1.; 3]),
        quad(0., 15., [0.12, 3.], [1.; 3]),
        quad(2., 1.5, [0.12, 3.], [1.; 3]),
    ];
    let sharp = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.depth_of_field = DepthOfField {
        enabled: true,
        focus_distance: 4.9,
        focal_length_mm: 200.,
        aperture: 0.7,
        max_blur_radius: 32.,
    };
    let blur = capture(&gpu, &mut renderer, &scene, size, false)?;
    diagnostic("focus-before", &sharp)?;
    diagnostic("focus-after", &blur)?;
    assert!(
        pixel(&blur, 80, 120)[0].abs_diff(pixel(&sharp, 80, 120)[0]) <= 2,
        "focused stripe must stay sharp"
    );
    assert!(
        pixel(&blur, 160, 120)[0] < pixel(&sharp, 160, 120)[0] - 15,
        "background must soften"
    );
    assert!(
        pixel(&blur, 240, 120)[0] < pixel(&sharp, 240, 120)[0] - 15,
        "foreground must soften"
    );
    assert!(
        pixel(&blur, 164, 120)[0] > pixel(&sharp, 164, 120)[0] + 10,
        "background bokeh must spread"
    );
    assert!(
        pixel(&blur, 244, 120)[0] > pixel(&sharp, 244, 120)[0] + 10,
        "foreground bokeh must spread"
    );
    // Rack focus from the middle stripe to the distant stripe.
    scene.display.depth_of_field.focus_distance = 14.9;
    let far = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(pixel(&far, 160, 120)[0].abs_diff(pixel(&sharp, 160, 120)[0]) <= 2);
    assert!(changed(&far, &blur) > 100);
    // A sharp foreground occluder must not pick up the bright background's blur.
    scene.display.depth_of_field.focus_distance = 4.9;
    scene.items = vec![
        quad(0., 15., [20., 20.], [1.; 3]),
        quad(0., 5., [1., 4.], [0.1, 0., 0.]),
    ];
    let occluded = capture(&gpu, &mut renderer, &scene, size, false)?;
    assert!(
        pixel(&occluded, 160, 120)[1] < 3,
        "background bled into focus"
    );
    scene.display.depth_of_field.max_blur_radius = 0.;
    let zero = capture(&gpu, &mut renderer, &scene, size, false)?;
    scene.display.depth_of_field.enabled = false;
    assert_eq!(
        zero.rgba,
        capture(&gpu, &mut renderer, &scene, size, false)?.rgba
    );
    let raw = capture(&gpu, &mut renderer, &scene, size, true)?;
    scene.display.depth_of_field.enabled = true;
    scene.display.depth_of_field.max_blur_radius = 32.;
    assert_eq!(
        raw.rgba,
        capture(&gpu, &mut renderer, &scene, size, true)?.rgba
    );
    for size in [[1, 1], [1, 17], [19, 1], [97, 73]] {
        scene.display.auto_exposure.enabled = true;
        scene.display.bloom.enabled = true;
        scene.display.ambient_occlusion.enabled = true;
        let full = capture(&gpu, &mut renderer, &scene, size, false)?;
        let mut fresh = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        assert_eq!(
            full.rgba,
            capture(&gpu, &mut fresh, &scene, size, false)?.rgba,
            "resize retained stale source"
        );
        scene.display.ambient_occlusion.enabled = false;
        let _ = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.display.ambient_occlusion.enabled = true;
        assert_eq!(
            full.rgba,
            capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
            "AO toggle retained stale source"
        );
        scene.display.depth_of_field.enabled = false;
        let _ = capture(&gpu, &mut renderer, &scene, size, false)?;
        scene.display.depth_of_field.enabled = true;
        assert_eq!(
            full.rgba,
            capture(&gpu, &mut renderer, &scene, size, false)?.rgba,
            "DOF toggle retained stale bloom"
        );
    }
    // Hardware and software sRGB targets share the same exposure and lens results.
    let software = capture(&gpu, &mut renderer, &scene, [97, 73], false)?;
    let mut hardware = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 97,
            height: 73,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    hardware.draw(
        &gpu,
        &texture.create_view(&Default::default()),
        [97, 73],
        &scene,
    )?;
    let encoded = read_texture(&gpu, &texture, 97, 73)?;
    assert!(
        software
            .rgba
            .iter()
            .zip(encoded.rgba)
            .all(|(a, b)| a.abs_diff(b) <= 1)
    );
    Ok(())
}
