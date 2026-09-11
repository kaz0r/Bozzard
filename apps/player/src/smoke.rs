use super::*;
use bozzard_demo::{Position, demo};
use bozzard_render::{DrawItem, Material, MeshKind, RenderScene, TextureKind};
use bozzard_render::{Frame, TriangleRenderer, capture_offscreen, render_offscreen};
use glam::{Mat4, Vec3};
mod benchmark;
mod bloom;
mod display;
mod environment;
mod gi;
mod overrides;
mod pbr;
mod shadows;
mod upload;
mod visibility;

fn capture(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    scene: &RenderScene,
    size: [u32; 2],
) -> Result<Frame> {
    capture_offscreen(gpu, size[0], size[1], |view| {
        renderer.draw_linear(gpu, view, size, scene)
    })
}

fn capture_display(
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
            source_key: "",
            shading: None,
            start: 0,
            count: 6,
            color: [1., 0., 0., 1.],
            alpha_cutoff: None,
            image: None,
        },
        ModelPart {
            source_key: "",
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
            source_key: "",
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
        surface_overrides: Default::default(),
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
                source_key: "",
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
            gi: None,
            lights: Vec::new(),
            environment: bozzard_render::EnvironmentSettings::disabled(),
            display: Default::default(),
            lighting: Default::default(),
            view_projection: Mat4::IDENTITY,
            items: vec![DrawItem {
                model: Mat4::IDENTITY,
                mesh: MeshKind::Imported("mip-test".into()),
                material: Material {
                    surface_overrides: Default::default(),
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
        gi: None,
        lights: Vec::new(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: Default::default(),
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
        source_key: "",
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
        gi: None,
        lights: Vec::new(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: Default::default(),
        view_projection: Mat4::IDENTITY,
        items: vec![
            DrawItem {
                model: Mat4::from_translation(Vec3::new(0., 0., 0.2)),
                mesh: MeshKind::Quad,
                material: Material {
                    surface_overrides: Default::default(),
                    texture: TextureKind::Imported("half-red".into()),
                    ..material.clone()
                },
            },
            DrawItem {
                model: Mat4::from_translation(Vec3::new(0., 0., 0.8)),
                mesh: MeshKind::Quad,
                material: Material {
                    surface_overrides: Default::default(),
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
        source_key: "",
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
        surface_overrides: Default::default(),
        tint: [1.0; 3],
        uv_scale: [1.0; 2],
        texture: TextureKind::Checker,
        lit: false,
    };
    let scene = RenderScene {
        gi: None,
        lights: Vec::new(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: Default::default(),
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
        gi: None,
        lights: Vec::new(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: Default::default(),
        view_projection,
        items: vec![
            DrawItem {
                model: Mat4::from_translation(Vec3::new(0.0, 0.0, 0.5)),
                mesh: MeshKind::Cube,
                material: Material {
                    surface_overrides: Default::default(),
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
                    surface_overrides: Default::default(),
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
    let empty_assets =
        bozzard_assets::AssetStore::new(std::path::Path::new("."), &document.assets)?;
    check_document(
        gpu,
        &mut renderer,
        &document,
        &empty_assets,
        options,
        "demo",
        true,
    )?;
    if let Some(path) = &options.scene {
        let document = load_document(Some(path))?;
        let mut assets = assets::Assets::load(&document, Some(path))?;
        assets.upload(gpu, &mut renderer)?;
        check_document(
            gpu,
            &mut renderer,
            &document,
            assets.store(),
            options,
            "loaded",
            false,
        )?;
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
    let mut residency = bozzard_render_assets::Residency::default();
    ensure!(
        residency.sync(gpu, renderer, &store)?.uploaded == 2,
        "initial residency upload missing"
    );
    ensure!(
        residency.is_current(&store, "test-quad") && !residency.is_current(&store, "missing"),
        "resident identity missing"
    );
    ensure!(
        residency.sync(gpu, renderer, &store.clone())?.uploaded == 0,
        "unchanged catalog snapshot re-uploaded assets"
    );
    let scene = RenderScene {
        gi: None,
        lights: Vec::new(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: Default::default(),
        view_projection: glam::camera::rh::proj::directx::orthographic(
            -2.0, 2.0, -1.5, 1.5, 0.1, 10.0,
        ) * Mat4::from_translation(Vec3::new(0.0, 0.0, -3.0)),
        items: vec![DrawItem {
            model: Mat4::from_scale(Vec3::new(2.0, 2.0, 1.0)),
            mesh: MeshKind::Imported("test-quad".into()),
            material: Material {
                surface_overrides: Default::default(),
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
    ensure!(
        residency.sync(gpu, renderer, &store)?.uploaded == 0,
        "failed CPU reload re-uploaded last-good data"
    );
    ensure!(
        residency.is_current(&store, "test-palette"),
        "failed reload lost current identity"
    );
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
    ensure!(
        residency.sync(gpu, renderer, &store)?.uploaded == 1,
        "texture recovery re-uploaded unrelated assets"
    );
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
    let wait_staged = |residency: &mut bozzard_render_assets::Residency,
                       renderer: &mut SceneRenderer,
                       store: &AssetStore|
     -> Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while residency.progress().is_none() {
            residency.advance(gpu, renderer, store, 32)?;
            ensure!(
                std::time::Instant::now() < deadline,
                "GPU preparation did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    };
    wait_staged(&mut residency, renderer, &store)?;
    ensure!(
        residency.has_all(&store) && !residency.is_current(&store, "test-quad"),
        "staged CPU geometry was treated as rendered geometry"
    );
    ensure!(
        capture(gpu, renderer, &scene, [257, 193])?.rgba == updated.rgba,
        "partial replacement changed the old model"
    );
    residency.cancel();
    ensure!(
        residency.advance(gpu, renderer, &store, 32)?.uploaded == 0
            && residency.progress().is_none(),
        "cancelled upload restarted automatically"
    );
    ensure!(
        capture(gpu, renderer, &scene, [257, 193])?.rgba == updated.rgba,
        "cancelled replacement lost the old model"
    );
    residency.retry_failed();
    wait_staged(&mut residency, renderer, &store)?;
    let superseded =
        std::sync::Arc::downgrade(&store.get(mesh_handle).unwrap().shared_data().unwrap());
    std::fs::write(
        root.join("quad.obj"),
        format!("{}\n# New generation\n", std::str::from_utf8(obj)?),
    )?;
    ensure!(
        store.refresh() == vec![mesh_handle],
        "new mesh generation not detected"
    );
    residency.advance(gpu, renderer, &store, 32)?;
    ensure!(
        superseded.upgrade().is_none(),
        "stale upload retained or published superseded source"
    );
    ensure!(
        residency.sync(gpu, renderer, &store)?.uploaded == 1,
        "new generation did not replace stale upload"
    );
    ensure!(
        residency.is_current(&store, "test-quad"),
        "published geometry identity not current"
    );
    ensure!(
        capture(gpu, renderer, &scene, [257, 193])?.rgba == updated.rgba,
        "stale geometry was published instead of newest generation"
    );
    std::fs::write(
        root.join("quad.obj"),
        std::str::from_utf8(obj)?
            .replace("v -0.5", "v -2.5")
            .replace("v 0.5", "v -1.5"),
    )?;
    ensure!(
        store.refresh() == vec![mesh_handle],
        "final mesh replacement not detected"
    );
    ensure!(
        residency.sync(gpu, renderer, &store)?.uploaded == 1,
        "mesh reload re-uploaded unrelated assets"
    );
    pixel(
        &capture(gpu, renderer, &scene, [257, 193])?,
        128,
        96,
        [5, 6, 10],
    )?;
    let empty = AssetStore::new(&root, &BTreeMap::new())?;
    ensure!(
        residency.sync(gpu, renderer, &empty)?.removed == 2,
        "removed catalog entries were not retired"
    );
    ensure!(
        capture(gpu, renderer, &scene, [257, 193]).is_err(),
        "removed GPU assets remained visible"
    );
    println!(
        "asset_gpu_ok imported_mesh texture_orientation srgb failed_reload recovery mesh_reload residency_reuse staged_cancel stale_generation retirement"
    );
    Ok(())
}

fn check_document(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    document: &Scene,
    assets: &bozzard_assets::AssetStore,
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
        let initial =
            capture_display(gpu, renderer, &extract(&demo, assets, layer, aspect)?, size)?;
        if layer == Layer::ThreeD
            && prefix == "loaded"
            && let Some(frames) = options.benchmark_frames
        {
            benchmark::run(
                gpu,
                renderer,
                &extract(&demo, assets, layer, aspect)?,
                size,
                frames,
            )?;
        }
        initial.write_ppm(&options.output.join(format!("{prefix}-{label}.ppm")))?;
        if layer == Layer::ThreeD && document.lighting.shadows {
            let mut without = extract(&demo, assets, layer, aspect)?;
            without.lighting.shadows = false;
            let unshadowed = capture_display(gpu, renderer, &without, size)?;
            unshadowed.write_ppm(
                &options
                    .output
                    .join(format!("{prefix}-{label}-no-shadows.ppm")),
            )?;
            let affected = initial
                .rgba
                .chunks_exact(4)
                .zip(unshadowed.rgba.chunks_exact(4))
                .filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > 3))
                .count();
            println!("scene_shadow_comparison scene={prefix} changed_pixels={affected}");
        }
        for _ in 0..120 {
            demo.app.step();
        }
        let moved = capture_display(gpu, renderer, &extract(&demo, assets, layer, aspect)?, size)?;
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
        let reloaded = capture_display(
            gpu,
            renderer,
            &extract(&restored, assets, layer, aspect)?,
            size,
        )?;
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
    visibility::checks(&gpu)?;
    environment::checks(&gpu)?;
    display::checks(&gpu)?;
    bloom::checks(&gpu, &options.output)?;
    gi::checks(&gpu)?;
    gi::baked_room(&gpu, &options.output)?;
    pbr::checks(&gpu)?;
    overrides::checks(&gpu)?;
    shadows::checks(&gpu)?;
    upload::checks(&gpu)?;
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
