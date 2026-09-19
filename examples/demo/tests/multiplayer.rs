//! Verify that the authored multiplayer scene agrees with the network reference rules.
use bozzard_demo::SceneDemo;
use bozzard_network::{
    MAX_PLAYERS, Message,
    flap::{Bird, Host},
};
use bozzard_scene::{
    Layer, Scene, Transform,
    middleware::ui::{Input, WidgetKind},
};

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
    let mut demo = SceneDemo::new(&scene).unwrap();
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
    let demo = SceneDemo::new(&scene).unwrap();
    let mut host = Host::new(1);
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
    assert_eq!(Bird::new(0).y, 0.65);
}
