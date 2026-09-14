use super::*;
use bozzard_assets::{AssetStore, job::Progress};
use bozzard_scene::{AssetKind, AssetSource, Prefab};
use std::{
    collections::BTreeMap,
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

/// A completed export waiting for publication. Dropping it removes only our staging folder.
pub struct PreparedExport {
    stage: PathBuf,
    destination: PathBuf,
}
impl PreparedExport {
    pub fn commit(self) -> Result<PathBuf> {
        ensure!(
            !self.destination.exists(),
            "export folder already exists; choose a new folder"
        );
        fs::rename(&self.stage, &self.destination).context("publishing exported game")?;
        Ok(self.destination.clone())
    }
}
impl Drop for PreparedExport {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.stage);
    }
}

/// Cook every declared asset and transitive prefab/model dependency, then validate the
/// relocated runtime before publishing. Never change the authored scene or overwrite a game.
pub fn prepare_export(
    project: &Project,
    scene: &Scene,
    source: &Path,
    player: &Path,
    destination: &Path,
    progress: &Progress,
) -> Result<PreparedExport> {
    project.validate_scene(scene)?;
    ensure!(
        player.is_file(),
        "player runtime is missing: {}",
        player.display()
    );
    let destination = std::path::absolute(destination)?;
    ensure!(
        !destination.exists(),
        "export folder already exists; choose a new folder"
    );
    let parent = destination
        .parent()
        .context("export folder needs a parent")?;
    fs::create_dir_all(parent)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stage = loop {
        let path = parent.join(format!(
            ".bozzard-export-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => break path,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    let prepared = PreparedExport { stage, destination };
    let (executable, data) = if cfg!(target_os = "macos") {
        (
            "Game.app/Contents/MacOS/Game",
            "Game.app/Contents/Resources/game",
        )
    } else if cfg!(windows) {
        ("Game.exe", ".")
    } else {
        ("Game", ".")
    };
    let data = prepared.stage.join(data);
    fs::create_dir_all(data.join("assets"))?;
    let mut cooker = Cooker {
        root: &data,
        packaged: BTreeMap::new(),
        progress,
    };
    let mut cooked = scene.clone();
    cooker.catalog(
        &mut cooked.assets,
        source.parent().unwrap_or(Path::new(".")),
        false,
    )?;
    for level in cooked.runtime_scenes.values_mut() {
        for (id, asset) in &mut std::sync::Arc::make_mut(level).assets {
            *asset = cooked.assets[id].clone();
        }
    }
    fs::write(data.join("scene.json"), cooked.to_json()?)?;
    let mut runtime_project = project.clone();
    runtime_project.start_scene = "scene.json".into();
    fs::write(
        data.join(MANIFEST),
        serde_json::to_vec_pretty(&runtime_project)?,
    )?;
    progress.stage("Validating exported scene and assets")?;
    let runtime =
        bozzard_demo::SceneDemo::new_with_prefabs(&cooked, Some(&data.join("scene.json")))?;
    let mut assets = AssetStore::new(&data, &runtime.instance().document().assets)?;
    assets.load_pending_with(progress)?;
    assets.require_ready()?;
    progress.stage("Copying native player")?;
    let binary = prepared.stage.join(executable);
    fs::create_dir_all(binary.parent().unwrap())?;
    fs::copy(player, &binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
    }
    if cfg!(target_os = "macos") {
        let name = xml(&project.name);
        fs::write(
            prepared.stage.join("Game.app/Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Game</string><key>CFBundleIdentifier</key><string>dev.bozzard.exported-game</string><key>CFBundleName</key><string>{name}</string><key>CFBundleDisplayName</key><string>{name}</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>CFBundleShortVersionString</key><string>0.1.0</string><key>NSHighResolutionCapable</key><true/></dict></plist>\n"
            ),
        )?;
    }
    let launch = if cfg!(target_os = "macos") {
        "Game.app"
    } else if cfg!(windows) {
        "Game.exe"
    } else {
        "Game"
    };
    fs::write(
        prepared.stage.join("README.txt"),
        format!(
            "{}\n\nOpen {launch} to play. No Rust, Python, or source checkout is required.\nKeep the exported folder's contents together. You can move or rename the folder.\n\nFirst Trail controls: physical WASD move; Space jumps; right mouse drag orbits.\nCollect all three gold crystals, jump the step, reach the blue checkpoint, then the green goal.\nProgress and YOU WIN appear in the window title. Physical R restarts; Escape quits.\nOther games can provide their own controls.\n\nBuilt for {} / {}. Requires compatible OS graphics drivers.\nThis local build is not signed for public distribution or notarized.\n",
            project.name,
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    )?;
    let mut files = BTreeMap::new();
    inventory(&prepared.stage, &prepared.stage, &mut files)?;
    fs::write(
        prepared.stage.join("package.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1, "name": project.name, "engine_version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "executable": executable, "files": files
        }))?,
    )?;
    progress.check()?;
    Ok(prepared)
}

struct Cooker<'a> {
    root: &'a Path,
    packaged: BTreeMap<(String, PathBuf), String>,
    progress: &'a Progress,
}
impl Cooker<'_> {
    fn catalog(
        &mut self,
        assets: &mut BTreeMap<String, AssetSource>,
        source: &Path,
        prefab: bool,
    ) -> Result<()> {
        for (id, asset) in assets {
            self.progress.stage(format!("Packaging asset {id}"))?;
            let path = self
                .asset(asset.kind, &source.join(&asset.path))
                .with_context(|| format!("packaging asset '{id}' ({})", asset.path))?;
            asset.path = if prefab {
                path.strip_prefix("assets/").unwrap().into()
            } else {
                path
            };
        }
        Ok(())
    }

    fn asset(&mut self, kind: AssetKind, source: &Path) -> Result<String> {
        let source = source.canonicalize()?;
        let key = (format!("{kind:?}"), source.clone());
        if let Some(path) = self.packaged.get(&key) {
            return Ok(path.clone());
        }
        ensure!(self.packaged.len() < 1024, "export exceeds 1024 assets");
        let index = self.packaged.len();
        let extension = source
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let relative = if kind == AssetKind::Mesh {
            format!(
                "assets/{index:04}/{}",
                source
                    .file_name()
                    .and_then(|n| n.to_str())
                    .context("model filename is not UTF-8")?
            )
        } else if kind == AssetKind::Prefab {
            format!("assets/{index:04}.prefab.json")
        } else {
            format!("assets/{index:04}.{extension}")
        };
        // Reserve before recursion so spawn-prefab cycles retain stable identities.
        self.packaged.insert(key, relative.clone());
        let destination = self.root.join(&relative);
        if kind == AssetKind::Prefab {
            let mut prefab = Prefab::from_json(&fs::read_to_string(&source)?)?;
            self.catalog(&mut prefab.assets, source.parent().unwrap(), true)?;
            fs::write(destination, prefab.to_json()?)?;
        } else if kind == AssetKind::Mesh {
            let package = bozzard_assets::package_model(&source, self.progress)?;
            let directory = destination.parent().unwrap();
            for (name, bytes) in package.files {
                let path = directory.join(name);
                fs::create_dir_all(path.parent().unwrap())?;
                fs::write(path, bytes)?;
            }
        } else {
            fs::copy(source, destination)?;
        }
        Ok(relative)
    }
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn inventory(root: &Path, directory: &Path, files: &mut BTreeMap<String, u64>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            inventory(root, &path, files)?;
        } else {
            files.insert(
                path.strip_prefix(root)?
                    .to_str()
                    .context("non-UTF8 package path")?
                    .replace('\\', "/"),
                fs::metadata(path)?.len(),
            );
        }
    }
    Ok(())
}
