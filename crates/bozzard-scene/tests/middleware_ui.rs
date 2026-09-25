use bozzard_ecs::World;
use bozzard_scene::{
    GamePhase, GameSession, GameplayInput, Layer, Object, Scene,
    blueprint::{Blueprint, BlueprintAttachment, Node, NodeKind as K, Socket, Value, Wire},
    middleware::{
        registry,
        signals::{Kind, Signals},
        ui::*,
    },
};
fn base() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"UI","views":{},"assets":{},"objects":[]}"#).unwrap()
}
fn widget(scene: &mut Scene, id: &str, parent: &str, w: Widget) {
    let mut o = Object {
        id: id.into(),
        name: id.into(),
        parent: Some(parent.into()),
        ..Default::default()
    };
    registry::set(&mut o, &w).unwrap();
    scene.objects.push(o);
}
fn canvas(scene: &mut Scene) {
    let mut root = Object {
        id: "canvas".into(),
        name: "Canvas".into(),
        ..Default::default()
    };
    registry::set(
        &mut root,
        &Canvas {
            layer: Layer::TwoD,
            scaling: ScaleMode::Pixels,
            ..Default::default()
        },
    )
    .unwrap();
    scene.objects.push(root);
}
fn wire(a: u32, b: u32) -> Wire {
    Wire {
        from: Socket { node: a, port: 0 },
        to: Socket { node: b, port: 0 },
    }
}

