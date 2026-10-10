//! Rhai host: the coding half of the gameplay-authoring path.
//!
//! Scripts drive the same engine actions blueprints do, so a scene may mix them freely. One
//! interpreter per scene instance compiles each script asset once at load and calls its hooks per
//! tick, so a running tick never parses source or rebuilds a function registry.
//!
//! A script reads this tick's state and *queues* what it wants done; the queue is applied after
//! every script has run, in order. A queued write is visible to later reads in the same tick (the
//! read view is updated as the command is recorded), so `set_position(...)` followed by
//! `get_position(...)` agrees with a blueprint graph.
use super::*;
use blueprint::{BlackboardValue as B, InputKey, ObjectRef, PinType, Value, VariableScope};
use rhai::{
    AST, Array, CallFnOptions, Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position,
    Scope,
};
use std::collections::BTreeSet;
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};
mod animation_api;
mod api;
mod bind;
mod commands;
mod compile;
mod compute_api;
mod convert;
mod imports;
mod loader;
mod module;
mod numeric_archive;
#[cfg(test)]
mod tests;
mod tick;
use api::register;
pub use compile::check_script_sources;
use compile::compile_source;
pub(crate) use compile::compile_sources;
use convert::*;
pub use loader::{load_sources, load_sources_with_progress};
pub use module::{NetworkFrame, NetworkOutbox, NetworkRequest, ScriptModule};

/// Largest accepted script source, matching the blueprint document limit.
const MAX_SCRIPT_BYTES: usize = 1024 * 1024;
/// Scripts a scene may compile, across every object.
pub(crate) const MAX_SCRIPT_ASSETS: usize = 1024;
/// Instructions one hook may run, so a runaway loop fails the tick instead of hanging the game.
const MAX_SCRIPT_OPERATIONS: u64 = 2_000_000;
/// Deepest spatial query result a script may receive.
const MAX_SCRIPT_OVERLAP: usize = 1024;
/// The inspector never retains an unbounded number of per-object counters.
const MAX_ATTACHMENT_STATS: usize = 4096;
/// Prefix of a spawn handle, which scene object IDs do not use.
const SPAWN_PREFIX: &str = "@script/";
static FRESH_SEED_COUNTER: LazyLock<AtomicU64> = LazyLock::new(|| {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    AtomicU64::new(nanos)
});

fn fresh_seed_value() -> rhai::INT {
    // A changing launch seed for procedural scenes. Once stored in a scene blackboard, the
    // scene's own PRNG can remain deterministic for that generated world.
    let mut value = FRESH_SEED_COUNTER.fetch_add(1, Ordering::Relaxed);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^= value >> 31;
    // Fits exactly in the numeric (f32) scene blackboard for save/replay of the layout.
    (value % 16_777_215 + 1) as rhai::INT
}

/// Every hook a script may define, with its parameter names.
///
/// These mirror the blueprint event nodes one for one. `On Input Pressed` has no hook: scripts run
/// every tick, so `input_pressed("jump")` inside `on_update` answers it directly.
const HOOK_SIGNATURES: &[(&str, &[&str])] = &[
    ("on_enable", &["me"]),
    ("on_start", &["me"]),
    ("on_update", &["me", "dt"]),
    ("on_object_enter", &["me", "other"]),
    ("on_object_exit", &["me", "other"]),
    ("on_overlap_enter", &["me"]),
    ("on_overlap_exit", &["me"]),
    ("on_collision_enter", &["me", "other", "normal", "impulse"]),
    ("on_disable", &["me"]),
    ("on_destroy", &["me"]),
    ("network_spawn", &["slot"]),
    ("network_predict", &["player", "pressed", "dt"]),
    ("network_input", &["key"]),
    ("network_pipes", &[]),
    ("network_step", &["pipes", "dt"]),
    ("network_resolve", &["player", "before", "after"]),
    ("network_finished", &["players"]),
    ("network_countdown", &[]),
];
static HOOKS: LazyLock<Vec<(&'static str, usize)>> = LazyLock::new(|| {
    HOOK_SIGNATURES
        .iter()
        .map(|(name, args)| (*name, args.len()))
        .collect()
});

