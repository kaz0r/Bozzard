//! Bounded fixed-step graph interpreter shared by editor Play, native player and server.
use super::*;
use blueprint::{Blackboard, BlackboardValue as B, VariableScope as Scope};
use blueprint::{Blueprint, Node, NodeKind as K, ObjectRef, Socket, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

/// Indexed once per attachment; reused across all ticks and data evaluations.
#[derive(Clone)]
struct Program {
    graph: Blueprint,
    nodes: BTreeMap<u32, usize>,
    incoming: BTreeMap<Socket, Socket>,
    outgoing: BTreeMap<Socket, Vec<u32>>,
}
impl Program {
    fn new(graph: &Blueprint) -> Self {
        let mut outgoing: BTreeMap<Socket, Vec<u32>> = BTreeMap::new();
        for w in &graph.wires {
            if graph
                .node(w.to.node)
                .is_ok_and(|n| n.input_pins()[w.to.port].1 == blueprint::PinType::Exec)
            {
                outgoing.entry(w.from).or_default().push(w.to.node);
            }
        }
        Self {
            graph: graph.clone(),
            nodes: graph
                .nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id, i))
                .collect(),
            incoming: graph.wires.iter().map(|w| (w.to, w.from)).collect(),
            outgoing,
        }
    }
    fn node(&self, id: u32) -> Result<&Node> {
        self.nodes
            .get(&id)
            .map(|&i| &self.graph.nodes[i])
            .context("missing blueprint node")
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct EventContext {
    event: u32,
    other: Option<String>,
    normal: [f32; 3],
    impulse: f32,
}
#[derive(Clone, Serialize, Deserialize)]
struct Timer {
    remaining: f32,
    output: Socket,
    context: EventContext,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Run {
    #[serde(skip)]
    program: Option<Arc<Program>>,
    started: bool,
    enabled: bool,
    overlap: BTreeSet<String>,
    collisions: BTreeSet<String>,
    held: u128,
    grounded: BTreeMap<u32, bool>,
    variables: BTreeMap<String, f32>,
    board: Blackboard,
    spawned: BTreeMap<u32, ObjectRef>,
    results: BTreeMap<u32, Vec<Value>>,
    timers: Vec<Timer>,
    random: u64,
}
#[derive(Clone, Default, Debug)]
pub struct BlueprintStats {
    pub compiled_graphs: usize,
    pub query_geometry_builds: usize,
    pub actions: usize,
}
#[derive(Clone, Default)]
pub struct BlueprintRuntime {
    pub stats: BlueprintStats,
    query_budget: usize,
    runs: BTreeMap<(String, usize), Run>,
    pub messages: VecDeque<String>,
    elapsed: f32,
    object_boards: BTreeMap<String, Blackboard>,
    scene_board: Blackboard,
    initialized: bool,
    destroying: BTreeSet<String>,
}
impl BlueprintRuntime {
    pub fn object_blackboard(&self, id: &str) -> Option<&Blackboard> {
        self.object_boards.get(id)
    }
    pub fn scene_blackboard(&self) -> &Blackboard {
        &self.scene_board
    }
    pub fn pending_timers(&self) -> usize {
        self.runs.values().map(|r| r.timers.len()).sum()
    }
}
#[derive(Clone, Copy)]
pub struct BlueprintHidden(pub bool);

struct Eval<'a> {
    program: &'a Program,
    board: &'a Blackboard,
    object_board: &'a Blackboard,
    scene_board: &'a Blackboard,
    results: &'a BTreeMap<u32, Vec<Value>>,
    variables: &'a BTreeMap<String, f32>,
    spawned: &'a BTreeMap<u32, ObjectRef>,
    grounded: &'a BTreeMap<u32, bool>,
    world: &'a World,
    entities: &'a BTreeMap<String, Entity>,
    owner: &'a str,
    context: &'a EventContext,
    overlap_count: usize,
    input: GameplayInput,
    dt: f32,
    elapsed: f32,
    cache: BTreeMap<Socket, Value>,
}
impl Eval<'_> {
    fn variable(&self, n: &Node) -> Result<&B> {
        let board = match n.scope {
            Scope::Graph => self.board,
            Scope::Object => self.object_board,
            Scope::Scene => self.scene_board,
        };
        board.get(&n.variable).context("unknown runtime variable")
    }
    fn input(&mut self, node: &Node, port: usize) -> Result<Value> {
        if let Some(&from) = self.program.incoming.get(&Socket {
            node: node.id,
            port,
        }) {
            self.output(from)
        } else {
            Ok(node.inputs[port].clone())
        }
    }
    fn output(&mut self, socket: Socket) -> Result<Value> {
        let id = socket.node;
        if let Some(value) = self.cache.get(&socket) {
            return Ok(value.clone());
        }
        let n = self.program.node(id)?;
        if socket.port > 0
            && let Some(value) = self.results.get(&id).and_then(|v| v.get(socket.port - 1))
        {
            return Ok(value.clone());
        }
        if n.kind == K::SpawnPrefab && socket.port == 1 {
            return Ok(Value::Object(
                self.spawned.get(&id).cloned().unwrap_or(ObjectRef::None),
            ));
        }
        if n.kind == K::MoveWithCollision && socket.port == 1 {
            return Ok(Value::Bool(*self.grounded.get(&id).unwrap_or(&false)));
        }
        if socket.port > 0 && n.kind.action() {
            return Ok(n.output_pins()[socket.port].1.default_value());
        }
        let v: Vec<_> = (0..n.inputs.len())
            .map(|p| {
                if n.input_pins()[p].1 == blueprint::PinType::Exec {
                    Ok(Value::Exec)
                } else {
                    self.input(n, p)
                }
            })
            .collect::<Result<_>>()?;
        let value = match n.kind {
            K::Reroute | K::Text | K::Number | K::Boolean | K::Vector | K::Object => v[0].clone(),
            K::NumberToText => {
                let decimals = v[1].number()?;
                ensure!(
                    (0.0..=6.0).contains(&decimals) && decimals.fract() == 0.,
                    "text decimals must be an integer in 0..6"
                );
                Value::Text(format!("{:.*}", decimals as usize, v[0].number()?))
            }
            K::JoinText => Value::Text(format!("{}{}", v[0].text()?, v[1].text()?)),
            K::GetText => {
                let id = reference_id(v[0].object()?, self.owner).context("text target is None")?;
                let entity = self
                    .entities
                    .get(id)
                    .context("text target does not exist")?;
                Value::Text(
                    self.world
                        .get::<TextRendering>(*entity)
                        .context("Get Text needs Text Rendering")?
                        .text
                        .clone(),
                )
            }
            K::SelfObject => Value::Object(ObjectRef::Id(self.owner.into())),
            K::BodyEnter | K::BodyExit | K::CollisionEnter if socket.port == 1 => {
                Value::Object(if self.context.event == id {
                    self.context
                        .other
                        .clone()
                        .map_or(ObjectRef::None, ObjectRef::Id)
                } else {
                    ObjectRef::None
                })
            }
            K::CollisionEnter if socket.port == 2 => Value::Vector(if self.context.event == id {
                self.context.normal
            } else {
                [0.; 3]
            }),
            K::CollisionEnter if socket.port == 3 => Value::Number(if self.context.event == id {
                self.context.impulse
            } else {
                0.
            }),
            K::OverlapCount => Value::Number(self.overlap_count as f32),
            K::ObjectEqual => Value::Bool(
                reference_id(v[0].object()?, self.owner)
                    == reference_id(v[1].object()?, self.owner),
            ),
            K::IsValidObject => Value::Bool(
                reference_id(v[0].object()?, self.owner)
                    .and_then(|id| self.entities.get(id))
                    .is_some_and(|e| self.world.get::<Transform>(*e).is_some()),
            ),
            K::IsRigidbody => Value::Bool(
                reference_id(v[0].object()?, self.owner)
                    .and_then(|id| self.entities.get(id))
                    .is_some_and(|e| {
                        self.world.get::<Gravity>(*e).is_some_and(|g| g.enabled)
                            && self.world.get::<PlayerController>(*e).is_none()
                    }),
            ),
            K::DeltaTime => Value::Number(self.dt),
            K::ElapsedTime => Value::Number(self.elapsed),
            K::Position | K::Rotation | K::Scale => {
                let id = reference_id(v[0].object()?, self.owner)
                    .context("object read target is None")?;
                let entity = self
                    .entities
                    .get(id)
                    .context("object read target does not exist")?;
                let transform = self
                    .world
                    .get::<Transform>(*entity)
                    .context("object read target was removed")?;
                Value::Vector(match n.kind {
                    K::Position => transform.translation,
                    K::Rotation => transform.rotation_degrees,
                    _ => transform.scale,
                })
            }
            K::InputHeld => Value::Bool(n.key.active(self.input)),
            K::MoveX => Value::Number(self.input.movement[0]),
            K::MoveY => Value::Number(self.input.movement[1]),
            K::MouseX => Value::Number(self.input.orbit[0]),
            K::MouseY => Value::Number(self.input.orbit[1]),
            K::ForwardVector => {
                let id = reference_id(v[0].object()?, self.owner)
                    .context("forward vector target is None")?;
                let entity = self
                    .entities
                    .get(id)
                    .context("forward vector target does not exist")?;
                let transform = self
                    .world
                    .get::<Transform>(*entity)
                    .context("forward vector target was removed")?;
                Value::Vector(crate::physics::forward(transform).to_array())
            }
            K::BreakVector => Value::Number(v[0].vector()?[socket.port]),
            K::GetVariable => {
                if n.scope == Scope::Graph
                    && let Some(value) = self.variables.get(&n.variable)
                {
                    Value::Number(*value)
                } else {
                    match self.variable(n)? {
                        B::Scalar(v) => v.clone(),
                        _ => anyhow::bail!("expected scalar variable"),
                    }
                }
            }
            K::ListGet => {
                let i = list_index(v[0].number()?)?;
                match self.variable(n)? {
                    B::List { values, .. } => {
                        values.get(i).context("list index out of bounds")?.clone()
                    }
                    _ => anyhow::bail!("expected list"),
                }
            }
            K::ListLength => match self.variable(n)? {
                B::List { values, .. } => Value::Number(values.len() as f32),
                _ => anyhow::bail!("expected list"),
            },
            K::Add => Value::Number(v[0].number()? + v[1].number()?),
            K::Subtract => Value::Number(v[0].number()? - v[1].number()?),
            K::Multiply => Value::Number(v[0].number()? * v[1].number()?),
            K::Divide => {
                ensure!(v[1].number()? != 0., "division by zero at node {id}");
                Value::Number(v[0].number()? / v[1].number()?)
            }
            K::Sine => Value::Number(v[0].number()?.sin()),
            K::Lerp => Value::Number(
                v[0].number()? * (1. - v[2].number()?) + v[1].number()? * v[2].number()?,
            ),
            K::LerpVector => Value::Vector(
                (Vec3::from(v[0].vector()?) * (1. - v[2].number()?)
                    + Vec3::from(v[1].vector()?) * v[2].number()?)
                .to_array(),
            ),
            K::Min => Value::Number(v[0].number()?.min(v[1].number()?)),
            K::Max => Value::Number(v[0].number()?.max(v[1].number()?)),
            K::Abs => Value::Number(v[0].number()?.abs()),
            K::Length => Value::Number(Vec3::from(v[0].vector()?).length()),
            K::Normalize => {
                Value::Vector(Vec3::from(v[0].vector()?).normalize_or_zero().to_array())
            }
            K::Dot => Value::Number(Vec3::from(v[0].vector()?).dot(Vec3::from(v[1].vector()?))),
            K::Cross => Value::Vector(
                Vec3::from(v[0].vector()?)
                    .cross(Vec3::from(v[1].vector()?))
                    .to_array(),
            ),
            K::Distance => {
                Value::Number(Vec3::from(v[0].vector()?).distance(Vec3::from(v[1].vector()?)))
            }
            K::Modulo => {
                ensure!(v[1].number()? != 0., "modulo by zero at node {id}");
                Value::Number(v[0].number()?.rem_euclid(v[1].number()?))
            }
            K::Power => Value::Number(v[0].number()?.powf(v[1].number()?)),
            K::Cosine => Value::Number(v[0].number()?.cos()),
            K::Tangent => Value::Number(v[0].number()?.tan()),
            K::ArcSine => Value::Number(v[0].number()?.asin()),
            K::ArcCosine => Value::Number(v[0].number()?.acos()),
            K::Atan2 => Value::Number(v[0].number()?.atan2(v[1].number()?)),
            K::ToRadians => Value::Number(v[0].number()?.to_radians()),
            K::ToDegrees => Value::Number(v[0].number()?.to_degrees()),
            K::Floor => Value::Number(v[0].number()?.floor()),
            K::Ceil => Value::Number(v[0].number()?.ceil()),
            K::Round => Value::Number(v[0].number()?.round()),
            K::Sqrt => Value::Number(v[0].number()?.sqrt()),
            K::Clamp => {
                let (value, min, max) = (v[0].number()?, v[1].number()?, v[2].number()?);
                ensure!(min <= max, "Clamp needs Min <= Max at node {id}");
                Value::Number(value.clamp(min, max))
            }
            K::Greater => Value::Bool(v[0].number()? > v[1].number()?),
            K::Less => Value::Bool(v[0].number()? < v[1].number()?),
            K::Equal => Value::Bool(v[0].number()? == v[1].number()?),
            K::Not => Value::Bool(!v[0].boolean()?),
            K::And => Value::Bool(v[0].boolean()? && v[1].boolean()?),
            K::Or => Value::Bool(v[0].boolean()? || v[1].boolean()?),
            K::MakeVector => Value::Vector([v[0].number()?, v[1].number()?, v[2].number()?]),
            K::ScaleVector => {
                Value::Vector((Vec3::from(v[0].vector()?) * v[1].number()?).to_array())
            }
            K::AddVector => {
                Value::Vector((Vec3::from(v[0].vector()?) + Vec3::from(v[1].vector()?)).to_array())
            }
            _ => anyhow::bail!("node {id} has no data output"),
        };
        ensure!(value.valid(), "invalid or oversized output at node {id}");
        self.cache.insert(socket, value.clone());
        Ok(value)
    }
}
fn reference_id<'a>(reference: &'a ObjectRef, owner: &'a str) -> Option<&'a str> {
    match reference {
        ObjectRef::SelfObject => Some(owner),
        ObjectRef::Id(id) => Some(id),
        ObjectRef::None => None,
    }
}
fn needs_overlap(graph: &Blueprint) -> bool {
    graph.nodes.iter().any(|n| {
        matches!(
            n.kind,
            K::TriggerEnter | K::TriggerExit | K::BodyEnter | K::BodyExit | K::OverlapCount
        )
    })
}

