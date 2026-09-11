use super::*;
use bozzard_render::{IrradianceVolume, ModelPart, ModelShading};
use std::{path::Path, sync::Arc};
const STRIDE: usize = 41;
fn volume(color: [f32; 3], resolution: [u32; 3]) -> IrradianceVolume {
    let mut data = vec![[0.; 4]; resolution.iter().product::<u32>() as usize * STRIDE];
    for p in data.chunks_exact_mut(STRIDE) {
        p[0] = [
            color[0] / 0.2820948,
            color[1] / 0.2820948,
            color[2] / 0.2820948,
            1.,
        ];
        p[9..].fill([10000., 100_000_000., 10000., 100_000_000.]);
    }
    IrradianceVolume {
        min: [-1.; 3],
        max: [1.; 3],
        resolution,
        intensity: 1.,
        normal_bias: 0.,
        probes: Arc::new(data),
    }
}
fn model(gpu: &Gpu, renderer: &mut SceneRenderer, pbr: bool, metallic: f32) -> Result<()> {
    let vertices = [
        [-0.8, -0.8, 0., 0., 0., 1., 0., 1.],
        [0.8, -0.8, 0., 0., 0., 1., 1., 1.],
        [0.8, 0.8, 0., 0., 0., 1., 1., 0.],
        [-0.8, 0.8, 0., 0., 0., 1., 0., 0.],
    ];
    let attrs = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 4];
    renderer.upload_model(
        gpu,
        "gi-receiver",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            start: 0,
            count: 6,
            source_key: "",
            color: [1.; 4],
            alpha_cutoff: None,
            image: None,
            shading: pbr.then_some(ModelShading {
                vertex_start: 0,
                vertices: &attrs,
                metallic,
                roughness: 1.,
                normal_scale: 1.,
                occlusion_strength: 1.,
                emissive_factor: [0.; 3],
                double_sided: false,
                base_color_sampler: Default::default(),
                normal: None,
                metallic_roughness: None,
                occlusion: None,
                emissive: None,
            }),
        }],
    )
}
pub(super) fn checks(gpu: &Gpu) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    model(gpu, &mut renderer, false, 0.)?;
    let mut scene = RenderScene {
        gi: Some(volume([0.2, 0.4, 0.6], [2; 3])),
        lights: vec![],
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: bozzard_render::Lighting {
            sun_intensity: 0.,
            ambient_intensity: 0.,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.)),
        items: vec![DrawItem {
            model: Mat4::IDENTITY,
            mesh: MeshKind::Imported("gi-receiver".into()),
            material: Material {
                surface_overrides: Default::default(),
                tint: [1.; 3],
                uv_scale: [1.; 2],
                texture: TextureKind::White,
                lit: true,
            },
        }],
    };
    for size in [[64, 64], [97, 53], [3, 5], [1, 1]] {
        pixel(
            &capture(gpu, &mut renderer, &scene, size)?,
            size[0] / 2,
            size[1] / 2,
            [51, 102, 153],
        )?;
    }
    let original = scene.gi.clone();
    scene.gi = None;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [0; 3],
    )?;
    scene.gi = original;
    scene.gi.as_mut().unwrap().intensity = 0.5;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [26, 51, 77],
    )?;
    scene.gi.as_mut().unwrap().intensity = 0.;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [0; 3],
    )?;
    scene.gi = Some(volume([0.2, 0.4, 0.6], [2; 3]));
    model(gpu, &mut renderer, true, 0.)?;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [49, 98, 147],
    )?;
    model(gpu, &mut renderer, true, 1.)?;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [0; 3],
    )?;
    scene.items[0].material.lit = false;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [255; 3],
    )?;
    scene.items[0].material.lit = true;
    model(gpu, &mut renderer, false, 0.)?;
    // Coefficient direction: a +Z receiver evaluates l=1,z analytically.
    scene.gi = Some(volume([0.; 3], [2; 3]));
    for p in Arc::make_mut(&mut scene.gi.as_mut().unwrap().probes).chunks_exact_mut(STRIDE) {
        p[2] = [0.2 / 0.48860252, 0.4 / 0.48860252, 0.6 / 0.48860252, 0.];
    }
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [51, 102, 153],
    )?;
    // Visibility should exclude red probes behind an intervening surface.
    scene.gi = Some(volume([0.; 3], [2; 3]));
    for (i, p) in Arc::make_mut(&mut scene.gi.as_mut().unwrap().probes)
        .chunks_exact_mut(STRIDE)
        .enumerate()
    {
        p[0] = if i < 4 {
            [1. / 0.2820948, 0., 0., 1.]
        } else {
            [0., 1. / 0.2820948, 0., 1.]
        };
    }
    let open = capture(gpu, &mut renderer, &scene, [64, 64])?;
    ensure!(
        open.rgba[(32 * 64 + 32) * 4] > 10,
        "visibility fixture has no red contribution"
    );
    for p in Arc::make_mut(&mut scene.gi.as_mut().unwrap().probes)
        .chunks_exact_mut(STRIDE)
        .take(4)
    {
        p[9..].fill([0.05, 0.0025, 0.05, 0.0025]);
    }
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [0, 255, 0],
    )?;
    for p in Arc::make_mut(&mut scene.gi.as_mut().unwrap().probes).chunks_exact_mut(STRIDE) {
        p[0][3] = 0.;
    }
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [0; 3],
    )?;
    // Replacement, largest legal grid, bounds fallback and uniform-only invalidation.
    scene.gi = Some(volume([0.3; 3], [16; 3]));
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [77; 3],
    )?;
    scene.gi.as_mut().unwrap().min = [2.; 3];
    scene.gi.as_mut().unwrap().max = [4.; 3];
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [0; 3],
    )?;
    scene.gi.as_mut().unwrap().intensity = f32::NAN;
    ensure!(
        capture(gpu, &mut renderer, &scene, [64, 64]).is_err(),
        "GI uniform validation missed"
    );
    scene.gi = Some(volume([0.2; 3], [2; 3]));
    Arc::make_mut(&mut scene.gi.as_mut().unwrap().probes)[9][0] = -1.;
    ensure!(
        capture(gpu, &mut renderer, &scene, [64, 64]).is_err(),
        "GI data validation missed"
    );
    println!(
        "gi_gpu_ok constant_energy directional_sh diffuse_pbr metallic unlit intensity visibility invalid_probes replacement max_grid outside_bounds resize validation"
    );
    Ok(())
}

