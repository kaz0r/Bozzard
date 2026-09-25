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
use std::sync::{Arc, Mutex};
mod compute_api;
mod module;
pub use module::{NetworkFrame, ScriptModule};

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

/// Every hook a script may define, with its parameter count.
///
/// These mirror the blueprint event nodes one for one. `On Input Pressed` has no hook: scripts run
/// every tick, so `input_pressed("jump")` inside `on_update` answers it directly.
const HOOKS: &[(&str, usize)] = &[
    ("on_enable", 1),
    ("on_start", 1),
    ("on_update", 2),
    ("on_object_enter", 2),
    ("on_object_exit", 2),
    ("on_overlap_enter", 1),
    ("on_overlap_exit", 1),
    ("on_collision_enter", 4),
    ("on_disable", 1),
    ("on_destroy", 1),
    ("network_spawn", 1),
    ("network_predict", 3),
    ("network_input", 1),
    ("network_pipes", 0),
    ("network_step", 2),
    ("network_resolve", 3),
    ("network_finished", 1),
    ("network_countdown", 0),
];

/// Hook names and argument counts accepted by the scene runtime.
pub fn script_hook_descriptions() -> &'static [(&'static str, usize)] {
    HOOKS
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
    hooks: BTreeMap<String, usize>,
    fingerprint: u64,
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
    forward: [f32; 3],
    text: Option<String>,
    rigidbody: bool,
    grounded: bool,
    overlaps: usize,
}

/// What a script asked the engine to do, applied in order once every script has run.
enum Command {
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
    Text {
        target: String,
        text: String,
    },
    Visible {
        target: String,
        visible: bool,
    },
    LightIntensity {
        target: String,
        intensity: f32,
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
    Print {
        level: bozzard_diagnostics::Level,
        owner: String,
        text: String,
    },
}

/// The read view and the write queue of the scripts running this tick.
#[derive(Default)]
struct Host {
    network: NetworkFrame,
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
    input: GameplayInput,
    /// Held keys of this attachment before the tick, for `input_pressed`.
    held: u128,
    objects: BTreeMap<String, ObjectView>,
    object_boards: BTreeMap<String, BTreeMap<String, B>>,
    scene_board: BTreeMap<String, B>,
    /// Spawn handles handed out so far, resolved to real IDs as they are created.
    tokens: BTreeMap<String, String>,
    geometry: Arc<CollisionSnapshot>,
    budget: usize,
    random: u64,
    commands: Vec<Command>,
}

impl Host {
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
            _ => return,
        }
        view.forward = crate::physics::forward(&Transform {
            translation: view.position,
            rotation_degrees: view.rotation,
            scale: view.scale,
        })
        .to_array();
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
/// Source is owned by the background compile job and is discarded before publication.
pub struct ScriptReloadRequest {
    asset: String,
    source: String,
    instance: u64,
    serial: u64,
    revision: u64,
    attachments: Vec<(String, usize)>,
}

/// Fully compiled candidate. Publishing it is a constant-time asset swap plus bounded scope
/// reset, and must happen between completed simulation ticks.
pub struct ScriptReloadCandidate {
    asset: String,
    instance: u64,
    serial: u64,
    revision: u64,
    attachments: Vec<(String, usize)>,
    compiled: Arc<CompiledScript>,
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
            let compiled = compile_source(&ScriptEngine::new(), &self.asset, &self.source)?;
            progress.check()?;
            Ok(ScriptReloadCandidate {
                asset: self.asset,
                instance: self.instance,
                serial: self.serial,
                revision: self.revision,
                attachments: self.attachments,
                compiled,
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

fn fail(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(message.into()),
        Position::NONE,
    ))
}

fn ensure_script(condition: bool, message: impl Fn() -> String) -> Result<(), Box<EvalAltResult>> {
    if condition {
        Ok(())
    } else {
        Err(fail(message()))
    }
}

fn array_of(vector: [f32; 3]) -> Array {
    vector.iter().map(|v| Dynamic::from(*v)).collect()
}

fn vector_of(value: Array) -> Result<[f32; 3], Box<EvalAltResult>> {
    ensure_script(value.len() == 3, || {
        format!("expected a vector of 3 numbers, got {}", value.len())
    })?;
    let mut out = [0.; 3];
    for (slot, value) in out.iter_mut().zip(value) {
        *slot = value
            .as_float()
            .map_err(|_| fail("vector components must be numbers"))?;
        ensure_script(slot.is_finite(), || "vector must be finite".into())?;
    }
    Ok(out)
}

fn number_of(value: Dynamic, what: &str) -> Result<f32, Box<EvalAltResult>> {
    let number = value
        .as_float()
        .map_err(|_| fail(format!("{what} must be a number")))?;
    ensure_script(number.is_finite(), || format!("{what} must be finite"))?;
    Ok(number)
}

fn text_of(value: Dynamic, what: &str) -> Result<String, Box<EvalAltResult>> {
    value
        .into_string()
        .map_err(|_| fail(format!("{what} must be text")))
}

/// Convert a script value into the kind a blackboard variable declares.
fn scalar_of(value: Dynamic, kind: PinType) -> Result<Value, Box<EvalAltResult>> {
    ensure_script(kind != PinType::Exec, || {
        "Exec cannot be stored in a variable".into()
    })?;
    Ok(match kind {
        PinType::Number => Value::Number(number_of(value, "variable")?),
        PinType::Bool => Value::Bool(
            value
                .as_bool()
                .map_err(|_| fail("variable must be a boolean"))?,
        ),
        PinType::Text => Value::Text(text_of(value, "variable")?),
        PinType::Vector => Value::Vector(vector_of(
            value
                .into_array()
                .map_err(|_| fail("variable must be a vector"))?,
        )?),
        PinType::Object => {
            if value.is_unit() {
                Value::Object(ObjectRef::None)
            } else {
                Value::Object(ObjectRef::Id(text_of(value, "object reference")?))
            }
        }
        PinType::Exec => unreachable!(),
    })
}

fn dynamic_of(value: &Value) -> Dynamic {
    match value {
        Value::Text(text) => Dynamic::from(text.clone()),
        Value::Number(number) => Dynamic::from(*number),
        Value::Bool(flag) => Dynamic::from(*flag),
        Value::Vector(vector) => Dynamic::from(array_of(*vector)),
        Value::Object(ObjectRef::Id(id)) => Dynamic::from(id.clone()),
        _ => Dynamic::UNIT,
    }
}

/// Registers every engine function a script may call onto one interpreter.
///
/// Reads return a value; writes queue a [`Command`]. Both are declared as one expression over
/// `state`, so a function and its blueprint node stay easy to compare.
fn register(host: Arc<Mutex<Host>>) -> Engine {
    let mut engine = Engine::new();
    engine
        .set_max_operations(MAX_SCRIPT_OPERATIONS)
        .set_max_call_levels(32)
        .set_max_expr_depths(64, 64)
        .set_max_string_size(MAX_SCRIPT_BYTES)
        .set_max_array_size(1 << 16)
        .set_max_map_size(1 << 16);
    compute_api::register(&mut engine, host.clone());
    macro_rules! borrow {
        ($host:expr) => {
            $host.lock().unwrap_or_else(|error| error.into_inner())
        };
    }
    macro_rules! read {
        ($name:expr, ($($arg:ident : $type:ty),*), |$state:ident| $body:expr) => {{
            let host = host.clone();
            engine.register_fn(
                $name,
                move |$($arg: $type),*| -> Result<Dynamic, Box<EvalAltResult>> {
                    let $state = borrow!(host);
                    let $state: &Host = &$state;
                    $body
                },
            );
        }};
    }
    macro_rules! write {
        ($name:expr, ($($arg:ident : $type:ty),*), |$state:ident| $body:expr) => {{
            let host = host.clone();
            engine.register_fn(
                $name,
                move |$($arg: $type),*| -> Result<(), Box<EvalAltResult>> {
                    let mut $state = borrow!(host);
                    let command: Command = $body;
                    $state.record(command);
                    Ok(())
                },
            );
        }};
    }

    // Object and clock reads, one per blueprint query node.
    read!("network_active", (), |state| Ok(Dynamic::from(
        state.network.active
    )));
    read!("network_state", (), |state| rhai::serde::to_dynamic(
        &state.network.state
    ));
    read!("network_object", (target: ImmutableString), |state| {
        match state.network.objects.get(target.as_str()) {
            Some(value) => rhai::serde::to_dynamic(value),
            None => Ok(Dynamic::from(Map::new())),
        }
    });
    read!("delta_time", (), |state| Ok(Dynamic::from(state.dt)));
    read!("elapsed_time", (), |state| Ok(Dynamic::from(state.elapsed)));
    read!("scene_loading", (), |state| Ok(Dynamic::from(
        state.loading.phase.busy()
    )));
    read!("scene_load_progress", (), |state| Ok(Dynamic::from(
        state.loading.progress
    )));
    read!("loaded_scene_handle", (), |state| Ok(Dynamic::from(
        state.loading.handle.clone()
    )));
    read!("scene_load_error", (), |state| Ok(Dynamic::from(
        state.loading.error.clone()
    )));
    read!(
        "is_valid_object",
        (target: ImmutableString), |state|
        Ok(Dynamic::from(
            state.objects.contains_key(state.object_id(&target))
        ))
    );
    read!(
        "is_rigidbody",
        (target: ImmutableString), |state|
        Ok(Dynamic::from(state.view(&target)?.rigidbody))
    );
    read!(
        "is_grounded",
        (target: ImmutableString), |state|
        Ok(Dynamic::from(state.view(&target)?.grounded))
    );
    read!(
        "same_object",
        (a: ImmutableString, b: ImmutableString), |state|
        Ok(Dynamic::from(state.object_id(&a) == state.object_id(&b)))
    );
    read!(
        "get_position",
        (target: ImmutableString), |state|
        Ok(Dynamic::from_array(array_of(state.view(&target)?.position)))
    );
    read!(
        "get_rotation",
        (target: ImmutableString), |state|
        Ok(Dynamic::from_array(array_of(state.view(&target)?.rotation)))
    );
    read!(
        "get_scale",
        (target: ImmutableString), |state|
        Ok(Dynamic::from_array(array_of(state.view(&target)?.scale)))
    );
    read!(
        "forward_vector",
        (target: ImmutableString), |state|
        Ok(Dynamic::from_array(array_of(state.view(&target)?.forward)))
    );
    read!("get_text", (target: ImmutableString), |state| {
        let text = state
            .view(&target)?
            .text
            .clone()
            .ok_or_else(|| fail(format!("'{target}' has no Text Rendering")))?;
        Ok(Dynamic::from(text))
    });
    read!(
        "overlap_count",
        (target: ImmutableString), |state|
        Ok(Dynamic::from(state.view(&target)?.overlaps as f32))
    );

    // Input, including the held-key edge that `On Input Pressed` provides in a graph.
    for (name, pressed) in [("input_held", false), ("input_pressed", true)] {
        read!(name, (key: ImmutableString), |state| {
            let key = InputKey::parse(&key).map_err(|error| fail(format!("{error:#}")))?;
            Ok(Dynamic::from(if pressed {
                key.pressed(state.input, state.held)
            } else {
                key.active(state.input)
            }))
        });
    }
    read!("move_x", (), |state| Ok(Dynamic::from(
        state.input.movement[0]
    )));
    read!("move_y", (), |state| Ok(Dynamic::from(
        state.input.movement[1]
    )));
    read!("mouse_x", (), |state| Ok(Dynamic::from(
        state.input.orbit[0]
    )));
    read!("mouse_y", (), |state| Ok(Dynamic::from(
        state.input.orbit[1]
    )));

    // Blackboards, shared with graphs on the same object or scene.
    for (name, scope) in [
        ("get_object_variable", VariableScope::Object),
        ("get_scene_variable", VariableScope::Scene),
    ] {
        read!(name, (variable: ImmutableString), |state| {
            let entry = state
                .board(scope, &state.owner)?
                .get(variable.as_str())
                .ok_or_else(|| fail(format!("unknown variable '{variable}'")))?;
            match entry {
                B::Scalar(value) => Ok(dynamic_of(value)),
                B::List { .. } => Err(fail(format!(
                    "variable '{variable}' is a list; scripts keep their own arrays"
                ))),
            }
        });
    }

    // Seeded randomness, matching the `Random` node.
    {
        let host = host.clone();
        engine.register_fn(
            "random",
            move |min: f32, max: f32| -> Result<f32, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                ensure_script(min <= max && (max - min).is_finite(), || {
                    "random needs finite min <= max".into()
                })?;
                state.random = state
                    .random
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let fraction = (state.random >> 40) as f32 / 16777216.;
                Ok(min + (max - min) * fraction)
            },
        );
    }

