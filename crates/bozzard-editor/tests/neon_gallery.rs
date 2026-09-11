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

#[test]
fn changing_shadowed_local_light_to_directional_preserves_settings_and_history() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/scene-lab.json");
    for kind in [LightKind::Point, LightKind::Spot] {
        let mut editor = Editor::open(&path).unwrap();
        editor.create_light(kind).unwrap();
        let id = editor.selected.clone().unwrap();
        let mut scene = editor.scene().clone();
        let light = scene
            .objects
            .iter_mut()
            .find(|o| o.id == id)
            .unwrap()
            .light
            .as_mut()
            .unwrap();
        light.shadows = true;
        light.shadow_bias = 0.03;
        editor.apply("Enable local shadows", scene).unwrap();
        assert!(
            editor.render(Layer::ThreeD, 1.).unwrap().lights[0]
                .shadows
                .is_some()
        );
        let local = editor.scene().clone();
        let mut directional = local.clone();
        directional
            .objects
            .iter_mut()
            .find(|o| o.id == id)
            .unwrap()
            .light
            .as_mut()
            .unwrap()
            .kind = LightKind::Directional;
        editor
            .apply("Switch to directional", directional.clone())
            .unwrap();
        let render = editor.render(Layer::ThreeD, 1.).unwrap();
        assert!(render.lights[0].directional);
        assert!(render.lights[0].shadows.is_none());
        render.lights[0].validate().unwrap();
        assert_eq!(
            Scene::from_json(&directional.to_json().unwrap()).unwrap(),
            directional
        );
        editor.start_play().unwrap();
        assert!(
            editor.render(Layer::ThreeD, 1.).unwrap().lights[0]
                .shadows
                .is_none()
        );
        editor.stop_play();
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &local);
        assert_eq!(
            editor.render(Layer::ThreeD, 1.).unwrap().lights[0]
                .shadows
                .unwrap()
                .bias,
            0.03
        );
        editor.redo().unwrap();
        assert_eq!(editor.scene(), &directional);
        assert!(
            editor.render(Layer::ThreeD, 1.).unwrap().lights[0]
                .shadows
                .is_none()
        );
    }
}
