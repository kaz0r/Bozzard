use super::*;
use bozzard_render::{
    Lighting, LocalLight, ModelImage, ModelPart, ModelShading, SpotShadowSettings,
};
use std::path::Path;

const VERTICES: [[f32; 8]; 4] = [
    [-0.5, -0.5, 0., 0., 0., 1., 0., 1.],
    [0.5, -0.5, 0., 0., 0., 1., 1., 1.],
    [0.5, 0.5, 0., 0., 0., 1., 1., 0.],
    [-0.5, 0.5, 0., 0., 0., 1., 0., 0.],
];
const ATTRIBUTES: [[f32; 12]; 4] = [[1., 0., 0., 1., 0., 0., 0., 0., 0., 0., 0., 0.]; 4];
fn shading() -> ModelShading<'static> {
    ModelShading {
        vertex_start: 0,
        vertices: &ATTRIBUTES,
        metallic: 0.,
        roughness: 1.,
        normal_scale: 1.,
        occlusion_strength: 1.,
        emissive_factor: [0.; 3],
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
    }
}
fn upload(
    gpu: &Gpu,
    renderer: &mut SceneRenderer,
    name: &str,
    cutoff: Option<f32>,
    double_sided: bool,
    alpha: f32,
) -> Result<()> {
    let mut shading = shading();
    shading.double_sided = double_sided;
    if name == "receiver" {
        shading.emissive_factor = [0., 0.2, 0.];
    }
    renderer.upload_model(
        gpu,
        name,
        &VERTICES,
        &[0, 1, 2, 0, 2, 3],
        &[ModelPart {
            source_key: "",
            start: 0,
            count: 6,
            color: [1., 1., 1., alpha],
            alpha_cutoff: cutoff,
            image: cutoff.map(|_| ModelImage {
                width: 2,
                height: 1,
                rgba: &[255, 255, 255, 0, 255, 255, 255, 255],
            }),
            shading: Some(shading),
        }],
    )
}
fn read(frame: &Frame, x: f32) -> [u8; 3] {
    let px = ((x + 2.) * frame.width as f32 / 4.).floor() as usize;
    let index = ((frame.height / 2) as usize * frame.width as usize + px) * 4;
    frame.rgba[index..index + 3].try_into().unwrap()
}
pub(super) fn checks(gpu: &Gpu, output: &Path) -> Result<()> {
    checks_for_kind(gpu, output, false).context("spot shadow regression")?;
    checks_for_kind(gpu, output, true).context("point shadow regression")
}
fn checks_for_kind(gpu: &Gpu, output: &Path, point: bool) -> Result<()> {
    let kind = if point { "point" } else { "spot" };
    let mut renderer = SceneRenderer::new(gpu, wgpu::TextureFormat::Rgba8Unorm);
    upload(gpu, &mut renderer, "receiver", None, false, 1.)?;
    let light = LocalLight {
        directional: false,
        position: [3., 0., 3.],
        direction: [-1., 0., -1.],
        color: [1.; 3],
        intensity: 40.,
        range: 20.,
        spot_angles: (!point).then_some([25., 35.]),
        shadows: Some(SpotShadowSettings::default()),
    };
    let receiver = DrawItem {
        motion_id: 0,
        model: Mat4::from_scale(Vec3::new(4., 4., 1.)),
        mesh: MeshKind::Imported("receiver".into()),
        material: Material {
            metallic: None,
            roughness: None,
            surface_overrides: Default::default(),
            tint: [1.; 3],
            uv_scale: [1.; 2],
            texture: TextureKind::White,
            lit: true,
        },
    };
    let caster = DrawItem {
        motion_id: 0,
        model: Mat4::from_translation(Vec3::new(0.6, 0., 1.))
            * Mat4::from_scale(Vec3::new(0.6, 0.6, 1.)),
        mesh: MeshKind::Quad,
        material: receiver.material.clone(),
    };
    let mut scene = RenderScene {
        particles: Vec::new(),
        fog: Default::default(),
        gi: None,
        lights: vec![light],
        environment: bozzard_render::EnvironmentSettings::disabled(),
        display: Default::default(),
        lighting: Lighting {
            shadows: false,
            sun_intensity: 0.,
            ambient_intensity: 0.1,
            ..Default::default()
        },
        view_projection: glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.)),
        items: vec![receiver, caster],
    };
    let shadowed = capture(gpu, &mut renderer, &scene, [128, 128])?;
    let shadow = read(&shadowed, -0.6);
    ensure!(
        (24..=27).contains(&shadow[0]) && (75..=79).contains(&shadow[1]),
        "spot shadow lost ambient/emissive or coverage: {shadow:?}"
    );
    shadowed.write_ppm(&output.join(format!("{kind}-shadow-on.ppm")))?;
    let resized = capture(gpu, &mut renderer, &scene, [97, 73])?;
    ensure!(
        read(&resized, -0.6) == shadow,
        "viewport resize moved the spotlight shadow"
    );
    // GI replacement and sun target allocation rebuild the shared lighting binding.
    scene.gi = Some(super::gi::volume([0., 0.2, 0.], [2; 3]));
    let with_gi = read(&capture(gpu, &mut renderer, &scene, [128, 128])?, -0.6);
    ensure!(
        with_gi[0] == shadow[0] && with_gi[1] > shadow[1] + 10,
        "spot shadow blocked GI or GI rebind lost spot depth: {with_gi:?}"
    );
    scene.gi = None;
    scene.lighting.sun_intensity = 1.;
    scene.lighting.sun_color = [0., 0., 1.];
    scene.lighting.shadows = true;
    scene.lighting.shadow_resolution = 512;
    let with_sun = read(&capture(gpu, &mut renderer, &scene, [128, 128])?, -0.6);
    ensure!(
        with_sun[0] == shadow[0] && with_sun[2] > shadow[2] + 10,
        "spot shadow blocked sun or sun rebind lost spot depth: {with_sun:?}"
    );
    scene.lighting.sun_intensity = 0.;
    scene.lighting.shadows = false;
    scene.lights[0].shadows = None;
    let clear = capture(gpu, &mut renderer, &scene, [128, 128])?;
    clear.write_ppm(&output.join(format!("{kind}-shadow-off.ppm")))?;
    ensure!(
        read(&clear, -0.6)[0] > shadow[0] + 40,
        "spot shadow toggle did not restore direct light"
    );
    ensure!(
        renderer.frame_stats().shadow_draws == 0,
        "disabled spot still draws shadows"
    );
    scene.lights[0] = light;
    ensure!(
        capture(gpu, &mut renderer, &scene, [128, 128])?.rgba == shadowed.rgba,
        "reenabling spot lost shadow map"
    );
    scene.items[1].material.lit = false;
    let unlit = read(&capture(gpu, &mut renderer, &scene, [128, 128])?, -0.6);
    ensure!(
        unlit == read(&clear, -0.6),
        "{kind} unlit caster blocked direct light: actual={unlit:?} clear={:?}",
        read(&clear, -0.6)
    );
    scene.items[1].material.lit = true;
    ensure!(
        shadowed
            .rgba
            .chunks_exact(4)
            .zip(clear.rgba.chunks_exact(4))
            .any(|(a, b)| a[0] > shadow[0] + 5 && i16::from(a[0]) + 5 < i16::from(b[0])),
        "spot PCF has no intermediate edge coverage"
    );

    // The same direct visibility is used by built-in Lambert geometry.
    scene.items[0].mesh = MeshKind::Quad;
    let lambert = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(read(&lambert, -0.6)[0] < 30, "Lambert spot shadow missing");
    scene.items[0].material.lit = false;
    ensure!(
        read(&capture(gpu, &mut renderer, &scene, [128, 128])?, -0.6) == [255; 3],
        "unlit receiver was shadowed"
    );
    scene.items[0].material.lit = true;
    scene.items[0].mesh = MeshKind::Imported("receiver".into());

    // Render only the shadow region: the caster is outside the camera, inside the light.
    let camera = scene.view_projection;
    scene.view_projection =
        glam::camera::rh::proj::directx::orthographic(-1.2, -0.1, -1., 1., 0.1, 10.)
            * Mat4::from_translation(Vec3::new(0., 0., -3.));
    let offscreen = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        renderer.frame_stats().culled_surfaces == 1 && renderer.frame_stats().shadow_draws >= 2,
        "camera culling removed an offscreen spot caster"
    );
    pixel(&offscreen, 64, 64, shadow)?;
    let shadow_draws = renderer.frame_stats().shadow_draws;
    // A third caster outside the light cone must be culled by the light, not the camera.
    let mut outside = scene.items[1].clone();
    outside.model = Mat4::from_translation(Vec3::new(100., 0., 1.));
    scene.items.push(outside);
    let culled = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        renderer.frame_stats().shadow_draws == shadow_draws,
        "{kind} frustum did not cull distant geometry"
    );
    renderer.set_culling_enabled(false);
    ensure!(
        capture(gpu, &mut renderer, &scene, [128, 128])?.rgba == culled.rgba,
        "spot culling differs from reference pixels"
    );
    ensure!(
        renderer.frame_stats().shadow_draws == if point { 18 } else { 3 },
        "unculled shadow reference skipped a caster"
    );
    renderer.set_culling_enabled(true);
    scene.items.pop();
    scene.view_projection = camera;

    upload(gpu, &mut renderer, "cutout", Some(0.5), false, 1.)?;
    scene.items[1].mesh = MeshKind::Imported("cutout".into());
    let masked = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&masked, -0.85)[0] > 65 && read(&masked, -0.35)[0] < 30,
        "spot cutout alpha was ignored"
    );
    scene.items[1].model *= Mat4::from_scale(Vec3::new(-1., 1., 1.));
    let mirrored = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&mirrored, -0.85)[0] < 30 && read(&mirrored, -0.35)[0] > 65,
        "mirrored spot caster lost winding/UVs"
    );
    scene.items[1].model = Mat4::from_translation(Vec3::new(0.6, 0., 1.))
        * Mat4::from_scale(Vec3::new(0.6, 0.6, 1.))
        * Mat4::from_rotation_y(std::f32::consts::PI);
    let back = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&back, -0.85)[0] > 65 && read(&back, -0.35)[0] > 65,
        "single-sided backface cast a spot shadow"
    );
    upload(gpu, &mut renderer, "cutout", Some(0.5), true, 1.)?;
    let double = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&double, -0.85)[0] < 30 && read(&double, -0.35)[0] > 65,
        "double-sided spot caster missing"
    );
    upload(gpu, &mut renderer, "cutout", None, true, 0.5)?;
    ensure!(
        read(&capture(gpu, &mut renderer, &scene, [128, 128])?, -0.6)[0] > 65,
        "blended caster cast an opaque spot shadow"
    );

    scene.items[1].mesh = MeshKind::Quad;
    scene.items[1].model =
        Mat4::from_translation(Vec3::new(0., 0., 1.)) * Mat4::from_scale(Vec3::new(0.6, 0.6, 1.));
    scene.lighting.ambient_intensity = 0.;
    scene.lights = vec![
        LocalLight {
            color: [1., 0., 0.],
            ..light
        },
        LocalLight {
            position: [-3., 0., 3.],
            direction: [1., 0., -1.],
            color: [0., 0., 1.],
            ..light
        },
    ];
    let multiple = capture(gpu, &mut renderer, &scene, [128, 128])?;
    multiple.write_ppm(&output.join(format!("{kind}-shadows-multiple.ppm")))?;
    let left = read(&multiple, -1.5);
    let right = read(&multiple, 1.5);
    ensure!(
        left[0] < 3 && left[2] > 60 && right[0] > 60 && right[2] < 3,
        "per-light shadow layers leaked: {left:?} {right:?}"
    );
    scene.lights.swap(0, 1);
    ensure!(
        capture(gpu, &mut renderer, &scene, [128, 128])?.rgba == multiple.rgba,
        "reordering spots reused stale matrices/layers"
    );
    scene.lights[0].intensity = 0.;
    let inactive = capture(gpu, &mut renderer, &scene, [128, 128])?;
    scene.lights.remove(0);
    ensure!(
        capture(gpu, &mut renderer, &scene, [128, 128])?.rgba == inactive.rgba,
        "inactive spotlight changed shadow slot assignment"
    );
    scene.lights[0].color = [0.; 3];
    let black = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        renderer.frame_stats().shadow_draws == 0,
        "black light retained shadow passes"
    );
    scene.lights.clear();
    ensure!(
        capture(gpu, &mut renderer, &scene, [128, 128])?.rgba == black.rgba,
        "removing all lights left a stale shadow"
    );

    // Exercise the final slot and resource growth, with unshadowed lights interleaved.
    scene.lights = vec![
        LocalLight {
            intensity: 0.1,
            ..light
        };
        if point {
            bozzard_render::MAX_SHADOWED_POINT_LIGHTS
        } else {
            bozzard_render::MAX_SHADOWED_SPOT_LIGHTS
        }
    ];
    scene.lights.last_mut().unwrap().intensity = 40.;
    scene.lights.insert(
        3,
        LocalLight {
            spot_angles: None,
            shadows: None,
            intensity: 0.,
            ..light
        },
    );
    let full = capture(gpu, &mut renderer, &scene, [128, 128])?;
    ensure!(
        read(&full, -1.5)[0] < 3 && read(&full, 1.5)[0] > 60,
        "last local shadow slot is missing"
    );
    scene.lights.push(light);
    ensure!(
        capture(gpu, &mut renderer, &scene, [128, 128]).is_err(),
        "excess shadow maps accepted"
    );
    scene.lights.pop();
    ensure!(
        capture(gpu, &mut renderer, &scene, [128, 128])?.rgba == full.rgba,
        "invalid frame corrupted spotlight shadows"
    );
    scene.lights[0].shadows.as_mut().unwrap().bias = f32::NAN;
    ensure!(
        capture(gpu, &mut renderer, &scene, [128, 128]).is_err(),
        "invalid spot bias accepted"
    );
    scene.lights = vec![LocalLight {
        spot_angles: point.then_some([25., 35.]),
        ..light
    }];
    ensure!(
        read(&capture(gpu, &mut renderer, &scene, [128, 128])?, -1.5)[0] < 3,
        "switching local light kind lost its shadow"
    );
    println!(
        "{kind}_shadow_gpu_ok pbr lambert toggle resize pcf ambient_emissive gi_sun_rebinding unlit offscreen_casters light_frustum reference_parity cutout mirror sidedness blend multiple reorder inactive removal last_slot validation"
    );
    Ok(())
}
