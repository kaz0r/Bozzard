use bozzard_editor::{Editor, OpenScenes};
use bozzard_scene::{Layer, Mesh};
use std::path::Path;

fn document(name: &str) -> Editor {
    let mut scene = bozzard_demo::scene_document().unwrap();
    scene.name = name.into();
    Editor::new(scene, Path::new(name)).unwrap()
}

#[test]
fn switching_documents_retains_independent_edits_selections_and_history() {
    let mut active = document("first.json");
    let first = active.scene().clone();
    active.create(Mesh::Cube, Layer::ThreeD).unwrap();
    let first_selection = active.selected.clone();
    let first_edited = active.scene().clone();
    let mut workspace = OpenScenes::default();
    let second_id = workspace.add(&mut active, document("second.json")).unwrap();
    let second = active.scene().clone();
    active.create_empty().unwrap();
    let second_selection = active.selected.clone();
    let second_edited = active.scene().clone();

    workspace.activate(&mut active, 0).unwrap();
    assert_eq!(*active.scene(), first_edited);
    assert_eq!(active.selected, first_selection);
    active.undo().unwrap();
    assert_eq!(*active.scene(), first);
    workspace.activate(&mut active, second_id).unwrap();
    assert_eq!(*active.scene(), second_edited);
    assert_eq!(active.selected, second_selection);
    active.undo().unwrap();
    assert_eq!(*active.scene(), second);
    assert!(!workspace.any_dirty(&active));
    workspace.activate(&mut active, 0).unwrap();
    active.redo().unwrap();
    assert_eq!(*active.scene(), first_edited);
    assert!(workspace.any_dirty(&active));
}

#[test]
fn rejected_close_or_switch_does_not_discard_a_document_or_active_gesture() {
    let mut active = document("first.json");
    let mut workspace = OpenScenes::default();
    assert!(workspace.close(&mut active, 0).is_err());
    workspace.add(&mut active, document("second.json")).unwrap();
    active.begin_gesture("Create");
    active.create_empty().unwrap();
    let before = active.scene().clone();
    let revision = workspace.revision();
    assert!(workspace.close(&mut active, workspace.active()).is_err());
    assert!(workspace.activate(&mut active, 99).is_err());
    assert_eq!(workspace.revision(), revision);
    assert_eq!(*active.scene(), before);
    active.undo().unwrap();
    workspace.close(&mut active, workspace.active()).unwrap();
    assert_eq!(workspace.active(), 0);
    assert_eq!(workspace.len(), 1);
    assert_eq!(active.path, Path::new("first.json"));
}

#[test]
fn duplicate_open_and_play_switches_are_rejected_before_changing_session_state() {
    let mut active = document("first.json");
    let mut workspace = OpenScenes::default();
    assert!(workspace.add(&mut active, document("first.json")).is_err());
    let second = workspace.add(&mut active, document("second.json")).unwrap();
    active.start_play().unwrap();
    assert!(workspace.activate(&mut active, 0).is_err());
    assert!(workspace.close(&mut active, 0).is_err());
    assert!(workspace.add(&mut active, document("third.json")).is_err());
    assert_eq!(workspace.active(), second);
    assert_eq!(workspace.len(), 2);
    assert!(active.play.is_some());
}

#[test]
fn saving_one_document_leaves_the_other_dirty_and_discard_is_local() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("bozzard-multiple-save-{}", std::process::id()));
    std::fs::create_dir(&root)?;
    let result = (|| -> anyhow::Result<()> {
        let mut first = document("first.json");
        first.save(&root.join("first.json"))?;
        first.create_empty()?;
        let edited_first = first.scene().clone();
        let mut workspace = OpenScenes::default();
        let second = workspace.add(
            &mut first,
            Editor::new(bozzard_demo::scene_document()?, &root.join("second.json"))?,
        )?;
        first.create_empty()?;
        let edited_second = first.scene().clone();
        first.save(&root.join("second.json"))?;
        assert_eq!(
            Editor::open(&root.join("second.json"))?.scene(),
            &edited_second
        );
        assert!(workspace.any_dirty(&first));
        assert_eq!(
            workspace.document(&first, 0).unwrap().scene(),
            &edited_first
        );
        workspace.activate(&mut first, 0)?;
        first.save(&root.join("first.json"))?;
        assert!(!workspace.any_dirty(&first));
        assert_eq!(
            Editor::open(&root.join("first.json"))?.scene(),
            &edited_first
        );
        first.create_empty()?;
        workspace.discard_and_close(&mut first, 0)?;
        assert_eq!(workspace.active(), second);
        assert_eq!(first.scene(), &edited_second);
        assert_eq!(
            Editor::open(&root.join("first.json"))?.scene(),
            &edited_first
        );
        Ok(())
    })();
    std::fs::remove_dir_all(root)?;
    result
}

