use bozzard_project::{Project, prepare_export};
use bozzard_scene::{Layer, Scene};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "bozzard-export-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("{e}"),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes")
}
fn project() -> Project {
    Project {
        version: 1,
        name: "Test & Game".into(),
        start_scene: "scene.json".into(),
        view: Layer::ThreeD,
    }
}
fn data(folder: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        folder.join("Game.app/Contents/Resources/game")
    } else {
        folder.into()
    }
}
fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let path = entry.unwrap().path();
        let destination = target.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_tree(&path, &destination);
        } else {
            fs::copy(path, destination).unwrap();
        }
    }
}
fn load(path: &Path) -> Scene {
    Scene::from_json(&fs::read_to_string(path).unwrap()).unwrap()
}
fn export(scene: &Scene, source: &Path, destination: &Path) {
    prepare_export(
        &project(),
        scene,
        source,
        &std::env::current_exe().unwrap(),
        destination,
        &Default::default(),
    )
    .unwrap()
    .commit()
    .unwrap();
}

#[test]
fn export_survives_source_removal_and_runs_the_whole_trail() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    fs::create_dir_all(source.join("assets")).unwrap();
    fs::copy(
        fixtures().join("first-trail.json"),
        source.join("scene.json"),
    )
    .unwrap();
    fs::copy(
        fixtures().join("assets/octahedron.obj"),
        source.join("assets/octahedron.obj"),
    )
    .unwrap();
    let scene = load(&source.join("scene.json"));
    let folder = temp.0.join("game");
    export(&scene, &source.join("scene.json"), &folder);
    fs::remove_dir_all(&source).unwrap();
    let relocated = temp.0.join("renamed game with spaces");
    fs::rename(folder, &relocated).unwrap();
    let (manifest, path) =
        Project::load(&data(&relocated).join(bozzard_project::MANIFEST)).unwrap();
    assert_eq!(manifest.name, "Test & Game");
    let cooked = load(&path);
    assert_eq!(scene.objects, cooked.objects);
    let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&cooked, Some(&path)).unwrap();
    let mut assets = bozzard_assets::AssetStore::new(
        path.parent().unwrap(),
        &runtime.instance().document().assets,
    )
    .unwrap();
    assets.load_pending().unwrap();
    assets.require_ready().unwrap();
    for tick in 0..340 {
        runtime.set_gameplay_input(bozzard_scene::GameplayInput {
            movement: [0., 1.],
            jump: tick == 80,
            ..Default::default()
        });
        runtime.app.step();
        runtime.check_simulation().unwrap();
    }
    let state = runtime.gameplay().unwrap();
    assert!(
        state.won
            && state.collected.len() == 3
            && state.checkpoint.as_deref() == Some("checkpoint")
    );
}

#[test]
fn model_images_buffers_and_spawn_prefabs_are_relocatable_and_deterministic() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    copy_tree(&fixtures(), &source);
    for name in ["model-lab", "bonfire-lab", "gold-yard", "prefab-lab"] {
        let scene_path = source.join(format!("{name}.json"));
        let scene = load(&scene_path);
        export(&scene, &scene_path, &temp.0.join(name));
        let exported = load(&data(&temp.0.join(name)).join("scene.json"));
        let mut before = bozzard_assets::AssetStore::new(&source, &scene.assets).unwrap();
        before.load_pending().unwrap();
        let mut after =
            bozzard_assets::AssetStore::new(&data(&temp.0.join(name)), &exported.assets).unwrap();
        after.load_pending().unwrap();
        for (id, asset) in &scene.assets {
            if asset.kind != bozzard_scene::AssetKind::Prefab {
                let a = before.get(before.handle(id).unwrap()).unwrap();
                let b = after.get(after.handle(id).unwrap()).unwrap();
                assert_eq!(
                    a.content_fingerprint(),
                    b.content_fingerprint(),
                    "{name}: {id} changed source identity"
                );
            }
        }

        export(&scene, &scene_path, &temp.0.join(format!("{name}-again")));
        assert_eq!(
            fs::read(temp.0.join(name).join("package.json")).unwrap(),
            fs::read(temp.0.join(format!("{name}-again")).join("package.json")).unwrap()
        );
    }
    fs::remove_dir_all(source).unwrap();
    for name in ["model-lab", "bonfire-lab", "gold-yard", "prefab-lab"] {
        let path = data(&temp.0.join(name)).join("scene.json");
        let scene = load(&path);
        let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
        let mut assets = bozzard_assets::AssetStore::new(
            path.parent().unwrap(),
            &runtime.instance().document().assets,
        )
        .unwrap();
        assets.load_pending().unwrap();
        assets.require_ready().unwrap();
        for _ in 0..120 {
            runtime.app.step();
            runtime.check_simulation().unwrap();
        }
        assert!(!fs::read_to_string(path).unwrap().contains("source/"));
    }
}