    // Spatial queries, sharing the collision snapshot the tick already built.
    {
        let host = host.clone();
        engine.register_fn(
            "raycast",
            move |origin: Array,
                  direction: Array,
                  distance: f32,
                  ignore: ImmutableString|
                  -> Result<Map, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                let origin = Vec3::from(vector_of(origin)?);
                let direction = Vec3::from(vector_of(direction)?);
                let ignore = state.object_id(&ignore).to_owned();
                let geometry = state.geometry.clone();
                let hit = geometry
                    .raycast_budget(
                        origin,
                        direction,
                        distance,
                        (!ignore.is_empty()).then_some(ignore.as_str()),
                        u32::MAX,
                        &mut state.budget,
                    )
                    .map_err(|error| fail(format!("{error:#}")))?;
                let (object, position, normal, distance) = match hit {
                    Some(hit) => (
                        hit.object,
                        hit.position.to_array(),
                        hit.normal.to_array(),
                        hit.distance,
                    ),
                    None => (String::new(), [0.; 3], [0.; 3], 0.),
                };
                let mut map = Map::new();
                map.insert("hit".into(), Dynamic::from(!object.is_empty()));
                map.insert("object".into(), Dynamic::from(object));
                map.insert("position".into(), Dynamic::from_array(array_of(position)));
                map.insert("normal".into(), Dynamic::from_array(array_of(normal)));
                map.insert("distance".into(), Dynamic::from(distance));
                Ok(map)
            },
        );
    }
    for (name, sphere) in [("sphere_overlap", true), ("box_overlap", false)] {
        let host = host.clone();
        engine.register_fn(
            name,
            move |center: Array,
                  size: Dynamic,
                  ignore: ImmutableString|
                  -> Result<Array, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                let center = Vec3::from(vector_of(center)?);
                let ignore = state.object_id(&ignore).to_owned();
                let geometry = state.geometry.clone();
                let ignore = (!ignore.is_empty()).then_some(ignore.as_str());
                let hits = if sphere {
                    let radius = number_of(size, "radius")?;
                    geometry.overlap_sphere_budget(
                        center,
                        radius,
                        ignore,
                        u32::MAX,
                        MAX_SCRIPT_OVERLAP,
                        &mut state.budget,
                    )
                } else {
                    let size = vector_of(
                        size.into_array()
                            .map_err(|_| fail("box size must be a vector"))?,
                    )?;
                    geometry.overlap_box_budget(
                        center,
                        Vec3::from(size),
                        ignore,
                        u32::MAX,
                        MAX_SCRIPT_OVERLAP,
                        &mut state.budget,
                    )
                }
                .map_err(|error| fail(format!("{error:#}")))?;
                Ok(hits.into_iter().map(Dynamic::from).collect())
            },
        );
    }
    {
        let host = host.clone();
        engine.register_fn(
            "line_of_sight",
            move |from: Array,
                  to: Array,
                  ignore: ImmutableString|
                  -> Result<bool, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                let from = Vec3::from(vector_of(from)?);
                let delta = Vec3::from(vector_of(to)?) - from;
                let ignore = state.object_id(&ignore).to_owned();
                if delta == Vec3::ZERO {
                    return Ok(true);
                }
                let geometry = state.geometry.clone();
                let blocked = geometry
                    .raycast_budget(
                        from,
                        delta,
                        delta.length(),
                        (!ignore.is_empty()).then_some(ignore.as_str()),
                        u32::MAX,
                        &mut state.budget,
                    )
                    .map_err(|error| fail(format!("{error:#}")))?;
                Ok(blocked.is_none())
            },
        );
    }

    // Writes: every one queues a command instead of touching the world mid-tick.
    write!(
        "set_velocity",
        (target: ImmutableString, velocity: Array), |state|
        Command::SetVelocity {
            target: state.target_of(&target)?,
            velocity: vector_of(velocity)?,
        }
    );
    write!(
        "move_with_collision",
        (target: ImmutableString, delta: Array), |state|
        Command::MoveWithCollision {
            target: state.target_of(&target)?,
            delta: vector_of(delta)?,
        }
    );
    write!("jump", (target: ImmutableString, speed: f32), |state| {
        ensure_script(speed > 0., || "jump speed must be positive".into())?;
        Command::Jump {
            target: state.target_of(&target)?,
            speed,
        }
    });
    for (name, kind) in [
        ("translate", blueprint::NodeKind::Translate),
        ("rotate", blueprint::NodeKind::Rotate),
        ("set_position", blueprint::NodeKind::SetPosition),
        ("set_rotation", blueprint::NodeKind::SetRotation),
        ("set_scale", blueprint::NodeKind::SetScale),
    ] {
        write!(name, (target: ImmutableString, value: Array), |state| {
            let value = vector_of(value)?;
            let target = state.target_of(&target)?;
            state.mirror_transform(&target, kind, value);
            Command::Transform {
                target,
                kind,
                value,
            }
        });
    }
    write!("set_color", (target: ImmutableString, color: Array), |state| {
        let color = vector_of(color)?;
        ensure_script(color.iter().all(|c| (0.0..=1.0).contains(c)), || {
            "colour components must be in 0..1".into()
        })?;
        Command::Color {
            target: state.target_of(&target)?,
            color,
        }
    });
    write!(
        "set_visible",
        (target: ImmutableString, visible: bool), |state|
        Command::Visible {
            target: state.target_of(&target)?,
            visible,
        }
    );
    write!(
        "set_text",
        (target: ImmutableString, text: ImmutableString), |state|
        {
            ensure_script(text.len() <= 4096, || "text exceeds 4096 UTF-8 bytes".into())?;
            let target = state.target_of(&target)?;
            let text = text.to_string();
            // A later read in this tick sees the queued text.
            state.mirror_text(&target, text.clone());
            Command::Text { target, text }
        }
    );
    write!(
        "set_light_intensity",
        (target: ImmutableString, intensity: f32), |state|
        Command::LightIntensity {
            target: state.target_of(&target)?,
            intensity,
        }
    );
    for (name, kind) in [
        ("set_focus_distance", blueprint::NodeKind::SetFocusDistance),
        ("set_aperture", blueprint::NodeKind::SetAperture),
        ("set_fog_density", blueprint::NodeKind::SetFogDensity),
        (
            "set_fog_light_intensity",
            blueprint::NodeKind::SetFogLightIntensity,
        ),
        ("set_exposure", blueprint::NodeKind::SetExposure),
        (
            "set_bloom_intensity",
            blueprint::NodeKind::SetBloomIntensity,
        ),
        ("set_saturation", blueprint::NodeKind::SetSaturation),
        ("set_heat_strength", blueprint::NodeKind::SetHeatStrength),
        (
            "set_grain_intensity",
            blueprint::NodeKind::SetGrainIntensity,
        ),
        (
            "set_vignette_intensity",
            blueprint::NodeKind::SetVignetteIntensity,
        ),
    ] {
        let host = host.clone();
        engine.register_fn(name, move |value: f32| -> Result<(), Box<EvalAltResult>> {
            let mut state = borrow!(host);
            ensure_script(value.is_finite(), || "value must be finite".into())?;
            state.record(Command::Display { kind, value });
            Ok(())
        });
    }
    {
        let host = host.clone();
        engine.register_fn(
            "spawn_prefab",
            move |asset: ImmutableString,
                  position: Array|
                  -> Result<ImmutableString, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                let position = vector_of(position)?;
                let serial = state
                    .tokens
                    .keys()
                    .filter(|key| key.starts_with(SPAWN_PREFIX))
                    .count();
                let token = format!("{SPAWN_PREFIX}{}/{serial}", state.owner);
                let owner = state.owner.clone();
                state.record(Command::Spawn {
                    owner,
                    token: token.clone(),
                    asset: asset.to_string(),
                    position,
                });
                // The handle is a real target for the rest of the tick and for later ticks.
                state.tokens.insert(token.clone(), token.clone());
                state.objects.insert(
                    token.clone(),
                    ObjectView {
                        position,
                        scale: [1.; 3],
                        ..Default::default()
                    },
                );
                Ok(token.into())
            },
        );
    }
    write!(
        "destroy_prefab",
        (target: ImmutableString), |state|
        Command::Destroy {
            target: state.target_of(&target)?,
        }
    );
    for (name, script) in [("set_graph_enabled", false), ("set_script_enabled", true)] {
        write!(
            name,
            (target: ImmutableString, index: i64, enabled: bool), |state|
            {
                ensure_script(index >= 0, || "attachment index must be nonnegative".into())?;
                let target = state.target_of(&target)?;
                if script {
                    Command::ScriptEnabled {
                        target,
                        index: index as usize,
                        enabled,
                    }
                } else {
                    Command::GraphEnabled {
                        target,
                        index: index as usize,
                        enabled,
                    }
                }
            }
        );
    }
    for (name, requested) in [("lock_cursor", true), ("unlock_cursor", false)] {
        let host = host.clone();
        engine.register_fn(name, move || {
            borrow!(host).record(Command::Cursor(requested));
        });
    }
    {
        let host = host.clone();
        engine.register_fn("end_game", move |message: ImmutableString| {
            borrow!(host).record(Command::EndGame(message.to_string()));
        });
    }
    for (name, kind) in [
        ("load_scene", blueprint::NodeKind::LoadScene),
        ("add_scene", blueprint::NodeKind::AddScene),
        ("load_scene_async", blueprint::NodeKind::LoadSceneAsync),
        ("add_scene_async", blueprint::NodeKind::AddSceneAsync),
        ("unload_scene", blueprint::NodeKind::UnloadScene),
        ("save_game", blueprint::NodeKind::SaveGame),
        ("load_game", blueprint::NodeKind::LoadGame),
    ] {
        let host = host.clone();
        engine.register_fn(name, move |scene: ImmutableString| {
            borrow!(host).record(Command::SceneControl {
                kind,
                name: scene.to_string(),
            });
        });
    }
    {
        let host = host.clone();
        engine.register_fn("cancel_scene_load", move || {
            borrow!(host).record(Command::SceneControl {
                kind: blueprint::NodeKind::CancelSceneLoad,
                name: String::new(),
            });
        });
    }
    {
        let host = host.clone();
        engine.register_fn("restart_scene", move || {
            borrow!(host).record(Command::SceneControl {
                kind: blueprint::NodeKind::RestartScene,
                name: String::new(),
            });
        });
    }
    for (name, scope) in [
        ("set_object_variable", VariableScope::Object),
        ("set_scene_variable", VariableScope::Scene),
    ] {
        write!(
            name,
            (variable: ImmutableString, value: Dynamic), |state|
            {
                let owner = state.owner.clone();
                let declared = match state.board(scope, &owner)?.get(variable.as_str()) {
                    Some(B::Scalar(declared)) => declared.kind(),
                    Some(B::List { .. }) => {
                        return Err(fail(format!(
                            "variable '{variable}' is a list; scripts keep their own arrays"
                        )));
                    }
                    None => return Err(fail(format!("unknown variable '{variable}'"))),
                };
                let value = scalar_of(value, declared)?;
                if scope == VariableScope::Object {
                    state
                        .object_boards
                        .get_mut(&owner)
                        .expect("object board")
                        .insert(variable.to_string(), B::Scalar(value.clone()));
                } else {
                    state
                        .scene_board
                        .insert(variable.to_string(), B::Scalar(value.clone()));
                }
                Command::Variable {
                    scope,
                    owner,
                    name: variable.to_string(),
                    value,
                }
            }
        );
    }
    // `print` is a Rhai keyword, so it is captured through the engine's own output hook rather
    // than registered as a function.
    {
        let host = host.clone();
        engine.on_print(move |text| {
            let mut host = borrow!(host);
            let owner = host.owner.clone();
            host.record(Command::Print {
                level: bozzard_diagnostics::Level::Info,
                owner,
                text: text.to_owned(),
            });
        });
    }

    for (name, level) in [
        ("log_info", bozzard_diagnostics::Level::Info),
        ("log_warning", bozzard_diagnostics::Level::Warning),
        ("log_error", bozzard_diagnostics::Level::Error),
    ] {
        let host = host.clone();
        engine.register_fn(name, move |text: ImmutableString| {
            let mut host = borrow!(host);
            let owner = host.owner.clone();
            host.record(Command::Print {
                level,
                owner,
                text: text.to_string(),
            });
        });
    }

    // The scalar vocabulary of a blueprint graph that Rhai does not already provide.
    engine.register_fn("lerp", |a: f32, b: f32, t: f32| -> f32 {
        a * (1. - t) + b * t
    });
    engine.register_fn(
        "lerp_vector",
        |a: Array, b: Array, t: f32| -> Result<Array, Box<EvalAltResult>> {
            let (a, b) = (Vec3::from(vector_of(a)?), Vec3::from(vector_of(b)?));
            Ok(array_of((a * (1. - t) + b * t).to_array()))
        },
    );
    engine.register_fn(
        "clamp",
        |value: f32, min: f32, max: f32| -> Result<f32, Box<EvalAltResult>> {
            ensure_script(min <= max, || "clamp needs min <= max".into())?;
            Ok(value.clamp(min, max))
        },
    );
    engine.register_fn(
        "length",
        |value: Array| -> Result<f32, Box<EvalAltResult>> {
            Ok(Vec3::from(vector_of(value)?).length())
        },
    );
    engine.register_fn(
        "normalize",
        |value: Array| -> Result<Array, Box<EvalAltResult>> {
            Ok(array_of(
                Vec3::from(vector_of(value)?).normalize_or_zero().to_array(),
            ))
        },
    );
    engine.register_fn(
        "dot",
        |a: Array, b: Array| -> Result<f32, Box<EvalAltResult>> {
            Ok(Vec3::from(vector_of(a)?).dot(Vec3::from(vector_of(b)?)))
        },
    );
    engine.register_fn(
        "cross",
        |a: Array, b: Array| -> Result<Array, Box<EvalAltResult>> {
            Ok(array_of(
                Vec3::from(vector_of(a)?)
                    .cross(Vec3::from(vector_of(b)?))
                    .to_array(),
            ))
        },
    );
    engine.register_fn(
        "distance",
        |a: Array, b: Array| -> Result<f32, Box<EvalAltResult>> {
            Ok(Vec3::from(vector_of(a)?).distance(Vec3::from(vector_of(b)?)))
        },
    );
    engine.register_fn(
        "add_vector",
        |a: Array, b: Array| -> Result<Array, Box<EvalAltResult>> {
            Ok(array_of(
                (Vec3::from(vector_of(a)?) + Vec3::from(vector_of(b)?)).to_array(),
            ))
        },
    );
    engine.register_fn(
        "scale_vector",
        |value: Array, factor: f32| -> Result<Array, Box<EvalAltResult>> {
            Ok(array_of(
                (Vec3::from(vector_of(value)?) * factor).to_array(),
            ))
        },
    );
    macro_rules! axis {
        ($name:literal, $axis:literal) => {
            engine.register_fn($name, |value: Array| -> Result<f32, Box<EvalAltResult>> {
                Ok(vector_of(value)?[$axis])
            });
        };
    }
    axis!("vector_x", 0);
    axis!("vector_y", 1);
    axis!("vector_z", 2);
    engine.register_fn(
        "modulo",
        |a: f32, b: f32| -> Result<f32, Box<EvalAltResult>> {
            ensure_script(b != 0., || "modulo by zero".into())?;
            Ok(a.rem_euclid(b))
        },
    );
    engine.register_fn("pow", |a: f32, b: f32| -> f32 { a.powf(b) });
    engine.register_fn("atan2", |y: f32, x: f32| -> f32 { y.atan2(x) });
    // Rhai spells these `ceiling` and `**`; blueprint authors expect the node names as well.
    engine.register_fn("ceil", |value: f32| -> f32 { value.ceil() });
    engine.register_fn("to_radians", |value: f32| -> f32 { value.to_radians() });
    engine.register_fn("to_degrees", |value: f32| -> f32 { value.to_degrees() });
    engine
}

