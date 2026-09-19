use bozzard_assets::{
    AssetData, ImageData,
    job::Progress,
    texture::{self, Compression},
};
use bozzard_editor::Editor;
use bozzard_scene::Scene;

#[test]
fn cooked_model_import_history_save_and_reload_preserve_skin_and_surface_identities()
-> anyhow::Result<()> {
    use bozzard_assets::{CookSource, cooked_model};
    use bozzard_scene::AssetKind;
    let root = std::env::temp_dir().join(format!(
        "bozzard-editor-cooked-model-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root)?;
    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/assets");
    for name in ["courier.glb", "animated-banner.gltf"] {
        let bytes = CookSource::read(AssetKind::Mesh, &fixtures.join(name), &Progress::default())?
            .cook(
                &[Compression::Bc3, Compression::Astc4x4],
                &Progress::default(),
            )?;
        let original = cooked_model::decode(&bytes)?;
        let source = root.join("input.bmesh");
        std::fs::write(&source, &bytes)?;
        let project = root.join(name);
        std::fs::create_dir(&project)?;
        let scene_path = project.join("scene.json");
        let scene = Scene::from_json(
            r#"{"version":1,"name":"Cooked model","views":{},"objects":[{
            "id":"mesh","name":"Mesh","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}
        }]}"#,
        )?;
        let mut editor = Editor::new(scene, &scene_path)?;
        let id = editor.import(&source)?;
        editor.undo()?;
        assert!(!editor.scene().assets.contains_key(&id));
        editor.redo()?;
        editor.select_object(Some("mesh".into()));
        editor.assign_asset_to_selected(&id)?;
        let assigned = editor.scene().clone();
        editor.undo()?;
        assert_ne!(editor.scene().objects, assigned.objects);
        editor.redo()?;
        assert_eq!(editor.scene().objects, assigned.objects);
        editor.save(&scene_path)?;
        std::fs::remove_file(source)?;
        let mut opened = Editor::open(&scene_path)?;
        assert_eq!(opened.scene(), &assigned);
        let handle = opened.assets.handle(&id).unwrap();
        let previous = opened.assets.get(handle).unwrap().shared_data().unwrap();
        let AssetData::Mesh(loaded) = previous.as_ref() else {
            panic!()
        };
        assert_eq!(loaded.vertices, original.vertices);
        assert_eq!(loaded.indices, original.indices);
        assert_eq!(
            loaded
                .parts
                .iter()
                .map(|p| &p.source_key)
                .collect::<Vec<_>>(),
            original
                .parts
                .iter()
                .map(|p| &p.source_key)
                .collect::<Vec<_>>()
        );
        if let Some(skin) = &original.skin {
            let restored = loaded.skin.as_ref().unwrap();
            assert_eq!(restored.rig, skin.rig);
            assert_eq!(restored.vertices, skin.vertices);
            assert_eq!(restored.rig.sample(0, 0.5)?, skin.rig.sample(0, 0.5)?);
        }
        std::fs::write(
            project.join(&opened.scene().assets[&id].path),
            b"broken cooked model",
        )?;
        opened.assets.refresh();
        assert!(opened.assets.require_ready().is_err());
        assert!(std::sync::Arc::ptr_eq(
            &previous,
            &opened.assets.get(handle).unwrap().shared_data().unwrap()
        ));
    }
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn cooked_texture_import_assignment_history_save_and_last_good_reload() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("bozzard-editor-cooked-{}", std::process::id()));
    std::fs::create_dir(&root)?;
    let image = ImageData {
        width: 16,
        height: 16,
        rgba: [40, 80, 160, 255].repeat(16 * 16),
        compressed: None,
    };
    let cooked = texture::cook(
        &image,
        &[Compression::Bc3, Compression::Astc4x4],
        &[true],
        &Progress::default(),
    )?;
    let bytes = texture::encode(&image, &cooked)?;
    let source = root.join("input.btex");
    std::fs::write(&source, &bytes)?;
    let project = root.join("project");
    std::fs::create_dir(&project)?;
    let scene_path = project.join("scene.json");
    let scene = Scene::from_json(
        r#"{
        "version":1,"name":"Cooked image","views":{},"objects":[{
            "id":"quad","name":"Quad","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "drawable":{"layer":"3d","mesh":"quad","texture":"white","color":[1,1,1],"uv_scale":[1,1]}
        }] }"#,
    )?;
    let mut editor = Editor::new(scene, &scene_path)?;
    let id = editor.import(&source)?;
    assert!(editor.scene().assets.contains_key(&id));
    editor.undo()?;
    assert!(!editor.scene().assets.contains_key(&id));
    editor.redo()?;
    assert!(editor.scene().assets.contains_key(&id));
    editor.select_object(Some("quad".into()));
    editor.assign_asset_to_selected(&id)?;
    let assigned = editor.scene().clone();
    editor.undo()?;
    assert_ne!(editor.scene().objects, assigned.objects);
    editor.redo()?;
    assert_eq!(editor.scene().objects, assigned.objects);
    editor.save(&scene_path)?;
    std::fs::remove_file(source)?;
    let mut opened = Editor::open(&scene_path)?;
    assert_eq!(opened.scene(), &assigned);
    let path = project.join(&opened.scene().assets[&id].path);
    assert_eq!(std::fs::read(&path)?, bytes);
    let handle = opened.assets.handle(&id).unwrap();
    let entry = opened.assets.get(handle).unwrap();
    assert!(
        matches!(entry.data(), Some(AssetData::Image(i)) if i.compressed.is_some() && i.rgba == image.rgba)
    );
    let previous = entry.shared_data().unwrap();
    std::fs::write(path, b"corrupt cooked file")?;
    opened.assets.refresh();
    assert!(opened.assets.require_ready().is_err());
    assert!(std::sync::Arc::ptr_eq(
        &previous,
        &opened.assets.get(handle).unwrap().shared_data().unwrap()
    ));
    std::fs::remove_dir_all(root)?;
    Ok(())
}
