use super::*;
use bozzard_assets::{AssetStore, job::Progress};
use bozzard_scene::{AssetKind, AssetSource};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    sync::atomic::{AtomicU64, Ordering},
};

/// A completed export waiting for publication. Dropping it removes only our staging folder.
pub struct PreparedExport {
    stage: PathBuf,
    destination: PathBuf,
    report: CookReport,
}
impl PreparedExport {
    pub fn report(&self) -> CookReport {
        self.report
    }
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
    let steam_app_id = bozzard_demo::multiplayer::app_id(scene)?;
    crate::runtime::validate_player(player, steam_app_id)?;
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
    let mut prepared = PreparedExport {
        stage,
        destination,
        report: CookReport::default(),
    };
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
    let mut cooker = Cooker::new(
        &data,
        source
            .parent()
            .unwrap_or(Path::new("."))
            .join(".bozzard-cache/cook-v1"),
        project.cook,
        has_bake(scene),
        progress,
    );
    let mut runtime_project = project.clone();
    runtime_project.start_scene = "scene.json".into();
    runtime_project.cook = CookTarget::Source;
    fs::write(
        data.join(MANIFEST),
        serde_json::to_vec_pretty(&runtime_project)?,
    )?;
    cooker.scene(scene, source, "scene.json")?;
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
    crate::runtime::stage(&prepared.stage, &binary, steam_app_id)?;
    if let Some(id) = steam_app_id {
        fs::write(
            prepared.stage.join("STEAM-README.txt"),
            format!(
                "Steam multiplayer · App ID {id}\nKeep the complete exported folder together.\n{}\nCreate a lobby, invite friends, then only the host starts. Use Invite without overlay if needed.\n",
                if id == 480 {
                    "Spacewar development example. Start Steam, then open the game. Each player needs a separate Steam account."
                } else {
                    "Launch this game through its Steam library entry. Configure this executable in your Steam depot launch options. No development App ID file or environment overrides are included."
                }
            ),
        )?;
    }
    let mut files = BTreeMap::new();
    inventory(&prepared.stage, &prepared.stage, &mut files)?;
    fs::write(
        prepared.stage.join("package.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1, "name": project.name, "engine_version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "executable": executable, "files": files, "cook": project.cook
        }))?,
    )?;
    progress.check()?;
    prepared.report = cooker.cache.report;
    Ok(prepared)
}

