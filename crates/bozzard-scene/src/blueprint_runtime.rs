//! Bounded fixed-step graph interpreter shared by editor Play, native player and server.
use super::*;
use blueprint::{Blueprint, InputKey, Node, NodeKind as K, Socket, Value};

#[derive(Clone, Default)]
struct Run {
    started: bool,
    overlap: bool,
    held: [bool; 5],
    variables: BTreeMap<String, f32>,
}
#[derive(Clone, Default)]
pub struct BlueprintRuntime {
    runs: BTreeMap<(String, usize), Run>,
    pub messages: VecDeque<String>,
    elapsed: f32,
}
#[derive(Clone, Copy)]
pub struct BlueprintHidden(pub bool);

struct Eval<'a> {
    graph: &'a Blueprint,
    variables: &'a BTreeMap<String, f32>,
    transform: Transform,
    input: GameplayInput,
    dt: f32,
    elapsed: f32,
    cache: BTreeMap<u32, Value>,
}
impl Eval<'_> {
    fn input(&mut self, node: &Node, port: usize) -> Result<Value> {
        if let Some(w) = self.graph.wires.iter().find(|w| {
            w.to == (Socket {
                node: node.id,
                port,
            })
        }) {
            self.output(w.from.node)
        } else {
            Ok(node.inputs[port].clone())
        }
    }
    fn output(&mut self, id: u32) -> Result<Value> {
        if let Some(value) = self.cache.get(&id) {
            return Ok(value.clone());
        }
        let n = self.graph.node(id)?;
        let v: Vec<_> = (0..n.inputs.len())
            .map(|p| self.input(n, p))
            .collect::<Result<_>>()?;
        let value = match n.kind {
            K::Number | K::Boolean | K::Vector => v[0].clone(),
            K::DeltaTime => Value::Number(self.dt),
            K::ElapsedTime => Value::Number(self.elapsed),
            K::Position => Value::Vector(self.transform.translation),
            K::Rotation => Value::Vector(self.transform.rotation_degrees),
            K::Scale => Value::Vector(self.transform.scale),
            K::InputHeld => Value::Bool(n.key.active(self.input)),
            K::MoveX => Value::Number(self.input.movement[0]),
            K::MoveY => Value::Number(self.input.movement[1]),
            K::GetVariable => Value::Number(self.variables[&n.variable]),
            K::Add => Value::Number(v[0].number()? + v[1].number()?),
            K::Subtract => Value::Number(v[0].number()? - v[1].number()?),
            K::Multiply => Value::Number(v[0].number()? * v[1].number()?),
            K::Divide => {
                ensure!(v[1].number()? != 0., "division by zero at node {id}");
                Value::Number(v[0].number()? / v[1].number()?)
            }
            K::Sine => Value::Number(v[0].number()?.sin()),
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
        ensure!(value.valid(), "non-finite output at node {id}");
        self.cache.insert(id, value.clone());
        Ok(value)
    }
}
impl SceneInstance {
    pub fn has_blueprints(&self) -> bool {
        self.document
            .objects
            .iter()
            .any(|o| o.blueprints.iter().any(|b| b.enabled))
    }
    pub fn step_blueprints(&self, world: &mut World, dt: f32, input: GameplayInput) -> Result<()> {
        if !self.has_blueprints() {
            return Ok(());
        }
        ensure!(
            dt.is_finite() && dt > 0. && input.movement.iter().all(|v| v.is_finite()),
            "invalid blueprint timestep/input"
        );
        let mut runtime = world
            .resource_mut::<BlueprintRuntime>()
            .map(std::mem::take)
            .unwrap_or_default();
        let result = (|| -> Result<()> {
            runtime.elapsed += dt;
            ensure!(runtime.elapsed.is_finite(), "blueprint clock overflow");
            let needs_overlap = self
                .document
                .objects
                .iter()
                .flat_map(|o| &o.blueprints)
                .filter(|b| b.enabled)
                .flat_map(|b| &b.graph.nodes)
                .any(|n| matches!(n.kind, K::TriggerEnter | K::TriggerExit));
            let collisions = needs_overlap.then(|| self.collisions(world)).transpose()?;
            let matrices = needs_overlap
                .then(|| self.global_transforms(world))
                .transpose()?;
            // ponytail: bounded linear graph lookups (128 nodes); compile indices if profiling warrants it.
            let mut budget = 100_000usize;
            for object in &self.document.objects {
                let entity = self.entities[&object.id];
                let overlap = if let (Some(collisions), Some(matrices)) = (&collisions, &matrices) {
                    let collider = world
                        .get::<Trigger>(entity)
                        .map(|t| t.volume)
                        .or_else(|| world.get::<BoxCollider>(entity).copied());
                    if let Some(collider) = collider.filter(|c| c.enabled) {
                        let (center, edges, corners) = collider.geometry(matrices[&object.id])?;
                        let volume = CollisionBox {
                            id: object.id.clone(),
                            entity,
                            center,
                            edges,
                            corners,
                        };
                        collisions
                            .boxes
                            .iter()
                            .any(|b| b.id != object.id && volume.intersects(b))
                    } else {
                        false
                    }
                } else {
                    false
                };
                for (index, attachment) in object
                    .blueprints
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.enabled)
                {
                    let graph = &attachment.graph;
                    let run = runtime.runs.entry((object.id.clone(), index)).or_default();
                    if !run.started {
                        run.variables = graph.variables.clone();
                    }
                    for event in graph.nodes.iter().filter(|n| n.kind.event()) {
                        let key_index = InputKey::ALL.iter().position(|k| *k == event.key).unwrap();
                        let fire = match event.kind {
                            K::Start => !run.started,
                            K::Update => true,
                            K::InputPressed => {
                                event.key.active(input)
                                    && (event.key == InputKey::Jump || !run.held[key_index])
                            }
                            K::TriggerEnter => overlap && !run.overlap,
                            K::TriggerExit => !overlap && run.overlap,
                            _ => false,
                        };
                        if !fire {
                            continue;
                        }
                        let mut queue = VecDeque::from([Socket {
                            node: event.id,
                            port: 0,
                        }]);
                        while let Some(output) = queue.pop_front() {
                            for wire in graph.wires.iter().filter(|w| w.from == output) {
                                ensure!(
                                    budget > 0,
                                    "blueprint execution budget exceeded (100000 actions/tick)"
                                );
                                budget -= 1;
                                let node = graph.node(wire.to.node)?;
                                let transform = *world
                                    .get::<Transform>(entity)
                                    .context("blueprint owner was removed")?;
                                let mut eval = Eval {
                                    graph,
                                    variables: &run.variables,
                                    transform,
                                    input,
                                    dt,
                                    elapsed: runtime.elapsed,
                                    cache: BTreeMap::new(),
                                };
                                let value = eval.input(node, 1).with_context(|| {
                                    format!(
                                        "blueprint '{}' on '{}', node {}",
                                        graph.name, object.id, node.id
                                    )
                                })?;
                                let mut port = 0;
                                match node.kind {
                                    K::Branch => port = usize::from(!value.boolean()?),
                                    K::SetVariable => {
                                        run.variables
                                            .insert(node.variable.clone(), value.number()?);
                                    }
                                    K::Translate
                                    | K::Rotate
                                    | K::SetPosition
                                    | K::SetRotation
                                    | K::SetScale => {
                                        let mut next = transform;
                                        let v = value.vector()?;
                                        match node.kind {
                                            K::Translate => {
                                                next.translation = (Vec3::from(next.translation)
                                                    + Vec3::from(v))
                                                .to_array()
                                            }
                                            K::Rotate => {
                                                next.rotation_degrees =
                                                    (Vec3::from(next.rotation_degrees)
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
                                        if let Err(error) = self.global_transforms(world) {
                                            world.insert(entity, transform)?;
                                            return Err(error);
                                        }
                                    }
                                    K::SetColor => {
                                        let color = value.vector()?;
                                        ensure!(
                                            color.iter().all(|c| (0.0..=1.0).contains(c)),
                                            "blueprint RGB must be in 0..1"
                                        );
                                        world
                                            .get_mut::<Drawable>(entity)
                                            .context("Set Color needs a Mesh Renderer")?
                                            .color = color;
                                    }
                                    K::SetVisible => {
                                        world.insert(entity, BlueprintHidden(!value.boolean()?))?;
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
                                        self.move_box(
                                            world,
                                            &object.id,
                                            Vec3::from(value.vector()?),
                                        )?;
                                    }
                                    K::Jump => {
                                        self.jump_box(world, &object.id, value.number()?)?;
                                    }
                                    K::Print => {
                                        runtime.messages.push_back(format!(
                                            "{} / {}: {}",
                                            object.name,
                                            graph.name,
                                            value.number()?
                                        ));
                                        while runtime.messages.len() > 64 {
                                            runtime.messages.pop_front();
                                        }
                                    }
                                    _ => anyhow::bail!("invalid execution node"),
                                }
                                queue.push_back(Socket {
                                    node: node.id,
                                    port,
                                });
                            }
                        }
                    }
                    run.started = true;
                    run.overlap = overlap;
                    run.held = InputKey::ALL.map(|key| key.active(input));
                }
            }
            Ok(())
        })();
        world.insert_resource(runtime);
        result
    }
}
