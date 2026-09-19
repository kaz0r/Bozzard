use bozzard_assets::{AssetData, AssetStore};
use bozzard_render::*;
use bozzard_render_assets::{Residency, required_assets};
use bozzard_scene::{AssetKind, AssetSource};
use glam::{Mat4, Vec3};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
fn required(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).into()).collect()
}
fn scene(id: &str) -> RenderScene {
    RenderScene {
        skin_poses: Default::default(),
        particles: vec![],
        fog: Default::default(),
        gi: None,
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        display: DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        },
        lighting: Lighting {
            shadows: false,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.),
        items: vec![DrawItem {
            motion_id: 1,
            model: Mat4::from_translation(Vec3::new(0., 0., -1.))
                * Mat4::from_scale(Vec3::splat(1.5)),
            mesh: MeshKind::Quad,
            material: Material {
                metallic: None,
                roughness: None,
                surface_overrides: Default::default(),
                tint: [1.; 3],
                uv_scale: [1.; 2],
                texture: TextureKind::Imported(id.into()),
                lit: false,
                shader: None,
            },
        }],
        shader_time: 0.,
    }
}
fn capture(gpu: &Gpu, renderer: &mut SceneRenderer, id: &str) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, 64, 64, |target| {
        renderer.draw(gpu, target, [64; 2], &scene(id))
    })
}