fn compile_source(engine: &ScriptEngine, asset: &str, source: &str) -> Result<Arc<CompiledScript>> {
    ensure!(
        source.len() <= MAX_SCRIPT_BYTES,
        "script '{asset}' exceeds 1 MiB"
    );
    let ast = engine
        .engine
        .compile(source)
        .map_err(|error| anyhow::anyhow!("script '{asset}': {error}"))?;
    let mut hooks = BTreeMap::new();
    for function in ast.iter_functions() {
        if let Some((name, args)) = HOOKS.iter().find(|(name, _)| *name == function.name) {
            let declaration = format!("fn {name}");
            let line = source
                .lines()
                .position(|line| line.contains(&declaration))
                .map_or(1, |index| index + 1);
            ensure!(
                function.params.len() == *args,
                "script '{asset}' line {line}: {name} takes {args} argument(s), got {}",
                function.params.len()
            );
            hooks.insert(function.name.to_owned(), function.params.len());
        }
    }
    let fingerprint = source.bytes().fold(14695981039346656037u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(1099511628211)
    });
    Ok(Arc::new(CompiledScript {
        ast,
        hooks,
        fingerprint,
    }))
}

pub(crate) fn compile_sources(
    sources: BTreeMap<String, String>,
    progress: &bozzard_app::job::Progress,
) -> Result<BTreeMap<String, Arc<CompiledScript>>> {
    ensure!(
        sources.len() <= MAX_SCRIPT_ASSETS,
        "scene script catalog exceeds its limit"
    );
    ensure!(
        sources.values().map(String::len).sum::<usize>() <= 32 * 1024 * 1024,
        "scripts exceed 32 MiB"
    );
    if sources.is_empty() {
        return Ok(BTreeMap::new());
    }
    let engine = ScriptEngine::new();
    sources
        .into_iter()
        .map(|(id, source)| {
            progress.stage(format!("Compiling script {id}"))?;
            let compiled = compile_source(&engine, &id, &source)?;
            Ok((id, compiled))
        })
        .collect()
}