/// The real CPU bake -> scene persistence -> player extraction -> GPU path.
pub(super) fn baked_room(gpu: &Gpu, output: &Path) -> Result<()> {
    let mut document = bozzard_scene::Scene::from_json(include_str!(
        "../../../../examples/demo/scenes/gi-lab.json"
    ))?;
    document.gi.volume.resolution = [4; 3];
    document.gi.volume.samples = 128;
    document.gi.volume.bounces = 2;
    let assets = bozzard_assets::AssetStore::new(Path::new("."), &document.assets)?;
    let baked =
        bozzard_assets::gi::bake(&document, &assets, document.gi.volume, &Default::default())?;
    document.gi.baked = Some(Arc::new(baked));
    document.gi.enabled = true;
    let document = bozzard_scene::Scene::from_json(&document.to_json()?)?;
    let demo = bozzard_demo::SceneDemo::new(&document)?;
    let mut scene = extract(&demo, &assets, Layer::ThreeD, 1.6)?;
    ensure!(
        scene.gi.is_some(),
        "CPU bake failed to reach renderer after scene roundtrip"
    );
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let on = capture_display(gpu, &mut renderer, &scene, [320, 200])?;
    scene.gi = None;
    let off = capture_display(gpu, &mut renderer, &scene, [320, 200])?;
    let (mut affected, mut color_bounce) = (0, 0);
    for (a, b) in on.rgba.chunks_exact(4).zip(off.rgba.chunks_exact(4)) {
        if a[..3].iter().zip(b).any(|(a, b)| a.abs_diff(*b) > 4) {
            affected += 1;
        }
        // Previously neutral surfaces gain green or red from the colored walls.
        if b[0].abs_diff(b[1]) < 4
            && b[1].abs_diff(b[2]) < 4
            && b[0] > 10
            && (a[0] as i32 - a[1] as i32 > 5 || a[1] as i32 - a[0] as i32 > 5)
        {
            color_bounce += 1;
        }
    }
    on.write_ppm(&output.join("gi-room-on.ppm"))?;
    off.write_ppm(&output.join("gi-room-off.ppm"))?;
    ensure!(
        affected > 500 && color_bounce > 50,
        "CPU/GPU GI room lacks indirect color transfer: affected={affected}, colored={color_bounce}"
    );
    println!(
        "gi_baked_room_gpu_ok affected_pixels={affected} color_bounce_pixels={color_bounce} cpu_bake source scene_roundtrip player_extraction"
    );
    Ok(())
}
