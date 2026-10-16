use bozzard_render::*;
use glam::{Mat4, Vec3};

fn scene() -> RenderScene {
    RenderScene {
        skin_poses: Default::default(),
        shader_time: 0.,
        particles: vec![],
        trails: Vec::new(),
        decals: Vec::new(),
        view_projection: glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.1, 20.),
        items: vec![DrawItem {
            motion_id: 1,
            mesh: MeshKind::Quad,
            model: Mat4::from_translation(Vec3::new(0., 0., -5.))
                * Mat4::from_scale(Vec3::splat(8.)),
            material: Material {
                metallic: None,
                roughness: None,
                tint: [0.2; 3],
                lit: false,
                texture: TextureKind::White,
                uv_scale: [1.; 2],
                surface_overrides: Default::default(),
                shader: None,
            },
        }],
        lighting: Lighting {
            shadows: false,
            sun_intensity: 0.,
            ambient_intensity: 1.,
            ..Default::default()
        },
        lights: vec![],
        environment: EnvironmentSettings::disabled(),
        fog: Default::default(),
        gi: None,
        display: DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        },
    }
}
/// A red decal facing the camera (+Z) over the wall at z = -5, spanning x -0.5..1.5 and
/// y -1..1. `depth` is how far it projects.
fn decal(depth: f32) -> DecalItem {
    DecalItem {
        volume: Mat4::from_translation(Vec3::new(0.5, 0., -5.))
            * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
            * Mat4::from_scale(Vec3::new(2., depth, 2.)),
        texture: TextureKind::White,
        color: [1., 0., 0., 1.],
        min_facing: 0.5,
        lit: false,
    }
}
fn capture(gpu: &Gpu, r: &mut SceneRenderer, s: &RenderScene, raw: bool) -> anyhow::Result<Frame> {
    capture_offscreen(gpu, 160, 160, |target| {
        if raw {
            r.draw_linear(gpu, target, [160, 160], s)
        } else {
            r.draw(gpu, target, [160, 160], s)
        }
    })
}
/// The view spans -2..2 on both axes over 160 pixels.
fn at(frame: &Frame, x: f32, y: f32) -> [u8; 3] {
    let (px, py) = (((x + 2.) * 40.) as u32, ((2. - y) * 40.) as u32);
    let i = ((py * frame.width + px) * 4) as usize;
    [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
}

#[test]
fn decals_paint_facing_surfaces_inside_their_box() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    let wall = capture(&gpu, &mut renderer, &scene, false)?;
    let grey = at(&wall, 0.5, 0.);
    scene.decals = vec![decal(1.)];
    let painted = capture(&gpu, &mut renderer, &scene, false)?;
    assert_eq!(renderer.frame_stats().decals, 1);
    let inside = at(&painted, 0.5, 0.);
    assert!(
        inside[0] > 200 && inside[1] < 20 && inside[2] < 20,
        "red inside: {inside:?}"
    );
    assert_eq!(
        at(&painted, -1.5, 0.),
        at(&wall, -1.5, 0.),
        "left of the box"
    );
    assert_eq!(at(&painted, 0.5, 1.5), at(&wall, 0.5, 1.5), "above the box");

    // Projected down -Y instead, the box meets the wall edge-on and leaves it alone.
    scene.decals[0].volume =
        Mat4::from_translation(Vec3::new(0.5, 0., -5.)) * Mat4::from_scale(Vec3::new(2., 2., 2.));
    assert_eq!(capture(&gpu, &mut renderer, &scene, false)?.rgba, wall.rgba);

    // Half opacity mixes with the wall; a lit decal follows the scene's light.
    scene.decals = vec![DecalItem {
        color: [1., 0., 0., 0.5],
        ..decal(1.)
    }];
    let half = at(&capture(&gpu, &mut renderer, &scene, false)?, 0.5, 0.);
    assert!(
        half[0] > grey[0] && half[0] < inside[0] && half[1] < grey[1],
        "{half:?}"
    );
    scene.decals = vec![DecalItem {
        lit: true,
        ..decal(1.)
    }];
    let bright = at(&capture(&gpu, &mut renderer, &scene, false)?, 0.5, 0.);
    scene.lighting.ambient_intensity = 0.2;
    let dim = at(&capture(&gpu, &mut renderer, &scene, false)?, 0.5, 0.);
    assert!(
        dim[0] + 40 < bright[0],
        "lit decal darkens with the light: {dim:?} vs {bright:?}"
    );
    scene.lighting.ambient_intensity = 1.;

    // A box deep enough to hold the camera still paints the wall.
    scene.decals = vec![decal(20.)];
    assert_eq!(
        at(&capture(&gpu, &mut renderer, &scene, false)?, 0.5, 0.),
        inside
    );

    // An image that is not uploaded yet skips its decal; raw diagnostics skip all decals.
    scene.decals = vec![DecalItem {
        texture: TextureKind::Imported("not-uploaded".into()),
        ..decal(1.)
    }];
    assert_eq!(capture(&gpu, &mut renderer, &scene, false)?.rgba, wall.rgba);
    assert_eq!(renderer.frame_stats().decals, 0);
    scene.decals = vec![decal(1.)];
    let raw = capture(&gpu, &mut renderer, &scene, true)?;
    scene.decals.clear();
    assert_eq!(raw.rgba, capture(&gpu, &mut renderer, &scene, true)?.rgba);
    Ok(())
}