#[test]
fn world_labels_project_at_viewport_aspect_and_fade_with_children() {
    let mut scene = base();
    canvas(&mut scene);
    let camera = Object {
        id: "camera".into(),
        name: "Camera".into(),
        transform: bozzard_scene::Transform {
            translation: [0., 0., 5.],
            ..Default::default()
        },
        camera: Some(bozzard_scene::Camera::Orthographic {
            vertical_size: 10.,
            near: 0.1,
            far: 100.,
        }),
        ..Default::default()
    };
    scene.objects.push(camera);
    scene.views.insert(Layer::TwoD, "camera".into());
    widget(
        &mut scene,
        "label",
        "canvas",
        Widget {
            anchors: Anchors {
                pivot: [0.5, 1.],
                size: [200., 40.],
                ..Default::default()
            },
            padding: [0.; 4],
            background: [1.; 4],
            ..Default::default()
        },
    );
    widget(
        &mut scene,
        "child",
        "label",
        Widget {
            text: "Nearby".into(),
            text_color: [1.; 4],
            ..Default::default()
        },
    );
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    instance
        .control_ui(&mut world, "label", Control::WorldPosition([1., 0., 0.]))
        .unwrap();
    instance
        .control_ui(&mut world, "label", Control::Opacity(0.5))
        .unwrap();
    instance
        .control_ui(&mut world, "child", Control::Opacity(0.5))
        .unwrap();
    for width in [800., 1200.] {
        let frame = instance
            .ui_frame(&world, Layer::TwoD, [width, 600.])
            .unwrap();
        let label = frame.element("label").unwrap();
        assert!((label.rect.min[0] - (width * 0.5 + 60. - 100.)).abs() < 0.01);
        assert!((label.rect.min[1] - 260.).abs() < 0.01);
        assert_eq!(label.widget.background[3], 0.5);
        assert_eq!(frame.element("child").unwrap().widget.text_color[3], 0.25);
    }
    instance
        .control_ui(&mut world, "label", Control::Size([80., 30.]))
        .unwrap();
    assert_eq!(
        instance
            .ui_frame(&world, Layer::TwoD, [800., 600.])
            .unwrap()
            .element("label")
            .unwrap()
            .rect
            .size,
        [80., 30.]
    );
    assert!(
        instance
            .control_ui(&mut world, "label", Control::Opacity(f32::NAN))
            .is_err()
    );
    assert!(
        instance
            .control_ui(&mut world, "label", Control::Size([-1., 20.]))
            .is_err()
    );
    world
        .resource::<Runtime>()
        .unwrap()
        .validate(&scene)
        .unwrap();
    instance
        .control_ui(&mut world, "label", Control::WorldPosition([0., 0., 10.]))
        .unwrap();
    assert!(
        instance
            .ui_frame(&world, Layer::TwoD, [800., 600.])
            .unwrap()
            .element("label")
            .is_none()
    );
}
#[test]
fn pointer_policy_follows_visible_enabled_controls_and_scroll_areas() {
    let mut scene = base();
    canvas(&mut scene);
    widget(
        &mut scene,
        "panel",
        "canvas",
        Widget {
            anchors: Anchors {
                size: [300., 160.],
                ..Default::default()
            },
            ..Default::default()
        },
    );
    widget(
        &mut scene,
        "control",
        "panel",
        Widget {
            kind: WidgetKind::Button,
            ..Default::default()
        },
    );
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let wants = |world: &World| {
        instance
            .ui_frame(world, Layer::TwoD, [800., 600.])
            .unwrap()
            .wants_pointer()
    };
    assert!(wants(&world));
    instance
        .control_ui(&mut world, "panel", Control::Enabled(false))
        .unwrap();
    assert!(!wants(&world), "disabled parents disable their controls");
    instance
        .control_ui(&mut world, "panel", Control::Enabled(true))
        .unwrap();
    assert!(wants(&world));
    instance
        .control_ui(&mut world, "panel", Control::Visible(false))
        .unwrap();
    assert!(!wants(&world), "hidden parents hide their controls");
    instance
        .control_ui(&mut world, "panel", Control::Visible(true))
        .unwrap();
    let control = instance.entity("control").unwrap();
    world.get_mut::<Widget>(control).unwrap().anchors.offset = [1000., 0.];
    assert!(!wants(&world), "clipped controls do not take the pointer");
    world.get_mut::<Widget>(control).unwrap().anchors.offset = [0.; 2];
    assert!(wants(&world));
    world.get_mut::<Widget>(control).unwrap().kind = WidgetKind::Label;
    assert!(
        !wants(&world),
        "decorative HUD content does not take the pointer"
    );
    {
        let mut label = world.get_mut::<Widget>(control).unwrap();
        label.anchors.size[1] = 500.;
        label.auto_text_height = false;
    }
    let panel = instance.entity("panel").unwrap();
    world.get_mut::<Widget>(panel).unwrap().scrollable = true;
    assert!(
        wants(&world),
        "overflowing scroll areas need a pointer without buttons"
    );
    let canvas = instance.entity("canvas").unwrap();
    world.get_mut::<Canvas>(canvas).unwrap().phase = Phase::Paused;
    assert!(
        !wants(&world),
        "inactive menu phases do not take the pointer"
    );
    world.insert_resource(GameSession {
        phase: GamePhase::Paused,
        ..Default::default()
    });
    assert!(wants(&world));
    assert!(
        !instance
            .ui_frame(&world, Layer::ThreeD, [800., 600.])
            .unwrap()
            .wants_pointer()
    );
    world.get_mut::<Canvas>(canvas).unwrap().enabled = false;
    assert!(!wants(&world));
}
#[test]
fn authored_menu_migration_and_ui_actions_leave_simulation_unticked() {
    let mut scene = base();
    scene.game_flow = Some(Default::default());
    assert!(scene.ensure_game_menus().unwrap());
    assert!(!scene.ensure_game_menus().unwrap());
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    world.insert_resource(GameSession::default());
    let frame = instance
        .ui_frame(&world, Layer::TwoD, [1280., 720.])
        .unwrap();
    assert_eq!(frame.focusable().len(), 2);
    let start = frame.focusable()[0];
    let p = [start.rect.min[0] + 20., start.rect.min[1] + 20.];
    instance
        .ui_input(
            &mut world,
            Layer::TwoD,
            [1280., 720.],
            Input::PointerDown(p),
        )
        .unwrap();
    instance.dispatch_ui_blueprints(&mut world).unwrap();
    assert_eq!(
        world.resource::<GameSession>().unwrap().phase,
        GamePhase::Ready
    );
    instance
        .ui_input(&mut world, Layer::TwoD, [1280., 720.], Input::PointerUp(p))
        .unwrap();
    instance.dispatch_ui_blueprints(&mut world).unwrap();
    assert_eq!(
        world.resource::<GameSession>().unwrap().phase,
        GamePhase::Playing
    );
    instance
        .ui_input(
            &mut world,
            Layer::TwoD,
            [1280., 720.],
            Input::Key("Escape".into()),
        )
        .unwrap();
    instance.dispatch_ui_blueprints(&mut world).unwrap();
    assert_eq!(
        world.resource::<GameSession>().unwrap().phase,
        GamePhase::Paused
    );
    instance
        .ui_input(
            &mut world,
            Layer::TwoD,
            [1280., 720.],
            Input::Key("Enter".into()),
        )
        .unwrap();
    instance.dispatch_ui_blueprints(&mut world).unwrap();
    assert_eq!(
        world.resource::<GameSession>().unwrap().phase,
        GamePhase::Playing
    );
    assert_eq!(
        instance
            .ui_frame(&world, Layer::TwoD, [1280., 720.])
            .unwrap()
            .focusable()
            .len(),
        1
    );
}
#[test]
fn paused_ui_delay_resumes_from_checkpoint_and_does_not_start_gameplay_graphs() {
    let mut scene = base();
    canvas(&mut scene);
    scene.game_flow = Some(Default::default());
    widget(
        &mut scene,
        "button",
        "canvas",
        Widget {
            kind: WidgetKind::Button,
            text: "Run".into(),
            ..Default::default()
        },
    );
    let mut delay = Node::new(2, K::Delay, [0.; 2]);
    delay.inputs[1] = Value::Number(1.);
    let mut label = Node::new(3, K::SetUiText, [0.; 2]);
    label.inputs[1] = Value::Text("Done".into());
    let graph = Blueprint {
        nodes: vec![Node::new(1, K::UiEvent, [0.; 2]), delay, label],
        wires: vec![
            wire(1, 2),
            wire(2, 3),
            Wire {
                from: Socket { node: 1, port: 1 },
                to: Socket { node: 3, port: 1 },
            },
        ],
        ..Default::default()
    };
    scene.objects[1].blueprints.push(BlueprintAttachment {
        enabled: true,
        graph,
    });
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    world.insert_resource(GameSession::default());
    instance
        .ui_input(
            &mut world,
            Layer::TwoD,
            [800., 600.],
            Input::ActivateObject("button".into()),
        )
        .unwrap();
    instance.dispatch_ui_blueprints(&mut world).unwrap();
    instance
        .step_blueprints(&mut world, 0.4, GameplayInput::default())
        .unwrap();
    let saved = instance.save_game_json(&world).unwrap();
    instance.load_game_json(&mut world, &saved).unwrap();
    instance
        .step_blueprints(&mut world, 0.7, GameplayInput::default())
        .unwrap();
    assert_eq!(
        instance
            .ui_frame(&world, Layer::TwoD, [800., 600.])
            .unwrap()
            .element("button")
            .unwrap()
            .text,
        "activate"
    );
    assert_eq!(
        world.resource::<GameSession>().unwrap().phase,
        GamePhase::Ready
    );
}
#[test]
fn layout_clipping_localization_focus_toggle_slider_and_saved_overrides() {
    let mut scene = base();
    canvas(&mut scene);
    let mut locale = Localization::default();
    std::sync::Arc::make_mut(&mut locale.translations)
        .insert("en".into(), [("sound".into(), "Sound".into())].into());
    std::sync::Arc::make_mut(&mut locale.translations)
        .insert("sv".into(), [("sound".into(), "Ljud".into())].into());
    registry::set(&mut scene.objects[0], &locale).unwrap();
    widget(
        &mut scene,
        "panel",
        "canvas",
        Widget {
            layout: Layout::Column,
            anchors: Anchors {
                min: [0.; 2],
                max: [1.; 2],
                size: [0.; 2],
                ..Default::default()
            },
            ..Default::default()
        },
    );
    widget(
        &mut scene,
        "toggle",
        "panel",
        Widget {
            kind: WidgetKind::Toggle,
            locale_key: "sound".into(),
            ..Default::default()
        },
    );
    widget(
        &mut scene,
        "slider",
        "panel",
        Widget {
            kind: WidgetKind::Slider,
            grow: 1.,
            step: 0.25,
            ..Default::default()
        },
    );
    let mut world = World::default();
    let mut instance = scene.spawn(&mut world).unwrap();
    instance.set_ui_language(&mut world, "sv").unwrap();
    let frame = instance
        .ui_frame(&world, Layer::TwoD, [800., 600.])
        .unwrap();
    assert_eq!(frame.element("toggle").unwrap().text, "Ljud");
    assert_eq!(frame.element("toggle").unwrap().rect.min, [12., 12.]);
    assert_eq!(frame.element("slider").unwrap().rect.size, [776., 516.]);
    instance
        .ui_input(
            &mut world,
            Layer::TwoD,
            [800., 600.],
            Input::FocusNext { reverse: false },
        )
        .unwrap();
    instance
        .ui_input(&mut world, Layer::TwoD, [800., 600.], Input::Activate)
        .unwrap();
    assert_eq!(
        world
            .resource::<Signals>()
            .unwrap()
            .for_owner("toggle", Kind::Ui)
            .next()
            .unwrap()
            .value,
        1.
    );
    instance
        .ui_input(
            &mut world,
            Layer::TwoD,
            [800., 600.],
            Input::FocusNext { reverse: false },
        )
        .unwrap();
    instance
        .ui_input(&mut world, Layer::TwoD, [800., 600.], Input::Adjust(1.))
        .unwrap();
    assert_eq!(
        world.resource::<Runtime>().unwrap().widgets["slider"].value,
        Some(0.25)
    );
    instance
        .control_ui(&mut world, "toggle", Control::Visible(false))
        .unwrap();
    let json = instance.save_game_json(&world).unwrap();
    instance.load_game_json(&mut world, &json).unwrap();
    let frame = instance
        .ui_frame(&world, Layer::TwoD, [800., 600.])
        .unwrap();
    assert!(frame.element("toggle").is_none());
    assert!(frame.element("slider").unwrap().focused);
    assert_eq!(
        world.resource::<Preferences>().unwrap().language.as_deref(),
        Some("sv")
    );
}

