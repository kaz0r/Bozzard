use super::*;
use bozzard_render::{Lighting, ModelImage, ModelPart, ModelShading};

pub(super) fn checks(gpu: &Gpu) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let vertices = [
        [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
        [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
        [0.5, 0.5, 0., 0., 0., 1., 1., 0.],
        [-0.5, 0.5, 0., 0., 0., 1., 0., 0.],
    ];
    let attributes = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 4];
    let shading = ModelShading {
        vertex_start: 0,
        vertices: &attributes,
        metallic: 0.,
        roughness: 1.,
        normal_scale: 1.,
        occlusion_strength: 1.,
        emissive_factor: [0., 0.2, 0.],
        double_sided: false,
        base_color_sampler: wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        },
        normal: None,
        metallic_roughness: None,
        occlusion: None,
        emissive: None,
    };
    renderer.upload_model(
        gpu,
        "receiver",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            start: 0,
            count: 6,
            color: [1.; 4],
            alpha_cutoff: None,
            image: None,
            shading: Some(shading.clone()),
        }],
    )?;
    let receiver = DrawItem {
        model: Mat4::from_scale(Vec3::new(4., 4., 1.)),
        mesh: MeshKind::Imported("receiver".into()),
        material: Material {
            tint: [1.; 3],
            uv_scale: [1.; 2],
            texture: TextureKind::White,
            lit: true,
        },
    };
    let caster = DrawItem {
        model: Mat4::from_translation(Vec3::new(0.6, 0., 1.))
            * Mat4::from_scale(Vec3::new(0.6, 0.6, 1.)),
        mesh: MeshKind::Quad,
        material: receiver.material.clone(),
    };
    let mut scene = RenderScene {
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: Lighting {
            sun_direction: [1., 0., 1.],
            sun_intensity: 2.,
            ambient_intensity: 0.1,
            shadow_resolution: 512,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.)),
        items: vec![receiver, caster],
    };
    let read = |frame: &Frame, x: f32| -> [u8; 3] {
        let px = ((x + 2.) * 32.).floor() as usize;
        frame.rgba[(64 * 128 + px) * 4..(64 * 128 + px) * 4 + 3]
            .try_into()
            .unwrap()
    };
    let shadowed = capture(gpu, &mut renderer, &scene, [128, 128])?;
    let shadow = read(&shadowed, -0.4);
    ensure!(
        (24..=27).contains(&shadow[0]) && (75..=79).contains(&shadow[1]),
        "shadow must preserve ambient and emissive: {shadow:?}"
    );
    scene.lighting.shadows = false;
    let clear = capture(gpu, &mut renderer, &scene, [128, 128])?;
    let sunlit = read(&clear, -0.4);
    ensure!(
        sunlit[0] > shadow[0] + 70,
        "shadow toggle did not restore direct light: {sunlit:?} {shadow:?}"
    );
    ensure!(
        shadowed
            .rgba
            .chunks_exact(4)
            .zip(clear.rgba.chunks_exact(4))
            .any(|(a, b)| a[0] > shadow[0] + 5 && a[0] + 5 < b[0]),
        "PCF edge has no intermediate coverage"
    );
    scene.lighting.shadows = true;
    scene.lighting.sun_direction = [-1., 0., 1.];
    let moved = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&moved, -0.4)[0] > shadow[0] + 70 && read(&moved, 1.6)[0] < 30,
        "shadow failed to follow sun direction"
    );
    scene.lighting.sun_direction = [1., 0., 1.];
    let mut cutout = shading.clone();
    cutout.emissive_factor = [0.; 3];
    renderer.upload_model(
        gpu,
        "cutout",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            start: 0,
            count: 6,
            color: [1.; 4],
            alpha_cutoff: Some(0.5),
            image: Some(ModelImage {
                width: 2,
                height: 1,
                rgba: &[255, 255, 255, 0, 255, 255, 255, 255],
            }),
            shading: Some(cutout.clone()),
        }],
    )?;
    scene.items[1].mesh = MeshKind::Imported("cutout".into());
    let masked = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&masked, -0.55)[0] > 90 && read(&masked, -0.25)[0] < 30,
        "cutout shadow ignored texture alpha"
    );
    scene.items[1].model *= Mat4::from_scale(Vec3::new(-1., 1., 1.));
    let mirrored = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&mirrored, -0.55)[0] < 30 && read(&mirrored, -0.25)[0] > 90,
        "mirrored caster winding or UVs incorrect"
    );
    scene.items[1].model = Mat4::from_translation(Vec3::new(0.6, 0., 1.))
        * Mat4::from_scale(Vec3::new(0.6, 0.6, 1.))
        * Mat4::from_rotation_y(std::f32::consts::PI);
    let back = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&back, -0.55)[0] > 90 && read(&back, -0.25)[0] > 90,
        "single-sided backface cast a shadow"
    );
    cutout.double_sided = true;
    renderer.upload_model(
        gpu,
        "cutout",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            start: 0,
            count: 6,
            color: [1.; 4],
            alpha_cutoff: Some(0.5),
            image: Some(ModelImage {
                width: 2,
                height: 1,
                rgba: &[255, 255, 255, 0, 255, 255, 255, 255],
            }),
            shading: Some(cutout.clone()),
        }],
    )?;
    let double = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&double, -0.55)[0] < 30 && read(&double, -0.25)[0] > 90,
        "double-sided backface shadow missing"
    );
    scene.lighting.shadow_resolution = 1024;
    let resized = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&resized, -0.55)[0] < 30 && read(&resized, -0.25)[0] > 90,
        "resized shadow map lost coverage"
    );
    scene.lighting.shadow_resolution = 999;
    ensure!(
        capture(gpu, &mut renderer, &scene, [128, 128]).is_err(),
        "invalid shadow resolution accepted"
    );
    scene.lighting.shadow_resolution = 512;
    scene.items[0].material.lit = false;
    let unlit = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&unlit, -0.55) == [255; 3],
        "unlit surface received a shadow"
    );
    scene.items[0].material.lit = true;
    renderer.upload_model(
        gpu,
        "cutout",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            start: 0,
            count: 6,
            color: [1., 1., 1., 0.5],
            alpha_cutoff: None,
            image: None,
            shading: Some(cutout),
        }],
    )?;
    let blended = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&blended, -0.55)[0] > 90,
        "blended surface cast an opaque shadow"
    );
    println!(
        "shadow_gpu_ok direction toggle ambient_emissive pcf_edges alpha_cutout mirrored_caster sidedness resize transparent_policy unlit"
    );
    Ok(())
}
