use bozzard_scene::Scene;
use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bozzard-scene-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_roundtrip_overwrites_atomically_without_a_gpu_and_preserves_valid_file_on_error() {
    let dir = Temp::new();
    let saved = dir.0.join("scene.json");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_bozzard-player"))
            .args(args)
            .current_dir(&dir.0)
            .output()
            .unwrap()
    };
    let first = run(&["--write-scene", saved.to_str().unwrap()]);
    assert!(first.status.success(), "{first:?}");
    let original = std::fs::read_to_string(&saved).unwrap();
    let doc = Scene::from_json(&original).unwrap();
    assert_eq!(doc.objects.len(), 10);
    let overwrite = run(&[
        "--scene",
        saved.to_str().unwrap(),
        "--write-scene",
        saved.to_str().unwrap(),
    ]);
    assert!(overwrite.status.success(), "{overwrite:?}");
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), original);
    std::fs::write(dir.0.join("invalid.json"), "{invalid}").unwrap();
    let invalid = run(&[
        "--scene",
        "invalid.json",
        "--write-scene",
        saved.to_str().unwrap(),
    ]);
    assert!(!invalid.status.success());
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), original);
    assert!(
        std::fs::read_dir(&dir.0).unwrap().all(|p| !p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
}

#[test]
fn rejects_ambiguous_command_modes_before_opening_a_window() {
    let output = Command::new(env!("CARGO_BIN_EXE_bozzard-player"))
        .args(["--smoke", "--write-scene", "unused.json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("standalone command"));
}
