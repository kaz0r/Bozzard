use bozzard_editor::Editor;
use bozzard_scene::{AssetKind, AssetSource, Blueprint, Layer, Mesh, Prefab};
use std::{collections::BTreeMap, path::PathBuf};

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn project_delete_removes_users_restores_files_and_protects_overwrites_and_references() {
    let temp = Temp(std::env::temp_dir().join(format!("bozzard-delete-{}", std::process::id())));
    std::fs::create_dir(&temp.0).unwrap();
    let mut editor = Editor::new(
        bozzard_demo::scene_document().unwrap(),
        &temp.0.join("scene.json"),
    )
    .unwrap();
    let image = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/assets/palette.png");
    let id = editor.import(&image).unwrap();
    editor.add_asset_to_scene(&id).unwrap();
    let owner = editor.selected.clone().unwrap();
    editor.duplicate().unwrap();
    editor.create(Mesh::Cube, Layer::ThreeD).unwrap();
    let child = editor.selected.clone().unwrap();
    let mut scene = editor.scene().clone();
    scene
        .objects
        .iter_mut()
        .find(|o| o.id == child)
        .unwrap()
        .parent = Some(owner.clone());
    editor.apply("Parent child", scene).unwrap();
    let before = editor.scene().clone();
    let source = temp.0.join(&before.assets[&id].path);
    let bytes = std::fs::read(&source).unwrap();
    assert_eq!(editor.delete_project_asset(&id).unwrap(), 3);
    assert!(!source.exists() && !editor.scene().assets.contains_key(&id));
    assert_eq!(editor.scene().objects.len(), before.objects.len() - 3);
    editor.undo().unwrap();
    assert_eq!(editor.scene(), &before);
    assert_eq!(std::fs::read(&source).unwrap(), bytes);
    editor.redo().unwrap();
    let deleted = editor.scene().clone();
    std::fs::write(&source, b"new file must survive undo").unwrap();
    assert!(editor.undo().is_err());
    assert_eq!(editor.scene(), &deleted);
    assert_eq!(
        std::fs::read(&source).unwrap(),
        b"new file must survive undo"
    );
    std::fs::remove_file(&source).unwrap();
    editor.undo().unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), bytes);

    // An active camera prevents both scene mutation and the file move.
    let mut protected = before.clone();
    protected
        .objects
        .iter_mut()
        .find(|o| o.id == owner)
        .unwrap()
        .camera = Some(bozzard_scene::Camera::Orthographic {
        vertical_size: 4.,
        near: 0.1,
        far: 100.,
    });
    protected.views.insert(Layer::TwoD, owner);
    editor.apply("Protected camera", protected.clone()).unwrap();
    assert!(editor.delete_project_asset(&id).is_err());
    assert_eq!(editor.scene(), &protected);
    assert!(source.exists());
    editor.undo().unwrap();

    // Even an unplaced prefab may own an on-disk dependency: do not break it.
    let object = before
        .objects
        .iter()
        .find(|o| o.id == child)
        .unwrap()
        .clone();
    let mut object = object;
    object.parent = None;
    let prefab = Prefab {
        version: 1,
        name: "Shared".into(),
        root: object.id.clone(),
        objects: vec![object],
        assets: BTreeMap::from([(
            "image".into(),
            AssetSource {
                kind: AssetKind::Image,
                path: source.file_name().unwrap().to_str().unwrap().into(),
            },
        )]),
    };
    std::fs::write(
        temp.0.join("assets/shared.prefab.json"),
        prefab.to_json().unwrap(),
    )
    .unwrap();
    let mut shared = before.clone();
    shared.assets.insert(
        "shared".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "assets/shared.prefab.json".into(),
        },
    );
    editor.apply("Shared dependency", shared.clone()).unwrap();
    assert!(editor.delete_project_asset(&id).is_err());
    assert_eq!(editor.scene(), &shared);
    assert!(source.exists());

    // Linked external files cannot be removed from disk by project deletion.
    std::fs::copy(&source, temp.0.join("external.png")).unwrap();
    let mut external = editor.scene().clone();
    external.assets.insert(
        "external".into(),
        AssetSource {
            kind: AssetKind::Image,
            path: "external.png".into(),
        },
    );
    editor.apply("External link", external.clone()).unwrap();
    assert!(editor.delete_project_asset("external").is_err());
    assert_eq!(editor.scene(), &external);
    assert!(temp.0.join("external.png").exists());

    // A graph file is independent of embedded attachments and is itself undoable.
    let path = temp.0.join("assets/copy.blueprint.json");
    std::fs::write(&path, Blueprint::default().to_json().unwrap()).unwrap();
    assert_eq!(editor.delete_project_file(&path).unwrap(), 0);
    assert!(!path.exists());
    editor.undo().unwrap();
    assert!(path.exists());
    assert_eq!(editor.scene(), &external);
}
