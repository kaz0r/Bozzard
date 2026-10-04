use bozzard_render::*;
use glam::{Mat4, Vec3};
use std::sync::Arc;

fn material(color: [f32; 3]) -> Material {
    Material {
        metallic: None,
        roughness: None,
        tint: color,
        uv_scale: [1.; 2],
        texture: TextureKind::White,
        lit: false,
        shader: None,
        surface_overrides: Default::default(),
    }
}
fn item(id: u64, x: f32, mesh: MeshKind) -> DrawItem {
    DrawItem {
        motion_id: id,
        model: Mat4::from_translation(Vec3::new(x, 0., -5.)),
        mesh,
        material: material([0.3, 0.8, 0.6]),
    }
}
fn scene() -> anyhow::Result<RenderScene> {
    let mut text = item(4, -3., MeshKind::Text(TextMesh::default()));
    text.model *= Mat4::from_translation(Vec3::new(0., 1.5, 0.));
    text.material.texture = TextureKind::Text;
    let mut sprite = item(
        5,
        2.,
        MeshKind::Sprite(SpriteMesh::new(vec![SpriteQuad {
            rect: [0., 1.5, 1., 1.],
            uv: [0., 0., 1., 1.],
        }])?),
    );
    sprite.material.texture = TextureKind::Checker;
    Ok(RenderScene {
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
        view_projection: glam::camera::rh::proj::directx::orthographic(-4., 4., -3., 3., 0.1, 30.),
        items: vec![
            item(1, -2., MeshKind::Imported("model".into())),
            item(2, 1., MeshKind::ModelPart("model".into(), 1)),
            item(3, 0., MeshKind::Cube),
            text,
            sprite,
        ],
        shader_time: 0.,
    })
}
const VERTICES: [[f32; 8]; 6] = [
    [-0.8, -0.8, 0., 0., 0., 1., 0., 1.],
    [0., -0.8, 0., 0., 0., 1., 1., 1.],
    [-0.4, 0.8, 0., 0., 0., 1., 0.5, 0.],
    [0., -0.8, 0.2, 0., 0., 1., 0., 1.],
    [0.8, -0.8, 0.2, 0., 0., 1., 1., 1.],
    [0.4, 0.8, 0.2, 0., 0., 1., 0.5, 0.],
];
fn upload(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    alpha: f32,
    skinned: bool,
) -> anyhow::Result<()> {
    let parts = [
        ModelPart {
            source_key: "0000000000000000",
            start: 0,
            count: 3,
            color: [1., 0.5, 0.4, 1.],
            alpha_cutoff: None,
            image: None,
            shading: None,
        },
        ModelPart {
            source_key: "1111111111111111",
            start: 3,
            count: 3,
            color: [0.4, 0.6, 1., alpha],
            alpha_cutoff: None,
            image: None,
            shading: None,
        },
    ];
    if skinned {
        renderer.upload_skinned_model(
            gpu,
            "model",
            &VERTICES,
            &[0, 1, 2, 3, 4, 5],
            &parts,
            SkinData {
                signature: 7,
                bindings: 1,
                vertices: &[[0, 0, 0, 0, 1f32.to_bits(), 0, 0, 0]; 6],
            },
        )
    } else {
        renderer.upload_model(gpu, "model", &VERTICES, &[0, 1, 2, 3, 4, 5], &parts)
    }
}
fn compare(
    gpu: &Gpu,
    renderers: &mut [SceneRenderer; 2],
    scene: &RenderScene,
) -> anyhow::Result<()> {
    let mut frames = Vec::new();
    for renderer in renderers.iter_mut() {
        frames.push(capture_offscreen(gpu, 256, 192, |target| {
            renderer.draw(gpu, target, [256, 192], scene)
        })?);
    }
    assert_eq!(
        frames[0].rgba, frames[1].rgba,
        "retained and per-frame preparation differ"
    );
    let reference = renderers[0].frame_stats();
    let retained = renderers[1].frame_stats();
    for stats in [reference, retained] {
        assert_eq!(
            stats.resource_metadata_hits
                + stats.resource_metadata_builds
                + stats.resource_metadata_bypasses,
            stats.surfaces
        );
        assert_eq!(
            stats.resource_bounds_lookups,
            stats.resource_metadata_builds + stats.resource_metadata_bypasses
        );
    }
    assert_eq!(reference.surfaces, retained.surfaces);
    assert_eq!(reference.visible_surfaces, retained.visible_surfaces);
    assert_eq!(reference.color_triangles, retained.color_triangles);
    assert!(
        frames[0]
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[1] > pixel[0].saturating_add(10)),
        "fixture must render colored geometry"
    );
    Ok(())
}
#[test]
fn retained_surfaces_match_reference_through_edits_reuploads_skinning_and_failed_frames()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_surface_preparation_caching_enabled(false);
    for renderer in &mut renderers {
        upload(&gpu, renderer, 0.45, false)?;
    }
    let mut scene = scene()?;
    compare(&gpu, &mut renderers, &scene)?;
    compare(&gpu, &mut renderers, &scene)?;
    let warm = renderers[1].frame_stats();
    assert_eq!(warm.surface_items_rebuilt, 0);
    assert_eq!(warm.surface_records_built, 0);
    assert_eq!(warm.surface_records_reused, 6);
    assert_eq!(warm.surface_model_updates, 0);
    assert_eq!(warm.surface_depth_updates, 0);
    assert!(warm.surface_order_reused);

    scene.view_projection *= Mat4::from_translation(Vec3::new(0.17, 0.11, 0.));
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().surface_depth_updates, 6);
    assert_eq!(renderers[1].frame_stats().surface_records_built, 0);
    scene.items[0].model *=
        Mat4::from_rotation_y(0.4) * Mat4::from_scale(Vec3::new(-1.2, 0.9, 1.7));
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().surface_model_updates, 2);
    assert_eq!(renderers[1].frame_stats().surface_records_built, 0);
    scene.items[0].material.tint[0] = 0.;
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().surface_items_rebuilt, 1);
    assert_eq!(renderers[1].frame_stats().surface_records_built, 2);
    assert_eq!(renderers[1].frame_stats().surface_records_reused, 4);
    scene.items[0].material.tint[0] = -0.;
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().surface_records_built, 2);

    scene.items[0].material.surface_overrides = Arc::from([SurfaceMaterialOverride {
        surface: 1,
        source: "1111111111111111".into(),
        transform: Mat4::from_rotation_z(0.23) * Mat4::from_scale(Vec3::new(1.2, 0.8, 1.)),
        texture: None,
        uv_scale: [2., 1.],
        tint: [0.9, 0.2, 0.7],
        metallic: Some(0.1),
        roughness: Some(0.7),
    }]);
    compare(&gpu, &mut renderers, &scene)?;
    scene.items[0].model *= Mat4::from_rotation_y(-0.7);
    scene.items[1].model *= Mat4::from_translation(Vec3::new(-0.7, 0., -0.5));
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().surface_records_built, 0);
    scene.items.swap(0, 1);
    compare(&gpu, &mut renderers, &scene)?;
    scene.items.remove(2);
    compare(&gpu, &mut renderers, &scene)?;
    if let MeshKind::Text(text) = &mut scene.items[2].mesh {
        text.text = "Changed".into();
    }
    if let MeshKind::Sprite(sprite) = &mut scene.items[3].mesh {
        sprite.opacity = 0.35;
    }
    compare(&gpu, &mut renderers, &scene)?;

    for renderer in &mut renderers {
        upload(&gpu, renderer, 1., true)?;
    }
    for id in [1, 2] {
        scene.skin_poses.insert(
            id,
            SkinPose {
                signature: 7,
                matrices: Arc::new(vec![Mat4::IDENTITY.to_cols_array()]),
            },
        );
    }
    compare(&gpu, &mut renderers, &scene)?;
    scene.display.temporal_aa.enabled = true;
    scene.display.motion_blur.enabled = true;
    for tick in 1..4 {
        scene.display.time_seconds = tick as f32 / 60.;
        scene.skin_poses.get_mut(&1).unwrap().matrices = Arc::new(vec![
            Mat4::from_translation(Vec3::new(0.1 * tick as f32, 0., 0.2 * tick as f32))
                .to_cols_array(),
        ]);
        compare(&gpu, &mut renderers, &scene)?;
        assert_eq!(renderers[1].frame_stats().surface_records_built, 0);
    }
    // Both early validation and post-preparation resource errors must allow a clean retry.
    let original_vp = scene.view_projection;
    scene.view_projection = Mat4::ZERO;
    for renderer in &mut renderers {
        assert!(
            capture_offscreen(&gpu, 256, 192, |target| renderer.draw(
                &gpu,
                target,
                [256, 192],
                &scene
            ))
            .is_err()
        );
    }
    scene.view_projection = original_vp;
    compare(&gpu, &mut renderers, &scene)?;
    scene.items[0].material.texture = TextureKind::Imported("late-image".into());
    for renderer in &mut renderers {
        assert!(
            capture_offscreen(&gpu, 256, 192, |target| renderer.draw(
                &gpu,
                target,
                [256, 192],
                &scene
            ))
            .is_err()
        );
        assert_eq!(renderer.frame_stats().surface_preparation_bytes, 0);
        renderer.upload_image(&gpu, "late-image", 1, 1, &[230, 170, 100, 120])?;
    }
    compare(&gpu, &mut renderers, &scene)?;
    for renderer in &mut renderers {
        renderer.upload_image(&gpu, "late-image", 1, 1, &[80, 210, 160, 255])?;
    }
    compare(&gpu, &mut renderers, &scene)?;
    scene.items.clear();
    scene.skin_poses.clear();
    for renderer in &mut renderers {
        renderer.remove_asset("model");
    }
    // Empty scene intentionally has no colored geometry; only exact parity applies here.
    let frames: Vec<_> = renderers
        .iter_mut()
        .map(|renderer| {
            capture_offscreen(&gpu, 256, 192, |target| {
                renderer.draw(&gpu, target, [256, 192], &scene)
            })
        })
        .collect::<anyhow::Result<_>>()?;
    assert_eq!(frames[0].rgba, frames[1].rgba);
    assert_eq!(renderers[1].frame_stats().surfaces, 0);
    Ok(())
}

