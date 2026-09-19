use anyhow::Result;
use bozzard_app::job::{Job, Progress};
use bozzard_assets::{AssetData, AssetStore};
use bozzard_demo::SceneDemo;
use bozzard_project::{
    CookTarget,
    content::{PackSpec, prepare_pack},
    streaming::{self, SceneAssets},
};
use bozzard_scene::{
    AssetKind, AssetSource, Object, Scene, Transform,
    scene_loading::{LoadPhase, SceneLoaderHandle, SceneSource},
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[path = "support/content_http.rs"]
mod http;

struct Fixture {
    root: PathBuf,
    main: Scene,
}
impl Fixture {
    fn new() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bozzard-streaming-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("levels"))?;
        let mut main = empty();
        main.objects.push(Object {
            id: "existing".into(),
            name: "Existing".into(),
            ..Default::default()
        });
        main.runtime_scene_sources.insert(
            "next".into(),
            SceneSource::File {
                path: "levels/next.json".into(),
            },
        );
        let mut next = bozzard_demo::scene_document()?;
        next.assets.insert(
            "incoming-motion".into(),
            AssetSource {
                kind: AssetKind::Script,
                path: "motion.rs".into(),
            },
        );
        next.assets.insert(
            "incoming-image".into(),
            AssetSource {
                kind: AssetKind::Image,
                path: "panel.png".into(),
            },
        );
        next.objects.push(Object {
            id: "incoming".into(),
            name: "Incoming".into(),
            script_manager: Some(serde_json::from_value(
                serde_json::json!({"scripts":[{"enabled":true,"script":"incoming-motion"}]}),
            )?),
            ..Default::default()
        });
        fs::write(root.join("levels/next.json"), next.to_json()?)?;
        fs::write(
            root.join("levels/motion.rs"),
            "fn on_start(me) { set_position(me, [3.0, 2.0, 1.0]); }",
        )?;
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/demo/scenes/assets/middleware-panel.png"),
            root.join("levels/panel.png"),
        )?;
        Ok(Self { root, main })
    }
    fn demo(&self) -> Result<SceneDemo> {
        let mut demo = SceneDemo::new_with_prefabs(&self.main, Some(&self.root.join("main.json")))?;
        let mut assets = AssetStore::new(&self.root, &self.main.assets)?;
        assets.load_pending()?;
        assets.require_ready()?;
        streaming::install_with_cache(
            &mut demo.app.world,
            &self.root.join("main.json"),
            &assets,
            self.root.join("cache"),
        )?;
        Ok(demo)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn empty() -> Scene {
    Scene::from_json(r#"{"version":1,"name":"Base","views":{},"objects":[]}"#).unwrap()
}
fn wait<T: Send + 'static>(job: &Job<T>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "scene acquisition worker timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn finish(demo: &mut SceneDemo) -> String {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        demo.app.step();
        demo.check_simulation().unwrap();
        let status = demo.instance().scene_load_status(&demo.app.world);
        if status.phase == LoadPhase::Loaded {
            return status.handle;
        }
        assert_ne!(status.phase, LoadPhase::Failed, "{}", status.error);
        assert!(Instant::now() < deadline, "scene load timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn file_scene_assets_and_scripts_publish_together_without_resetting_existing_entities() -> Result<()>
{
    let fixture = Fixture::new()?;
    let mut demo = fixture.demo()?;
    let existing = demo.instance().entity("existing").unwrap();
    demo.app
        .world
        .get_mut::<Transform>(existing)
        .unwrap()
        .translation = [9., 8., 7.];
    assert!(
        demo.app
            .world
            .resource::<SceneAssets>()
            .unwrap()
            .store
            .handle("incoming-image")
            .is_none()
    );
    demo.with_instance(|instance, world| instance.begin_scene_load(world, "next", true))?;
    let handle = finish(&mut demo);
    demo.app.step();
    demo.check_simulation()?;
    assert_eq!(demo.instance().entity("existing"), Some(existing));
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(existing)
            .unwrap()
            .translation,
        [9., 8., 7.]
    );
    let entity = demo
        .instance()
        .entity(&format!("{handle}-incoming"))
        .unwrap();
    assert_eq!(
        demo.app.world.get::<Transform>(entity).unwrap().translation,
        [3., 2., 1.]
    );
    let loaded = demo.app.world.resource::<SceneAssets>().unwrap();
    assert_eq!(loaded.generation, 1);
    let entry = loaded
        .store
        .get(loaded.store.handle("incoming-image").unwrap())
        .unwrap();
    assert!(matches!(entry.data(), Some(AssetData::Image(_))));
    let saved = demo.instance().save_game_json(&demo.app.world)?;
    demo.with_instance(|instance, world| instance.unload_runtime_scene(world, &handle))?;
    assert!(
        demo.instance()
            .entity(&format!("{handle}-incoming"))
            .is_none()
    );
    assert!(demo.app.world.contains(existing));
    demo.with_instance(|instance, world| instance.load_game_json(world, &saved))?;
    demo.with_instance(|instance, world| instance.unload_runtime_scene(world, &handle))?;
    Ok(())
}

#[test]
fn cancelled_and_stale_acquisition_cannot_replace_entities_or_publish_assets() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut demo = fixture.demo()?;
    for cancel in [true, false] {
        let plan = demo.instance().prepare_scene_load("next", false)?;
        let job = demo
            .app
            .world
            .resource::<SceneLoaderHandle>()
            .unwrap()
            .0
            .start(plan, &demo.app.world)?;
        let prepared = wait(&job)?;
        if cancel {
            job.cancel();
        } else {
            demo.with_instance(|instance, world| instance.restart_runtime_scene(world))?;
        }
        assert!(
            demo.with_instance(|instance, world| instance.accept_scene_load(world, prepared))
                .is_err()
        );
        assert!(demo.instance().entity("existing").is_some());
        assert_eq!(
            demo.app.world.resource::<SceneAssets>().unwrap().generation,
            0
        );
        assert!(demo.instance().document().assets.is_empty());
    }
    Ok(())
}

#[test]
fn invalid_script_or_image_fails_without_publishing_any_part_of_the_scene() -> Result<()> {
    for broken_script in [true, false] {
        let fixture = Fixture::new()?;
        let mut demo = fixture.demo()?;
        let target = if broken_script {
            "motion.rs"
        } else {
            "panel.png"
        };
        fs::write(
            fixture.root.join("levels").join(target),
            "invalid dependency",
        )?;
        let plan = demo.instance().prepare_scene_load("next", true)?;
        let job = demo
            .app
            .world
            .resource::<SceneLoaderHandle>()
            .unwrap()
            .0
            .start(plan, &demo.app.world)?;
        assert!(wait(&job).is_err());
        assert_eq!(
            demo.app.world.resource::<SceneAssets>().unwrap().generation,
            0
        );
        assert_eq!(demo.instance().document().objects.len(), 1);
        // Shared status also reports acquisition failures while the old scene keeps ticking.
        demo.with_instance(|instance, world| instance.begin_scene_load(world, "next", true))?;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            demo.app.step();
            demo.check_simulation()?;
            if demo.instance().scene_load_status(&demo.app.world).phase == LoadPhase::Failed {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(demo.instance().document().objects.len(), 1);
    }
    Ok(())
}

#[test]
fn addressable_pack_can_replace_the_scene_after_authoring_files_are_deleted() -> Result<()> {
    let mut fixture = Fixture::new()?;
    let spec = PackSpec {
        version: 1,
        id: "stream".into(),
        name: "Streamed scene".into(),
        cook: CookTarget::Rgba,
        scenes: BTreeMap::from([("levels/next".into(), "next.json".into())]),
        assets: BTreeMap::new(),
    };
    let spec_path = fixture.root.join("levels/pack.json");
    fs::write(&spec_path, serde_json::to_vec(&spec)?)?;
    prepare_pack(
        &spec_path,
        &fixture.root.join("release"),
        &Progress::default(),
    )?
    .commit()?;
    fixture.main.runtime_scene_sources.insert(
        "next".into(),
        SceneSource::Content {
            catalog: "release/catalog.json".into(),
            address: "levels/next".into(),
        },
    );
    fs::remove_dir_all(fixture.root.join("levels"))?;
    let mut demo = fixture.demo()?;
    let old = demo.instance().entity("existing").unwrap();
    demo.with_instance(|instance, world| instance.begin_scene_load(world, "next", false))?;
    finish(&mut demo);
    demo.app.step();
    demo.check_simulation()?;
    assert!(!demo.app.world.contains(old));
    let incoming = demo.instance().entity("incoming").unwrap();
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(incoming)
            .unwrap()
            .translation,
        [3., 2., 1.]
    );
    let loaded = demo.app.world.resource::<SceneAssets>().unwrap();
    loaded.store.require_ready()?;
    assert_eq!(loaded.generation, 1);
    Ok(())
}

#[test]
fn lazy_scene_graphs_are_cooked_transitively_and_keep_cycles_after_relocation() -> Result<()> {
    use bozzard_project::content::{ContentStore, load_catalog};
    let fixture = Fixture::new()?;
    let mut main = bozzard_demo::scene_document()?;
    main.runtime_scene_sources = fixture.main.runtime_scene_sources.clone();
    fs::write(fixture.root.join("main.json"), main.to_json()?)?;
    let next_path = fixture.root.join("levels/next.json");
    let mut next = Scene::from_json(&fs::read_to_string(&next_path)?)?;
    next.runtime_scene_sources.insert(
        "home".into(),
        SceneSource::File {
            path: "../main.json".into(),
        },
    );
    fs::write(next_path, next.to_json()?)?;
    let spec = PackSpec {
        version: 1,
        id: "lazy".into(),
        name: "Lazy graph".into(),
        cook: CookTarget::Rgba,
        scenes: BTreeMap::from([("home".into(), "main.json".into())]),
        assets: BTreeMap::new(),
    };
    let spec_path = fixture.root.join("pack.json");
    fs::write(&spec_path, serde_json::to_vec(&spec)?)?;
    prepare_pack(
        &spec_path,
        &fixture.root.join("release"),
        &Progress::default(),
    )?
    .commit()?;
    fs::rename(fixture.root.join("release"), fixture.root.join("relocated"))?;
    fs::remove_file(fixture.root.join("main.json"))?;
    fs::remove_dir_all(fixture.root.join("levels"))?;
    let catalog = load_catalog(
        fixture
            .root
            .join("relocated/catalog.json")
            .to_str()
            .unwrap(),
        &Progress::default(),
    )?;
    let mounted = ContentStore::new(fixture.root.join("cache")).resolve(
        &catalog,
        "home",
        &Progress::default(),
    )?;
    let path = mounted.path();
    let document = Scene::from_json(&fs::read_to_string(&path)?)?;
    let mut demo = SceneDemo::new_with_prefabs(&document, Some(&path))?;
    let mut assets = AssetStore::new(path.parent().unwrap(), &document.assets)?;
    assets.load_pending()?;
    streaming::install_with_cache(
        &mut demo.app.world,
        &path,
        &assets,
        fixture.root.join("cache"),
    )?;
    demo.with_instance(|instance, world| instance.begin_scene_load(world, "next", false))?;
    finish(&mut demo);
    demo.app.step();
    demo.check_simulation()?;
    assert!(demo.instance().entity("incoming").is_some());
    demo.with_instance(|instance, world| instance.begin_scene_load(world, "home", false))?;
    finish(&mut demo);
    assert!(demo.instance().entity("incoming").is_none());
    assert_eq!(demo.instance().document().name, main.name);
    assert_eq!(
        demo.app.world.resource::<SceneAssets>().unwrap().generation,
        2
    );
    Ok(())
}

#[test]
fn save_as_rebases_lazy_files_and_catalogs_without_opening_them() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut scene = fixture.main.clone();
    scene.runtime_scene_sources.insert(
        "future".into(),
        SceneSource::File {
            path: "not-created-yet.json".into(),
        },
    );
    scene.runtime_scene_sources.insert(
        "local".into(),
        SceneSource::Content {
            catalog: "releases/catalog.json".into(),
            address: "level".into(),
        },
    );
    scene.runtime_scene_sources.insert(
        "remote".into(),
        SceneSource::Content {
            catalog: "https://example.invalid/catalog.json".into(),
            address: "level".into(),
        },
    );
    let saved = bozzard_demo::prepare_document_from(
        &scene,
        &fixture.root.join("saved/main.json"),
        Some(&fixture.root.join("main.json")),
    )?;
    assert_eq!(
        saved.runtime_scene_sources["future"],
        SceneSource::File {
            path: "../not-created-yet.json".into()
        }
    );
    assert_eq!(
        saved.runtime_scene_sources["local"],
        SceneSource::Content {
            catalog: "../releases/catalog.json".into(),
            address: "level".into()
        }
    );
    assert_eq!(
        saved.runtime_scene_sources["remote"],
        scene.runtime_scene_sources["remote"]
    );
    Ok(())
}

