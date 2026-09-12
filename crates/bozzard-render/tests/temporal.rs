use bozzard_render::*;
use glam::{Mat4, Vec3};
fn scene() -> RenderScene {
    RenderScene {
        particles: vec![],
        view_projection: glam::camera::rh::proj::directx::orthographic(-3., 3., -2., 2., 0.1, 30.),
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
fn item(id: u64, position: Vec3, scale: Vec3, color: [f32; 3]) -> DrawItem {
    DrawItem {
        motion_id: id,
        model: Mat4::from_translation(position) * Mat4::from_scale(scale),
        mesh: MeshKind::Cube,
        material: Material {
            metallic: None,
            roughness: None,
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
) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, size[0], size[1], |target| {
        renderer.draw(gpu, target, size, scene)
    })
}
fn changed(a: &Frame, b: &Frame) -> usize {
    a.rgba
        .chunks_exact(4)
        .zip(b.rgba.chunks_exact(4))
        .filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > 3))
        .count()
}
fn diagnostic(name: &str, frame: &Frame) -> anyhow::Result<()> {
    if let Some(path) = std::env::var_os("BOZZARD_TEMPORAL_CAPTURE_DIR") {
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&path)?;
        frame.write_ppm(&path.join(format!("{name}.ppm")))?;
    }
    Ok(())
}
#[test]
fn paused_text_edits_refresh_temporal_history() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let size = [320, 240];
    scene.display.temporal_aa.enabled = true;
    let empty = capture(&gpu, &mut renderer, &scene, size)?;
    let mut label = item(0, Vec3::new(-2., 1., -3.), Vec3::ONE, [1., 0.1, 0.05]);
    label.mesh = MeshKind::Text(TextMesh {
        text: "Hello!".into(),
        ..Default::default()
    });
    label.material.texture = TextureKind::Text;
    scene.items.push(label);
    let original = capture(&gpu, &mut renderer, &scene, size)?;
    assert!(
        changed(&empty, &original) > 100,
        "text must render with TAA enabled"
    );
    assert_eq!(
        original.rgba,
        capture(&gpu, &mut renderer, &scene, size)?.rgba
    );
    if let MeshKind::Text(text) = &mut scene.items[0].mesh {
        text.text = "World?".into();
    }
    let edited = capture(&gpu, &mut renderer, &scene, size)?;
    assert!(
        changed(&original, &edited) > 100,
        "paused text edits must refresh history"
    );
    assert_eq!(
        edited.rgba,
        capture(&gpu, &mut renderer, &scene, size)?.rgba
    );
    if let MeshKind::Text(text) = &mut scene.items[0].mesh {
        text.opacity = 0.;
    }
    let hidden = capture(&gpu, &mut renderer, &scene, size)?;
    assert!(
        changed(&edited, &hidden) > 100,
        "paused opacity edits must refresh history"
    );
    assert!(
        changed(&empty, &hidden) < 10,
        "hidden text must not leave a history trail"
    );
    Ok(())
}
#[test]
fn material_reflections_trace_hits_preserve_misses_and_fade_rough_surfaces() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let size = [256, 192];
    scene.view_projection = glam::camera::rh::proj::directx::perspective(0.9, 4. / 3., 0.1, 40.)
        * glam::camera::rh::view::look_at_mat4(
            Vec3::new(0., 2., 5.),
            Vec3::new(0., 0.5, -1.),
            Vec3::Y,
        );
    let mut floor = item(
        1,
        Vec3::new(0., -0.1, -2.),
        Vec3::new(12., 0.2, 12.),
        [0.1; 3],
    );
    floor.material.lit = true;
    floor.material.roughness = Some(0.08);
    floor.material.metallic = Some(0.7);
    scene.items = vec![
        floor,
        item(
            2,
            Vec3::new(0., 1., -2.),
            Vec3::new(1., 2., 1.),
            [1., 0.01, 0.01],
        ),
    ];
    let baseline = capture(&gpu, &mut renderer, &scene, size)?;
    scene.display.reflections.enabled = true;
    let reflected = capture(&gpu, &mut renderer, &scene, size)?;
    diagnostic("reflections-off", &baseline)?;
    diagnostic("reflections-on", &reflected)?;
    let red = reflected
        .rgba
        .chunks_exact(4)
        .zip(baseline.rgba.chunks_exact(4))
        .enumerate()
        .filter(|(i, (a, b))| {
            *i > size[0] as usize * size[1] as usize / 2
                && a[0] > b[0].saturating_add(12)
                && a[0] > a[1].saturating_add(12)
        })
        .count();
    assert!(
        red > 100,
        "a visible object must reflect red onto the wet floor, got {red}"
    );
    scene.items[0].material.roughness = Some(0.9);
    let rough = capture(&gpu, &mut renderer, &scene, size)?;
    scene.display.reflections.enabled = false;
    assert_eq!(
        rough.rgba,
        capture(&gpu, &mut renderer, &scene, size)?.rgba,
        "rough surfaces preserve their existing lighting"
    );
    scene.items.remove(1);
    scene.items[0].material.roughness = Some(0.08);
    let empty = capture(&gpu, &mut renderer, &scene, size)?;
    scene.display.reflections.enabled = true;
    assert_eq!(
        empty.rgba,
        capture(&gpu, &mut renderer, &scene, size)?.rgba,
        "misses do not invent reflected geometry"
    );
    // Switching through an empty small viewport must rebind both color and depth.
    capture(&gpu, &mut renderer, &scene, [31, 23])?;
    assert_eq!(empty.rgba, capture(&gpu, &mut renderer, &scene, size)?.rgba);
    Ok(())
}
#[test]
fn camera_object_motion_blur_pause_reset_and_identity_rejection() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let size = [240, 160];
    let mut cube = item(1, Vec3::new(0., 0., -5.), Vec3::new(2., 2., 0.1), [1.; 3]);
    cube.material.texture = TextureKind::Checker;
    cube.material.uv_scale = [8.; 2];
    scene.items = vec![cube];
    scene.display.motion_blur = MotionBlur {
        enabled: true,
        shutter_angle: 360.,
        max_radius: 128.,
        samples: 24,
    };
    capture(&gpu, &mut renderer, &scene, size)?;
    scene.items[0].model =
        Mat4::from_translation(Vec3::new(0.3, 0., -5.)) * Mat4::from_scale(Vec3::new(2., 2., 0.1));
    scene.display.time_seconds = 1. / 60.;
    let moving = capture(&gpu, &mut renderer, &scene, size)?;
    diagnostic("motion-object", &moving)?;
    let paused = capture(&gpu, &mut renderer, &scene, size)?;
    assert!(
        changed(&moving, &paused) > 500,
        "object movement must blur its checker pattern: {}",
        changed(&moving, &paused)
    );
    assert_eq!(
        paused.rgba,
        capture(&gpu, &mut renderer, &scene, size)?.rgba,
        "paused motion settles immediately and stays fixed"
    );
    renderer.reset_display_history();
    assert_eq!(
        paused.rgba,
        capture(&gpu, &mut renderer, &scene, size)?.rgba
    );
    scene.view_projection = Mat4::from_translation(Vec3::new(-0.1, 0., 0.)) * scene.view_projection;
    scene.display.time_seconds += 1. / 60.;
    let camera = capture(&gpu, &mut renderer, &scene, size)?;
    let camera_still = capture(&gpu, &mut renderer, &scene, size)?;
    assert!(
        changed(&camera, &camera_still) > 300,
        "camera movement must blur objects"
    );
    scene.items[0].motion_id = 2;
    scene.items[0].model =
        Mat4::from_translation(Vec3::new(-0.3, 0., -5.)) * Mat4::from_scale(Vec3::new(2., 2., 0.1));
    scene.display.time_seconds += 1. / 60.;
    let spawned = capture(&gpu, &mut renderer, &scene, size)?;
    assert_eq!(
        spawned.rgba,
        capture(&gpu, &mut renderer, &scene, size)?.rgba,
        "a new identity must not inherit an old object's velocity"
    );
    Ok(())
}
#[test]
fn taa_accumulates_subpixel_geometry_rejects_departed_objects_and_resets() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let size = [192, 128];
    scene.display.temporal_aa.enabled = true;
    let mut bar = item(1, Vec3::new(0.013, 0., -5.), Vec3::ONE, [1.; 3]);
    bar.model *= Mat4::from_rotation_z(0.17) * Mat4::from_scale(Vec3::new(0.025, 3., 0.02));
    scene.items = vec![bar];
    let first = capture(&gpu, &mut renderer, &scene, size)?;
    for i in 1..16 {
        scene.display.time_seconds = i as f32 / 60.;
        capture(&gpu, &mut renderer, &scene, size)?;
    }
    let accumulated = capture(&gpu, &mut renderer, &scene, size)?;
    diagnostic("taa-first", &first)?;
    diagnostic("taa-accumulated", &accumulated)?;
    assert!(
        changed(&first, &accumulated) > 20,
        "subpixel coverage must accumulate across jittered samples"
    );
    assert_eq!(
        accumulated.rgba,
        capture(&gpu, &mut renderer, &scene, size)?.rgba,
        "paused TAA must hold the resolved image"
    );
    scene.items.clear();
    scene.display.time_seconds += 1. / 60.;
    let removed = capture(&gpu, &mut renderer, &scene, size)?;
    renderer.reset_display_history();
    let clean = capture(&gpu, &mut renderer, &scene, size)?;
    assert_eq!(
        removed.rgba, clean.rgba,
        "removed objects leave no history trail"
    );
    scene
        .items
        .push(item(3, Vec3::new(0., 0., -5.), Vec3::ONE, [0.4, 0.7, 0.2]));
    scene.display.time_seconds = 0.;
    let rewind = capture(&gpu, &mut renderer, &scene, size)?;
    renderer.reset_display_history();
    assert_eq!(
        rewind.rgba,
        capture(&gpu, &mut renderer, &scene, size)?.rgba
    );
    Ok(())
}

