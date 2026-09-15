//! The same glTF rig is cooked for headless sampling and deformed by the native GPU pipeline.
use bozzard_assets::{AssetData, AssetStore};
use bozzard_render::*;
use bozzard_scene::{AssetKind, AssetSource};
use glam::{Mat4, Vec3};
use std::{collections::BTreeMap, sync::Arc};
#[test]
fn gltf_skin_matches_cpu_reference_and_invalidates_shadows() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/assets");
    let mut assets = AssetStore::new(
        &root,
        &BTreeMap::from([(
            "banner".into(),
            AssetSource {
                kind: AssetKind::Mesh,
                path: "animated-banner.gltf".into(),
            },
        )]),
    )?;
    assets.load_pending()?;
    let data = assets
        .get(assets.handle("banner").unwrap())
        .unwrap()
        .shared_data()
        .unwrap();
    let AssetData::Mesh(mesh) = data.as_ref() else {
        unreachable!()
    };
    let skin = mesh.skin.as_ref().unwrap();
    assert_eq!(
        skin.rig
            .clips
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Bend", "Walk"]
    );
    let gpu = pollster::block_on(Gpu::request(
        &instance(Backend::native()),
        None,
        cfg!(not(target_os = "macos")),
    ))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut upload =
        renderer.begin_upload(&gpu, bozzard_render_assets::upload_source(data.clone())?)?;
    while !upload.progress().complete {
        upload.advance(&gpu, &renderer, 256 * 1024)?;
    }
    upload.finish(&mut renderer, "banner")?;
    let mut scene = RenderScene {
        skin_poses: BTreeMap::new(),
        shader_time: 0.,
        particles: vec![],
        fog: Default::default(),
        gi: None,
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        display: DisplaySettings::default(),
        lighting: Lighting {
            sun_direction: [0.3, -0.5, -1.],
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-2., 2., -1., 3., 0.1, 20.),
        items: vec![DrawItem {
            motion_id: 1,
            model: Mat4::from_translation(Vec3::new(0., 0., -5.)),
            mesh: MeshKind::Imported("banner".into()),
            material: Material {
                tint: [1.; 3],
                texture: TextureKind::White,
                lit: true,
                metallic: None,
                roughness: None,
                uv_scale: [1.; 2],
                surface_overrides: Default::default(),
                shader: None,
            },
        }],
    };
    let rest = skin.rig.palette(&skin.rig.rest_pose())?;
    scene.skin_poses.insert(
        1,
        SkinPose {
            signature: skin.rig.signature(),
            matrices: Arc::new(rest),
        },
    );
    let initial = capture_offscreen(&gpu, 64, 64, |target| {
        renderer.draw_linear(&gpu, target, [64, 64], &scene)
    })?;
    let palette = skin.rig.palette(&skin.rig.sample(0, 1.)?)?;
    scene.skin_poses.get_mut(&1).unwrap().matrices = Arc::new(palette.clone());
    let animated = capture_offscreen(&gpu, 64, 64, |target| {
        renderer.draw_linear(&gpu, target, [64, 64], &scene)
    })?;
    let mut direct = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    bozzard_render_assets::upload(&gpu, &mut direct, "banner", data.as_ref())?;
    let direct_frame = capture_offscreen(&gpu, 64, 64, |target| {
        direct.draw_linear(&gpu, target, [64, 64], &scene)
    })?;
    assert_eq!(
        animated.rgba, direct_frame.rgba,
        "direct upload discarded glTF skin data"
    );
    assert_ne!(
        initial.rgba, animated.rgba,
        "animation never changed rendered geometry"
    );
    assert!(
        !renderer.frame_stats().shadow_cache_hit,
        "deformation reused stale shadow maps"
    );
    let mut reference = mesh.clone();
    reference.skin = None;
    for (vertex, inf) in reference.vertices.iter_mut().zip(&skin.vertices) {
        let mut matrix = Mat4::ZERO;
        for axis in 0..4 {
            matrix +=
                Mat4::from_cols_array(&palette[inf[axis] as usize]) * f32::from_bits(inf[axis + 4]);
        }
        let p = matrix.transform_point3(Vec3::from_slice(&vertex[..3]));
        let n = matrix
            .inverse()
            .transpose()
            .transform_vector3(Vec3::from_slice(&vertex[3..6]))
            .normalize();
        vertex[..3].copy_from_slice(&p.to_array());
        vertex[3..6].copy_from_slice(&n.to_array());
    }
    let mut oracle = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let source = Arc::new(AssetData::Mesh(reference));
    let mut upload = oracle.begin_upload(&gpu, bozzard_render_assets::upload_source(source)?)?;
    while !upload.progress().complete {
        upload.advance(&gpu, &oracle, 256 * 1024)?;
    }
    upload.finish(&mut oracle, "banner")?;
    let mut reference_scene = scene.clone();
    reference_scene.skin_poses.clear();
    let expected = capture_offscreen(&gpu, 64, 64, |target| {
        oracle.draw_linear(&gpu, target, [64, 64], &reference_scene)
    })?;
    let delta = animated
        .rgba
        .iter()
        .zip(&expected.rgba)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(delta <= 2, "GPU skin differs from CPU oracle by {delta}");
    capture_offscreen(&gpu, 64, 64, |target| {
        renderer.draw_linear(&gpu, target, [64, 64], &scene)
    })?;
    assert!(
        renderer.frame_stats().shadow_cache_hit,
        "unchanged pose invalidated the shadow cache"
    );
    let valid_pose = scene.skin_poses[&1].clone();
    Arc::make_mut(&mut scene.skin_poses.get_mut(&1).unwrap().matrices)[0][0] = f32::NAN;
    let invalid = capture_offscreen(&gpu, 64, 64, |target| {
        renderer.draw_linear(&gpu, target, [64, 64], &scene)
    });
    assert!(
        invalid.is_err(),
        "changed invalid palette bypassed validation"
    );
    scene.skin_poses.insert(1, valid_pose);
    let restored = capture_offscreen(&gpu, 64, 64, |target| {
        renderer.draw_linear(&gpu, target, [64, 64], &scene)
    })?;
    assert_eq!(animated.rgba, restored.rgba);
    Ok(())
}
