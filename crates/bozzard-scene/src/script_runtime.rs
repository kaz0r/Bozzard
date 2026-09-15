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
use rhai::{AST, Array, Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position, Scope};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

/// Largest accepted script source, matching the blueprint document limit.
const MAX_SCRIPT_BYTES: usize = 1024 * 1024;
/// Scripts a scene may compile, across every object.
const MAX_SCRIPT_ASSETS: usize = 1024;
/// Instructions one hook may run, so a runaway loop fails the tick instead of hanging the game.
const MAX_SCRIPT_OPERATIONS: u64 = 2_000_000;
/// Deepest spatial query result a script may receive.
const MAX_SCRIPT_OVERLAP: usize = 1024;
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
];

/// One object's script attachments, as the tick needs them: enabled flag and compiled source.
type Attachments = Vec<(bool, Option<Arc<CompiledScript>>)>;

/// One compiled script asset, shared by every attachment that references it.
#[derive(Clone)]
pub(crate) struct CompiledScript {
    ast: AST,
    hooks: BTreeMap<String, usize>,
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
    Print(String),
}

/// The read view and the write queue of the scripts running this tick.
#[derive(Default)]
struct Host {
    /// The attachment currently running, which bare `me` arguments resolve to.
    owner: String,
    dt: f32,
    elapsed: f32,
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
}

#[derive(Clone, Default)]
pub struct ScriptRuntimeStats {
    /// Hook calls made during the last tick.
    pub hooks: usize,
    /// Commands the last tick queued.
    pub commands: usize,
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
    read!("delta_time", (), |state| Ok(Dynamic::from(state.dt)));
    read!("elapsed_time", (), |state| Ok(Dynamic::from(state.elapsed)));
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
            let active = key.active(state.input);
            Ok(Dynamic::from(if pressed {
                active && (key.instant() || state.held & key.bit() == 0)
            } else {
                active
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
                state.record(Command::Spawn {
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
            borrow!(host).record(Command::Print(text.to_owned()));
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
            for (index, attachment) in self
                .document_attachments(&object.id)
                .into_iter()
                .enumerate()
            {
                ensure!(
                    self.scripts.contains_key(&attachment),
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
            source.len() <= MAX_SCRIPT_BYTES,
            "script '{asset}' exceeds 1 MiB"
        );
        ensure!(
            self.scripts.len() < MAX_SCRIPT_ASSETS,
            "scene compiles at most {MAX_SCRIPT_ASSETS} scripts"
        );
        let engine = self.script_engine();
        let ast = engine
            .engine
            .compile(&source)
            .map_err(|error| anyhow::anyhow!("script '{asset}': {error}"))?;
        let mut hooks = BTreeMap::new();
        for function in ast.iter_functions() {
            if let Some((name, args)) = HOOKS.iter().find(|(name, _)| *name == function.name) {
                ensure!(
                    function.params.len() == *args,
                    "script '{asset}': {name} takes {args} argument(s), got {}",
                    function.params.len()
                );
                hooks.insert(function.name.to_owned(), function.params.len());
            }
        }
        self.scripts
            .insert(asset, Arc::new(CompiledScript { ast, hooks }));
        Ok(())
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
        let snapshot = Arc::new(self.collision_snapshot(world)?.0);
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
                let mut run = runtime.runs.remove(&key).unwrap_or_default();
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
                runtime.runs.insert(key, run);
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
        host.dt = dt;
        host.elapsed = runtime.elapsed;
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
        let matrices = self.global_transforms(world)?;
        for object in owners() {
            let entity = self.entities[&object.id];
            let Some(volume) = world
                .get::<Trigger>(entity)
                .map(|trigger| trigger.volume)
                .filter(|volume| volume.enabled)
            else {
                continue;
            };
            let (center, edges, corners) = volume.geometry(matrices[&object.id])?;
            let volume = CollisionBox {
                id: object.id.clone(),
                entity,
                center,
                edges,
                corners,
            };
            let overlap = result.get_mut(&object.id).expect("script owner");
            for body in &snapshot.boxes {
                if body.id != object.id && volume.intersects(body) {
                    overlap.insert(body.id.clone());
                }
            }
            for mesh in &snapshot.meshes {
                if mesh.id != object.id && mesh.intersects(&volume) {
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
        for (hook, args) in events {
            if !compiled.takes(hook, args.len()) {
                continue;
            }
            runtime.stats.hooks += 1;
            // A hook's return value is ignored: scripts write through engine actions.
            let _ = engine
                .engine
                .call_fn::<Dynamic>(&mut run.scope, &compiled.ast, hook, args)
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
                    world
                        .get_mut::<TextRendering>(entity)
                        .context("Set Text needs Text Rendering")?
                        .text = text;
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
                    token,
                    asset,
                    position,
                } => {
                    let id = self.spawn_prefab(world, &asset, position)?;
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
                Command::Print(text) => {
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
            self.destroy_prefab_raw(world, target)
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
            result.map(|_| ())?;
        }
        Ok(())
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
            material.color = color;
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
        if !self.has_scripts() {
            return Ok(());
        }
        let engine = self.script_engine();
        let mut runtime = world.remove_resource::<ScriptRuntime>().unwrap_or_default();
        let owners: Vec<String> = self
            .document
            .objects
            .iter()
            .filter(|object| object.script_manager.is_some())
            .map(|object| object.id.clone())
            .collect();
        let result = (|| -> Result<()> {
            for owner in &owners {
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
        ensure!(
            sources.len() < MAX_SCRIPT_ASSETS,
            "scene catalog holds at most {MAX_SCRIPT_ASSETS} scripts"
        );
        let text = std::fs::read_to_string(root.join(&source.path))
            .with_context(|| format!("loading script '{id}'"))?;
        bytes += text.len();
        ensure!(bytes <= 32 * 1024 * 1024, "scripts exceed 32 MiB");
        sources.insert(id.clone(), text);
    }
    if !sources.is_empty() {
        for entry in sources.keys() {
            ensure!(
                document.assets[entry].kind == AssetKind::Script,
                "script catalog entry changed while loading"
            );
        }
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;

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
