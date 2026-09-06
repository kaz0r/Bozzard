use super::*;
use bozzard_demo::{Position, demo};
use bozzard_render::{DrawItem, Material, MeshKind, RenderScene};
use bozzard_render::{Frame, TriangleRenderer, capture_offscreen, render_offscreen};
use glam::{Mat4, Vec3};

fn capture(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    scene: &RenderScene,
    size: [u32; 2],
) -> Result<Frame> {
    capture_offscreen(gpu, size[0], size[1], |view| {
        renderer.draw(gpu, view, size, scene)
    })
}

fn pixel(frame: &Frame, x: u32, y: u32, expected: [u8; 3]) -> Result<()> {
    let index = ((y * frame.width + x) * 4) as usize;
    let actual = &frame.rgba[index..index + 3];
    ensure!(
        actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2),
        "scene pixel at ({x},{y}): expected {expected:?}, got {actual:?}"
    );
    Ok(())
}

fn scene_checks(gpu: &Gpu, options: &Options) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let projection = glam::camera::rh::proj::directx::orthographic(-2.0, 2.0, -1.5, 1.5, 0.1, 10.0);
    let view_projection = projection * Mat4::from_translation(Vec3::new(0.0, 0.0, -3.0));
    let material = Material {
        tint: [1.0; 3],
        uv_scale: [1.0; 2],
        checker: true,
        lit: false,
    };
    let scene = RenderScene {
        view_projection,
        items: vec![DrawItem {
            model: Mat4::from_scale(Vec3::new(2.0, 2.0, 1.0)),
            mesh: MeshKind::Quad,
            material,
        }],
    };
    let frame = capture(gpu, &mut renderer, &scene, [257, 193])?;
    frame.write_ppm(&options.output.join("texture.ppm"))?;
    pixel(&frame, 96, 64, [240, 180, 70])?;
    pixel(&frame, 160, 64, [20, 90, 105])?;
    pixel(&frame, 96, 128, [20, 90, 105])?;
    pixel(&frame, 160, 128, [240, 180, 70])?;
    pixel(&frame, 8, 8, [5, 6, 10])?;
    // Exceed downlevel's default 2048 dimension to catch Retina/large-window regressions.
    let wide = capture(gpu, &mut renderer, &scene, [2053, 129])?;
    pixel(&wide, 767, 42, [240, 180, 70])?;
    let mut depth_scene = RenderScene {
        view_projection,
        items: vec![
            DrawItem {
                model: Mat4::from_translation(Vec3::new(0.0, 0.0, 0.5)),
                mesh: MeshKind::Cube,
                material: Material {
                    tint: [0.9, 0.1, 0.2],
                    checker: false,
                    ..material
                },
            },
            DrawItem {
                model: Mat4::from_translation(Vec3::new(0.0, 0.0, -1.0))
                    * Mat4::from_scale(Vec3::splat(2.0)),
                mesh: MeshKind::Cube,
                material: Material {
                    tint: [0.1, 0.3, 0.9],
                    checker: false,
                    ..material
                },
            },
        ],
    };
    let front_first = capture(gpu, &mut renderer, &depth_scene, [257, 193])?;
    front_first.write_ppm(&options.output.join("depth.ppm"))?;
    pixel(&front_first, 128, 96, [230, 26, 51])?;
    pixel(&front_first, 80, 96, [26, 77, 230])?;
    depth_scene.items.reverse();
    let back_first = capture(gpu, &mut renderer, &depth_scene, [257, 193])?;
    ensure!(
        front_first.rgba == back_first.rgba,
        "depth test depends on draw order"
    );
    depth_scene.view_projection = projection * Mat4::from_translation(Vec3::new(-1.5, 0.0, -3.0));
    let panned = capture(gpu, &mut renderer, &depth_scene, [257, 193])?;
    pixel(&panned, 128, 96, [5, 6, 10])?;
    let document = bozzard_demo::scene_document()?;
    check_document(gpu, &mut renderer, &document, options, "demo", true)?;
    if let Some(path) = &options.scene {
        check_document(
            gpu,
            &mut renderer,
            &load_document(Some(path))?,
            options,
            "loaded",
            false,
        )?;
    }
    println!("scene_gpu_ok texture_quadrants depth_order camera_pan resize scene_roundtrip");
    Ok(())
}

fn check_document(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    document: &Scene,
    options: &Options,
    prefix: &str,
    animated: bool,
) -> Result<()> {
    for (layer, label, size) in [
        (Layer::TwoD, "2d", [640, 400]),
        (Layer::ThreeD, "3d", [800, 500]),
    ] {
        let mut demo = SceneDemo::new(document)?;
        if !demo.instance.has_view(layer) {
            continue;
        }
        let aspect = size[0] as f32 / size[1] as f32;
        let initial = capture(gpu, renderer, &extract(&demo, layer, aspect)?, size)?;
        initial.write_ppm(&options.output.join(format!("{prefix}-{label}.ppm")))?;
        for _ in 0..120 {
            demo.app.step();
        }
        let moved = capture(gpu, renderer, &extract(&demo, layer, aspect)?, size)?;
        moved.write_ppm(
            &options
                .output
                .join(format!("{prefix}-{label}-animated.ppm")),
        )?;
        if animated {
            ensure!(initial.rgba != moved.rgba, "{label} scene did not animate");
        }
        let saved = demo.instance.capture(&demo.app.world)?;
        let restored = SceneDemo::new(&Scene::from_json(&saved.to_json()?)?)?;
        let reloaded = capture(gpu, renderer, &extract(&restored, layer, aspect)?, size)?;
        ensure!(
            moved.rgba == reloaded.rgba,
            "{label} save/reload changed the image"
        );
    }
    Ok(())
}

pub fn run(options: &Options) -> Result<()> {
    std::fs::create_dir_all(&options.output)?;
    let instance = instance(options.backend);
    let gpu = pollster::block_on(Gpu::request(&instance, None, options.software))?;
    if options.hardware {
        gpu.require_hardware()?;
    }
    let renderer = TriangleRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let (mut app, entity) = demo();
    let first = render_offscreen(&gpu, &renderer, [0.0, 0.0])?;
    first.write_ppm(&options.output.join("initial.ppm"))?;
    first.verify_triangle([0.0, 0.0])?;
    for _ in 0..120 {
        app.step();
    }
    let p = app
        .world
        .get::<Position>(entity)
        .context("demo entity missing")?
        .0;
    ensure!(
        (p[0] - 0.2).abs() < 1e-5,
        "simulation did not reach the expected position"
    );
    let moved = render_offscreen(&gpu, &renderer, [p[0], p[1]])?;
    moved.write_ppm(&options.output.join("moved.ppm"))?;
    moved.verify_triangle([0.2, 0.0])?;
    ensure!(
        first.rgba != moved.rgba,
        "simulation did not change the rendered output"
    );
    println!(
        "gpu_smoke_ok ticks={} output={}",
        app.ticks(),
        options.output.display()
    );
    scene_checks(&gpu, options)?;
    Ok(())
}
