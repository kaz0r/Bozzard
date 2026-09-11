use super::*;
use bozzard_render::{MaterialMap, ModelImage, ModelPart, ModelShading};

fn map(rgba: &[u8]) -> MaterialMap<'_> {
    MaterialMap {
        image: ModelImage {
            width: 1,
            height: 1,
            rgba,
        },
        sampler: Default::default(),
    }
}
struct Fixture<'a> {
    shading: ModelShading<'a>,
    base: Option<ModelImage<'a>>,
    color: [f32; 4],
    lit: bool,
    pbr: bool,
    reversed: bool,
    model: Mat4,
    lights: Vec<bozzard_render::LocalLight>,
    lighting: bozzard_render::Lighting,
}
impl Fixture<'_> {
    fn draw(&self, gpu: &Gpu, renderer: &mut SceneRenderer) -> Result<Frame> {
        let vertices = [
            [-0.8, -0.8, 0., 0., 0., 1., 2.25, 0.5],
            [0.8, -0.8, 0., 0., 0., 1., 2.25, 0.5],
            [0.8, 0.8, 0., 0., 0., 1., 2.25, 0.5],
            [-0.8, 0.8, 0., 0., 0., 1., 2.25, 0.5],
        ];
        let indices = if self.reversed {
            [0, 2, 1, 0, 3, 2]
        } else {
            [0, 1, 2, 0, 2, 3]
        };
        renderer.upload_model(
            gpu,
            "pbr-fixture",
            &vertices,
            &indices,
            &[ModelPart {
                source_key: "",
                start: 0,
                count: 6,
                color: self.color,
                alpha_cutoff: None,
                image: self.base.clone(),
                shading: self.pbr.then(|| self.shading.clone()),
            }],
        )?;
        let scene = RenderScene {
            lights: self.lights.clone(),
            environment: bozzard_render::EnvironmentSettings::disabled(),
            display: Default::default(),
            lighting: self.lighting,
            view_projection: glam::camera::rh::proj::directx::orthographic(
                -1., 1., -1., 1., 0.1, 10.,
            ) * Mat4::from_translation(Vec3::new(0., 0., -3.)),
            items: vec![DrawItem {
                model: self.model,
                mesh: MeshKind::Imported("pbr-fixture".into()),
                material: Material {
                    surface_overrides: Default::default(),
                    tint: [1.; 3],
                    uv_scale: [1.; 2],
                    texture: TextureKind::White,
                    lit: self.lit,
                },
            }],
        };
        capture(gpu, renderer, &scene, [64, 64])
    }
}
fn center(frame: &Frame) -> [u8; 3] {
    frame.rgba[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 3]
        .try_into()
        .unwrap()
}

