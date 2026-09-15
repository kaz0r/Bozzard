use bozzard_editor::Editor;
use bozzard_scene::{
    GamePhase, Layer, Transform,
    middleware::{
        animation, audio, sprite,
        ui::{self, Input},
    },
};
use std::path::Path;
fn scene(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../examples/demo/scenes/{name}.json"))
}
#[test]
fn middleware_lab_runs_controls_events_animation_navigation_and_restore() -> anyhow::Result<()> {
    let mut editor = Editor::open(&scene("middleware-lab"))?;
    let authored = editor.scene().clone();
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    for _ in 0..45 {
        play.app.step();
    }
    play.check_simulation()?;
    let frame = play
        .instance()
        .view(&play.app.world, Layer::ThreeD, 16. / 9.)?;
    assert!(!frame.skin_poses.is_empty());
    assert!(!frame.particles.is_empty());
    assert!(
        play.app.world.resource::<audio::Runtime>().unwrap().voices["animated-banner"].epoch > 0,
        "animation marker did not execute its audio Blueprint"
    );
    let position = play
        .app
        .world
        .get::<Transform>(play.instance().entity("patrol-agent").unwrap())
        .unwrap()
        .translation;
    assert_ne!(position, [-3., 0., 0.]);
    play.ui_input(Layer::ThreeD, [1280., 720.], Input::Focus("blend".into()))?;
    play.ui_input(Layer::ThreeD, [1280., 720.], Input::Adjust(1.))?;
    assert!(
        (play
            .app
            .world
            .resource::<animation::Runtime>()
            .unwrap()
            .players["animated-banner"]
            .parameters["Blend"]
            - 0.6)
            .abs()
            < 1e-5
    );
    play.ui_input(
        Layer::ThreeD,
        [1280., 720.],
        Input::ActivateObject("swedish".into()),
    )?;
    assert_eq!(
        play.instance()
            .ui_frame(&play.app.world, Layer::ThreeD, [1280., 720.])?
            .element("volume")
            .unwrap()
            .text,
        "Ljudvolym"
    );
    let save = play.instance().save_game_json(&play.app.world)?;
    for _ in 0..30 {
        play.app.step();
    }
    play.check_simulation()?;
    let expected = play
        .app
        .world
        .get::<Transform>(play.instance().entity("curve-cube").unwrap())
        .unwrap()
        .translation;
    play.with_instance(|instance, world| instance.load_game_json(world, &save))?;
    for _ in 0..30 {
        play.app.step();
    }
    play.check_simulation()?;
    assert_eq!(
        play.app
            .world
            .get::<Transform>(play.instance().entity("curve-cube").unwrap())
            .unwrap()
            .translation,
        expected
    );
    editor.stop_play();
    assert_eq!(editor.scene(), &authored);
    Ok(())
}
#[test]
fn two_d_authored_menus_pause_sprite_animation_and_restore_accessibility_preferences()
-> anyhow::Result<()> {
    let mut editor = Editor::open(&scene("ui-2d-lab"))?;
    editor.start_play()?;
    let play = editor.play.as_mut().unwrap();
    play.ui_input(Layer::TwoD, [1280., 720.], Input::Key("Enter".into()))?;
    assert_eq!(play.game_session().unwrap().phase, GamePhase::Playing);
    for _ in 0..20 {
        play.app.step();
    }
    play.check_simulation()?;
    let frame = play
        .app
        .world
        .resource::<sprite::Runtime>()
        .unwrap()
        .players["courier-sprite"]
        .frame;
    assert_ne!(frame, 0);
    play.ui_input(Layer::TwoD, [1280., 720.], Input::Key("Escape".into()))?;
    assert_eq!(play.game_session().unwrap().phase, GamePhase::Paused);
    for _ in 0..90 {
        play.app.step();
    }
    assert_eq!(
        play.app
            .world
            .resource::<sprite::Runtime>()
            .unwrap()
            .players["courier-sprite"]
            .frame,
        frame
    );
    play.ui_input(Layer::TwoD, [1280., 720.], Input::Key("Enter".into()))?;
    play.ui_input(
        Layer::TwoD,
        [1280., 720.],
        Input::Focus("text-scale".into()),
    )?;
    play.ui_input(Layer::TwoD, [1280., 720.], Input::Adjust(1.))?;
    assert!(
        play.app
            .world
            .resource::<ui::Preferences>()
            .unwrap()
            .text_scale
            .unwrap()
            > 1.
    );
    play.ui_input(Layer::TwoD, [1280., 720.], Input::Key("Escape".into()))?;
    play.ui_input(Layer::TwoD, [1280., 720.], Input::Key("R".into()))?;
    assert_eq!(play.game_session().unwrap().phase, GamePhase::Playing);
    assert!(
        play.app
            .world
            .resource::<ui::Preferences>()
            .unwrap()
            .text_scale
            .unwrap()
            > 1.
    );
    play.check_simulation()?;
    Ok(())
}
