use bozzard_editor::{Editor, ScriptReloadFeedback};
use bozzard_scene::Transform;
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn spinner_rotation(editor: &Editor) -> f32 {
    let play = editor.play.as_ref().unwrap();
    let spinner = play.instance().entity("spinner").unwrap();
    play.app
        .world
        .get::<Transform>(spinner)
        .unwrap()
        .rotation_degrees[1]
}

fn await_reload(editor: &mut Editor) -> ScriptReloadFeedback {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        editor.advance(Duration::ZERO);
        match editor.script_reload_feedback("spin").unwrap() {
            ScriptReloadFeedback::Compiling { .. } => {
                assert!(Instant::now() < deadline, "script reload timed out");
                std::thread::yield_now();
            }
            feedback => return feedback.clone(),
        }
    }
}

#[test]
fn script_lab_edit_changes_running_play_and_bad_edit_keeps_last_program() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/script-lab.json");
    let mut editor = Editor::open(&path).unwrap();
    let authored = editor.scene().clone();
    editor.start_play().unwrap();
    editor.advance(Duration::from_secs_f64(1. / 60.));
    let before = spinner_rotation(&editor);

    let original = std::fs::read_to_string(path.parent().unwrap().join("scripts/spin.rs")).unwrap();
    editor
        .request_script_reload("spin", original.replace("45.0", "90.0"))
        .unwrap();
    assert!(matches!(
        await_reload(&mut editor),
        ScriptReloadFeedback::Applied { .. }
    ));
    editor.advance(Duration::from_secs_f64(1. / 60.));
    let changed = spinner_rotation(&editor);
    assert!((changed - before - 1.5).abs() < 0.01);

    editor
        .request_script_reload("spin", "fn on_update(me) {}".into())
        .unwrap();
    match await_reload(&mut editor) {
        ScriptReloadFeedback::Failed { message } => {
            assert!(
                message.contains("spin") && message.contains("line 1"),
                "{message}"
            );
        }
        feedback => panic!("expected script diagnostic, got {feedback:?}"),
    }
    editor.advance(Duration::from_secs_f64(1. / 60.));
    assert!((spinner_rotation(&editor) - changed - 1.5).abs() < 0.01);
    editor.stop_play();
    assert_eq!(editor.scene(), &authored);
}
