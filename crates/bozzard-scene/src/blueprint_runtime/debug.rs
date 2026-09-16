//! Optional debugger state. Kept outside authored documents and gameplay saves.
use super::*;
use bozzard_diagnostics::{Diagnostics, ExecutionControl};

pub const TRACE_LIMIT: usize = 256;
const PREVIEW_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Breakpoint {
    pub scene: String,
    pub object: String,
    pub attachment: usize,
    pub node: u32,
}
#[derive(Clone, Debug, Serialize)]
pub struct PinWatch {
    pub direction: &'static str,
    pub name: String,
    pub kind: blueprint::PinType,
    pub value: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct VariableWatch {
    pub scope: &'static str,
    pub name: String,
    pub value: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct NodeSnapshot {
    /// True for teardown performed atomically by a host/scene transition.
    pub atomic: bool,
    pub location: Breakpoint,
    pub graph: String,
    pub node_name: String,
    pub tick: Option<u64>,
    pub event: String,
    pub context: String,
    pub pins: Vec<PinWatch>,
}
#[derive(Clone, Debug, Serialize)]
pub struct WatchSnapshot {
    pub node: Option<NodeSnapshot>,
    pub variables: Vec<VariableWatch>,
    pub timers: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugPause {
    Requested,
    Breakpoint,
    NodeStep,
    TickStep,
    Error,
}
#[derive(Clone, Copy, Debug)]
pub enum DebugCommand {
    Pause,
    Continue,
    StepNode,
    StepTick,
}
#[derive(Clone)]
pub struct BlueprintDebugger {
    pub enabled: bool,
    pub breakpoints: BTreeSet<Breakpoint>,
    /// Optional authoring identities prevent stale saved breakpoints from targeting a changed graph.
    pub breakpoint_guards: BTreeMap<Breakpoint, (String, blueprint::NodeKind)>,
    pub tracing: bool,
    pub trace: VecDeque<NodeSnapshot>,
    pub discarded: u64,
    pub current: Option<NodeSnapshot>,
    pub paused: Option<DebugPause>,
    pub revision: u64,
    pub error: Option<String>,
    remaining: Option<usize>,
    skip: Option<Breakpoint>,
    ignore_breakpoints: bool,
}
impl Default for BlueprintDebugger {
    fn default() -> Self {
        Self {
            enabled: false,
            breakpoints: BTreeSet::new(),
            breakpoint_guards: BTreeMap::new(),
            tracing: true,
            trace: VecDeque::new(),
            discarded: 0,
            current: None,
            paused: None,
            revision: 0,
            error: None,
            remaining: None,
            skip: None,
            ignore_breakpoints: false,
        }
    }
}
impl BlueprintDebugger {
    pub fn new(breakpoints: impl IntoIterator<Item = Breakpoint>) -> Self {
        Self {
            enabled: true,
            breakpoints: breakpoints.into_iter().collect(),
            ..Self::default()
        }
    }
    pub fn send(world: &mut World, command: DebugCommand) {
        let mut debugger = world.remove_resource::<Self>().unwrap_or_default();
        let mut control = world
            .remove_resource::<ExecutionControl>()
            .unwrap_or_default();
        debugger.command(&mut control, command);
        world.insert_resource(control);
        world.insert_resource(debugger);
    }
    pub fn command(&mut self, control: &mut ExecutionControl, command: DebugCommand) {
        if self.error.is_some() {
            return;
        }
        self.enabled = true;
        self.ignore_breakpoints = matches!(command, DebugCommand::StepTick);
        self.remaining = matches!(command, DebugCommand::StepNode).then_some(1);
        control.pause_after_tick = matches!(command, DebugCommand::StepTick);
        if matches!(command, DebugCommand::Pause) {
            if control.paused {
                return;
            }
            self.current = None;
            control.paused = true;
            self.paused = Some(DebugPause::Requested);
            self.revision = self.revision.wrapping_add(1);
        } else {
            self.skip = self
                .current
                .as_ref()
                .filter(|_| {
                    control.paused
                        && matches!(
                            self.paused,
                            Some(DebugPause::Breakpoint | DebugPause::NodeStep)
                        )
                })
                .map(|s| s.location.clone());
            control.paused = false;
            self.paused = None;
        }
    }
    pub fn clear_trace(&mut self) {
        self.trace.clear();
        self.discarded = 0;
    }
    fn pause_at(
        &mut self,
        location: &Breakpoint,
        graph: &str,
        kind: blueprint::NodeKind,
    ) -> Option<DebugPause> {
        if self.remaining == Some(0) {
            return Some(DebugPause::NodeStep);
        }
        if self.skip.as_ref() == Some(location) {
            self.skip = None;
            return None;
        }
        if !self.ignore_breakpoints && self.breakpoint_matches(location, graph, kind) {
            Some(DebugPause::Breakpoint)
        } else {
            None
        }
    }
    fn breakpoint_matches(
        &self,
        location: &Breakpoint,
        graph: &str,
        kind: blueprint::NodeKind,
    ) -> bool {
        self.breakpoints.contains(location)
            && self
                .breakpoint_guards
                .get(location)
                .is_none_or(|(name, k)| name == graph && *k == kind)
    }
    fn record(&mut self, snapshot: NodeSnapshot) {
        if self.trace.len() == TRACE_LIMIT {
            self.trace.pop_front();
            self.discarded = self.discarded.saturating_add(1);
        }
        self.trace.push_back(snapshot);
    }
}
fn bounded(text: &str) -> String {
    if text.len() <= PREVIEW_BYTES {
        return text.to_owned();
    }
    let mut preview = text[..text.floor_char_boundary(PREVIEW_BYTES)].to_owned();
    preview.push('…');
    preview
}
fn preview(value: &Value) -> String {
    match value {
        Value::Exec => "Execution".into(),
        Value::Text(text) => bounded(text),
        Value::Number(n) => n.to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Vector(v) => format!("[{}, {}, {}]", v[0], v[1], v[2]),
        Value::Object(ObjectRef::None) => "None".into(),
        Value::Object(ObjectRef::SelfObject) => "Self".into(),
        Value::Object(ObjectRef::Id(id)) => bounded(id),
    }
}
fn board_watches(out: &mut Vec<VariableWatch>, scope: &'static str, board: &Blackboard) {
    for (name, value) in board {
        let value = match value {
            B::Scalar(value) => preview(value),
            B::List {
                values, capacity, ..
            } => {
                let mut text = format!("{} / {capacity} items [", values.len());
                let mut shown = 0;
                for (i, value) in values.iter().take(8).enumerate() {
                    if i > 0 {
                        text.push_str(", ");
                    }
                    text.push_str(&preview(value));
                    shown += 1;
                    if text.len() >= PREVIEW_BYTES {
                        break;
                    }
                }
                text.truncate(text.floor_char_boundary(text.len().min(PREVIEW_BYTES)));
                text.push_str(if values.len() > shown || text.len() >= PREVIEW_BYTES {
                    ", …]"
                } else {
                    "]"
                });
                text
            }
        };
        out.push(VariableWatch {
            scope,
            name: name.clone(),
            value,
        });
    }
}

#[derive(Clone, Default)]
pub(super) struct LastEvent {
    pub context: EventContext,
    pub input: GameplayInput,
    pub dt: f32,
    pub overlap: usize,
}

impl SceneInstance {
    #[allow(clippy::too_many_arguments)]
    fn node_snapshot(
        &self,
        world: &World,
        runtime: &BlueprintRuntime,
        run: &Run,
        owner: &str,
        program: &Program,
        node: &Node,
        context: &EventContext,
        input: GameplayInput,
        dt: f32,
        overlap_count: usize,
    ) -> NodeSnapshot {
        let mut eval = Eval {
            program,
            board: &run.board,
            object_board: &runtime.object_boards[owner],
            scene_board: &runtime.scene_board,
            results: &run.results,
            variables: &run.variables,
            spawned: &run.spawned,
            grounded: &run.grounded,
            world,
            entities: &self.entities,
            owner,
            context,
            overlap_count,
            input,
            dt,
            elapsed: runtime.elapsed,
            cache: BTreeMap::new(),
        };
        let mut pins = Vec::new();
        for (port, (name, kind)) in node.input_pins().iter().enumerate() {
            let value = if *kind == blueprint::PinType::Exec {
                Ok(Value::Exec)
            } else {
                eval.input(node, port)
            };
            pins.push(PinWatch {
                direction: "Input",
                name: (*name).into(),
                kind: *kind,
                value: value
                    .map_or_else(|e| bounded(&format!("Unavailable: {e}")), |v| preview(&v)),
            });
        }
        for (port, (name, kind)) in node.output_pins().iter().enumerate() {
            let value = if *kind == blueprint::PinType::Exec {
                Ok(Value::Exec)
            } else {
                eval.output(Socket {
                    node: node.id,
                    port,
                })
            };
            pins.push(PinWatch {
                direction: if node.kind.action() {
                    "Previous output"
                } else {
                    "Output"
                },
                name: (*name).into(),
                kind: *kind,
                value: value
                    .map_or_else(|e| bounded(&format!("Unavailable: {e}")), |v| preview(&v)),
            });
        }
        let event = program
            .graph
            .nodes
            .iter()
            .find(|n| n.id == context.event)
            .map_or("Event", |n| n.kind.title());
        NodeSnapshot {
            atomic: false,
            location: Breakpoint {
                scene: self.document.name.clone(),
                object: owner.into(),
                attachment: run.attachment,
                node: node.id,
            },
            graph: program.graph.name.clone(),
            node_name: node.kind.title().into(),
            tick: world.resource::<Diagnostics>().and_then(|d| d.tick),
            event: event.into(),
            context: format!(
                "Other: {} · Normal: {:?} · Impulse/value: {} · Message: {}",
                context.other.as_deref().unwrap_or("None"),
                context.normal,
                context.impulse,
                &context.message[..context
                    .message
                    .floor_char_boundary(context.message.len().min(PREVIEW_BYTES))]
            ),
            pins,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn debug_before_node(
        &self,
        world: &mut World,
        runtime: &BlueprintRuntime,
        run: &Run,
        owner: &str,
        program: &Program,
        node: &Node,
        context: &EventContext,
        input: GameplayInput,
        dt: f32,
        overlap_count: usize,
    ) -> bool {
        let Some(debugger) = world
            .resource_mut::<BlueprintDebugger>()
            .filter(|d| d.enabled)
        else {
            return false;
        };
        if !debugger.tracing
            && debugger.remaining.is_none()
            && (debugger.breakpoints.is_empty() || debugger.ignore_breakpoints)
        {
            return false;
        }
        let location = Breakpoint {
            scene: self.document.name.clone(),
            object: owner.into(),
            attachment: run.attachment,
            node: node.id,
        };
        let reason = debugger.pause_at(&location, &program.graph.name, node.kind);
        let capture = reason.is_some() || debugger.tracing;
        let snapshot = capture.then(|| {
            self.node_snapshot(
                world,
                runtime,
                run,
                owner,
                program,
                node,
                context,
                input,
                dt,
                overlap_count,
            )
        });
        let debugger = world.resource_mut::<BlueprintDebugger>().unwrap();
        if let Some(reason) = reason {
            debugger.current = snapshot;
            debugger.paused = Some(reason);
            debugger.revision = debugger.revision.wrapping_add(1);
            world
                .resource_mut::<ExecutionControl>()
                .expect("debugger execution control")
                .paused = true;
            true
        } else {
            if let Some(remaining) = &mut debugger.remaining {
                *remaining = remaining.saturating_sub(1);
            }
            if let Some(snapshot) = snapshot {
                debugger.record(snapshot);
            }
            false
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn debug_atomic_node(
        &self,
        world: &mut World,
        runtime: &BlueprintRuntime,
        run: &Run,
        owner: &str,
        program: &Program,
        node: &Node,
        context: &EventContext,
        input: GameplayInput,
        dt: f32,
        overlap_count: usize,
    ) {
        let Some(debugger) = world.resource::<BlueprintDebugger>().filter(|d| d.enabled) else {
            return;
        };
        if !debugger.tracing && debugger.breakpoints.is_empty() {
            return;
        }
        let mut snapshot = self.node_snapshot(
            world,
            runtime,
            run,
            owner,
            program,
            node,
            context,
            input,
            dt,
            overlap_count,
        );
        snapshot.atomic = true;
        let debugger = world.resource_mut::<BlueprintDebugger>().unwrap();
        let hit = debugger.breakpoint_matches(&snapshot.location, &program.graph.name, node.kind);
        if debugger.tracing || hit {
            debugger.record(snapshot);
        }
        if hit {
            bozzard_diagnostics::log(
                world,
                bozzard_diagnostics::Level::Warning,
                "Blueprint debugger",
                "Breakpoint encountered during atomic teardown. Scene/host teardown finishes as one operation; inspect its recorded pins in the Blueprint trace.",
                bozzard_diagnostics::Location {
                    object: Some(owner.into()),
                    attachment: Some(run.attachment),
                    node: Some(node.id),
                    ..Default::default()
                },
            );
        }
    }
    /// Read values without running actions. Container previews are bounded; gameplay data is untouched.
    pub fn inspect_blueprint(
        &self,
        world: &World,
        owner: &str,
        attachment: usize,
        node: Option<u32>,
    ) -> Option<WatchSnapshot> {
        let runtime = world.resource::<BlueprintRuntime>()?;
        let active = runtime
            .pending
            .as_ref()
            .and_then(|p| p.active.as_ref())
            .filter(|a| a.job.owner == owner && a.job.index == attachment);
        let run = active
            .map(|a| &a.run)
            .or_else(|| runtime.runs.get(&(owner.to_owned(), attachment)))?;
        let program = run.program.as_ref()?;
        let fallback = EventContext::default();
        let last = run.watch.as_deref();
        let context = active
            .and_then(|a| a.events.front().map(|e| &e.1))
            .unwrap_or_else(|| last.map_or(&fallback, |w| &w.context));
        let pending = runtime.pending.as_ref();
        let node = node.and_then(|id| program.node(id).ok()).map(|node| {
            self.node_snapshot(
                world,
                runtime,
                run,
                owner,
                program,
                node,
                context,
                pending.map_or_else(
                    || last.map_or(GameplayInput::default(), |w| w.input),
                    |p| p.input,
                ),
                pending.map_or_else(|| last.map_or(0., |w| w.dt), |p| p.dt),
                active.map_or_else(|| last.map_or(0, |w| w.overlap), |a| a.overlap.len()),
            )
        });
        let mut variables = Vec::new();
        for (name, value) in &run.variables {
            variables.push(VariableWatch {
                scope: "Graph",
                name: name.clone(),
                value: value.to_string(),
            });
        }
        board_watches(&mut variables, "Graph", &run.board);
        if let Some(board) = runtime.object_boards.get(owner) {
            board_watches(&mut variables, "Object", board);
        }
        board_watches(&mut variables, "Scene", &runtime.scene_board);
        let timers = run
            .timers
            .iter()
            .map(|t| {
                format!(
                    "Node {}: {:.3} s remaining",
                    t.output.node,
                    t.remaining.max(0.)
                )
            })
            .collect();
        Some(WatchSnapshot {
            node,
            variables,
            timers,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) enum ExecTask {
    Output(Socket),
    Action(u32),
}
#[derive(Clone)]
struct GraphJob {
    owner: String,
    index: usize,
    enabled: bool,
    destroying: bool,
}
#[derive(Clone)]
struct ActiveGraph {
    job: GraphJob,
    run: Run,
    events: VecDeque<(Socket, EventContext)>,
    queue: VecDeque<ExecTask>,
    overlap: BTreeSet<String>,
    collisions: Vec<Contact>,
}
#[derive(Clone)]
pub(super) struct PendingTick {
    pub ui_dispatch: bool,
    ui_only: bool,
    dt: f32,
    input: GameplayInput,
    contacts: BTreeMap<String, BTreeSet<String>>,
    solid_contacts: BTreeMap<String, Vec<Contact>>,
    jobs: VecDeque<GraphJob>,
    active: Option<ActiveGraph>,
    finish_destroy: Option<String>,
    budget: usize,
    geometry: Option<CollisionSnapshot>,
}
impl PendingTick {
    pub(super) fn pending_timers(&self) -> usize {
        self.active
            .as_ref()
            .map_or(0, |active| active.run.timers.len())
    }
}
impl SceneInstance {
    pub(super) fn step_blueprints_debug(
        &mut self,
        world: &mut World,
        dt: f32,
        input: GameplayInput,
        ui_dispatch: bool,
    ) -> Result<()> {
        if world
            .resource::<ExecutionControl>()
            .is_some_and(|c| c.paused)
        {
            return Ok(());
        }
        if world.resource::<ExecutionControl>().is_none() {
            world.insert_resource(ExecutionControl::default());
        }
        let mut runtime = world
            .remove_resource::<BlueprintRuntime>()
            .unwrap_or_default();
        let result = (|| -> Result<()> {
            let mut pending = if let Some(pending) = runtime.pending.take() {
                pending
            } else {
                ensure!(
                    dt.is_finite()
                        && (dt > 0. || ui_dispatch)
                        && input
                            .movement
                            .iter()
                            .chain(&input.orbit)
                            .all(|v| v.is_finite()),
                    "invalid blueprint timestep/input"
                );
                self.prepare_blueprints(&mut runtime);
                let ui_only = ui_dispatch || !crate::game_flow::simulation_running(world);
                runtime.query_budget = 1_000_000;
                runtime.stats.query_geometry_builds = 0;
                runtime.stats.actions = 0;
                if !ui_only {
                    runtime.elapsed += dt;
                }
                ensure!(runtime.elapsed.is_finite(), "blueprint clock overflow");
                let (contacts, solid_contacts) = self.blueprint_event_contacts(world, ui_only)?;
                let jobs = self
                    .document
                    .objects
                    .iter()
                    .flat_map(|o| {
                        o.blueprints.iter().enumerate().map(|(index, a)| GraphJob {
                            owner: o.id.clone(),
                            index,
                            enabled: a.enabled,
                            destroying: false,
                        })
                    })
                    .collect();
                Box::new(PendingTick {
                    ui_dispatch,
                    ui_only,
                    dt,
                    input,
                    contacts,
                    solid_contacts,
                    jobs,
                    active: None,
                    finish_destroy: None,
                    budget: 100_000,
                    geometry: None,
                })
            };
            loop {
                if let Some(mut active) = pending.active.take() {
                    let program = active.run.program.as_ref().unwrap().clone();
                    while let Some((output, context)) = active.events.front() {
                        let result = self.execute_blueprint(
                            world,
                            &mut runtime,
                            &mut active.run,
                            &active.job.owner,
                            &program,
                            *output,
                            context,
                            pending.input,
                            pending.dt,
                            active.overlap.len(),
                            &mut pending.budget,
                            &mut pending.geometry,
                            active.job.destroying,
                            Some(&mut active.queue),
                        );
                        match result {
                            Ok(false) => {
                                pending.active = Some(active);
                                runtime.pending = Some(pending);
                                return Ok(());
                            }
                            Ok(true) => {
                                active.events.pop_front();
                            }
                            Err(error) => {
                                let node =
                                    active.run.last_node.and_then(|id| program.node(id).ok());
                                let snapshot = node.map(|node| {
                                    self.node_snapshot(
                                        world,
                                        &runtime,
                                        &active.run,
                                        &active.job.owner,
                                        &program,
                                        node,
                                        context,
                                        pending.input,
                                        pending.dt,
                                        active.overlap.len(),
                                    )
                                });
                                bozzard_diagnostics::log(
                                    world,
                                    bozzard_diagnostics::Level::Error,
                                    "Blueprint",
                                    &format!("{error:#}"),
                                    bozzard_diagnostics::Location {
                                        object: Some(active.job.owner.clone()),
                                        attachment: Some(active.job.index),
                                        node: active.run.last_node,
                                        ..Default::default()
                                    },
                                );
                                if let Some(debugger) = world.resource_mut::<BlueprintDebugger>() {
                                    debugger.error = Some(format!("{error:#}"));
                                    debugger.current = snapshot;
                                    debugger.paused = Some(DebugPause::Error);
                                    debugger.revision = debugger.revision.wrapping_add(1);
                                }
                                world.resource_mut::<ExecutionControl>().unwrap().paused = true;
                                runtime
                                    .runs
                                    .insert((active.job.owner, active.job.index), active.run);
                                return Err(error);
                            }
                        }
                    }
                    if !pending.ui_only && !active.job.destroying {
                        if active.job.enabled {
                            active.run.started = true;
                            active.run.overlap = active.overlap;
                            active.run.collisions =
                                active.collisions.iter().map(|c| c.other.clone()).collect();
                            active.run.held = pending.input.binding_mask();
                        } else {
                            active.run.overlap.clear();
                            active.run.collisions.clear();
                            active.run.held = 0;
                        }
                        active.run.enabled = active.job.enabled;
                    }
                    runtime
                        .runs
                        .insert((active.job.owner, active.job.index), active.run);
                }
                if let Some(job) = pending.jobs.pop_front() {
                    let Some(mut run) = runtime.runs.remove(&(job.owner.clone(), job.index)) else {
                        continue;
                    };
                    run.last_node = None;
                    let program = run.program.as_ref().unwrap().clone();
                    let overlap = pending
                        .contacts
                        .get(&job.owner)
                        .cloned()
                        .unwrap_or_default();
                    let collisions = pending
                        .solid_contacts
                        .get(&job.owner)
                        .cloned()
                        .unwrap_or_default();
                    let events = if job.destroying {
                        program
                            .graph
                            .nodes
                            .iter()
                            .filter(|n| n.kind == K::Destroy)
                            .map(|n| {
                                (
                                    Socket {
                                        node: n.id,
                                        port: 0,
                                    },
                                    EventContext {
                                        event: n.id,
                                        ..Default::default()
                                    },
                                )
                            })
                            .collect()
                    } else {
                        self.blueprint_events(
                            world,
                            &program,
                            &mut run,
                            &job.owner,
                            job.enabled,
                            &overlap,
                            &collisions,
                            pending.dt,
                            pending.input,
                            pending.ui_dispatch,
                            pending.ui_only,
                        )
                    };
                    pending.active = Some(ActiveGraph {
                        job,
                        run,
                        events: events.into(),
                        queue: VecDeque::new(),
                        overlap,
                        collisions,
                    });
                    continue;
                }
                if let Some(target) = pending.finish_destroy.take() {
                    self.destroy_prefab_raw(world, &target)?;
                }
                if let Some(target) = runtime.destroying.pop_first() {
                    if !self.entities.contains_key(&target) {
                        continue;
                    }
                    self.prepare_blueprints(&mut runtime);
                    let ids: Vec<_> = self
                        .document
                        .prefabs
                        .values()
                        .find(|p| p.members.values().any(|id| id == &target))
                        .context("Destroy Prefab target is not a live prefab instance")?
                        .members
                        .values()
                        .cloned()
                        .collect();
                    ensure!(
                        !self.document.views.values().any(|id| ids.contains(id)),
                        "cannot destroy an active camera; switch views first"
                    );
                    for owner in ids {
                        for (id, index) in runtime.runs.keys().filter(|(id, _)| id == &owner) {
                            pending.jobs.push_back(GraphJob {
                                owner: id.clone(),
                                index: *index,
                                enabled: true,
                                destroying: true,
                            });
                        }
                    }
                    pending.finish_destroy = Some(target);
                    continue;
                }
                runtime
                    .runs
                    .retain(|(id, _), _| self.entities.contains_key(id));
                runtime
                    .object_boards
                    .retain(|id, _| self.entities.contains_key(id));
                if let Some(debugger) = world.resource_mut::<BlueprintDebugger>() {
                    let node_step = debugger.remaining.is_some();
                    let tick_step = debugger.ignore_breakpoints;
                    if node_step || tick_step {
                        debugger.paused = Some(if node_step {
                            DebugPause::NodeStep
                        } else {
                            DebugPause::TickStep
                        });
                        // The last event has completed; there is no stopped node to skip on resume.
                        debugger.current = None;
                        debugger.revision = debugger.revision.wrapping_add(1);
                        let control = world.resource_mut::<ExecutionControl>().unwrap();
                        if pending.ui_dispatch && node_step {
                            control.paused = true;
                        } else {
                            control.pause_after_tick = true;
                        }
                    }
                }
                return Ok(());
            }
        })();
        let suspended = runtime.pending.is_some();
        world.insert_resource(runtime);
        if !suspended
            && let Some(signals) = world.resource_mut::<crate::middleware::signals::Signals>()
        {
            signals.begin(crate::middleware::signals::Kind::Ui);
        }
        let result = result.and_then(|()| {
            if suspended {
                Ok(())
            } else {
                self.apply_scene_controls(world)
            }
        });
        if let Err(error) = &result {
            if let Some(debugger) = world.resource_mut::<BlueprintDebugger>()
                && debugger.error.is_none()
            {
                debugger.error = Some(format!("{error:#}"));
                debugger.current = None;
                debugger.paused = Some(DebugPause::Error);
                debugger.revision = debugger.revision.wrapping_add(1);
            }
            world.resource_mut::<ExecutionControl>().unwrap().paused = true;
        }
        result
    }
}