#[cfg(unix)]
#[test]
fn path_aliases_cannot_open_or_overwrite_another_document() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("bozzard-document-alias-{}", std::process::id()));
    std::fs::create_dir(&root)?;
    let result = (|| -> anyhow::Result<()> {
        let path = root.join("scene.json");
        let mut first = document("scene.json");
        first.save(&path)?;
        std::os::unix::fs::symlink(&path, root.join("alias.json"))?;
        let mut workspace = OpenScenes::default();
        assert!(
            workspace
                .add(&mut first, Editor::open(&root.join("alias.json"))?)
                .is_err()
        );
        workspace.add(&mut first, document("other.json"))?;
        assert!(
            workspace
                .validate_save_path(&first, &root.join("alias.json"))
                .is_err()
        );
        assert!(
            workspace
                .validate_save_path(&first, &root.join("./scene.json"))
                .is_err()
        );
        Ok(())
    })();
    std::fs::remove_dir_all(root)?;
    result
}

#[test]
fn combined_view_picks_scene_owners_and_retains_independent_saved_documents() -> anyhow::Result<()>
{
    use bozzard_scene::Scene;
    use glam::Vec3;
    let mut base = Scene::from_json(
        r#"{"version":1,"name":"Base","views":{},"objects":[
      {"id":"camera","name":"Camera","transform":{"translation":[0,0,8],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
      {"id":"box","name":"Box","transform":{"translation":[-2,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,0,0],"uv_scale":[1,1]}}
    ]}"#,
    )?;
    base.objects[0].camera = Some(bozzard_scene::Camera::Perspective {
        vertical_fov_degrees: 60.,
        near: 0.1,
        far: 100.,
    });
    base.views.insert(Layer::ThreeD, "camera".into());
    let mut second = base.clone();
    second.name = "Chunk".into();
    second.views.clear();
    second.objects.retain(|o| o.camera.is_none());
    second.objects[0].transform.translation[0] = 2.;
    let mut active = Editor::new(base.clone(), Path::new("base.json"))?;
    let mut workspace = OpenScenes::default();
    let chunk = workspace.add(
        &mut active,
        Editor::new(second.clone(), Path::new("chunk.json"))?,
    )?;
    workspace.sync_view(&active)?;
    let view = workspace.view(&active);
    let revision = view.revision();
    let projection = view.render(Layer::ThreeD, 1.)?.view_projection;
    assert_eq!(view.scene().objects.len(), 3);
    for (x, owner) in [(-2., 0), (2., chunk)] {
        let ndc = projection.project_point3(Vec3::new(x, 0., 0.));
        let pick = view
            .pick_surface_with_projection(Layer::ThreeD, projection, [ndc.x, ndc.y])?
            .unwrap();
        let (id, local) = workspace.owner(pick).unwrap();
        assert_eq!(id, owner);
        assert_eq!(local.object, "box");
    }
    workspace.sync_view(&active)?;
    assert_eq!(workspace.view(&active).revision(), revision);
    active.select_object(Some("box".into()));
    active.create_empty()?;
    workspace.sync_view(&active)?;
    assert!(workspace.view(&active).revision() > revision);
    workspace.set_visible(0, false);
    workspace.sync_view(&active)?;
    // A hidden scene can still supply the inspection camera.
    let projection = workspace
        .view(&active)
        .render(Layer::ThreeD, 1.)?
        .view_projection;
    let ndc = projection.project_point3(Vec3::new(-2., 0., 0.));
    assert!(
        workspace
            .view(&active)
            .pick_surface_with_projection(Layer::ThreeD, projection, [ndc.x, ndc.y])?
            .is_none()
    );
    assert_eq!(workspace.document(&active, 0).unwrap().scene(), &base);
    active.undo()?;
    assert_eq!(active.scene(), &second);
    assert!(!workspace.any_dirty(&active));
    Ok(())
}

#[test]
fn overlapping_asset_names_share_decoded_data_without_reopening_files() -> anyhow::Result<()> {
    use bozzard_scene::{AssetKind, AssetSource, Scene};
    use std::sync::Arc;
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes");
    let mut scene = Scene::from_json(r#"{"version":1,"name":"Assets","views":{},"objects":[]}"#)?;
    scene.assets.insert(
        "image".into(),
        AssetSource {
            kind: AssetKind::Image,
            path: "assets/middleware-panel.png".into(),
        },
    );
    let mut active = Editor::new(scene.clone(), &root.join("base.json"))?;
    let first = active
        .assets
        .get(active.assets.handle("image").unwrap())
        .unwrap()
        .shared_data()
        .unwrap();
    let mut workspace = OpenScenes::default();
    workspace.add(&mut active, Editor::new(scene, &root.join("chunk.json"))?)?;
    let second = active
        .assets
        .get(active.assets.handle("image").unwrap())
        .unwrap()
        .shared_data()
        .unwrap();
    workspace.sync_view(&active)?;
    let shared = &workspace.view(&active).assets;
    assert_eq!(shared.entries().count(), 2);
    assert!(Arc::ptr_eq(
        &first,
        &shared
            .get(shared.handle("document-0-image").unwrap())
            .unwrap()
            .shared_data()
            .unwrap()
    ));
    assert!(Arc::ptr_eq(
        &second,
        &shared
            .get(shared.handle("document-1-image").unwrap())
            .unwrap()
            .shared_data()
            .unwrap()
    ));
    workspace.sync_view(&active)?;
    assert!(
        workspace
            .validate_save_path(&active, &root.join("base.json"))
            .is_err()
    );
    assert!(
        workspace
            .validate_save_path(&active, &root.join("chunk.json"))
            .is_ok()
    );
    Ok(())
}