/// Hook names and argument counts accepted by the scene runtime.
pub fn script_hook_descriptions() -> &'static [(&'static str, usize)] {
    HOOKS.as_slice()
}

/// Hook signatures for source completion, from the same table used for runtime arity checks.
pub fn script_hook_signatures() -> &'static [(&'static str, &'static [&'static str])] {
    HOOK_SIGNATURES
}

/// Signatures of the native functions available to Rhai scripts. Build this only for an
/// authoring panel, never in the simulation tick.
pub fn script_function_descriptions() -> Vec<String> {
    let mut signatures = ScriptEngine::new().engine.gen_fn_signatures(false);
    signatures.sort();
    signatures.dedup();
    signatures
}

/// One object's script attachments, as the tick needs them: enabled flag and compiled source.
type Attachments = Vec<(bool, Option<Arc<CompiledScript>>)>;

/// One compiled script asset, shared by every attachment that references it.
#[derive(Clone)]
pub(crate) struct CompiledScript {
    ast: AST,
    hook_ast: AST,
    hooks: BTreeMap<String, usize>,
    fingerprint: u64,
    source: Arc<str>,
    dependencies: BTreeSet<String>,
}

impl CompiledScript {
    /// Whether the script declares this hook with the argument count the engine calls it with.
    fn takes(&self, hook: &str, args: usize) -> bool {
        self.hooks.get(hook) == Some(&args)
    }
}

/// What a script reads about one object this tick.
#[derive(Clone, Default)]
struct ObjectView {
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    text: Option<String>,
    rigidbody: bool,
    grounded: bool,
    overlaps: usize,
    animation: Option<animation_api::View>,
}

/// What a script asked the engine to do, applied in order once every script has run.
enum Command {
    TileView(tile_view::TileView),
    NetworkRequest(NetworkRequest),
    SetVelocity {
        target: String,
        velocity: [f32; 3],
    },
    MoveWithCollision {
        target: String,
        delta: [f32; 3],
    },
    Jump {
        target: String,
        speed: f32,
    },
    ResetInterpolation {
        target: String,
    },
    /// Translate, Rotate, SetPosition, SetRotation or SetScale, matching the blueprint node.
    Transform {
        target: String,
        kind: blueprint::NodeKind,
        value: [f32; 3],
    },
    Color {
        target: String,
        color: [f32; 3],
    },
    Mesh {
        target: String,
        asset: String,
    },
    Text {
        target: String,
        text: String,
    },
    Ui {
        target: String,
        control: middleware::ui::Control,
    },
    Animation {
        target: String,
        control: middleware::animation::Control,
    },
    Visible {
        target: String,
        visible: bool,
    },
    LightIntensity {
        target: String,
        intensity: f32,
    },
    LightColor {
        target: String,
        color: [f32; 3],
    },
    SceneLight {
        ambient: bool,
        color: [f32; 3],
        intensity: f32,
    },
    Environment {
        zenith: [f32; 3],
        horizon: [f32; 3],
        ground: [f32; 3],
        intensity: f32,
    },
    Stars(f32),
    /// Analytic distance fog color and density; density 0 disables the fog.
    Fog {
        color: [f32; 3],
        density: f32,
    },
    /// One of the ten display overrides, named by the blueprint node that sets it.
    Display {
        kind: blueprint::NodeKind,
        value: f32,
    },
    Spawn {
        owner: String,
        token: String,
        asset: String,
        position: [f32; 3],
    },
    Destroy {
        target: String,
    },
    GraphEnabled {
        target: String,
        index: usize,
        enabled: bool,
    },
    ScriptEnabled {
        target: String,
        index: usize,
        enabled: bool,
    },
    Cursor(bool),
    EndGame(String),
    QuitGame,
    CameraSize {
        target: String,
        size: f32,
    },
    SceneControl {
        kind: blueprint::NodeKind,
        name: String,
    },
    Variable {
        scope: VariableScope,
        owner: String,
        name: String,
        value: Value,
    },
    ListVariable {
        scope: VariableScope,
        owner: String,
        name: String,
        values: Vec<Value>,
    },
    Print {
        level: bozzard_diagnostics::Level,
        owner: String,
        text: String,
    },
    Settings(crate::player_settings::Request),
}

