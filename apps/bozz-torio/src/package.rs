//! A game-specific export that keeps the native factory runtime and its scene together.
use crate::{module::NAME, scene::SceneSource};
use anyhow::{Context, Result, ensure};
use bozzard_project::{CookTarget, Project};
use bozzard_scene::{Layer, Scene};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

fn packaged_asset_path(staging: &Path, relative: &str) -> Result<PathBuf> {
    let mut parts = vec!["scene".to_owned()];
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
            Component::ParentDir if !parts.is_empty() => {
                parts.pop();
            }
            _ => anyhow::bail!("scene asset path escapes export: {relative}"),
        }
    }
    ensure!(!parts.is_empty(), "scene asset path is empty: {relative}");
    Ok(parts
        .into_iter()
        .fold(staging.to_path_buf(), |path, part| path.join(part)))
}

pub fn companion_factory(editor: &Path) -> Result<PathBuf> {
    let directory = editor
        .parent()
        .context("editor executable has no directory")?;
    let binary = directory.join(if cfg!(windows) {
        "bozz-torio.exe"
    } else {
        "bozz-torio"
    });
    ensure!(
        binary.is_file(),
        "Bozz-torio runtime missing beside the editor. Build bozz-torio in the same Cargo profile before exporting."
    );
    Ok(binary)
}

pub fn export_scene(
    scene: &Scene,
    source: &Path,
    binary: &Path,
    destination: &Path,
) -> Result<PathBuf> {
    export_scene_with_progress(scene, source, binary, destination, &Default::default())
}

pub fn export_scene_with_progress(
    scene: &Scene,
    source: &Path,
    binary: &Path,
    destination: &Path,
    progress: &bozzard_app::job::Progress,
) -> Result<PathBuf> {
    progress.check()?;
    ensure!(
        !destination.exists(),
        "export destination already exists: {}",
        destination.display()
    );
    SceneSource::from_scene(source.to_path_buf(), scene)?;
    ensure!(
        binary.is_file(),
        "missing Bozz-torio runtime: {}",
        binary.display()
    );
    let parent = destination
        .parent()
        .context("export destination has no parent")?;
    ensure!(
        parent.is_dir(),
        "export parent is missing: {}",
        parent.display()
    );
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let staging = parent.join(format!(
        ".bozz-torio-export-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&staging)?;
    let result = (|| -> Result<()> {
        progress.check()?;
        let executable = binary.file_name().context("runtime has no filename")?;
        fs::copy(binary, staging.join(executable))?;
        let library = if cfg!(windows) {
            "steam_api64.dll"
        } else if cfg!(target_os = "macos") {
            "libsteam_api.dylib"
        } else {
            "libsteam_api.so"
        };
        let sdk = binary.parent().unwrap().join(library);
        ensure!(
            sdk.is_file(),
            "Steam runtime library missing beside {}: {library}",
            binary.display()
        );
        fs::copy(sdk, staging.join(library))?;
        let appid = binary.parent().unwrap().join("steam_appid.txt");
        if appid.is_file() {
            fs::copy(appid, staging.join("steam_appid.txt"))?;
        } else {
            fs::write(staging.join("steam_appid.txt"), "480\n")?;
        }
        let scene_dir = staging.join("scene");
        fs::create_dir(&scene_dir)?;
        let source_dir = source.parent().context("scene source has no parent")?;
        for (id, asset) in &scene.assets {
            progress.check()?;
            let input = source_dir
                .join(&asset.path)
                .canonicalize()
                .with_context(|| format!("missing scene asset '{id}' at {}", asset.path))?;
            ensure!(input.is_file(), "scene asset '{id}' is not a file");
            let output = packaged_asset_path(&staging, &asset.path)?;
            let folder = output.parent().context("asset has no export parent")?;
            fs::create_dir_all(folder)?;
            fs::copy(input, output)?;
        }
        let manifest = source_dir.join("../assets/sprite_manifest.json");
        if manifest.is_file() {
            fs::create_dir_all(staging.join("assets"))?;
            fs::copy(manifest, staging.join("assets/sprite_manifest.json"))?;
        }
        fs::write(scene_dir.join("bozz-torio.json"), scene.to_json()?)?;
        let project = Project {
            version: 1,
            name: scene.name.clone(),
            start_scene: "scene/bozz-torio.json".into(),
            view: Layer::TwoD,
            runtime_modules: vec![NAME.into()],
            cook: CookTarget::Source,
        };
        project.validate_scene(scene)?;
        fs::write(
            staging.join("bozzard.project.json"),
            serde_json::to_vec_pretty(&project)?,
        )?;
        progress.check()?;
        fs::rename(&staging, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(destination.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_export_contains_a_relocatable_factory_scene_and_required_module() {
        let source = SceneSource::default_path();
        let scene = SceneSource::open(source.clone()).unwrap().authored;
        let root = std::env::temp_dir().join(format!(
            "bozz-torio-export-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let binary = root.join(if cfg!(windows) {
            "bozz-torio.exe"
        } else {
            "bozz-torio"
        });
        fs::write(&binary, b"test runtime").unwrap();
        let library = if cfg!(windows) {
            "steam_api64.dll"
        } else if cfg!(target_os = "macos") {
            "libsteam_api.dylib"
        } else {
            "libsteam_api.so"
        };
        fs::write(root.join(library), b"test sdk").unwrap();
        let exported = export_scene(&scene, &source, &binary, &root.join("unrelated/factory"));
        assert!(exported.is_err(), "export parent must exist");
        let output = root.join("factory");
        export_scene(&scene, &source, &binary, &output).unwrap();
        let relocated = SceneSource::open(output.join("scene/bozz-torio.json")).unwrap();
        let relocated_game = relocated.new_game_from_seed(7).unwrap();
        crate::stage::Stage::new(&relocated, &relocated_game).unwrap();
        assert_eq!(
            relocated_game.hub,
            SceneSource::open(source)
                .unwrap()
                .new_game_from_seed(7)
                .unwrap()
                .hub
        );
        let (project, _) = Project::load(&output.join("bozzard.project.json")).unwrap();
        assert_eq!(project.runtime_modules, [NAME]);
        assert!(project.require_runtime_modules(&[]).is_err());
        assert!(project.require_runtime_modules(&[NAME]).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
