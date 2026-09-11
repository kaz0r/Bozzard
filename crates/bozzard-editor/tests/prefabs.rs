use bozzard_assets::job::Job;
use bozzard_editor::{Editor, PrefabCommand};
use bozzard_scene::{AssetKind, Layer, Mesh, Prefab, Scene};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let p = std::env::temp_dir().join(format!(
                "bozzard-prefabs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&p) {
                Ok(()) => return Self(p),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => panic!("{e}"),
            }
        }
    }
    fn editor(&self) -> Editor {
        let scene = Scene::from_json(r#"{"version":1,"name":"Prefab tests","views":{},"objects":[
            {"id":"root","name":"Lamp","transform":{"translation":[3,2,1],"rotation_degrees":[0,15,0],"scale":[1,1,1]},
             "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[0.2,0.8,0.7],"uv_scale":[1,1]}},
            {"id":"child","name":"Bulb","parent":"root","transform":{"translation":[0,2,0],"rotation_degrees":[0,0,0],"scale":[0.5,0.5,0.5]},
             "light":{"kind":"point","intensity":10}}
        ]}"#).unwrap();
        let mut e = Editor::new(scene, &self.0.join("scene.json")).unwrap();
        e.selected = Some("root".into());
        e
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn wait<T: Send + 'static>(job: &Job<T>) -> anyhow::Result<T> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(value) = job.poll() {
            return value;
        }
        assert!(Instant::now() < deadline, "prefab worker timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn run(e: &mut Editor, command: PrefabCommand) -> String {
    let job = e.prefab_job(command).unwrap();
    let prepared = wait(&job).unwrap();
    e.accept_prefab(prepared).unwrap()
}
fn edit(e: &mut Editor, id: &str, f: impl FnOnce(&mut bozzard_scene::Object)) {
    let mut scene = e.scene().clone();
    f(scene.objects.iter_mut().find(|o| o.id == id).unwrap());
    e.apply("Test edit", scene).unwrap();
}
fn object<'a>(e: &'a Editor, id: &str) -> &'a bozzard_scene::Object {
    e.scene().objects.iter().find(|o| o.id == id).unwrap()
}
fn source_path(e: &Editor, asset: &str) -> PathBuf {
    e.path.parent().unwrap().join(&e.scene().assets[asset].path)
}
fn source(e: &Editor, asset: &str) -> Prefab {
    Prefab::from_json(&std::fs::read_to_string(source_path(e, asset)).unwrap()).unwrap()
}

#[test]
fn create_place_history_save_play_and_headless_expansion() {
    let t = Temp::new();
    let mut e = t.editor();
    let before = e.scene().clone();
    let asset = run(&mut e, PrefabCommand::Create);
    let p = source(&e, &asset);
    assert_eq!(p.objects.len(), 2);
    assert_eq!(p.objects[0].transform.translation, [0.0; 3]);
    assert_eq!(object(&e, "root").transform, before.objects[0].transform);
    assert_eq!(e.scene().assets[&asset].kind, AssetKind::Prefab);
    e.undo().unwrap();
    assert_eq!(*e.scene(), before);
    assert!(t.0.join(format!("assets/{asset}.prefab.json")).exists());
    e.redo().unwrap();
    run(
        &mut e,
        PrefabCommand::Instantiate {
            asset: asset.clone(),
            position: Some([-4.0, 0.0, 1.0]),
        },
    );
    let second = e.selected.clone().unwrap();
    let state = e.scene().clone();
    assert_ne!(second, "root");
    assert_eq!(object(&e, &second).transform.translation, [-4.0, 0.0, 1.0]);
    assert_eq!(e.scene().prefabs.len(), 2);
    assert_eq!(e.scene().objects.len(), 4);
    e.undo().unwrap();
    assert_eq!(e.scene().objects.len(), 2);
    e.redo().unwrap();
    assert_eq!(*e.scene(), state);
    let path = t.0.join("elsewhere/saved.json");
    e.save(&path).unwrap();
    let saved = e.scene().clone();
    let mut reopened = Editor::open(&path).unwrap();
    assert_eq!(*reopened.scene(), saved);
    reopened.start_play().unwrap();
    for _ in 0..3 {
        reopened.advance(Duration::from_millis(17));
    }
    assert_eq!(*reopened.scene(), saved);
    reopened.stop_play();
    // Headless simulation does not read authoring prefab files.
    std::fs::remove_file(source_path(&reopened, &asset)).unwrap();
    let document = Scene::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
    bozzard_demo::SceneDemo::new(&document).unwrap();
}

