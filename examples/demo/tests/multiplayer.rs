//! Verify that the authored multiplayer scene agrees with the network reference rules.
use bozzard_demo::SceneDemo;
use bozzard_network::{MAX_PLAYERS, Message, flap::Host};
use bozzard_scene::{
    Layer, Scene, Transform,
    middleware::ui::{Input, WidgetKind},
};

fn demo(scene: &Scene) -> SceneDemo {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scenes/flap-woods-multiplayer.json");
    SceneDemo::new_with_prefabs(scene, Some(&path)).unwrap()
}

#[test]
fn steam_scene_loads_roundtrips_and_has_no_local_start_or_solo_physics() {
    let scene = Scene::from_json(include_str!("../scenes/flap-woods-multiplayer.json")).unwrap();
    let serialized = scene.to_json().unwrap();
    let scene = Scene::from_json(&serialized).unwrap();
    assert!(
        scene
            .objects
            .iter()
            .any(|o| o.extras.contains_key("steam_multiplayer"))
    );
    let mut demo = demo(&scene);
    assert!(demo.game_session().is_none());
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, Layer::ThreeD, [1280., 720.])
        .unwrap();
    for id in [
        "steam-create",
        "steam-invite",
        "steam-start",
        "steam-leave",
        "steam-quit",
    ] {
        assert_eq!(frame.element(id).unwrap().widget.kind, WidgetKind::Button);
    }
    // An editor or non-network engine cannot start a round by dispatching the button.
    let entity = demo.instance().entity("bird-0").unwrap();
    let before = *demo.app.world.get::<Transform>(entity).unwrap();
    demo.ui_input(
        Layer::ThreeD,
        [1280., 720.],
        Input::ActivateObject("steam-start".into()),
    )
    .unwrap();
    for _ in 0..120 {
        demo.app.step();
    }
    assert_eq!(*demo.app.world.get::<Transform>(entity).unwrap(), before);
    assert!(demo.game_session().is_none());
}

#[test]
fn replicated_slots_and_collisions_match_authored_geometry() {
    let scene = Scene::from_json(include_str!("../scenes/flap-woods-multiplayer.json")).unwrap();
    let demo = demo(&scene);
    let rules = bozzard_demo::multiplayer::rules_for(demo.instance()).unwrap();
    let mut host = Host::new(1, rules.clone()).unwrap();
    for peer in 2..=MAX_PLAYERS as u64 {
        host.join(peer).unwrap();
    }
    host.start(1).unwrap();
    let snapshot = host.snapshot(2).unwrap();
    let bytes = bozzard_network::encode(480, Message::Snapshot(snapshot)).unwrap();
    let Message::Snapshot(snapshot) = bozzard_network::decode(480, &bytes).unwrap() else {
        panic!()
    };
    for bird in snapshot.birds.values() {
        let entity = demo
            .instance()
            .entity(&format!("bird-{}", bird.slot))
            .unwrap();
        let authored = demo.app.world.get::<Transform>(entity).unwrap();
        assert!((authored.translation[0] - bird.x()).abs() < 0.001);
        assert_eq!(authored.translation[1], bird.y);
        assert_eq!(authored.scale, [0.8; 3]);
    }
    let bird_half = 0.4;
    for (i, pipe) in snapshot.pipes.iter().enumerate() {
        let e = demo
            .instance()
            .entity(&format!("pipe-{}-bottom", i + 1))
            .unwrap();
        let bottom = demo.app.world.get::<Transform>(e).unwrap();
        let clearance = pipe.gap - (bottom.translation[1] + bottom.scale[1] / 2.) - bird_half;
        assert!((clearance - 2.15).abs() < 0.001);
        assert!((bottom.scale[0] / 2. + bird_half - 1.).abs() < 0.001);
    }
    assert_eq!(rules.spawn(0).unwrap().y, 0.65);
}

#[test]
fn editing_the_player_asset_changes_host_prediction_and_replay_without_native_rules() {
    use bozzard_network::{
        DT,
        flap::{InputFrame, Replica},
    };
    let scene = Scene::from_json(include_str!("../scenes/flap-woods-multiplayer.json")).unwrap();
    let mut demo = demo(&scene);
    let original = bozzard_demo::multiplayer::rules_for(demo.instance()).unwrap();
    let source = include_str!("../scenes/scripts/flap-woods-multiplayer/player.rs")
        .replace("6.5", "9.0")
        .replace("22.0", "10.0")
        .replace("key == \"Space\"", "key == \"F\"");
    demo.with_instance(|instance, _| instance.register_script("flap-player".into(), source))
        .unwrap();
    let rules = bozzard_demo::multiplayer::rules_for(demo.instance()).unwrap();
    assert_ne!(rules.fingerprint(), original.fingerprint());
    assert!(rules.input("F").unwrap());
    assert!(!rules.input("Space").unwrap());
    let mut host = Host::new(1, rules.clone()).unwrap();
    host.join(2).unwrap();
    host.start(1).unwrap();
    for _ in 0..rules.countdown().unwrap() {
        host.step().unwrap();
    }
    let mut replica = Replica::new(rules);
    replica.apply(1, 1, 2, host.snapshot(2).unwrap()).unwrap();
    replica.input(true).unwrap();
    host.receive(
        2,
        Message::Input {
            round: host.round,
            ack: 0,
            frames: vec![InputFrame {
                sequence: 1,
                flap: true,
            }],
        },
    )
    .unwrap();
    host.step().unwrap();
    let authoritative = host.bird(2).unwrap();
    assert!((authoritative.velocity - (9.0 - 10.0 * DT)).abs() < 0.00001);
    assert_eq!(replica.predicted.unwrap().y, authoritative.y);
    // Second input is predicted, then replayed over an acknowledgement of the first.
    replica.input(false).unwrap();
    let predicted = replica.predicted.unwrap();
    replica.apply(1, 1, 2, host.snapshot(2).unwrap()).unwrap();
    assert_eq!(replica.predicted.unwrap().y, predicted.y);
    assert_eq!(replica.predicted.unwrap().velocity, predicted.velocity);
}