#[test]
fn resource_certificates_reuse_static_surfaces_bypass_dynamic_geometry_and_respect_switches()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_resource_metadata_caching_enabled(false);
    for renderer in &mut renderers {
        upload(&gpu, renderer, 0.45, false)?;
    }
    let mut scene = scene()?;
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_builds, 4);
    compare(&gpu, &mut renderers, &scene)?;
    let warm = renderers[1].frame_stats();
    assert_eq!(warm.resource_metadata_hits, 4);
    assert_eq!(warm.resource_metadata_builds, 0);
    assert_eq!(warm.resource_metadata_bypasses, 2); // Text and sprite.
    assert_eq!(warm.resource_bounds_lookups, 2);
    assert_eq!(warm.mesh_validation_checks, 0);
    assert_eq!(renderers[0].frame_stats().mesh_validation_checks, 3);
    assert_eq!(renderers[0].frame_stats().resource_bounds_lookups, 6);
    assert!(warm.resource_metadata_bytes > 0);
    assert!(warm.resource_metadata_bytes < warm.surface_preparation_bytes);

    scene.view_projection *= Mat4::from_translation(Vec3::new(0.17, 0.11, 0.));
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 4);
    assert_eq!(renderers[1].frame_stats().object_matrix_checks, 6);
    assert_eq!(renderers[1].frame_stats().object_uniform_builds, 0);
    scene.items[0].model *= Mat4::from_rotation_y(0.3);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 4);
    scene.items[0].material.tint[0] = 0.4;
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_builds, 2);
    assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 2);
    scene.items.swap(0, 1);
    compare(&gpu, &mut renderers, &scene)?;

    renderers[1].set_resource_metadata_caching_enabled(false);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().surface_records_built, 0);
    assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 0);
    assert_eq!(renderers[1].frame_stats().resource_metadata_bytes, 0);
    assert_eq!(renderers[1].frame_stats().resource_bounds_lookups, 6);
    renderers[1].set_resource_metadata_caching_enabled(true);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_builds, 4);
    renderers[1].set_surface_preparation_caching_enabled(false);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_builds, 0);
    assert_eq!(renderers[1].frame_stats().resource_metadata_bypasses, 6);
    renderers[1].set_surface_preparation_caching_enabled(true);
    renderers[1].set_state_caching_enabled(false);
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_bypasses, 6);
    assert_eq!(renderers[1].frame_stats().resource_metadata_bytes, 0);
    renderers[1].set_state_caching_enabled(true);

    for renderer in &mut renderers {
        upload(&gpu, renderer, 0.45, true)?;
    }
    for id in [1, 2] {
        scene.skin_poses.insert(
            id,
            SkinPose {
                signature: 7,
                matrices: Arc::new(vec![Mat4::IDENTITY.to_cols_array()]),
            },
        );
    }
    compare(&gpu, &mut renderers, &scene)?;
    for x in [0.2, 8., 0.] {
        scene.skin_poses.get_mut(&1).unwrap().matrices = Arc::new(vec![
            Mat4::from_translation(Vec3::new(x, 0., 0.4)).to_cols_array(),
        ]);
        compare(&gpu, &mut renderers, &scene)?;
        let dynamic = renderers[1].frame_stats();
        assert_eq!(dynamic.resource_metadata_hits, 1); // Only the cube is static.
        assert_eq!(dynamic.resource_metadata_builds, 0);
        assert_eq!(dynamic.resource_metadata_bypasses, 5);
        assert_eq!(dynamic.resource_bounds_lookups, 5);
        assert_eq!(dynamic.mesh_validation_checks, 3);
    }
    scene.items.clear();
    let frames: Vec<_> = renderers
        .iter_mut()
        .map(|renderer| {
            capture_offscreen(&gpu, 256, 192, |target| {
                renderer.draw(&gpu, target, [256, 192], &scene)
            })
        })
        .collect::<anyhow::Result<_>>()?;
    assert_eq!(frames[0].rgba, frames[1].rgba);
    assert_eq!(renderers[1].frame_stats().resource_metadata_bytes, 0);
    assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 0);
    Ok(())
}

