use std::{fs, path::Path, process::Command};

#[test]
fn headless_binary_runs_simulation_and_rejects_bad_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_bozzard-server"))
        .args(["--ticks", "120"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("headless_ok ticks=120 entities=10"),
        "{stdout}"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_bozzard-server"))
        .args(["--ticks", "wrong"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn relocated_server_loads_adjacent_libraries_without_cargo_environment() {
    let root =
        std::env::temp_dir().join(format!("bozzard-server-relocated-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let package = root.join("Renamed package");
    let empty = root.join("empty-working-directory");
    fs::create_dir(&package).unwrap();
    fs::create_dir(&empty).unwrap();
    let source = Path::new(env!("CARGO_BIN_EXE_bozzard-server"));
    let executable = package.join(source.file_name().unwrap());
    fs::copy(source, &executable).unwrap();
    // Workspace builds can enable Steam on the server's shared dependencies.
    // Cargo's normal test environment masks missing loader paths, so copy the
    // same adjacent SDK as the packager and remove those environment overrides.
    for library in ["libsteam_api.so", "libsteam_api.dylib", "steam_api64.dll"] {
        let source = source.parent().unwrap().join(library);
        if source.is_file() {
            fs::copy(source, package.join(library)).unwrap();
        }
    }
    let result = Command::new(&executable)
        .args(["--ticks", "120"])
        .current_dir(&empty)
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("DYLD_LIBRARY_PATH")
        .env_remove("DYLD_FALLBACK_LIBRARY_PATH")
        .output();
    fs::remove_dir_all(&root).unwrap();
    let output = result.unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("headless_ok ticks=120 entities=10")
    );
}

#[test]
fn script_log_lines_reach_the_terminal() {
    let root = std::env::temp_dir().join(format!("bozzard-server-log-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("speaker.rhai"),
        r#"fn on_update(me, dt) { print("hello from a script"); log_warning("mind the gap"); }"#,
    )
    .unwrap();
    fs::write(
        root.join("scene.json"),
        r#"{"version":1,"name":"Server log","views":{},
            "assets":{"speaker":{"kind":"script","path":"speaker.rhai"}},
            "objects":[{"id":"speaker","name":"Speaker",
                "transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                "script_manager":{"scripts":[{"enabled":true,"script":"speaker"}]}}]}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bozzard-server"))
        .args(["--ticks", "2", "--scene"])
        .arg(root.join("scene.json"))
        .output();
    fs::remove_dir_all(&root).unwrap();
    let output = output.unwrap();
    assert!(output.status.success(), "{output:?}");
    let (stdout, stderr) = (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    );
    // print() is one line on stdout per call; warnings go to stderr with their source.
    assert_eq!(
        stdout.matches("hello from a script\n").count(),
        2,
        "{stdout}"
    );
    assert!(!stdout.contains("mind the gap"), "{stdout}");
    assert_eq!(
        stderr.matches("warning: Script: mind the gap\n").count(),
        2,
        "{stderr}"
    );
}