#[test]
fn apply_updates_siblings_preserves_component_overrides_placement_and_source_history() {
    let t = Temp::new();
    let mut e = t.editor();
    let asset = run(&mut e, PrefabCommand::Create);
    run(
        &mut e,
        PrefabCommand::Instantiate {
            asset: asset.clone(),
            position: Some([8.0, 0.0, 0.0]),
        },
    );
    let second = e.selected.clone().unwrap();
    let second_child = e.scene().prefabs[&second].members["child"].clone();
    edit(&mut e, &second, |o| {
        o.drawable.as_mut().unwrap().color = [1.0, 0.0, 0.0]
    });
    edit(&mut e, "root", |o| {
        o.drawable.as_mut().unwrap().color = [0.0, 0.0, 1.0]
    });
    edit(&mut e, "child", |o| {
        o.light.as_mut().unwrap().intensity = 30.0
    });
    e.selected = Some("root".into());
    let before = e.scene().clone();
    run(&mut e, PrefabCommand::Apply);
    assert_eq!(
        object(&e, &second).drawable.as_ref().unwrap().color,
        [1.0, 0.0, 0.0]
    );
    assert_eq!(object(&e, &second_child).light.unwrap().intensity, 30.0);
    assert_eq!(object(&e, &second).transform.translation, [8.0, 0.0, 0.0]);
    let published = source(&e, &asset);
    assert_eq!(published.objects[0].transform.translation, [0.0; 3]);
    e.undo().unwrap();
    assert_eq!(*e.scene(), before);
    assert_eq!(source(&e, &asset), published);
    e.redo().unwrap();
    e.selected = Some(second.clone());
    e.duplicate().unwrap();
    assert_eq!(e.scene().prefabs.len(), 3);
    let copy = e.selected.clone().unwrap();
    e.delete().unwrap();
    assert_eq!(e.scene().prefabs.len(), 2);
    e.undo().unwrap();
    assert!(e.scene().prefabs.contains_key(&copy));
    e.selected = Some(second);
    e.unpack_prefab().unwrap();
    assert_eq!(e.scene().prefabs.len(), 2);
    e.undo().unwrap();
    assert_eq!(e.scene().prefabs.len(), 3);
}

#[test]
fn refresh_adds_and_removes_children_but_rejects_local_conflicts_and_invalid_source() {
    let t = Temp::new();
    let mut e = t.editor();
    let asset = run(&mut e, PrefabCommand::Create);
    let path = source_path(&e, &asset);
    let mut p = source(&e, &asset);
    let mut added = p.objects[1].clone();
    added.id = "extra".into();
    added.name = "Extra".into();
    p.objects.push(added);
    std::fs::write(&path, p.to_json().unwrap()).unwrap();
    run(
        &mut e,
        PrefabCommand::Refresh {
            asset: asset.clone(),
        },
    );
    assert_eq!(e.scene().objects.len(), 3);
    let id = e.scene().prefabs["root"].members["extra"].clone();
    edit(&mut e, &id, |o| o.name = "Local edit".into());
    p.objects.retain(|o| o.id != "extra");
    std::fs::write(&path, p.to_json().unwrap()).unwrap();
    let before = e.scene().clone();
    assert!(
        wait(
            &e.prefab_job(PrefabCommand::Refresh {
                asset: asset.clone()
            })
            .unwrap()
        )
        .is_err()
    );
    assert_eq!(*e.scene(), before);
    e.undo().unwrap();
    run(
        &mut e,
        PrefabCommand::Refresh {
            asset: asset.clone(),
        },
    );
    assert_eq!(e.scene().objects.len(), 2);
    std::fs::write(&path, "broken").unwrap();
    let before = e.scene().clone();
    assert!(wait(&e.prefab_job(PrefabCommand::Refresh { asset }).unwrap()).is_err());
    assert_eq!(*e.scene(), before);
}

