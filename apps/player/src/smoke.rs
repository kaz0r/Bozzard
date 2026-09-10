use super::*;
use bozzard_demo::{Position, demo};
use bozzard_render::{DrawItem, Material, MeshKind, RenderScene, TextureKind};
use bozzard_render::{Frame, TriangleRenderer, capture_offscreen, render_offscreen};
use glam::{Mat4, Vec3};
mod pbr;

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

fn model_material_checks(gpu: &Gpu) -> Result<()> {
    use bozzard_render::{ModelImage, ModelPart};
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let vertices = [
        [-0.9, -0.8, 0.5, 0., 0., 1., 0., 1.],
        [-0.1, -0.8, 0.5, 0., 0., 1., 1., 1.],
        [-0.1, 0.8, 0.5, 0., 0., 1., 1., 0.],
        [-0.9, 0.8, 0.5, 0., 0., 1., 0., 0.],
        [0.1, -0.8, 0.5, 0., 0., 1., 0., 1.],
        [0.9, -0.8, 0.5, 0., 0., 1., 1., 1.],
        [0.9, 0.8, 0.5, 0., 0., 1., 1., 0.],
        [0.1, 0.8, 0.5, 0., 0., 1., 0., 0.],
    ];
    let indices = [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
    let green = [0, 255, 0, 255];
    let parts = [
        ModelPart {
            shading: None,
            start: 0,
            count: 6,
            color: [1., 0., 0., 1.],
            alpha_cutoff: None,
            image: None,
        },
        ModelPart {
            shading: None,
            start: 6,
            count: 6,
            color: [1.; 4],
            alpha_cutoff: None,
            image: Some(ModelImage {
                width: 1,
                height: 1,
                rgba: &green,
            }),
        },
    ];
    renderer.upload_model(gpu, "multipart", &vertices, &indices, &parts)?;
    let shared_parts: Vec<_> = [0, 6]
        .into_iter()
        .map(|start| ModelPart {
            shading: None,
            start,
            count: 6,
            color: [1.; 4],
            alpha_cutoff: None,
            image: Some(ModelImage {
                width: 1,
                height: 1,
                rgba: &green,
            }),
        })
        .collect();
    renderer.upload_model(gpu, "shared-texture", &vertices, &indices, &shared_parts)?;
    let stats = renderer
        .model_upload_stats("shared-texture")
        .context("missing upload stats")?;
    ensure!(
        stats.surfaces == 2 && stats.unique_images == 1 && stats.texture_bytes == 4,
        "shared model texture was uploaded more than once: {stats:?}"
    );
    let material = Material {
        tint: [1.; 3],
        uv_scale: [1.; 2],
        texture: TextureKind::White,
        lit: false,
    };
    // Extreme minification must converge to the linear-light average, rather
    // than aliasing between black/white or averaging sRGB bytes (about 55).
    for (width, height, checker, expected) in [
        (64, 64, true, [128; 3]),
        (7, 3, false, [55; 3]),
        (1, 17, false, [55; 3]),
    ] {
        let rgba: Vec<u8> = (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    let value = if checker {
                        if (x + y) % 2 == 0 { 0 } else { 255 }
                    } else {
                        128
                    };
                    [value, value, value, 255]
                })
            })
            .collect();
        renderer.upload_model(
            gpu,
            "mip-test",
            &vertices,
            &indices,
            &[ModelPart {
                shading: None,
                start: 0,
                count: 12,
                color: [1.; 4],
                alpha_cutoff: None,
                image: Some(ModelImage {
                    width,
                    height,
                    rgba: &rgba,
                }),
            }],
        )?;
        let mip_scene = RenderScene {
            view_projection: Mat4::IDENTITY,
            items: vec![DrawItem {
                model: Mat4::IDENTITY,
                mesh: MeshKind::Imported("mip-test".into()),
                material: Material {
                    uv_scale: [128.; 2],
                    ..material.clone()
                },
            }],
        };
        let mip_frame = capture(gpu, &mut renderer, &mip_scene, [64, 64])?;
        for x in 10..24 {
            pixel(&mip_frame, x, 32, expected)?;
        }
    }
    let scene = RenderScene {
        view_projection: Mat4::IDENTITY,
        items: vec![DrawItem {
            model: Mat4::IDENTITY,
            mesh: MeshKind::Imported("multipart".into()),
            material: material.clone(),
        }],
    };
    let image = capture(gpu, &mut renderer, &scene, [64, 64])?;
    pixel(&image, 16, 32, [255, 0, 0])?;
    pixel(&image, 48, 32, [0, 255, 0])?;
    let invalid = [ModelPart {
        shading: None,
        start: 0,
        count: 999,
        color: [1.; 4],
        alpha_cutoff: None,
        image: None,
    }];
    ensure!(
        renderer
            .upload_model(gpu, "multipart", &vertices, &indices, &invalid)
            .is_err(),
        "invalid model upload accepted"
    );
    ensure!(
        capture(gpu, &mut renderer, &scene, [64, 64])?.rgba == image.rgba,
        "failed model upload replaced last good model"
    );
    renderer.upload_image(gpu, "half-red", 1, 1, &[255, 0, 0, 128])?;
    let alpha_scene = RenderScene {
        view_projection: Mat4::IDENTITY,
        items: vec![
            DrawItem {
                model: Mat4::from_translation(Vec3::new(0., 0., 0.2)),
                mesh: MeshKind::Quad,
                material: Material {
                    texture: TextureKind::Imported("half-red".into()),
                    ..material.clone()
                },
            },
            DrawItem {
                model: Mat4::from_translation(Vec3::new(0., 0., 0.8)),
                mesh: MeshKind::Quad,
                material: Material {
                    tint: [0., 0., 1.],
                    ..material.clone()
                },
            },
        ],
    };
    pixel(
        &capture(gpu, &mut renderer, &alpha_scene, [64, 64])?,
        32,
        32,
        [128, 0, 127],
    )?;
    let masked = [ModelPart {
        shading: None,
        start: 0,
        count: 12,
        color: [1., 1., 1., 0.5],
        alpha_cutoff: Some(0.75),
        image: None,
    }];
    renderer.upload_model(gpu, "multipart", &vertices, &indices, &masked)?;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        16,
        32,
        [5, 6, 10],
    )?;
    println!(
        "model_materials_ok multipart base_color_texture alpha_blend alpha_mask transactional_upload linear_light_mipmaps"
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
        texture: TextureKind::Checker,
        lit: false,
    };
    let scene = RenderScene {
        view_projection,
        items: vec![DrawItem {
            model: Mat4::from_scale(Vec3::new(2.0, 2.0, 1.0)),
            mesh: MeshKind::Quad,
            material: material.clone(),
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
                    texture: TextureKind::White,
                    ..material.clone()
                },
            },
            DrawItem {
                model: Mat4::from_translation(Vec3::new(0.0, 0.0, -1.0))
                    * Mat4::from_scale(Vec3::splat(2.0)),
                mesh: MeshKind::Cube,
                material: Material {
                    tint: [0.1, 0.3, 0.9],
                    texture: TextureKind::White,
                    ..material.clone()
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
        let document = load_document(Some(path))?;
        let assets = assets::Assets::load(&document, Some(path))?;
        assets.upload(gpu, &mut renderer)?;
        check_document(gpu, &mut renderer, &document, options, "loaded", false)?;
    }
    asset_checks(gpu, &mut renderer, options)?;
    println!("scene_gpu_ok texture_quadrants depth_order camera_pan resize scene_roundtrip");
    Ok(())
}

fn asset_checks(gpu: &Gpu, renderer: &mut SceneRenderer, options: &Options) -> Result<()> {
    use bozzard_assets::{AssetStore, LoadState};
    use bozzard_scene::{AssetKind, AssetSource};
    use std::collections::BTreeMap;
    let root = options.output.join("asset-fixture");
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join("palette.png"),
        include_bytes!("../../../examples/demo/scenes/assets/palette.png"),
    )?;
    let obj = include_bytes!("../../../examples/demo/scenes/assets/quad.obj");
    std::fs::write(root.join("quad.obj"), obj)?;
    let sources = BTreeMap::from([
        (
            "test-palette".into(),
            AssetSource {
                kind: AssetKind::Image,
                path: "palette.png".into(),
            },
        ),
        (
            "test-quad".into(),
            AssetSource {
                kind: AssetKind::Mesh,
                path: "quad.obj".into(),
            },
        ),
    ]);
    let mut store = AssetStore::new(&root, &sources)?;
    store.refresh();
    store.require_ready()?;
    for entry in store.entries() {
        assets::upload(
            gpu,
            renderer,
            &entry.id,
            entry.data().context("missing imported data")?,
        )?;
    }
    let scene = RenderScene {
        view_projection: glam::camera::rh::proj::directx::orthographic(
            -2.0, 2.0, -1.5, 1.5, 0.1, 10.0,
        ) * Mat4::from_translation(Vec3::new(0.0, 0.0, -3.0)),
        items: vec![DrawItem {
            model: Mat4::from_scale(Vec3::new(2.0, 2.0, 1.0)),
            mesh: MeshKind::Imported("test-quad".into()),
            material: Material {
                tint: [1.0; 3],
                uv_scale: [1.0; 2],
                texture: TextureKind::Imported("test-palette".into()),
                lit: false,
            },
        }],
    };
    let first = capture(gpu, renderer, &scene, [257, 193])?;
    first.write_ppm(&options.output.join("imported.ppm"))?;
    pixel(&first, 96, 64, [255, 0, 0])?;
    pixel(&first, 160, 64, [0, 255, 0])?;
    pixel(&first, 96, 128, [0, 0, 255])?;
    // 128 sRGB decodes to about 55 in a linear RGBA8 readback target.
    pixel(&first, 160, 128, [55, 55, 55])?;
    let handle = store
        .handle("test-palette")
        .context("missing texture handle")?;
    std::fs::write(root.join("palette.png"), b"incomplete file while editing")?;
    ensure!(
        store.refresh() == vec![handle],
        "corrupt reload did not report texture change"
    );
    let entry = store.get(handle).context("lost texture handle")?;
    ensure!(
        matches!(entry.state(), LoadState::Failed(_)),
        "corrupt image was accepted"
    );
    assets::upload(
        gpu,
        renderer,
        &entry.id,
        entry.data().context("lost last good asset")?,
    )?;
    ensure!(
        capture(gpu, renderer, &scene, [257, 193])?.rgba == first.rgba,
        "failed reload changed the image"
    );
    std::fs::write(
        root.join("palette.png"),
        include_bytes!("../../../examples/demo/scenes/assets/palette-reloaded.png"),
    )?;
    ensure!(
        store.refresh() == vec![handle],
        "texture recovery not detected"
    );
    let entry = store.get(handle).context("lost texture handle")?;
    assets::upload(
        gpu,
        renderer,
        &entry.id,
        entry.data().context("missing recovered data")?,
    )?;
    let updated = capture(gpu, renderer, &scene, [257, 193])?;
    pixel(&updated, 96, 64, [0, 255, 255])?;
    updated.write_ppm(&options.output.join("imported-reloaded.ppm"))?;
    // Same-sized OBJ edit verifies mesh buffer replacement without relying on timestamps.
    let shifted = std::str::from_utf8(obj)?
        .replace("v -0.5", "v -2.5")
        .replace("v 0.5", "v -1.5");
    std::fs::write(root.join("quad.obj"), shifted)?;
    let mesh_handle = store.handle("test-quad").context("missing mesh handle")?;
    ensure!(
        store.refresh() == vec![mesh_handle],
        "mesh change not detected"
    );
    let entry = store.get(mesh_handle).context("lost mesh handle")?;
    assets::upload(
        gpu,
        renderer,
        &entry.id,
        entry.data().context("missing changed mesh")?,
    )?;
    pixel(
        &capture(gpu, renderer, &scene, [257, 193])?,
        128,
        96,
        [5, 6, 10],
    )?;
    println!(
        "asset_gpu_ok imported_mesh texture_orientation srgb failed_reload recovery mesh_reload"
    );
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
    model_material_checks(&gpu)?;
    pbr::checks(&gpu)?;
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