#[test]
fn cooked_bc_astc_upload_mip_tails_memory_fallback_and_restoration() -> anyhow::Result<()> {
    use bozzard_assets::{
        ImageData, MeshData, MeshPart,
        job::Progress,
        texture::{self, Compression},
    };
    use bozzard_render_assets::upload_source_with_features;
    use std::sync::Arc;
    let gpu = pollster::block_on(Gpu::request_prefer_software(&wgpu::Instance::default()))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut image = ImageData {
        width: 32,
        height: 20,
        rgba: (0..20)
            .flat_map(|y| (0..32).flat_map(move |x| [(x * 7) as u8, (y * 11) as u8, 90, 255]))
            .collect(),
        compressed: None,
    };
    let cooked = texture::cook(
        &image,
        &[Compression::Bc3, Compression::Astc4x4],
        &[false, true],
        &Progress::default(),
    )?;
    image.compressed = Some(Arc::new(cooked));
    let data = Arc::new(AssetData::Image(image.clone()));
    let publish =
        |renderer: &mut SceneRenderer, source: Arc<dyn UploadSource>| -> anyhow::Result<usize> {
            let expected = upload_memory_bytes(source.as_ref())?;
            let mut job = renderer.begin_upload(&gpu, source)?;
            assert_eq!(job.memory_bytes(), expected);
            while !job.advance(&gpu, renderer, 128)?.complete {}
            job.finish(renderer, "texture")?;
            Ok(expected)
        };
    assert_eq!(
        publish(
            &mut renderer,
            upload_source_with_features(data.clone(), wgpu::Features::empty())?
        )?,
        2560
    );
    let reference = capture(&gpu, &mut renderer, "texture")?;
    for feature in [
        wgpu::Features::TEXTURE_COMPRESSION_BC,
        wgpu::Features::TEXTURE_COMPRESSION_ASTC,
    ] {
        if !gpu.device.features().contains(feature) {
            continue;
        }
        let source = upload_source_with_features(data.clone(), feature)?;
        assert!(source.compressed(None, true).is_some());
        assert_eq!(publish(&mut renderer, source)?, 640);
        let compressed = capture(&gpu, &mut renderer, "texture")?;
        let rmse = (compressed
            .rgba
            .iter()
            .zip(&reference.rgba)
            .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
            .sum::<f64>()
            / reference.rgba.len() as f64)
            .sqrt();
        assert!(rmse < 12., "{feature:?} render error {rmse}");
        println!("{feature:?}: 2560 -> 640 GPU bytes, native render RMSE {rmse:.3}");
        renderer.remove_asset("texture");
        publish(
            &mut renderer,
            upload_source_with_features(data.clone(), feature)?,
        )?;
        assert_eq!(
            capture(&gpu, &mut renderer, "texture")?.rgba,
            compressed.rgba
        );

        // Full model mip chain includes odd virtual mip sizes and 1x1 tails.
        let image = Arc::new(image.clone());
        let uv = [[0., 1.], [1., 1.], [1., 0.], [0., 0.]];
        let shading = bozzard_assets::SurfaceShading {
            vertex_start: 0,
            vertices: uv
                .map(|uv| {
                    [
                        1., 0., 0., 1., uv[0], uv[1], uv[0], uv[1], uv[0], uv[1], uv[0], uv[1],
                    ]
                })
                .to_vec(),
            material: bozzard_assets::PbrMaterial {
                metallic: 0.,
                roughness: 1.,
                normal_scale: 1.,
                occlusion_strength: 1.,
                emissive_factor: [0.; 3],
                double_sided: true,
                base_color_sampler: Default::default(),
                normal: None,
                metallic_roughness: Some(bozzard_assets::TextureMap {
                    image: image.clone(),
                    sampler: Default::default(),
                }),
                occlusion: Some(bozzard_assets::TextureMap {
                    image: image.clone(),
                    sampler: Default::default(),
                }),
                emissive: Some(bozzard_assets::TextureMap {
                    image: image.clone(),
                    sampler: Default::default(),
                }),
            },
        };
        let model = Arc::new(AssetData::Mesh(MeshData {
            vertices: vec![
                [-1., -1., 0., 0., 0., 1., 0., 1.],
                [1., -1., 0., 0., 0., 1., 1., 1.],
                [1., 1., 0., 0., 0., 1., 1., 0.],
                [-1., 1., 0., 0., 0., 1., 0., 0.],
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            skin: None,
            warnings: vec![],
            parts: vec![MeshPart {
                source_key: "quad".into(),
                name: "Quad".into(),
                material_name: None,
                start: 0,
                count: 6,
                color: [1.; 4],
                image: Some(image.clone()),
                alpha_cutoff: None,
                shading: Some(shading),
            }],
        }));
        let source = upload_source_with_features(model, feature)?;
        let mips = image.compressed.as_ref().unwrap().variants()[0].bytes();
        // Two unique textures (sRGB + linear), despite four material references.
        assert_eq!(
            upload_memory_bytes(source.as_ref())?,
            4 * 32 + 6 * 4 + 4 * 48 + 32 + 2 * mips
        );
        publish(&mut renderer, source)?;
        assert_eq!(
            renderer
                .model_upload_stats("texture")
                .unwrap()
                .texture_bytes,
            2 * mips
        );
        let mut render = scene("texture");
        render.items[0].mesh = MeshKind::Imported("texture".into());
        render.items[0].material.texture = TextureKind::White;
        let frame = capture_offscreen(&gpu, 64, 64, |target| {
            renderer.draw(&gpu, target, [64; 2], &render)
        })?;
        assert!(frame.rgba.chunks_exact(4).any(|p| p[0] > 40));
        // Cooked alpha must choose the same transparent pass as source pixels.
        let mut alpha = ImageData {
            width: 4,
            height: 4,
            rgba: [255, 0, 0, 128].repeat(16),
            compressed: None,
        };
        alpha.compressed = Some(Arc::new(texture::cook(
            &alpha,
            &[Compression::Bc3, Compression::Astc4x4],
            &[true],
            &Progress::default(),
        )?));
        let alpha = Arc::new(AssetData::Image(alpha));
        publish(
            &mut renderer,
            upload_source_with_features(alpha.clone(), wgpu::Features::empty())?,
        )?;
        let reference = capture(&gpu, &mut renderer, "texture")?;
        publish(&mut renderer, upload_source_with_features(alpha, feature)?)?;
        let encoded = capture(&gpu, &mut renderer, "texture")?;
        assert!(
            encoded
                .rgba
                .iter()
                .zip(&reference.rgba)
                .all(|(a, b)| a.abs_diff(*b) <= 1)
        );
    }
    let mut odd = image.clone();
    odd.width = 31;
    odd.rgba.truncate(31 * 20 * 4);
    odd.compressed = Some(Arc::new(texture::cook(
        &odd,
        &[Compression::Bc3, Compression::Astc4x4],
        &[true],
        &Progress::default(),
    )?));
    let source =
        upload_source_with_features(Arc::new(AssetData::Image(odd)), gpu.device.features())?;
    assert!(source.compressed(None, true).is_none());
    assert_eq!(publish(&mut renderer, source)?, 31 * 20 * 4);
    let mut stale = image;
    stale.rgba[0] ^= 255;
    let source =
        upload_source_with_features(Arc::new(AssetData::Image(stale)), gpu.device.features())?;
    assert!(renderer.begin_upload(&gpu, source).is_err());
    Ok(())
}

#[test]
fn requirements_include_model_surfaces_overrides_and_offscreen_shadow_casters() {
    let mut scene = scene("base");
    let item = &mut scene.items[0];
    item.mesh = MeshKind::ModelPart("mesh".into(), 2);
    item.material.surface_overrides = [SurfaceMaterialOverride {
        surface: 2,
        source: "surface".into(),
        transform: Mat4::IDENTITY,
        texture: Some(TextureKind::ModelPart("override".into(), 1)),
        uv_scale: [1.; 2],
        tint: [1.; 3],
        metallic: None,
        roughness: None,
    }]
    .into();
    item.model = Mat4::from_translation(Vec3::splat(10000.));
    assert_eq!(
        required_assets(&scene),
        required(&["mesh", "base", "override"])
    );
}
fn store() -> anyhow::Result<AssetStore> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/assets");
    let mut sources: BTreeMap<_, _> = ["a", "b", "c"]
        .map(|id| {
            (
                id.into(),
                AssetSource {
                    kind: AssetKind::Image,
                    path: "palette.png".into(),
                },
            )
        })
        .into();
    sources.insert(
        "skin".into(),
        AssetSource {
            kind: AssetKind::Mesh,
            path: "animated-banner.gltf".into(),
        },
    );
    let mut store = AssetStore::new(&root, &sources)?;
    store.load_pending()?;
    store.require_ready()?;
    Ok(store)
}
#[test]
fn budget_evicts_lru_unused_assets_restores_them_and_pins_an_oversized_working_set()
-> anyhow::Result<()> {
    let instance = wgpu::Instance::default();
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let store = store()?;
    let AssetData::Image(image) = store
        .get(store.handle("a").unwrap())
        .unwrap()
        .data()
        .unwrap()
    else {
        panic!()
    };
    let bytes = image.rgba.len();
    let mut residency = Residency::default();
    residency.set_budget(Some(bytes * 2));
    residency.set_required(required(&["a"]));
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 1);
    assert_eq!(residency.stats().resident_assets, 1);
    assert_eq!(residency.stats().resident_bytes, bytes);
    assert!(residency.has_required(&store));
    assert!(!residency.has_all(&store));
    let original = capture(&gpu, &mut renderer, "a")?;
    assert!(
        original
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[..3] != original.rgba[..3])
    );
    residency.set_required(required(&["b"]));
    residency.sync(&gpu, &mut renderer, &store)?;
    residency.set_required(required(&["a"]));
    residency.advance(&gpu, &mut renderer, &store, 1024)?;
    residency.set_required(required(&["c"]));
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.evicted, 1);
    assert!(residency.is_current(&store, "a") && residency.is_current(&store, "c"));
    assert!(!residency.is_current(&store, "b"));
    residency.set_required(required(&["b"]));
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 1);
    assert!(!residency.is_current(&store, "a"));
    assert!(residency.is_current(&store, "b") && residency.is_current(&store, "c"));
    assert_eq!(residency.stats().resident_bytes, bytes * 2);
    residency.set_budget(Some(bytes));
    residency.set_required(required(&["b", "c"]));
    for _ in 0..10 {
        let report = residency.advance(&gpu, &mut renderer, &store, 1024)?;
        assert_eq!(report.evicted + report.uploaded, 0);
    }
    assert_eq!(residency.stats().over_budget_bytes, bytes);
    assert!(residency.required_current(&store));
    residency.set_required(required(&[]));
    residency.set_budget(Some(0));
    assert_eq!(
        residency
            .advance(&gpu, &mut renderer, &store, 1024)?
            .evicted,
        2
    );
    assert_eq!(residency.stats().resident_bytes, 0);
    residency.set_required(required(&["a"]));
    residency.sync(&gpu, &mut renderer, &store)?;
    assert_eq!(residency.stats().resident_bytes, bytes);
    assert_eq!(residency.stats().over_budget_bytes, bytes);
    let restored = capture(&gpu, &mut renderer, "a")?;
    assert_eq!(
        original.rgba, restored.rgba,
        "eviction/restoration changed rendered pixels"
    );
    // A requirement change cancels preparation without poisoning the cancelled revision.
    residency.set_required(required(&["b"]));
    residency.advance(&gpu, &mut renderer, &store, 1024)?;
    assert_eq!(residency.preparing().unwrap().0, "b");
    assert_eq!(residency.stats().staged_bytes, bytes);
    residency.set_required(required(&["c"]));
    residency.sync(&gpu, &mut renderer, &store)?;
    assert!(!residency.is_current(&store, "b"));
    assert!(residency.is_current(&store, "c"));
    residency.set_required(required(&["b"]));
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 1);
    assert_eq!(residency.stats().resident_bytes, bytes);
    assert_eq!(capture(&gpu, &mut renderer, "b")?.rgba, original.rgba);
    // Preparation preflights every model image, per-surface material uniform and skin buffer.
    residency.set_budget(None);
    residency.set_required(required(&["skin"]));
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 1);
    let source = bozzard_render_assets::upload_source(
        store
            .get(store.handle("skin").unwrap())
            .unwrap()
            .shared_data()
            .unwrap(),
    )?;
    assert!(source.skin().is_some());
    let expected = bozzard_render::upload_memory_bytes(source.as_ref())?;
    let pending = renderer.begin_upload(&gpu, source)?;
    assert_eq!(expected, pending.memory_bytes());
    assert_eq!(residency.stats().resident_bytes, bytes + expected);
    assert!(residency.stats().staged_bytes == 0);
    drop(pending);
    residency.set_required(required(&["missing"]));
    assert!(residency.sync(&gpu, &mut renderer, &store).is_err());
    gpu.wait()?;
    Ok(())
}

