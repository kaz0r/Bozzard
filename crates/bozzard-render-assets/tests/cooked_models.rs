use bozzard_assets::{AssetData, AssetStore, cooked_model, job::Progress, texture::Compression};
use bozzard_render::*;
use bozzard_scene::{AssetKind, AssetSource};
use glam::{Mat4, Vec3};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

fn scene(mesh: &bozzard_assets::MeshData, time: f32) -> anyhow::Result<RenderScene> {
    let mut poses = BTreeMap::new();
    if let Some(skin) = &mesh.skin {
        let pose = if skin.rig.clips.is_empty() {
            skin.rig.rest_pose()
        } else {
            skin.rig.sample(0, time)?
        };
        poses.insert(
            1,
            SkinPose {
                signature: skin.rig.signature(),
                matrices: Arc::new(skin.rig.palette(&pose)?),
            },
        );
    }
    Ok(RenderScene {
        skin_poses: poses,
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
        view_projection: glam::camera::rh::proj::directx::perspective(
            50_f32.to_radians(),
            1.,
            0.1,
            100.,
        ) * glam::camera::rh::view::look_at_mat4(
            Vec3::new(0., 0.8, 4.),
            Vec3::new(0., 0.8, 0.),
            Vec3::Y,
        ),
        items: vec![DrawItem {
            motion_id: 1,
            model: Mat4::IDENTITY,
            mesh: MeshKind::Imported("model".into()),
            material: Material {
                metallic: None,
                roughness: None,
                surface_overrides: Default::default(),
                tint: [1.; 3],
                uv_scale: [1.; 2],
                texture: TextureKind::White,
                lit: true,
                shader: None,
            },
        }],
        shader_time: 0.,
    })
}
fn render(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    data: Arc<AssetData>,
    scene: &RenderScene,
    features: wgpu::Features,
) -> anyhow::Result<Frame> {
    let source = bozzard_render_assets::upload_source_reference_with_features(data, features)?;
    render_source(gpu, renderer, source, scene)
}
fn render_source(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    source: Arc<dyn UploadSource>,
    scene: &RenderScene,
) -> anyhow::Result<Frame> {
    let expected = upload_memory_bytes(source.as_ref())?;
    let mut upload = renderer.begin_upload(gpu, source)?;
    assert_eq!(upload.memory_bytes(), expected);
    while !upload.advance(gpu, renderer, 65536)?.complete {}
    upload.finish(renderer, "model")?;
    capture_offscreen(gpu, 128, 128, |view| {
        renderer.draw(gpu, view, [128; 2], scene)
    })
}
#[test]
fn cooked_static_and_skinned_models_match_native_reference_and_reduce_texture_storage()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&wgpu::Instance::default()))?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/assets");
    for name in ["courier.glb", "animated-banner.gltf"] {
        let sources = [(
            "model".into(),
            AssetSource {
                kind: AssetKind::Mesh,
                path: name.into(),
            },
        )]
        .into();
        let mut store = AssetStore::new(&root, &sources)?;
        store.load_pending()?;
        store.require_ready()?;
        let original = store
            .get(store.handle("model").unwrap())
            .unwrap()
            .shared_data()
            .unwrap();
        let AssetData::Mesh(mesh) = original.as_ref() else {
            panic!()
        };
        let cooked = cooked_model::decode(&cooked_model::encode(
            mesh,
            &[Compression::Bc3, Compression::Astc4x4],
            &Progress::default(),
        )?)?;
        let cooked = Arc::new(AssetData::Mesh(cooked));
        let mut reference_renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut cooked_renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
        let mut first = None;
        for time in [0., 0.5] {
            let frame_scene = scene(mesh, time)?;
            let reference = render(
                &gpu,
                &mut reference_renderer,
                original.clone(),
                &frame_scene,
                wgpu::Features::empty(),
            )?;
            assert!(
                reference
                    .rgba
                    .chunks_exact(4)
                    .any(|p| p[..3] != reference.rgba[..3]),
                "empty model frame"
            );
            let restored = render(
                &gpu,
                &mut cooked_renderer,
                cooked.clone(),
                &frame_scene,
                wgpu::Features::empty(),
            )?;
            assert_eq!(
                reference.rgba, restored.rgba,
                "{name} time {time}: cooked RGBA reference changed"
            );
            let compressed = render(
                &gpu,
                &mut cooked_renderer,
                cooked.clone(),
                &frame_scene,
                gpu.device.features(),
            )?;
            let rmse = (compressed
                .rgba
                .iter()
                .zip(&reference.rgba)
                .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
                .sum::<f64>()
                / reference.rgba.len() as f64)
                .sqrt();
            assert!(rmse < 12., "{name}: compressed render RMSE {rmse}");
            let before = reference_renderer.model_upload_stats("model").unwrap();
            let after = cooked_renderer.model_upload_stats("model").unwrap();
            assert!(after.texture_bytes <= before.texture_bytes);
            println!(
                "cooked_model {name} time={time} texture_bytes={} -> {} rmse={rmse:.3}",
                before.texture_bytes, after.texture_bytes
            );
            if let Some(first) = &first {
                if mesh.skin.is_some() {
                    assert_ne!(first, &reference.rgba, "skin did not animate");
                }
            } else {
                first = Some(reference.rgba);
            }
        }
    }
    Ok(())
}

