//! Native SDK staging must work before unrelated workspace crates have been downloaded.
#[allow(dead_code)]
#[path = "../../../tools/steam_build.rs"]
mod build_script;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "bozzard-sdk-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("{error}"),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cached(
    home: &Path,
    registry: &str,
    version: &str,
    folder: &str,
    name: &str,
    bytes: &[u8],
) -> PathBuf {
    let path = home.join("registry/src").join(registry).join(format!(
        "steamworks-sys-{version}/lib/steam/redistributable_bin/{folder}/{name}"
    ));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path.canonicalize().unwrap()
}

#[test]
fn resolves_only_locked_sdk_and_refuses_conflicting_caches() {
    let temp = Temp::new();
    let version = build_script::locked_sys_version().unwrap();
    cached(
        &temp.0,
        "older-index",
        "0.0.0",
        "linux64",
        "libsteam_api.so",
        b"old SDK",
    );
    let selected = cached(
        &temp.0,
        "a-index",
        version,
        "linux64",
        "libsteam_api.so",
        b"current SDK",
    );
    assert_eq!(
        build_script::find_library(None, Some(&temp.0), "linux64", "libsteam_api.so").unwrap(),
        selected
    );
    cached(
        &temp.0,
        "b-index",
        version,
        "linux64",
        "libsteam_api.so",
        b"current SDK",
    );
    assert_eq!(
        build_script::find_library(None, Some(&temp.0), "linux64", "libsteam_api.so").unwrap(),
        selected
    );
    cached(
        &temp.0,
        "b-index",
        version,
        "linux64",
        "libsteam_api.so",
        b"different SDK",
    );
    assert!(
        build_script::find_library(None, Some(&temp.0), "linux64", "libsteam_api.so")
            .unwrap_err()
            .to_string()
            .contains("Conflicting")
    );
    assert!(
        build_script::find_library(
            None,
            Some(&temp.0.join("absent")),
            "linux64",
            "libsteam_api.so"
        )
        .unwrap_err()
        .to_string()
        .contains("STEAM_SDK_LOCATION")
    );
    let sdk = selected
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    assert_eq!(
        build_script::find_library(Some(sdk), None, "linux64", "libsteam_api.so").unwrap(),
        selected
    );
}

#[test]
fn actual_build_script_stages_sdk_with_minimal_cargo_home_and_no_other_crates() {
    let temp = Temp::new();
    let home = temp.0.join("cargo home");
    let (folder, library) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => ("win64", "steam_api64.dll"),
        ("macos", _) => ("osx", "libsteam_api.dylib"),
        ("linux", "aarch64") => ("linuxarm64", "libsteam_api.so"),
        ("linux", "x86_64") => ("linux64", "libsteam_api.so"),
        target => panic!("unsupported Steam test target: {target:?}"),
    };
    let bytes = b"SDK staging fixture; no actual native API calls";
    cached(
        &home,
        "fixture-index",
        build_script::locked_sys_version().unwrap(),
        folder,
        library,
        bytes,
    );
    let project = temp.0.join("minimal project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/lib.rs"), "").unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/steam_build.rs")
        .canonicalize()
        .unwrap();
    fs::write(project.join("Cargo.toml"), format!(
        "[package]\nname = \"steam-staging-fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\nbuild = {}\n[features]\ndefault = [\"steam\"]\nsteam = []\n",
        serde_json::to_string(&script.to_string_lossy()).unwrap()
    )).unwrap();
    let target = temp.0.join("target");
    // A real Cargo build with no registry index or cached packages other than the SDK fixture.
    // The shared script needs only std; resolving any workspace/package metadata would fail.
    let output = Command::new(env!("CARGO"))
        .args(["check", "--offline"])
        .arg("--target-dir")
        .arg(&target)
        .current_dir(&project)
        .env("CARGO_HOME", &home)
        .env_remove("STEAM_SDK_LOCATION")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for directory in [target.join("debug"), target.join("debug/deps")] {
        assert_eq!(fs::read(directory.join(library)).unwrap(), bytes);
    }
    assert!(!home.join("registry/src/fixture-index/ctrlc-3.5.2").exists());
}