pub(super) fn checks(gpu: &Gpu) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let attributes = [[1., 0., 0., 1., 0.75, 0.5, 0.75, 0.5, 0.75, 0.5, 0.75, 0.5]; 4];
    let shading = ModelShading {
        vertex_start: 0,
        vertices: &attributes,
        metallic: 0.,
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
    };
    let mut f = Fixture {
        shading: shading.clone(),
        base: None,
        color: [1.; 4],
        lit: true,
        pbr: true,
        reversed: false,
        model: Mat4::IDENTITY,
        lights: Vec::new(),
        lighting: Default::default(),
    };
    local_light_checks(gpu, &mut renderer, &mut f)?;
    for pbr in [true, false] {
        f.pbr = pbr;
        f.lighting.ambient_intensity = 0.;
        f.lighting.sun_direction = [0., 0., 1.];
        f.lighting.sun_intensity = 1.;
        f.lighting.sun_color = [1., 0., 0.];
        let red = center(&f.draw(gpu, &mut renderer)?);
        ensure!(
            red[0] > 60 && red[1] == 0 && red[2] == 0,
            "authored sun color missing: {red:?}"
        );
        f.lighting.sun_intensity = 0.5;
        let half = center(&f.draw(gpu, &mut renderer)?);
        ensure!(
            (i32::from(half[0]) * 2 - i32::from(red[0])).abs() <= 2,
            "sun intensity is not linear"
        );
        f.lighting.sun_direction = [0., 0., -1.];
        pixel(&f.draw(gpu, &mut renderer)?, 32, 32, [0, 0, 0])?;
        f.lighting.sun_intensity = 0.;
        f.lighting.ambient_color = [0., 1., 0.];
        f.lighting.ambient_intensity = 0.25;
        pixel(&f.draw(gpu, &mut renderer)?, 32, 32, [0, 64, 0])?;
        f.lit = false;
        pixel(&f.draw(gpu, &mut renderer)?, 32, 32, [255; 3])?;
        f.lit = true;
        f.lighting.sun_direction = [0.; 3];
        ensure!(
            f.draw(gpu, &mut renderer).is_err(),
            "renderer accepted invalid sun direction"
        );
        f.lighting = Default::default();
    }
    f.pbr = true;
    println!(
        "lighting_gpu_ok pbr_and_diffuse sun_direction color linear_intensity ambient unlit invalid_input"
    );
    let baseline = center(&f.draw(gpu, &mut renderer)?);
    ensure!(
        baseline[0] > 60,
        "PBR direct lighting missing: {baseline:?}"
    );

    f.shading.occlusion = Some(map(&[0, 255, 255, 255]));
    let occluded = center(&f.draw(gpu, &mut renderer)?);
    ensure!(
        (7..=9).contains(&baseline[0].saturating_sub(occluded[0])),
        "occlusion must affect only indirect light: {baseline:?} {occluded:?}"
    );
    f.shading.occlusion = Some(map(&[128, 255, 255, 255]));
    let half_ao = center(&f.draw(gpu, &mut renderer)?);
    ensure!(
        (3..=5).contains(&baseline[0].saturating_sub(half_ao[0])),
        "occlusion data must remain linear: {half_ao:?}"
    );
    f.shading = shading.clone();
    f.shading.normal = Some(map(&[128, 128, 0, 255]));
    let turned = center(&f.draw(gpu, &mut renderer)?);
    ensure!(
        turned[0] < baseline[0] / 2,
        "normal map is not changing lighting: {turned:?}"
    );
    f.shading.normal = Some(map(&[255, 128, 255, 255]));
    f.shading.normal_scale = 0.;
    pixel(&f.draw(gpu, &mut renderer)?, 32, 32, baseline)?;

    f.shading = shading.clone();
    f.shading.metallic = 1.;
    f.shading.metallic_roughness = Some(map(&[0, 255, 0, 255]));
    pixel(&f.draw(gpu, &mut renderer)?, 32, 32, baseline)?;
    f.shading.metallic_roughness = Some(map(&[0, 255, 255, 255]));
    let metal = center(&f.draw(gpu, &mut renderer)?);
    ensure!(
        metal[0].abs_diff(baseline[0]) > 20,
        "metallic blue channel ignored: {metal:?}"
    );
    f.shading.metallic_roughness = Some(map(&[0, 64, 255, 255]));
    let smooth = center(&f.draw(gpu, &mut renderer)?);
    ensure!(
        smooth[0].abs_diff(metal[0]) > 10,
        "roughness green channel ignored: {smooth:?} {metal:?}"
    );

    f.shading = shading.clone();
    f.color = [0., 0., 0., 1.];
    f.shading.emissive_factor = [0.25, 0.5, 1.];
    f.shading.emissive = Some(map(&[128, 64, 32, 255]));
    pixel(&f.draw(gpu, &mut renderer)?, 32, 32, [14, 7, 4])?;

    f.shading = shading.clone();
    f.color = [1.; 4];
    f.reversed = true;
    pixel(&f.draw(gpu, &mut renderer)?, 32, 32, [5, 6, 10])?;
    f.shading.double_sided = true;
    ensure!(
        center(&f.draw(gpu, &mut renderer)?) != [5, 6, 10],
        "double sided surface culled"
    );
    f.reversed = false;
    f.shading = shading.clone();
    f.model = Mat4::from_scale(Vec3::new(-1., 1., 1.));
    pixel(&f.draw(gpu, &mut renderer)?, 32, 32, baseline)?;
    f.model = Mat4::IDENTITY;

    // One source can require two GPU color spaces; same-space maps must share.
    let shared = [128, 128, 255, 255];
    f.base = Some(ModelImage {
        width: 1,
        height: 1,
        rgba: &shared,
    });
    f.shading.normal = Some(map(&shared));
    f.shading.metallic_roughness = Some(map(&shared));
    f.shading.occlusion = Some(map(&shared));
    f.shading.emissive = Some(map(&shared));
    f.draw(gpu, &mut renderer)?;
    let stats = renderer.model_upload_stats("pbr-fixture").unwrap();
    ensure!(
        stats.unique_images == 2 && stats.texture_bytes == 8,
        "PBR color-space sharing incorrect: {stats:?}"
    );

    // Constant UV 2.25 selects red with repeat, green with clamp.
    let colors = [255, 0, 0, 255, 0, 255, 0, 255];
    f.shading = shading;
    f.lit = false;
    f.base = Some(ModelImage {
        width: 2,
        height: 1,
        rgba: &colors,
    });
    f.shading.base_color_sampler.address_mode_u = wgpu::AddressMode::Repeat;
    pixel(&f.draw(gpu, &mut renderer)?, 32, 32, [255, 0, 0])?;
    f.shading.base_color_sampler.address_mode_u = wgpu::AddressMode::ClampToEdge;
    pixel(&f.draw(gpu, &mut renderer)?, 32, 32, [0, 255, 0])?;
    f.lit = true;
    f.color = [0., 0., 0., 1.];
    f.shading.emissive_factor = [1.; 3];
    f.shading.emissive = Some(MaterialMap {
        image: ModelImage {
            width: 2,
            height: 1,
            rgba: &colors,
        },
        sampler: Default::default(),
    });
    pixel(&f.draw(gpu, &mut renderer)?, 32, 32, [0, 255, 0])?;
    println!(
        "pbr_gpu_ok normal_scale metallic_roughness linear_ao srgb_emissive double_sided mirrored_tangents sampler_uv color_space_sharing"
    );
    Ok(())
}

