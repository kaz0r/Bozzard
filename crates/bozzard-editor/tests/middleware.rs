use bozzard_editor::{Editor, OpenScenes};
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
fn timeline_scrub_samples_in_edit_world_without_changing_document() -> anyhow::Result<()> {
    use bozzard_scene::{
        Scene,
        blueprint::ObjectRef,
        middleware::{
            curve::Curve,
            registry,
            timeline::{Marker, Timeline},
            tween::{Property, Track},
        },
    };
    use std::sync::Arc;
    let mut document = Scene::from_json(
        r#"{"version":1,"name":"Scrub","views":{},"objects":[
        {"id":"director","name":"Director","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
        {"id":"target","name":"Target","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#,
    )?;
    let mut track = Track::new(Property::Translation);
    track.target = ObjectRef::Id("target".into());
    track.channels[0] = Curve::linear(0., 10., 2.);
    let mut timeline = Timeline::default();
    timeline.motion.duration = 2.;
    timeline.motion.tracks = Arc::new(vec![track]);
    timeline.markers = Arc::new(vec![Marker {
        time: 1.,
        name: "Never fire in Edit".into(),
    }]);
    registry::set(&mut document.objects[0], &timeline)?;
    let editor = Editor::new(document.clone(), &scene("unused-scrub-test"))?;
    assert_eq!(
        editor
            .timeline_preview_transform("target")?
            .unwrap()
            .translation[0],
        0.
    );
    editor.scrub_timeline_preview("director", 1.)?;
    assert_eq!(
        editor
            .timeline_preview_transform("target")?
            .unwrap()
            .translation[0],
        5.
    );
    assert_eq!(editor.scene(), &document);
    assert!(!editor.dirty());
    editor.clear_timeline_preview();
    assert_eq!(
        editor
            .timeline_preview_transform("target")?
            .unwrap()
            .translation[0],
        0.
    );
    Ok(())
}

#[test]
fn ui_widget_layout_gesture_undo_save_and_reopen() -> anyhow::Result<()> {
    use bozzard_scene::middleware::{registry, ui::Widget};
    let mut editor = Editor::open(&scene("ui-2d-lab"))?;
    let id = editor
        .scene()
        .objects
        .iter()
        .find(|o| o.extras.contains_key("ui_widget"))
        .unwrap()
        .id
        .clone();
    editor.selected = Some(id.clone());
    let original = editor.scene().clone();
    editor.begin_gesture("Edit UI widget layout");
    let mut updated = original.clone();
    let widget = updated.objects.iter_mut().find(|o| o.id == id).unwrap();
    let mut value = registry::get::<Widget>(widget)?.unwrap();
    value.anchors.offset[0] += 20.;
    value.anchors.size[0] += 30.;
    registry::set(widget, &value)?;
    editor.apply("Edit UI widget layout", updated.clone())?;
    editor.finish_gesture();
    assert_eq!(editor.scene(), &updated);
    editor.undo()?;
    assert_eq!(editor.scene(), &original);
    editor.redo()?;
    assert_eq!(editor.scene(), &updated);
    for size in [[1280., 720.], [1440., 900.], [480., 800.]] {
        assert_eq!(editor.ui_frame(Layer::TwoD, size)?.size, size);
    }
    assert_eq!(
        editor.scene(),
        &updated,
        "preview dimensions must not enter authored data"
    );
    let temp = std::env::temp_dir().join(format!(
        "bozzard-ui-layout-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir(&temp)?;
    let destination = temp.join("scene.json");
    editor.save(&destination)?;
    let reopened = Editor::open(&destination)?;
    assert_eq!(reopened.scene(), editor.scene());
    std::fs::remove_dir_all(temp)?;
    Ok(())
}

#[test]
fn every_middleware_object_can_be_hidden_and_restored_without_changing_the_source()
-> anyhow::Result<()> {
    for name in ["middleware-lab", "ui-2d-lab"] {
        let editor = Editor::open(&scene(name))?;
        let authored = editor.scene().clone();
        let mut open = OpenScenes::default();
        for object in &authored.objects {
            open.set_object_visible(open.active(), &object.id, false);
            open.sync_view(&editor)?;
            let view = open.view(&editor);
            for &layer in authored.views.keys() {
                view.render(layer, 16. / 9.)?;
                view.ui_frame(layer, [1280., 720.])?;
            }
            open.set_object_visible(open.active(), &object.id, true);
            open.sync_view(&editor)?;
            assert_eq!(open.view(&editor).scene(), &authored);
        }
        assert_eq!(editor.scene(), &authored);
        assert!(!editor.dirty());
    }
    Ok(())
}
#[test]
fn play_refreshes_audio_lengths_when_only_runtime_scenes_have_sources() -> anyhow::Result<()> {
    use bozzard_scene::middleware::registry;
    let document = bozzard_scene::Scene::from_json(
        r#"{"version":1,"name":"Silent entry scene","views":{},"objects":[],
        "assets":{"chime":{"kind":"audio","path":"assets/middleware-chime.wav"}},
        "runtime_scenes":{"level":{"version":1,"name":"Audio level","views":{},
        "assets":{"chime":{"kind":"audio","path":"assets/middleware-chime.wav"}},
        "objects":[{"id":"sound","name":"Sound","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"audio_source":{"asset":"chime","duration":0.25}}]}}}"#,
    )?;
    let mut editor = Editor::new(document.clone(), &scene("audio-library-test"))?;
    // A stale authored clip length can also arrive in a refreshed runtime scene template.
    editor.apply("Replace runtime scene template", document.clone())?;
    editor.start_play()?;
    let level = &editor
        .play
        .as_ref()
        .unwrap()
        .instance()
        .document()
        .runtime_scenes["level"];
    let source = registry::get::<audio::AudioSource>(&level.objects[0])?.unwrap();
    assert!(
        (source.duration - 1.).abs() < 1e-6,
        "stale runtime-scene audio duration: {}",
        source.duration
    );
    editor.stop_play();
    editor.undo()?;
    assert_eq!(editor.scene(), &document);
    Ok(())
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