#[test]
fn http_scene_acquisition_and_cancellation_keep_simulation_responsive() -> Result<()> {
    for cancel in [false, true] {
        let mut fixture = Fixture::new()?;
        let spec = PackSpec {
            version: 1,
            id: "network".into(),
            name: "Network scene".into(),
            cook: CookTarget::Rgba,
            scenes: BTreeMap::from([("next".into(), "next.json".into())]),
            assets: BTreeMap::new(),
        };
        let spec_path = fixture.root.join("levels/pack.json");
        fs::write(&spec_path, serde_json::to_vec(&spec)?)?;
        prepare_pack(
            &spec_path,
            &fixture.root.join("release"),
            &Progress::default(),
        )?
        .commit()?;
        let catalog = fs::read(fixture.root.join("release/catalog.json"))?;
        let pack = fs::read(fixture.root.join("release/content.bpack"))?;
        let server = http::Server::start(move |path| match path {
            "/entry" => http::Response::redirect("/catalog.json"),
            "/catalog.json" => http::Response::ok(catalog.clone()),
            "/content.bpack" => http::Response {
                chunk: 4096,
                delay: Duration::from_millis(20),
                ..http::Response::ok(pack.clone())
            },
            _ => panic!("unexpected content request: {path}"),
        })?;
        fixture.main.runtime_scene_sources.insert(
            "next".into(),
            SceneSource::Content {
                catalog: format!("{}/entry", server.base),
                address: "next".into(),
            },
        );
        fs::remove_dir_all(fixture.root.join("levels"))?;
        let mut demo = fixture.demo()?;
        let old = demo.instance().entity("existing").unwrap();
        demo.with_instance(|instance, world| instance.begin_scene_load(world, "next", true))?;
        server.wait_for("/content.bpack");
        // A tick still runs while the body is arriving.
        demo.app.step();
        demo.check_simulation()?;
        assert_eq!(demo.instance().entity("existing"), Some(old));
        if cancel {
            demo.with_instance(|instance, world| instance.cancel_scene_load(world));
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                demo.app.step();
                demo.check_simulation()?;
                let phase = demo.instance().scene_load_status(&demo.app.world).phase;
                if phase == LoadPhase::Cancelled {
                    break;
                }
                assert_eq!(phase, LoadPhase::Cancelling);
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(
                demo.app.world.resource::<SceneAssets>().unwrap().generation,
                0
            );
            assert_eq!(demo.instance().document().objects.len(), 1);
        } else {
            let handle = finish(&mut demo);
            assert!(
                demo.instance()
                    .entity(&format!("{handle}-incoming"))
                    .is_some()
            );
            assert_eq!(
                demo.app.world.resource::<SceneAssets>().unwrap().generation,
                1
            );
        }
    }
    Ok(())
}

