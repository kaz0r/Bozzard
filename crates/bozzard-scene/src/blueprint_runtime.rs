//! Bounded fixed-step graph interpreter shared by editor Play, native player and server.
use super::*;
use blueprint::{Blueprint, InputKey, Node, NodeKind as K, ObjectRef, Socket, Value};
use std::collections::BTreeSet;

#[derive(Clone, Default)]
struct Run {
    started: bool,
    overlap: BTreeSet<String>,
    held: [bool; 5],
    variables: BTreeMap<String, f32>,
    spawned: BTreeMap<u32, ObjectRef>,
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
    spawned: &'a BTreeMap<u32, ObjectRef>,
    world: &'a World,
    entities: &'a BTreeMap<String, Entity>,
    owner: &'a str,
    other: Option<(u32, &'a str)>,
    overlap_count: usize,
    input: GameplayInput,
    dt: f32,
    elapsed: f32,
    cache: BTreeMap<Socket, Value>,
}
impl Eval<'_> {
    fn input(&mut self, node: &Node, port: usize) -> Result<Value> {
        if let Some(w) = self.graph.wires.iter().find(|w| {
            w.to == (Socket {
                node: node.id,
                port,
            })
        }) {
            self.output(w.from)
        } else {
            Ok(node.inputs[port].clone())
        }
    }
    fn output(&mut self, socket: Socket) -> Result<Value> {
        let id = socket.node;
        if let Some(value) = self.cache.get(&socket) {
            return Ok(value.clone());
        }
        let n = self.graph.node(id)?;
        if n.kind == K::SpawnPrefab && socket.port == 1 {
            return Ok(Value::Object(
                self.spawned.get(&id).cloned().unwrap_or(ObjectRef::None),
            ));
        }
        let v: Vec<_> = (0..n.inputs.len())
            .map(|p| self.input(n, p))
            .collect::<Result<_>>()?;
        let value = match n.kind {
            K::Number | K::Boolean | K::Vector | K::Object => v[0].clone(),
            K::SelfObject => Value::Object(ObjectRef::Id(self.owner.into())),
            K::BodyEnter | K::BodyExit if socket.port == 1 => Value::Object(
                self.other
                    .filter(|(event, _)| *event == id)
                    .map_or(ObjectRef::None, |(_, id)| ObjectRef::Id(id.into())),
            ),
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
impl SceneInstance {
    pub fn has_blueprints(&self) -> bool {
        self.document
            .objects
            .iter()
            .any(|o| o.blueprints.iter().any(|b| b.enabled))
    }
    pub fn step_blueprints(
        &mut self,
        world: &mut World,
        dt: f32,
        input: GameplayInput,
    ) -> Result<()> {
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
            let query_overlaps = self
                .document
                .objects
                .iter()
                .flat_map(|o| &o.blueprints)
                .filter(|b| b.enabled)
                .any(|b| needs_overlap(&b.graph));
            let collisions = query_overlaps.then(|| self.collisions(world)).transpose()?;
            let matrices = query_overlaps
                .then(|| self.global_transforms(world))
                .transpose()?;
            // Snapshot contacts once before graph actions. Order does not change this tick's events.
            let mut contacts = BTreeMap::new();
            let mut overlap_budget = 1_000_000usize;
            if let (Some(collisions), Some(matrices)) = (&collisions, &matrices) {
                for object in self.document.objects.iter().filter(|o| {
                    o.blueprints
                        .iter()
                        .any(|b| b.enabled && needs_overlap(&b.graph))
                }) {
                    let entity = self.entities[&object.id];
                    let collider = world
                        .get::<Trigger>(entity)
                        .map(|t| t.volume)
                        .or_else(|| world.get::<BoxCollider>(entity).copied());
                    let mut overlap = BTreeSet::new();
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
                    }
                    contacts.insert(object.id.clone(), overlap);
                }
            }
            let empty = BTreeSet::new();
            let mut budget = 100_000usize;
            // New instances begin their graphs on the next tick, never recursively during Spawn.
            let owners: Vec<_> = self
                .document
                .objects
                .iter()
                .filter(|o| o.blueprints.iter().any(|b| b.enabled))
                .cloned()
                .collect();
            for object in &owners {
                if !self.entities.contains_key(&object.id) {
                    continue;
                }
                let overlap = contacts.get(&object.id).unwrap_or(&empty);
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
                        if !self.entities.contains_key(&object.id) {
                            break;
                        }
                        let key_index = InputKey::ALL.iter().position(|k| *k == event.key).unwrap();
                        let fire = match event.kind {
                            K::Start => !run.started,
                            K::Update => true,
                            K::InputPressed => {
                                event.key.active(input)
                                    && (event.key == InputKey::Jump || !run.held[key_index])
                            }
                            K::TriggerEnter => !overlap.is_empty() && run.overlap.is_empty(),
                            K::TriggerExit => overlap.is_empty() && !run.overlap.is_empty(),
                            _ => false,
                        };
                        let events: Vec<Option<&str>> = match event.kind {
                            K::BodyEnter => overlap
                                .difference(&run.overlap)
                                .map(|id| Some(id.as_str()))
                                .collect(),
                            K::BodyExit => run
                                .overlap
                                .difference(overlap)
                                .map(|id| Some(id.as_str()))
                                .collect(),
                            _ if fire => vec![None],
                            _ => Vec::new(),
                        };
                        for other in events {
                            ensure!(
                                budget > 0,
                                "blueprint execution budget exceeded (100000 actions/tick)"
                            );
                            budget -= 1;
                            let mut queue = VecDeque::from([Socket {
                                node: event.id,
                                port: 0,
                            }]);
                            while let Some(output) = queue.pop_front() {
                                if !self.entities.contains_key(&object.id) {
                                    break;
                                }
                                for wire in graph.wires.iter().filter(|w| w.from == output) {
                                    if !self.entities.contains_key(&object.id) {
                                        break;
                                    }
                                    ensure!(
                                        budget > 0,
                                        "blueprint execution budget exceeded (100000 actions/tick)"
                                    );
                                    budget -= 1;
                                    let node = graph.node(wire.to.node)?;
                                    let mut eval = Eval {
                                        graph,
                                        variables: &run.variables,
                                        spawned: &run.spawned,
                                        world,
                                        entities: &self.entities,
                                        owner: &object.id,
                                        other: other.map(|id| (event.id, id)),
                                        overlap_count: overlap.len(),
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
                                    let target = if let Some(port) = node.kind.target_port() {
                                        let target = eval.input(node, port)?;
                                        reference_id(target.object()?, &object.id).context("action target is None; choose an object or guard with Is Valid Object")?.to_owned()
                                    } else {
                                        object.id.clone()
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
                                            let id = self.spawn_prefab(
                                                world,
                                                &node.prefab,
                                                value.vector()?,
                                            )?;
                                            run.spawned.insert(node.id, ObjectRef::Id(id));
                                        }
                                        K::DestroyPrefab => self.destroy_prefab(world, &target)?,
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
                                                    next.translation =
                                                        (Vec3::from(next.translation)
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
                                            if let Err(error) =
                                                self.validate_transform_change(world, &target)
                                            {
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
                                            if let Some(material) =
                                                world.get_mut::<Material>(entity)
                                            {
                                                material.color = color;
                                            } else {
                                                // Legacy graphs also work on meshes using their source material.
                                                world
                                                    .get_mut::<Drawable>(entity)
                                                    .context("Set Color needs a mesh or Material")?
                                                    .color = color;
                                            }
                                        }
                                        K::SetVisible => {
                                            world.insert(
                                                entity,
                                                BlueprintHidden(!value.boolean()?),
                                            )?;
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
                                            self.move_box(
                                                world,
                                                &target,
                                                Vec3::from(value.vector()?),
                                            )?;
                                        }
                                        K::Jump => {
                                            self.jump_box(world, &target, value.number()?)?;
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
                    }
                    run.started = true;
                    run.overlap = overlap.clone();
                    run.held = InputKey::ALL.map(|key| key.active(input));
                }
            }
            runtime
                .runs
                .retain(|(id, _), _| self.entities.contains_key(id));
            Ok(())
        })();
        world.insert_resource(runtime);
        result
    }
}
