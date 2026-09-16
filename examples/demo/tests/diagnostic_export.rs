#[test]
fn failed_streaming_capture_keeps_the_existing_file() {
    use std::io::Write;
    let path =
        std::env::temp_dir().join(format!("bozzard-capture-test-{}.json", std::process::id()));
    std::fs::write(&path, "previous capture").unwrap();
    let error = bozzard_demo::save_atomic(&path, |file| {
        file.write_all(b"unfinished new capture")?;
        anyhow::bail!("serialization failed");
    });
    assert!(error.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous capture");
    bozzard_demo::save_atomic(&path, |file| {
        serde_json::to_writer(file, &serde_json::json!({"version":1}))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"version\":1}");
    std::fs::remove_file(path).unwrap();
}
