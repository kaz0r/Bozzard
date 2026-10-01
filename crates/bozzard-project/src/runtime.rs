//! Native runtime capabilities and SDK files shared by editor and command-line export.
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

// Bump when adding/changing native script APIs used by exported game scripts.
// The engine package version alone stays fixed during development.
pub const SCRIPT_API_VERSION: u32 = 1;

pub fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "development"
    } else {
        "release"
    }
}

/// A Cargo development editor can export with its optimized sibling runtime.
/// Installed bundles continue to use their adjacent companion.
pub fn release_companion(editor: &Path, name: &str) -> Option<PathBuf> {
    let directory = editor.parent()?;
    (directory.file_name()? == "debug")
        .then(|| {
            directory
                .parent()
                .map(|root| root.join("release").join(name))
        })
        .flatten()
}

/// Build profile affects performance, but not the serialized runtime contract.
pub fn compatible(mut actual: serde_json::Value, mut expected: serde_json::Value) -> bool {
    for value in [&mut actual, &mut expected] {
        if let Some(object) = value.as_object_mut() {
            object.remove("build_profile");
        }
    }
    actual == expected
}

pub fn description() -> serde_json::Value {
    let library = bozzard_demo::steam_runtime::redistributable().map(|(name, bytes)| {
        serde_json::json!({"name": name, "sha256": format!("{:x}", Sha256::digest(bytes))})
    });
    serde_json::json!({
        "version": 1, "engine_version": env!("CARGO_PKG_VERSION"), "gamepack_version": 1,
        "script_api_version": SCRIPT_API_VERSION,
        "build_profile": build_profile(),
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "steam_library": library
    })
}

pub(crate) fn validate_player(player: &Path, app_id: Option<u32>) -> Result<()> {
    ensure!(
        app_id.is_none() || bozzard_demo::steam_runtime::redistributable().is_some(),
        "Steam export requires the standard Steam-enabled editor/player build"
    );
    let output = Command::new(player)
        .arg("--runtime-info")
        .output()
        .context(
            "Checking the companion player's gamepack support; rebuild the editor and player together",
        )?;
    ensure!(
        output.status.success(),
        "The companion player cannot report its runtime: {}. Rebuild bozzard-player with default features.",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("Companion player is outdated; rebuild bozzard-player")?;
    ensure!(
        compatible(actual, description()),
        "The companion player has different gamepack support, Steam SDK or target. Build the editor and player together."
    );
    Ok(())
}

/// Prefer a verified release player at export time; probe at most once per
/// export rather than launching a process on every editor UI frame.
pub fn export_player(editor: &Path) -> Result<PathBuf> {
    select_player(editor, |path| {
        let output = Command::new(path).arg("--runtime-info").output()?;
        let actual: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        Ok(output.status.success()
            && actual["build_profile"] == "release"
            && compatible(actual, description()))
    })
}

fn select_player(
    editor: &Path,
    is_compatible_release: impl Fn(&Path) -> Result<bool>,
) -> Result<PathBuf> {
    let name = if cfg!(windows) {
        "bozzard-player.exe"
    } else {
        "bozzard-player"
    };
    if let Some(release) = release_companion(editor, name)
        && release.is_file()
        && is_compatible_release(&release).unwrap_or(false)
    {
        return Ok(release);
    }
    crate::companion_player(editor)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_does_not_hide_runtime_incompatibilities() {
        let expected = description();
        let mut actual = expected.clone();
        actual["build_profile"] = "release".into();
        assert!(compatible(actual.clone(), expected.clone()));
        actual["gamepack_version"] = 999.into();
        assert!(!compatible(actual, expected));
        let mut actual = description();
        actual["script_api_version"] = 999.into();
        assert!(!compatible(actual, description()));
    }

    #[test]
    fn development_exports_prefer_only_a_verified_release_sibling() {
        let root =
            std::env::temp_dir().join(format!("bozzard-companion-test-{}", std::process::id()));
        let name = if cfg!(windows) {
            "bozzard-player.exe"
        } else {
            "bozzard-player"
        };
        let debug = root.join("debug");
        let release = root.join("release");
        fs::create_dir_all(&debug).unwrap();
        fs::create_dir_all(&release).unwrap();
        let editor = debug.join("bozzard-editor");
        fs::write(debug.join(name), "development").unwrap();
        fs::write(release.join(name), "release").unwrap();
        assert_eq!(
            select_player(&editor, |_| Ok(true)).unwrap(),
            release.join(name)
        );
        assert_eq!(
            select_player(&editor, |_| Ok(false)).unwrap(),
            debug.join(name)
        );
        assert_eq!(
            select_player(&editor, |_| anyhow::bail!("old runtime")).unwrap(),
            debug.join(name)
        );
        assert!(release_companion(&release.join("bozzard-editor"), name).is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
