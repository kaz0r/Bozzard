use bozzard_assets::AssetStore;
use bozzard_render::{Gpu, RenderScene, SceneRenderer, wgpu};
use bozzard_scene::{
    GameSession, Layer, Scene,
    middleware::ui::{Input, Preferences},
};

#[test]
fn content_sized_labels_center_real_glyphs_after_text_and_scale_changes() -> anyhow::Result<()> {
    use bozzard_render::{MeshKind, text_bounds};
    use bozzard_scene::middleware::ui::Control;
    let scene = Scene::from_json(
        r#"{
        "version":1,"name":"Labels","views":{},"assets":{},"objects":[
            {"id":"canvas","name":"Canvas","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"ui_canvas":{
                "layer":"2d","reference":[1080,600],"scaling":"fit"
            }},
            {"id":"label","name":"Label","parent":"canvas","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"ui_widget":{
                "kind":"label","text":"Creative","font_size":16,
                "text_alignment":"center","auto_text_width":true,"auto_text_height":true,
                "anchors":{"min":[0.5,0.5],"max":[0.5,0.5],"pivot":[0.5,0.5],"size":[0,36]},
                "padding":[14,8,14,8]
            }}
        ]
    }"#,
    )?;
    let mut world = Default::default();
    let instance = scene.spawn(&mut world)?;
    let assets = AssetStore::new(std::path::Path::new("."), &scene.assets)?;
    for (size, text_scale) in [([1080., 600.], 1.), ([1920., 1080.], 1.5)] {
        world.insert_resource(Preferences {
            text_scale: Some(text_scale),
            ..Default::default()
        });
        let mut widths = Vec::new();
        for label in [
            "iiii",
            "WWWW",
            "Creative",
            "Tier 1",
            "Fuel dock / INPUT  [E] Inspect",
        ] {
            instance.control_ui(&mut world, "label", Control::Text(label.into()))?;
            let frame = instance.ui_frame(&world, Layer::TwoD, size)?;
            let element = frame.element("label").unwrap();
            let items = bozzard_render_assets::widget_items(&frame, &assets)?;
            let text = items
                .iter()
                .find_map(|item| match &item.mesh {
                    MeshKind::Text(text) => Some(text),
                    _ => None,
                })
                .unwrap();
            let bounds = text_bounds(text)?.unwrap();
            let x = text.screen.unwrap().offset[0];
            let left = x + bounds[0].x - element.rect.min[0];
            let right = element.rect.min[0] + element.rect.size[0] - x - bounds[1].x;
            assert!(
                // Rasterized glyph positions may round to opposite pixel edges.
                (left - right).abs() <= 1.1,
                "uneven padding for {label}: {left} / {right}"
            );
            assert!((left - 14. * element.scale).abs() <= 1.1);
            assert!(
                (element.rect.min[0] + element.rect.size[0] * 0.5 - size[0] * 0.5).abs() < 0.01
            );
            assert!(
                (-bounds[0].y + bounds[1].y) < element.rect.size[1],
                "label wrapped: {label}"
            );
            widths.push(element.rect.size[0]);
        }
        assert!(
            widths[1] > widths[0] * 1.5,
            "equal character counts need different glyph widths"
        );
    }
    Ok(())
}

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

#[test]
fn pointer_focus_omits_the_ring_and_keyboard_navigation_restores_it() -> anyhow::Result<()> {
    let mut scene = Scene::from_json(
        r#"{"version":1,"name":"Focus feedback","game_flow":{"title":"Focus","instructions":"Test"},"views":{},"assets":{},"objects":[]}"#,
    )?;
    scene.ensure_game_menus()?;
    let mut world = Default::default();
    let instance = scene.spawn(&mut world)?;
    world.insert_resource(GameSession::default());
    let assets = AssetStore::new(std::path::Path::new("."), &scene.assets)?;
    let frame = instance.ui_frame(&world, Layer::TwoD, [1080., 600.])?;
    let button = frame.focusable()[0];
    let id = button.owner.clone();
    let p = [
        button.rect.min[0] + button.rect.size[0] / 2.,
        button.rect.min[1] + button.rect.size[1] / 2.,
    ];
    instance.ui_input(
        &mut world,
        Layer::TwoD,
        [1080., 600.],
        Input::PointerDown(p),
    )?;
    let frame = instance.ui_frame(&world, Layer::TwoD, [1080., 600.])?;
    assert!(frame.element(&id).unwrap().focused);
    assert!(!frame.element(&id).unwrap().focus_visible);
    let without = bozzard_render_assets::widget_items(&frame, &assets)?.len();
    instance.ui_input(
        &mut world,
        Layer::TwoD,
        [1080., 600.],
        Input::Focus(id.clone()),
    )?;
    let frame = instance.ui_frame(&world, Layer::TwoD, [1080., 600.])?;
    assert!(frame.element(&id).unwrap().focus_visible);
    assert_eq!(
        bozzard_render_assets::widget_items(&frame, &assets)?.len(),
        without + 4
    );
    instance.control_ui(
        &mut world,
        "",
        bozzard_scene::middleware::ui::Control::ClearFocus,
    )?;
    assert!(
        !instance
            .ui_frame(&world, Layer::TwoD, [1080., 600.])?
            .element(&id)
            .unwrap()
            .focused
    );
    Ok(())
}