#[test]
fn moving_silhouettes_spread_and_static_foreground_stays_sharp() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let size = [240, 160];
    scene.display.motion_blur = MotionBlur {
        enabled: true,
        shutter_angle: 360.,
        max_radius: 128.,
        samples: 32,
    };
    scene.items = vec![item(1, Vec3::new(-0.3, 0., -5.), Vec3::ONE, [1.; 3])];
    capture(&gpu, &mut renderer, &scene, size)?;
    scene.items[0].model = Mat4::from_translation(Vec3::new(0., 0., -5.));
    scene.display.time_seconds = 1. / 60.;
    let moving = capture(&gpu, &mut renderer, &scene, size)?;
    let still = capture(&gpu, &mut renderer, &scene, size)?;
    let outside = ((80 * 240 + 97) * 4) as usize;
    assert!(
        moving.rgba[outside] > still.rgba[outside] + 15,
        "solid silhouettes must blur beyond their current rasterized edge: {} vs {}",
        moving.rgba[outside],
        still.rgba[outside]
    );
    diagnostic("motion-silhouette", &moving)?;
    scene.items.push(item(
        2,
        Vec3::new(-0.5, 0., -3.),
        Vec3::new(0.6, 1.5, 0.1),
        [0.05, 0.3, 0.05],
    ));
    renderer.reset_display_history();
    capture(&gpu, &mut renderer, &scene, size)?;
    scene.items[0].model = Mat4::from_translation(Vec3::new(0.3, 0., -5.));
    scene.display.time_seconds += 1. / 60.;
    let obscured = capture(&gpu, &mut renderer, &scene, size)?;
    let sharp = capture(&gpu, &mut renderer, &scene, size)?;
    for y in 62..98 {
        for x in 92..106 {
            let index = ((y * 240 + x) * 4) as usize;
            assert_eq!(
                &obscured.rgba[index..index + 3],
                &sharp.rgba[index..index + 3],
                "static foreground rejects background movement"
            );
        }
    }
    Ok(())
}