/// The read view and the write queue of the scripts running this tick.
#[derive(Default)]
struct Host {
    render: bozzard_diagnostics::RenderMetrics,
    simulation: bozzard_diagnostics::SimulationMetrics,
    network: NetworkFrame,
    network_requests: usize,
    /// The attachment currently running, which bare `me` arguments resolve to.
    owner: String,
    attachment: usize,
    compute: Option<Arc<Mutex<crate::SceneCompute>>>,
    compute_ready: bool,
    compute_capabilities: crate::compute::Capabilities,
    compute_kernels: BTreeMap<String, Arc<crate::compute::Kernel>>,
    dt: f32,
    elapsed: f32,
    loading: crate::scene_loading::LoadStatus,
    /// Edited player settings, including this tick's queued changes.
    settings: crate::player_settings::PlayerSettings,
    input: GameplayInput,
    ui_events: Array,
    ui_pointer: [f32; 2],
    ui_pointer_blocked: bool,
    view_projection: Option<Mat4>,
    /// Held keys of this attachment before the tick, for `input_pressed`.
    held: u128,
    objects: BTreeMap<String, ObjectView>,
    object_boards: BTreeMap<String, BTreeMap<String, B>>,
    scene_board: BTreeMap<String, B>,
    borrowed_boards: bool,
    original_object_values: BTreeMap<String, BTreeMap<String, B>>,
    original_scene_values: BTreeMap<String, B>,
    /// Spawn handles handed out so far, resolved to real IDs as they are created.
    tokens: BTreeMap<String, String>,
    /// Keep handles unique even after a spawned prefab is destroyed.
    next_token_serial: u64,
    geometry: Arc<CollisionSnapshot>,
    budget: usize,
    random: u64,
    commands: Vec<Command>,
}

impl Host {
    fn mirror_variable(&mut self, scope: VariableScope, owner: &str, name: String, value: B) {
        let (board, originals) = match scope {
            VariableScope::Object => (
                self.object_boards
                    .get_mut(owner)
                    .expect("validated object board"),
                &mut self.original_object_values,
            ),
            VariableScope::Scene => {
                let previous = self
                    .scene_board
                    .insert(name.clone(), value)
                    .expect("validated variable");
                if self.borrowed_boards {
                    self.original_scene_values.entry(name).or_insert(previous);
                }
                return;
            }
            VariableScope::Graph => unreachable!("scripts have no graph scope"),
        };
        let previous = board
            .insert(name.clone(), value)
            .expect("validated variable");
        if self.borrowed_boards {
            originals
                .entry(owner.to_owned())
                .or_default()
                .entry(name)
                .or_insert(previous);
        }
    }

    fn return_boards(&mut self, runtime: &mut BlueprintRuntime) {
        self.scene_board.append(&mut self.original_scene_values);
        for (owner, mut originals) in std::mem::take(&mut self.original_object_values) {
            self.object_boards
                .get_mut(&owner)
                .unwrap()
                .append(&mut originals);
        }
        runtime.swap_script_boards(&mut self.scene_board, &mut self.object_boards);
        self.borrowed_boards = false;
    }

    // Destruction callbacks run during command application, where Blueprints also
    // need the boards. Their uncommon path retains an independent read snapshot.
    fn copy_boards(&mut self, runtime: &BlueprintRuntime, document: &Scene) {
        self.object_boards.clear();
        for object in &document.objects {
            if let Some(board) = runtime.object_blackboard(&object.id) {
                self.object_boards.insert(object.id.clone(), board.clone());
            }
        }
        self.scene_board = runtime.scene_blackboard().clone();
    }

