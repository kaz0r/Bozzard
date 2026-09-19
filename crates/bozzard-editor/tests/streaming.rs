use bozzard_editor::Editor;
use bozzard_scene::{
    Scene,
    scene_loading::{LoadPhase, SceneSource},
};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
fn play_adopts_streamed_assets_and_stop_restores_the_authoring_catalog() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("bozzard-editor-stream-{}", std::process::id()));
    fs::create_dir(&root)?;
    let result = (|| -> anyhow::Result<()> {
        let mut main =
            Scene::from_json(r#"{"version":1,"name":"Editor stream","views":{},"objects":[]}"#)?;
        main.runtime_scene_sources.insert(
            "next".into(),
            SceneSource::File {
                path: "next.json".into(),
            },
        );
        let next = Scene::from_json(
            r#"{"version":1,"name":"Next","views":{},"objects":[],"assets":{"streamed-image":{"kind":"image","path":"image.png"}}}"#,
        )?;
        fs::write(root.join("next.json"), next.to_json()?)?;
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/demo/scenes/assets/middleware-panel.png"),
            root.join("image.png"),
        )?;
        let mut editor = Editor::new(main.clone(), &root.join("main.json"))?;
        let source = main.runtime_scene_sources["next"].clone();
        editor.set_runtime_scene_source("next", None)?;
        assert!(editor.scene().runtime_scene_sources.is_empty());
        editor.undo()?;
        assert_eq!(editor.scene(), &main);
        editor.redo()?;
        editor.set_runtime_scene_source("next", Some(source))?;
        assert_eq!(editor.scene(), &main);
        assert!(
            editor
                .set_runtime_scene_source(
                    "bad",
                    Some(SceneSource::File {
                        path: "/absolute.json".into()
                    })
                )
                .is_err()
        );
        editor.start_play()?;
        editor
            .play
            .as_mut()
            .unwrap()
            .with_instance(|instance, world| instance.begin_scene_load(world, "next", true))?;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            editor.advance(Duration::from_secs_f64(1.0 / 60.0));
            let play = editor.play.as_ref().unwrap();
            play.check_simulation()?;
            let status = play.instance().scene_load_status(&play.app.world);
            assert_ne!(status.phase, LoadPhase::Failed, "{}", status.error);
            if status.phase == LoadPhase::Loaded {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(editor.assets.handle("streamed-image").is_some());
        assert_eq!(editor.scene(), &main);
        assert!(!editor.dirty());
        editor.stop_play();
        assert!(editor.assets.handle("streamed-image").is_none());
        assert_eq!(editor.scene(), &main);
        assert!(!editor.dirty());
        Ok(())
    })();
    fs::remove_dir_all(root)?;
    result
}
