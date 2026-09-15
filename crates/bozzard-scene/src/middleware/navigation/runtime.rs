use super::*;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentState {
    pub initialized: bool,
    pub state: usize,
    pub elapsed: f32,
    pub repath: f32,
    pub path: Vec<[f32; 3]>,
    pub cursor: usize,
    pub velocity: [f32; 3],
    pub destination: Option<[f32; 3]>,
    pub target: Option<String>,
    pub halted: bool,
    pub sees_target: bool,
    pub arrived: bool,
    pub blocked: bool,
    pub waypoint: usize,
    pub goal: Option<[f32; 3]>,
}
impl AgentState {
    fn initialize(&mut self, agent: &NavAgent) {
        if !self.initialized {
            self.state = agent
                .states
                .iter()
                .position(|s| s.name == agent.initial)
                .unwrap_or(0);
            self.initialized = true;
        }
    }
    fn enter(&mut self, state: usize) {
        self.state = state;
        self.elapsed = 0.;
        self.repath = 0.;
        self.path.clear();
        self.cursor = 0;
        self.destination = None;
        self.halted = false;
        self.arrived = false;
        self.blocked = false;
        self.waypoint = 0;
        self.goal = None;
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Runtime {
    pub agents: BTreeMap<String, AgentState>,
    pub next: usize,
}
pub enum Control {
    Destination([f32; 3]),
    State(String),
    Target(Option<String>),
    Stop,
}
fn signal(
    signals: &mut Signals,
    owner: &str,
    name: &str,
    other: Option<String>,
    value: f32,
) -> Result<()> {
    signals.emit(
        owner,
        Signal {
            kind: Kind::Navigation,
            name: name.into(),
            other,
            value,
        },
    )
}
impl SceneInstance {
    pub fn control_navigation(
        &self,
        world: &mut World,
        owner: &str,
        control: Control,
    ) -> Result<()> {
        let entity = self.entity(owner).context("navigation target missing")?;
        let agent = world
            .get::<NavAgent>(entity)
            .context("target has no Navigation Agent")?
            .clone();
        let state = match &control {
            Control::State(name) => Some(
                agent
                    .states
                    .iter()
                    .position(|s| &s.name == name)
                    .context("agent state missing")?,
            ),
            Control::Destination(p) => {
                ensure!(
                    Vec3::from(*p).is_finite() && Vec3::from(*p).abs().max_element() <= 1e6,
                    "invalid navigation destination"
                );
                None
            }
            Control::Target(Some(id)) => {
                ensure!(
                    self.entity(id).is_some() && id != owner,
                    "navigation follow target missing or points to itself"
                );
                None
            }
            _ => None,
        };
        if world.resource::<Runtime>().is_none() {
            world.insert_resource(Runtime::default());
        }
        let run = world
            .resource_mut::<Runtime>()
            .unwrap()
            .agents
            .entry(owner.into())
            .or_default();
        run.initialize(&agent);
        match control {
            Control::Destination(point) => {
                run.destination = Some(point);
                run.halted = false;
                run.repath = 0.;
                run.arrived = false;
                run.blocked = false;
            }
            Control::State(_) => run.enter(state.unwrap()),
            Control::Target(target) => {
                run.target = target;
                run.repath = 0.;
            }
            Control::Stop => {
                run.halted = true;
                run.path.clear();
                run.cursor = 0;
                run.velocity = [0.; 3];
                run.destination = None;
                run.goal = None;
            }
        }
        Ok(())
    }
    pub fn step_navigation(&self, world: &mut World, dt: f32) -> Result<()> {
        ensure!(dt.is_finite() && dt >= 0., "invalid navigation timestep");
        let agents: Vec<_> = self
            .entities
            .iter()
            .filter_map(|(id, &e)| {
                world
                    .get::<NavAgent>(e)
                    .filter(|a| a.enabled)
                    .map(|a| (id.clone(), e, a.clone()))
            })
            .collect();
        ensure!(
            agents.is_empty() || dt <= 10.,
            "navigation timestep exceeds 10 seconds"
        );
        let mut signals = world.remove_resource::<Signals>().unwrap_or_default();
        signals.begin(Kind::Navigation);
        if agents.is_empty() {
            world.insert_resource(signals);
            return Ok(());
        }
        let mut runtime = world.remove_resource::<Runtime>().unwrap_or_default();
        let result = (|| -> Result<()> {
            let matrices = self.global_transforms(world)?;
            let parents: BTreeMap<_, _> = self
                .document
                .objects
                .iter()
                .map(|o| (o.id.as_str(), o.parent.as_deref()))
                .collect();
            let mut geometry = self.query_geometry(world)?;
            geometry.boxes.retain(|b| {
                world.get::<crate::Trigger>(b.entity).is_none()
                    && world.get::<NavAgent>(b.entity).is_none()
            });
            geometry.meshes.retain(|b| {
                world.get::<crate::Trigger>(b.entity).is_none()
                    && world.get::<NavAgent>(b.entity).is_none()
            });
            let mut budget = 1_000_000;
            let mut path_budget = 8;
            // Buckets keep separation local rather than comparing every agent pair each tick.
            let bucket_size = agents
                .iter()
                .map(|(_, _, a)| a.radius * 4.)
                .fold(0.1, f32::max);
            let mut buckets: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
            for (i, (id, _, _)) in agents.iter().enumerate() {
                let p = matrices[id].w_axis.truncate();
                buckets
                    .entry((
                        (p.x / bucket_size).floor().clamp(-1e9, 1e9) as i32,
                        (p.z / bucket_size).floor().clamp(-1e9, 1e9) as i32,
                    ))
                    .or_default()
                    .push(i);
            }
            let start = runtime.next % agents.len();
            runtime.next = (start + 8) % agents.len();
            for (owner, entity, agent) in agents.iter().cycle().skip(start).take(agents.len()) {
                let run = runtime.agents.entry(owner.clone()).or_default();
                let newly = !run.initialized;
                run.initialize(agent);
                if newly {
                    signal(
                        &mut signals,
                        owner,
                        &format!("State:{}", agent.states[run.state].name),
                        None,
                        0.,
                    )?;
                }
                run.elapsed = (run.elapsed + dt).min(86400.);
                run.repath = (run.repath - dt).max(0.);
                let position = matrices[owner].w_axis.truncate();
                let forward = -matrices[owner].z_axis.truncate().normalize_or_zero();
                let state = &agent.states[run.state];
                let target = run
                    .target
                    .as_ref()
                    .or(agent.perception_target.as_ref())
                    .or(state.target.as_ref());
                let target_position = target
                    .and_then(|id| matrices.get(id))
                    .map(|m| m.w_axis.truncate());
                let eye = position + Vec3::Y * agent.eye_height;
                let visible = if let Some(target_position) = target_position {
                    let delta = target_position + Vec3::Y * agent.eye_height - eye;
                    let distance = delta.length();
                    if distance > agent.sight_range {
                        false
                    } else if distance < 1e-5 {
                        true
                    } else if forward.dot(delta / distance)
                        < (agent.sight_degrees * 0.5).to_radians().cos()
                    {
                        false
                    } else {
                        geometry
                            .raycast_budget(eye, delta, distance, Some(owner), &mut budget)?
                            .is_none_or(|hit| target.is_some_and(|id| &hit.object == id))
                    }
                } else {
                    false
                };
                if visible != run.sees_target {
                    signal(
                        &mut signals,
                        owner,
                        if visible { "TargetSeen" } else { "TargetLost" },
                        target.cloned(),
                        target_position.map_or(0., |p| p.distance(position)),
                    )?;
                    run.sees_target = visible;
                }
                if let Some(transition) = agent.transitions.iter().find(|t| {
                    (t.from == "*" || t.from == state.name)
                        && match t.condition {
                            Condition::SeeTarget => visible,
                            Condition::LostTarget => !visible,
                            Condition::Arrived => run.arrived,
                            Condition::After => run.elapsed >= t.seconds,
                            Condition::Blocked => run.blocked,
                        }
                }) {
                    let next = agent
                        .states
                        .iter()
                        .position(|s| s.name == transition.to)
                        .unwrap();
                    if next != run.state {
                        run.enter(next);
                        signal(
                            &mut signals,
                            owner,
                            &format!("State:{}", agent.states[next].name),
                            None,
                            0.,
                        )?;
                    }
                }
                let state = &agent.states[run.state];
                let state_target = run
                    .target
                    .as_ref()
                    .or(state.target.as_ref())
                    .and_then(|id| matrices.get(id))
                    .map(|m| m.w_axis.truncate());
                let goal = if run.halted {
                    None
                } else if let Some(p) = run.destination {
                    Some(Vec3::from(p))
                } else {
                    match state.behavior {
                        Behavior::Idle => None,
                        Behavior::MoveTo => Some(Vec3::from(state.destination)),
                        Behavior::Follow => state_target,
                        Behavior::Flee => state_target.map(|p| {
                            position
                                + (position - p).with_y(0.).normalize_or_zero()
                                    * agent.sight_range.max(1.)
                        }),
                        Behavior::Patrol => state
                            .patrol
                            .get(run.waypoint % state.patrol.len())
                            .copied()
                            .map(Vec3::from),
                    }
                };
                let Some(goal) = goal else {
                    run.velocity = [0.; 3];
                    continue;
                };
                let nav_entity = self
                    .entity(&agent.surface)
                    .context("moving agent needs a Navigation Surface")?;
                let nav = world
                    .get::<NavSurface>(nav_entity)
                    .and_then(|s| s.baked.as_ref())
                    .context("moving agent needs a baked Navigation Surface")?;
                ensure!(
                    agent.radius <= nav.settings.radius + 1e-4
                        && agent.height <= nav.settings.height + 1e-4,
                    "agent is larger than baked clearance"
                );
                if run.repath <= 0. && path_budget > 0 {
                    path_budget -= 1;
                    run.repath = agent.repath_seconds;
                    let path = nav.path(position, goal);
                    run.path = path
                        .unwrap_or_default()
                        .into_iter()
                        .map(|p| p.to_array())
                        .collect();
                    run.cursor = usize::from(run.path.len() > 1);
                    run.goal = run.path.last().copied();
                    let blocked = run.path.is_empty();
                    if blocked && !run.blocked {
                        signal(&mut signals, owner, "Blocked", None, 0.)?;
                    }
                    run.blocked = blocked;
                }
                if run.path.is_empty() {
                    run.velocity = [0.; 3];
                    continue;
                }
                let endpoint = Vec3::from(run.goal.unwrap());
                let distance = (endpoint - position).with_y(0.).length();
                if distance <= agent.stopping_distance {
                    if !run.arrived {
                        signal(&mut signals, owner, "Arrived", None, distance)?;
                    }
                    run.arrived = true;
                    run.velocity = [0.; 3];
                    if state.behavior == Behavior::Patrol && run.destination.is_none() {
                        run.waypoint = (run.waypoint + 1) % state.patrol.len();
                        run.repath = 0.;
                        run.arrived = false;
                    }
                    continue;
                }
                run.arrived = false;
                // Skip already reached centers, but preserve corners so paths cannot cut through walls.
                while run.cursor + 1 < run.path.len()
                    && (Vec3::from(run.path[run.cursor]) - position)
                        .with_y(0.)
                        .length()
                        < agent.stopping_distance.min(nav.settings.cell * 0.2)
                {
                    run.cursor += 1;
                }
                let waypoint = Vec3::from(run.path[run.cursor]);
                let delta = (waypoint - position).with_y(0.);
                let mut direction = delta.normalize_or_zero();
                let mut separation = Vec3::ZERO;
                let (bx, bz) = (
                    (position.x / bucket_size).floor().clamp(-1e9, 1e9) as i32,
                    (position.z / bucket_size).floor().clamp(-1e9, 1e9) as i32,
                );
                for z in bz - 1..=bz + 1 {
                    for x in bx - 1..=bx + 1 {
                        for &i in buckets.get(&(x, z)).into_iter().flatten() {
                            let (id, _, other) = &agents[i];
                            if id == owner {
                                continue;
                            }
                            let away = (position - matrices[id].w_axis.truncate()).with_y(0.);
                            let distance = away.length();
                            let radius = (agent.radius + other.radius) * 2.;
                            if distance > 1e-4 && distance < radius {
                                separation += away / distance * (1. - distance / radius);
                            }
                        }
                    }
                }
                direction = (direction + separation * agent.separation).normalize_or_zero();
                let desired = direction * (agent.speed * state.speed);
                let old = Vec3::from(run.velocity);
                let change = (desired - old).clamp_length_max(agent.acceleration * dt);
                let velocity = old + change;
                // Limit to the next corner, then re-evaluate on the following fixed simulation tick.
                let displacement = (velocity * dt).clamp_length_max(delta.length());
                let mut next = position;
                let steps = (displacement.length()
                    / (nav.settings.cell * 0.25).min(agent.radius * 0.5))
                .ceil()
                .max(1.) as usize;
                ensure!(steps <= 4096, "navigation movement budget exceeded");
                for i in 1..=steps {
                    let mut p = position + displacement * (i as f32 / steps as f32);
                    let Some(cell) = nav.nearest(p) else {
                        break;
                    };
                    let floor = nav.point(cell).unwrap();
                    if (floor.x - p.x).abs() > nav.settings.cell * 0.501
                        || (floor.z - p.z).abs() > nav.settings.cell * 0.501
                        || (floor.y - next.y).abs()
                            > nav.settings.climb
                                + nav.settings.cell * nav.settings.slope_degrees.to_radians().tan()
                                + 0.02
                    {
                        break;
                    }
                    let normal = Vec3::from(nav.cells[cell].as_ref().unwrap().normal);
                    p.y = floor.y
                        - (normal.x * (p.x - floor.x) + normal.z * (p.z - floor.z)) / normal.y;
                    let obstruction = geometry.overlap_box_budget(
                        p + Vec3::Y * (agent.height * 0.5 + 0.03),
                        Vec3::new(agent.radius * 2., agent.height - 0.02, agent.radius * 2.),
                        Some(owner),
                        geometry.boxes.len() + geometry.meshes.len(),
                        &mut budget,
                    )?;
                    if !obstruction.is_empty() {
                        break;
                    }
                    next = p;
                }
                let moved = (next - position).with_y(0.);
                if displacement.length_squared() > 1e-8 && moved.length_squared() < 1e-10 {
                    if !run.blocked {
                        signal(&mut signals, owner, "Blocked", None, 0.)?;
                    }
                    run.blocked = true;
                    run.velocity = [0.; 3];
                } else {
                    run.velocity = if dt > 0. {
                        (moved / dt).to_array()
                    } else {
                        [0.; 3]
                    };
                    run.blocked = false;
                }
                if next != position {
                    let parent = parents
                        .get(owner.as_str())
                        .copied()
                        .flatten()
                        .and_then(|id| matrices.get(id));
                    let mut transform = *world
                        .get::<Transform>(*entity)
                        .context("agent transform missing")?;
                    transform.translation = parent
                        .map_or(next, |m| m.inverse().transform_point3(next))
                        .to_array();
                    if moved.length_squared() > 1e-8 {
                        let local = parent.map_or(moved, |m| m.inverse().transform_vector3(moved));
                        transform.rotation_degrees[1] = (-local.x).atan2(-local.z).to_degrees();
                    }
                    transform.validate()?;
                    world.insert(*entity, transform)?;
                }
            }
            runtime.agents.retain(|id, _| {
                self.entity(id)
                    .is_some_and(|e| world.get::<NavAgent>(e).is_some())
            });
            Ok(())
        })();
        world.insert_resource(runtime);
        world.insert_resource(signals);
        result
    }
}
