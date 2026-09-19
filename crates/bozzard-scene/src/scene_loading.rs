//! Scene preparation owns an isolated world; acceptance preserves live entity handles.
use super::*;
use bozzard_app::job::{Job, Progress};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub(crate) fn next_instance_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .expect("scene instance identity exhausted")
}

/// A snapshot of authoring state only. Live transforms, velocities, timers and
/// blackboards continue to advance while its worker prepares the new scene.
pub struct SceneLoadPlan {
    instance: u64,
    base: Scene,
    level: Option<Arc<Scene>>,
    catalog: Option<SceneCatalog>,
    bound_scripts: BTreeSet<String>,
    serial: u64,
    name: String,
    additive: bool,
    checkpoint: Option<scene_control::PreparedGameState>,
    requires_bindings: bool,
}

pub struct PreparedScene {
    instance: u64,
    base: Scene,
    candidate: Option<Scene>,
    world: World,
    entities: BTreeMap<String, Entity>,
    order: Vec<usize>,
    replacement: Option<SceneInstance>,
    serial: u64,
    name: String,
    members: BTreeSet<String>,
    progress: Progress,
    bound_scripts: BTreeSet<String>,
    bindings: RuntimeBindings,
    publications: Vec<Publication>,
    needs_bindings: bool,
    checkpoint: Option<scene_control::PreparedGameState>,
}
type Publication = Box<dyn FnOnce(&mut World) + Send + Sync>;
#[derive(Default)]
struct RuntimeBindings {
    templates: BTreeMap<String, Prefab>,
    scripts: BTreeMap<String, Arc<script_runtime::CompiledScript>>,
    kernels: BTreeMap<String, Arc<compute::Kernel>>,
}
struct SceneCatalog {
    assets: BTreeMap<String, AssetSource>,
    scenes: BTreeMap<String, Arc<Scene>>,
    sources: BTreeMap<String, SceneSource>,
}

/// Paths are relative to the owning scene. Content catalogs may also use HTTPS URLs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneSource {
    File { path: String },
    Content { catalog: String, address: String },
}
impl SceneSource {
    pub(crate) fn validate(&self) -> Result<()> {
        let relative = |path: &str| {
            !path.is_empty()
                && path.len() <= 4096
                && !path.contains(['\\', ':'])
                && !path.starts_with('/')
                && !path.chars().any(char::is_control)
        };
        match self {
            Self::File { path } => ensure!(relative(path), "invalid runtime scene file path"),
            Self::Content { catalog, address } => {
                ensure!(
                    catalog.len() <= 4096
                        && !catalog.chars().any(char::is_control)
                        && (relative(catalog)
                            || catalog.starts_with("https://")
                            || catalog.starts_with("http://")),
                    "invalid runtime content catalog"
                );
                ensure!(
                    !address.is_empty()
                        && address.len() <= 256
                        && !address.chars().any(char::is_control),
                    "invalid runtime content address"
                );
            }
        }
        Ok(())
    }
}

