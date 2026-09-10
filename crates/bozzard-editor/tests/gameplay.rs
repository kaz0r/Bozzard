use bozzard_editor::Editor;
use bozzard_scene::{GameplayInput, GameplayState, Scene, Transform};
use std::{path::PathBuf, time::Duration};

fn editor() -> Editor {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/first-trail.json");
    Editor::new(
        Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap(),
        &path,
    )
    .unwrap()
}
#[test]
fn play_controller_is_selection_independent_and_stop_resets_all_state() {
    let mut editor = editor();
    let original = editor.scene().clone();
    let mut outcomes = Vec::new();
    for selection in [None, Some("camera"), Some("gold-1"), Some("player")] {
        editor.selected = selection.map(str::to_owned);
        editor.start_play().unwrap();
        for tick in 0..340 {
            editor
                .play
                .as_mut()
                .unwrap()
                .set_gameplay_input(GameplayInput {
                    movement: [0.0, 1.0],
                    jump: tick == 80,
                    ..Default::default()
                });
            editor.advance(Duration::from_secs_f64(1.0 / 60.0));
        }
        let play = editor.play.as_ref().unwrap();
        play.check_simulation().unwrap();
        let state = play.gameplay().unwrap();
        assert!(state.won);
        assert_eq!(state.collected.len(), 3);
        let player = play.instance.entity("player").unwrap();
        outcomes.push(*play.app.world.get::<Transform>(player).unwrap());
        assert_eq!(editor.scene(), &original);
        editor.stop_play();
        assert_eq!(editor.scene(), &original);
        editor.start_play().unwrap();
        let play = editor.play.as_ref().unwrap();
        let state = play.app.world.resource::<GameplayState>().unwrap();
        assert!(!state.won && state.collected.is_empty() && state.checkpoint.is_none());
        assert_eq!(state.respawns, 0);
        assert_eq!(
            play.app.world.resource::<GameplayInput>().unwrap().movement,
            [0.0; 2]
        );
        assert_eq!(play.instance.capture(&play.app.world).unwrap(), original);
        editor.stop_play();
    }
    assert!(outcomes.windows(2).all(|pair| pair[0] == pair[1]));
}
#[test]
fn inspector_settings_are_undoable_and_invalid_camera_edits_preserve_document() {
    let mut editor = editor();
    let original = editor.scene().clone();
    let mut changed = original.clone();
    changed
        .objects
        .iter_mut()
        .find(|o| o.id == "player")
        .unwrap()
        .player_controller
        .as_mut()
        .unwrap()
        .camera_distance = 8.0;
    editor.apply("Controller tuning", changed.clone()).unwrap();
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &original);
    editor.redo().unwrap();
    assert_eq!(editor.scene(), &changed);
    let mut invalid = changed.clone();
    invalid
        .objects
        .iter_mut()
        .find(|o| o.id == "player")
        .unwrap()
        .player_controller
        .as_mut()
        .unwrap()
        .camera = "missing".into();
    assert!(editor.apply("Invalid reference", invalid).is_err());
    assert_eq!(editor.scene(), &changed);
    editor.selected = Some("player".into());
    assert!(editor.duplicate().is_err());
    assert_eq!(editor.scene(), &changed);
}

#[test]
fn authored_lighting_survives_undo_play_and_scene_roundtrip() {
    let mut editor = editor();
    let original = editor.scene().clone();
    let mut changed = original.clone();
    changed.lighting.sun_intensity = 8.;
    changed.display.exposure_ev = 2.;
    changed.display.tone_mapping = false;
    changed.lighting.shadow_resolution = 4096;
    changed.lighting.shadow_bias = 0.02;
    changed.lighting.sun_direction = [-1., 1., 0.];
    changed.lighting.ambient_color = [0.2, 0.4, 0.8];
    editor.apply("Lighting", changed.clone()).unwrap();
    assert_eq!(
        editor
            .render(bozzard_scene::Layer::ThreeD, 1.)
            .unwrap()
            .lighting
            .sun_intensity,
        8.
    );
    assert_eq!(
        editor
            .render(bozzard_scene::Layer::ThreeD, 1.)
            .unwrap()
            .display
            .exposure_ev,
        2.
    );
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &original);
    editor.redo().unwrap();
    assert_eq!(editor.scene(), &changed);
    assert_eq!(
        Scene::from_json(&changed.to_json().unwrap()).unwrap(),
        changed
    );
    editor.start_play().unwrap();
    assert_eq!(
        editor
            .render(bozzard_scene::Layer::ThreeD, 1.)
            .unwrap()
            .lighting
            .sun_direction,
        [-1., 1., 0.]
    );
    assert!(editor.apply("Forbidden in Play", original).is_err());
    editor.stop_play();
    assert_eq!(editor.scene(), &changed);
    let mut invalid = changed.clone();
    invalid.lighting.sun_direction = [0.; 3];
    assert!(editor.apply("Invalid", invalid).is_err());
    assert_eq!(editor.scene(), &changed);
}

#[test]
fn display_exposure_is_3d_only() {
    let mut scene = bozzard_demo::scene_document().unwrap();
    scene.display.exposure_ev = 3.;
    let demo = bozzard_demo::SceneDemo::new(&scene).unwrap();
    let two = bozzard_editor::extract(&demo, bozzard_scene::Layer::TwoD, 1.).unwrap();
    assert_eq!(two.display.exposure_ev, 0.);
    assert!(!two.display.tone_mapping);
    let three = bozzard_editor::extract(&demo, bozzard_scene::Layer::ThreeD, 1.).unwrap();
    assert_eq!(three.display.exposure_ev, 3.);
    assert!(three.display.tone_mapping);
}