    fn view(&self, target: &str) -> Result<&ObjectView, Box<EvalAltResult>> {
        let id = self.object_id(target);
        self.objects
            .get(id)
            .ok_or_else(|| fail(format!("unknown object '{target}'")))
    }
    /// Spawn handles address the object they created, so a script can keep using one.
    fn object_id<'a>(&'a self, target: &'a str) -> &'a str {
        if target.starts_with(SPAWN_PREFIX) {
            self.tokens.get(target).map_or(target, String::as_str)
        } else {
            target
        }
    }
    /// Resolve a target a command will act on, so a bad ID fails at the call site.
    fn target_of(&self, target: &str) -> Result<String, Box<EvalAltResult>> {
        let id = self.object_id(target);
        ensure_script(self.objects.contains_key(id), || {
            format!("unknown object '{target}'")
        })?;
        Ok(id.to_owned())
    }
    fn board(
        &self,
        scope: VariableScope,
        owner: &str,
    ) -> Result<&BTreeMap<String, B>, Box<EvalAltResult>> {
        match scope {
            VariableScope::Object => self.object_boards.get(owner).ok_or_else(|| {
                fail(format!(
                    "'{owner}' has no object blackboard to hold variables"
                ))
            }),
            VariableScope::Scene => Ok(&self.scene_board),
            VariableScope::Graph => Err(fail(
                "scripts have no graph scope; use object or scene variables",
            )),
        }
    }
    fn record(&mut self, command: Command) {
        self.commands.push(command);
    }
    /// Keep the read view in step with a queued transform, so later reads in this tick see it.
    fn mirror_transform(&mut self, target: &str, kind: blueprint::NodeKind, value: [f32; 3]) {
        use blueprint::NodeKind as K;
        let id = self.object_id(target).to_owned();
        let Some(view) = self.objects.get_mut(&id) else {
            return;
        };
        match kind {
            K::Translate => {
                view.position = (Vec3::from(view.position) + Vec3::from(value)).to_array()
            }
            K::Rotate => {
                view.rotation = (Vec3::from(view.rotation) + Vec3::from(value))
                    .to_array()
                    .map(|r| r.rem_euclid(360.))
            }
            K::SetPosition => view.position = value,
            K::SetRotation => view.rotation = value,
            K::SetScale => view.scale = value,
            _ => (),
        }
    }
    /// A queued text or visibility write is readable through `get_text` in the same tick.
    fn mirror_text(&mut self, target: &str, text: String) {
        let id = self.object_id(target).to_owned();
        if let Some(view) = self.objects.get_mut(&id) {
            view.text = Some(text);
        }
    }
}

/// Per-attachment state that outlives a tick, mirroring a blueprint `Run`.
#[derive(Clone, Default)]
struct ScriptRun {
    started: bool,
    enabled: bool,
    held: u128,
    overlap: BTreeSet<String>,
    collisions: BTreeSet<String>,
    /// Top-level constants and any state a script keeps at global scope.
    scope: Scope<'static>,
    scope_initialized: bool,
}