/// Capture host state here; disk/network/decode work belongs in the returned job.
/// The scene core deliberately has no dependency on files, codecs or HTTP clients.
pub trait SceneLoader: Send + Sync + 'static {
    fn start(&self, plan: SceneLoadPlan, world: &World) -> Result<Job<PreparedScene>>;
}
/// Install this resource once in hosts that support file/content scene sources.
pub struct SceneLoaderHandle(pub Arc<dyn SceneLoader>);
impl PreparedScene {
    pub fn document(&self) -> &Scene {
        self.replacement.as_ref().map_or_else(
            || self.candidate.as_ref().expect("prepared additive scene"),
            |scene| &scene.document,
        )
    }
    /// A prepared host resource becomes visible only after all publication guards pass.
    /// This is intended for decoded asset catalogs and immutable content mounts.
    pub fn publish_resource<T: Send + Sync + 'static>(&mut self, resource: T) {
        self.publications.push(Box::new(move |world| {
            world.insert_resource(resource);
        }));
    }

    /// Bind newly acquired gameplay dependencies before accepting any scene entities.
    pub fn bind_catalog(
        &mut self,
        templates: BTreeMap<String, Prefab>,
        sources: BTreeMap<String, String>,
        kernels: BTreeMap<String, Arc<compute::Kernel>>,
    ) -> Result<()> {
        self.progress.check()?;
        let scene = self.document();
        for (id, prefab) in &templates {
            runtime_prefabs::validate_template(scene, id, prefab)?;
        }
        for id in sources.keys() {
            ensure!(
                scene
                    .assets
                    .get(id)
                    .is_some_and(|a| a.kind == AssetKind::Script),
                "asset '{id}' is not a script"
            );
        }
        for object in &scene.objects {
            for attachment in object.script_manager.iter().flat_map(|m| &m.scripts) {
                ensure!(
                    self.bound_scripts.contains(&attachment.script)
                        || sources.contains_key(&attachment.script),
                    "script '{}' on '{}' was not loaded",
                    attachment.script,
                    object.id
                );
            }
        }
        ensure!(
            scene
                .assets
                .values()
                .filter(|a| a.kind == AssetKind::Script)
                .count()
                <= script_runtime::MAX_SCRIPT_ASSETS,
            "scene script catalog exceeds its limit"
        );
        compute_runtime::validate_kernels(scene, &kernels)?;
        let sources = sources
            .into_iter()
            .filter(|(id, _)| !self.bound_scripts.contains(id))
            .collect();
        let scripts = script_runtime::compile_sources(sources, &self.progress)?;
        self.bindings = RuntimeBindings {
            templates,
            scripts,
            kernels,
        };
        self.needs_bindings = false;
        Ok(())
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn additive(&self) -> bool {
        self.replacement.is_none()
    }
    pub fn object_count(&self) -> usize {
        self.replacement
            .as_ref()
            .map_or(self.entities.len(), |scene| scene.entities.len())
    }
}
impl SceneLoadPlan {
    /// Saved scene paths already use the host root; hosts must prepare their dependencies
    /// without reacquiring newer versions of the scene documents themselves.
    pub fn checkpoint_document(&self) -> Option<&Scene> {
        self.checkpoint.as_ref().and(self.level.as_deref())
    }
    pub fn requires_acquisition(&self) -> bool {
        self.source().is_some() || self.requires_bindings
    }
    pub(crate) fn with_checkpoint(
        mut self,
        mut scene: Scene,
        state: scene_control::PreparedGameState,
    ) -> Result<Self> {
        self.requires_bindings = scene
            .assets
            .iter()
            .any(|(id, source)| self.base.assets.get(id) != Some(source));
        // Catalogs can grow while playing. Keep currently prepared entries, but never
        // reinterpret a global ID from a checkpoint as a different source.
        for (id, source) in &self.base.assets {
            ensure!(
                scene.assets.get(id).is_none_or(|saved| saved == source),
                "save belongs to a different scene catalog: asset '{id}'"
            );
            scene.assets.insert(id.clone(), source.clone());
        }
        for (name, source) in &self.base.runtime_scene_sources {
            ensure!(
                scene
                    .runtime_scene_sources
                    .get(name)
                    .is_none_or(|saved| saved == source),
                "save belongs to a different scene catalog: source '{name}'"
            );
            scene
                .runtime_scene_sources
                .insert(name.clone(), source.clone());
        }
        for (name, level) in &self.base.runtime_scenes {
            ensure!(
                scene
                    .runtime_scenes
                    .get(name)
                    .is_none_or(|saved| saved == level),
                "save belongs to a different scene catalog: scene '{name}'"
            );
            scene.runtime_scenes.insert(name.clone(), level.clone());
        }
        scene.validate()?;
        self.catalog = Some(SceneCatalog {
            assets: scene.assets.clone(),
            scenes: scene.runtime_scenes.clone(),
            sources: scene.runtime_scene_sources.clone(),
        });
        self.level = Some(Arc::new(scene));
        self.checkpoint = Some(state);
        Ok(self)
    }
    pub fn source(&self) -> Option<&SceneSource> {
        if self.checkpoint.is_some() {
            return None;
        }
        self.base.runtime_scene_sources.get(&self.name)
    }
    pub fn base(&self) -> &Scene {
        &self.base
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    /// The host must first rebase the incoming asset/source paths to the base scene's root.
    /// Conflicting global IDs are rejected instead of silently rebinding live objects.
    pub fn with_acquired_scene(mut self, mut level: Scene) -> Result<Self> {
        ensure!(
            self.checkpoint.is_none(),
            "a checkpoint must retain its saved scene"
        );
        level.validate()?;
        let mut assets = self.base.assets.clone();
        for (id, asset) in &level.assets {
            ensure!(
                assets.get(id).is_none_or(|old| old == asset),
                "scene asset conflict '{id}'"
            );
            assets.insert(id.clone(), asset.clone());
        }
        let mut scenes = self.base.runtime_scenes.clone();
        for (name, scene) in std::mem::take(&mut level.runtime_scenes) {
            ensure!(
                name != self.name && scenes.get(&name).is_none_or(|old| old == &scene),
                "runtime scene conflict '{name}'"
            );
            scenes.insert(name, scene);
        }
        let mut sources = self.base.runtime_scene_sources.clone();
        for (name, source) in std::mem::take(&mut level.runtime_scene_sources) {
            ensure!(
                sources.get(&name).is_none_or(|old| old == &source),
                "runtime scene source conflict '{name}'"
            );
            sources.insert(name, source);
        }
        level.assets = assets.clone();
        let level = Arc::new(level);
        scenes.insert(self.name.clone(), level.clone());
        self.catalog = Some(SceneCatalog {
            assets,
            scenes,
            sources,
        });
        self.requires_bindings = true;
        self.level = Some(level);
        Ok(self)
    }
    pub fn start(self) -> Result<Job<PreparedScene>> {
        Job::start("Preparing scene", move |progress| self.prepare(&progress))
    }
    pub fn prepare(self, progress: &Progress) -> Result<PreparedScene> {
        progress.report(0, 100, format!("Preparing {}", self.name))?;
        let needs_bindings = self.requires_bindings;
        let mut level = Arc::unwrap_or_clone(self.level.context("host did not acquire the scene")?);
        let catalog = self.catalog.unwrap_or_else(|| SceneCatalog {
            assets: self.base.assets.clone(),
            scenes: self.base.runtime_scenes.clone(),
            sources: self.base.runtime_scene_sources.clone(),
        });
        if !self.additive {
            level.runtime_scenes = catalog.scenes;
            level.runtime_scene_sources = catalog.sources;
            level.assets = catalog.assets;
            level.ensure_game_menus()?;
            progress.report(10, 100, "Validating replacement scene")?;
            let mut world = World::new();
            let replacement = level.spawn_reporting(&mut world, |index, count| {
                progress.check()?;
                if index % 128 == 0 || index == count {
                    progress.report(
                        20 + index * 70 / count.max(1),
                        100,
                        format!("Preparing {} ({index}/{count})", self.name),
                    )?;
                }
                Ok(())
            })?;
            progress.report(95, 100, "Replacement scene ready")?;
            return Ok(PreparedScene {
                instance: self.instance,
                base: self.base,
                candidate: None,
                world,
                entities: BTreeMap::new(),
                order: Vec::new(),
                replacement: Some(replacement),
                serial: self.serial,
                name: self.name,
                members: BTreeSet::new(),
                progress: progress.clone(),
                bound_scripts: self.bound_scripts,
                bindings: Default::default(),
                publications: Vec::new(),
                needs_bindings,
                checkpoint: self.checkpoint,
            });
        }
        let mapping: BTreeMap<_, _> = level
            .objects
            .iter()
            .map(|o| (o.id.clone(), format!("scene-{}-{}", self.serial, o.id)))
            .collect();
        let existing: BTreeSet<_> = self.base.objects.iter().map(|o| o.id.as_str()).collect();
        ensure!(
            mapping.values().all(|id| !existing.contains(id.as_str())),
            "additive scene ID collision"
        );
        blueprint::remap_board(&mut level.blackboard, &mapping);
        for object in &mut level.objects {
            remap_object(object, &mapping);
        }
        let mut candidate = self.base.clone();
        candidate.assets = catalog.assets;
        candidate.runtime_scenes = catalog.scenes;
        candidate.runtime_scene_sources = catalog.sources;
        // Fill missing views without changing the live scene's active cameras.
        for (layer, camera) in level.views {
            candidate
                .views
                .entry(layer)
                .or_insert_with(|| mapping[&camera].clone());
        }
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
        for (root, mut link) in level.prefabs {
            for id in link.members.values_mut() {
                *id = mapping[id].clone();
            }
            for object in &mut link.baseline {
                remap_object(object, &mapping);
            }
            candidate.prefabs.insert(mapping[&root].clone(), link);
        }
        progress.report(10, 100, "Validating combined scene")?;
        let order = candidate.order()?;
        let mut world = World::new();
        let mut entities = BTreeMap::new();
        let count = candidate.objects.len() - start;
        for (index, object) in candidate.objects[start..].iter().enumerate() {
            progress.check()?;
            if index % 128 == 0 {
                progress.report(
                    20 + index * 70 / count.max(1),
                    100,
                    format!("Preparing {} ({}/{count})", self.name, index + 1),
                )?;
            }
            entities.insert(object.id.clone(), object.spawn_in(&mut world)?);
        }
        progress.report(95, 100, "Additive scene ready")?;
        Ok(PreparedScene {
            instance: self.instance,
            base: self.base,
            candidate: Some(candidate),
            world,
            entities,
            order,
            replacement: None,
            serial: self.serial,
            name: self.name,
            members: mapping.into_values().collect(),
            progress: progress.clone(),
            bound_scripts: self.bound_scripts,
            bindings: Default::default(),
            publications: Vec::new(),
            needs_bindings,
            checkpoint: None,
        })
    }
}

pub(crate) fn remap_object(object: &mut Object, mapping: &BTreeMap<String, String>) {
    object.remap_blueprint_objects(mapping);
    object.id = mapping[&object.id].clone();
    object.parent = object.parent.take().map(|p| mapping[&p].clone());
    if let Some(joint) = &mut object.joint
        && let Some(other) = mapping.get(&joint.other)
    {
        joint.other = other.clone();
    }
    if let Some(player) = &mut object.player_controller
        && let Some(camera) = mapping.get(&player.camera)
    {
        player.camera = camera.clone();
    }
}

impl SceneInstance {
    pub fn loaded_scenes(&self) -> &BTreeMap<String, LoadedScene> {
        &self.additive_scenes
    }

