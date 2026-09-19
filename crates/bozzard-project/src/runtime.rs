//! Native runtime capabilities and SDK files shared by editor and command-line export.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

pub fn description() -> serde_json::Value {
    let library = bozzard_demo::steam_runtime::redistributable().map(|(name, bytes)| {
        serde_json::json!({"name": name, "sha256": format!("{:x}", Sha256::digest(bytes))})
    });
    serde_json::json!({
        "version": 1, "engine_version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "steam_library": library
    })
}

pub(crate) fn validate_player(player: &Path, app_id: Option<u32>) -> Result<()> {
    if app_id.is_none() {
        return Ok(());
    }
    ensure!(
        bozzard_demo::steam_runtime::redistributable().is_some(),
        "Steam export requires the standard Steam-enabled editor/player build"
    );
    let output = Command::new(player)
        .arg("--runtime-info")
        .output()
        .context(
            "Checking the companion player's Steam support; rebuild the editor and player together",
        )?;
    ensure!(
        output.status.success(),
        "The companion player cannot report its runtime: {}. Rebuild bozzard-player with default features.",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("Companion player is outdated; rebuild bozzard-player")?;
    ensure!(
        actual == description(),
        "The companion player has different Steam support, SDK or target. Build the editor and player together with default features."
    );
    Ok(())
}

pub(crate) fn stage(root: &Path, executable: &Path, app_id: Option<u32>) -> Result<()> {
    let Some((name, bytes)) = bozzard_demo::steam_runtime::redistributable() else {
        return Ok(());
    };
    let directory = executable
        .parent()
        .context("missing executable directory")?;
    let library = directory.join(name);
    fs::write(&library, bytes).context("Bundling Steam API runtime")?;
    if app_id == Some(480) {
        fs::write(directory.join("steam_appid.txt"), "480\n")?;
    }
    fs::write(
        root.join("steam-runtime.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "app_id": app_id,
            "mode": match app_id { Some(480) => "spacewar-development", Some(_) => "steam-store", None => "runtime-only" },
            "library": library.strip_prefix(root)?.to_string_lossy().replace('\\', "/"),
            "sha256": format!("{:x}", Sha256::digest(bytes))
        }))?,
    )?;
    Ok(())
}