#[test]
fn common_assets_reuse_decoded_data_but_conflicting_global_ids_are_rejected() -> Result<()> {
    for conflict in [false, true] {
        let mut fixture = Fixture::new()?;
        fs::copy(
            fixture.root.join("levels/panel.png"),
            fixture.root.join("other.png"),
        )?;
        fixture.main.assets.insert(
            "incoming-image".into(),
            AssetSource {
                kind: AssetKind::Image,
                path: if conflict {
                    "other.png"
                } else {
                    "./levels/panel.png"
                }
                .into(),
            },
        );
        let mut demo = fixture.demo()?;
        let store = &demo.app.world.resource::<SceneAssets>().unwrap().store;
        let before = store
            .get(store.handle("incoming-image").unwrap())
            .unwrap()
            .shared_data()
            .unwrap();
        let plan = demo.instance().prepare_scene_load("next", true)?;
        let job = demo
            .app
            .world
            .resource::<SceneLoaderHandle>()
            .unwrap()
            .0
            .start(plan, &demo.app.world)?;
        let result = wait(&job);
        if conflict {
            assert!(
                result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("scene asset conflict")
            );
            assert_eq!(
                demo.app.world.resource::<SceneAssets>().unwrap().generation,
                0
            );
        } else {
            demo.with_instance(|instance, world| {
                instance.accept_scene_load(world, result.unwrap())
            })?;
            let store = &demo.app.world.resource::<SceneAssets>().unwrap().store;
            let after = store
                .get(store.handle("incoming-image").unwrap())
                .unwrap()
                .shared_data()
                .unwrap();
            assert!(std::sync::Arc::ptr_eq(&before, &after));
        }
    }
    Ok(())
}