    /// Unload a particular additive instance, including prefabs spawned by its objects.
    /// Cross-scene structural dependencies must be removed before unloading their target.
    pub fn unload_runtime_scene(&mut self, world: &mut World, handle: &str) -> Result<()> {
        super::scene_control::require_tick_boundary(world)?;
        let ids = self
            .additive_scenes
            .get(handle)
            .context("unknown loaded scene handle")?
            .members
            .clone();
        self.prepare_object_removal(&ids, true)?;
        ensure!(
            ids.iter()
                .all(|id| self.entity(id).is_some_and(|e| world.contains(e))),
            "loaded scene object was removed outside runtime"
        );
        let owners: Vec<_> = ids.into_iter().collect();
        self.object_destroy_events(world, &owners)?;
        self.object_script_destroy_events(world, &owners)?;
        // Destruction hooks may spawn owned prefabs or remove existing members.
        let ids = self.additive_scenes[handle].members.clone();
        self.remove_objects_raw(world, &ids, true)?;
        self.additive_scenes.remove(handle);
        Ok(())
    }

    pub fn prepare_scene_load(&self, name: &str, additive: bool) -> Result<SceneLoadPlan> {
        let external = self.document.runtime_scene_sources.contains_key(name);
        let level = (!external)
            .then(|| self.document.runtime_scenes.get(name).cloned())
            .flatten();
        ensure!(external || level.is_some(), "unknown runtime scene");
        self.scene_load_plan(name, level, additive)
    }