#[test]
fn errors_cancellation_and_existing_destinations_never_publish_partial_games() {
    let temp = Temp::new();
    let path = fixtures().join("first-trail.json");
    let mut scene = load(&path);
    let runtime = std::env::current_exe().unwrap();
    let destination = temp.0.join("game");
    let prepared = prepare_export(
        &project(),
        &scene,
        &path,
        &runtime,
        &destination,
        &Default::default(),
    )
    .unwrap();
    assert!(!destination.exists());
    drop(prepared); // Job cancellation drops an unpublished export.
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
    scene.assets.values_mut().next().unwrap().path = "missing.obj".into();
    assert!(
        prepare_export(
            &project(),
            &scene,
            &path,
            &runtime,
            &destination,
            &Default::default()
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("keep.txt"), "keep me").unwrap();
    assert!(
        prepare_export(
            &project(),
            &load(&path),
            &path,
            &runtime,
            &destination,
            &Default::default()
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(destination.join("keep.txt")).unwrap(),
        "keep me"
    );
}

#[test]
fn project_rejects_unknown_versions_escaping_paths_and_missing_views() {
    let mut manifest = project();
    manifest.version = 2;
    assert!(manifest.validate().is_err());
    manifest.version = 1;
    for path in [
        "../scene.json",
        "/scene.json",
        "C:/scene.json",
        "scene\\file.json",
        "",
    ] {
        manifest.start_scene = path.into();
        assert!(manifest.validate().is_err(), "{path}");
    }
    manifest.start_scene = "scene.json".into();
    manifest.view = Layer::TwoD;
    assert!(
        manifest
            .validate_scene(&load(&fixtures().join("first-trail.json")))
            .is_err()
    );
    let temp = Temp::new();
    fs::write(
        temp.0.join("bad.json"),
        r#"{"version":1,"name":"Test","start_scene":"scene.json","view":"3d","typo":true}"#,
    )
    .unwrap();
    assert!(Project::load(&temp.0.join("bad.json")).is_err());
}

#[test]
fn runtime_scene_library_assets_are_relocated_and_load_without_sources() {
    let temp = Temp::new();
    let source = temp.0.join("source");
    fs::create_dir_all(source.join("assets")).unwrap();
    fs::copy(
        fixtures().join("assets/octahedron.obj"),
        source.join("assets/octahedron.obj"),
    )
    .unwrap();
    let mut scene = load(&fixtures().join("first-trail.json"));
    let mut level = scene.clone();
    level.name = "Second trail".into();
    scene.runtime_scenes.insert("second".into(), level.into());
    let folder = temp.0.join("export");
    export(&scene, &source.join("scene.json"), &folder);
    fs::remove_dir_all(&source).unwrap();
    let path = data(&folder).join("scene.json");
    let cooked = load(&path);
    let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&cooked, Some(&path)).unwrap();
    runtime
        .with_instance(|i, w| i.load_runtime_scene(w, "second", false))
        .unwrap();
    assert_eq!(runtime.instance().document().name, "Second trail");
    let mut assets = bozzard_assets::AssetStore::new(
        path.parent().unwrap(),
        &runtime.instance().document().assets,
    )
    .unwrap();
    assets.load_pending().unwrap();
    assets.require_ready().unwrap();
}