// ---------------------------------------------------------------- instance binding

impl SceneInstance {
    /// Compiles one script asset and binds it for the attachments that reference it.
    ///
    /// Called for every `script` asset of the scene catalog when the scene loads, so a syntax error
    /// fails where the scene is opened instead of on the first tick that runs it.
    /// Compile every loaded source and fail if an attachment has none.
    ///
    /// This is what a scene loader calls once: the check turns a scene whose catalog and
    /// attachments disagree into an error where the scene is opened, instead of a simulation that
    /// stops on the first tick that runs the script.
    pub fn register_scripts(&mut self, sources: BTreeMap<String, String>) -> Result<()> {
        for (asset, source) in sources {
            self.register_script(asset, source)?;
        }
        for object in &self.document.objects {
            for (index, attachment) in object
                .script_manager
                .iter()
                .flat_map(|manager| &manager.scripts)
                .enumerate()
            {
                let attachment = &attachment.script;
                ensure!(
                    self.scripts.contains_key(attachment),
                    "script '{attachment}' on '{}' (attachment {index}) was not loaded; \
                     the scene catalog does not list it as a script asset",
                    object.id
                );
            }
        }
        Ok(())
    }
    /// Script asset IDs the object's attachments name, in order.
    fn document_attachments(&self, object: &str) -> Vec<String> {
        self.document
            .objects
            .iter()
            .find(|candidate| candidate.id == object)
            .and_then(|candidate| candidate.script_manager.as_ref())
            .map(|manager| {
                manager
                    .scripts
                    .iter()
                    .map(|attachment| attachment.script.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn register_script(&mut self, asset: String, source: String) -> Result<()> {
        ensure!(
            self.document
                .assets
                .get(&asset)
                .is_some_and(|entry| entry.kind == AssetKind::Script),
            "asset '{asset}' is not a script"
        );
        ensure!(
            self.scripts.contains_key(&asset) || self.scripts.len() < MAX_SCRIPT_ASSETS,
            "scene compiles at most {MAX_SCRIPT_ASSETS} scripts"
        );
        let compiled = compile_source(&self.script_engine(), &asset, &source)?;
        *self
            .script_reload_revisions
            .entry(asset.clone())
            .or_default() += 1;
        self.scripts.insert(asset, compiled);
        Ok(())
    }
    /// Reserve a revision before starting background compilation. A newer edit invalidates any
    /// older result, even if the older worker completes last. The caller must reject this route
    /// while multiplayer is active and coordinate a restart instead.
    pub fn request_script_reload(
        &mut self,
        asset: &str,
        source: String,
    ) -> Result<ScriptReloadRequest> {
        ensure!(
            source.len() <= MAX_SCRIPT_BYTES,
            "script '{asset}' exceeds 1 MiB"
        );
        ensure!(
            self.document
                .assets
                .get(asset)
                .is_some_and(|entry| entry.kind == AssetKind::Script),
            "asset '{asset}' is not a script"
        );
        ensure!(
            self.scripts.contains_key(asset),
            "script '{asset}' was not loaded"
        );
        let revision = self
            .script_reload_revisions
            .entry(asset.to_owned())
            .or_default();
        *revision = revision
            .checked_add(1)
            .context("script edit revision exhausted")?;
        let attachments = self
            .document
            .objects
            .iter()
            .flat_map(|object| {
                object
                    .script_manager
                    .iter()
                    .flat_map(|manager| manager.scripts.iter().enumerate())
                    .filter(move |(_, attachment)| attachment.script == asset)
                    .map(move |(index, _)| (object.id.clone(), index))
            })
            .collect();
        Ok(ScriptReloadRequest {
            asset: asset.to_owned(),
            source,
            instance: self.instance_id,
            serial: self.scene_serial,
            revision: *revision,
            attachments,
        })
    }
    /// Publish only at a completed tick boundary. This does not call lifecycle hooks, discard
    /// queued actions or reset world/blackboard state. Each matching attachment retains its
    /// started/enabled/input/contact state and gets a fresh script-local scope.
    pub fn publish_script_reload(
        &mut self,
        world: &mut World,
        candidate: ScriptReloadCandidate,
    ) -> Result<ScriptReloadStatus> {
        crate::scene_control::require_tick_boundary(world)?;
        let stale = if self.instance_id != candidate.instance
            || self.scene_serial != candidate.serial
        {
            Some("scene changed")
        } else if self.script_reload_revisions.get(&candidate.asset) != Some(&candidate.revision) {
            Some("newer script edit")
        } else if !self.scripts.contains_key(&candidate.asset)
            || !self
                .document
                .assets
                .get(&candidate.asset)
                .is_some_and(|a| a.kind == AssetKind::Script)
        {
            Some("script asset removed")
        } else if candidate.attachments.iter().any(|(owner, index)| {
            self.document
                .objects
                .iter()
                .find(|o| &o.id == owner)
                .and_then(|o| o.script_manager.as_ref())
                .and_then(|m| m.scripts.get(*index))
                .is_none_or(|a| a.script != candidate.asset)
        }) {
            Some("script attachment removed or changed")
        } else {
            None
        };
        if let Some(reason) = stale {
            return Ok(ScriptReloadStatus::Stale {
                asset: candidate.asset,
                reason: reason.into(),
            });
        }
        if let Some(runtime) = world.resource_mut::<ScriptRuntime>() {
            // Prefab and additive-scene attachments can appear while a worker compiles.
            // Every live consumer of the asset needs fresh script-local globals, including
            // attachments that were not present when this request was made.
            for object in &self.document.objects {
                if let Some(manager) = &object.script_manager {
                    for (index, attachment) in manager.scripts.iter().enumerate() {
                        if attachment.script == candidate.asset
                            && let Some(run) = runtime.runs.get_mut(&(object.id.clone(), index))
                        {
                            run.scope = Scope::new();
                            run.scope_initialized = false;
                        }
                    }
                }
            }
        }
        let asset = candidate.asset;
        let revision = candidate.revision;
        self.scripts.insert(asset.clone(), candidate.compiled);
        Ok(ScriptReloadStatus::Applied { asset, revision })
    }
    fn script_engine(&self) -> Arc<ScriptEngine> {
        self.script_engine
            .get_or_init(|| Arc::new(ScriptEngine::new()))
            .clone()
    }
    /// Whether the scene runs gameplay logic at all, from graphs, scripts, or both.
    pub fn has_gameplay_logic(&self) -> bool {
        self.has_blueprints() || self.has_scripts()
    }
    /// Whether any object carries a script, which makes the scene a gameplay scene.
    pub fn has_scripts(&self) -> bool {
        self.document.has_scripts()
    }
    pub fn set_script_enabled(&mut self, owner: &str, index: usize, enabled: bool) -> Result<()> {
        self.document
            .objects
            .iter_mut()
            .find(|object| object.id == owner)
            .context("unknown script owner")?
            .script_manager
            .as_mut()
            .context("owner has no Script Manager")?
            .scripts
            .get_mut(index)
            .context("script attachment index out of bounds")?
            .enabled = enabled;
        Ok(())
    }
    /// Runs every script attachment once, then applies what they asked for.
    pub fn step_scripts(&mut self, world: &mut World, dt: f32, input: GameplayInput) -> Result<()> {
        if !crate::game_flow::simulation_running(world) {
            return Ok(());
        }
        self.begin_compute_tick(world);
        if !self.has_scripts() {
            return Ok(());
        }
        ensure!(
            dt.is_finite()
                && dt > 0.
                && input
                    .movement
                    .iter()
                    .chain(&input.orbit)
                    .all(|v| v.is_finite()),
            "invalid script timestep/input"
        );
        // Variables live on the blueprint runtime, so scripts and graphs share one set of boards.
        if world.resource::<BlueprintRuntime>().is_none() {
            world.insert_resource(BlueprintRuntime::default());
        }
        {
            let runtime = world
                .resource_mut::<BlueprintRuntime>()
                .expect("blueprint runtime");
            runtime.initialize_boards(&self.document);
            for object in &self.document.objects {
                if object.script_manager.is_some() {
                    runtime.add_object_defaults(&object.id, &object.blackboard);
                }
            }
        }
        let mut runtime = world.remove_resource::<ScriptRuntime>().unwrap_or_default();
        let result = (|| -> Result<()> {
            runtime.elapsed += dt;
            ensure!(runtime.elapsed.is_finite(), "script clock overflow");
            self.run_scripts(world, &mut runtime, dt, input)
        })();
        runtime
            .tokens
            .retain(|_, id| self.entities.contains_key(id));
        world.insert_resource(runtime);
        result?;
        self.apply_scene_controls(world)
    }
    /// One tick of hook calls plus the command pass that follows them.
    fn run_scripts(
        &mut self,
        world: &mut World,
        runtime: &mut ScriptRuntime,
        dt: f32,
        input: GameplayInput,
    ) -> Result<()> {
        let engine = self.script_engine();
        // Presentation-only scenes can have scripts but no collision geometry.
        // Avoid rebuilding every object's global transform and an empty broad
        // phase on each redraw. Inspect the live world so spawned colliders
        // immediately take the ordinary path on the next tick.
        let has_colliders = world.resource::<crate::physics::Physics>().is_some()
            || self.entities.values().copied().any(|entity| {
                world.get::<BoxCollider>(entity).is_some()
                    || world.get::<MeshCollider>(entity).is_some()
                    || world
                        .get::<crate::middleware::sprite::Tilemap>(entity)
                        .is_some_and(|map| map.enabled && !map.solid.is_empty())
            });
        let snapshot = Arc::new(if has_colliders {
            self.collision_snapshot(world)?.0
        } else {
            CollisionSnapshot::default()
        });
        // Contacts are only needed by a script that listens for solid collisions.
        let contacts = if self
            .scripts
            .values()
            .any(|script| script.hooks.contains_key("on_collision_enter"))
        {
            let matrices = self.global_transforms(world)?;
            self.blueprint_contacts(world, &snapshot, &matrices)
        } else {
            BTreeMap::new()
        };
        let overlaps = self.script_overlaps(world, &snapshot)?;
        let owners: Vec<(String, Attachments)> = self
            .document
            .objects
            .iter()
            .filter_map(|object| {
                let manager = object.script_manager.as_ref()?;
                Some((
                    object.id.clone(),
                    manager
                        .scripts
                        .iter()
                        .map(|attachment| {
                            (
                                attachment.enabled,
                                self.scripts.get(&attachment.script).cloned(),
                            )
                        })
                        .collect(),
                ))
            })
            .collect();
        {
            let mut host = engine.lock();
            self.build_view(world, &mut host, runtime, &snapshot, dt, input);
        }
        runtime.stats.hooks = 0;
        runtime.stats.commands = 0;
        runtime.stats.attachments.clear();
        runtime.stats.truncated = false;
        for (owner, attachments) in owners {
            let overlap = overlaps.get(&owner).cloned().unwrap_or_default();
            let owner_contacts = contacts.get(&owner).cloned().unwrap_or_default();
            for (index, (enabled, compiled)) in attachments.into_iter().enumerate() {
                let compiled = compiled.with_context(|| {
                    let asset = self
                        .document_attachments(&owner)
                        .get(index)
                        .cloned()
                        .unwrap_or_default();
                    format!(
                        "script '{asset}' on '{owner}': no compiled source is bound; the scene was \
                         opened without loading its script catalog"
                    )
                })?;
                let key = (owner.clone(), index);
                engine.lock().attachment = index;
                let mut run = runtime.runs.remove(&key).unwrap_or_default();
                let hooks_before = runtime.stats.hooks;
                let commands_before = engine.lock().commands.len();
                let result = self.run_attachment(
                    &engine,
                    runtime,
                    &mut run,
                    &owner,
                    &compiled,
                    enabled,
                    &overlap,
                    &owner_contacts,
                    dt,
                );
                if runtime.stats.attachments.len() < MAX_ATTACHMENT_STATS {
                    runtime.stats.attachments.insert(
                        key.clone(),
                        ScriptAttachmentStats {
                            hooks: runtime.stats.hooks - hooks_before,
                            commands: engine.lock().commands.len() - commands_before,
                        },
                    );
                } else {
                    runtime.stats.truncated = true;
                }
                runtime.runs.insert(key, run);
                self.adopt_script_compute(&engine);
                if let Err(error) = &result {
                    bozzard_diagnostics::log(
                        world,
                        bozzard_diagnostics::Level::Error,
                        "Script",
                        &format!("{error:#}"),
                        bozzard_diagnostics::Location {
                            object: Some(owner.clone()),
                            attachment: Some(index),
                            node: None,
                            asset: self.document_attachments(&owner).get(index).cloned(),
                            ..Default::default()
                        },
                    );
                }
                result?;
            }
        }
        let commands = std::mem::take(&mut engine.lock().commands);
        runtime.stats.commands = commands.len();
        let mut tokens = std::mem::take(&mut runtime.tokens);
        let result = self.apply_commands(world, runtime, engine, commands, &mut tokens);
        runtime.tokens = tokens;
        result
    }
    /// Seeds the shared read view: every object's readable state and both blackboards.
    fn build_view(
        &self,
        world: &World,
        host: &mut Host,
        runtime: &ScriptRuntime,
        snapshot: &Arc<CollisionSnapshot>,
        dt: f32,
        input: GameplayInput,
    ) {
        host.network = world
            .resource::<NetworkFrame>()
            .cloned()
            .unwrap_or_default();
        host.dt = dt;
        self.prepare_script_compute(host);
        host.elapsed = runtime.elapsed;
        host.loading = self.scene_load_status(world);
        host.input = input;
        host.tokens = runtime.tokens.clone();
        host.geometry = snapshot.clone();
        host.budget = 1_000_000;
        host.commands.clear();
        host.objects.clear();
        host.object_boards.clear();
        host.scene_board.clear();
        let mut overlaps: BTreeMap<&str, usize> = BTreeMap::new();
        for (a, b) in &snapshot.overlaps {
            *overlaps.entry(a).or_default() += 1;
            *overlaps.entry(b).or_default() += 1;
        }
        for (id, entity) in &self.entities {
            let Some(transform) = world.get::<Transform>(*entity) else {
                continue;
            };
            host.objects.insert(
                id.clone(),
                ObjectView {
                    position: transform.translation,
                    rotation: transform.rotation_degrees,
                    scale: transform.scale,
                    forward: crate::physics::forward(transform).to_array(),
                    text: world
                        .get::<TextRendering>(*entity)
                        .map(|text| text.text.clone()),
                    rigidbody: world.get::<Gravity>(*entity).is_some_and(|g| g.enabled)
                        && world.get::<PlayerController>(*entity).is_none(),
                    grounded: runtime.moved.get(id).copied().unwrap_or_else(|| {
                        world
                            .get::<GravityState>(*entity)
                            .is_some_and(|state| state.grounded)
                    }),
                    overlaps: overlaps.get(id.as_str()).copied().unwrap_or(0),
                },
            );
        }
        if let Some(runtime) = world.resource::<BlueprintRuntime>() {
            for object in &self.document.objects {
                if let Some(board) = runtime.object_blackboard(&object.id) {
                    host.object_boards.insert(object.id.clone(), board.clone());
                }
            }
            host.scene_board = runtime.scene_blackboard().clone();
        }
    }
    /// Overlap sets for script owners, matching what a blueprint sees for the same object.
    ///
    /// Trigger volumes are not colliders, so each one is tested here; this repeats the inline
    /// overlap pass of the blueprint step, which is scheduled to move onto this helper.
    fn script_overlaps(
        &self,
        world: &World,
        snapshot: &CollisionSnapshot,
    ) -> Result<BTreeMap<String, BTreeSet<String>>> {
        let owners = || {
            self.document.objects.iter().filter(|object| {
                object
                    .script_manager
                    .as_ref()
                    .is_some_and(|manager| !manager.scripts.is_empty())
            })
        };
        let mut result: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for object in owners() {
            result.entry(object.id.clone()).or_default();
        }
        for (a, b) in &snapshot.overlaps {
            if let Some(overlap) = result.get_mut(a) {
                overlap.insert(b.clone());
            }
            if let Some(overlap) = result.get_mut(b) {
                overlap.insert(a.clone());
            }
        }
        // In particular, network presentation scenes have no trigger owners.
        // Their scripts still receive an empty overlap set without a second
        // full-scene hierarchy traversal.
        let trigger_owners: Vec<_> = owners()
            .filter_map(|object| {
                world
                    .get::<Trigger>(self.entities[&object.id])
                    .map(|trigger| trigger.volume)
                    .filter(|volume| volume.enabled)
                    .map(|volume| (object, volume))
            })
            .collect();
        if trigger_owners.is_empty() {
            return Ok(result);
        }
        let matrices = self.global_transforms(world)?;
        for (object, volume) in trigger_owners {
            let entity = self.entities[&object.id];
            let (center, edges, corners) = volume.geometry(matrices[&object.id])?;
            let volume = CollisionBox {
                id: object.id.clone(),
                entity,
                center,
                edges,
                corners,
                layers: volume.layers,
                mask: volume.mask,
            };
            let meets = |other_layers: u32, other_mask: u32| {
                layers_interact(volume.layers, volume.mask, other_layers, other_mask)
            };
            let overlap = result.get_mut(&object.id).expect("script owner");
            for body in &snapshot.boxes {
                if body.id != object.id && meets(body.layers, body.mask) && volume.intersects(body)
                {
                    overlap.insert(body.id.clone());
                }
            }
            for mesh in &snapshot.meshes {
                if mesh.id != object.id && meets(mesh.layers, mesh.mask) && mesh.intersects(&volume)
                {
                    overlap.insert(mesh.id.clone());
                }
            }
        }
        Ok(result)
    }
    /// Calls one attachment's hooks for this tick, in the order the module documents.
    #[allow(clippy::too_many_arguments)]
    fn run_attachment(
        &self,
        engine: &ScriptEngine,
        runtime: &mut ScriptRuntime,
        run: &mut ScriptRun,
        owner: &str,
        compiled: &CompiledScript,
        enabled: bool,
        overlap: &BTreeSet<String>,
        contacts: &[Contact],
        dt: f32,
    ) -> Result<()> {
        let me = |override_owner: Option<&str>| {
            vec![Dynamic::from(override_owner.unwrap_or(owner).to_owned())]
        };
        let mut events: Vec<(&str, Vec<Dynamic>)> = Vec::new();
        if enabled && !run.enabled {
            events.push(("on_enable", me(None)));
        }
        if enabled && !run.started {
            events.push(("on_start", me(None)));
        }
        if enabled {
            let mut update = me(None);
            update.push(Dynamic::from(dt));
            events.push(("on_update", update));
            for other in overlap.difference(&run.overlap) {
                events.push((
                    "on_object_enter",
                    vec![
                        Dynamic::from(owner.to_owned()),
                        Dynamic::from(other.clone()),
                    ],
                ));
            }
            for other in run.overlap.difference(overlap) {
                events.push((
                    "on_object_exit",
                    vec![
                        Dynamic::from(owner.to_owned()),
                        Dynamic::from(other.clone()),
                    ],
                ));
            }
            if !overlap.is_empty() && run.overlap.is_empty() {
                events.push(("on_overlap_enter", me(None)));
            }
            if overlap.is_empty() && !run.overlap.is_empty() {
                events.push(("on_overlap_exit", me(None)));
            }
            for contact in contacts
                .iter()
                .filter(|contact| !run.collisions.contains(&contact.other))
            {
                events.push((
                    "on_collision_enter",
                    vec![
                        Dynamic::from(owner.to_owned()),
                        Dynamic::from(contact.other.clone()),
                        Dynamic::from_array(array_of(contact.normal.to_array())),
                        Dynamic::from(contact.impulse),
                    ],
                ));
            }
        } else if run.enabled {
            events.push(("on_disable", me(None)));
        }
        {
            let mut host = engine.lock();
            host.owner = owner.to_owned();
            host.held = run.held;
            host.random = owner
                .bytes()
                .fold(1, |n, byte| n.wrapping_mul(1099511628211) ^ u64::from(byte));
        }
        if enabled && !run.scope_initialized {
            // Rhai's call_fn evaluates top-level statements then rewinds the scope on every
            // invocation. Evaluate once explicitly so globals survive ticks and a reload gives
            // each attachment fresh script-local state.
            let _ = engine
                .engine
                .eval_ast_with_scope::<Dynamic>(&mut run.scope, &compiled.ast)
                .map_err(|error| anyhow::anyhow!("script initialization on '{owner}': {error}"))?;
            run.scope_initialized = true;
        }
        for (hook, args) in events {
            if !compiled.takes(hook, args.len()) {
                continue;
            }
            runtime.stats.hooks += 1;
            // A hook's return value is ignored: scripts write through engine actions.
            let _ = engine
                .engine
                .call_fn_with_options::<Dynamic>(
                    CallFnOptions::new().eval_ast(false),
                    &mut run.scope,
                    &compiled.ast,
                    hook,
                    args,
                )
                .map_err(|error| anyhow::anyhow!("script hook {hook} on '{owner}': {error}"))?;
        }
        if enabled {
            run.started = true;
            run.held = engine.lock().input.binding_mask();
        } else {
            run.held = 0;
        }
        run.overlap = overlap.clone();
        run.collisions = contacts
            .iter()
            .map(|contact| contact.other.clone())
            .collect();
        run.enabled = enabled;
        let attachment = engine.lock().attachment;
        if !enabled && let Some(mut compute) = self.compute_if_initialized() {
            compute.cancel_owner(&crate::compute::Owner::new(owner, attachment), false)?;
        }
        Ok(())
    }
    /// Applies queued script commands in order, then the destroys they asked for.
    fn apply_commands(
        &mut self,
        world: &mut World,
        runtime: &mut ScriptRuntime,
        engine: Arc<ScriptEngine>,
        commands: Vec<Command>,
        tokens: &mut BTreeMap<String, String>,
    ) -> Result<()> {
        let mut destroy = Vec::new();
        for command in commands {
            match command {
                Command::SetVelocity { target, velocity } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Velocity target does not exist")?;
                    self.set_velocity(world, &target, entity, Vec3::from(velocity))?;
                }
                Command::MoveWithCollision { target, delta } => {
                    let target = resolve(tokens, &target);
                    let movement = self.move_box(world, &target, Vec3::from(delta))?;
                    runtime.moved.insert(
                        target,
                        movement
                            .contact_normals
                            .iter()
                            .any(|normal| normal.y >= 0.5),
                    );
                }
                Command::Jump { target, speed } => {
                    let target = resolve(tokens, &target);
                    self.jump_box(world, &target, speed)?;
                }
                Command::Transform {
                    target,
                    kind,
                    value,
                } => self.apply_transform(world, &resolve(tokens, &target), kind, value)?,
                Command::Color { target, color } => {
                    self.apply_color(world, &resolve(tokens, &target), color)?
                }
                Command::Text { target, text } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Text target does not exist")?;
                    let previous = world
                        .get::<TextRendering>(entity)
                        .context("Set Text needs Text Rendering")?;
                    if previous.text != text {
                        world
                            .get_mut::<TextRendering>(entity)
                            .expect("validated Text Rendering")
                            .text = text;
                    }
                }
                Command::Visible { target, visible } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Visible target does not exist")?;
                    world.insert(entity, BlueprintHidden(!visible))?;
                }
                Command::LightIntensity { target, intensity } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Light Intensity target does not exist")?;
                    let mut light = *world
                        .get::<Light>(entity)
                        .context("Set Light Intensity needs a Light")?;
                    light.intensity = intensity;
                    light.validate()?;
                    world.insert(entity, light)?;
                }
                Command::Display { kind, value } => self.set_display_parameter(kind, value)?,
                Command::Spawn {
                    owner,
                    token,
                    asset,
                    position,
                } => {
                    let id = self.spawn_prefab_for(world, &owner, &asset, position)?;
                    tokens.insert(token, id);
                }
                Command::Destroy { target } => destroy.push(resolve(tokens, &target)),
                Command::GraphEnabled {
                    target,
                    index,
                    enabled,
                } => self.set_blueprint_enabled(&resolve(tokens, &target), index, enabled)?,
                Command::ScriptEnabled {
                    target,
                    index,
                    enabled,
                } => self.set_script_enabled(&resolve(tokens, &target), index, enabled)?,
                Command::Cursor(requested) => {
                    world.insert_resource(CursorCapture {
                        requested: Some(requested),
                    });
                }
                Command::EndGame(message) => {
                    world
                        .resource_mut::<crate::GameSession>()
                        .context("End Game needs Game Flow enabled in scene settings")?
                        .end_game(&message)?;
                }
                Command::SceneControl { kind, name } => {
                    self.request_scene_control(world, kind, &name)?
                }
                Command::Variable {
                    scope,
                    owner,
                    name,
                    value,
                } => world
                    .resource_mut::<BlueprintRuntime>()
                    .context("script variables need the blueprint runtime")?
                    .set_board_scalar(scope, &owner, &name, value)?,
                Command::Print { level, owner, text } => {
                    bozzard_diagnostics::log(
                        world,
                        level,
                        "Script",
                        &text,
                        bozzard_diagnostics::Location {
                            object: Some(owner),
                            ..Default::default()
                        },
                    );
                    runtime.messages.push_back(text.clone());
                    while runtime.messages.len() > 64 {
                        runtime.messages.pop_front();
                    }
                    println!("{text}");
                }
            }
        }
        for target in destroy {
            self.destroy_script_prefab(world, runtime, engine.clone(), &target)?;
        }
        Ok(())
    }
    /// `on_destroy` for a destroyed prefab's scripts, then the destroy itself.
    fn destroy_script_prefab(
        &mut self,
        world: &mut World,
        runtime: &mut ScriptRuntime,
        engine: Arc<ScriptEngine>,
        target: &str,
    ) -> Result<()> {
        if !self.entities.contains_key(target) {
            return Ok(());
        }
        let members: Vec<String> = self
            .document
            .prefabs
            .values()
            .find(|prefab| prefab.members.values().any(|id| id == target))
            .context("Destroy Prefab target is not a live prefab instance")?
            .members
            .values()
            .cloned()
            .collect();
        for owner in &members {
            self.run_destroy_hooks(&engine, runtime, owner)?;
        }
        let mut blueprint_runtime = world
            .remove_resource::<BlueprintRuntime>()
            .unwrap_or_default();
        let mut budget = 100_000;
        let result = (|| -> Result<()> {
            for owner in &members {
                self.destroy_blueprint_events(
                    world,
                    &mut blueprint_runtime,
                    owner,
                    GameplayInput::default(),
                    0.,
                    &mut budget,
                )?;
            }
            self.destroy_prefab_raw(world, target)?;
            let ids = members.into_iter().collect();
            blueprint_runtime.remove_objects(&ids);
            runtime.remove_objects(&ids);
            Ok(())
        })();
        world.insert_resource(blueprint_runtime);
        result
    }
    /// `on_destroy` for every script attachment of one object.
    fn run_destroy_hooks(
        &self,
        engine: &ScriptEngine,
        runtime: &mut ScriptRuntime,
        owner: &str,
    ) -> Result<()> {
        let Some(manager) = self
            .document
            .objects
            .iter()
            .find(|object| object.id == owner)
            .and_then(|object| object.script_manager.as_ref())
        else {
            return Ok(());
        };
        for (index, attachment) in manager.scripts.iter().enumerate() {
            {
                let mut host = engine.lock();
                host.owner = owner.to_owned();
                host.attachment = index;
                self.prepare_script_compute(&mut host);
            }
            let Some(compiled) = self.scripts.get(&attachment.script).cloned() else {
                continue;
            };
            let args = vec![Dynamic::from(owner.to_owned())];
            if !compiled.takes("on_destroy", args.len()) {
                continue;
            }
            let key = (owner.to_owned(), index);
            let mut run = runtime.runs.remove(&key).unwrap_or_default();
            let result = engine
                .engine
                .call_fn::<Dynamic>(&mut run.scope, &compiled.ast, "on_destroy", args)
                .map_err(|error| anyhow::anyhow!("script hook on_destroy on '{owner}': {error}"));
            runtime.runs.insert(key, run);
            self.adopt_script_compute(engine);
            result.map(|_| ())?;
        }
        if let Some(mut compute) = self.compute_if_initialized() {
            for index in 0..manager.scripts.len() {
                compute.cancel_owner(&crate::compute::Owner::new(owner, index), true)?;
            }
            compute.materials.remove(owner);
        }
        Ok(())
    }
    // Seed an allocation-free context for ordinary scenes. Only an actual compute API call
    // creates runtime state; this also handles indirect Rhai calls without scanning source text.
    fn prepare_script_compute(&self, host: &mut Host) {
        host.compute_ready = true;
        host.compute = self.compute_state.get().cloned();
        if host.compute.is_none() {
            host.compute_capabilities = self.compute_capabilities.clone();
            if host.compute_kernels.len() != self.compute_kernels.len()
                || self.compute_kernels.iter().any(|(id, kernel)| {
                    host.compute_kernels
                        .get(id)
                        .is_none_or(|k| k.id() != kernel.id())
                })
            {
                host.compute_kernels.clone_from(&self.compute_kernels);
            }
        }
    }
    fn adopt_script_compute(&self, engine: &ScriptEngine) {
        if self.compute_state.get().is_none()
            && let Some(state) = &engine.lock().compute
        {
            let _ = self.compute_state.set(state.clone());
        }
    }
    /// One transform write, shared by every script transform function.
    fn apply_transform(
        &self,
        world: &mut World,
        target: &str,
        kind: blueprint::NodeKind,
        value: [f32; 3],
    ) -> Result<()> {
        use blueprint::NodeKind as K;
        let entity = *self
            .entities
            .get(target)
            .context("transform target does not exist")?;
        let previous = *world
            .get::<Transform>(entity)
            .context("transform target was removed")?;
        let mut next = previous;
        match kind {
            K::Translate => {
                next.translation = (Vec3::from(next.translation) + Vec3::from(value)).to_array()
            }
            K::Rotate => {
                next.rotation_degrees = (Vec3::from(next.rotation_degrees) + Vec3::from(value))
                    .to_array()
                    .map(|r| r.rem_euclid(360.))
            }
            K::SetPosition => next.translation = value,
            K::SetRotation => next.rotation_degrees = value,
            K::SetScale => next.scale = value,
            _ => anyhow::bail!("not a transform action"),
        }
        next.validate()?;
        if next == previous {
            return Ok(());
        }
        world.insert(entity, next)?;
        if let Err(error) = self.validate_transform_change(world, target) {
            world.insert(entity, previous)?;
            return Err(error);
        }
        Ok(())
    }
    /// One colour write, matching the blueprint `Set Color` node.
    fn apply_color(&self, world: &mut World, target: &str, color: [f32; 3]) -> Result<()> {
        let entity = *self
            .entities
            .get(target)
            .context("Set Color target does not exist")?;
        let has_text = if let Some(mut text) = world.get_mut::<TextRendering>(entity) {
            text.color[..3].copy_from_slice(&color);
            true
        } else {
            false
        };
        if let Some(mut material) = world.get_mut::<Material>(entity) {
            material.set_color(color);
        } else if let Some(mut drawable) = world.get_mut::<Drawable>(entity) {
            // Legacy objects also colour the mesh using its source material.
            drawable.color = color;
        } else {
            ensure!(
                has_text,
                "Set Color needs a mesh, Material or Text Rendering"
            );
        }
        Ok(())
    }
    /// `on_destroy` for every script of a scene being torn down, before its entities go.
    pub(crate) fn scene_script_destroy_events(&mut self, world: &mut World) -> Result<()> {
        let owners: Vec<String> = self
            .document
            .objects
            .iter()
            .filter(|object| object.script_manager.is_some())
            .map(|object| object.id.clone())
            .collect();
        self.object_script_destroy_events(world, &owners)
    }
    pub(crate) fn object_script_destroy_events(
        &mut self,
        world: &mut World,
        owners: &[String],
    ) -> Result<()> {
        if !self.has_scripts() {
            return Ok(());
        }
        let engine = self.script_engine();
        let snapshot = Arc::new(self.collision_snapshot(world)?.0);
        let mut runtime = world.remove_resource::<ScriptRuntime>().unwrap_or_default();
        self.build_view(
            world,
            &mut engine.lock(),
            &runtime,
            &snapshot,
            0.,
            GameplayInput::default(),
        );
        let result = (|| -> Result<()> {
            for owner in owners {
                self.run_destroy_hooks(&engine, &mut runtime, owner)?;
            }
            Ok(())
        })();
        let commands = std::mem::take(&mut engine.lock().commands);
        let mut tokens = std::mem::take(&mut runtime.tokens);
        let applied = self.apply_commands(world, &mut runtime, engine, commands, &mut tokens);
        runtime.tokens = tokens;
        world.insert_resource(runtime);
        result.and(applied)
    }
}

