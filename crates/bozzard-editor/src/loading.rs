use super::*;
use bozzard_assets::job::{Job, Progress};

pub struct LoadedScene {
    pub(super) scene: Scene,
    pub(super) path: PathBuf,
    pub(super) assets: AssetStore,
    pub(super) prefab_source: Option<prefabs::PrefabSource>,
}

pub struct PreparedPlay {
    path: PathBuf,
    revision: u64,
    play: SceneDemo,
    assets: AssetStore,
    progress: Progress,
}

pub struct PreparedSave {
    loaded: LoadedScene,
    original_path: PathBuf,
    revision: u64,
    prefab_write: Option<prefabs::SourceWrite>,
}
impl LoadedScene {
    pub fn into_editor(self) -> Editor {
        let mut editor = Editor::from_loaded(self.scene, self.path, self.assets);
        editor.selected = self
            .prefab_source
            .as_ref()
            .map(|source| source.root.clone());
        editor.prefab_source = self.prefab_source;
        editor
    }
}

/// Owns only the newly created file until the main thread accepts this import.
/// Cancellation, stale results, and failed delivery all remove that file.
pub struct PreparedImport {
    path: PathBuf,
    catalog: BTreeMap<String, AssetSource>,
    id: String,
    source: AssetSource,
    assets: Option<AssetStore>,
    created: Option<PathBuf>,
}
impl Drop for PreparedImport {
    fn drop(&mut self) {
        if let Some(path) = &self.created {
            if matches!(self.source.kind, AssetKind::Mesh | AssetKind::Material)
                && self
                    .source
                    .path
                    .starts_with(&format!("assets/{}/", self.id))
            {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

impl Editor {
    /// Prepare file-backed prefab/script catalogs, runtime state and decoded assets off the UI thread.
    pub fn play_job(&mut self) -> Result<Job<PreparedPlay>> {
        ensure!(
            !self.is_prefab_source(),
            "Place this prefab in a scene to Play it"
        );
        ensure!(self.play.is_none(), "Play is already running");
        self.finish_gesture();
        self.refresh_audio_metadata()?;
        let scene = self.scene.clone();
        let path = self.path.clone();
        let revision = self.revision;
        let cached = self.assets.clone();
        Job::start("Preparing Play", move |progress| {
            progress.report(0, 3, "Loading runtime scenes, prefabs and scripts")?;
            let mut play = SceneDemo::new_with_prefabs_progress(&scene, Some(&path), &progress)?;
            progress.report(1, 3, "Preparing runtime assets")?;
            let mut assets = cached.for_catalog(root(&path), &play.instance().document().assets)?;
            assets.refresh_with(&progress)?;
            assets.require_ready()?;
            assets.validate_scene_resources(play.instance().document())?;
            bozzard_project::streaming::install(&mut play.app.world, &path, &assets)?;
            progress.report(3, 3, "Play ready")?;
            Ok(PreparedPlay {
                path,
                revision,
                play,
                assets,
                progress,
            })
        })
    }
    pub fn accept_play(&mut self, prepared: PreparedPlay) -> Result<()> {
        prepared.progress.check()?;
        ensure!(
            self.play.is_none() && self.path == prepared.path && self.revision == prepared.revision,
            "Scene changed while preparing Play; start Play again"
        );
        self.surface_selection = None;
        self.edit_assets = Some(std::mem::replace(&mut self.assets, prepared.assets));
        self.asset_revision += 1;
        self.runtime_asset_generation = 0;
        self.play = Some(prepared.play);
        Ok(())
    }

    pub fn save_job(&mut self, path: PathBuf) -> Result<Job<PreparedSave>> {
        self.finish_gesture();
        let scene = self.scene.clone();
        let original_path = self.path.clone();
        let revision = self.revision;
        let cached = self.assets.clone();
        let source = self.prefab_source.clone();
        Job::start("Preparing scene save", move |progress| {
            let mut scene = prepare_document_from(&scene, &path, Some(&original_path))?;
            let mut assets = cached.for_catalog(root(&path), &scene.assets)?;
            assets.refresh_with(&progress)?;
            assets.require_ready()?;
            assets.validate_scene_resources(&scene)?;
            assets.bake_audio_metadata(&mut scene)?;
            let (prefab_write, prefab_source) = if let Some(source) = source {
                let (write, updated) =
                    source.prepare_save(&scene, &original_path, &path, &progress)?;
                (Some(write), Some(updated))
            } else {
                (None, None)
            };
            Ok(PreparedSave {
                loaded: LoadedScene {
                    scene,
                    path,
                    assets,
                    prefab_source,
                },
                original_path,
                revision,
                prefab_write,
            })
        })
    }

    pub fn accept_save(&mut self, prepared: PreparedSave) -> Result<()> {
        ensure!(
            self.path == prepared.original_path && self.revision == prepared.revision,
            "Scene changed while preparing save; save again"
        );
        let LoadedScene {
            scene,
            path,
            assets,
            prefab_source,
        } = prepared.loaded;
        if let Some(write) = &prepared.prefab_write {
            prefabs::publish(write)?;
        } else {
            save_document(&scene, &path)?;
        }
        if path != self.path {
            self.past.clear();
            self.future.clear();
        }
        self.scene = scene.clone();
        self.saved = scene;
        self.path = path;
        self.prefab_source = prefab_source;
        if self.play.is_some() {
            self.edit_assets = Some(assets);
        } else {
            self.assets = assets;
        }
        self.asset_revision += 1;
        self.revision += 1;
        Ok(())
    }
    pub(super) fn cached_assets(&self, scene: &Scene, path: &Path) -> Result<AssetStore> {
        let mut assets = self.assets.for_catalog(root(path), &scene.assets)?;
        assets.load_pending()?;
        assets.require_ready()?;
        assets.validate_scene_resources(scene)?;
        Ok(assets)
    }
    pub fn open_job(path: PathBuf) -> Result<Job<LoadedScene>> {
        Job::start("Opening scene", move |progress| {
            if prefabs::is_prefab_path(&path) {
                return prefabs::load_source(path, &progress);
            }
            progress.report(0, 4, "Reading scene")?;
            let mut scene = Scene::from_json(&std::fs::read_to_string(&path)?)?;
            scene.ensure_game_menus()?;
            scene.validate()?;
            progress.report(1, 4, "Preparing scene assets")?;
            let mut assets = AssetStore::new(root(&path), &scene.assets)?;
            assets.refresh_with(&progress)?;
            assets.require_ready()?;
            assets.validate_scene_resources(&scene)?;
            assets.bake_audio_metadata(&mut scene)?;
            progress.report(4, 4, "Scene ready")?;
            Ok(LoadedScene {
                scene,
                path,
                assets,
                prefab_source: None,
            })
        })
    }

    pub fn import_job(&self, source: PathBuf) -> Result<Job<PreparedImport>> {
        ensure!(self.play.is_none(), "Stop Play before importing");
        let scene = self.scene.clone();
        let path = self.path.clone();
        let assets = self.assets.clone();
        Job::start("Preparing import", move |progress| {
            let catalog = scene.assets.clone();
            let mut worker = Self::from_loaded(scene, path.clone(), assets);
            let id = worker.import_with(&source, &progress)?;
            let source = worker.scene.assets[&id].clone();
            let created = if source.kind == AssetKind::Prefab {
                None
            } else {
                Some(
                    if matches!(source.kind, AssetKind::Mesh | AssetKind::Material)
                        && source.path.starts_with(&format!("assets/{id}/"))
                    {
                        root(&path).join(format!("assets/{id}"))
                    } else {
                        root(&path).join(&source.path)
                    },
                )
            };
            Ok(PreparedImport {
                path,
                catalog,
                id,
                source,
                assets: Some(worker.assets),
                created,
            })
        })
    }

    pub fn accept_import(&mut self, mut prepared: PreparedImport) -> Result<String> {
        ensure!(self.play.is_none(), "Stop Play before accepting an import");
        ensure!(
            self.path == prepared.path && self.scene.assets == prepared.catalog,
            "Project assets changed during import; retry the import"
        );
        let mut scene = self.scene.clone();
        scene
            .assets
            .insert(prepared.id.clone(), prepared.source.clone());
        scene.validate()?;
        self.finish_gesture();
        self.record(Change {
            restore_file: None,
            label: "Import asset".into(),
            scene: self.scene.clone(),
            assets: Some(self.assets.clone()),
        });
        self.scene = scene;
        self.assets = prepared.assets.take().expect("prepared assets");
        self.asset_revision += 1;
        self.revision += 1;
        prepared.created = None;
        Ok(prepared.id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, Instant},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    const PNG: &[u8] = include_bytes!("../../../examples/demo/scenes/assets/palette.png");
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            loop {
                let path = std::env::temp_dir().join(format!(
                    "bozzard-loading-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("{e}"),
                }
            }
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn wait<T: Send + 'static>(job: &Job<T>) -> Result<T> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = job.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "worker timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn import_is_published_once_preserves_edits_and_undo_needs_no_source() {
        let temp = Temp::new();
        let source = temp.0.join("image.png");
        std::fs::write(&source, PNG).unwrap();
        let mut editor = Editor::new(
            bozzard_demo::scene_document().unwrap(),
            &temp.0.join("scene.json"),
        )
        .unwrap();
        let job = editor.import_job(source).unwrap();
        editor.create(Mesh::Cube, Layer::ThreeD).unwrap();
        let objects = editor.scene.objects.clone();
        let prepared = wait(&job).unwrap();
        assert!(editor.scene.assets.is_empty());
        let id = editor.accept_import(prepared).unwrap();
        assert_eq!(editor.scene.objects, objects);
        let path = temp.0.join(&editor.scene.assets[&id].path);
        std::fs::remove_file(path).unwrap();
        editor.undo().unwrap();
        assert!(editor.scene.assets.is_empty());
        editor.redo().unwrap();
        assert!(
            editor
                .assets
                .get(editor.assets.handle(&id).unwrap())
                .unwrap()
                .data()
                .is_some()
        );
    }
    #[test]
    fn abandoned_and_stale_imports_remove_only_their_new_file() {
        let temp = Temp::new();
        let source = temp.0.join("image.png");
        std::fs::write(&source, PNG).unwrap();
        let mut editor = Editor::new(
            bozzard_demo::scene_document().unwrap(),
            &temp.0.join("scene.json"),
        )
        .unwrap();
        let prepared = wait(&editor.import_job(source.clone()).unwrap()).unwrap();
        let created = prepared.created.clone().unwrap();
        assert!(created.exists());
        drop(prepared);
        assert!(!created.exists());
        let prepared = wait(&editor.import_job(source.clone()).unwrap()).unwrap();
        let created = prepared.created.clone().unwrap();
        editor.path = temp.0.join("different.json");
        assert!(editor.accept_import(prepared).is_err());
        assert!(!created.exists());
        assert_eq!(std::fs::read(source).unwrap(), PNG);
        assert!(editor.scene.assets.is_empty());
    }
    #[test]
    fn background_save_validates_assets_and_cancel_or_stale_result_never_writes() {
        let temp = Temp::new();
        let source = temp.0.join("image.png");
        std::fs::write(&source, PNG).unwrap();
        let path = temp.0.join("scene.json");
        let mut editor = Editor::new(bozzard_demo::scene_document().unwrap(), &path).unwrap();
        let id = editor.import(&source).unwrap();
        let prepared = wait(&editor.save_job(path.clone()).unwrap()).unwrap();
        assert!(!path.exists());
        editor.accept_save(prepared).unwrap();
        let original = std::fs::read(&path).unwrap();
        let prepared = wait(&editor.save_job(path.clone()).unwrap()).unwrap();
        editor.create(Mesh::Cube, Layer::ThreeD).unwrap();
        assert!(editor.accept_save(prepared).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::write(temp.0.join(&editor.scene.assets[&id].path), b"broken").unwrap();
        assert!(wait(&editor.save_job(path.clone()).unwrap()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), original);
        assert!(editor.dirty());
    }
    #[test]
    fn abandoned_model_package_removes_its_owned_directory_only() {
        let temp = Temp::new();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/assets/courier.gltf");
        let editor = Editor::new(
            bozzard_demo::scene_document().unwrap(),
            &temp.0.join("scene.json"),
        )
        .unwrap();
        let prepared = wait(&editor.import_job(fixture).unwrap()).unwrap();
        let directory = prepared.created.clone().unwrap();
        assert!(directory.is_dir());
        assert!(directory.join("model.gltf").is_file());
        let neighbour = directory.parent().unwrap().join("user-file.txt");
        std::fs::write(&neighbour, b"keep").unwrap();
        drop(prepared);
        assert!(!directory.exists());
        assert_eq!(std::fs::read(neighbour).unwrap(), b"keep");
    }

    #[test]
    fn background_open_loads_a_complete_catalog_or_fails() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/model-lab.json");
        let editor = wait(&Editor::open_job(fixture).unwrap())
            .unwrap()
            .into_editor();
        editor.assets.require_ready().unwrap();
        assert!(!editor.scene.assets.is_empty());
        let temp = Temp::new();
        assert!(wait(&Editor::open_job(temp.0.join("missing.json")).unwrap()).is_err());
    }
    #[test]
    fn background_play_publishes_ready_assets_and_preserves_authoring_on_stop() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/target-range-rs.json");
        let mut editor = Editor::open(&fixture).unwrap();
        let authored = editor.scene.clone();
        let job = editor.play_job().unwrap();
        let prepared = wait(&job).unwrap();
        assert!(editor.play.is_none());
        editor.accept_play(prepared).unwrap();
        editor.assets.require_ready().unwrap();
        editor
            .play
            .as_mut()
            .unwrap()
            .game_action(bozzard_scene::GameAction::Start)
            .unwrap();
        editor.advance(std::time::Duration::from_secs_f64(1. / 60.));
        let play = editor.play.as_ref().unwrap();
        play.check_simulation().unwrap();
        assert!(
            play.app
                .world
                .resource::<bozzard_scene::ScriptRuntime>()
                .unwrap()
                .stats
                .hooks
                > 0
        );
        assert_eq!(editor.scene, authored);
        editor.stop_play();
        assert!(editor.play.is_none());
        assert_eq!(editor.scene, authored);
    }
    #[test]
    fn cancelled_and_stale_play_preparation_never_enters_play() {
        let temp = Temp::new();
        let mut editor = Editor::new(
            bozzard_demo::scene_document().unwrap(),
            &temp.0.join("scene.json"),
        )
        .unwrap();
        let job = editor.play_job().unwrap();
        let prepared = wait(&job).unwrap();
        job.cancel();
        assert!(editor.accept_play(prepared).is_err());
        assert!(editor.play.is_none());
        let job = editor.play_job().unwrap();
        let prepared = wait(&job).unwrap();
        editor.create_empty().unwrap();
        let edited = editor.scene.clone();
        assert!(editor.accept_play(prepared).is_err());
        assert!(editor.play.is_none());
        assert_eq!(editor.scene, edited);
    }
}
