use super::*;
use bozzard_render::{DisplaySettings, Lighting, ModelPart};

pub(super) fn checks(gpu: &Gpu) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = RenderScene {
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: DisplaySettings {
            exposure_ev: -2.,
            tone_mapping: true,
        },
        lighting: Lighting {
            shadows: false,
            sun_direction: [0., 0., 1.],
            sun_intensity: 4. * std::f32::consts::PI,
            ambient_intensity: 0.,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.)),
        items: vec![DrawItem {
            model: Mat4::IDENTITY,
            mesh: MeshKind::Quad,
            material: Material {
                surface_overrides: Default::default(),
                tint: [1.; 3],
                texture: TextureKind::White,
                uv_scale: [1.; 2],
                lit: true,
            },
        }],
    };
    // Radiance4 / exposure4 =1, Reinhard(1)=0.5, sRGB(0.5)=188.
    // An LDR intermediate would clip first, giving a substantially darker result.
    let exposed = capture_display(gpu, &mut renderer, &scene, [64, 64])?;
    pixel(&exposed, 32, 32, [188; 3])?;
    scene.display.exposure_ev = 0.;
    pixel(
        &capture_display(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [231; 3],
    )?;
    scene.display.exposure_ev = -4.;
    scene.display.tone_mapping = false;
    pixel(
        &capture_display(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [137; 3],
    )?;
    scene.display.exposure_ev = -2.;
    scene.display.tone_mapping = true;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sRGB display parity fixture"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut srgb_renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
    srgb_renderer.draw(
        gpu,
        &texture.create_view(&Default::default()),
        [64, 64],
        &scene,
    )?;
    let hardware = bozzard_render::read_texture(gpu, &texture, 64, 64)?;
    ensure!(
        exposed
            .rgba
            .iter()
            .zip(&hardware.rgba)
            .all(|(a, b)| a.abs_diff(*b) <= 1),
        "sRGB output was encoded twice or omitted"
    );
    pixel(
        &capture_display(gpu, &mut renderer, &scene, [97, 53])?,
        48,
        26,
        [188; 3],
    )?;
    let vertices = [
        [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
        [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
        [0.5, 0.5, 0., 0., 0., 1., 1., 0.],
        [-0.5, 0.5, 0., 0., 0., 1., 0., 0.],
    ];
    renderer.upload_model(
        gpu,
        "hdr-glass",
        &vertices,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            source_key: "",
            start: 0,
            count: 6,
            color: [0., 1., 0., 0.5],
            alpha_cutoff: None,
            image: None,
            shading: None,
        }],
    )?;
    scene.display.exposure_ev = 0.;
    scene.items[0].material.tint = [1., 0., 0.];
    let mut glass = scene.items[0].clone();
    glass.material.tint = [1.; 3];
    glass.mesh = MeshKind::Imported("hdr-glass".into());
    glass.model = Mat4::from_translation(Vec3::new(0., 0., 0.5));
    scene.items.push(glass);
    pixel(
        &capture_display(gpu, &mut renderer, &scene, [64, 64])?,
        32,
        32,
        [213, 213, 0],
    )?;
    scene.display.exposure_ev = f32::NAN;
    ensure!(
        capture_display(gpu, &mut renderer, &scene, [64, 64]).is_err(),
        "invalid exposure accepted"
    );
    println!(
        "display_gpu_ok hdr_exposure reinhard srgb_once hardware_encoding_parity linear_transparency resize validation"
    );
    Ok(())
}
