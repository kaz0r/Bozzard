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
        items: vec![],
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
/// A ribbon across the view at depth `z`, red at its left end and blue at its right.
fn ribbon(z: f32, additive: bool) -> TrailRibbon {
    TrailRibbon {
        points: [-1.5f32, 0.0, 1.5]
            .into_iter()
            .map(|x| TrailPoint {
                position: Vec3::new(x, 0., z),
                width: 0.5,
                color: if x < 0. {
                    [1., 0., 0., 1.]
                } else {
                    [0., 0., 1., 1.]
                },
            })
            .collect(),
        additive,
    }
}
fn quad(z: f32) -> DrawItem {
    DrawItem {
        motion_id: 1,
        mesh: MeshKind::Quad,
        model: Mat4::from_translation(Vec3::new(0., 0., z)) * Mat4::from_scale(Vec3::splat(8.)),
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
fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * frame.width + x) * 4) as usize;
    [frame.rgba[i], frame.rgba[i + 1], frame.rgba[i + 2]]
}

#[test]
fn ribbons_draw_blend_and_hide_behind_geometry() -> anyhow::Result<()> {
    let gpu = pollster::block_on(Gpu::request(&instance(Backend::native()), None, false))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let mut scene = scene();
    scene.items = vec![quad(-6.)];
    let backdrop = capture(&gpu, &mut renderer, &scene, false)?;
    scene.trails = vec![ribbon(-5., false)];
    let drawn = capture(&gpu, &mut renderer, &scene, false)?;
    assert_eq!(renderer.frame_stats().trail_triangles, 4);
    // The view spans x and y in -2..2 over 160 pixels (40 per unit): the ribbon crosses the
    // middle row. Its points sit at x = -1.5 (red), 0 and 1.5 (blue).
    let left = pixel(&drawn, 22, 80);
    let right = pixel(&drawn, 130, 80);
    assert!(left[0] > 200 && left[2] < 60, "left end is red: {left:?}");
    assert!(
        right[2] > 200 && right[0] < 60,
        "right end is blue: {right:?}"
    );
    // Colour blends along the ribbon and the soft edges fade into the backdrop.
    let middle = pixel(&drawn, 50, 80);
    assert!(
        middle[0] > 60 && middle[2] > 60,
        "half way mixes both colours: {middle:?}"
    );
    assert_eq!(
        pixel(&drawn, 80, 40),
        pixel(&backdrop, 80, 40),
        "nothing off the ribbon"
    );
    // Three quarters of the way to the long edge, the ribbon is part transparent.
    let edge = pixel(&drawn, 22, 72);
    assert!(
        edge[0] < left[0] && edge[0] > pixel(&backdrop, 22, 72)[0],
        "edge is softened: {edge:?}"
    );

    // Additive light brightens the backdrop instead of covering it.
    scene.trails = vec![ribbon(-5., true)];
    let added = capture(&gpu, &mut renderer, &scene, false)?;
    let base = pixel(&backdrop, 22, 80);
    let glow = pixel(&added, 22, 80);
    assert!(
        glow[0] > base[0] && glow[1] >= base[1],
        "additive keeps the backdrop: {glow:?}"
    );

    // A wall in front hides the ribbon; raw diagnostics skip ribbons entirely.
    scene.items = vec![quad(-6.), quad(-4.)];
    let wall = capture(&gpu, &mut renderer, &scene, false)?;
    scene.trails.clear();
    assert_eq!(wall.rgba, capture(&gpu, &mut renderer, &scene, false)?.rgba);
    scene.items = vec![quad(-6.)];
    scene.trails = vec![ribbon(-5., false)];
    let raw = capture(&gpu, &mut renderer, &scene, true)?;
    scene.trails.clear();
    assert_eq!(raw.rgba, capture(&gpu, &mut renderer, &scene, true)?.rgba);
    Ok(())
}
