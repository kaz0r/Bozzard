use bozzard_demo::SceneDemo;
use bozzard_scene::{GameAction as A, GamePhase as P, GameplayInput, Scene, TextRendering};

fn load() -> Scene {
    Scene::from_json(include_str!("../scenes/game-flow-lab.json")).unwrap()
}
fn step(d: &mut SceneDemo, jump: bool) {
    d.set_gameplay_input(GameplayInput {
        jump,
        ..Default::default()
    });
    d.app.step();
    d.check_simulation().unwrap();
}
fn text(d: &SceneDemo) -> &str {
    &d.app
        .world
        .get::<TextRendering>(d.instance().entity("counter").unwrap())
        .unwrap()
        .text
}
#[test]
fn ready_pause_end_and_retry_freeze_and_reset_the_complete_world() {
    let source = load();
    assert_eq!(
        Scene::from_json(&source.to_json().unwrap()).unwrap(),
        source
    );
    let mut d = SceneDemo::new(&source).unwrap();
    let initial = d.instance().capture(&d.app.world).unwrap();
    for _ in 0..30 {
        step(&mut d, true);
    }
    assert_eq!(d.instance().capture(&d.app.world).unwrap(), initial);
    d.game_action(A::Start).unwrap();
    step(&mut d, false);
    assert_eq!(text(&d), "Taps: 0", "menu input must not leak into play");
    step(&mut d, true);
    d.game_action(A::Pause).unwrap();
    let paused = d.instance().capture(&d.app.world).unwrap();
    for _ in 0..100 {
        step(&mut d, true);
    }
    assert_eq!(d.instance().capture(&d.app.world).unwrap(), paused);
    d.game_action(A::Resume).unwrap();
    step(&mut d, false);
    assert_eq!(text(&d), "Taps: 1");
    step(&mut d, true);
    step(&mut d, true);
    assert_eq!(d.game_session().unwrap().phase, P::GameOver);
    assert_eq!(text(&d), "Taps: 3");
    let ended = d.instance().capture(&d.app.world).unwrap();
    for _ in 0..60 {
        step(&mut d, true);
    }
    assert_eq!(d.instance().capture(&d.app.world).unwrap(), ended);
    d.game_action(A::Restart).unwrap();
    assert_eq!(d.game_session().unwrap().phase, P::Playing);
    assert_eq!(d.instance().capture(&d.app.world).unwrap(), initial);
    step(&mut d, false);
    assert_eq!(text(&d), "Taps: 0");
    step(&mut d, true);
    assert_eq!(text(&d), "Taps: 1", "Blueprint variables restart too");
    d.game_action(A::Quit).unwrap();
    let quit = d.instance().capture(&d.app.world).unwrap();
    step(&mut d, true);
    assert_eq!(d.instance().capture(&d.app.world).unwrap(), quit);
}

#[test]
fn end_game_stops_other_events_and_requires_opt_in() {
    let mut source = load();
    let graph = &mut source
        .objects
        .iter_mut()
        .find(|o| o.id == "counter")
        .unwrap()
        .blueprints[0]
        .graph;
    // Ending on Start must prevent the later InputPressed event in the same tick.
    graph.wires.retain(|w| w.to.node != 12);
    graph.wires.push(bozzard_scene::blueprint::Wire {
        from: bozzard_scene::blueprint::Socket { node: 1, port: 0 },
        to: bozzard_scene::blueprint::Socket { node: 12, port: 0 },
    });
    let mut d = SceneDemo::new(&source).unwrap();
    d.game_action(A::Start).unwrap();
    step(&mut d, true);
    assert_eq!(text(&d), "Taps: 0");
    assert_eq!(d.game_session().unwrap().phase, P::GameOver);
    source.game_flow = None;
    let mut legacy = SceneDemo::new(&source).unwrap();
    legacy.app.step();
    assert!(format!("{:#}", legacy.check_simulation().unwrap_err()).contains("Game Flow enabled"));
}

#[test]
fn menus_are_editable_scene_widgets_and_game_metadata_is_validated() {
    let mut source = load();
    let demo = SceneDemo::new(&source).unwrap();
    assert!(
        demo.instance()
            .document()
            .objects
            .iter()
            .any(|o| o.extras.contains_key("ui_canvas"))
    );
    assert!(
        demo.instance()
            .document()
            .objects
            .iter()
            .any(|o| o.extras.contains_key("ui_widget") && !o.blueprints.is_empty())
    );
    assert!(
        bozzard_scene::GameSession::default()
            .end_game(&"x".repeat(241))
            .is_err()
    );
    source.game_flow.as_mut().unwrap().title.clear();
    assert!(source.validate().is_err());
}

#[test]
fn retry_removes_spawned_objects_and_keeps_templates_without_source_files() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scenes");
    let temporary = std::env::temp_dir().join(format!("bozzard-game-flow-{}", std::process::id()));
    std::fs::create_dir_all(temporary.join("assets/bonfire")).unwrap();
    std::fs::copy(
        source.join("assets/bonfire/ember.prefab.json"),
        temporary.join("assets/bonfire/ember.prefab.json"),
    )
    .unwrap();
    let mut scene = Scene::from_json(include_str!("../scenes/bonfire-lab.json")).unwrap();
    scene.game_flow = Some(Default::default());
    let mut d = SceneDemo::new_with_prefabs(&scene, Some(&temporary.join("scene.json"))).unwrap();
    std::fs::remove_dir_all(temporary).unwrap();
    d.game_action(A::Start).unwrap();
    let original = d.instance().document().objects.len();
    for _ in 0..2 {
        d.with_instance(|instance, world| instance.spawn_prefab(world, "bonfire-ember", [0.; 3]))
            .unwrap();
        assert!(d.instance().document().objects.len() > original);
        d.game_action(A::Restart).unwrap();
        assert_eq!(d.instance().document().objects.len(), original);
    }
}