#[test]
fn round_rules_and_visuals_are_authored_scripts_too() {
    use bozzard_scene::{GameplayInput, NetworkFrame, TextRendering};
    let scene = Scene::from_json(include_str!("../scenes/flap-woods-multiplayer.json")).unwrap();
    let mut demo = demo(&scene);
    let world = include_str!("../scenes/scripts/flap-woods-multiplayer/round.rs")
        .replace("{ 300 }", "{ 60 }")
        .replace("3.2 * dt", "8.0 * dt")
        .replace("player.score += 1", "player.score += 7");
    demo.with_instance(|instance, _| instance.register_script("flap-round".into(), world))
        .unwrap();
    let player = include_str!("../scenes/scripts/flap-woods-multiplayer/player.rs")
        .replace("player.velocity * 4.0", "player.velocity * 2.0");
    demo.with_instance(|instance, _| instance.register_script("flap-player".into(), player))
        .unwrap();
    let rules = bozzard_demo::multiplayer::rules_for(demo.instance()).unwrap();
    assert_eq!(rules.countdown().unwrap(), 60);
    let mut pipes = rules.pipes().unwrap();
    rules.step(&mut pipes).unwrap();
    assert!((pipes[0].x - (2.0 - 8.0 * bozzard_network::DT)).abs() < 0.00001);
    let mut bird = rules.spawn(0).unwrap();
    let mut before = rules.pipes().unwrap();
    before[0].x = bird.x - 0.99;
    let mut after = before;
    after[0].x = bird.x - 1.01;
    rules.resolve(&mut bird, &before, &after).unwrap();
    assert_eq!(bird.score, 7);
    bird.velocity = 2.0;
    let mut frame = NetworkFrame {
        active: true,
        ..Default::default()
    };
    frame
        .objects
        .insert("bird-0".into(), serde_json::to_value(bird).unwrap());
    let mut hud = serde_json::to_value(bird).unwrap();
    hud["local"] = true.into();
    frame.state = serde_json::json!({"players": [hud]});
    demo.app.world.insert_resource(frame);
    demo.with_instance(|instance, world| {
        instance.step_scripts(world, bozzard_network::DT, GameplayInput::default())
    })
    .unwrap();
    let entity = demo.instance().entity("bird-0").unwrap();
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(entity)
            .unwrap()
            .rotation_degrees[2],
        4.0
    );
    let score = demo.instance().entity("score").unwrap();
    assert_eq!(
        demo.app.world.get::<TextRendering>(score).unwrap().text,
        "P1 YOU: 7"
    );
}

#[test]
fn missing_disabled_bad_and_throwing_network_scripts_fail_loudly() {
    let scene = Scene::from_json(include_str!("../scenes/flap-woods-multiplayer.json")).unwrap();
    let unloaded = SceneDemo::new(&scene).unwrap();
    assert!(bozzard_demo::multiplayer::rules_for(unloaded.instance()).is_err());
    let mut demo = demo(&scene);
    demo.with_instance(|instance, _| instance.set_script_enabled("bird-2", 0, false))
        .unwrap();
    assert!(
        bozzard_demo::multiplayer::rules_for(demo.instance())
            .err()
            .unwrap()
            .to_string()
            .contains("bird-2")
    );
    demo.with_instance(|instance, _| instance.set_script_enabled("bird-2", 0, true))
        .unwrap();
    assert!(
        demo.with_instance(|instance, _| instance.register_script(
            "flap-player".into(),
            "fn network_predict(player) { player }".into()
        ))
        .is_err()
    );
    let source = include_str!("../scenes/scripts/flap-woods-multiplayer/player.rs")
        .replace("player.velocity = 6.5;", "throw \"broken flap\";");
    demo.with_instance(|instance, _| instance.register_script("flap-player".into(), source))
        .unwrap();
    let rules = bozzard_demo::multiplayer::rules_for(demo.instance()).unwrap();
    let mut bird = rules.spawn(0).unwrap();
    let error = rules.predict(&mut bird, true).unwrap_err().to_string();
    assert!(
        error.contains("flap-player")
            && error.contains("network_predict")
            && error.contains("broken flap")
            && error.contains("line"),
        "{error}"
    );
}