    pub(crate) fn scene_load_plan(
        &self,
        name: &str,
        level: Option<Arc<Scene>>,
        additive: bool,
    ) -> Result<SceneLoadPlan> {
        ensure!(
            !additive || self.additive_scenes.len() < 64,
            "additive scene limit: 64"
        );
        Ok(SceneLoadPlan {
            instance: self.instance_id,
            base: self.document.clone(),
            level,
            catalog: None,
            bound_scripts: self.scripts.keys().cloned().collect(),
            serial: self
                .scene_serial
                .checked_add(1)
                .context("scene ID space exhausted")?,
            name: name.into(),
            additive,
            checkpoint: None,
            requires_bindings: false,
        })
    }

    /// Accept only at a completed tick. Cancelled or stale work cannot alter the world.
    /// Runtime poses and other state of existing additive entities are retained.
    pub fn accept_scene_load(
        &mut self,
        world: &mut World,
        mut prepared: PreparedScene,
    ) -> Result<String> {
        super::scene_control::require_tick_boundary(world)?;
        prepared.progress.check()?;
        ensure!(
            !prepared.needs_bindings,
            "host did not bind acquired scene dependencies"
        );
        ensure!(
            self.instance_id == prepared.instance
                && self.document == prepared.base
                && self.scene_serial.checked_add(1) == Some(prepared.serial),
            "Scene changed during loading; request the scene again"
        );
        ensure!(
            self.entities.values().all(|entity| world.contains(*entity)),
            "scene entity removed outside runtime"
        );
        let mut kernels = std::mem::take(&mut prepared.bindings.kernels);
        // Loaded IDs are immutable for live objects. Keep already registered kernels.
        kernels.extend(self.compute_kernels.clone());
        compute_runtime::validate_kernels(prepared.document(), &kernels)?;
        let handle = format!("scene-{}", prepared.serial);
        if let Some(mut next) = prepared.replacement.take() {
            self.scene_destroy_events(world)?;
            self.scene_script_destroy_events(world)?;
            // Destruction handlers can create objects, so inspect the current membership.
            for entity in self.entities.values() {
                world.despawn(*entity)?;
            }
            world.remove_resource::<crate::physics::Physics>();
            world.remove_resource::<BlueprintRuntime>();
            world.remove_resource::<crate::ScriptRuntime>();
            crate::middleware::checkpoint::clear(world);
            world.remove_resource::<GameplayState>();
            world.remove_resource::<GameSession>();
            world.insert_resource(GameplayInput::default());
            world.insert_resource(PlayerMotion::default());
            world.insert_resource(CursorCapture::default());
            if let Some(gameplay) = prepared.world.remove_resource::<GameplayState>() {
                world.insert_resource(gameplay);
            }
            middleware::registry::accept_prepared(&mut prepared.world, world);
            let mapping = world.append_entities(prepared.world);
            for entity in next.entities.values_mut() {
                *entity = mapping[entity];
            }
            next.set_gpu_particles(self.gpu_particles_enabled());
            next.templates.extend(std::mem::take(&mut self.templates));
            next.script_engine = std::mem::take(&mut self.script_engine);
            next.scripts = std::mem::take(&mut self.scripts);
            next.compute_kernels = std::mem::take(&mut self.compute_kernels);
            next.compute_capabilities = self.compute_capabilities.clone();
            next.scene_serial = prepared.serial;
            if next.document.game_flow.is_some() {
                world.insert_resource(GameSession {
                    phase: GamePhase::Playing,
                    ..Default::default()
                });
            }
            *self = next;
        } else {
            let candidate = prepared
                .candidate
                .take()
                .expect("prepared additive document");
            middleware::registry::accept_prepared(&mut prepared.world, world);
            let mapping = world.append_entities(prepared.world);
            for (id, entity) in prepared.entities {
                self.entities.insert(id, mapping[&entity]);
            }
            if let Some(runtime) = world.resource_mut::<BlueprintRuntime>() {
                runtime.add_scene_defaults(&candidate.blackboard);
            }
            self.document = candidate;
            self.order = prepared.order;
            self.rebuild_hierarchy_index();
            self.scene_serial = prepared.serial;
            self.additive_scenes.insert(
                handle.clone(),
                LoadedScene {
                    name: prepared.name,
                    members: prepared.members,
                },
            );
            if world.resource::<GameplayState>().is_none()
                && self
                    .document
                    .objects
                    .iter()
                    .any(|o| o.player_controller.is_some())
            {
                self.initialize_gameplay(world);
            }
            self.refresh_collectibles(world);
        }
        for (asset, template) in prepared.bindings.templates {
            self.templates.entry(asset).or_insert(template);
        }
        for (asset, script) in prepared.bindings.scripts {
            self.scripts.entry(asset).or_insert(script);
        }
        self.register_compute_kernels(kernels)?;
        if let Some(checkpoint) = prepared.checkpoint {
            checkpoint.apply(self, world)?;
        }
        for publish in prepared.publications {
            publish(world);
        }
        Ok(handle)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadedScene {
    pub name: String,
    pub members: BTreeSet<String>,
}

pub(crate) fn validate_loaded_scenes(
    groups: &BTreeMap<String, LoadedScene>,
    scene: &Scene,
    serial: u64,
) -> Result<()> {
    ensure!(groups.len() <= 64, "saved additive scene limit: 64");
    let ids: BTreeSet<_> = scene.objects.iter().map(|o| o.id.as_str()).collect();
    let mut owners = BTreeMap::new();
    for (handle, group) in groups {
        let index = handle
            .strip_prefix("scene-")
            .and_then(|s| s.parse::<u64>().ok())
            .context("invalid saved scene handle")?;
        ensure!(
            index > 0
                && index <= serial
                && *handle == format!("scene-{index}")
                && scene.runtime_scenes.contains_key(&group.name),
            "invalid saved scene ownership"
        );
        for id in &group.members {
            ensure!(
                ids.contains(id.as_str()) && owners.insert(id.as_str(), handle).is_none(),
                "invalid or overlapping saved scene membership"
            );
        }
    }
    for prefab in scene.prefabs.values() {
        let mut members = prefab.members.values();
        if let Some(first) = members.next() {
            let owner = owners.get(first.as_str());
            ensure!(
                members.all(|id| owners.get(id.as_str()) == owner),
                "saved prefab crosses scene ownership boundaries"
            );
        }
    }
    Ok(())
}

/// One bounded operation per runtime. The last result remains readable until a new request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadPhase {
    #[default]
    Idle,
    Loading,
    Cancelling,
    Loaded,
    Cancelled,
    Failed,
}
impl LoadPhase {
    pub fn busy(self) -> bool {
        matches!(self, Self::Loading | Self::Cancelling)
    }
}
#[derive(Clone, Debug, Default)]
pub struct LoadStatus {
    pub phase: LoadPhase,
    pub name: String,
    pub additive: bool,
    pub progress: f32,
    pub label: String,
    pub handle: String,
    pub error: String,
}
#[derive(Default)]
struct SceneLoading {
    job: std::sync::Mutex<Option<Job<PreparedScene>>>,
    status: LoadStatus,
}
impl SceneInstance {
    /// Start CPU preparation without stopping simulation. Publication occurs when the host
    /// polls at a completed tick; the shared Blueprint tick already does that in every host.
    pub fn begin_scene_load(&self, world: &mut World, name: &str, additive: bool) -> Result<()> {
        self.begin_prepared_scene_load(world, self.prepare_scene_load(name, additive)?)
    }
    pub(crate) fn begin_prepared_scene_load(
        &self,
        world: &mut World,
        plan: SceneLoadPlan,
    ) -> Result<()> {
        ensure!(
            world
                .resource::<SceneLoading>()
                .is_none_or(|loading| loading
                    .job
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_none()),
            "a scene worker is still active"
        );
        let name = plan.name.clone();
        let additive = plan.additive;
        let job = if plan.requires_acquisition() {
            world
                .resource::<SceneLoaderHandle>()
                .context("this host has no file/content scene loader")?
                .0
                .start(plan, world)?
        } else {
            plan.start()?
        };
        world.insert_resource(SceneLoading {
            job: std::sync::Mutex::new(Some(job)),
            status: LoadStatus {
                phase: LoadPhase::Loading,
                name,
                additive,
                ..Default::default()
            },
        });
        Ok(())
    }
    pub fn scene_load_status(&self, world: &World) -> LoadStatus {
        load_status(world)
    }
    pub fn cancel_scene_load(&self, world: &mut World) {
        if let Some(loading) = world.resource_mut::<SceneLoading>() {
            let job = loading.job.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(active) = job.as_ref() {
                active.cancel();
                loading.status.progress = active.fraction();
                loading.status.phase = LoadPhase::Cancelling;
                loading.status.label = "Cancelling scene load".into();
            }
        }
    }
    pub fn poll_scene_load(&mut self, world: &mut World) -> Result<Option<String>> {
        // A debugger can hold a partially executed Blueprint tick. Never publish into it.
        if world
            .resource::<BlueprintRuntime>()
            .is_some_and(BlueprintRuntime::suspended)
        {
            return Ok(None);
        }
        let Some(mut loading) = world.remove_resource::<SceneLoading>() else {
            return Ok(None);
        };
        let job = loading.job.get_mut().unwrap_or_else(|e| e.into_inner());
        let result = job.as_ref().and_then(Job::poll);
        let result = match result {
            None => Ok(None),
            Some(result) => {
                let result = result.and_then(|prepared| self.accept_scene_load(world, prepared));
                // Keep the cancellation guard alive through publication.
                job.take();
                match result {
                    Ok(handle) => {
                        loading.status.phase = LoadPhase::Loaded;
                        loading.status.progress = 1.;
                        loading.status.label = "Scene loaded".into();
                        loading.status.handle = handle.clone();
                        Ok(Some(handle))
                    }
                    Err(_) if loading.status.phase == LoadPhase::Cancelling => {
                        loading.status.phase = LoadPhase::Cancelled;
                        loading.status.label = "Loading cancelled".into();
                        Ok(None)
                    }
                    Err(error) => {
                        loading.status.phase = LoadPhase::Failed;
                        loading.status.label = "Scene loading failed".into();
                        loading.status.error = format!("{error:#}");
                        Err(error)
                    }
                }
            }
        };
        world.insert_resource(loading);
        result
    }
}

pub(crate) fn load_status(world: &World) -> LoadStatus {
    let Some(loading) = world.resource::<SceneLoading>() else {
        return LoadStatus::default();
    };
    let mut status = loading.status.clone();
    if let Some(job) = loading
        .job
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        status.progress = job.fraction();
        if status.phase == LoadPhase::Loading {
            status.label = job.label();
        }
    }
    status
}