#[test]
fn replacement_identity_cancellation_corruption_and_removal_keep_last_good_pixels()
-> anyhow::Result<()> {
    let path =
        std::env::temp_dir().join(format!("bozzard-residency-replace-{}", std::process::id()));
    std::fs::create_dir_all(&path)?;
    let texture = path.join("palette.png");
    std::fs::write(
        &texture,
        include_bytes!("../../../examples/demo/scenes/assets/palette.png"),
    )?;
    let sources = BTreeMap::from([(
        "texture".into(),
        AssetSource {
            kind: AssetKind::Image,
            path: "palette.png".into(),
        },
    )]);
    let mut store = AssetStore::new(&path, &sources)?;
    store.load_pending()?;
    let gpu = pollster::block_on(Gpu::request_prefer_software(&wgpu::Instance::default()))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut residency = Residency::default();
    residency.set_required(required(&["texture"]));
    residency.sync(&gpu, &mut renderer, &store)?;
    let original = capture(&gpu, &mut renderer, "texture")?;
    std::fs::write(
        &texture,
        include_bytes!("../../../examples/demo/scenes/assets/palette-reloaded.png"),
    )?;
    store.refresh();
    store.require_ready()?;
    assert!(!residency.required_current(&store) && residency.has_required(&store));
    residency.advance(&gpu, &mut renderer, &store, 1024)?;
    residency.cancel();
    residency.sync(&gpu, &mut renderer, &store)?;
    assert!(!residency.required_current(&store));
    assert_eq!(capture(&gpu, &mut renderer, "texture")?.rgba, original.rgba);
    residency.retry_failed();
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 1);
    let replacement = capture(&gpu, &mut renderer, "texture")?;
    assert_ne!(replacement.rgba, original.rgba);
    assert!(residency.required_current(&store));
    std::fs::write(&texture, b"invalid PNG")?;
    store.refresh();
    assert!(store.require_ready().is_err());
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 0);
    assert_eq!(
        capture(&gpu, &mut renderer, "texture")?.rgba,
        replacement.rgba
    );
    let empty = store.for_catalog(&path, &BTreeMap::new())?;
    residency.set_required(BTreeSet::new());
    assert_eq!(
        residency
            .advance(&gpu, &mut renderer, &empty, 1024)?
            .removed,
        1
    );
    assert_eq!(residency.stats().resident_bytes, 0);
    std::fs::remove_dir_all(path)?;
    Ok(())
}