#[derive(Clone, Default)]
pub struct ScriptRuntimeStats {
    /// Hook calls made during the last tick.
    pub hooks: usize,
    /// Commands the last tick queued.
    pub commands: usize,
    /// Last tick's counters, keyed by object ID and Script Manager attachment index.
    pub attachments: BTreeMap<(String, usize), ScriptAttachmentStats>,
    /// More attachments ran than the bounded snapshot can display.
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptAttachmentStats {
    pub hooks: usize,
    pub commands: usize,
}

/// An edit request identifies the exact running scene and attachment set it was made for.
/// The worker owns a source snapshot; compiled assets retain source for dependent reloads.
pub struct ScriptReloadRequest {
    asset: String,
    sources: BTreeMap<String, String>,
    baseline: BTreeMap<String, Arc<CompiledScript>>,
    revisions: BTreeMap<String, u64>,
    instance: u64,
    serial: u64,
    revision: u64,
    attachments: Vec<(String, usize, String)>,
}

/// Fully compiled candidate. Publishing swaps affected assets and resets their consumer scopes
/// between completed simulation ticks; compilation and import resolution happen on the worker.
pub struct ScriptReloadCandidate {
    asset: String,
    instance: u64,
    serial: u64,
    revision: u64,
    attachments: Vec<(String, usize, String)>,
    compiled: BTreeMap<String, Arc<CompiledScript>>,
    stamps: BTreeMap<String, (u64, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptReloadStatus {
    Applied { asset: String, revision: u64 },
    Stale { asset: String, reason: String },
}

impl ScriptReloadRequest {
    pub fn asset(&self) -> &str {
        &self.asset
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Compile on a worker. Errors include the asset and Rhai source position.
    pub fn start(self) -> Result<bozzard_app::job::Job<ScriptReloadCandidate>> {
        bozzard_app::job::Job::start("Compiling script", move |progress| {
            progress.check()?;
            let all = compile_sources(self.sources, &progress)?;
            let compiled: BTreeMap<_, _> = all
                .iter()
                .filter(|(id, script)| {
                    *id == &self.asset || script.dependencies.contains(&self.asset)
                })
                .map(|(id, script)| (id.clone(), script.clone()))
                .collect();
            let mut reads: BTreeSet<_> = compiled.keys().cloned().collect();
            for script in compiled.values() {
                reads.extend(script.dependencies.iter().cloned());
            }
            let stamps = reads
                .into_iter()
                .map(|id| {
                    let stamp = (
                        self.revisions.get(&id).copied().unwrap_or_default(),
                        self.baseline[&id].fingerprint,
                    );
                    (id, stamp)
                })
                .collect();
            progress.check()?;
            Ok(ScriptReloadCandidate {
                asset: self.asset,
                instance: self.instance,
                serial: self.serial,
                revision: self.revision,
                attachments: self.attachments,
                compiled,
                stamps,
            })
        })
    }
}

/// Resource holding every attachment's state across ticks.
#[derive(Clone, Default)]
pub struct ScriptRuntime {
    runs: BTreeMap<(String, usize), ScriptRun>,
    /// The floor contact of each object's last `move_with_collision`, which is what a graph reads
    /// from the move node's own Grounded output. Objects that never move keep using the physics
    /// state of their Gravity/Rigidbody component instead.
    moved: BTreeMap<String, bool>,
    tokens: BTreeMap<String, String>,
    messages: VecDeque<String>,
    elapsed: f32,
    pub stats: ScriptRuntimeStats,
}

impl ScriptRuntime {
    pub(crate) fn remove_objects(&mut self, ids: &BTreeSet<String>) {
        self.runs.retain(|(id, _), _| !ids.contains(id));
        self.moved.retain(|id, _| !ids.contains(id));
        self.tokens.retain(|_, id| !ids.contains(id));
        for run in self.runs.values_mut() {
            run.overlap.retain(|id| !ids.contains(id));
            run.collisions.retain(|id| !ids.contains(id));
        }
    }

    /// Script output, oldest first, bounded like blueprint messages.
    pub fn messages(&self) -> impl Iterator<Item = &str> {
        self.messages.iter().map(String::as_str)
    }
}

// ---------------------------------------------------------------- interpreter

/// The Rhai interpreter of one scene instance, with the read view its native functions see.
pub(crate) struct ScriptEngine {
    engine: Engine,
    host: Arc<Mutex<Host>>,
}

impl ScriptEngine {
    fn new() -> Self {
        let host = Arc::new(Mutex::new(Host::default()));
        let engine = register(host.clone());
        Self { engine, host }
    }
    /// The lock is only ever held for one native call, so poisoning cannot be observed.
    fn lock(&self) -> std::sync::MutexGuard<'_, Host> {
        self.host.lock().unwrap_or_else(|error| error.into_inner())
    }
}