#[test]
fn acquired_prefabs_compile_their_own_scripts_and_inherit_loaded_scene_ownership() -> Result<()> {
    let fixture = Fixture::new()?;
    let path = fixture.root.join("levels/next.json");
    let mut next = Scene::from_json(&fs::read_to_string(&path)?)?;
    next.assets.insert(
        "incoming-prefab".into(),
        AssetSource {
            kind: AssetKind::Prefab,
            path: "prop.prefab.json".into(),
        },
    );
    fs::write(&path, next.to_json()?)?;
    fs::write(
        fixture.root.join("levels/motion.rs"),
        "fn on_start(me) { let child = spawn_prefab(\"incoming-prefab\", [0.0, 0.0, 0.0]); }",
    )?;
    fs::write(
        fixture.root.join("levels/prop.prefab.json"),
        r#"{"version":1,"name":"Prop","root":"root",
      "assets":{"child-script":{"kind":"script","path":"child.rs"}},
      "objects":[{"id":"root","name":"Streamed prop",
        "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
        "script_manager":{"scripts":[{"enabled":true,"script":"child-script"}]}}]}"#,
    )?;
    fs::write(
        fixture.root.join("levels/child.rs"),
        "fn on_start(me) { rotate(me, [0.0, 90.0, 0.0]); }",
    )?;
    let mut demo = fixture.demo()?;
    demo.with_instance(|instance, world| instance.begin_scene_load(world, "next", true))?;
    let handle = finish(&mut demo);
    for _ in 0..3 {
        demo.app.step();
        demo.check_simulation()?;
    }
    let spawned = demo
        .instance()
        .document()
        .prefabs
        .keys()
        .next()
        .unwrap()
        .clone();
    let entity = demo.instance().entity(&spawned).unwrap();
    assert_eq!(
        demo.app
            .world
            .get::<Transform>(entity)
            .unwrap()
            .rotation_degrees,
        [0., 90., 0.]
    );
    assert!(
        demo.instance().loaded_scenes()[&handle]
            .members
            .contains(&spawned)
    );
    demo.with_instance(|instance, world| instance.unload_runtime_scene(world, &handle))?;
    assert!(!demo.app.world.contains(entity));
    assert!(demo.instance().entity("existing").is_some());
    Ok(())
}