#[test]
fn enlarged_localized_text_reflows_and_keyboard_focus_scrolls_buttons_into_view() {
    let mut scene = base();
    scene.game_flow = Some(bozzard_scene::GameFlowSettings {
        title: "The midnight garden".into(),
        instructions: "Explore the garden, collect three lights, and find your way home.".into(),
    });
    scene.ensure_game_menus().unwrap();
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    world.insert_resource(GameSession::default());
    world.insert_resource(Preferences {
        text_scale: Some(3.),
        ..Default::default()
    });
    let frame = instance
        .ui_frame(&world, Layer::TwoD, [1280., 720.])
        .unwrap();
    let title = frame
        .elements
        .iter()
        .find(|e| e.widget.binding == "game.title")
        .unwrap();
    assert!(title.rect.size[1] > 100.);
    assert!(frame.elements.iter().any(|e| e.scroll_max > 100.));
    for _ in 0..2 {
        instance
            .ui_input(
                &mut world,
                Layer::TwoD,
                [1280., 720.],
                Input::FocusNext { reverse: false },
            )
            .unwrap();
    }
    let frame = instance
        .ui_frame(&world, Layer::TwoD, [1280., 720.])
        .unwrap();
    let focused = frame.elements.iter().find(|e| e.focused).unwrap();
    assert_eq!(focused.text, "Quit");
    assert_eq!(
        focused.rect.size, focused.clip.size,
        "focused button must be fully visible after scrolling"
    );
    assert!(frame.scroll_ancestor(focused).unwrap().scroll > 0.);
}

