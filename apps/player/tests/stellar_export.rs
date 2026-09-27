#![cfg(feature = "steam")]
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
#[test]
fn earth_factory_explores_and_opens_its_journal_after_source_independent_export()
-> anyhow::Result<()> {
    let _steam_shutdown = bozzard_demo::steam_runtime::ShutdownGuard;
    use bozzard_scene::{BlueprintRuntime, GameplayInput, blueprint::Value, keys};
    let temp = Temp::new();
    let source = temp.0.join("source");
    copy_tree(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/earth-factory"),
        &source,
    );
    let (project, scene_path) = Project::load(&source.join("bozzard.project.json"))?;
    let scene = load(&scene_path);
    let exported = temp.0.join("export");
    prepare_export(
        &project,
        &scene,
        &scene_path,
        &PathBuf::from(env!("CARGO_BIN_EXE_bozzard-player")),
        &exported,
        &Default::default(),
    )?
    .commit()?;
    fs::remove_dir_all(source)?;
    let relocated = temp.0.join("Relocated Earth Factory");
    fs::rename(exported, &relocated)?;
    let steam: serde_json::Value =
        serde_json::from_slice(&fs::read(relocated.join("steam-runtime.json"))?)?;
    assert_eq!(steam["app_id"], 480);
    assert_eq!(steam["mode"], "spacewar-development");
    assert!(relocated.join(steam["library"].as_str().unwrap()).is_file());
    assert!(fs::read_to_string(relocated.join("STEAM-README.txt"))?.contains("480"));
    let (_, scene_path) = Project::load(&data(&relocated).join(bozzard_project::MANIFEST))?;
    let scene = load(&scene_path);
    let mut runtime = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&scene_path))?;
    runtime.app.step();
    runtime.check_simulation()?;
    runtime.enable_multiplayer(None)?;
    if std::env::var_os("BOZZARD_EXPORT_LIVE_STEAM").is_some() {
        runtime.pump_multiplayer()?;
        for id in ["coop-open-title", "coop-create"] {
            runtime.ui_input(
                Layer::ThreeD,
                [1080., 600.],
                bozzard_scene::middleware::ui::Input::ActivateObject(id.into()),
            )?;
            runtime.pump_multiplayer()?;
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !runtime.multiplayer_active() {
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "exported Steam lobby did not start"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
            runtime.pump_multiplayer()?;
        }
        runtime.ui_input(
            Layer::ThreeD,
            [1080., 600.],
            bozzard_scene::middleware::ui::Input::ActivateObject("coop-close".into()),
        )?;
    }
    assert!(runtime.ui_input(
        Layer::ThreeD,
        [1080., 600.],
        bozzard_scene::middleware::ui::Input::ActivateObject("title-creative".into())
    )?);
    runtime.app.step();
    runtime.check_simulation()?;
    assert!(runtime.ui_input(
        bozzard_scene::Layer::ThreeD,
        [1080., 600.],
        bozzard_scene::middleware::ui::Input::ActivateObject("title-create".into())
    )?);
    runtime.app.step();
    runtime.check_simulation()?;
    if std::env::var_os("BOZZARD_EXPORT_LIVE_STEAM").is_some() {
        runtime.pump_multiplayer()?;
        assert!(
            runtime
                .app
                .world
                .resource::<bozzard_demo::factory::host::HostRuntime>()
                .is_some(),
            "exported lobby did not connect its host simulation"
        );
    }
    for _ in 0..8 {
        for key in [keys::bit("D"), 0] {
            runtime.set_gameplay_input(GameplayInput {
                keys: key,
                ..Default::default()
            });
            runtime.app.step();
            runtime.check_simulation()?;
        }
    }
    let board = runtime.app.world.resource::<BlueprintRuntime>().unwrap();
    assert_eq!(
        board.scene_blackboard()["chunk_x"].values(),
        &[Value::Number(1.)]
    );
    runtime.set_gameplay_input(GameplayInput {
        keys: keys::bit("J"),
        ..Default::default()
    });
    runtime.app.step();
    runtime.check_simulation()?;
    let frame = runtime
        .instance()
        .ui_frame(&runtime.app.world, Layer::ThreeD, [1080., 600.])?;
    assert!(frame.element("journal-book").is_some());
    // Travel through the exported UI after deleting the entire source project.
    // This exercises the lunar script, prefabs, mesh and material dependencies.
    assert!(runtime.ui_input(
        Layer::ThreeD,
        [1080., 600.],
        bozzard_scene::middleware::ui::Input::ActivateObject("journal-close".into())
    )?);
    runtime.set_gameplay_input(GameplayInput::default());
    for _ in 0..15 {
        runtime.app.step();
        runtime.check_simulation()?;
    }
    for _ in 0..8 {
        for key in [keys::bit("A"), 0] {
            runtime.set_gameplay_input(GameplayInput {
                keys: key,
                ..Default::default()
            });
            runtime.app.step();
            runtime.check_simulation()?;
        }
    }
    runtime.set_gameplay_input(GameplayInput {
        keys: keys::bit("E"),
        ..Default::default()
    });
    runtime.app.step();
    runtime.check_simulation()?;
    runtime.set_gameplay_input(GameplayInput::default());
    assert!(runtime.ui_input(
        Layer::ThreeD,
        [1080., 600.],
        bozzard_scene::middleware::ui::Input::ActivateObject("rocket-launch".into())
    )?);
    runtime.app.step();
    runtime.check_simulation()?;
    for _ in 0..270 {
        runtime.app.step();
        runtime.check_simulation()?;
    }
    let frame = runtime
        .instance()
        .ui_frame(&runtime.app.world, Layer::ThreeD, [1080., 600.])?;
    assert_eq!(
        frame.element("world-status").unwrap().text,
        "STELLA-Z2 / NIGHT"
    );
    let mut assets = bozzard_assets::AssetStore::new(
        scene_path.parent().unwrap(),
        &runtime.instance().capture(&runtime.app.world)?.assets,
    )?;
    assets.load_pending()?;
    assets.require_ready()?;
    Ok(())
}
