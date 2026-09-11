use bozzard_editor::Editor;
use bozzard_scene::{Layer, Texture};

#[test]
fn demo_effects_survive_authoring_save_and_play() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/shader-lab.json");
    let mut editor = Editor::open(&path).unwrap();
    let original = editor.scene().clone();
    let serialized = original.to_json().unwrap();
    assert_eq!(
        bozzard_scene::Scene::from_json(&serialized).unwrap(),
        original
    );
    let rendered = editor.render(Layer::ThreeD, 1.0).unwrap();
    assert!(
        rendered
            .items
            .iter()
            .any(|item| matches!(item.material.texture, bozzard_render::TextureKind::Normals))
    );
    assert!(rendered.items.iter().any(|item| matches!(
        item.material.texture,
        bozzard_render::TextureKind::ProceduralChecker
    )));
    assert!(
        rendered
            .items
            .iter()
            .any(|item| matches!(item.material.texture, bozzard_render::TextureKind::Toon))
    );
    let mut edited = original.clone();
    edited
        .objects
        .iter_mut()
        .find(|object| object.id == "normals")
        .unwrap()
        .drawable
        .as_mut()
        .unwrap()
        .texture = Texture::Toon;
    editor.apply("Change demo effect", edited.clone()).unwrap();
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &original);
    editor.redo().unwrap();
    assert_eq!(editor.scene(), &edited);
    editor.start_play().unwrap();
    assert!(
        editor
            .render(Layer::ThreeD, 1.0)
            .unwrap()
            .items
            .iter()
            .filter(|item| matches!(item.material.texture, bozzard_render::TextureKind::Toon))
            .count()
            >= 2
    );
    editor.stop_play();
    assert_eq!(editor.scene(), &edited);
    assert!(
        bozzard_scene::Scene::from_json(&serialized.replace("\"normals\"", "\"invalid_effect\""))
            .is_err()
    );
    // Legacy documents still use their ordinary texture without a schema migration.
    let legacy = bozzard_scene::Scene::from_json(include_str!(
        "../../../examples/demo/scenes/scene-lab.json"
    ))
    .unwrap();
    assert!(
        legacy
            .objects
            .iter()
            .filter_map(|object| object.drawable.as_ref())
            .all(|drawable| matches!(
                drawable.texture,
                Texture::White | Texture::Checker | Texture::Asset(_)
            ))
    );
}
