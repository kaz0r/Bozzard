//! Native user-game projects and relocatable exports shared by editor and player CLI.
pub mod content;
mod cook;
mod export;
pub mod runtime;
pub mod streaming;
pub use cook::{CookReport, CookTarget};
mod merge;
use anyhow::{Context, Result, ensure};
use bozzard_scene::{Layer, Scene};
pub use export::{PreparedExport, prepare_export};
pub use merge::{MergeConflict, SceneMerge, merge_scenes};
mod templates;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
pub use templates::{ProjectTemplate, create_project};

pub const MANIFEST: &str = "bozzard.project.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub version: u32,
    pub name: String,
    /// Relative to the manifest, independent of the process working directory.
    pub start_scene: String,
    pub view: Layer,
    /// Compiled-in gameplay modules required by the runtime, in dependency order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_modules: Vec<String>,
    /// Offline representation used when exporting. Existing manifests keep source assets.
    #[serde(default, skip_serializing_if = "CookTarget::is_source")]
    pub cook: CookTarget,
}

impl Project {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported project version");
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 120
                && !self.name.chars().any(char::is_control),
            "project name must contain 1..120 bytes without control characters"
        );
        ensure!(
            !self.start_scene.is_empty()
                && !self.start_scene.contains(['\\', ':'])
                && Path::new(&self.start_scene)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "start_scene must be a relative path inside the project"
        );
        ensure!(
            self.runtime_modules.len() <= 16,
            "project has too many runtime modules"
        );
        let mut names = std::collections::BTreeSet::new();
        for name in &self.runtime_modules {
            ensure!(
                !name.is_empty()
                    && name.len() <= 128
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    && names.insert(name),
                "invalid or duplicate runtime module '{name}'"
            );
        }
        Ok(())
    }
    pub fn require_runtime_modules(&self, available: &[&str]) -> Result<()> {
        for name in &self.runtime_modules {
            ensure!(
                available.contains(&name.as_str()),
                "Missing runtime module '{name}'. Open this project with its game-specific editor or executable."
            );
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<(Self, PathBuf)> {
        let project: Self = serde_json::from_slice(
            &std::fs::read(path).with_context(|| format!("reading project {}", path.display()))?,
        )?;
        project.validate()?;
        let root = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let scene = root
            .join(&project.start_scene)
            .canonicalize()
            .with_context(|| format!("opening starting scene '{}'", project.start_scene))?;
        ensure!(
            scene.starts_with(&root),
            "starting scene resolves outside the project"
        );
        Ok((project, scene))
    }

    pub fn validate_scene(&self, scene: &Scene) -> Result<()> {
        self.validate()?;
        scene.validate()?;
        ensure!(
            scene.views.contains_key(&self.view),
            "starting scene has no {:?} camera",
            self.view
        );
        Ok(())
    }
}

/// Only inspect the executable's package locations; never discover a project in cwd.
pub fn bundled_project(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    let adjacent = directory.join(MANIFEST);
    if adjacent.exists() || directory.join("package.json").exists() {
        return Some(adjacent);
    }
    if directory.file_name().is_some_and(|n| n == "MacOS") {
        let resources = directory.parent()?.join("Resources/game").join(MANIFEST);
        if resources.parent().is_some_and(Path::is_dir) {
            return Some(resources);
        }
    }
    None
}

/// Locate a same-build runtime beside a development editor or in its development bundle.
pub fn companion_player(editor: &Path) -> Result<PathBuf> {
    let directory = editor
        .parent()
        .context("editor executable has no directory")?;
    let candidate = directory.join(if cfg!(windows) {
        "bozzard-player.exe"
    } else {
        "bozzard-player"
    });
    if candidate.is_file() {
        return Ok(candidate);
    }
    if cfg!(target_os = "macos")
        && let Some(bundle_root) = directory.ancestors().nth(3)
    {
        let candidate = bundle_root.join("Bozzard.app/Contents/MacOS/bozzard-player");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "Player runtime missing. Install the full native editor bundle, or build bozzard-player in the same profile as the editor."
    )
}

#[cfg(test)]
mod module_tests {
    use super::*;
    #[test]
    fn project_reports_missing_compiled_runtime_before_launch_or_export() {
        let project = Project {
            version: 1,
            name: "Factory".into(),
            start_scene: "scene.json".into(),
            view: Layer::TwoD,
            cook: Default::default(),
            runtime_modules: vec!["bozz-torio".into()],
        };
        project.validate().unwrap();
        assert!(
            project
                .require_runtime_modules(&[])
                .unwrap_err()
                .to_string()
                .contains("bozz-torio")
        );
        project.require_runtime_modules(&["bozz-torio"]).unwrap();
        let mut duplicate = project;
        duplicate.runtime_modules.push("bozz-torio".into());
        assert!(duplicate.validate().is_err());
    }
}
