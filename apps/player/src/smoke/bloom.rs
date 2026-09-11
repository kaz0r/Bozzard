use super::*;
use bozzard_render::{BloomSettings, DisplaySettings, EnvironmentSettings, Lighting};
use std::path::Path;

pub(super) fn checks(gpu: &Gpu, output: &Path) -> Result<()> {
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = RenderScene {
        lights: Vec::new(),
        environment: EnvironmentSettings::disabled(),
        lighting: Lighting {
            shadows: false,
            sun_direction: [0., 0., 1.],
            sun_intensity: 4. * std::f32::consts::PI,
            ambient_intensity: 0.,
            ..Default::default()
        },
        display: DisplaySettings {
            bloom: BloomSettings {
                enabled: false,
                intensity: 1.,
                threshold: 1.,
                scatter: 0.7,
            },
            exposure_ev: 0.,
            tone_mapping: true,
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-1., 1., -1., 1., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.)),
        items: vec![DrawItem {
            model: Mat4::from_scale(Vec3::splat(0.25)),
            mesh: MeshKind::Quad,
            material: Material {
                surface_overrides: Default::default(),
                tint: [1.; 3],
                uv_scale: [1.; 2],
                texture: TextureKind::White,
                lit: true,
            },
        }],
    };
    let off = capture_display(gpu, &mut renderer, &scene, [64, 64])?;
    let raw = capture(gpu, &mut renderer, &scene, [64, 64])?;
    scene.display.bloom.enabled = true;
    let on = capture_display(gpu, &mut renderer, &scene, [64, 64])?;
    let at = |frame: &Frame, x: usize, y: usize| frame.rgba[(y * 64 + x) * 4];
    ensure!(
        at(&on, 40, 32) > at(&off, 40, 32) + 2,
        "bloom halo did not extend beyond bright geometry"
    );
    ensure!(
        at(&on, 32, 32) > at(&off, 32, 32),
        "bloom did not compose bright center"
    );
    ensure!(
        off.rgba
            .chunks_exact(4)
            .zip(on.rgba.chunks_exact(4))
            .all(|(a, b)| a[3] == b[3]),
        "bloom changed output alpha"
    );
    off.write_ppm(&output.join("bloom-off.ppm"))?;
    on.write_ppm(&output.join("bloom-on.ppm"))?;
    ensure!(
        raw.rgba == capture(gpu, &mut renderer, &scene, [64, 64])?.rgba,
        "raw linear fixture gained bloom"
    );
    scene.display.bloom.intensity = 0.;
    ensure!(
        off.rgba == capture_display(gpu, &mut renderer, &scene, [64, 64])?.rgba,
        "zero intensity changed pixels"
    );
    scene.display.bloom.intensity = 1.;
    scene.display.bloom.enabled = false;
    ensure!(
        off.rgba == capture_display(gpu, &mut renderer, &scene, [64, 64])?.rgba,
        "disable retained bloom"
    );
    scene.display.bloom.enabled = true;
    scene.display.bloom.threshold = 10.;
    ensure!(
        off.rgba == capture_display(gpu, &mut renderer, &scene, [64, 64])?.rgba,
        "below-threshold scene blooms"
    );
    scene.display.bloom.threshold = 1.;
    scene.display.bloom.scatter = 0.;
    let narrow = capture_display(gpu, &mut renderer, &scene, [64, 64])?;
    ensure!(
        at(&on, 45, 32) > at(&narrow, 45, 32),
        "spread did not widen halo"
    );
    // Constant field: bloom=(radiance4-threshold1)=3 regardless of pyramid depth.
    // Total7 exposed by-3EV is0.875; Reinhard then a single sRGB encoding.
    scene.items[0].model = Mat4::from_scale(Vec3::splat(5.));
    scene.display.bloom.scatter = 0.7;
    scene.display.exposure_ev = -3.;
    let linear = 0.875_f64 / (1. + 0.875);
    let expected = ((1.055 * linear.powf(1. / 2.4) - 0.055) * 255.).round() as u8;
    for size in [[64, 64], [97, 53], [1, 1], [1, 17], [3, 5]] {
        let frame = capture_display(gpu, &mut renderer, &scene, size)?;
        pixel(&frame, size[0] / 2, size[1] / 2, [expected; 3])?;
    }
    // Hardware sRGB output must agree with shader encoding when bloom is enabled.
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bloom sRGB parity"),
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
    let mut srgb = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
    srgb.draw(
        gpu,
        &texture.create_view(&Default::default()),
        [64, 64],
        &scene,
    )?;
    pixel(
        &bozzard_render::read_texture(gpu, &texture, 64, 64)?,
        32,
        32,
        [expected; 3],
    )?;
    for invalid in [
        BloomSettings {
            intensity: f32::NAN,
            ..Default::default()
        },
        BloomSettings {
            threshold: -1.,
            ..Default::default()
        },
        BloomSettings {
            scatter: 1.1,
            ..Default::default()
        },
    ] {
        scene.display.bloom = invalid;
        ensure!(
            capture_display(gpu, &mut renderer, &scene, [64, 64]).is_err(),
            "invalid bloom accepted"
        );
    }
    println!(
        "bloom_gpu_ok halo threshold intensity spread disable raw alpha hdr_before_display constant_energy odd_tiny_resize srgb_parity validation"
    );
    Ok(())
}
