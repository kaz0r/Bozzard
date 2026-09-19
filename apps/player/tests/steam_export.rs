#![cfg(feature = "steam")]
use bozzard_assets::job::Progress;
use bozzard_project::{Project, prepare_export};
use bozzard_scene::{FieldValue, Layer};
use std::{fs, path::PathBuf, process::Command};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("bozzard-steam-export-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_exports_include_verified_sdk_and_run_relocated_without_python_or_library_overrides() {
    let temp = Temp::new();
    let empty = temp.0.join("empty");
    fs::create_dir(&empty).unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/flap-woods-multiplayer.json");
    let mut scene = bozzard_demo::load_document(Some(&source)).unwrap();
    let project = Project {
        version: 1,
        name: "Together".into(),
        start_scene: "scene.json".into(),
        view: Layer::ThreeD,
        cook: Default::default(),
    };
    let player = PathBuf::from(env!("CARGO_BIN_EXE_bozzard-player"));
    for app_id in [480, 123456] {
        let settings = scene
            .objects
            .iter_mut()
            .find(|o| o.extra("steam_multiplayer").is_some())
            .unwrap();
        let row = bozzard_scene::component_type("steam_multiplayer").unwrap();
        (row.set)(settings, "app_id", FieldValue::Text(app_id.to_string())).unwrap();
        let destination = temp.0.join(format!("game-{app_id}"));
        // Exactly the same exporter called by the editor's background export job.
        prepare_export(
            &project,
            &scene,
            &source,
            &player,
            &destination,
            &Progress::default(),
        )
        .unwrap()
        .commit()
        .unwrap();
        let moved = temp.0.join(format!("Renamed game {app_id}"));
        fs::rename(destination, &moved).unwrap();
        let inventory: serde_json::Value =
            serde_json::from_slice(&fs::read(moved.join("package.json")).unwrap()).unwrap();
        let binary = moved.join(inventory["executable"].as_str().unwrap());
        let runtime: serde_json::Value =
            serde_json::from_slice(&fs::read(moved.join("steam-runtime.json")).unwrap()).unwrap();
        let library = runtime["library"].as_str().unwrap();
        assert!(inventory["files"].get(library).is_some());
        assert!(inventory["files"].get("steam-runtime.json").is_some());
        assert_eq!(runtime["app_id"], app_id);
        assert_eq!(
            runtime["mode"],
            if app_id == 480 {
                "spacewar-development"
            } else {
                "steam-store"
            }
        );
        assert_eq!(
            binary.parent().unwrap().join("steam_appid.txt").exists(),
            app_id == 480
        );
        assert!(
            !moved.join("play-steam.sh").exists(),
            "no development launch overrides in native exports"
        );
        for (name, size) in inventory["files"].as_object().unwrap() {
            assert_eq!(
                fs::metadata(moved.join(name)).unwrap().len(),
                size.as_u64().unwrap(),
                "{name}"
            );
        }
        let (name, bytes) = bozzard_demo::steam_runtime::redistributable().unwrap();
        assert_eq!(
            fs::read(binary.parent().unwrap().join(name)).unwrap(),
            bytes
        );
        let run = |arguments: &[&str]| {
            Command::new(&binary)
                .args(arguments)
                .current_dir(&empty)
                .env("PATH", &empty)
                .env_remove("LD_LIBRARY_PATH")
                .env_remove("DYLD_LIBRARY_PATH")
                .env_remove("SteamAppId")
                .env_remove("SteamGameId")
                .output()
                .unwrap()
        };
        let info = run(&["--runtime-info"]);
        assert!(info.status.success(), "{info:?}");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&info.stdout).unwrap(),
            bozzard_project::runtime::description()
        );
        let snapshot = empty.join("snapshot.json");
        let saved = run(&["--write-scene", snapshot.to_str().unwrap()]);
        assert!(saved.status.success(), "{saved:?}");
        assert_eq!(
            bozzard_demo::multiplayer::app_id(
                &bozzard_demo::load_document(Some(&snapshot)).unwrap()
            )
            .unwrap(),
            Some(app_id)
        );
    }
    // An unrelated executable must never produce a multiplayer package that merely looks valid.
    let invalid = temp.0.join("invalid");
    assert!(
        prepare_export(
            &project,
            &scene,
            &source,
            &std::env::current_exe().unwrap(),
            &invalid,
            &Progress::default()
        )
        .is_err()
    );
    assert!(!invalid.exists());
}
