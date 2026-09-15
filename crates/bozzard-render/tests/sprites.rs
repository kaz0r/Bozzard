use bozzard_render::{
    DrawItem, Gpu, Material, MeshKind, RenderScene, SceneRenderer, ScreenText, SpriteMesh,
    SpriteQuad, TextureKind, wgpu,
};
use glam::Mat4;
#[test]
fn atlas_sprites_and_screen_clipping_share_color_without_camera_or_display_effects()
-> anyhow::Result<()> {
    let instance = bozzard_render::instance(bozzard_render::Backend::native());
    let gpu = pollster::block_on(Gpu::request(
        &instance,
        None,
        cfg!(not(target_os = "macos")),
    ))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    let atlas: Vec<u8> = (0..16)
        .flat_map(|i| {
            if i % 4 < 2 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            }
        })
        .collect();
    renderer.upload_image(&gpu, "atlas", 4, 4, &atlas)?;
    let mut sprite = SpriteMesh::new(vec![SpriteQuad {
        rect: [0., 0., 64., 48.],
        uv: [0.5, 0., 0.5, 1.],
    }])?;
    sprite.screen = Some(ScreenText {
        anchor: [0., 0.],
        offset: [20., 20.],
    });
    sprite.clip = Some([40., 20., 44., 48.]);
    let mut scene = RenderScene {
        skin_poses: Default::default(),
        particles: vec![],
        fog: Default::default(),
        gi: None,
        lights: vec![],
        environment: Default::default(),
        display: Default::default(),
        lighting: Default::default(),
        view_projection: Mat4::IDENTITY,
        shader_time: 0.,
        items: vec![DrawItem {
            motion_id: 1,
            model: Mat4::IDENTITY,
            mesh: MeshKind::Sprite(sprite),
            material: Material {
                metallic: None,
                roughness: None,
                surface_overrides: Default::default(),
                tint: [1.; 3],
                uv_scale: [1.; 2],
                texture: TextureKind::Imported("atlas".into()),
                lit: false,
                shader: None,
            },
        }],
    };
    let capture = |renderer: &mut SceneRenderer, scene: &RenderScene| {
        bozzard_render::capture_offscreen(&gpu, 128, 96, |target| {
            renderer.draw(&gpu, target, [128, 96], scene)
        })
    };
    let a = capture(&mut renderer, &scene)?;
    let green = |image: &bozzard_render::Frame, x: usize, y: usize| {
        let p = &image.rgba[(y * 128 + x) * 4..][..4];
        p[1] > 200 && p[0] < 20
    };
    assert!(green(&a, 60, 40));
    assert!(!green(&a, 30, 40));
    assert!(!green(&a, 90, 40));
    scene.view_projection = Mat4::from_translation(glam::Vec3::splat(100.));
    scene.display.exposure_ev = -8.;
    let b = capture(&mut renderer, &scene)?;
    assert!(green(&b, 60, 40));
    renderer.upload_image(&gpu, "atlas", 4, 4, &[0, 0, 255, 255].repeat(16))?;
    let c = capture(&mut renderer, &scene)?;
    let p = &c.rgba[(40 * 128 + 60) * 4..][..4];
    assert!(
        p[2] > 200 && p[1] < 20,
        "hot reload must invalidate HUD bindings: {p:?}"
    );
    scene.items.clear();
    let d = capture(&mut renderer, &scene)?;
    assert!(!green(&d, 60, 40));
    Ok(())
}
