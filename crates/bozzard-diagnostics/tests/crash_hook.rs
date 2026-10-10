//! The installed hook is process-wide, so this file holds a single test in its own binary.
use bozzard_diagnostics::crash;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn spawned_thread_panic_writes_a_bounded_report_then_runs_the_previous_hook() {
    let directory = std::env::temp_dir().join(format!("bozzard-crash-hook-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let previous = Arc::new(AtomicUsize::new(0));
    let calls = previous.clone();
    std::panic::set_hook(Box::new(move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
    }));
    let active = crash::install(crash::CrashConfig {
        app: "hook-test-before-rename".into(),
        version: "9.9.9".into(),
        git: Some("deadbeef".into()),
        directory: directory.clone(),
    });
    assert_eq!(active, directory);
    assert_eq!(crash::installed().unwrap().app, "hook-test-before-rename");
    crash::set_app_name("hook test");
    crash::set_app_name("ignored");
    crash::record("Warning", "Test", "before the panic");

    let joined = std::thread::Builder::new()
        .name("worker".into())
        .spawn(|| panic!("worker exploded"))
        .unwrap()
        .join();
    assert!(joined.is_err());
    assert_eq!(
        previous.load(Ordering::SeqCst),
        1,
        "default behaviour continues"
    );
    let reports = crash::reports(&directory).unwrap();
    assert_eq!(reports.len(), 1);
    let name = reports[0]
        .1
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with("crash-hook-test-") && name.ends_with(".txt"),
        "{name}"
    );
    let text = std::fs::read_to_string(&reports[0].1).unwrap();
    for expected in [
        "app: hook test\n",
        "version: 9.9.9\n",
        "git: deadbeef\n",
        "thread: worker (ThreadId(",
        "message: worker exploded\n",
        &format!("location: {}:", file!()),
        "\nbacktrace:\n",
        "Warning Test: before the panic\n",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }

    // A panic loop cannot fill the disk: later panics still reach the previous hook.
    for _ in 0..crash::MAX_REPORTS_PER_PROCESS + 2 {
        let _ = std::thread::spawn(|| panic!("again")).join();
    }
    assert_eq!(
        crash::reports(&directory).unwrap().len(),
        crash::MAX_REPORTS_PER_PROCESS as usize
    );
    assert_eq!(
        previous.load(Ordering::SeqCst),
        crash::MAX_REPORTS_PER_PROCESS as usize + 3
    );
    drop(std::panic::take_hook());
    std::fs::remove_dir_all(&directory).unwrap();
}
