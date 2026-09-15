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
