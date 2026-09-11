use bozzard_editor::Editor;
use bozzard_scene::Layer;

#[test]
fn fog_history_play_save_and_2d_isolation() {
    let path = std::env::temp_dir().join(format!("bozzard-fog-{}.json", std::process::id()));
    let mut e = Editor::new(bozzard_demo::scene_document().unwrap(), &path).unwrap();
    let initial = e.scene().clone();
    e.begin_gesture("Fog slider");
    for density in [0.1, 0.2] {
        let mut scene = e.scene().clone();
        scene.fog.enabled = true;
        scene.fog.distance_density = density;
        scene.fog.height_density = 0.3;
        e.apply("Fog slider", scene).unwrap();
    }
    e.finish_gesture();
    let authored = e.scene().clone();
    let view = e.render(Layer::ThreeD, 1.).unwrap();
    assert!(view.fog.enabled);
    assert_eq!(view.fog.distance_density, 0.2);
    assert_eq!(view.fog.height_density, 0.3);
    assert!(!e.render(Layer::TwoD, 1.).unwrap().fog.enabled);
    e.undo().unwrap();
    assert_eq!(e.scene(), &initial);
    e.redo().unwrap();
    assert_eq!(e.scene(), &authored);
    e.start_play().unwrap();
    assert!(e.render(Layer::ThreeD, 1.).unwrap().fog.enabled);
    e.save(&path).unwrap();
    assert_eq!(Editor::open(&path).unwrap().scene(), &authored);
    e.stop_play();
    assert_eq!(e.scene(), &authored);
    let mut reset = e.scene().clone();
    reset.fog = Default::default();
    e.apply("Reset fog", reset).unwrap();
    assert!(!e.render(Layer::ThreeD, 1.).unwrap().fog.enabled);
    e.undo().unwrap();
    assert_eq!(e.scene(), &authored);
    let mut invalid = e.scene().clone();
    invalid.fog.height_density = f32::NAN;
    assert!(e.apply("Invalid fog", invalid).is_err());
    assert_eq!(e.scene(), &authored);
    std::fs::remove_file(path).unwrap();
}