fn local_light_checks(gpu: &Gpu, renderer: &mut SceneRenderer, f: &mut Fixture<'_>) -> Result<()> {
    use bozzard_render::LocalLight;
    let point = LocalLight {
        position: [0., 0., 1.],
        direction: [0., 0., -1.],
        color: [1., 0., 0.],
        intensity: 1.,
        range: 100.,
        spot_angles: None,
    };
    f.lighting.sun_intensity = 0.;
    f.lighting.ambient_intensity = 0.;
    f.lighting.shadows = false;
    for pbr in [true, false] {
        f.pbr = pbr;
        f.lights = vec![point];
        let red = center(&f.draw(gpu, renderer)?);
        ensure!(
            red[0] > 65 && red[1] == 0 && red[2] == 0,
            "local light color missing: {red:?}"
        );
        f.lights[0].position[2] = 2.;
        let far = center(&f.draw(gpu, renderer)?);
        ensure!(
            (i32::from(far[0]) * 4 - i32::from(red[0])).abs() <= 4,
            "inverse-square falloff failed: {red:?}/{far:?}"
        );
        f.lights[0] = point;
        f.lights[0].intensity = 0.5;
        let half = center(&f.draw(gpu, renderer)?);
        ensure!(
            (i32::from(half[0]) * 2 - i32::from(red[0])).abs() <= 2,
            "local intensity nonlinear"
        );
        f.lights[0] = point;
        f.lights[0].range = 1.;
        pixel(&f.draw(gpu, renderer)?, 32, 32, [0, 0, 0])?;
        f.lights[0].range = 2.;
        let cutoff = center(&f.draw(gpu, renderer)?);
        ensure!(
            cutoff[0] < red[0] && cutoff[0] > red[0] / 2,
            "range edge not smoothly faded"
        );
        f.lights[0] = LocalLight {
            spot_angles: Some([10., 20.]),
            ..point
        };
        let spot = f.draw(gpu, renderer)?;
        pixel(&spot, 32, 32, red)?;
        pixel(&spot, 54, 32, [0, 0, 0])?;
        // At the cone edge the same surface receives less light than a point light.
        let penumbra = center_at(&spot, 40, 32);
        ensure!(
            penumbra[0] > 0 && penumbra[0] < red[0],
            "spot penumbra missing: {penumbra:?}"
        );
        f.lights[0].direction = [0., 0., 1.];
        pixel(&f.draw(gpu, renderer)?, 32, 32, [0, 0, 0])?;
        f.lights[0] = LocalLight {
            spot_angles: Some([20., 20.]),
            ..point
        };
        pixel(&f.draw(gpu, renderer)?, 32, 32, red)?;
        f.lights = vec![
            point,
            LocalLight {
                color: [0., 1., 0.],
                ..point
            },
        ];
        pixel(&f.draw(gpu, renderer)?, 32, 32, [red[0], red[0], 0])?;
        f.lights.clear();
        pixel(&f.draw(gpu, renderer)?, 32, 32, [0, 0, 0])?;
        f.lights = vec![point; bozzard_render::MAX_LOCAL_LIGHTS];
        // Isolate the last slot so an off-by-one count/upload bug cannot pass.
        for light in &mut f.lights[..bozzard_render::MAX_LOCAL_LIGHTS - 1] {
            light.intensity = 0.;
        }
        pixel(&f.draw(gpu, renderer)?, 32, 32, red)?;
        f.lights.push(point);
        ensure!(
            f.draw(gpu, renderer).is_err(),
            "over-limit lights silently accepted"
        );
        f.lights = vec![LocalLight {
            range: f32::NAN,
            ..point
        }];
        ensure!(f.draw(gpu, renderer).is_err(), "invalid light reached GPU");
        f.lights = vec![point];
        f.lit = false;
        pixel(&f.draw(gpu, renderer)?, 32, 32, [255; 3])?;
        f.lit = true;
    }
    f.lights.clear();
    f.pbr = true;
    f.lighting = Default::default();
    println!(
        "local_lights_gpu_ok pbr diffuse colors inverse_square range cone penumbra rotation equal_angles multiple removal limit validation unlit"
    );
    Ok(())
}
fn center_at(frame: &Frame, x: usize, y: usize) -> [u8; 3] {
    frame.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 3]
        .try_into()
        .unwrap()
}