pub(crate) struct Cooker<'a> {
    root: &'a Path,
    packaged: BTreeMap<(String, PathBuf), String>,
    progress: &'a Progress,
    cache: cook::Cache,
    target: CookTarget,
    fingerprints: BTreeMap<PathBuf, u64>,
    track_fingerprints: bool,
    scene_files: BTreeMap<PathBuf, String>,
    content_cache: PathBuf,
}
pub(crate) fn has_bake(scene: &Scene) -> bool {
    scene.gi.baked.is_some()
        || !scene.runtime_scene_sources.is_empty()
        || scene.runtime_scenes.values().any(|s| s.gi.baked.is_some())
}
impl<'a> Cooker<'a> {
    pub(crate) fn new(
        root: &'a Path,
        cache_root: PathBuf,
        target: CookTarget,
        track_fingerprints: bool,
        progress: &'a Progress,
    ) -> Self {
        Self {
            root,
            packaged: BTreeMap::new(),
            progress,
            content_cache: cache_root.join("scene-content"),
            cache: cook::Cache::new(cache_root, target),
            target,
            fingerprints: BTreeMap::new(),
            track_fingerprints,
            scene_files: BTreeMap::new(),
        }
    }
}
impl Cooker<'_> {
    pub(crate) fn report(&self) -> CookReport {
        self.cache.report
    }
    pub(crate) fn scene(&mut self, scene: &Scene, source: &Path, filename: &str) -> Result<()> {
        scene.validate()?;
        let source_key = source
            .canonicalize()
            .or_else(|_| std::path::absolute(source))?;
        if let Some(existing) = self.scene_files.get(&source_key)
            && existing != filename
        {
            fs::copy(self.root.join(existing), self.root.join(filename))?;
            return Ok(());
        }
        ensure!(
            self.scene_files.contains_key(&source_key) || self.scene_files.len() < 64,
            "export exceeds 64 scene files"
        );
        self.scene_files.insert(source_key, filename.into());
        let data = self.root;
        let progress = self.progress;
        let mut cooked = scene.clone();
        self.catalog(
            &mut cooked.assets,
            source.parent().unwrap_or(Path::new(".")),
            false,
        )?;
        for level in cooked.runtime_scenes.values_mut() {
            for (id, asset) in &mut std::sync::Arc::make_mut(level).assets {
                *asset = cooked.assets[id].clone();
            }
        }
        self.scene_sources(&mut cooked, source.parent().unwrap_or(Path::new(".")))?;
        progress.stage("Validating exported scene and assets")?;
        let runtime =
            bozzard_demo::SceneDemo::new_with_prefabs(&cooked, Some(&data.join(filename)))?;
        let mut assets = AssetStore::new(data, &runtime.instance().document().assets)?;
        assets.load_pending_with(progress)?;
        assets.require_ready()?;
        assets.validate_scene_resources(runtime.instance().document())?;
        if self.target != CookTarget::Source {
            rebind_gi(
                scene,
                source,
                &mut cooked,
                runtime.instance().document(),
                &assets,
                &self.fingerprints,
                progress,
            )?;
            for (id, level) in &mut cooked.runtime_scenes {
                if level.gi.baked.is_none() {
                    continue;
                }
                let runtime =
                    bozzard_demo::SceneDemo::new_with_prefabs(level, Some(&data.join(filename)))?;
                let mut level_assets =
                    assets.for_catalog(data, &runtime.instance().document().assets)?;
                level_assets.load_pending_with(progress)?;
                level_assets.require_ready()?;
                rebind_gi(
                    &scene.runtime_scenes[id],
                    source,
                    std::sync::Arc::make_mut(level),
                    runtime.instance().document(),
                    &level_assets,
                    &self.fingerprints,
                    progress,
                )?;
            }
        }
        assets.bake_audio_metadata(&mut cooked)?;
        fs::write(data.join(filename), cooked.to_json()?)?;
        Ok(())
    }
    fn scene_sources(&mut self, scene: &mut Scene, source: &Path) -> Result<()> {
        use bozzard_scene::scene_loading::SceneSource;
        for input in scene.runtime_scene_sources.values_mut() {
            self.progress.stage("Packaging runtime scene source")?;
            let path =
                match input {
                    SceneSource::File { path } => source.join(path),
                    SceneSource::Content { catalog, address } => {
                        // Remote catalogs intentionally remain downloadable at runtime.
                        if catalog.starts_with("https://") || catalog.starts_with("http://") {
                            continue;
                        }
                        let catalog_path = source.join(catalog);
                        let catalog = crate::content::load_catalog(
                            catalog_path.to_str().context("catalog path is not UTF-8")?,
                            self.progress,
                        )?;
                        let resolved = crate::content::ContentStore::new(&self.content_cache)
                            .resolve(&catalog, address, self.progress)?;
                        resolved.scene_path()?
                    }
                }
                .canonicalize()?;
            let filename = if let Some(filename) = self.scene_files.get(&path) {
                filename.clone()
            } else {
                ensure!(self.scene_files.len() < 64, "export exceeds 64 scene files");
                let filename = format!("scene-input-{:04}.json", self.scene_files.len());
                let mut json = String::new();
                fs::File::open(&path)?
                    .take(64 * 1024 * 1024 + 1)
                    .read_to_string(&mut json)?;
                ensure!(
                    json.len() <= 64 * 1024 * 1024,
                    "runtime scene exceeds 64 MiB"
                );
                let document = Scene::from_json(&json)?;
                self.scene(&document, &path, &filename)?;
                filename
            };
            *input = SceneSource::File { path: filename };
        }
        Ok(())
    }
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

    pub(crate) fn asset(&mut self, kind: AssetKind, source: &Path) -> Result<String> {
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
        let relative = if let Some(extension) = self.target.extension(kind) {
            format!("assets/{index:04}.{extension}")
        } else if kind == AssetKind::Material {
            format!("assets/{index:04}/material.material.json")
        } else if kind == AssetKind::Mesh {
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
        if self.target.extension(kind).is_some() {
            let input = bozzard_assets::CookSource::read(kind, &source, self.progress)?;
            let bytes = self.cache.cook(&input, self.progress)?;
            if self.track_fingerprints {
                self.fingerprints
                    .insert(source, input.content_fingerprint());
            }
            fs::write(destination, bytes)?;
        } else if kind == AssetKind::Prefab {
            let mut prefab = bozzard_demo::load_prefab(&source, self.progress)?.prefab;
            self.catalog(&mut prefab.assets, source.parent().unwrap(), true)?;
            fs::write(destination, prefab.to_json()?)?;
        } else if kind == AssetKind::Material {
            let package =
                bozzard_assets::materials::package_with(&source, self.progress, |name, bytes| {
                    if let Some(extension) = self.target.extension(AssetKind::Image) {
                        let input = bozzard_assets::CookSource::image_bytes(&name, bytes)?;
                        let cooked = self.cache.cook(&input, self.progress)?;
                        let name = Path::new(&name)
                            .with_extension(extension)
                            .to_str()
                            .unwrap()
                            .to_owned();
                        Ok((name, cooked))
                    } else {
                        Ok((name, bytes))
                    }
                })?;
            if self.track_fingerprints {
                self.fingerprints
                    .insert(source, package.content_fingerprint);
            }
            let directory = destination.parent().unwrap();
            fs::create_dir_all(directory)?;
            for (name, bytes) in package.source.files {
                fs::write(directory.join(name), bytes)?;
            }
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
        if self.target.extension(kind).is_none() {
            self.cache.report.copied += 1;
        }
        Ok(relative)
    }
}

// Cooking is lossless for CPU geometry/materials/pixels used by GI. Rebind only a
// previously current bake, after checking that source bytes did not change during
// cooking. Stale authoring bakes remain stale; no source document is modified.
fn rebind_gi(
    original: &Scene,
    source: &Path,
    cooked: &mut Scene,
    expanded: &Scene,
    assets: &AssetStore,
    fingerprints: &BTreeMap<PathBuf, u64>,
    progress: &Progress,
) -> Result<()> {
    if original.gi.baked.is_none() {
        return Ok(());
    }
    progress.stage("Checking baked lighting against cooking inputs")?;
    let before = bozzard_demo::SceneDemo::new_with_prefabs(original, Some(source))?;
    let before = before.instance().document();
    let root = source.parent().unwrap_or(Path::new("."));
    let mut originals = AssetStore::new(root, &before.assets)?;
    originals.load_pending_with(progress)?;
    originals.require_ready()?;
    for (id, asset) in &before.assets {
        if asset.kind == AssetKind::Prefab {
            continue;
        }
        let entry = originals
            .get(originals.handle(id).context("original GI asset")?)
            .unwrap();
        let expected =
            if let Some(fingerprint) = fingerprints.get(&root.join(&asset.path).canonicalize()?) {
                Some(*fingerprint)
            } else {
                assets
                    .get(assets.handle(id).context("exported GI asset")?)
                    .unwrap()
                    .content_fingerprint()
            };
        ensure!(
            entry.content_fingerprint() == expected,
            "asset '{id}' changed while exporting; retry export"
        );
    }
    if bozzard_assets::gi::is_current(before, &originals)? {
        std::sync::Arc::make_mut(cooked.gi.baked.as_mut().context("cooked GI bake")?).source =
            bozzard_assets::gi::source(expanded, assets, cooked.gi.volume)?;
    }
    Ok(())
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