#[test]
fn loading_lab_runs_the_same_load_unload_reload_controls_as_the_native_player() -> Result<()> {
    let fixture = Fixture::new()?;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/scene-loading-lab.json");
    let document = Scene::from_json(&fs::read_to_string(&path)?)?;
    let mut demo = SceneDemo::new_with_prefabs(&document, Some(&path))?;
    let mut assets = AssetStore::new(path.parent().unwrap(), &document.assets)?;
    assets.load_pending()?;
    streaming::install_with_cache(
        &mut demo.app.world,
        &path,
        &assets,
        fixture.root.join("cache"),
    )?;
    let hero = demo.instance().entity("hero-cube").unwrap();
    let handle = finish(&mut demo);
    for _ in 0..3 {
        demo.app.step();
        demo.check_simulation()?;
    }
    assert!(
        demo.instance()
            .entity(&format!("{handle}-annex-0"))
            .is_some()
    );
    let status = demo.instance().entity("loading-status").unwrap();
    assert!(
        demo.app
            .world
            .get::<bozzard_scene::TextRendering>(status)
            .unwrap()
            .text
            .contains("Annex loaded")
    );
    demo.set_gameplay_input(bozzard_scene::GameplayInput {
        keys: bozzard_scene::keys::bit("U"),
        ..Default::default()
    });
    demo.app.step();
    demo.check_simulation()?;
    demo.clear_gameplay_input();
    demo.app.step();
    demo.check_simulation()?;
    assert!(demo.instance().loaded_scenes().is_empty());
    assert_eq!(demo.instance().entity("hero-cube"), Some(hero));
    demo.set_gameplay_input(bozzard_scene::GameplayInput {
        keys: bozzard_scene::keys::bit("L"),
        ..Default::default()
    });
    demo.app.step();
    demo.check_simulation()?;
    demo.clear_gameplay_input();
    assert_ne!(finish(&mut demo), handle);
    assert_eq!(demo.instance().entity("hero-cube"), Some(hero));
    Ok(())
}

