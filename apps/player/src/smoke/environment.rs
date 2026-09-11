use super::*;
use bozzard_render::{
    EnvironmentSettings, Lighting, MaterialMap, ModelImage, ModelPart, ModelShading,
};

fn surface(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    normal: [f32; 3],
    metallic: f32,
    roughness: f32,
    occluded: bool,
) -> Result<()> {
    let mut vertices = [
        [-0.8, -0.8, 0., 0., 0., 1., 0., 1.],
        [0.8, -0.8, 0., 0., 0., 1., 1., 1.],
        [0.8, 0.8, 0., 0., 0., 1., 1., 0.],
        [-0.8, 0.8, 0., 0., 0., 1., 0., 0.],
    ];
    for v in &mut vertices {
        v[3..6].copy_from_slice(&normal);
    }
    let attributes = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 4];
    let shading = ModelShading {
        vertex_start: 0,
        vertices: &attributes,
        metallic,
        roughness,
        normal_scale: 1.,
        occlusion_strength: 1.,
        emissive_factor: [0.; 3],
        double_sided: false,
        base_color_sampler: Default::default(),
        normal: None,
        metallic_roughness: None,
        emissive: None,
        occlusion: occluded.then_some(MaterialMap {
            image: ModelImage {
                width: 1,
                height: 1,
                rgba: &[0, 0, 0, 255],
            },
            sampler: Default::default(),
        }),
    };
    renderer.upload_model(
        gpu,
        "environment-fixture",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            source_key: "",
            start: 0,
            count: 6,
            color: [1.; 4],
            alpha_cutoff: None,
            image: None,
            shading: Some(shading),
        }],
    )
}
fn center(frame: &Frame) -> [u8; 3] {
    frame.rgba[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 3]
        .try_into()
        .unwrap()
}
pub(super) fn checks(gpu: &Gpu) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = RenderScene {
        lights: Vec::new(),
        display: Default::default(),
        lighting: Lighting {
            sun_intensity: 0.,
            ambient_intensity: 0.,
            shadows: false,
            ..Default::default()
        },
        environment: EnvironmentSettings {
            zenith: [0.25; 3],
            horizon: [0.25; 3],
            ground: [0.25; 3],
            intensity: 1.,
            background: false,
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.)),
        items: vec![DrawItem {
            model: Mat4::IDENTITY,
            mesh: MeshKind::Imported("environment-fixture".into()),
            material: Material {
                surface_overrides: Default::default(),
                tint: [1.; 3],
                uv_scale: [1.; 2],
                texture: TextureKind::White,
                lit: true,
            },
        }],
    };
    surface(gpu, &mut renderer, [0., 0., 1.], 0., 0.5, false)?;
    let constant = center(&capture(gpu, &mut renderer, &scene, [64, 64])?);
    scene.items[0].mesh = MeshKind::Quad;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [64; 3],
    )?;
    scene.items[0].mesh = MeshKind::Imported("environment-fixture".into());
    ensure!(
        constant.iter().all(|c| (59..=67).contains(c)),
        "constant environment lost energy: {constant:?}"
    );
    scene.environment.intensity = 0.5;
    let half = center(&capture(gpu, &mut renderer, &scene, [64, 64])?);
    ensure!(
        (i32::from(half[0]) * 2 - i32::from(constant[0])).abs() <= 2,
        "environment intensity is not linear"
    );
    scene.environment.intensity = 0.;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [0; 3],
    )?;
    scene.environment.intensity = 1.;
    surface(gpu, &mut renderer, [0., 0., 1.], 0., 0.5, true)?;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [0; 3],
    )?;
    scene.environment.zenith = [1., 0., 0.];
    scene.environment.horizon = [0.; 3];
    scene.environment.ground = [0., 0., 1.];
    surface(gpu, &mut renderer, [0., 1., 0.], 0., 1., false)?;
    let up = center(&capture(gpu, &mut renderer, &scene, [64, 64])?);
    surface(gpu, &mut renderer, [0., -1., 0.], 0., 1., false)?;
    let down = center(&capture(gpu, &mut renderer, &scene, [64, 64])?);
    ensure!(
        up[0] > up[2] + 80 && down[2] > down[0] + 80,
        "diffuse cube orientation wrong: up={up:?} down={down:?}"
    );
    surface(gpu, &mut renderer, [0., 0., 1.], 1., 0.045, false)?;
    let smooth = center(&capture(gpu, &mut renderer, &scene, [64, 64])?);
    surface(gpu, &mut renderer, [0., 0., 1.], 1., 1., false)?;
    let rough = center(&capture(gpu, &mut renderer, &scene, [64, 64])?);
    ensure!(
        smooth.iter().any(|c| *c > 0)
            && rough[2] > smooth[2] + 8
            && rough[0].abs_diff(rough[2]) < smooth[0].abs_diff(smooth[2]),
        "GGX reflection roughness missing: smooth={smooth:?} rough={rough:?}"
    );
    scene.items[0].material.lit = false;
    pixel(
        &capture(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [255; 3],
    )?;
    scene.items.clear();
    scene.environment.background = true;
    // Looking upward from a view camera must show zenith, downward ground.
    scene.view_projection = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 10.)
        * glam::camera::rh::view::look_to_mat4(Vec3::ZERO, Vec3::Y, Vec3::Z);
    let sky = center(&capture(gpu, &mut renderer, &scene, [64, 64])?);
    ensure!(
        sky[0] > 240 && sky[2] < 5,
        "sky camera orientation wrong: {sky:?}"
    );
    scene.environment.intensity = f32::NAN;
    ensure!(
        capture(gpu, &mut renderer, &scene, [64, 64]).is_err(),
        "invalid environment accepted"
    );
    println!(
        "environment_gpu_ok constant_energy intensity disable diffuse_orientation specular_roughness occlusion unlit sky_camera validation"
    );
    Ok(())
}
