use bozzard_editor::Editor;
use bozzard_scene::{DisplayPreset, DisplaySettings, Layer, PostProcessVolume};
#[test]
fn post_processing_history_save_play_and_2d_isolation() {
    let path = std::env::temp_dir().join(format!("bozzard-post-{}.json", std::process::id()));
    let mut editor = Editor::new(bozzard_demo::scene_document().unwrap(), &path).unwrap();
    let initial = editor.scene().clone();
    editor.begin_gesture("Grade");
    for preset in [DisplayPreset::Cinematic, DisplayPreset::Bonfire] {
        let mut scene = editor.scene().clone();
        scene.display = DisplaySettings::preset(preset);
        scene.post_process_volumes = vec![PostProcessVolume {
            center: [1000.; 3],
            ..Default::default()
        }];
        editor.apply("Grade", scene).unwrap();
    }
    editor.finish_gesture();
    let authored = editor.scene().clone();
    let view = editor.render(Layer::ThreeD, 1.).unwrap();
    assert_eq!(view.display.tone_mapper, bozzard_render::ToneMapper::Filmic);
    assert!(view.display.heat_distortion.enabled && view.display.ambient_occlusion.enabled);
    assert_eq!(
        editor.render(Layer::TwoD, 1.).unwrap().display,
        bozzard_render::DisplaySettings {
            tone_mapping: false,
            ..Default::default()
        }
    );
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &initial);
    editor.redo().unwrap();
    assert_eq!(editor.scene(), &authored);
    editor.start_play().unwrap();
    editor.save(&path).unwrap();
    assert_eq!(Editor::open(&path).unwrap().scene(), &authored);
    editor.stop_play();
    assert_eq!(editor.scene(), &authored);
    let mut invalid = authored.clone();
    invalid.display.heat_distortion.strength = -1.;
    assert!(editor.apply("Invalid", invalid).is_err());
    assert_eq!(editor.scene(), &authored);
    let mut reset = authored.clone();
    reset.display = Default::default();
    editor.apply("Reset", reset).unwrap();
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &authored);
    std::fs::remove_file(path).unwrap();
}