#[test]
fn script_pointer_events_and_screen_popups_keep_viewport_coordinates_and_order() {
    let mut scene = base();
    canvas(&mut scene);
    widget(
        &mut scene,
        "slot",
        "canvas",
        Widget {
            kind: WidgetKind::Button,
            anchors: Anchors {
                min: [0.; 2],
                max: [0.; 2],
                pivot: [0.; 2],
                offset: [20., 30.],
                size: [120., 80.],
            },
            padding: [0.; 4],
            ..Default::default()
        },
    );
    widget(
        &mut scene,
        "label",
        "slot",
        Widget {
            kind: WidgetKind::Label,
            text: "Item".into(),
            anchors: Anchors {
                size: [100., 40.],
                ..Default::default()
            },
            ..Default::default()
        },
    );
    widget(
        &mut scene,
        "popup",
        "canvas",
        Widget {
            anchors: Anchors {
                size: [160., 100.],
                ..Default::default()
            },
            visible: false,
            ..Default::default()
        },
    );
    let mut world = World::default();
    let instance = scene.spawn(&mut world).unwrap();
    let point = [60., 50.];
    for input in [
        Input::PointerDown(point),
        Input::PointerUp(point),
        Input::SecondaryDown(point),
    ] {
        assert!(
            instance
                .ui_input(&mut world, Layer::TwoD, [800., 600.], input)
                .unwrap()
        );
    }
    let ui = world.resource::<Runtime>().unwrap();
    assert_eq!(
        ui.script_events.iter().map(|e| e.kind).collect::<Vec<_>>(),
        ["down", "up", "activate", "secondary"]
    );
    assert!(ui.script_events.iter().all(|e| e.target == "slot"));
    assert_eq!(ui.script_events[3].position, [60. / 800., 50. / 600.]);
    assert!(
        ui.active.is_none(),
        "right-click must not start a left-button drag"
    );
    instance
        .control_ui(&mut world, "popup", Control::Visible(true))
        .unwrap();
    instance
        .control_ui(&mut world, "popup", Control::ScreenPosition([0.99, 0.99]))
        .unwrap();
    instance
        .control_ui(&mut world, "popup", Control::Offset([0., 8.]))
        .unwrap();
    for size in [[800., 600.], [360., 240.]] {
        let frame = instance.ui_frame(&world, Layer::TwoD, size).unwrap();
        let rect = frame.element("popup").unwrap().rect;
        assert_eq!(rect.min, [size[0] - 160., size[1] - 100.]);
    }
    instance
        .ui_input(&mut world, Layer::TwoD, [800., 600.], Input::CancelPointer)
        .unwrap();
    let ui = world.resource::<Runtime>().unwrap();
    assert_eq!(ui.script_events.last().unwrap().kind, "cancel");
    let serialized = serde_json::to_string(ui).unwrap();
    assert!(!serialized.contains("script_events"));
    let restored: Runtime = serde_json::from_str(&serialized).unwrap();
    restored.validate(&scene).unwrap();
    assert!(
        restored.script_events.is_empty(),
        "pointer actions cannot replay on reload"
    );
    assert_eq!(
        restored.widgets["popup"].screen_position,
        Some([0.99, 0.99])
    );
}