#[test]
fn fresh_runtime_restores_acquired_catalogs_and_ownership_from_files_or_cached_content()
-> Result<()> {
    for content in [false, true] {
        let mut fixture = Fixture::new()?;
        fs::write(
            fixture.root.join("levels/motion.rs"),
            "fn on_update(me, dt) { rotate(me, [0.0, 1.0, 0.0]); }",
        )?;
        if content {
            let path = fixture.root.join("levels/next.json");
            let mut chunk = Scene::from_json(&fs::read_to_string(&path)?)?;
            chunk.views.clear();
            fs::write(path, chunk.to_json()?)?;
            let spec = PackSpec {
                version: 1,
                id: "restore".into(),
                name: "Restore".into(),
                cook: CookTarget::Rgba,
                scenes: BTreeMap::from([("next".into(), "next.json".into())]),
                assets: BTreeMap::new(),
            };
            let path = fixture.root.join("levels/pack.json");
            fs::write(&path, serde_json::to_vec(&spec)?)?;
            prepare_pack(&path, &fixture.root.join("release"), &Progress::default())?.commit()?;
            fixture.main.runtime_scene_sources.insert(
                "next".into(),
                SceneSource::Content {
                    catalog: "release/catalog.json".into(),
                    address: "next".into(),
                },
            );
        }
        let mut first = fixture.demo()?;
        let before_acquisition = first.instance().save_game_json(&first.app.world)?;
        first.with_instance(|instance, world| instance.begin_scene_load(world, "next", true))?;
        let handle = finish(&mut first);
        if content {
            assert!(
                first.instance().document().views.is_empty(),
                "additive content need not introduce a camera"
            );
            let catalog = bozzard_project::content::load_catalog(
                fixture.root.join("release/catalog.json").to_str().unwrap(),
                &Progress::default(),
            )?;
            let resolved = bozzard_project::content::ContentStore::new(fixture.root.join("cache"))
                .resolve(&catalog, "next", &Progress::default())?;
            assert!(resolved.scene_path()?.is_file());
            assert!(
                resolved.scene_view().is_err(),
                "a chunk cannot be a standalone player entry point"
            );
        }
        first.app.step();
        first.check_simulation()?;
        let id = format!("{handle}-incoming");
        let incoming = first.instance().entity(&id).unwrap();
        first
            .app
            .world
            .get_mut::<Transform>(incoming)
            .unwrap()
            .translation = [7., 8., 9.];
        let saved = first.instance().save_game_json(&first.app.world)?;
        // Older snapshots remain valid after the live catalog grows.
        first
            .with_instance(|instance, world| instance.load_game_json(world, &before_acquisition))?;
        assert!(first.instance().loaded_scenes().is_empty());
        assert!(
            first
                .instance()
                .document()
                .assets
                .contains_key("incoming-image")
        );
        drop(first);
        if content {
            // The save refers to its immutable cache generation, not the latest catalog.
            fs::remove_dir_all(fixture.root.join("levels"))?;
            fs::remove_dir_all(fixture.root.join("release"))?;
        }
        let mut restored = fixture.demo()?;
        let old = restored.instance().entity("existing").unwrap();
        assert!(
            restored
                .with_instance(|instance, world| instance.load_game_json(world, &saved))
                .is_err()
        );
        restored.with_instance(|instance, world| instance.begin_game_load(world, &saved))?;
        assert_eq!(restored.instance().entity("existing"), Some(old));
        assert_eq!(
            restored
                .app
                .world
                .resource::<SceneAssets>()
                .unwrap()
                .generation,
            0
        );
        finish(&mut restored);
        let incoming = restored.instance().entity(&id).unwrap();
        assert_eq!(
            restored
                .app
                .world
                .get::<Transform>(incoming)
                .unwrap()
                .translation,
            [7., 8., 9.]
        );
        assert!(
            restored.instance().loaded_scenes()[&handle]
                .members
                .contains(&id)
        );
        let assets = restored.app.world.resource::<SceneAssets>().unwrap();
        assert_eq!(assets.generation, 1);
        assert!(
            assets
                .store
                .get(assets.store.handle("incoming-image").unwrap())
                .unwrap()
                .data()
                .is_some()
        );
        let rotation = restored
            .app
            .world
            .get::<Transform>(incoming)
            .unwrap()
            .rotation_degrees[1];
        restored.app.step();
        restored.check_simulation()?;
        assert_eq!(
            restored
                .app
                .world
                .get::<Transform>(incoming)
                .unwrap()
                .rotation_degrees[1],
            rotation + 1.
        );
        restored.with_instance(|instance, world| instance.unload_runtime_scene(world, &handle))?;
        assert!(restored.instance().entity("existing").is_some());
        assert!(!restored.app.world.contains(incoming));
    }
    Ok(())
}