#[test]
fn stale_cancelled_and_external_source_changes_never_publish() {
    let t = Temp::new();
    let mut e = t.editor();
    let job = e.prefab_job(PrefabCommand::Create).unwrap();
    let prepared = wait(&job).unwrap();
    let asset = prepared.asset.clone();
    let path = t.0.join(format!("assets/{asset}.prefab.json"));
    assert!(!path.exists());
    edit(&mut e, "root", |o| o.name = "Changed".into());
    assert!(e.accept_prefab(prepared).is_err());
    assert!(!path.exists());
    let job = e.prefab_job(PrefabCommand::Create).unwrap();
    let prepared = wait(&job).unwrap();
    job.cancel();
    assert!(e.accept_prefab(prepared).is_err());
    assert!(!t.0.join("assets").exists());
    let asset = run(&mut e, PrefabCommand::Create);
    let path = source_path(&e, &asset);
    edit(&mut e, "child", |o| {
        o.light.as_mut().unwrap().intensity = 50.0
    });
    let job = e.prefab_job(PrefabCommand::Apply).unwrap();
    let prepared = wait(&job).unwrap();
    let mut external = source(&e, &asset);
    external.objects[1].name = "External".into();
    let bytes = external.to_json().unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let before = e.scene().clone();
    assert!(e.accept_prefab(prepared).is_err());
    assert_eq!(*e.scene(), before);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    // Apply must not clobber a definition this instance has not refreshed yet.
    assert!(wait(&e.prefab_job(PrefabCommand::Apply).unwrap()).is_err());
    let job = e
        .prefab_job(PrefabCommand::Refresh {
            asset: asset.clone(),
        })
        .unwrap();
    let prepared = wait(&job).unwrap();
    std::fs::write(&path, "broken").unwrap();
    assert!(e.accept_prefab(prepared).is_err());
    assert_eq!(*e.scene(), before);
}

#[test]
fn dependencies_rebase_import_bind_colliding_ids_and_are_protected_in_history() {
    let t = Temp::new();
    let mut e = t.editor();
    let image = t.0.join("image.png");
    std::fs::write(
        &image,
        include_bytes!("../../../examples/demo/scenes/assets/soft-sprite.png"),
    )
    .unwrap();
    let image_id = e.import(&image).unwrap();
    e.selected = Some("root".into());
    e.assign_asset_to_selected(&image_id).unwrap();
    let asset = run(&mut e, PrefabCommand::Create);
    let definition = source_path(&e, &asset);
    let project = t.0.join("other");
    std::fs::create_dir(&project).unwrap();
    let mut other = Editor::new(
        bozzard_demo::scene_document().unwrap(),
        &project.join("scene.json"),
    )
    .unwrap();
    // Deliberately occupy the source's image ID with a different file.
    let mut scene = other.scene().clone();
    scene.assets.insert(
        image_id.clone(),
        bozzard_scene::AssetSource {
            kind: AssetKind::Image,
            path: "../image.png".into(),
        },
    );
    other.apply("catalog", scene).unwrap();
    let imported = other.import(&definition).unwrap();
    run(
        &mut other,
        PrefabCommand::Instantiate {
            asset: imported.clone(),
            position: None,
        },
    );
    assert!(other.assets.require_ready().is_ok());
    let root_id = other.selected.clone().unwrap();
    let texture = &object(&other, &root_id).drawable.as_ref().unwrap().texture;
    assert_ne!(texture, &bozzard_scene::Texture::Asset(image_id));
    let bozzard_scene::Texture::Asset(bound) = texture else {
        panic!("missing image")
    };
    let bound = bound.clone();
    assert!(other.remove_asset(&bound).is_err());
    assert!(other.remove_asset(&imported).is_err());
    other.save(&project.join("deeper/scene.json")).unwrap();
    let mut reopened = Editor::open(&other.path).unwrap();
    run(&mut reopened, PrefabCommand::Refresh { asset: imported });
    assert_eq!(reopened.scene().prefabs.len(), 1);
}

#[test]
fn linked_structure_requires_unpack_and_operations_reject_play() {
    let t = Temp::new();
    let mut e = t.editor();
    let asset = run(&mut e, PrefabCommand::Create);
    let before = e.scene().clone();
    e.selected = Some("child".into());
    assert!(e.delete().is_err());
    assert!(e.reparent("child", None).is_err());
    assert_eq!(*e.scene(), before);
    assert!(wait(&e.prefab_job(PrefabCommand::Create).unwrap()).is_err());
    e.start_play().unwrap();
    assert!(
        e.prefab_job(PrefabCommand::Instantiate {
            asset,
            position: None
        })
        .is_err()
    );
    assert!(e.unpack_prefab().is_err());
    e.stop_play();
    e.unpack_prefab().unwrap();
    e.delete().unwrap();
    assert_eq!(e.scene().objects.len(), 1);
    e.create(Mesh::Cube, Layer::ThreeD).unwrap();
}