#[test]
fn lossless_full_resolution_cooking_reduces_uploaded_vertices_with_exact_alpha_output()
-> anyhow::Result<()> {
    use bozzard_assets::{MeshData, MeshPart};
    let a = [-1., 0., 0., 0., 0., 1., 0., 0.];
    let b = [1., 0., 0., 0., 0., 1., 1., 0.];
    let c = [0., 2., 0., 0., 0., 1., 0., 1.];
    let mesh = MeshData {
        vertices: vec![a, b, c, a, b, c],
        indices: vec![3, 4, 5, 0, 1, 2],
        parts: vec![MeshPart {
            source_key: "original-alpha-triangles".into(),
            name: "overlapping alpha".into(),
            material_name: None,
            start: 0,
            count: 6,
            color: [1., 0.2, 0.3, 0.5],
            image: None,
            alpha_cutoff: None,
            shading: None,
        }],
        skin: None,
        warnings: Vec::new(),
    };
    let cooked = cooked_model::decode(&cooked_model::encode(&mesh, &[], &Progress::default())?)?;
    assert_eq!(mesh.vertices.len(), 6);
    assert_eq!(cooked.vertices.len(), 3);
    assert_eq!(cooked.indices, [0, 1, 2, 0, 1, 2]);
    assert_eq!(cooked.parts[0].source_key, mesh.parts[0].source_key);
    let original = Arc::new(AssetData::Mesh(mesh.clone()));
    let cooked = Arc::new(AssetData::Mesh(cooked));
    let features = wgpu::Features::empty();
    let mut sizes = Vec::new();
    for (data, expected_vertices) in [(original.clone(), 6), (cooked.clone(), 3)] {
        let source = bozzard_render_assets::upload_source_reference_with_features(data, features)?;
        let UploadData::Model {
            vertices, indices, ..
        } = source.data()
        else {
            panic!("mesh did not produce a model upload")
        };
        assert_eq!(vertices.len(), expected_vertices);
        assert_eq!(indices.len(), 6);
        sizes.push(upload_memory_bytes(source.as_ref())?);
    }
    assert_eq!(sizes[0] - sizes[1], 3 * std::mem::size_of::<[f32; 8]>());
    let optimized_raw =
        bozzard_render_assets::upload_source_with_features(original.clone(), features)?;
    let UploadData::Model {
        vertices, indices, ..
    } = optimized_raw.data()
    else {
        panic!()
    };
    assert_eq!(
        vertices.len(),
        3,
        "ordinary raw uploads use optimized GPU buffers"
    );
    assert_eq!(indices, [0, 1, 2, 0, 1, 2]);
    assert_eq!(upload_memory_bytes(optimized_raw.as_ref())?, sizes[1]);
    let gpu = pollster::block_on(Gpu::request_prefer_software(&wgpu::Instance::default()))?;
    let mut reference_renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut optimized_renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let frame_scene = scene(&mesh, 0.)?;
    let before = render(
        &gpu,
        &mut reference_renderer,
        original,
        &frame_scene,
        features,
    )?;
    let after = render(
        &gpu,
        &mut optimized_renderer,
        cooked,
        &frame_scene,
        features,
    )?;
    assert!(
        before
            .rgba
            .chunks_exact(4)
            .any(|p| p[..3] != before.rgba[..3])
    );
    assert_eq!(before.rgba, after.rgba);
    let mut raw_renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let raw = render_source(&gpu, &mut raw_renderer, optimized_raw, &frame_scene)?;
    assert_eq!(before.rgba, raw.rgba);
    println!(
        "lossless_geometry vertices=6 -> 3 upload_bytes={} -> {} exact_rgba=true",
        sizes[0], sizes[1]
    );
    Ok(())
}
