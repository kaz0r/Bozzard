use std::process::Command;

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