#[test]
fn shared_source_updates_across_scenes_and_abandoned_import_keeps_user_file() {
    let t = Temp::new();
    let mut first = t.editor();
    let asset = run(&mut first, PrefabCommand::Create);
    let path = source_path(&first, &asset);
    let mut second = Editor::new(
        bozzard_demo::scene_document().unwrap(),
        &t.0.join("second/scene.json"),
    )
    .unwrap();
    let job = second.import_job(path.clone()).unwrap();
    let prepared = wait(&job).unwrap();
    drop(prepared);
    assert!(path.exists());
    let shared = second.import(&path).unwrap();
    assert_eq!(second.import(&path).unwrap(), shared);
    assert_eq!(
        source_path(&second, &shared).canonicalize().unwrap(),
        path.canonicalize().unwrap()
    );
    run(
        &mut second,
        PrefabCommand::Instantiate {
            asset: shared.clone(),
            position: Some([-2.0, 1.0, 0.0]),
        },
    );
    let root = second.selected.clone().unwrap();
    let child = second.scene().prefabs[&root].members["child"].clone();
    edit(&mut first, "child", |o| {
        o.light.as_mut().unwrap().intensity = 81.0
    });
    run(&mut first, PrefabCommand::Apply);
    assert_eq!(object(&second, &child).light.unwrap().intensity, 10.0);
    run(&mut second, PrefabCommand::Refresh { asset: shared });
    assert_eq!(object(&second, &child).light.unwrap().intensity, 81.0);
    assert_eq!(
        object(&second, &root).transform.translation,
        [-2.0, 1.0, 0.0]
    );
}

#[test]
fn source_reparent_preserves_local_child_transform_and_rejects_deleted_active_camera() {
    let t = Temp::new();
    let mut e = t.editor();
    edit(&mut e, "child", |o| {
        o.camera = Some(bozzard_scene::Camera::Perspective {
            near: 0.1,
            far: 100.0,
            vertical_fov_degrees: 60.0,
        })
    });
    let mut scene = e.scene().clone();
    scene.views.insert(Layer::ThreeD, "child".into());
    e.apply("camera", scene).unwrap();
    let asset = run(&mut e, PrefabCommand::Create);
    let path = source_path(&e, &asset);
    let mut p = source(&e, &asset);
    let mut wrapper = p.objects[0].clone();
    wrapper.id = "wrapper".into();
    wrapper.parent = Some(p.root.clone());
    wrapper.camera = None;
    wrapper.drawable = None;
    p.objects.push(wrapper);
    p.objects[1].parent = Some("wrapper".into());
    edit(&mut e, "child", |o| {
        o.transform.translation = [1.0, 3.0, 2.0]
    });
    std::fs::write(&path, p.to_json().unwrap()).unwrap();
    run(
        &mut e,
        PrefabCommand::Refresh {
            asset: asset.clone(),
        },
    );
    assert_eq!(object(&e, "child").transform.translation, [1.0, 3.0, 2.0]);
    assert_eq!(
        object(&e, "child").parent.as_ref(),
        e.scene().prefabs["root"].members.get("wrapper")
    );
    // First remove the local override so the active-camera reference is the reason for rejection.
    let mut baseline = e.scene().prefabs["root"]
        .baseline
        .iter()
        .find(|o| o.id == "child")
        .unwrap()
        .clone();
    edit(&mut e, "child", |o| std::mem::swap(o, &mut baseline));
    p.objects.retain(|o| o.id != "child");
    std::fs::write(&path, p.to_json().unwrap()).unwrap();
    let before = e.scene().clone();
    let error = wait(&e.prefab_job(PrefabCommand::Refresh { asset }).unwrap())
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("camera"));
    assert_eq!(*e.scene(), before);
}

#[test]
fn malformed_metadata_and_nested_prefabs_are_rejected() {
    let t = Temp::new();
    let mut e = t.editor();
    run(&mut e, PrefabCommand::Create);
    let mut s = e.scene().clone();
    s.prefabs
        .get_mut("root")
        .unwrap()
        .members
        .insert("duplicate".into(), "child".into());
    assert!(s.validate().is_err());
    let mut s = e.scene().clone();
    s.prefabs.get_mut("root").unwrap().baseline[1].parent = Some("child".into());
    assert!(s.validate().is_err());
    let mut s = e.scene().clone();
    let link = s.prefabs["root"].clone();
    s.prefabs.insert("child".into(), link);
    assert!(s.validate().is_err());
    let mut s = e.scene().clone();
    s.prefabs.get_mut("root").unwrap().baseline[1]
        .transform
        .scale = [0.0; 3];
    assert!(s.validate().is_err());
    let mut p = source(&e, &e.scene().prefabs["root"].asset);
    p.assets.insert(
        "nested".into(),
        e.scene().assets.values().next().unwrap().clone(),
    );
    assert!(p.validate().is_err());
}
