use bozzard_editor::Editor;
use bozzard_scene::{
    Layer,
    blueprint::{BlackboardValue, Value},
};

#[test]
fn bozz_torio_scene_opens_edits_and_previews_in_the_editor() -> anyhow::Result<()> {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/bozz-torio/scene/bozz-torio.json");
    let root = std::env::temp_dir().join(format!(
        "bozz-torio-editor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join("scene"))?;
    std::fs::create_dir_all(root.join("assets"))?;
    let path = root.join("scene/bozz-torio.json");
    std::fs::copy(&source, &path)?;
    for asset in std::fs::read_dir(source.parent().unwrap().join("../assets"))? {
        let asset = asset?;
        if asset.file_type()?.is_file() {
            std::fs::copy(asset.path(), root.join("assets").join(asset.file_name()))?;
        }
    }
    let mut editor = Editor::open(&path)?;
    let original = editor.scene().clone();
    assert_eq!(original.name, "Bozz-torio — Factory Floor");
    assert_eq!(
        original.views.get(&Layer::TwoD).map(String::as_str),
        Some("camera")
    );
    assert!(
        original
            .objects
            .iter()
            .any(|object| object.id == "factory-floor")
    );

    let mut changed = original.clone();
    changed
        .objects
        .iter_mut()
        .find(|object| object.id == "delivery-hub")
        .unwrap()
        .transform
        .translation[0] += 1.0;
    changed.blackboard.insert(
        "world_seed".into(),
        BlackboardValue::Scalar(Value::Number(42.0)),
    );
    editor.apply("Move delivery hub", changed.clone())?;
    assert_eq!(editor.scene(), &changed);
    editor.undo()?;
    assert_eq!(editor.scene(), &original);
    editor.redo()?;
    editor.save(&path)?;
    assert_eq!(Editor::open(&path)?.scene(), &changed);

    editor.start_play()?;
    editor.play.as_mut().unwrap().app.step();
    let view = editor.play.as_ref().unwrap().instance().view(
        &editor.play.as_ref().unwrap().app.world,
        Layer::TwoD,
        16.0 / 9.0,
    )?;
    assert!(!view.sprites.is_empty());
    editor.stop_play();
    assert_eq!(editor.scene(), &changed);
    std::fs::remove_dir_all(root)?;
    Ok(())
}