fn list_index(v: f32) -> Result<usize> {
    ensure!(
        v.is_finite() && v >= 0. && v.fract() == 0. && v <= (usize::MAX as f32),
        "list/attachment index must be a nonnegative integer"
    );
    Ok(v as usize)
}
fn bind_self(value: Value, owner: &str) -> Value {
    if value == Value::Object(ObjectRef::SelfObject) {
        Value::Object(ObjectRef::Id(owner.into()))
    } else {
        value
    }
}
fn variable_mut<'a>(
    runtime: &'a mut BlueprintRuntime,
    run: &'a mut Run,
    owner: &str,
    node: &Node,
) -> Result<&'a mut B> {
    let board = match node.scope {
        Scope::Graph => &mut run.board,
        Scope::Object => runtime
            .object_boards
            .get_mut(owner)
            .context("missing object board")?,
        Scope::Scene => &mut runtime.scene_board,
    };
    board
        .get_mut(&node.variable)
        .context("unknown runtime variable")
}
impl SceneInstance {
    pub fn has_blueprints(&self) -> bool {
        self.document
            .objects
            .iter()
            .any(|o| !o.blueprints.is_empty())
    }
    pub fn set_blueprint_enabled(
        &mut self,
        owner: &str,
        index: usize,
        enabled: bool,
    ) -> Result<()> {
        self.document
            .objects
            .iter_mut()
            .find(|o| o.id == owner)
            .context("unknown graph owner")?
            .blueprints
            .get_mut(index)
            .context("attachment index out of bounds")?
            .enabled = enabled;
        Ok(())
    }
    fn prepare_blueprints(&self, runtime: &mut BlueprintRuntime) {
        if !runtime.initialized {
            runtime.scene_board = self.document.blackboard.clone();
            runtime.initialized = true;
        }
        for object in &self.document.objects {
            if object.blackboard.is_empty() && object.blueprints.is_empty() {
                continue;
            }
            runtime
                .object_boards
                .entry(object.id.clone())
                .or_insert_with(|| object.blackboard.clone());
            for (index, attachment) in object.blueprints.iter().enumerate() {
                let run = runtime
                    .runs
                    .entry((object.id.clone(), index))
                    .or_insert_with(|| Run {
                        variables: attachment.graph.variables.clone(),
                        board: attachment.graph.blackboard.clone(),
                        random: object.id.bytes().fold(index as u64 + 1, |n, b| {
                            n.wrapping_mul(1099511628211) ^ u64::from(b)
                        }),
                        ..Run::default()
                    });
                if run.program.is_none() {
                    runtime.stats.compiled_graphs += 1;
                    run.program = Some(Arc::new(Program::new(&attachment.graph)));
                }
            }
        }
    }
    pub fn step_blueprints(
        &mut self,
        world: &mut World,
        dt: f32,
        input: GameplayInput,
    ) -> Result<()> {
        if !crate::game_flow::simulation_running(world) {
            return Ok(());
        }
        if !self.has_blueprints() {
            return self.apply_scene_controls(world);
        }
        ensure!(
            dt.is_finite()
                && dt > 0.
                && input
                    .movement
                    .iter()
                    .chain(&input.orbit)
                    .all(|v| v.is_finite()),
            "invalid blueprint timestep/input"
        );
        let mut runtime = world
            .remove_resource::<BlueprintRuntime>()
            .unwrap_or_default();
        self.prepare_blueprints(&mut runtime);
        runtime.query_budget = 1_000_000;
        runtime.stats.query_geometry_builds = 0;
        runtime.stats.actions = 0;
        let result = (|| -> Result<()> {
            runtime.elapsed += dt;
            ensure!(runtime.elapsed.is_finite(), "blueprint clock overflow");
            let query_overlaps = self
                .document
                .objects
                .iter()
                .flat_map(|o| &o.blueprints)
                .filter(|b| b.enabled)
                .any(|b| needs_overlap(&b.graph));
            let query_solids = self
                .document
                .objects
                .iter()
                .flat_map(|o| &o.blueprints)
                .filter(|a| a.enabled)
                .any(|a| a.graph.nodes.iter().any(|n| n.kind == K::CollisionEnter));
            let collision_data = (query_overlaps || query_solids)
                .then(|| self.collision_snapshot(world))
                .transpose()?;
            let collisions = collision_data.as_ref().map(|(snapshot, _)| snapshot);
            let matrices = collision_data.as_ref().map(|(_, matrices)| matrices);
            // Snapshot contacts once before graph actions. Order does not change this tick's events.
            let mut contacts = BTreeMap::new();
            let mut overlap_budget = 1_000_000usize;
            if query_overlaps && let (Some(collisions), Some(matrices)) = (&collisions, &matrices) {
                for object in self.document.objects.iter().filter(|o| {
                    o.blueprints
                        .iter()
                        .any(|b| b.enabled && needs_overlap(&b.graph))
                }) {
                    contacts.insert(object.id.clone(), BTreeSet::new());
                }
                // Reuse solid pairs once, rather than scanning every pair for every graph owner.
                for (a, b) in &collisions.overlaps {
                    ensure!(
                        overlap_budget > 0,
                        "blueprint overlap budget exceeded (1000000 tests/tick)"
                    );
                    overlap_budget -= 1;
                    if let Some(overlap) = contacts.get_mut(a) {
                        overlap.insert(b.clone());
                    }
                    if let Some(overlap) = contacts.get_mut(b) {
                        overlap.insert(a.clone());
                    }
                }
                for object in self.document.objects.iter().filter(|o| {
                    o.blueprints
                        .iter()
                        .any(|b| b.enabled && needs_overlap(&b.graph))
                }) {
                    let entity = self.entities[&object.id];
                    let collider = world.get::<Trigger>(entity).map(|t| t.volume);
                    let mut overlap = contacts.remove(&object.id).unwrap_or_default();
                    if let Some(collider) = collider.filter(|c| c.enabled) {
                        let (center, edges, corners) = collider.geometry(matrices[&object.id])?;
                        let volume = CollisionBox {
                            id: object.id.clone(),
                            entity,
                            center,
                            edges,
                            corners,
                        };
                        for body in &collisions.boxes {
                            ensure!(
                                overlap_budget > 0,
                                "blueprint overlap budget exceeded (1000000 tests/tick)"
                            );
                            overlap_budget -= 1;
                            if body.id != object.id && volume.intersects(body) {
                                overlap.insert(body.id.clone());
                            }
                        }
                        for mesh in &collisions.meshes {
                            ensure!(
                                overlap_budget > 0,
                                "blueprint overlap budget exceeded (1000000 tests/tick)"
                            );
                            overlap_budget -= 1;
                            if mesh.id != object.id && mesh.intersects(&volume) {
                                overlap.insert(mesh.id.clone());
                            }
                        }
                    }
                    contacts.insert(object.id.clone(), overlap);
                }
            }

            let solid_contacts = if query_solids {
                self.blueprint_contacts(world, collisions.unwrap(), matrices.unwrap())
            } else {
                BTreeMap::new()
            };
            let owners: Vec<_> = self
                .document
                .objects
                .iter()
                .filter(|o| !o.blueprints.is_empty())
                .map(|o| {
                    (
                        o.id.clone(),
                        o.blueprints.iter().map(|a| a.enabled).collect::<Vec<_>>(),
                    )
                })
                .collect();
            let mut budget = 100_000usize;
            let mut geometry = None;
            for (owner, attachments) in owners {
                let overlap = contacts.get(&owner).cloned().unwrap_or_default();
                let collisions = solid_contacts.get(&owner).cloned().unwrap_or_default();
                for (index, enabled) in attachments.into_iter().enumerate() {
                    let key = (owner.clone(), index);
                    let mut run = runtime.runs.remove(&key).unwrap();
                    let program = run.program.clone().unwrap();
                    let step =
                        (|| -> Result<()> {
                            let mut events = Vec::new();
                            if enabled {
                                for timer in &mut run.timers {
                                    timer.remaining -= dt;
                                }
                                for timer in &run.timers {
                                    if timer.remaining <= 0. {
                                        events.push((timer.output, timer.context.clone()));
                                    }
                                }
                                run.timers.retain(|t| t.remaining > 0.);
                            } else {
                                run.timers.clear();
                            }
                            for event in program.graph.nodes.iter().filter(|n| n.kind.event()) {
                                let fire = match event.kind {
                                    K::Enable => enabled && !run.enabled,
                                    K::Disable => !enabled && run.enabled,
                                    K::Start => enabled && !run.started,
                                    K::Update => enabled,
                                    K::InputPressed => {
                                        enabled
                                            && event.key.active(input)
                                            && (event.key.instant()
                                                || run.held & event.key.bit() == 0)
                                    }
                                    K::TriggerEnter => {
                                        enabled && !overlap.is_empty() && run.overlap.is_empty()
                                    }
                                    K::TriggerExit => {
                                        enabled && overlap.is_empty() && !run.overlap.is_empty()
                                    }
                                    _ => false,
                                };
                                let mut contexts = Vec::new();
                                match event.kind {
                                    K::BodyEnter if enabled => contexts.extend(
                                        overlap.difference(&run.overlap).map(|id| EventContext {
                                            event: event.id,
                                            other: Some(id.clone()),
                                            ..Default::default()
                                        }),
                                    ),
                                    K::BodyExit if enabled => contexts.extend(
                                        run.overlap.difference(&overlap).map(|id| EventContext {
                                            event: event.id,
                                            other: Some(id.clone()),
                                            ..Default::default()
                                        }),
                                    ),
                                    K::CollisionEnter if enabled => contexts.extend(
                                        collisions
                                            .iter()
                                            .filter(|c| !run.collisions.contains(&c.other))
                                            .map(|c| EventContext {
                                                event: event.id,
                                                other: Some(c.other.clone()),
                                                normal: c.normal.to_array(),
                                                impulse: c.impulse,
                                            }),
                                    ),
                                    _ if fire => contexts.push(EventContext {
                                        event: event.id,
                                        ..Default::default()
                                    }),
                                    _ => {}
                                }
                                events.extend(contexts.into_iter().map(|c| {
                                    (
                                        Socket {
                                            node: event.id,
                                            port: 0,
                                        },
                                        c,
                                    )
                                }));
                            }
                            for (output, context) in events {
                                self.execute_blueprint(
                                    world,
                                    &mut runtime,
                                    &mut run,
                                    &owner,
                                    &program,
                                    output,
                                    &context,
                                    input,
                                    dt,
                                    overlap.len(),
                                    &mut budget,
                                    &mut geometry,
                                    false,
                                )?;
                            }
                            if enabled {
                                run.started = true;
                                run.overlap = overlap.clone();
                                run.collisions =
                                    collisions.iter().map(|c| c.other.clone()).collect();
                                run.held = input.binding_mask();
                            } else {
                                run.overlap.clear();
                                run.collisions.clear();
                                run.held = 0;
                            }
                            run.enabled = enabled;
                            Ok(())
                        })();
                    runtime.runs.insert(key, run);
                    step?;
                }
            }
            if !runtime.destroying.is_empty() {
                self.prepare_blueprints(&mut runtime);
            }
            while let Some(target) = runtime.destroying.pop_first() {
                if !self.entities.contains_key(&target) {
                    continue;
                }
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
                for owner in &ids {
                    self.destroy_blueprint_events(
                        world,
                        &mut runtime,
                        owner,
                        input,
                        dt,
                        &mut budget,
                    )?;
                }
                self.destroy_prefab_raw(world, &target)?;
            }
            runtime
                .runs
                .retain(|(id, _), _| self.entities.contains_key(id));
            runtime
                .object_boards
                .retain(|id, _| self.entities.contains_key(id));
            Ok(())
        })();
        world.insert_resource(runtime);
        result?;
        self.apply_scene_controls(world)
    }
    #[allow(clippy::too_many_arguments)]
    fn execute_blueprint(
        &mut self,
        world: &mut World,
        runtime: &mut BlueprintRuntime,
        run: &mut Run,
        owner: &str,
        program: &Program,
        output: Socket,
        context: &EventContext,
        input: GameplayInput,
        dt: f32,
        overlap_count: usize,
        budget: &mut usize,
        geometry: &mut Option<CollisionSnapshot>,
        destroying: bool,
    ) -> Result<()> {
        ensure!(*budget > 0, "blueprint execution budget exceeded");
        *budget -= 1;
        let mut queue = VecDeque::from([output]);
        while let Some(output) = queue.pop_front() {
            for &id in program.outgoing.get(&output).into_iter().flatten() {
                if !destroying
                    && (!crate::game_flow::simulation_running(world)
                        || runtime.destroying.iter().any(|target| {
                            target == owner
                                || self.document.prefabs.values().any(|p| {
                                    p.members.values().any(|id| id == target)
                                        && p.members.values().any(|id| id == owner)
                                })
                        }))
                {
                    return Ok(());
                }
                ensure!(*budget > 0, "blueprint execution budget exceeded");
                *budget -= 1;
                runtime.stats.actions += 1;
                let node = program.node(id)?;
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
                let value = if node.inputs.len() > 1 {
                    eval.input(node, 1)?
                } else {
                    Value::Exec
                };
                ensure!(value.valid(), "invalid action value at node {id}");
                let target = if let Some(port) = node.kind.target_port() {
                    let v = eval.input(node, port)?;
                    reference_id(v.object()?, owner)
                        .context("action target is None")?
                        .to_owned()
                } else {
                    owner.to_owned()
                };
                let entity = *self
                    .entities
                    .get(&target)
                    .context("blueprint target does not exist")?;
                let transform = *world
                    .get::<Transform>(entity)
                    .context("blueprint target was removed")?;
                let mut port = 0;
                match node.kind {
                    K::SpawnPrefab => {
                        let id = self.spawn_prefab(world, &node.prefab, value.vector()?)?;
                        run.spawned.insert(node.id, ObjectRef::Id(id));
                    }
                    K::DestroyPrefab => {
                        runtime.destroying.insert(target.clone());
                    }
                    K::Branch => port = usize::from(!value.boolean()?),
                    K::SetVariable => {
                        if node.scope == Scope::Graph && run.variables.contains_key(&node.variable)
                        {
                            run.variables.insert(node.variable.clone(), value.number()?);
                        } else {
                            let entry = variable_mut(runtime, run, owner, node)?;
                            ensure!(
                                matches!(entry,B::Scalar(old) if old.kind()==value.kind()),
                                "variable type mismatch"
                            );
                            *entry = B::Scalar(bind_self(value.clone(), owner));
                        }
                    }
                    K::ListPush | K::ListSet | K::ListRemove | K::ListClear => {
                        let index = if node.kind == K::ListSet {
                            Some(list_index(eval.input(node, 2)?.number()?)?)
                        } else if node.kind == K::ListRemove {
                            Some(list_index(value.number()?)?)
                        } else {
                            None
                        };
                        let B::List {
                            element,
                            capacity,
                            values,
                        } = variable_mut(runtime, run, owner, node)?
                        else {
                            anyhow::bail!("expected list")
                        };
                        match node.kind {
                            K::ListPush => {
                                ensure!(
                                    values.len() < *capacity && value.kind() == *element,
                                    "list full or value type mismatch"
                                );
                                values.push(bind_self(value.clone(), owner));
                            }
                            K::ListSet => {
                                ensure!(value.kind() == *element, "list value type mismatch");
                                *values
                                    .get_mut(index.unwrap())
                                    .context("list index out of bounds")? =
                                    bind_self(value.clone(), owner);
                            }
                            K::ListRemove => {
                                let i = index.unwrap();
                                ensure!(i < values.len(), "list index out of bounds");
                                values.remove(i);
                            }
                            _ => values.clear(),
                        }
                    }
                    K::Reroute => {}
                    K::Delay => {
                        let seconds = value.number()?;
                        ensure!(
                            seconds.is_finite() && seconds >= 0.,
                            "delay must be finite and nonnegative"
                        );
                        ensure!(
                            run.timers.len() < 256,
                            "timer limit: 256 pending per attachment"
                        );
                        run.timers.push(Timer {
                            remaining: seconds,
                            output: Socket {
                                node: node.id,
                                port: 0,
                            },
                            context: context.clone(),
                        });
                        continue;
                    }
                    K::SetGraphEnabled => {
                        let index = list_index(eval.input(node, 3)?.number()?)?;
                        self.set_blueprint_enabled(&target, index, value.boolean()?)?;
                    }
                    K::Random => {
                        let min = value.number()?;
                        let max = eval.input(node, 2)?.number()?;
                        ensure!(
                            min <= max && (max - min).is_finite(),
                            "Random needs finite Min <= Max"
                        );
                        run.random = run
                            .random
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        let fraction = (run.random >> 40) as f32 / 16777216.;
                        run.results
                            .insert(node.id, vec![Value::Number(min + (max - min) * fraction)]);
                    }
                    K::Raycast | K::SphereOverlap | K::BoxOverlap | K::LineOfSight => {
                        let inputs = (1..node.inputs.len())
                            .map(|p| eval.input(node, p))
                            .collect::<Result<Vec<_>>>()?;
                        if geometry.is_none() {
                            ensure!(
                                runtime.query_budget >= self.entities.len(),
                                "blueprint spatial query budget exceeded"
                            );
                            runtime.query_budget -= self.entities.len();
                            *geometry = Some(self.query_geometry(world)?);
                            runtime.stats.query_geometry_builds += 1;
                        }
                        let query = geometry.as_ref().unwrap();
                        let ignore = reference_id(inputs.last().unwrap().object()?, owner);
                        let origin = Vec3::from(inputs[0].vector()?);
                        let result = match node.kind {
                            K::Raycast => {
                                let hit = query.raycast_budget(
                                    origin,
                                    Vec3::from(inputs[1].vector()?),
                                    inputs[2].number()?,
                                    ignore,
                                    &mut runtime.query_budget,
                                )?;
                                match hit {
                                    Some(h) => vec![
                                        Value::Bool(true),
                                        Value::Object(ObjectRef::Id(h.object)),
                                        Value::Vector(h.position.to_array()),
                                        Value::Vector(h.normal.to_array()),
                                        Value::Number(h.distance),
                                    ],
                                    None => vec![
                                        Value::Bool(false),
                                        Value::Object(ObjectRef::None),
                                        Value::Vector([0.; 3]),
                                        Value::Vector([0.; 3]),
                                        Value::Number(0.),
                                    ],
                                }
                            }
                            K::LineOfSight => {
                                let delta = Vec3::from(inputs[1].vector()?) - origin;
                                let clear = if delta == Vec3::ZERO {
                                    true
                                } else {
                                    query
                                        .raycast_budget(
                                            origin,
                                            delta,
                                            delta.length(),
                                            ignore,
                                            &mut runtime.query_budget,
                                        )?
                                        .is_none()
                                };
                                vec![Value::Bool(clear)]
                            }
                            _ => {
                                let capacity = match variable_mut(runtime, run, owner, node)? {
                                    B::List { capacity, .. } => *capacity,
                                    _ => anyhow::bail!("overlap requires an Object list"),
                                };
                                let hits = if node.kind == K::SphereOverlap {
                                    query.overlap_sphere_budget(
                                        origin,
                                        inputs[1].number()?,
                                        ignore,
                                        capacity,
                                        &mut runtime.query_budget,
                                    )?
                                } else {
                                    query.overlap_box_budget(
                                        origin,
                                        Vec3::from(inputs[1].vector()?),
                                        ignore,
                                        capacity,
                                        &mut runtime.query_budget,
                                    )?
                                };
                                let count = hits.len();
                                let B::List { values, .. } =
                                    variable_mut(runtime, run, owner, node)?
                                else {
                                    unreachable!()
                                };
                                *values = hits
                                    .into_iter()
                                    .map(|id| Value::Object(ObjectRef::Id(id)))
                                    .collect();
                                vec![Value::Number(count as f32)]
                            }
                        };
                        run.results.insert(node.id, result);
                    }
                    K::LoadScene | K::AddScene | K::RestartScene | K::SaveGame | K::LoadGame => {
                        self.request_scene_control(
                            world,
                            node.kind,
                            if node.inputs.len() > 1 {
                                value.text()?
                            } else {
                                ""
                            },
                        )?;
                    }
                    K::Translate | K::Rotate | K::SetPosition | K::SetRotation | K::SetScale => {
                        let mut next = transform;
                        let v = value.vector()?;
                        match node.kind {
                            K::Translate => {
                                next.translation =
                                    (Vec3::from(next.translation) + Vec3::from(v)).to_array()
                            }
                            K::Rotate => {
                                next.rotation_degrees = (Vec3::from(next.rotation_degrees)
                                    + Vec3::from(v))
                                .to_array()
                                .map(|r| r.rem_euclid(360.))
                            }
                            K::SetPosition => next.translation = v,
                            K::SetRotation => next.rotation_degrees = v,
                            K::SetScale => next.scale = v,
                            _ => unreachable!(),
                        }
                        next.validate()?;
                        world.insert(entity, next)?;
                        if let Err(error) = self.validate_transform_change(world, &target) {
                            world.insert(entity, transform)?;
                            return Err(error);
                        }
                    }
                    K::EndGame => {
                        world
                            .resource_mut::<crate::GameSession>()
                            .context("End Game needs Game Flow enabled in scene settings")?
                            .end_game(value.text()?)?;
                    }
                    K::SetText => {
                        let next = value.text()?;
                        ensure!(next.len() <= 4096, "text exceeds 4096 UTF-8 bytes");
                        world
                            .get_mut::<TextRendering>(entity)
                            .context("Set Text needs Text Rendering")?
                            .text = next.into();
                    }
                    K::SetColor => {
                        let color = value.vector()?;
                        ensure!(
                            color.iter().all(|c| (0.0..=1.0).contains(c)),
                            "blueprint RGB must be in 0..1"
                        );
                        let has_text = if let Some(text) = world.get_mut::<TextRendering>(entity) {
                            text.color[..3].copy_from_slice(&color);
                            true
                        } else {
                            false
                        };
                        if let Some(material) = world.get_mut::<Material>(entity) {
                            material.color = color;
                        } else if let Some(drawable) = world.get_mut::<Drawable>(entity) {
                            // Legacy graphs also work on meshes using their source material.
                            drawable.color = color;
                        } else {
                            ensure!(
                                has_text,
                                "Set Color needs a mesh, Material or Text Rendering"
                            );
                        }
                    }
                    K::SetVisible => {
                        world.insert(entity, BlueprintHidden(!value.boolean()?))?;
                    }
                    K::SetFocusDistance
                    | K::SetAperture
                    | K::SetFogDensity
                    | K::SetFogLightIntensity
                    | K::SetExposure
                    | K::SetBloomIntensity
                    | K::SetSaturation
                    | K::SetHeatStrength
                    | K::SetGrainIntensity
                    | K::SetVignetteIntensity => {
                        self.set_display_parameter(node.kind, value.number()?)?
                    }
                    K::SetLightIntensity => {
                        let mut light = *world
                            .get::<Light>(entity)
                            .context("Set Light Intensity needs a Light")?;
                        light.intensity = value.number()?;
                        light.validate()?;
                        world.insert(entity, light)?;
                    }
                    K::MoveWithCollision => {
                        let movement =
                            self.move_box(world, &target, Vec3::from(value.vector()?))?;
                        run.grounded.insert(
                            node.id,
                            movement
                                .contact_normals
                                .iter()
                                .any(|normal| normal.y >= 0.5),
                        );
                    }
                    K::Jump => {
                        self.jump_box(world, &target, value.number()?)?;
                    }
                    K::SetVelocity => {
                        self.set_velocity(world, &target, entity, Vec3::from(value.vector()?))?;
                    }
                    K::LockCursor => {
                        world.insert_resource(CursorCapture {
                            requested: Some(true),
                        });
                    }
                    K::UnlockCursor => {
                        world.insert_resource(CursorCapture {
                            requested: Some(false),
                        });
                    }
                    K::Print => {
                        runtime.messages.push_back(format!(
                            "{} / {}: {}",
                            owner,
                            program.graph.name,
                            value.number()?
                        ));
                        while runtime.messages.len() > 64 {
                            runtime.messages.pop_front();
                        }
                    }
                    _ => anyhow::bail!("invalid execution node"),
                }

                if matches!(
                    node.kind,
                    K::Translate
                        | K::Rotate
                        | K::SetPosition
                        | K::SetRotation
                        | K::SetScale
                        | K::MoveWithCollision
                        | K::SpawnPrefab
                        | K::DestroyPrefab
                ) {
                    *geometry = None;
                }
                queue.push_back(Socket {
                    node: node.id,
                    port,
                });
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn destroy_blueprint_events(
        &mut self,
        world: &mut World,
        runtime: &mut BlueprintRuntime,
        owner: &str,
        input: GameplayInput,
        dt: f32,
        budget: &mut usize,
    ) -> Result<()> {
        let keys: Vec<_> = runtime
            .runs
            .range((owner.to_owned(), 0)..=(owner.to_owned(), usize::MAX))
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            let mut run = runtime.runs.remove(&key).unwrap();
            let program = run.program.clone().unwrap();
            for n in program.graph.nodes.iter().filter(|n| n.kind == K::Destroy) {
                self.execute_blueprint(
                    world,
                    runtime,
                    &mut run,
                    owner,
                    &program,
                    Socket {
                        node: n.id,
                        port: 0,
                    },
                    &EventContext {
                        event: n.id,
                        ..Default::default()
                    },
                    input,
                    dt,
                    0,
                    budget,
                    &mut None,
                    true,
                )?;
            }
            runtime.runs.insert(key, run);
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimeSave {
    runs: Vec<((String, usize), Run)>,
    elapsed: f32,
    object_boards: BTreeMap<String, Blackboard>,
    scene_board: Blackboard,
}
impl BlueprintRuntime {
    pub(crate) fn save(&self) -> RuntimeSave {
        RuntimeSave {
            runs: self
                .runs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            elapsed: self.elapsed,
            object_boards: self.object_boards.clone(),
            scene_board: self.scene_board.clone(),
        }
    }
    pub(crate) fn restore(save: RuntimeSave, scene: &Scene) -> Result<Self> {
        ensure!(
            save.elapsed.is_finite() && save.elapsed >= 0.,
            "invalid saved blueprint time"
        );
        blueprint::validate_blackboard(&save.scene_board)?;
        validate_board_shape(&scene.blackboard, &save.scene_board)?;
        ensure!(
            save.object_boards.len() <= scene.objects.len(),
            "too many saved object boards"
        );
        for (id, board) in &save.object_boards {
            let object = scene
                .objects
                .iter()
                .find(|o| &o.id == id)
                .context("saved board owner missing")?;
            blueprint::validate_blackboard(board)?;
            validate_board_shape(&object.blackboard, board)?;
        }
        let mut runs = BTreeMap::new();
        for (key, mut run) in save.runs {
            let attachment = scene
                .objects
                .iter()
                .find(|o| o.id == key.0)
                .and_then(|o| o.blueprints.get(key.1))
                .context("saved graph missing")?;
            let graph = &attachment.graph;
            ensure!(
                run.variables.len() == graph.variables.len()
                    && run
                        .variables
                        .iter()
                        .all(|(k, v)| graph.variables.contains_key(k) && v.is_finite()),
                "invalid saved variables"
            );
            blueprint::validate_blackboard(&run.board)?;
            validate_board_shape(&graph.blackboard, &run.board)?;
            ensure!(
                run.timers.len() <= 256
                    && run.timers.iter().all(|t| t.remaining.is_finite()
                        && t.remaining >= 0.
                        && t.output.port == 0
                        && graph.node(t.output.node).is_ok_and(|n| n.kind == K::Delay)
                        && graph.node(t.context.event).is_ok_and(|n| n.kind.event())
                        && t.context.normal.iter().all(|v| v.is_finite())
                        && t.context.impulse.is_finite()),
                "invalid saved timer"
            );
            ensure!(
                run.results.len() <= 128
                    && run
                        .results
                        .iter()
                        .all(
                            |(id, vs)| graph.node(*id).is_ok_and(|n| n.output_pins().len()
                                == vs.len() + 1
                                && n.output_pins()[1..]
                                    .iter()
                                    .zip(vs)
                                    .all(|((_, t), v)| *t == v.kind() && v.valid()))
                        ),
                "invalid saved action results"
            );
            ensure!(
                run.grounded.len() <= 128
                    && run.spawned.len() <= 128
                    && run.overlap.len() <= 100_000
                    && run.collisions.len() <= 100_000,
                "saved graph state exceeds limits"
            );
            run.program = Some(Arc::new(Program::new(graph)));
            ensure!(
                runs.insert(key, run).is_none(),
                "duplicate saved attachment"
            );
        }
        Ok(Self {
            runs,
            elapsed: save.elapsed,
            object_boards: save.object_boards,
            scene_board: save.scene_board,
            initialized: true,
            ..Self::default()
        })
    }
}
fn validate_board_shape(expected: &Blackboard, actual: &Blackboard) -> Result<()> {
    ensure!(
        expected.len() == actual.len()
            && expected
                .iter()
                .all(|(k, v)| actual.get(k).is_some_and(|a| match (v, a) {
                    (B::Scalar(v), B::Scalar(a)) => v.kind() == a.kind(),
                    (
                        B::List {
                            element, capacity, ..
                        },
                        B::List {
                            element: e,
                            capacity: c,
                            ..
                        },
                    ) => element == e && capacity == c,
                    _ => false,
                })),
        "saved blackboard schema mismatch"
    );
    Ok(())
}

impl BlueprintRuntime {
    pub(crate) fn add_scene_defaults(&mut self, defaults: &Blackboard) {
        for (k, v) in defaults {
            self.scene_board
                .entry(k.clone())
                .or_insert_with(|| v.clone());
        }
    }
}

impl SceneInstance {
    pub fn destroy_prefab(&mut self, world: &mut World, target: &str) -> Result<()> {
        let ids: Vec<_> = self
            .document
            .prefabs
            .values()
            .find(|p| p.members.values().any(|id| id == target))
            .context("Destroy Prefab target is not a live prefab instance")?
            .members
            .values()
            .cloned()
            .collect();
        ensure!(
            !self.document.views.values().any(|id| ids.contains(id)),
            "cannot destroy an active camera; switch views first"
        );
        let mut runtime = world
            .remove_resource::<BlueprintRuntime>()
            .unwrap_or_default();
        runtime.query_budget = 1_000_000;
        self.prepare_blueprints(&mut runtime);
        let mut budget = 100_000;
        let result = (|| -> Result<()> {
            for owner in &ids {
                self.destroy_blueprint_events(
                    world,
                    &mut runtime,
                    owner,
                    GameplayInput::default(),
                    0.,
                    &mut budget,
                )?;
            }
            self.destroy_prefab_raw(world, target)?;
            runtime
                .runs
                .retain(|(id, _), _| self.entities.contains_key(id));
            runtime
                .object_boards
                .retain(|id, _| self.entities.contains_key(id));
            Ok(())
        })();
        world.insert_resource(runtime);
        result
    }
    pub(crate) fn scene_destroy_events(&mut self, world: &mut World) -> Result<()> {
        let mut runtime = world
            .remove_resource::<BlueprintRuntime>()
            .unwrap_or_else(|| BlueprintRuntime {
                query_budget: 1_000_000,
                ..Default::default()
            });
        self.prepare_blueprints(&mut runtime);
        let owners: Vec<_> = self
            .document
            .objects
            .iter()
            .filter(|o| !o.blueprints.is_empty())
            .map(|o| o.id.clone())
            .collect();
        let mut budget = 100_000;
        let result = (|| -> Result<()> {
            for owner in owners {
                self.destroy_blueprint_events(
                    world,
                    &mut runtime,
                    &owner,
                    GameplayInput::default(),
                    0.,
                    &mut budget,
                )?;
            }
            Ok(())
        })();
        world.insert_resource(runtime);
        result
    }
}
