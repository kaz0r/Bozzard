use super::*;
use bozzard_render::{MaterialMap, ModelImage, ModelPart, ModelShading, SurfaceMaterialOverride};

const KEYS: [&str; 2] = ["0000000000000001", "0000000000000002"];

fn upload(gpu: &Gpu, renderer: &mut SceneRenderer, id: &str, factors: [[f32; 2]; 2]) -> Result<()> {
    let vertices: Vec<_> = [-0.5, 0.5]
        .into_iter()
        .flat_map(|x| {
            [
                [x - 0.35, -0.35],
                [x + 0.35, -0.35],
                [x + 0.35, 0.35],
                [x - 0.35, 0.35],
            ]
            .map(|[x, y]| [x, y, 0., 0., 0., 1., 0.5, 0.5])
        })
        .collect();
    let attributes = [[1., 0., 0., 1., 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5]; 4];
    let parts: Vec<_> = factors
        .iter()
        .enumerate()
        .map(|(index, &[metallic, roughness])| ModelPart {
            source_key: KEYS[index],
            start: index as u32 * 6,
            count: 6,
            color: [0.8, 0.6, 0.4, 1.],
            alpha_cutoff: None,
            image: None,
            shading: Some(ModelShading {
                vertex_start: index as u32 * 4,
                vertices: &attributes,
                metallic,
                roughness,
                normal_scale: 1.,
                occlusion_strength: 1.,
                emissive_factor: [0.; 3],
                double_sided: false,
                base_color_sampler: Default::default(),
                normal: None,
                occlusion: None,
                emissive: None,
                metallic_roughness: Some(MaterialMap {
                    image: ModelImage {
                        width: 1,
                        height: 1,
                        rgba: &[0, 180, 220, 255],
                    },
                    sampler: Default::default(),
                }),
            }),
        })
        .collect();
    renderer.upload_model(
        gpu,
        id,
        &vertices,
        &[0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
        &parts,
    )
}

pub(super) fn checks(gpu: &Gpu) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    upload(gpu, &mut renderer, "override-source", [[0.2, 0.7]; 2])?;
    let upload_stamp = renderer
        .model_upload_stats("override-source")
        .unwrap()
        .cpu_upload_ms
        .to_bits();
    let mut scene = RenderScene {
        fog: Default::default(),
        gi: None,
        lights: Vec::new(),
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: bozzard_render::Lighting {
            sun_direction: [0., 0., 1.],
            sun_intensity: 1.,
            ambient_intensity: 0.1,
            shadows: false,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(
            -1.5, 1.5, -1.5, 1.5, 0.1, 10.,
        ) * Mat4::from_translation(Vec3::new(0., 0., -3.)),
        items: [0.6, -0.6]
            .into_iter()
            .map(|y| DrawItem {
                model: Mat4::from_translation(Vec3::new(0., y, 0.)),
                mesh: MeshKind::Imported("override-source".into()),
                material: Material {
                    tint: [1.; 3],
                    uv_scale: [1.; 2],
                    texture: TextureKind::White,
                    lit: false,
                    surface_overrides: Default::default(),
                },
            })
            .collect(),
    };
    let render = |renderer: &mut SceneRenderer, scene: &RenderScene| {
        capture(gpu, renderer, scene, [128, 128])
    };
    let baseline = render(&mut renderer, &scene)?;
    let value = SurfaceMaterialOverride {
        surface: 0,
        source: KEYS[0].into(),
        tint: [0.25, 0.5, 1.],
        metallic: None,
        roughness: None,
    };
    scene.items[0].material.surface_overrides = vec![value.clone()].into();
    let tinted = render(&mut renderer, &scene)?;
    pixel(&tinted, 43, 38, [51, 77, 102])?;
    for (x, y) in [(85, 38), (43, 90), (85, 90)] {
        pixel(&tinted, x, y, [204, 153, 102])?;
    }
    scene.items[0].material.surface_overrides = vec![SurfaceMaterialOverride {
        source: KEYS[1].into(),
        ..value.clone()
    }]
    .into();
    ensure!(
        render(&mut renderer, &scene)?.rgba == baseline.rgba,
        "stale source signature tinted another surface"
    );
    scene.items[0].material.surface_overrides = Default::default();
    ensure!(
        render(&mut renderer, &scene)?.rgba == baseline.rgba,
        "reset did not restore source pixels"
    );
    for item in &mut scene.items {
        item.material.lit = true;
    }
    let lit_baseline = render(&mut renderer, &scene)?;
    for (metallic, roughness) in [
        (Some(0.9), None),
        (None, Some(0.18)),
        (Some(0.9), Some(0.18)),
    ] {
        scene.items[0].material.surface_overrides = vec![SurfaceMaterialOverride {
            tint: [1.; 3],
            metallic,
            roughness,
            ..value.clone()
        }]
        .into();
        let actual = render(&mut renderer, &scene)?;
        ensure!(
            actual.rgba != lit_baseline.rgba,
            "PBR factor override had no effect"
        );
        ensure!(
            actual.rgba[64 * 128 * 4..] == lit_baseline.rgba[64 * 128 * 4..],
            "shared model's second instance changed"
        );
        upload(
            gpu,
            &mut renderer,
            "override-reference",
            [
                [metallic.unwrap_or(0.2), roughness.unwrap_or(0.7)],
                [0.2, 0.7],
            ],
        )?;
        let mut reference = scene.clone();
        reference.items[0].mesh = MeshKind::Imported("override-reference".into());
        reference.items[0].material.surface_overrides = Default::default();
        ensure!(
            actual.rgba == render(&mut renderer, &reference)?.rgba,
            "override did not match authored PBR factors with existing maps"
        );
    }
    ensure!(
        renderer
            .model_upload_stats("override-source")
            .unwrap()
            .cpu_upload_ms
            .to_bits()
            == upload_stamp,
        "material edit uploaded shared source data"
    );
    for bad in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        scene.items[0].material.surface_overrides = vec![SurfaceMaterialOverride {
            metallic: Some(bad),
            ..value.clone()
        }]
        .into();
        ensure!(
            render(&mut renderer, &scene).is_err(),
            "renderer accepted invalid override"
        );
    }
    println!(
        "material_overrides_gpu_ok tint surface_and_instance_isolation reset stale_source pbr_factor_map_parity no_source_upload validation"
    );
    Ok(())
}