#[test]
fn resource_certificates_invalidate_on_mesh_publication_removal_alias_and_failed_retry()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_resource_metadata_caching_enabled(false);
    let mut scene = scene()?;
    scene.items = vec![
        item(1, 0., MeshKind::Imported("raw".into())),
        item(2, 2., MeshKind::Cube),
    ];
    for renderer in &mut renderers {
        renderer.upload_mesh(&gpu, "raw", &VERTICES, &[0, 1, 2, 3, 4, 5])?;
    }
    compare(&gpu, &mut renderers, &scene)?;
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 2);
    let before = renderers[1].frame_stats().visible_surfaces;
    let mut shifted = VERTICES;
    for vertex in &mut shifted {
        vertex[0] += 50.;
    }
    for renderer in &mut renderers {
        renderer.upload_mesh(&gpu, "raw", &shifted, &[0, 1, 2, 3, 4, 5])?;
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_builds, 2);
    assert_eq!(renderers[1].frame_stats().visible_surfaces, before - 1);
    for renderer in &mut renderers {
        assert!(
            renderer
                .upload_mesh(&gpu, "raw", &VERTICES, &[0, 1, 99])
                .is_err()
        );
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 2); // Failed upload preserves publication.

    for renderer in &mut renderers {
        renderer.remove_asset("raw");
    }
    for _ in 0..2 {
        let errors: Vec<_> = renderers
            .iter_mut()
            .map(|renderer| {
                let error = capture_offscreen(&gpu, 256, 192, |target| {
                    renderer.draw(&gpu, target, [256, 192], &scene)
                })
                .err()
                .expect("removed mesh must still fail validation");
                assert_eq!(renderer.frame_stats().resource_metadata_bytes, 0);
                error.to_string()
            })
            .collect();
        assert_eq!(errors[0], errors[1]);
        assert!(errors[0].contains("mesh 'raw' is not uploaded"));
    }
    for renderer in &mut renderers {
        renderer.upload_mesh(&gpu, "raw", &VERTICES, &[0, 1, 2, 3, 4, 5])?;
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_builds, 2);
    compare(&gpu, &mut renderers, &scene)?;
    scene.items[1].material.texture = TextureKind::Imported("missing".into());
    for renderer in &mut renderers {
        assert!(
            capture_offscreen(&gpu, 256, 192, |target| renderer.draw(
                &gpu,
                target,
                [256, 192],
                &scene
            ))
            .is_err()
        );
        assert_eq!(renderer.frame_stats().resource_metadata_bytes, 0);
        renderer.upload_image(&gpu, "image", 1, 1, &[80, 210, 160, 255])?;
        renderer.alias_image("image", "missing")?;
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 0);
    assert_eq!(renderers[1].frame_stats().resource_metadata_builds, 2);
    Ok(())
}

