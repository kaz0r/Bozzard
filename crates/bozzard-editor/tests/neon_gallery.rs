use bozzard_editor::Editor;
use bozzard_scene::{Layer, LightKind, Scene};

#[test]
fn gallery_loads_with_lights_effects_and_child_meshes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/neon-gallery.json");
    let mut editor = Editor::open(&path).unwrap();
    let scene = editor.scene().clone();
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    assert!(scene.display.bloom.enabled);
    for kind in [LightKind::Point, LightKind::Spot, LightKind::Directional] {
        assert!(
            scene
                .objects
                .iter()
                .any(|o| o.light.is_some_and(|l| l.kind == kind))
        );
    }
    for parent in ["normals", "checker", "toon"] {
        assert!(
            scene
                .objects
                .iter()
                .any(|o| o.parent.as_deref() == Some(parent) && o.drawable.is_some())
        );
    }
    assert!(
        !editor
            .render(Layer::ThreeD, 16. / 9.)
            .unwrap()
            .items
            .is_empty()
    );
    editor.start_play().unwrap();
    editor.render(Layer::ThreeD, 16. / 9.).unwrap();
    editor.stop_play();
    assert_eq!(editor.scene(), &scene);
}
