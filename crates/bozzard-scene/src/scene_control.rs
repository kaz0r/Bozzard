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
    display_time: f32,
    display_overrides: crate::display::DisplayOverrides,
}
pub(crate) fn validate_library(scene: &Scene) -> Result<()> {
    ensure!(
        scene.runtime_scenes.len() <= 64,
        "runtime scene library limit: 64 scenes"
    );
    let mut objects = 0;
    for (name, level) in &scene.runtime_scenes {
        ensure!(
            !name.trim().is_empty() && name.len() <= 128,
            "invalid runtime scene name"
        );
        ensure!(
            level.runtime_scenes.is_empty(),
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
impl SceneInstance {
    pub(crate) fn request_scene_control(
        &self,
        world: &mut World,
        kind: K,
        name: &str,
    ) -> Result<()> {
        match kind {
            K::LoadScene | K::AddScene => ensure!(
                self.document.runtime_scenes.contains_key(name),
                "unknown runtime scene '{name}'"
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
        while let Some((kind, name)) = commands.pop_front() {
            ensure!(applied < 16, "scene command limit: 16 per tick");
            applied += 1;
            match kind {
                K::LoadScene => self.load_runtime_scene(world, &name, false)?,
                K::AddScene => self.load_runtime_scene(world, &name, true)?,
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
                    self.load_game_json(world, &json)?;
                }
                _ => unreachable!(),
            }
            // Teardown handlers can enqueue follow-up saves. Preserve their order and the tick bound.
            commands.extend(world.remove_resource::<Commands>().unwrap_or_default().0);
        }
        Ok(())
    }
    pub fn restart_runtime_scene(&mut self, world: &mut World) -> Result<()> {
        self.replace_runtime_scene(world, (*self.restart_document).clone())
    }
    /// Additive objects get unique persistent IDs; their references and prefab links follow them.
    pub fn load_runtime_scene(
        &mut self,
        world: &mut World,
        name: &str,
        additive: bool,
    ) -> Result<()> {
        let mut level = self
            .document
            .runtime_scenes
            .get(name)
            .context("unknown runtime scene")?
            .as_ref()
            .clone();
        if !additive {
            level.runtime_scenes = self.document.runtime_scenes.clone();
            level.assets = self.document.assets.clone();
            return self.replace_runtime_scene(world, level);
        }
        let serial = self
            .scene_serial
            .checked_add(1)
            .context("scene ID space exhausted")?;
        let mapping: BTreeMap<_, _> = level
            .objects
            .iter()
            .map(|o| (o.id.clone(), format!("scene-{serial}-{}", o.id)))
            .collect();
        ensure!(
            mapping.values().all(|id| !self.entities.contains_key(id)),
            "additive scene ID collision"
        );
        blueprint::remap_board(&mut level.blackboard, &mapping);
        for object in &mut level.objects {
            object.remap_blueprint_objects(&mapping);
            object.id = mapping[&object.id].clone();
            object.parent = object.parent.take().map(|p| mapping[&p].clone());
        }
        let mut candidate = self.capture(world)?;
        for (key, value) in level.blackboard {
            if let Some(existing) = candidate.blackboard.get(&key) {
                ensure!(
                    existing == &value,
                    "additive scene blackboard conflict '{key}'"
                );
            } else {
                candidate.blackboard.insert(key, value);
            }
        }
        let start = candidate.objects.len();
        candidate.objects.extend(level.objects);
        for (root, mut prefab) in level.prefabs {
            for id in prefab.members.values_mut() {
                *id = mapping[id].clone();
            }
            for object in &mut prefab.baseline {
                object.remap_blueprint_objects(&mapping);
                object.id = mapping[&object.id].clone();
                object.parent = object.parent.take().map(|p| mapping[&p].clone());
            }
            candidate.prefabs.insert(mapping[&root].clone(), prefab);
        }
        let order = candidate.order()?;
        for object in &candidate.objects[start..] {
            self.entities
                .insert(object.id.clone(), object.spawn_in(world)?);
        }
        if let Some(runtime) = world.resource_mut::<BlueprintRuntime>() {
            runtime.add_scene_defaults(&candidate.blackboard);
        }
        self.document = candidate;
        self.order = order;
        self.rebuild_hierarchy_index();
        self.scene_serial = serial;
        Ok(())
    }
    fn replace_runtime_scene(&mut self, world: &mut World, scene: Scene) -> Result<()> {
        // Validate all spawn-time invariants in isolation before removing any live entity.
        let mut check = World::default();
        scene.spawn(&mut check)?;
        for entity in self.entities.values() {
            ensure!(
                world.contains(*entity),
                "scene entity removed outside runtime"
            );
        }
        self.scene_destroy_events(world)?;
        self.scene_script_destroy_events(world)?;
        for entity in self.entities.values() {
            world.despawn(*entity)?;
        }
        world.remove_resource::<crate::physics::Physics>();
        world.remove_resource::<BlueprintRuntime>();
        world.remove_resource::<crate::ScriptRuntime>();
        world.remove_resource::<GameplayState>();
        world.remove_resource::<GameSession>();
        world.insert_resource(GameplayInput::default());
        world.insert_resource(CursorCapture::default());
        let templates = self.templates.clone();
        let mut next = scene.spawn(world)?;
        // Loaded script sources and the compiled engine belong to the runtime, not to the document
        // that was just spawned: without them the replacement scene has attachments nothing can run.
        // Attachment state is deliberately not carried, so `on_start` fires again in the new scene.
        next.templates = templates;
        next.script_engine = std::mem::take(&mut self.script_engine);
        next.scripts = std::mem::take(&mut self.scripts);
        if scene.game_flow.is_some() {
            world.insert_resource(GameSession {
                phase: GamePhase::Playing,
                ..Default::default()
            });
        }
        *self = next;
        Ok(())
    }
    pub fn save_game_json(&self, world: &World) -> Result<String> {
        let save = GameSave {
            version: 1,
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
            display_time: self.display_time,
            display_overrides: self.display_overrides.clone(),
        };
        let json = serde_json::to_string(&save)?;
        ensure!(json.len() <= 64 * 1024 * 1024, "save game exceeds 64 MiB");
        Ok(json)
    }
    pub fn load_game_json(&mut self, world: &mut World, json: &str) -> Result<()> {
        ensure!(json.len() <= 64 * 1024 * 1024, "save game exceeds 64 MiB");
        let save: GameSave = serde_json::from_str(json).context("parsing saved game")?;
        ensure!(save.version == 1, "unsupported save game version");
        save.scene.validate()?;
        save.restart.validate()?;
        ensure!(
            save.scene.assets == self.document.assets
                && save.scene.runtime_scenes == self.document.runtime_scenes,
            "save belongs to a different scene catalog"
        );
        ensure!(
            save.display_time.is_finite() && save.display_time >= 0.,
            "invalid saved display time"
        );
        let runtime = save
            .blueprint
            .map(|s| BlueprintRuntime::restore(s, &save.scene))
            .transpose()?;
        for (id, g) in &save.gravity {
            ensure!(
                save.scene
                    .objects
                    .iter()
                    .any(|o| &o.id == id && o.gravity.is_some())
                    && g.vertical_velocity.is_finite(),
                "invalid saved gravity"
            );
        }
        ensure!(
            save.hidden
                .iter()
                .all(|id| save.scene.objects.iter().any(|o| &o.id == id)),
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
                save.scene
                    .objects
                    .iter()
                    .any(|o| o.id == state.player && o.player_controller.is_some())
                    && state
                        .respawn
                        .iter()
                        .chain([&state.yaw, &state.pitch])
                        .all(|v| v.is_finite())
                    && state.collected.iter().all(|id| save
                        .scene
                        .objects
                        .iter()
                        .any(|o| &o.id == id)),
                "invalid saved gameplay state"
            );
        }
        save.display_overrides
            .apply(save.scene.display)
            .validate()?;
        self.replace_runtime_scene(world, save.scene)?;
        self.restart_document = Arc::new(save.restart);
        self.next_spawn = save.next_spawn;
        self.scene_serial = save.scene_serial;
        self.display_time = save.display_time;
        self.display_overrides = save.display_overrides;
        for id in save.hidden {
            world.insert(self.entities[&id], BlueprintHidden(true))?;
        }
        for (id, g) in save.gravity {
            world.insert(self.entities[&id], g)?;
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
        world.insert_resource(physics);
        Ok(())
    }
}