#[test]
fn resource_certificates_refresh_sidedness_and_model_expansion_on_same_id_publication()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_resource_metadata_caching_enabled(false);
    let mut scene = scene()?;
    scene.items = vec![item(1, 0., MeshKind::Imported("sided".into()))];
    scene.items[0].model *= Mat4::from_rotation_y(std::f32::consts::PI);
    let attrs = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 6];
    let mut pixels = Vec::new();
    for double_sided in [false, true, false] {
        for renderer in &mut renderers {
            renderer.upload_model(
                &gpu,
                "sided",
                &VERTICES,
                &[0, 1, 2],
                &[ModelPart {
                    source_key: "0000000000000000",
                    start: 0,
                    count: 3,
                    color: [1.; 4],
                    alpha_cutoff: None,
                    image: None,
                    shading: Some(ModelShading {
                        vertex_start: 0,
                        vertices: &attrs,
                        metallic: 0.,
                        roughness: 0.6,
                        normal_scale: 1.,
                        occlusion_strength: 1.,
                        emissive_factor: [0.; 3],
                        double_sided,
                        base_color_sampler: Default::default(),
                        normal: None,
                        metallic_roughness: None,
                        occlusion: None,
                        emissive: None,
                    }),
                }],
            )?;
        }
        let mut frames = Vec::new();
        for renderer in &mut renderers {
            frames.push(capture_offscreen(&gpu, 256, 192, |target| {
                renderer.draw(&gpu, target, [256, 192], &scene)
            })?);
        }
        assert_eq!(frames[0].rgba, frames[1].rgba);
        assert_eq!(renderers[1].frame_stats().resource_metadata_builds, 1);
        assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 0);
        pixels.push(frames.remove(0).rgba);
        for renderer in &mut renderers {
            capture_offscreen(&gpu, 256, 192, |target| {
                renderer.draw(&gpu, target, [256, 192], &scene)
            })?;
        }
        assert_eq!(renderers[1].frame_stats().resource_metadata_hits, 1);
        assert_eq!(renderers[1].frame_stats().mesh_validation_checks, 0);
    }
    assert_ne!(pixels[0], pixels[1], "fixture must exercise sidedness");
    assert_eq!(pixels[0], pixels[2]);
    // The same catalog ID can change from expanded PBR model to ordinary imported mesh.
    for renderer in &mut renderers {
        renderer.upload_mesh(&gpu, "sided", &VERTICES, &[0, 1, 2, 3, 4, 5])?;
    }
    compare(&gpu, &mut renderers, &scene)?;
    assert_eq!(renderers[1].frame_stats().resource_metadata_builds, 1);
    assert_eq!(renderers[1].frame_stats().mesh_validation_checks, 1);
    // A raw mesh cannot satisfy a model-surface reference, even after a warm certificate.
    scene.items[0].mesh = MeshKind::ModelPart("sided".into(), 0);
    for renderer in &mut renderers {
        let error = capture_offscreen(&gpu, 256, 192, |target| {
            renderer.draw(&gpu, target, [256, 192], &scene)
        })
        .err()
        .expect("missing model surface must fail");
        assert_eq!(error.to_string(), "model surface is not uploaded");
        assert_eq!(renderer.frame_stats().resource_metadata_bytes, 0);
    }
    scene.items[0].mesh = MeshKind::Imported("sided".into());
    compare(&gpu, &mut renderers, &scene)?;
    for renderer in &mut renderers {
        renderer.clear_imported();
    }
    for renderer in &mut renderers {
        assert!(
            capture_offscreen(&gpu, 256, 192, |target| {
                renderer.draw(&gpu, target, [256, 192], &scene)
            })
            .is_err()
        );
        assert_eq!(renderer.frame_stats().resource_metadata_bytes, 0);
    }
    Ok(())
}