/// Follow a spawn handle to the object it created.
fn resolve(tokens: &BTreeMap<String, String>, target: &str) -> String {
    if target.starts_with(SPAWN_PREFIX) {
        tokens
            .get(target)
            .cloned()
            .unwrap_or_else(|| target.to_owned())
    } else {
        target.to_owned()
    }
}

/// Reads every `script` asset of a scene catalog next to the scene file.
///
/// Mirrors the prefab loader: fixed ticks never touch the filesystem, so sources are read once when
/// the scene is opened and handed to [`SceneInstance::register_script`].
pub fn load_sources(
    document: &Scene,
    path: Option<&std::path::Path>,
) -> Result<BTreeMap<String, String>> {
    load_sources_with_progress(document, path, &bozzard_app::job::Progress::default())
}
pub fn load_sources_with_progress(
    document: &Scene,
    path: Option<&std::path::Path>,
    progress: &bozzard_app::job::Progress,
) -> Result<BTreeMap<String, String>> {
    use std::io::Read;
    // Every `script` catalog entry is read, not only the ones an object names: a prefab member may
    // carry a script, and the loader merges that prefab's catalog into the scene before calling
    // this. Reading the whole catalog is cheap and leaves no source unbound.
    let root = path
        .and_then(std::path::Path::parent)
        .unwrap_or(std::path::Path::new("."));
    let mut sources = BTreeMap::new();
    let mut bytes = 0;
    for (id, source) in document
        .assets
        .iter()
        .filter(|(_, source)| source.kind == AssetKind::Script)
    {
        progress.stage(format!("Reading script {id}"))?;
        ensure!(
            sources.len() < MAX_SCRIPT_ASSETS,
            "scene catalog holds at most {MAX_SCRIPT_ASSETS} scripts"
        );
        let mut text = String::new();
        std::fs::File::open(root.join(&source.path))
            .with_context(|| format!("loading script '{id}'"))?
            .take(MAX_SCRIPT_BYTES as u64 + 1)
            .read_to_string(&mut text)
            .with_context(|| format!("reading script '{id}'"))?;
        ensure!(
            text.len() <= MAX_SCRIPT_BYTES,
            "script '{id}' exceeds 1 MiB"
        );
        progress.check()?;
        bytes += text.len();
        ensure!(bytes <= 32 * 1024 * 1024, "scripts exceed 32 MiB");
        sources.insert(id.clone(), text);
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collisionless_script_queries_observe_a_collider_added_to_the_live_world() {
        let scene = Scene::from_json(
            r#"{"version":1,"name":"spatial script","views":{},
                "assets":{"look":{"kind":"script","path":"look.rs"}},
                "objects":[
                  {"id":"observer","name":"Observer","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                   "script_manager":{"scripts":[{"enabled":true,"script":"look"}]}},
                  {"id":"target","name":"Target","transform":{"translation":[3,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#,
        )
        .unwrap();
        let mut world = World::new();
        let mut instance = scene.spawn(&mut world).unwrap();
        instance
            .register_script(
                "look".into(),
                r#"fn on_update(me, dt) {
                    let hit = raycast([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 10.0, me);
                    set_position(me, if hit.hit { [1.0, 0.0, 0.0] } else { [0.0, 0.0, 0.0] });
                }"#
                .into(),
            )
            .unwrap();

        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        let observer = instance.entity("observer").unwrap();
        assert_eq!(world.get::<Transform>(observer).unwrap().translation[0], 0.);

        let target = instance.entity("target").unwrap();
        world.insert(target, BoxCollider::default()).unwrap();
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert_eq!(world.get::<Transform>(observer).unwrap().translation[0], 1.);
    }

    #[test]
    fn unchanged_script_transform_preserves_the_ecs_change_tick() {
        let (mut instance, mut world) =
            demo("fn on_update(me, dt) { set_position(me, [0.0, 0.0, 0.0]); }");
        let entity = instance.entity("thing").unwrap();
        let before = world.changed_tick::<Transform>(entity).unwrap();
        world.advance_change_tick();
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert_eq!(world.changed_tick::<Transform>(entity), Some(before));

        instance
            .register_script(
                "drift".into(),
                "fn on_update(me, dt) { set_position(me, [1.0, 0.0, 0.0]); }".into(),
            )
            .unwrap();
        let tick = world.advance_change_tick();
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert_eq!(world.get::<Transform>(entity).unwrap().translation[0], 1.);
        assert_eq!(world.changed_tick::<Transform>(entity), Some(tick));
    }

    /// A scene with one drawable object that runs one script.
    fn demo(source: &str) -> (SceneInstance, World) {
        let scene = Scene::from_json(
            r#"{"version":1,"name":"scripts","views":{},
                "assets":{"drift":{"kind":"script","path":"drift.rs"}},
                "objects":[
                  {"id":"thing","name":"thing","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                    "drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]},
                    "collider":{"size":[1,1,1]},
                    "gravity":{"enabled":true},
                    "script_manager":{"scripts":[{"enabled":true,"script":"drift"}]}}]}"#,
        )
        .unwrap();
        let mut world = World::default();
        world.insert_resource(crate::GameSession {
            phase: crate::GamePhase::Playing,
            message: String::new(),
        });
        let mut instance = scene.spawn(&mut world).unwrap();
        instance
            .register_script("drift".into(), source.into())
            .unwrap();
        (instance, world)
    }

    #[test]
    fn a_script_moves_its_object_through_the_same_actions_blueprints_use() {
        let (mut instance, mut world) = demo(
            r#"
            fn on_start(me) { print("starting"); }
            fn on_update(me, dt) {
                set_position(me, [1.0, 2.0, 3.0]);
                // A queued write is visible to later reads in the same tick.
                if get_position(me)[1] < 2.0 { set_position(me, [0.0, 0.0, 0.0]); }
                set_velocity(me, [0.0, 0.0, 0.0]);
            }
            "#,
        );
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        let entity = instance.entity("thing").unwrap();
        assert_eq!(
            world.get::<Transform>(entity).unwrap().translation,
            [1., 2., 3.]
        );
        let runtime = world.resource::<ScriptRuntime>().unwrap();
        assert_eq!(runtime.messages().collect::<Vec<_>>(), ["starting"]);
        assert_eq!(
            runtime.stats.hooks, 2,
            "on_start and on_update ran once each"
        );
        assert_eq!(runtime.stats.commands, 3);
        assert_eq!(
            runtime.stats.attachments.get(&("thing".into(), 0)),
            Some(&ScriptAttachmentStats {
                hooks: 2,
                commands: 3
            })
        );
    }

    fn finish_reload(request: ScriptReloadRequest) -> Result<ScriptReloadCandidate> {
        let job = request.start()?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = job.poll() {
                return result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "script compile timed out"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn live_reload_is_atomic_and_keeps_world_state_without_restarting_hooks() {
        let (mut instance, mut world) = demo(
            "fn on_start(me) { rotate(me, [0.0, 90.0, 0.0]); }\nfn on_update(me, dt) { rotate(me, [0.0, 1.0, 0.0]); }",
        );
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        let entity = instance.entity("thing").unwrap();
        let rotation = world.get::<Transform>(entity).unwrap().rotation_degrees;
        assert_eq!(rotation, [0., 91., 0.]);
        let bad = instance
            .request_script_reload("drift", "fn on_update(me) {}".into())
            .unwrap();
        assert!(
            finish_reload(bad)
                .err()
                .unwrap()
                .to_string()
                .contains("drift")
        );
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert_eq!(
            world.get::<Transform>(entity).unwrap().rotation_degrees,
            [0., 92., 0.]
        );
        let good = instance.request_script_reload("drift",
            "fn on_start(me) { rotate(me, [0.0, 100.0, 0.0]); }\nfn on_update(me, dt) { rotate(me, [0.0, 2.0, 0.0]); }".into()).unwrap();
        let candidate = finish_reload(good).unwrap();
        assert_eq!(
            instance
                .publish_script_reload(&mut world, candidate)
                .unwrap(),
            ScriptReloadStatus::Applied {
                asset: "drift".into(),
                revision: 3
            }
        );
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert_eq!(
            world.get::<Transform>(entity).unwrap().rotation_degrees,
            [0., 94., 0.]
        );
        assert_eq!(
            world
                .resource::<ScriptRuntime>()
                .unwrap()
                .stats
                .attachments
                .get(&("thing".into(), 0))
                .unwrap()
                .hooks,
            1
        );
    }

    #[test]
    fn stale_reload_cannot_replace_a_newer_edit_or_restarted_scene() {
        let (mut instance, mut world) = demo("fn on_update(me, dt) {}");
        let older = finish_reload(
            instance
                .request_script_reload("drift", "fn on_update(me, dt) {}".into())
                .unwrap(),
        )
        .unwrap();
        let _newer = instance
            .request_script_reload("drift", "fn on_update(me, dt) {}".into())
            .unwrap();
        assert!(matches!(
            instance.publish_script_reload(&mut world, older).unwrap(),
            ScriptReloadStatus::Stale { .. }
        ));
        let before_restart = finish_reload(
            instance
                .request_script_reload("drift", "fn on_update(me, dt) {}".into())
                .unwrap(),
        )
        .unwrap();
        instance.restart_runtime_scene(&mut world).unwrap();
        assert!(matches!(
            instance
                .publish_script_reload(&mut world, before_restart)
                .unwrap(),
            ScriptReloadStatus::Stale { .. }
        ));
        let removed = finish_reload(
            instance
                .request_script_reload("drift", "fn on_update(me, dt) {}".into())
                .unwrap(),
        )
        .unwrap();
        instance.document.objects[0]
            .script_manager
            .as_mut()
            .unwrap()
            .scripts
            .clear();
        assert!(matches!(
            instance.publish_script_reload(&mut world, removed).unwrap(),
            ScriptReloadStatus::Stale { .. }
        ));
    }

    #[test]
    fn script_scope_is_reinitialized_after_replacement() {
        let (mut instance, mut world) =
            demo("let speed = 1.0; fn on_update(me, dt) { rotate(me, [0.0, speed, 0.0]); }");
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        let entity = instance.entity("thing").unwrap();
        assert_eq!(
            world.get::<Transform>(entity).unwrap().rotation_degrees[1],
            1.
        );
        let candidate = finish_reload(
            instance
                .request_script_reload(
                    "drift",
                    "let speed = 2.0; fn on_update(me, dt) { rotate(me, [0.0, speed, 0.0]); }"
                        .into(),
                )
                .unwrap(),
        )
        .unwrap();
        instance
            .publish_script_reload(&mut world, candidate)
            .unwrap();
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert_eq!(
            world.get::<Transform>(entity).unwrap().rotation_degrees[1],
            3.
        );
    }

    #[test]
    fn prefab_spawned_during_reload_gets_the_new_script_scope() {
        let old = "let speed = 1.0; fn on_update(me, dt) { rotate(me, [0.0, speed, 0.0]); }";
        let new = "let speed = 2.0; fn on_update(me, dt) { rotate(me, [0.0, speed, 0.0]); }";
        let (mut instance, mut world) = demo(old);
        instance.document.assets.insert(
            "copy".into(),
            AssetSource {
                kind: AssetKind::Prefab,
                path: "copy.prefab.json".into(),
            },
        );
        let mut child = instance.document.objects[0].clone();
        child.id = "child".into();
        instance
            .register_prefab(
                "copy".into(),
                Prefab {
                    nested: Default::default(),
                    base: None,
                    version: 1,
                    name: "Scripted copy".into(),
                    root: "child".into(),
                    objects: vec![child],
                    assets: BTreeMap::from([(
                        "drift".into(),
                        instance.document.assets["drift"].clone(),
                    )]),
                },
            )
            .unwrap();
        let request = instance.request_script_reload("drift", new.into()).unwrap();
        let spawned = instance
            .spawn_prefab(&mut world, "copy", [0., 0., 0.])
            .unwrap();
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        let child = instance.entity(&spawned).unwrap();
        assert_eq!(
            world.get::<Transform>(child).unwrap().rotation_degrees[1],
            1.
        );

        instance
            .publish_script_reload(&mut world, finish_reload(request).unwrap())
            .unwrap();
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert_eq!(
            world.get::<Transform>(child).unwrap().rotation_degrees[1],
            3.,
            "the spawned attachment must initialize the new top-level speed"
        );
    }

    /// Restarting or loading a scene respawns the world. Script sources are runtime state the
    /// document cannot carry, so a replacement that dropped them left every attachment unbound.
    #[test]
    fn a_replaced_scene_keeps_its_scripts_and_starts_their_state_over() {
        let (mut instance, mut world) = demo(
            r#"
            fn on_start(me) { rotate(me, [0.0, 90.0, 0.0]); }
            fn on_update(me, dt) { rotate(me, [0.0, 1.0, 0.0]); }
            "#,
        );
        let rotation = |instance: &SceneInstance, world: &World| {
            world
                .get::<Transform>(instance.entity("thing").unwrap())
                .unwrap()
                .rotation_degrees
        };
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert_eq!(rotation(&instance, &world), [0., 91., 0.]);

        instance.restart_runtime_scene(&mut world).unwrap();
        instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap();
        assert_eq!(
            rotation(&instance, &world),
            [0., 91., 0.],
            "the restarted scene must run its script again from the new world's state"
        );
    }

    #[test]
    fn a_wrong_hook_signature_fails_at_load_and_a_throwing_script_stops_the_tick() {
        let scene = Scene::from_json(
            r#"{"version":1,"name":"scripts","views":{},
                "assets":{"drift":{"kind":"script","path":"drift.rs"}},
                "objects":[{"id":"thing","name":"thing","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                    "script_manager":{"scripts":[{"enabled":true,"script":"drift"}]}}]}"#,
        )
        .unwrap();
        let mut world = World::default();
        let mut instance = scene.spawn(&mut world).unwrap();
        assert!(instance.has_scripts());
        let error = instance
            .register_script("drift".into(), "fn on_update(me) {}".into())
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("on_update takes 2"),
            "{error:#}"
        );

        let (mut instance, mut world) = demo("fn on_update(me, dt) { jump(me, 0.0); }");
        let error = instance
            .step_scripts(&mut world, 1. / 60., GameplayInput::default())
            .unwrap_err();
        assert!(format!("{error:#}").contains("jump speed"), "{error:#}");
    }

    /// A scene whose attachment names an asset the catalog does not hold as a script cannot run:
    /// the loader says so when the scene opens instead of the simulation stopping mid-run.
    #[test]
    fn registering_sources_reports_an_attachment_with_no_source() {
        let json = r#"{"version":1,"name":"scripts","views":{},
            "assets":{"drift":{"kind":"script","path":"drift.rs"}},
            "objects":[{"id":"thing","name":"thing","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                "script_manager":{"scripts":[{"enabled":true,"script":"drift"}]}}]}"#;
        let mut world = World::default();
        let mut instance = Scene::from_json(json).unwrap().spawn(&mut world).unwrap();
        // Nothing registered: the attachment has no source.
        let error = format!(
            "{:#}",
            instance.register_scripts(BTreeMap::new()).unwrap_err()
        );
        assert!(
            error.contains("script 'drift' on 'thing' (attachment 0)")
                && error.contains("was not loaded"),
            "{error}"
        );
        // With its source it compiles, and the check passes.
        instance
            .register_scripts(BTreeMap::from([(
                "drift".into(),
                "fn on_update(me, dt) {}".into(),
            )]))
            .unwrap();
    }

    /// A script names the prefab it spawns in source, which the loader cannot read, so the scene
    /// catalog is the declaration and a scripted scene preloads its prefabs.
    #[test]
    fn a_scene_with_scripts_preloads_the_prefabs_its_scripts_can_spawn() {
        let scene = |object: &str| {
            format!(
                r#"{{"version":1,"name":"spawner","views":{{}},
                    "assets":{{"shot":{{"kind":"prefab","path":"assets/shot.prefab.json"}}}},
                    "objects":[{object}]}}"#
            )
        };
        let transform =
            r#""transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}"#;
        // A graph names its prefab on the node, so exactly that prefab is loaded.
        let node = Scene::from_json(&scene(&format!(
            r#"{{"id":"gun","name":"gun",{transform},
                "blueprints":[{{"enabled":true,"graph":{{"version":1,"name":"fire",
                    "nodes":[{{"id":1,"position":[0,0],"kind":"spawn_prefab","prefab":"shot",
                        "inputs":["exec",{{"vector":[0,0,0]}}]}}],"wires":[]}}}}]}}"#
        )))
        .unwrap();
        assert_eq!(node.spawn_asset_ids(), BTreeSet::from(["shot".into()]));

        // A script cannot, so every prefab in the catalog stays ready to be spawned by name.
        let script = Scene::from_json(&format!(
            r#"{{"version":1,"name":"spawner","views":{{}},
                "assets":{{"shot":{{"kind":"prefab","path":"assets/shot.prefab.json"}},
                    "fire":{{"kind":"script","path":"fire.rs"}}}},
                "objects":[{{"id":"gun","name":"gun",{transform},
                    "script_manager":{{"scripts":[{{"enabled":true,"script":"fire"}}]}}}}]}}"#
        ))
        .unwrap();
        assert!(script.has_scripts());
        assert_eq!(script.spawn_asset_ids(), BTreeSet::from(["shot".into()]));

        // Without either authoring path there is nothing to preload.
        let empty = Scene::from_json(&scene(&format!(
            r#"{{"id":"gun","name":"gun",{transform}}}"#
        )))
        .unwrap();
        assert!(empty.spawn_asset_ids().is_empty());
    }
}
