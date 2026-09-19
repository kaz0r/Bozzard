//! Validated scene transitions and bounded game checkpoints shared by all hosts.
use super::*;
use blueprint::NodeKind as K;
use std::{io::Read, path::PathBuf, sync::Arc};
#[derive(Default)]
struct Commands(Vec<(K, String)>);
/// Hosts choose a directory; headless callers can use in-memory slots or export/import JSON.
#[derive(Default)]
pub struct GameSaves {
    pub directory: Option<PathBuf>,
    slots: BTreeMap<String, String>,
}
impl GameSaves {
    pub fn in_directory(path: PathBuf) -> Self {
        Self {
            directory: Some(path),
            ..Self::default()
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GameSave {
    version: u32,
    #[serde(default)]
    middleware: crate::middleware::checkpoint::Save,
    scene: Scene,
    restart: Scene,
    blueprint: Option<crate::blueprint_runtime::RuntimeSave>,
    hidden: Vec<String>,
    gravity: BTreeMap<String, GravityState>,
    physics: Vec<crate::physics::BodySave>,
    session: Option<GameSession>,
    gameplay: Option<GameplayState>,
    cursor: Option<bool>,
    next_spawn: u64,
    scene_serial: u64,
    #[serde(default)]
    additive_scenes: BTreeMap<String, crate::scene_loading::LoadedScene>,
    display_time: f32,
    display_overrides: crate::display::DisplayOverrides,
}
pub(crate) fn validate_library(scene: &Scene) -> Result<()> {
    ensure!(
        scene
            .runtime_scenes
            .keys()
            .chain(scene.runtime_scene_sources.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            <= 64,
        "runtime scene library limit: 64 scenes"
    );
    for (name, source) in &scene.runtime_scene_sources {
        ensure!(
            !name.trim().is_empty() && name.len() <= 128,
            "invalid runtime scene name"
        );
        source.validate()?;
    }
    let mut objects = 0;
    for (name, level) in &scene.runtime_scenes {
        ensure!(
            !name.trim().is_empty() && name.len() <= 128,
            "invalid runtime scene name"
        );
        ensure!(
            level.runtime_scenes.is_empty() && level.runtime_scene_sources.is_empty(),
            "runtime scenes cannot contain nested libraries"
        );
        objects += level.objects.len();
        ensure!(
            objects <= 100_000,
            "runtime scene library exceeds 100000 objects"
        );
        level.validate()?;
        ensure!(
            level
                .assets
                .iter()
                .all(|(id, a)| scene.assets.get(id) == Some(a)),
            "runtime scene '{name}' assets must be in the shared catalog"
        );
    }
    Ok(())
}
pub(crate) fn require_tick_boundary(world: &World) -> Result<()> {
    ensure!(
        !world
            .resource::<BlueprintRuntime>()
            .is_some_and(BlueprintRuntime::suspended),
        "Finish the suspended Blueprint tick before saving or replacing game state"
    );
    Ok(())
}
impl SceneInstance {
    pub(crate) fn request_scene_control(
        &self,
        world: &mut World,
        kind: K,
        name: &str,
    ) -> Result<()> {
        match kind {
            K::LoadScene | K::AddScene | K::LoadSceneAsync | K::AddSceneAsync => ensure!(
                self.document.runtime_scenes.contains_key(name)
                    || self.document.runtime_scene_sources.contains_key(name),
                "unknown runtime scene '{name}'"
            ),
            K::UnloadScene => ensure!(
                self.loaded_scenes().contains_key(name),
                "unknown loaded scene handle"
            ),
            K::SaveGame | K::LoadGame => ensure!(
                !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
                "save slot needs 1–64 letters, digits, '_' or '-'"
            ),
            _ => {}
        }
        if world.resource::<Commands>().is_none() {
            world.insert_resource(Commands::default());
        }
        let commands = &mut world.resource_mut::<Commands>().unwrap().0;
        ensure!(commands.len() < 16, "scene command limit: 16 per tick");
        commands.push((kind, name.into()));
        Ok(())
    }
    pub(crate) fn apply_scene_controls(&mut self, world: &mut World) -> Result<()> {
        let mut commands =
            VecDeque::from(world.remove_resource::<Commands>().unwrap_or_default().0);
        let mut applied = 0;
        let mut polled = false;
        loop {
            let Some((kind, name)) = commands.pop_front() else {
                if polled {
                    break;
                }
                polled = true;
                // Async failures remain readable in status. Publish only after this action pass,
                // then apply any teardown requests under the same per-tick command budget.
                let _ = self.poll_scene_load(world);
                commands.extend(world.remove_resource::<Commands>().unwrap_or_default().0);
                continue;
            };
            ensure!(applied < 16, "scene command limit: 16 per tick");
            applied += 1;
            match kind {
                K::LoadScene => self.load_runtime_scene(world, &name, false)?,
                K::AddScene => self.load_runtime_scene(world, &name, true)?,
                K::LoadSceneAsync => self.begin_scene_load(world, &name, false)?,
                K::AddSceneAsync => self.begin_scene_load(world, &name, true)?,
                K::CancelSceneLoad => self.cancel_scene_load(world),
                K::UnloadScene => self.unload_runtime_scene(world, &name)?,
                K::RestartScene => self.restart_runtime_scene(world)?,
                K::SaveGame => {
                    let json = self.save_game_json(world)?;
                    if world.resource::<GameSaves>().is_none() {
                        world.insert_resource(GameSaves::default());
                    }
                    let store = world.resource_mut::<GameSaves>().unwrap();
                    ensure!(
                        store.slots.contains_key(&name) || store.slots.len() < 64,
                        "save slot limit: 64"
                    );
                    if let Some(directory) = &store.directory {
                        std::fs::create_dir_all(directory)?;
                        let path = directory.join(format!("{name}.json"));
                        static SERIAL: std::sync::atomic::AtomicU64 =
                            std::sync::atomic::AtomicU64::new(0);
                        let temp = directory.join(format!(
                            ".{name}-{}-{}.tmp",
                            std::process::id(),
                            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                        ));
                        let result = (|| -> Result<()> {
                            use std::io::Write;
                            let mut file = std::fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(&temp)?;
                            file.write_all(json.as_bytes())?;
                            file.sync_all()?;
                            drop(file);
                            std::fs::rename(&temp, &path)?;
                            Ok(())
                        })();
                        if result.is_err() {
                            let _ = std::fs::remove_file(&temp);
                        }
                        result?;
                    }
                    if store.directory.is_none() {
                        let total = store
                            .slots
                            .iter()
                            .filter(|(slot, _)| *slot != &name)
                            .map(|(_, json)| json.len())
                            .sum::<usize>();
                        ensure!(
                            total + json.len() <= 64 * 1024 * 1024,
                            "in-memory saves exceed 64 MiB"
                        );
                        store.slots.insert(name, json);
                    }
                }
                K::LoadGame => {
                    let store = world.resource::<GameSaves>().context("no saved games")?;
                    let json = if let Some(directory) = &store.directory {
                        let mut json = String::new();
                        std::fs::File::open(directory.join(format!("{name}.json")))?
                            .take(64 * 1024 * 1024 + 1)
                            .read_to_string(&mut json)?;
                        json
                    } else {
                        store
                            .slots
                            .get(&name)
                            .context("save slot does not exist")?
                            .clone()
                    };
                    let plan = self.prepare_game_load(&json)?;
                    if plan.requires_acquisition() {
                        self.begin_prepared_scene_load(world, plan)?;
                    } else {
                        self.accept_scene_load(
                            world,
                            plan.prepare(&bozzard_app::job::Progress::default())?,
                        )?;
                    }
                }
                _ => unreachable!(),
            }
            // Teardown handlers can enqueue follow-up saves. Preserve their order and the tick bound.
            commands.extend(world.remove_resource::<Commands>().unwrap_or_default().0);
        }
        Ok(())
    }
    pub fn restart_runtime_scene(&mut self, world: &mut World) -> Result<()> {
        require_tick_boundary(world)?;
        self.replace_runtime_scene(world, (*self.restart_document).clone())
    }
    /// Additive objects get unique persistent IDs; their references and prefab links follow them.
    pub fn load_runtime_scene(
        &mut self,
        world: &mut World,
        name: &str,
        additive: bool,
    ) -> Result<()> {
        require_tick_boundary(world)?;
        ensure!(
            !self.document.runtime_scene_sources.contains_key(name),
            "file/content scenes require asynchronous loading"
        );
        let prepared = self
            .prepare_scene_load(name, additive)?
            .prepare(&bozzard_app::job::Progress::default())?;
        self.accept_scene_load(world, prepared)?;
        Ok(())
    }
    fn replace_runtime_scene(&mut self, world: &mut World, scene: Scene) -> Result<()> {
        let name = scene.name.clone();
        let prepared = self
            .scene_load_plan(&name, Some(Arc::new(scene)), false)?
            .prepare(&bozzard_app::job::Progress::default())?;
        self.accept_scene_load(world, prepared)?;
        Ok(())
    }
    pub fn save_game_json(&self, world: &World) -> Result<String> {
        require_tick_boundary(world)?;
        let save = GameSave {
            version: 1,
            middleware: crate::middleware::checkpoint::Save::capture(world),
            scene: self.capture(world)?,
            restart: (*self.restart_document).clone(),
            blueprint: world
                .resource::<BlueprintRuntime>()
                .map(BlueprintRuntime::save),
            hidden: self
                .entities
                .iter()
                .filter(|(_, e)| world.get::<BlueprintHidden>(**e).is_some_and(|h| h.0))
                .map(|(id, _)| id.clone())
                .collect(),
            gravity: self
                .entities
                .iter()
                .filter_map(|(id, e)| world.get::<GravityState>(*e).map(|g| (id.clone(), *g)))
                .collect(),
            physics: world
                .resource::<crate::physics::Physics>()
                .map(|p| p.save())
                .unwrap_or_default(),
            session: world.resource::<GameSession>().cloned(),
            gameplay: world.resource::<GameplayState>().cloned(),
            cursor: world.resource::<CursorCapture>().and_then(|c| c.requested),
            next_spawn: self.next_spawn,
            scene_serial: self.scene_serial,
            additive_scenes: self.additive_scenes.clone(),
            display_time: self.display_time,
            display_overrides: self.display_overrides.clone(),
        };
        let json = serde_json::to_string(&save)?;
        ensure!(json.len() <= 64 * 1024 * 1024, "save game exceeds 64 MiB");
        Ok(json)
    }
    /// Synchronous restore for a catalog that this runtime has already prepared.
    /// Use `begin_game_load` when a saved catalog needs file/content dependencies.
    pub fn load_game_json(&mut self, world: &mut World, json: &str) -> Result<()> {
        require_tick_boundary(world)?;
        let plan = self.prepare_game_load(json)?;
        ensure!(
            !plan.requires_acquisition(),
            "saved game needs asynchronous dependency preparation"
        );
        self.accept_scene_load(world, plan.prepare(&bozzard_app::job::Progress::default())?)?;
        Ok(())
    }
    pub fn prepare_game_load(&self, json: &str) -> Result<crate::scene_loading::SceneLoadPlan> {
        let (scene, state) = PreparedGameState::parse(json)?;
        self.scene_load_plan("Saved game", None, false)?
            .with_checkpoint(scene, state)
    }
    /// Restore through the same cancellable worker/status boundary as scene loading.
    pub fn begin_game_load(&self, world: &mut World, json: &str) -> Result<()> {
        self.begin_prepared_scene_load(world, self.prepare_game_load(json)?)
    }
}

pub(crate) struct PreparedGameState {
    restart: Scene,
    hidden: Vec<String>,
    gravity: BTreeMap<String, GravityState>,
    session: Option<GameSession>,
    gameplay: Option<GameplayState>,
    cursor: Option<bool>,
    next_spawn: u64,
    scene_serial: u64,
    additive_scenes: BTreeMap<String, crate::scene_loading::LoadedScene>,
    display_time: f32,
    display_overrides: crate::display::DisplayOverrides,
    middleware: crate::middleware::checkpoint::Save,
    runtime: Option<BlueprintRuntime>,
    physics: crate::physics::Physics,
}
impl PreparedGameState {
    fn parse(json: &str) -> Result<(Scene, Self)> {
        ensure!(json.len() <= 64 * 1024 * 1024, "save game exceeds 64 MiB");
        let mut save: GameSave = serde_json::from_str(json).context("parsing saved game")?;
        ensure!(save.version == 1, "unsupported save game version");
        save.scene.validate()?;
        save.restart.validate()?;
        save.middleware.validate(&save.scene)?;
        crate::scene_loading::validate_loaded_scenes(
            &save.additive_scenes,
            &save.scene,
            save.scene_serial,
        )?;
        ensure!(
            save.display_time.is_finite() && save.display_time >= 0.,
            "invalid saved display time"
        );
        let runtime = save
            .blueprint
            .take()
            .map(|s| BlueprintRuntime::restore(s, &save.scene))
            .transpose()?;
        let objects: BTreeMap<_, _> = save
            .scene
            .objects
            .iter()
            .map(|o| (o.id.as_str(), o))
            .collect();
        for (id, g) in &save.gravity {
            ensure!(
                objects
                    .get(id.as_str())
                    .is_some_and(|o| o.gravity.is_some())
                    && g.vertical_velocity.is_finite(),
                "invalid saved gravity"
            );
        }
        ensure!(
            save.hidden
                .iter()
                .all(|id| objects.contains_key(id.as_str())),
            "invalid saved visibility"
        );
        let physics = crate::physics::Physics::restore(&save.physics, &save.scene)?;
        if let Some(session) = &save.session {
            ensure!(
                session.message.len() <= 240 && save.scene.game_flow.is_some(),
                "invalid saved game session"
            );
        }
        if let Some(state) = &save.gameplay {
            ensure!(
                objects
                    .get(state.player.as_str())
                    .is_some_and(|o| o.player_controller.is_some())
                    && state
                        .respawn
                        .iter()
                        .chain([&state.yaw, &state.pitch])
                        .all(|v| v.is_finite())
                    && state
                        .collected
                        .iter()
                        .all(|id| objects.contains_key(id.as_str())),
                "invalid saved gameplay state"
            );
        }
        save.display_overrides
            .apply(save.scene.display)
            .validate()?;
        Ok((
            save.scene,
            Self {
                restart: save.restart,
                hidden: save.hidden,
                gravity: save.gravity,
                session: save.session,
                gameplay: save.gameplay,
                cursor: save.cursor,
                next_spawn: save.next_spawn,
                scene_serial: save.scene_serial,
                additive_scenes: save.additive_scenes,
                display_time: save.display_time,
                display_overrides: save.display_overrides,
                middleware: save.middleware,
                runtime,
                physics,
            },
        ))
    }
    pub(crate) fn apply(mut self, instance: &mut SceneInstance, world: &mut World) -> Result<()> {
        let runtime = self.runtime.take();
        let save = self;
        instance.restart_document = Arc::new(save.restart);
        instance.next_spawn = save.next_spawn;
        instance.scene_serial = save.scene_serial;
        instance.additive_scenes = save.additive_scenes;
        instance.display_time = save.display_time;
        instance.display_overrides = save.display_overrides;
        for id in save.hidden {
            world.insert(instance.entities[&id], BlueprintHidden(true))?;
        }
        for (id, g) in save.gravity {
            world.insert(instance.entities[&id], g)?;
        }
        if let Some(runtime) = runtime {
            world.insert_resource(runtime);
        }
        if let Some(gameplay) = save.gameplay {
            world.insert_resource(gameplay);
        }
        if let Some(session) = save.session {
            world.insert_resource(session);
        }
        world.insert_resource(CursorCapture {
            requested: save.cursor,
        });
        world.insert_resource(save.physics);
        save.middleware.restore(world);
        Ok(())
    }
}