#[test]
fn skin_deformation_resorts_overlapping_translucent_surfaces_and_restores_stable_ties()
-> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request_prefer_software(&instance(Backend::native())))?;
    let mut renderers: [SceneRenderer; 2] =
        std::array::from_fn(|_| SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm));
    renderers[0].set_surface_preparation_caching_enabled(false);
    let triangle = [
        [-1., -1., 0., 0., 0., 1., 0., 1.],
        [1., -1., 0., 0., 0., 1., 1., 1.],
        [0., 1., 0., 0., 0., 1., 0.5, 0.],
    ];
    let vertices: Vec<_> = triangle.into_iter().chain(triangle).collect();
    let parts = [
        ModelPart {
            source_key: "0000000000000000",
            start: 0,
            count: 3,
            color: [1., 1., 1., 0.5],
            alpha_cutoff: None,
            image: None,
            shading: None,
        },
        ModelPart {
            source_key: "1111111111111111",
            start: 3,
            count: 3,
            color: [1., 1., 1., 0.5],
            alpha_cutoff: None,
            image: None,
            shading: None,
        },
    ];
    for renderer in &mut renderers {
        renderer.upload_skinned_model(
            &gpu,
            "overlap",
            &vertices,
            &[0, 1, 2, 3, 4, 5],
            &parts,
            SkinData {
                signature: 21,
                bindings: 1,
                vertices: &[[0, 0, 0, 0, 1f32.to_bits(), 0, 0, 0]; 6],
            },
        )?;
    }
    let mut scene = scene()?;
    scene.items = vec![
        item(1, 0., MeshKind::ModelPart("overlap".into(), 0)),
        item(2, 0., MeshKind::ModelPart("overlap".into(), 1)),
    ];
    scene.items[0].material.tint = [1., 0.1, 0.1];
    scene.items[1].material.tint = [0.1, 1., 0.1];
    let mut center_colors = Vec::new();
    // Only palettes change. The first surface crosses the second, reaches an
    // exact tie, then returns behind it without rebuilding static surface data.
    for first_depth in [-0.4, 0.8, 0.4, -0.4] {
        for (id, depth) in [(1, first_depth), (2, 0.4)] {
            scene.skin_poses.insert(
                id,
                SkinPose {
                    signature: 21,
                    matrices: Arc::new(vec![
                        Mat4::from_translation(Vec3::Z * depth).to_cols_array(),
                    ]),
                },
            );
        }
        let mut frames = Vec::new();
        for renderer in &mut renderers {
            frames.push(capture_offscreen(&gpu, 128, 96, |target| {
                renderer.draw(&gpu, target, [128, 96], &scene)
            })?);
        }
        assert_eq!(
            frames[0].rgba, frames[1].rgba,
            "deformation-driven transparent order differs"
        );
        let center = (48 * 128 + 64) * 4;
        center_colors.push(frames[0].rgba[center..center + 3].to_vec());
        if center_colors.len() > 1 {
            let stats = renderers[1].frame_stats();
            assert_eq!(stats.surface_records_built, 0);
            assert_eq!(stats.surface_records_reused, 2);
            assert_eq!(stats.surface_model_updates, 0);
            assert!(stats.surface_depth_updates > 0);
            assert!(!stats.surface_order_reused);
        }
    }
    assert_ne!(
        center_colors[0], center_colors[1],
        "crossing palettes must change visible compositing"
    );
    assert_eq!(center_colors[0], center_colors[3]);
    Ok(())
}