#[test]
fn restoring_missing_cancelled_or_stale_dependencies_never_publishes_partial_state() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut first = fixture.demo()?;
    first.with_instance(|instance, world| instance.begin_scene_load(world, "next", true))?;
    finish(&mut first);
    let saved = first.instance().save_game_json(&first.app.world)?;
    drop(first);
    for cancel in [false, true] {
        let mut demo = fixture.demo()?;
        let plan = demo.instance().prepare_game_load(&saved)?;
        let job = demo
            .app
            .world
            .resource::<SceneLoaderHandle>()
            .unwrap()
            .0
            .start(plan, &demo.app.world)?;
        let prepared = wait(&job)?;
        if cancel {
            job.cancel();
        } else {
            demo.with_instance(|instance, world| instance.restart_runtime_scene(world))?;
        }
        let existing = demo.instance().entity("existing").unwrap();
        assert!(
            demo.with_instance(|instance, world| instance.accept_scene_load(world, prepared))
                .is_err()
        );
        assert_eq!(demo.instance().entity("existing"), Some(existing));
        assert_eq!(
            demo.app.world.resource::<SceneAssets>().unwrap().generation,
            0
        );
        assert!(demo.instance().loaded_scenes().is_empty());
    }
    fs::remove_file(fixture.root.join("levels/panel.png"))?;
    let demo = fixture.demo()?;
    let job = demo
        .app
        .world
        .resource::<SceneLoaderHandle>()
        .unwrap()
        .0
        .start(demo.instance().prepare_game_load(&saved)?, &demo.app.world)?;
    assert!(wait(&job).is_err());
    assert_eq!(
        demo.app.world.resource::<SceneAssets>().unwrap().generation,
        0
    );
    assert_eq!(demo.instance().document().objects.len(), 1);
    Ok(())
}

#[test]
fn load_game_script_action_acquires_the_saved_catalog_before_publication() -> Result<()> {
    let mut fixture = Fixture::new()?;
    fixture.main.assets.insert(
        "restore-script".into(),
        AssetSource {
            kind: AssetKind::Script,
            path: "restore.rs".into(),
        },
    );
    fixture.main.objects[0].script_manager = Some(serde_json::from_value(
        serde_json::json!({"scripts":[{"enabled":true,"script":"restore-script"}]}),
    )?);
    fs::write(
        fixture.root.join("restore.rs"),
        "fn on_update(me, dt) { if input_pressed(\"G\") { load_game(\"restore\"); } }",
    )?;
    let mut first = fixture.demo()?;
    first.with_instance(|instance, world| instance.begin_scene_load(world, "next", true))?;
    let handle = finish(&mut first);
    fs::create_dir_all(fixture.root.join("saves"))?;
    fs::write(
        fixture.root.join("saves/restore.json"),
        first.instance().save_game_json(&first.app.world)?,
    )?;
    drop(first);
    let mut fresh = fixture.demo()?;
    fresh
        .app
        .world
        .insert_resource(bozzard_scene::scene_control::GameSaves::in_directory(
            fixture.root.join("saves"),
        ));
    fresh.set_gameplay_input(bozzard_scene::GameplayInput {
        keys: bozzard_scene::keys::bit("G"),
        ..Default::default()
    });
    fresh.app.step();
    fresh.check_simulation()?;
    fresh.clear_gameplay_input();
    finish(&mut fresh);
    assert!(fresh.instance().loaded_scenes().contains_key(&handle));
    assert_eq!(
        fresh
            .app
            .world
            .resource::<SceneAssets>()
            .unwrap()
            .generation,
        1
    );
    Ok(())
}