#[test]
fn inherited_material_images_share_storage_and_survive_alias_eviction_and_source_edits()
-> anyhow::Result<()> {
    use bozzard_scene::material_asset::{MaterialAsset, MaterialTexture};
    use std::sync::Arc;
    let root = std::env::temp_dir().join(format!("bozzard-material-gpu-{}", std::process::id()));
    std::fs::create_dir(&root)?;
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/assets");
    std::fs::copy(fixtures.join("palette.png"), root.join("map.png"))?;
    let base = MaterialAsset {
        texture: Some(MaterialTexture::Image("map.png".into())),
        ..Default::default()
    };
    std::fs::write(root.join("base.material.json"), base.to_json()?)?;
    let variant = MaterialAsset {
        parent: Some("base.material.json".into()),
        ..Default::default()
    };
    std::fs::write(root.join("variant.material.json"), variant.to_json()?)?;
    let mut store = AssetStore::new(
        &root,
        &BTreeMap::from([
            (
                "base".into(),
                AssetSource {
                    kind: AssetKind::Material,
                    path: "base.material.json".into(),
                },
            ),
            (
                "variant".into(),
                AssetSource {
                    kind: AssetKind::Material,
                    path: "variant.material.json".into(),
                },
            ),
        ]),
    )?;
    store.load_pending()?;
    store.require_ready()?;
    assert!(Arc::ptr_eq(
        store.material("base")?.image.as_ref().unwrap(),
        store.material("variant")?.image.as_ref().unwrap()
    ));
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut residency = Residency::default();
    let report = residency.sync(&gpu, &mut renderer, &store)?;
    assert_eq!(report.uploaded, 1);
    assert_eq!(residency.stats().resident_assets, 2);
    let bytes = residency.stats().resident_bytes;
    let first = capture(&gpu, &mut renderer, "base")?;
    assert_eq!(capture(&gpu, &mut renderer, "variant")?.rgba, first.rgba);
    // Changing only material properties does not replace the image allocation or GPU upload.
    let mut changed = variant.clone();
    changed.properties.color = Some([0.5, 0.6, 0.7]);
    std::fs::write(root.join("variant.material.json"), changed.to_json()?)?;
    store.refresh();
    store.require_ready()?;
    assert!(residency.is_current(&store, "variant"));
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 0);
    assert_eq!(residency.stats().resident_bytes, bytes);
    // Evicting the first alias cannot free storage still used by the second alias.
    residency.set_required(required(&["variant"]));
    residency.set_budget(Some(0));
    residency.sync(&gpu, &mut renderer, &store)?;
    assert_eq!(residency.stats().resident_assets, 1);
    assert_eq!(residency.stats().resident_bytes, bytes);
    assert_eq!(capture(&gpu, &mut renderer, "variant")?.rgba, first.rgba);
    residency.set_budget(None);
    residency.set_required(required(&["base", "variant"]));
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 0);
    assert_eq!(capture(&gpu, &mut renderer, "base")?.rgba, first.rgba);
    residency.set_required(BTreeSet::new());
    residency.set_budget(Some(0));
    residency.sync(&gpu, &mut renderer, &store)?;
    assert_eq!(residency.stats().resident_bytes, 0);
    residency.set_budget(None);
    residency.require_catalog();
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 1);
    assert_eq!(capture(&gpu, &mut renderer, "base")?.rgba, first.rgba);
    // A real image edit changes both aliases, and still uploads only once.
    std::fs::copy(fixtures.join("courier-paint.png"), root.join("map.png"))?;
    store.refresh();
    store.require_ready()?;
    assert!(!residency.is_current(&store, "base"));
    assert_eq!(residency.sync(&gpu, &mut renderer, &store)?.uploaded, 1);
    let second = capture(&gpu, &mut renderer, "base")?;
    assert_ne!(second.rgba, first.rgba);
    assert_eq!(capture(&gpu, &mut renderer, "variant")?.rgba, second.rgba);
    std::fs::remove_dir_all(root)?;
    Ok(())
}
