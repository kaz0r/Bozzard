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
    #[serde(default)]
    wall_clock: bool,
    event: u32,
    other: Option<String>,
    normal: [f32; 3],
    impulse: f32,
    #[serde(default)]
    message: String,
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
    attachment: usize,
    #[serde(skip)]
    last_node: Option<u32>,
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
            K::TimelineEvent
            | K::AnimationEvent
            | K::NavigationEvent
            | K::SpriteEvent
            | K::UiEvent
                if socket.port == 1 =>
            {
                Value::Text(if self.context.event == id {
                    self.context.message.clone()
                } else {
                    String::new()
                })
            }
            K::TimelineEvent
            | K::AnimationEvent
            | K::NavigationEvent
            | K::SpriteEvent
            | K::UiEvent
                if socket.port == 2 =>
            {
                Value::Number(if self.context.event == id {
                    self.context.impulse
                } else {
                    0.
                })
            }
            K::UiValue => {
                use crate::middleware::ui::{Runtime, Widget};
                let owner =
                    reference_id(v[0].object()?, self.owner).context("UI target is None")?;
                let entity = self.entities.get(owner).context("UI target missing")?;
                let widget = self
                    .world
                    .get::<Widget>(*entity)
                    .context("target has no UI Widget")?;
                Value::Number(
                    self.world
                        .resource::<Runtime>()
                        .and_then(|r| r.widgets.get(owner))
                        .and_then(|s| s.value)
                        .unwrap_or(widget.value),
                )
            }
            K::UiReducedMotion => Value::Bool(
                self.world
                    .resource::<crate::middleware::ui::Preferences>()
                    .and_then(|p| p.reduced_motion)
                    .unwrap_or_else(|| {
                        self.world
                            .query::<crate::middleware::ui::Canvas>()
                            .any(|(_, c)| c.enabled && c.reduced_motion)
                    }),
            ),
            K::SpriteFrame => {
                use crate::middleware::sprite::{Runtime, Sprite};
                let owner =
                    reference_id(v[0].object()?, self.owner).context("sprite target is None")?;
                let entity = self.entities.get(owner).context("sprite target missing")?;
                let sprite = self
                    .world
                    .get::<Sprite>(*entity)
                    .context("target has no Sprite")?;
                Value::Number(
                    self.world
                        .resource::<Runtime>()
                        .and_then(|r| r.players.get(owner))
                        .map_or(sprite.frame, |r| r.frame) as f32,
                )
            }
            K::GetTile => {
                let owner =
                    reference_id(v[2].object()?, self.owner).context("tilemap target is None")?;
                let entity = self.entities.get(owner).context("tilemap target missing")?;
                let map = self
                    .world
                    .get::<crate::middleware::sprite::Tilemap>(*entity)
                    .context("target has no Tilemap")?;
                let x = list_index(v[0].number()?)?;
                let y = list_index(v[1].number()?)?;
                ensure!(
                    x < map.dimensions[0] as usize && y < map.dimensions[1] as usize,
                    "tile coordinate outside map"
                );
                Value::Number(map.cells[y * map.dimensions[0] as usize + x] as f32)
            }
            K::NavigationEvent if socket.port == 3 => Value::Object(if self.context.event == id {
                self.context
                    .other
                    .clone()
                    .map_or(ObjectRef::None, ObjectRef::Id)
            } else {
                ObjectRef::None
            }),
            K::NavState | K::NavHasPath | K::NavSeesTarget | K::NavVelocity => {
                use crate::middleware::navigation::{NavAgent, Runtime};
                let owner = reference_id(v[0].object()?, self.owner)
                    .context("navigation target is None")?;
                let entity = self
                    .entities
                    .get(owner)
                    .context("navigation target missing")?;
                let agent = self
                    .world
                    .get::<NavAgent>(*entity)
                    .context("target has no Navigation Agent")?;
                let run = self
                    .world
                    .resource::<Runtime>()
                    .and_then(|r| r.agents.get(owner));
                match n.kind {
                    K::NavState => Value::Text(run.map_or_else(
                        || agent.initial.clone(),
                        |r| agent.states[r.state].name.clone(),
                    )),
                    K::NavHasPath => Value::Bool(run.is_some_and(|r| {
                        !r.halted && !r.arrived && !r.blocked && r.cursor < r.path.len()
                    })),
                    K::NavSeesTarget => Value::Bool(run.is_some_and(|r| r.sees_target)),
                    _ => Value::Vector(run.map_or([0.; 3], |r| r.velocity)),
                }
            }
            K::AudioPosition | K::AudioPlaying => {
                use crate::middleware::audio::{AudioSource, Runtime, Transport};
                let owner =
                    reference_id(v[0].object()?, self.owner).context("audio target is None")?;
                let entity = self
                    .entities
                    .get(owner)
                    .context("audio target is missing")?;
                let source = self
                    .world
                    .get::<AudioSource>(*entity)
                    .context("target has no Audio Source")?;
                let voice = self
                    .world
                    .resource::<Runtime>()
                    .and_then(|r| r.voices.get(owner));
                if n.kind == K::AudioPosition {
                    Value::Number(voice.map_or(0., |v| v.position as f32))
                } else {
                    Value::Bool(
                        source.enabled
                            && voice.map_or(source.autoplay && !source.asset.is_empty(), |v| {
                                v.transport == Transport::Playing
                            }),
                    )
                }
            }
            K::AnimationProgress | K::AnimationState => {
                use crate::middleware::animation::{Animator, Runtime};
                let id =
                    reference_id(v[0].object()?, self.owner).context("animation target is None")?;
                let entity = self
                    .entities
                    .get(id)
                    .context("animation target is missing")?;
                let animator = self
                    .world
                    .get::<Animator>(*entity)
                    .context("target has no Animator")?;
                let player = self
                    .world
                    .resource::<Runtime>()
                    .and_then(|r| r.players.get(id));
                let state = player.and_then(|p| animator.states.get(p.state));
                if n.kind == K::AnimationState {
                    Value::Text(state.map_or_else(|| animator.initial.clone(), |s| s.name.clone()))
                } else {
                    Value::Number(
                        player
                            .zip(state)
                            .map_or(0., |(p, s)| p.clock.position(1., s.repeat)),
                    )
                }
            }
            K::TimelineProgress => {
                use crate::middleware::timeline::{Runtime, Timeline};
                let id =
                    reference_id(v[0].object()?, self.owner).context("timeline target is None")?;
                let entity = self
                    .entities
                    .get(id)
                    .context("timeline target is missing")?;
                let timeline = self
                    .world
                    .get::<Timeline>(*entity)
                    .context("target has no Timeline")?;
                Value::Number(
                    self.world
                        .resource::<Runtime>()
                        .and_then(|r| r.players.get(id))
                        .map_or(0., |s| {
                            s.clock
                                .position(timeline.motion.duration, timeline.motion.repeat)
                                / timeline.motion.duration
                        }),
                )
            }
            K::TweenProgress | K::SampleCurve => {
                use crate::middleware::tween::{Runtime, Tween};
                let port = n.kind.target_port().unwrap();
                let id =
                    reference_id(v[port].object()?, self.owner).context("motion target is None")?;
                let entity = self.entities.get(id).context("motion target is missing")?;
                let tween = self
                    .world
                    .get::<Tween>(*entity)
                    .context("target has no Tween")?;
                Value::Number(if n.kind == K::TweenProgress {
                    self.world
                        .resource::<Runtime>()
                        .and_then(|r| r.players.get(id))
                        .map_or(0., |s| {
                            s.clock.position(tween.duration, tween.repeat) / tween.duration
                        })
                } else {
                    let track = v[1].number()?;
                    let channel = v[2].number()?;
                    ensure!(
                        track >= 0.
                            && track.fract() == 0.
                            && channel >= 0.
                            && channel.fract() == 0.,
                        "curve indices must be nonnegative integers"
                    );
                    tween
                        .tracks
                        .get(track as usize)
                        .and_then(|t| t.channels.get(channel as usize))
                        .context("motion curve index out of range")?
                        .sample(v[0].number()?)
                })
            }
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
        runtime.initialize_boards(&self.document);
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
                run.attachment = index;
            }
        }
    }
    pub fn step_blueprints(
        &mut self,
        world: &mut World,
        dt: f32,
        input: GameplayInput,
    ) -> Result<()> {
        self.step_blueprints_inner(world, dt, input, false)
    }
    /// Dispatch native UI actions immediately without ticking gameplay or its timers.
    pub fn dispatch_ui_blueprints(&mut self, world: &mut World) -> Result<()> {
        self.step_blueprints_inner(world, 0., GameplayInput::default(), true)
    }
    fn step_blueprints_inner(
        &mut self,
        world: &mut World,
        dt: f32,
        input: GameplayInput,
        ui_dispatch: bool,
    ) -> Result<()> {
        let ui_only = ui_dispatch || !crate::game_flow::simulation_running(world);
        if !self.has_blueprints() {
            if let Some(signals) = world.resource_mut::<crate::middleware::signals::Signals>() {
                signals.begin(crate::middleware::signals::Kind::Ui);
            }
            return self.apply_scene_controls(world);
        }
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
        let mut runtime = world
            .remove_resource::<BlueprintRuntime>()
            .unwrap_or_default();
        self.prepare_blueprints(&mut runtime);
        runtime.query_budget = 1_000_000;
        runtime.stats.query_geometry_builds = 0;
        runtime.stats.actions = 0;
        let result = (|| -> Result<()> {
            if !ui_only {
                runtime.elapsed += dt;
            }
            ensure!(runtime.elapsed.is_finite(), "blueprint clock overflow");
            let query_overlaps = !ui_only
                && self
                    .document
                    .objects
                    .iter()
                    .flat_map(|o| &o.blueprints)
                    .filter(|b| b.enabled)
                    .any(|b| needs_overlap(&b.graph));
            let query_solids = !ui_only
                && self
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
                            layers: collider.layers,
                            mask: collider.mask,
                        };
                        let meets = |other_layers: u32, other_mask: u32| {
                            layers_interact(volume.layers, volume.mask, other_layers, other_mask)
                        };
                        for body in &collisions.boxes {
                            ensure!(
                                overlap_budget > 0,
                                "blueprint overlap budget exceeded (1000000 tests/tick)"
                            );
                            overlap_budget -= 1;
                            if body.id != object.id
                                && meets(body.layers, body.mask)
                                && volume.intersects(body)
                            {
                                overlap.insert(body.id.clone());
                            }
                        }
                        for mesh in &collisions.meshes {
                            ensure!(
                                overlap_budget > 0,
                                "blueprint overlap budget exceeded (1000000 tests/tick)"
                            );
                            overlap_budget -= 1;
                            if mesh.id != object.id
                                && meets(mesh.layers, mesh.mask)
                                && mesh.intersects(&volume)
                            {
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
                    run.last_node = None;
                    let program = run.program.clone().unwrap();
                    let step =
                        (|| -> Result<()> {
                            let mut events = Vec::new();
                            if enabled && !ui_dispatch {
                                for timer in &mut run.timers {
                                    if !ui_only || timer.context.wall_clock {
                                        timer.remaining -= dt;
                                    }
                                }
                                for timer in &run.timers {
                                    if timer.remaining <= 0. {
                                        events.push((timer.output, timer.context.clone()));
                                    }
                                }
                                run.timers.retain(|t| t.remaining > 0.);
                            } else if !enabled {
                                run.timers.clear();
                            }
                            for event in program.graph.nodes.iter().filter(|n| {
                                n.kind.event()
                                    && (!ui_dispatch || n.kind == K::UiEvent)
                                    && (!ui_only || matches!(n.kind, K::UiEvent | K::AudioFinished))
                            }) {
                                let fire = match event.kind {
                                    K::Enable => enabled && !run.enabled,
                                    K::Disable => !enabled && run.enabled,
                                    K::Start => enabled && !run.started,
                                    K::AudioFinished => {
                                        enabled
                                            && world
                                                .resource::<crate::middleware::audio::Runtime>()
                                                .is_some_and(|r| r.finished.contains(&owner))
                                    }
                                    K::TweenFinished => {
                                        enabled
                                            && world
                                                .resource::<crate::middleware::tween::Runtime>()
                                                .is_some_and(|r| r.finished.contains(&owner))
                                    }
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
                                    K::TimelineEvent
                                    | K::AnimationEvent
                                    | K::NavigationEvent
                                    | K::UiEvent
                                    | K::SpriteEvent
                                        if enabled =>
                                    {
                                        use crate::middleware::signals::{Kind, Signals};
                                        if let Some(signals) = world.resource::<Signals>() {
                                            contexts.extend(
                                                signals
                                                    .for_owner(
                                                        &owner,
                                                        if event.kind == K::TimelineEvent {
                                                            Kind::Timeline
                                                        } else if event.kind == K::NavigationEvent {
                                                            Kind::Navigation
                                                        } else if event.kind == K::UiEvent {
                                                            Kind::Ui
                                                        } else if event.kind == K::SpriteEvent {
                                                            Kind::Sprite
                                                        } else {
                                                            Kind::Animation
                                                        },
                                                    )
                                                    .map(|signal| EventContext {
                                                        wall_clock: event.kind == K::UiEvent,
                                                        event: event.id,
                                                        message: signal.name.clone(),
                                                        impulse: signal.value,
                                                        other: signal.other.clone(),
                                                        ..Default::default()
                                                    }),
                                            );
                                        }
                                    }
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
                                                ..Default::default()
                                            }),
                                    ),
                                    _ if fire => contexts.push(EventContext {
                                        wall_clock: event.kind == K::AudioFinished,
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
                            if !ui_only {
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
                            }
                            Ok(())
                        })();
                    if let Err(error) = &step {
                        bozzard_diagnostics::log(
                            world,
                            bozzard_diagnostics::Level::Error,
                            "Blueprint",
                            &format!("{error:#}"),
                            bozzard_diagnostics::Location {
                                object: Some(owner.clone()),
                                attachment: Some(index),
                                node: run.last_node,
                                asset: None,
                                ..Default::default()
                            },
                        );
                    }
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
        if let Some(signals) = world.resource_mut::<crate::middleware::signals::Signals>() {
            signals.begin(crate::middleware::signals::Kind::Ui);
        }
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
                    && (!context.wall_clock && !crate::game_flow::simulation_running(world)
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
                run.last_node = Some(id);
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
                                    u32::MAX,
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
                                            u32::MAX,
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
                                        u32::MAX,
                                        capacity,
                                        &mut runtime.query_budget,
                                    )?
                                } else {
                                    query.overlap_box_budget(
                                        origin,
                                        Vec3::from(inputs[1].vector()?),
                                        ignore,
                                        u32::MAX,
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
                    K::SetUiText
                    | K::SetUiValue
                    | K::SetUiVisible
                    | K::SetUiEnabled
                    | K::FocusUi => {
                        use crate::middleware::ui::Control;
                        let control = match node.kind {
                            K::SetUiText => Control::Text(value.text()?.into()),
                            K::SetUiValue => Control::Value(value.number()?),
                            K::SetUiVisible => Control::Visible(value.boolean()?),
                            K::SetUiEnabled => Control::Enabled(value.boolean()?),
                            _ => Control::Focus,
                        };
                        self.control_ui(world, &target, control)?;
                    }
                    K::SetUiLanguage => self.set_ui_language(world, value.text()?)?,
                    K::SetUiTextScale | K::SetUiContrast | K::SetUiReducedMotion => {
                        use crate::middleware::ui::Preferences;
                        let scale = if node.kind == K::SetUiTextScale {
                            let s = value.number()?;
                            ensure!(
                                s.is_finite() && (1.0..=3.).contains(&s),
                                "UI text scale outside 1–3"
                            );
                            Some(s)
                        } else {
                            None
                        };
                        let enabled = if scale.is_none() {
                            Some(value.boolean()?)
                        } else {
                            None
                        };
                        if world.resource::<Preferences>().is_none() {
                            world.insert_resource(Preferences::default());
                        }
                        let preferences = world.resource_mut::<Preferences>().unwrap();
                        match node.kind {
                            K::SetUiTextScale => preferences.text_scale = scale,
                            K::SetUiContrast => preferences.high_contrast = enabled,
                            _ => preferences.reduced_motion = enabled,
                        }
                    }
                    K::StartGame | K::PauseGame | K::ResumeGame | K::RestartGame | K::QuitGame => {
                        use crate::game_flow::GamePhase as P;
                        if node.kind == K::RestartGame {
                            self.request_scene_control(world, K::RestartScene, "")?;
                        } else {
                            let session = world
                                .resource_mut::<crate::GameSession>()
                                .context("game flow is not enabled")?;
                            match (node.kind, session.phase) {
                                (K::StartGame, P::Ready) | (K::ResumeGame, P::Paused) => {
                                    session.phase = P::Playing
                                }
                                (K::PauseGame, P::Playing) => session.phase = P::Paused,
                                (K::QuitGame, _) => session.phase = P::Quit,
                                _ => {}
                            }
                        }
                    }
                    K::PlaySprite | K::PauseSprite | K::StopSprite | K::SetSpriteFrame => {
                        use crate::middleware::sprite::Control;
                        let control = match node.kind {
                            K::PlaySprite => Control::Play {
                                clip: value.text()?.into(),
                                restart: eval.input(node, 2)?.boolean()?,
                            },
                            K::PauseSprite => Control::Pause,
                            K::StopSprite => Control::Stop,
                            _ => Control::Frame(u32::try_from(list_index(value.number()?)?)?),
                        };
                        self.control_sprite(world, &target, control)?;
                    }
                    K::SetTile => {
                        let x = u32::try_from(list_index(value.number()?)?)?;
                        let y = u32::try_from(list_index(eval.input(node, 2)?.number()?)?)?;
                        let tile = u32::try_from(list_index(eval.input(node, 3)?.number()?)?)?;
                        self.set_tile(world, &target, x, y, tile)?;
                    }
                    K::SetNavDestination | K::SetNavState | K::SetNavTarget | K::StopNavigation => {
                        use crate::middleware::navigation::Control;
                        let control = match node.kind {
                            K::SetNavDestination => Control::Destination(value.vector()?),
                            K::SetNavState => Control::State(value.text()?.into()),
                            K::SetNavTarget => Control::Target(
                                reference_id(value.object()?, owner).map(str::to_owned),
                            ),
                            _ => Control::Stop,
                        };
                        self.control_navigation(world, &target, control)?;
                    }
                    K::PlayAudio
                    | K::PauseAudio
                    | K::StopAudio
                    | K::SeekAudio
                    | K::SetAudioVolume
                    | K::SetAudioPitch
                    | K::SetAudioPan => {
                        use crate::middleware::audio::Control;
                        let control = match node.kind {
                            K::PlayAudio => Control::Play {
                                restart: value.boolean()?,
                            },
                            K::PauseAudio => Control::Pause,
                            K::StopAudio => Control::Stop,
                            K::SeekAudio => Control::Seek(f64::from(value.number()?)),
                            K::SetAudioVolume => Control::Volume(value.number()?),
                            K::SetAudioPitch => Control::Pitch(value.number()?),
                            _ => Control::Pan(value.number()?),
                        };
                        self.control_audio(world, &target, control)?;
                    }
                    K::SetAudioBusVolume => {
                        let volume = eval.input(node, 2)?.number()?;
                        self.set_audio_bus_volume(world, value.text()?, volume)?;
                    }
                    K::PlayAnimation
                    | K::PauseAnimation
                    | K::StopAnimation
                    | K::SeekAnimation
                    | K::SetAnimationParameter => {
                        use crate::middleware::animation::Control;
                        let control = match node.kind {
                            K::PlayAnimation => Control::Play {
                                state: value.text()?.into(),
                                fade: eval.input(node, 2)?.number()?,
                            },
                            K::PauseAnimation => Control::Pause,
                            K::StopAnimation => Control::Stop,
                            K::SeekAnimation => Control::Seek(value.number()?),
                            _ => Control::Parameter {
                                name: value.text()?.into(),
                                value: eval.input(node, 2)?.number()?,
                            },
                        };
                        self.control_animation(world, &target, control)?;
                    }
                    K::PlayTween
                    | K::PauseTween
                    | K::StopTween
                    | K::SeekTween
                    | K::PlayTimeline
                    | K::PauseTimeline
                    | K::StopTimeline
                    | K::SeekTimeline => {
                        use crate::middleware::tween::Control;
                        let control = match node.kind {
                            K::PlayTween | K::PlayTimeline => Control::Play {
                                restart: value.boolean()?,
                            },
                            K::PauseTween | K::PauseTimeline => Control::Pause,
                            K::StopTween | K::StopTimeline => Control::Stop,
                            _ => Control::Seek(value.number()?),
                        };
                        let transport = if matches!(
                            node.kind,
                            K::PlayTween | K::PauseTween | K::StopTween | K::SeekTween
                        ) {
                            Self::control_tween
                        } else {
                            Self::control_timeline
                        };
                        transport(self, world, &target, control)?;
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
                        let has_text =
                            if let Some(mut text) = world.get_mut::<TextRendering>(entity) {
                                text.color[..3].copy_from_slice(&color);
                                true
                            } else {
                                false
                            };
                        if let Some(mut material) = world.get_mut::<Material>(entity) {
                            material.color = color;
                        } else if let Some(mut drawable) = world.get_mut::<Drawable>(entity) {
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
                    K::Print | K::LogInfo | K::LogWarning | K::LogError => {
                        use bozzard_diagnostics::Level;
                        let message = if node.kind == K::Print {
                            format!("{} / {}: {}", owner, program.graph.name, value.number()?)
                        } else {
                            value.text()?.to_owned()
                        };
                        bozzard_diagnostics::log(
                            world,
                            match node.kind {
                                K::LogWarning => Level::Warning,
                                K::LogError => Level::Error,
                                _ => Level::Info,
                            },
                            "Blueprint",
                            &message,
                            bozzard_diagnostics::Location {
                                object: Some(owner.into()),
                                attachment: Some(run.attachment),
                                node: Some(id),
                                asset: None,
                                ..Default::default()
                            },
                        );
                        runtime.messages.push_back(message);
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
                        && graph.node(t.context.event).is_ok_and(|n| n.kind.event()
                            && t.context.wall_clock
                                == matches!(n.kind, K::UiEvent | K::AudioFinished))
                        && t.context.normal.iter().all(|v| v.is_finite())
                        && t.context.impulse.is_finite()
                        && t.context.message.len() <= 256),
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
    /// Seed the boards from the document, once, for whichever gameplay step runs first.
    ///
    /// Scripts and graphs share these boards and a script-only scene never runs the blueprint pass,
    /// so the script step seeds them too. Both steps must set the same "seeded" flag: the blueprint
    /// pass resets the scene board while the flag is clear, which would throw away a script write
    /// made earlier in the same tick.
    pub(crate) fn initialize_boards(&mut self, scene: &Scene) {
        if self.initialized {
            return;
        }
        self.scene_board = scene.blackboard.clone();
        for object in &scene.objects {
            if object.blackboard.is_empty() {
                continue;
            }
            self.object_boards
                .entry(object.id.clone())
                .or_insert_with(|| object.blackboard.clone());
        }
        self.initialized = true;
    }
    /// Scripts share these boards with graphs, so an object that declares variables gets one even
    /// when it carries no graph at all.
    pub(crate) fn add_object_defaults(&mut self, owner: &str, defaults: &Blackboard) {
        let board = self.object_boards.entry(owner.to_owned()).or_default();
        for (name, value) in defaults {
            board.entry(name.clone()).or_insert_with(|| value.clone());
        }
    }
    /// Write one scalar through the same type check a `Set Variable` node performs.
    pub(crate) fn set_board_scalar(
        &mut self,
        scope: Scope,
        owner: &str,
        name: &str,
        value: Value,
    ) -> Result<()> {
        let board = match scope {
            Scope::Object => self
                .object_boards
                .get_mut(owner)
                .context("missing object board")?,
            Scope::Scene => &mut self.scene_board,
            Scope::Graph => anyhow::bail!("a graph-scoped variable needs a graph"),
        };
        let entry = board
            .get_mut(name)
            .with_context(|| format!("unknown {scope:?} variable '{name}'"))?;
        ensure!(
            matches!(entry, B::Scalar(old) if old.kind() == value.kind()),
            "variable '{name}' type mismatch"
        );
        *entry = B::Scalar(value);
        Ok(())
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
