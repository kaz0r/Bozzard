use bozzard_assets::AssetStore;
use bozzard_render::{Gpu, RenderScene, SceneRenderer, wgpu};
use bozzard_scene::{
    GameSession, Layer, Scene,
    middleware::ui::{Input, Preferences},
};
#[test]
fn authored_widgets_render_and_accessible_actions_match_hit_geometry() -> anyhow::Result<()> {
    let mut scene = Scene::from_json(
        r#"{"version":1,"name":"Authorable menus","game_flow":{"title":"The midnight garden","instructions":"Explore the garden, collect three lights, and find your way home."},"views":{},"assets":{},"objects":[]}"#,
    )?;
    scene.ensure_game_menus()?;
    let mut world = Default::default();
    let instance = scene.spawn(&mut world)?;
    world.insert_resource(GameSession::default());
    let assets = AssetStore::new(std::path::Path::new("."), &scene.assets)?;
    let gpu = pollster::block_on(Gpu::request_prefer_software(&bozzard_render::instance(
        bozzard_render::Backend::native(),
    )))?;
    let mut renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8Unorm);
    instance.ui_input(
        &mut world,
        Layer::TwoD,
        [1280., 720.],
        Input::FocusNext { reverse: false },
    )?;
    for (name, size, scale, contrast) in [
        ("menu", [1280, 720], 1., false),
        ("menu-accessible", [1280, 720], 2., true),
        ("menu-small", [640, 360], 1., false),
    ] {
        world.insert_resource(Preferences {
            text_scale: Some(scale),
            high_contrast: Some(contrast),
            ..Default::default()
        });
        let frame = instance.ui_frame(&world, Layer::TwoD, size.map(|v| v as f32))?;
        let focus = frame.focusable()[0];
        let node = bozzard_render_assets::accessibility::node(focus, [0.; 2], 1.);
        assert_eq!(node.label(), Some("Start game"));
        assert_eq!(node.role(), accesskit::Role::Button);
        assert!(node.supports_action(accesskit::Action::Click));
        let bounds = node.bounds().unwrap();
        assert_eq!(bounds.x0, focus.rect.min[0] as f64);
        let render = RenderScene {
            skin_poses: Default::default(),
            shader_time: 0.,
            particles: vec![],
            fog: Default::default(),
            gi: None,
            lights: vec![],
            environment: bozzard_render::EnvironmentSettings::disabled(),
            display: Default::default(),
            lighting: Default::default(),
            view_projection: glam::Mat4::IDENTITY,
            items: bozzard_render_assets::widget_items(&frame, &assets)?,
        };
        let capture = bozzard_render::capture_offscreen(&gpu, size[0], size[1], |target| {
            renderer.draw(&gpu, target, size, &render)
        })?;
        assert!(
            capture
                .rgba
                .chunks_exact(4)
                .filter(|p| p[0] > 180 && p[1] > 180)
                .count()
                > 100,
            "widget text must be visible"
        );
        capture.write_ppm(&std::env::temp_dir().join(format!("middleware-{name}.ppm")))?;
    }
    Ok(())
}
