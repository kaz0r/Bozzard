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
    let source = bozzard_render_assets::upload_source_with_features(data, features)?;
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
