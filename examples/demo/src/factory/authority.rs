//! Data-only, host-side actor transactions. The same loaded Rhai modules provide
//! recipes, inventory fitting, deposits and power rules as local gameplay.
//! No temporary camera/planet switch or remote scene models are needed.
use super::{
    replication::requests::Action,
    shared::{Player, Position, Stack, World},
    state::{self, State},
};
use anyhow::{Context, Result, ensure};
use bozzard_network::Peer;
use bozzard_scene::{
    SceneInstance, ScriptModule,
    blueprint::{BlackboardValue as B, Value},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cell {
    node: f32,
    build: f32,
    facing: f32,
    recipe: f32,
    item: f32,
    amount: f32,
    input: f32,
    input_amount: f32,
    progress: f32,
    iron: f32,
    copper: f32,
    split: f32,
    storage: Vec<f32>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transaction {
    ok: bool,
    message: String,
    phase: f32,
    creative: bool,
    landing_clear: bool,
    here: [i32; 3],
    at: [i32; 3],
    inventory: Vec<f32>,
    cell: Cell,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub accepted: bool,
    pub message: String,
}
fn accepted(message: &str) -> Outcome {
    Outcome {
        accepted: true,
        message: message.into(),
    }
}

#[derive(Default)]
struct Actor {
    gathering: bool,
    next_gather: Duration,
    last_move: Option<Duration>,
    doors: BTreeMap<(Position, usize), Duration>,
    flight: Option<Flight>,
}
#[derive(Clone, Copy)]
struct Flight {
    began: Duration,
    destination: u8,
    arrived: bool,
}
#[derive(Clone, Copy)]
struct Rotation {
    began: Duration,
    turns: u8,
    kind: u8,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationView {
    pub at: Position,
    pub progress: f32,
    pub turns: u8,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlightView {
    pub peer: Peer,
    pub elapsed: f32,
    pub destination: u8,
}

pub struct Executor {
    rules: ScriptModule,
    architecture: ScriptModule,
    actors: BTreeMap<Peer, Actor>,
    rotations: BTreeMap<Position, Rotation>,
}
impl Executor {
    pub fn new(scene: &SceneInstance) -> Result<Self> {
        let rules = scene.script_module("factory-authority")?;
        rules.require_function("apply", 3)?;
        Ok(Self {
            rules,
            architecture: scene.script_module("factory-architecture")?,
            actors: BTreeMap::new(),
            rotations: BTreeMap::new(),
        })
    }
    pub fn fingerprint(&self) -> u64 {
        self.rules.fingerprint()
    }
    pub fn retain_members(&mut self, peers: &BTreeSet<Peer>) {
        self.actors.retain(|peer, _| peers.contains(peer));
    }
    pub fn rotation_views(&self, now: Duration) -> Vec<RotationView> {
        self.rotations
            .iter()
            .map(|(at, r)| RotationView {
                at: *at,
                progress: (now.saturating_sub(r.began).as_secs_f32() / 0.6).clamp(0., 1.),
                turns: r.turns,
            })
            .collect()
    }
    pub fn flight_views(&self, now: Duration) -> Vec<FlightView> {
        self.actors
            .iter()
            .filter_map(|(peer, a)| {
                a.flight.map(|f| FlightView {
                    peer: *peer,
                    elapsed: now.saturating_sub(f.began).as_secs_f32(),
                    destination: f.destination,
                })
            })
            .collect()
    }
    pub fn apply(
        &mut self,
        world: &mut World,
        peer: Peer,
        player: &mut Player,
        action: &Action,
        now: Duration,
    ) -> Result<Outcome> {
        ensure!(peer != 0, "invalid player identity");
        action.validate()?;
        player.validate()?;
        ensure!(
            world.discovered(player.position)?,
            "player location is not discovered"
        );
        if matches!(action, Action::Gather { active: false }) {
            self.actors.entry(peer).or_default().gathering = false;
            return Ok(accepted("Gathering stopped."));
        }
        ensure!(
            self.actors.get(&peer).is_none_or(|a| a.flight.is_none()),
            "Wait until the rocket lands."
        );
        self.observe_doors(world.state(), peer, player.position, now)?;
        match action {
            Action::Structure {
                kind,
                direction,
                remove,
            } => return self.structure(world, player, *kind, *direction, *remove),
            Action::Point { at } => {
                ensure!(
                    player.position.near(*at, 90) && world.discovered(*at)?,
                    "Select nearby discovered terrain."
                );
                self.check_path(world.state(), player.position, *at, now)?;
                player.position = *at;
                return Ok(accepted("Cursor moved."));
            }
            Action::InventoryClear => {
                ensure!(world.phase() >= 2, "Inventory is locked.");
                player.backpack.fill(Stack::default());
                return Ok(accepted("Backpack cleared."));
            }
            Action::Move { x, z } => {
                let actor = self.actors.entry(peer).or_default();
                ensure!(actor.last_move.is_none_or(|at| now.saturating_sub(at)>=Duration::from_nanos(16_666_666)), "Moving too quickly.");
                let mut destination = player.position;
                destination.x += i16::from(*x);
                destination.z += i16::from(*z);
                destination.validate()?;
                self.check_path(world.state(), player.position, destination, now)?;
                let mut state = world.state().clone();
                self.discover_near(&mut state, destination)?;
                let next = World::from_canonical(state)?;
                *world = next;
                player.position = destination;
                self.actors.entry(peer).or_default().last_move = Some(now);
                return Ok(accepted("Moved."));
            }
            Action::Discover { planet, x, z } => {
                ensure!(
                    *planet == player.position.planet,
                    "Discover from the planet you are on."
                );
                let (cx, cz) = player.position.chunk();
                let (lx, lz) = player.position.cell();
                let (x, z) = (i16::from(*x), i16::from(*z));
                ensure!(
                    (x == cx && z == cz)
                        || (z == cz && ((x == cx + 1 && lx >= 6) || (x == cx - 1 && lx <= -6)))
                        || (x == cx && ((z == cz + 1 && lz >= 6) || (z == cz - 1 && lz <= -6))),
                    "Move to the edge before discovering that region."
                );
                let mut state = world.state().clone();
                self.discover(&mut state, *planet, x, z)?;
                *world = World::from_canonical(state)?;
                return Ok(accepted("Region discovered."));
            }
            Action::Gather { active } => {
                self.actors.entry(peer).or_default().gathering = *active;
                return Ok(accepted("Gathering started."));
            }
            Action::Travel => {
                ensure!(
                    !world.state().dev_world(),
                    "Dev World has no planet travel."
                );
                ensure!(
                    world.phase() >= 7
                        && player.position.x.abs() <= 1
                        && (player.position.z - 1).abs() <= 1,
                    "Board the completed rocket at the landing site."
                );
                let actor = self.actors.entry(peer).or_default();
                actor.gathering = false;
                actor.flight = Some(Flight {
                    began: now,
                    destination: 1 - player.position.planet,
                    arrived: false,
                });
                return Ok(accepted("Rocket launched."));
            }
            Action::Wire { from, to } => return self.wire(world, player, *from, Some(*to), true),
            Action::Unwire { from, to } => return self.wire(world, player, *from, *to, false),
            Action::Rotate => {
                let at = array_owner(world.state(), player.position)?;
                let cell = read_cell(world.state(), at)?;
                ensure!(cell.build > 0., "No machine here.");
                if cell.build == 41. {
                    return self.transaction(world, player, at, "rotate", serde_json::Value::Null);
                }
                let rotation = self.rotations.entry(player.position).or_insert(Rotation {
                    began: now,
                    turns: 0,
                    kind: cell.build as u8,
                });
                rotation.turns = (rotation.turns + 1).min(8);
                player.direction = (cell.facing as u8 + rotation.turns) % 4;
                return Ok(accepted("Turning machine."));
            }
            _ => (),
        }
        let mut at = match action {
            Action::Configure { at, .. }
            | Action::Collect { at }
            | Action::Feed { at }
            | Action::Insert { at, .. }
            | Action::StorageTransfer { at, .. }
            | Action::TakeStorage { at }
            | Action::StorageMove { at, .. }
            | Action::StorageSplit { at, .. }
            | Action::StorageDeleteKind { at, .. } => *at,
            _ => player.position,
        };
        if matches!(action, Action::Remove) {
            at = array_owner(world.state(), at)?;
        }
        ensure!(
            player.position.near(at, 1),
            "Walk next to that machine first."
        );
        self.check_path(world.state(), player.position, at, now)?;
        if !matches!(action, Action::Remove) {
            ensure!(
                !self.rotations.contains_key(&at),
                "Wait for the machine to finish turning."
            );
        }
        let value = serde_json::to_value(action)?;
        let (kind, args) = match &value {
            serde_json::Value::String(kind) => (kind.as_str(), serde_json::Value::Null),
            serde_json::Value::Object(fields) => {
                let (kind, args) = fields.iter().next().context("empty action")?;
                (kind.as_str(), args.clone())
            }
            _ => anyhow::bail!("invalid action"),
        };
        let result = self.transaction(world, player, at, kind, args)?;
        if result.accepted {
            match action {
                Action::Remove => {
                    self.rotations.remove(&at);
                }
                Action::Place { kind, direction } => {
                    player.selected = *kind;
                    player.direction = *direction;
                }
                _ => (),
            }
        }
        Ok(result)
    }
    /// Advance on host simulation time, not client timestamps. Production uses
    /// rotation_views as its pause mask; disconnected actors stop gathering.
    pub fn advance(
        &mut self,
        world: &mut World,
        players: &mut BTreeMap<Peer, Player>,
        now: Duration,
    ) -> Result<()> {
        for (peer, player) in players.iter() {
            self.observe_doors(world.state(), *peer, player.position, now)?;
        }
        for (at, rotation) in self.rotations.clone() {
            let mut cell = read_cell(world.state(), at)?;
            if cell.build as u8 != rotation.kind {
                self.rotations.remove(&at);
                continue;
            }
            let completed = (now.saturating_sub(rotation.began).as_secs_f64() / 0.6)
                .floor()
                .min(f64::from(rotation.turns)) as u8;
            if completed == 0 {
                continue;
            }
            let before = cell.clone();
            cell.facing = ((cell.facing as u8 + completed) % 4) as f32;
            let mut state = world.state().clone();
            write_cell(&mut state, at, &before, &cell)?;
            *world = World::from_canonical(state)?;
            if completed == rotation.turns {
                self.rotations.remove(&at);
            } else {
                self.rotations.insert(
                    at,
                    Rotation {
                        began: rotation.began + Duration::from_millis(600) * u32::from(completed),
                        turns: rotation.turns - completed,
                        kind: rotation.kind,
                    },
                );
            }
        }
        for peer in self.actors.keys().copied().collect::<Vec<_>>() {
            let Some(player) = players.get_mut(&peer) else {
                self.actors.remove(&peer);
                continue;
            };
            if let Some(flight) = self.actors[&peer].flight {
                let elapsed = now.saturating_sub(flight.began);
                if !flight.arrived && elapsed >= Duration::from_secs(2) {
                    let mut state = world.state().clone();
                    self.discover(&mut state, flight.destination, 0, 0)?;
                    let mut graph = read_power(&state, flight.destination)?;
                    graph
                        .entry((144 * 225 + 112).to_string())
                        .or_insert(vec![10, 0]);
                    self.write_power(&mut state, flight.destination, graph)?;
                    *world = World::from_canonical(state)?;
                    player.position = Position {
                        planet: flight.destination,
                        x: 0,
                        z: 2,
                    };
                    self.actors
                        .get_mut(&peer)
                        .unwrap()
                        .flight
                        .as_mut()
                        .unwrap()
                        .arrived = true;
                }
                if elapsed >= Duration::from_millis(4400) {
                    self.actors.get_mut(&peer).unwrap().flight = None;
                }
                continue;
            }
            if self.actors[&peer].gathering && now >= self.actors[&peer].next_gather {
                let _ = self.transaction(
                    world,
                    player,
                    player.position,
                    "gather_tick",
                    serde_json::json!({}),
                )?;
                self.actors.get_mut(&peer).unwrap().next_gather = now + Duration::from_millis(300);
            }
        }
        Ok(())
    }
    fn observe_doors(
        &mut self,
        state: &State,
        peer: Peer,
        at: Position,
        now: Duration,
    ) -> Result<()> {
        if state::values(&state.controller, "cache_structures")?
            .iter()
            .all(|v| matches!(v,Value::Text(s) if s.is_empty()))
        {
            if let Some(actor) = self.actors.get_mut(&peer) {
                actor.doors.clear();
            }
            return Ok(());
        }
        let mut near = BTreeSet::new();
        for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            let p = Position {
                x: at.x + dx,
                z: at.z + dz,
                ..at
            };
            if p.validate().is_err() {
                continue;
            }
            for dir in 0..4 {
                let (kind, base, slot) = structure_edge(state, p, dir)?;
                if kind == 38 {
                    near.insert((base, slot));
                }
            }
        }
        let doors = &mut self.actors.entry(peer).or_default().doors;
        doors.retain(|key, _| near.contains(key));
        for key in near {
            doors.entry(key).or_insert(now);
        }
        Ok(())
    }
    fn check_path(&self, state: &State, from: Position, to: Position, now: Duration) -> Result<()> {
        ensure!(
            from.planet == to.planet,
            "Use the rocket to change planets."
        );
        let check =
            |at, dir| -> Result<()> {
                let (kind, base, slot) = structure_edge(state, at, dir)?;
                if kind == 38 {
                    ensure!(
                        self.actors.values().any(|actor| actor
                            .doors
                            .get(&(base, slot))
                            .is_some_and(
                                |began| now.saturating_sub(*began) >= Duration::from_millis(650)
                            )),
                        "Wait for the sliding door to open."
                    );
                } else {
                    ensure!(kind == 0, "A wall blocks the way. Use the door.");
                }
                Ok(())
            };
        let mut at = from;
        let (dx, dz) = ((to.x - from.x).abs(), (to.z - from.z).abs());
        let (sx, sz) = ((to.x - from.x).signum(), (to.z - from.z).signum());
        let (mut ix, mut iz) = (0, 0);
        while ix < dx || iz < dz {
            let (a, b) = ((1 + 2 * ix) * dz, (1 + 2 * iz) * dx);
            let xd = if sx > 0 { 0 } else { 2 };
            let zd = if sz > 0 { 1 } else { 3 };
            if a == b {
                check(at, xd)?;
                check(at, zd)?;
                check(Position { x: at.x + sx, ..at }, zd)?;
                check(Position { z: at.z + sz, ..at }, xd)?;
                at.x += sx;
                at.z += sz;
                ix += 1;
                iz += 1;
            } else if a < b {
                check(at, xd)?;
                at.x += sx;
                ix += 1;
            } else {
                check(at, zd)?;
                at.z += sz;
                iz += 1;
            }
        }
        Ok(())
    }
    fn structure(
        &mut self,
        world: &mut World,
        player: &mut Player,
        kind: u8,
        direction: u8,
        remove: bool,
    ) -> Result<Outcome> {
        #[derive(Deserialize)]
        struct Edit {
            ok: bool,
            message: String,
            region: usize,
            slot: usize,
            data: Vec<f32>,
        }
        #[derive(Deserialize)]
        struct Payment {
            ok: bool,
            message: String,
            inventory: Vec<f32>,
        }
        let state = world.state();
        let at = player.position;
        let pages = state::values(&state.controller, "cache_structures")?
            .iter()
            .map(|v| match v {
                Value::Text(s) => Ok(s.clone()),
                _ => anyhow::bail!("invalid structure archive"),
            })
            .collect::<Result<Vec<_>>>()?;
        let demonstration = matches!(state.scene["demo_mode"], B::Scalar(Value::Bool(true)));
        let edit: Edit = self.architecture.call_args(
            "edit",
            (
                pages,
                i64::from(at.planet),
                i64::from(at.x),
                i64::from(at.z),
                f32::from(kind),
                i64::from(direction),
                remove,
                state::number(&state.scene, "seed")? as i64,
                demonstration,
                state.dev_world(),
            ),
        )?;
        if !edit.ok {
            return Ok(Outcome {
                accepted: false,
                message: edit.message,
            });
        }
        ensure!(
            edit.data.len() == 900 && edit.region < 289 && edit.slot < 900,
            "invalid structure transaction"
        );
        let mut next_player = player.clone();
        if !remove {
            let payment: Payment = self.rules.call_args(
                "structure_payment",
                (
                    inventory(player),
                    f32::from(kind),
                    f32::from(world.phase()),
                    demonstration
                        || matches!(state.controller["creative"], B::Scalar(Value::Bool(true))),
                ),
            )?;
            if !payment.ok {
                return Ok(Outcome {
                    accepted: false,
                    message: payment.message,
                });
            }
            set_inventory(&mut next_player, &payment.inventory)?;
        }
        next_player.selected = kind;
        next_player.direction = direction;
        let mut next = state.clone();
        let base = Position {
            planet: at.planet,
            x: ((edit.region % 17) as i16 - 8) * 15 + (edit.slot / 4 % 15) as i16 - 7,
            z: ((edit.region / 17) as i16 - 8) * 15 + (edit.slot / 4 / 15) as i16 - 7,
        };
        self.discover_near(&mut next, base)?;
        set_page(
            &mut next,
            "cache_structures",
            usize::from(at.planet) * 289 + edit.region,
            &edit.data,
        )?;
        *world = World::from_canonical(next)?;
        *player = next_player;
        for actor in self.actors.values_mut() {
            actor.doors.clear();
        }
        Ok(accepted(if remove {
            "Structure removed. Machines stay in place."
        } else {
            "Structure built."
        }))
    }
    fn transaction(
        &self,
        world: &mut World,
        player: &mut Player,
        at: Position,
        kind: &str,
        args: serde_json::Value,
    ) -> Result<Outcome> {
        ensure!(world.discovered(at)?, "That region is not discovered.");
        let state = world.state();
        let before = read_cell(state, at)?;
        if kind == "place" || kind == "rotate" && before.build == 41. {
            let machine = if kind == "place" {
                args["kind"].as_u64().context("missing machine kind")? as f32
            } else {
                41.
            };
            let direction = if kind == "place" {
                args["direction"].as_u64().context("missing facing")? as i64
            } else {
                (before.facing as i64 + 1) % 4
            };
            let pages = |name: &str| -> Result<Vec<String>> {
                state::values(&state.controller, name)?
                    .iter()
                    .map(|v| match v {
                        Value::Text(t) => Ok(t.clone()),
                        _ => anyhow::bail!("invalid archive"),
                    })
                    .collect()
            };
            let error: String = self.rules.call_args(
                "footprint_error",
                (
                    pages("cache_builds")?,
                    pages("cache_facings")?,
                    pages("chunk_nodes")?,
                    i64::from(at.planet),
                    address(at),
                    machine,
                    direction,
                    if kind == "rotate" { address(at) } else { -1 },
                    state::number(&state.scene, "seed")? as i64,
                    matches!(state.scene["demo_mode"], B::Scalar(Value::Bool(true))),
                    state.dev_world(),
                    f32::from(world.phase()),
                ),
            )?;
            if !error.is_empty() {
                return Ok(Outcome {
                    accepted: false,
                    message: error,
                });
            }
        }
        if kind == "place"
            && !state.dev_world()
            && !matches!(state.scene["demo_mode"], B::Scalar(Value::Bool(true)))
            && self.rules.call_args::<_, bool>(
                "debris_blocked",
                (
                    state::number(&state.scene, "seed")? as i64,
                    i64::from(at.planet),
                    i64::from(at.x),
                    i64::from(at.z),
                ),
            )?
        {
            return Ok(Outcome {
                accepted: false,
                message: "Keep the spaceship wreckage clear.".into(),
            });
        }
        let input = Transaction {
            ok: false,
            message: String::new(),
            phase: world.phase() as f32,
            creative: matches!(state.controller["creative"], B::Scalar(Value::Bool(true)))
                || matches!(state.scene["demo_mode"], B::Scalar(Value::Bool(true))),
            landing_clear: landing_clear(state)?,
            here: position_array(player.position),
            at: position_array(at),
            inventory: inventory(player),
            cell: before.clone(),
        };
        let output: Transaction = self.rules.call_args("apply", (input, kind, args))?;
        if !output.ok {
            return Ok(Outcome {
                accepted: false,
                message: output.message,
            });
        }
        let mut next_player = player.clone();
        set_inventory(&mut next_player, &output.inventory)?;
        let mut next = state.clone();
        next.controller
            .insert("phase".into(), B::Scalar(Value::Number(output.phase)));
        write_cell(&mut next, at, &before, &output.cell)?;
        if before.build != output.cell.build {
            let mut graph = read_power(&next, at.planet)?;
            remove_power(&mut graph, address(at));
            if self
                .rules
                .call_args::<_, bool>("electrical", (output.cell.build as i64,))?
            {
                graph.insert(address(at).to_string(), vec![output.cell.build as i64, 0]);
            }
            self.write_power(&mut next, at.planet, graph)?;
        }
        let next = World::from_canonical(next)?;
        *world = next;
        *player = next_player;
        Ok(Outcome {
            accepted: true,
            message: output.message,
        })
    }
    fn discover_near(&self, state: &mut State, position: Position) -> Result<()> {
        let (cx, cz) = position.chunk();
        let (x, z) = position.cell();
        self.discover(state, position.planet, cx, cz)?;
        for (dx, dz, show) in [
            (1, 0, x >= 6),
            (-1, 0, x <= -6),
            (0, 1, z >= 6),
            (0, -1, z <= -6),
        ] {
            if show && state.allows_chunk(position.planet, cx + dx, cz + dz) {
                self.discover(state, position.planet, cx + dx, cz + dz)?;
            }
        }
        Ok(())
    }
    fn discover(&self, state: &mut State, planet: u8, x: i16, z: i16) -> Result<()> {
        ensure!(state.allows_chunk(planet, x, z), "World boundary reached.");
        let position = Position {
            planet,
            x: x * 15,
            z: z * 15,
        };
        position.validate()?;
        let at = position.archive_index();
        if matches!(&state.controller["chunk_nodes"].values()[at],Value::Text(t) if !t.is_empty()) {
            return Ok(());
        }
        ensure!(!state.dev_world(), "Dev World has no generated regions.");
        let seed = state::number(&state.scene, "seed")? as i64;
        let nodes: Vec<f32> = self.rules.call_args(
            "discover",
            (seed, i64::from(planet), i64::from(x), i64::from(z)),
        )?;
        ensure!(nodes.len() == 225, "invalid generated region");
        set_page(state, "chunk_nodes", at, &nodes)?;
        Ok(())
    }
    fn write_power(&self, state: &mut State, planet: u8, graph: Graph) -> Result<()> {
        let time = state::numeric(&state::values(&state.controller, "session")?[120])?;
        let seed = state::number(&state.scene, "seed")? as i64;
        let graph: Graph = self
            .rules
            .call_args("resolve_power", (graph, time, i64::from(planet), seed))?;
        let name = if planet == 0 {
            "power_data"
        } else {
            "power_other"
        };
        let mut pages = BTreeMap::<usize, Vec<String>>::new();
        for (key, entry) in graph {
            let id: usize = key.parse()?;
            ensure!(id < 289 * 225, "power terminal outside planet");
            pages
                .entry(id / 75)
                .or_insert_with(|| vec![String::new(); 75])[id % 75] = entry
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
        }
        for (i, value) in state
            .controller
            .get_mut(name)
            .unwrap()
            .values_mut()
            .iter_mut()
            .enumerate()
        {
            *value = Value::Text(pages.get(&i).map_or_else(String::new, |p| p.join("|")));
        }
        Ok(())
    }
    fn wire(
        &self,
        world: &mut World,
        player: &mut Player,
        from: Position,
        to: Option<Position>,
        connect: bool,
    ) -> Result<Outcome> {
        ensure!(
            player.position.near(from, 1) || to.is_some_and(|p| player.position.near(p, 1)),
            "Walk to a cable endpoint first."
        );
        ensure!(
            from.planet == player.position.planet
                && world.discovered(from)?
                && to.map(|p| world.discovered(p)).transpose()?.unwrap_or(true),
            "Unknown cable endpoint."
        );
        let from = array_owner(world.state(), from)?;
        let to = to.map(|at| array_owner(world.state(), at)).transpose()?;
        let mut state = world.state().clone();
        let mut graph = read_power(&state, from.planet)?;
        let a = address(from);
        let ak = a.to_string();
        ensure!(graph.contains_key(&ak), "No power terminal here.");
        let mut next_player = player.clone();
        if connect {
            let to = to.context("missing cable endpoint")?;
            let b = address(to);
            let error: String = self.rules.call_args("connection_error", (&graph, a, b))?;
            ensure!(error.is_empty(), "{error}");
            let creative = matches!(state.controller["creative"], B::Scalar(Value::Bool(true)))
                || matches!(state.scene["demo_mode"], B::Scalar(Value::Bool(true)));
            ensure!(creative || world.phase() >= 1, "Cables are locked.");
            if !creative {
                let stack = next_player
                    .backpack
                    .iter_mut()
                    .rev()
                    .find(|s| s.kind == 17 && s.amount > 0)
                    .context("Needs one cable.")?;
                stack.amount -= 1;
                if stack.amount == 0 {
                    stack.kind = 0;
                }
            }
            graph.get_mut(&ak).unwrap().push(b);
            graph.get_mut(&b.to_string()).unwrap().push(a);
        } else {
            let removed: Vec<_> = graph[&ak][2..]
                .iter()
                .copied()
                .filter(|id| to.is_none_or(|p| address(p) == *id))
                .collect();
            for id in removed {
                unlink(&mut graph, a, id);
            }
        }
        self.write_power(&mut state, from.planet, graph)?;
        *world = World::from_canonical(state)?;
        *player = next_player;
        Ok(accepted(if connect {
            "Cable connected."
        } else {
            "Cable disconnected."
        }))
    }
}

fn structure_edge(state: &State, mut at: Position, direction: u8) -> Result<(u8, Position, usize)> {
    if direction == 2 {
        at.x -= 1;
    } else if direction == 3 {
        at.z -= 1;
    }
    let slot = cell_index(at) * 4 + 2 + usize::from(direction % 2);
    if at.validate().is_err() {
        return Ok((0, at, slot));
    }
    if matches!(&state::values(&state.controller,"cache_structures")?[at.archive_index()],Value::Text(s) if s.is_empty())
    {
        return Ok((0, at, slot));
    }
    let values = page(state, "cache_structures", at.archive_index(), 900)?;
    Ok((values[slot] as u8, at, slot))
}

type Graph = BTreeMap<String, Vec<i64>>;
fn position_array(p: Position) -> [i32; 3] {
    [i32::from(p.planet), i32::from(p.x), i32::from(p.z)]
}
fn address(p: Position) -> i64 {
    let (cx, cz) = p.chunk();
    let (x, z) = p.cell();
    i64::from((cz + 8) * 17 + cx + 8) * 225 + i64::from((z + 7) * 15 + x + 7)
}
fn cell_index(p: Position) -> usize {
    (address(p) % 225) as usize
}
fn inventory(player: &Player) -> Vec<f32> {
    player
        .backpack
        .iter()
        .flat_map(|s| [s.kind as f32, s.amount as f32])
        .collect()
}
fn set_inventory(player: &mut Player, slots: &[f32]) -> Result<()> {
    ensure!(slots.len() == 50, "invalid inventory length");
    for (slot, values) in player.backpack.iter_mut().zip(slots.chunks_exact(2)) {
        ensure!(
            values[0].is_finite()
                && values[0].fract() == 0.
                && (0. ..=50.).contains(&values[0])
                && values[1].is_finite()
                && values[1].fract() == 0.
                && (0. ..=100.).contains(&values[1]),
            "invalid inventory result"
        );
        *slot = Stack {
            kind: values[0] as u8,
            amount: values[1] as u16,
        };
    }
    player.validate()
}
fn page(state: &State, name: &str, at: usize, count: usize) -> Result<Vec<f32>> {
    let Value::Text(text) = state
        .controller
        .get(name)
        .context("missing archive")?
        .values()
        .get(at)
        .context("missing archive page")?
    else {
        anyhow::bail!("invalid archive");
    };
    if text.is_empty() {
        return Ok(vec![0.; count]);
    }
    let mut result = Vec::with_capacity(count);
    for token in text.split(',') {
        let (value, run) = token.split_once(':').unwrap_or((token, "1"));
        let value: u32 = value.parse()?;
        let run: usize = run.parse()?;
        ensure!(
            run > 0 && run <= count && result.len() + run <= count,
            "invalid archive bounds"
        );
        result.extend(std::iter::repeat_n(value as f32, run));
    }
    ensure!(result.len() == count, "invalid archive length");
    Ok(result)
}
fn set_page(state: &mut State, name: &str, at: usize, values: &[f32]) -> Result<()> {
    let packed = if name == "cache_builds" && values.iter().all(|v| *v == 0.) {
        String::new()
    } else {
        state::pack_numbers(
            &values
                .iter()
                .copied()
                .map(Value::Number)
                .collect::<Vec<_>>(),
        )?
    };
    *state
        .controller
        .get_mut(name)
        .context("missing archive")?
        .values_mut()
        .get_mut(at)
        .context("invalid archive index")? = Value::Text(packed);
    Ok(())
}
fn read_cell(state: &State, p: Position) -> Result<Cell> {
    p.validate()?;
    let at = p.archive_index();
    let i = cell_index(p);
    let read = |key: &str| -> Result<f32> { Ok(page(state, key, at, 225)?[i]) };
    let mut storage = Vec::with_capacity(32);
    for group in 0..4 {
        let kinds = page(state, &format!("cache_storage_kinds_{group}"), at, 900)?;
        let amounts = page(state, &format!("cache_storage_amounts_{group}"), at, 900)?;
        for slot in 0..4 {
            storage.extend([kinds[i * 4 + slot], amounts[i * 4 + slot]]);
        }
    }
    Ok(Cell {
        node: read("chunk_nodes")?,
        build: read("cache_builds")?,
        facing: read("cache_facings")?,
        recipe: read("cache_recipes")?,
        item: read("cache_items")?,
        amount: read("cache_item_amounts")?,
        input: read("cache_input_items")?,
        input_amount: read("cache_input_amounts")?,
        progress: read("cache_progress")?,
        iron: read("cache_assembler_iron")?,
        copper: read("cache_assembler_copper")?,
        split: read("cache_split_state")?,
        storage,
    })
}

// A two-tile array has one saved machine/circuit node. Resolve either occupied
// tile back to that anchor for demolition, rotation and cable interaction.
fn array_owner(state: &State, p: Position) -> Result<Position> {
    let kind = |at: Position| -> Result<f32> {
        Ok(page(state, "cache_builds", at.archive_index(), 225)?[cell_index(at)])
    };
    if kind(p)? > 0. {
        return Ok(p);
    }
    for (x, z) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
        let anchor = Position {
            planet: p.planet,
            x: p.x + x,
            z: p.z + z,
        };
        if anchor.validate().is_err() || kind(anchor)? != 41. {
            continue;
        }
        let facing =
            page(state, "cache_facings", anchor.archive_index(), 225)?[cell_index(anchor)] as usize;
        let (dx, dz) = [(1, 0), (0, 1), (-1, 0), (0, -1)][facing];
        if anchor.x + dx == p.x && anchor.z + dz == p.z {
            return Ok(anchor);
        }
    }
    Ok(p)
}
fn write_cell(state: &mut State, p: Position, before: &Cell, cell: &Cell) -> Result<()> {
    ensure!(
        cell.storage.len() == 32 && cell.node == before.node,
        "invalid cell transaction"
    );
    let expansion = (12. ..=25.).contains(&cell.build);
    let load = cell.amount
        + cell.input_amount
        + cell.iron
        + cell.copper
        + if expansion {
            cell.storage.chunks_exact(2).map(|s| s[1]).sum::<f32>()
        } else {
            0.
        };
    let capacity = if [2., 28., 29.].contains(&cell.build) {
        1.
    } else if cell.build == 7. || cell.build == 8. {
        10.
    } else {
        100.
    };
    ensure!(
        load.is_finite() && load <= capacity,
        "machine buffer capacity exceeded"
    );
    let at = p.archive_index();
    let i = cell_index(p);
    for (key, value, old) in [
        ("builds", cell.build, before.build),
        ("facings", cell.facing, before.facing),
        ("recipes", cell.recipe, before.recipe),
        ("items", cell.item, before.item),
        ("item_amounts", cell.amount, before.amount),
        ("input_items", cell.input, before.input),
        ("input_amounts", cell.input_amount, before.input_amount),
        ("progress", cell.progress, before.progress),
        ("assembler_iron", cell.iron, before.iron),
        ("assembler_copper", cell.copper, before.copper),
        ("split_state", cell.split, before.split),
    ] {
        if value == old {
            continue;
        }
        let name = format!("cache_{key}");
        let mut values = page(state, &name, at, 225)?;
        values[i] = value;
        set_page(state, &name, at, &values)?;
    }
    if cell.storage != before.storage {
        for group in 0..4 {
            for (offset, prefix) in [(0, "kinds"), (1, "amounts")] {
                let name = format!("cache_storage_{prefix}_{group}");
                let mut values = page(state, &name, at, 900)?;
                for slot in 0..4 {
                    values[i * 4 + slot] = cell.storage[(group * 4 + slot) * 2 + offset];
                }
                set_page(state, &name, at, &values)?;
            }
        }
        for (stored, sign) in [(before, -1.), (cell, 1.)] {
            if stored.build != 4. {
                continue;
            }
            let counts = if p.planet == 0 {
                state.scene.get_mut("counts").unwrap().values_mut()
            } else {
                state.controller.get_mut("session").unwrap().values_mut()
            };
            let slots = &stored.storage;
            for stack in slots.chunks_exact(2) {
                ensure!(
                    stack[0].is_finite()
                        && stack[0].fract() == 0.
                        && (0. ..=50.).contains(&stack[0]),
                    "invalid storage item"
                );
                let item = stack[0] as usize;
                let index = if p.planet == 0 {
                    item
                } else if item < 32 {
                    8 + item
                } else {
                    96 + item
                };
                counts[index] = Value::Number(state::numeric(&counts[index])? + sign * stack[1]);
            }
        }
    }
    Ok(())
}
fn landing_clear(state: &State) -> Result<bool> {
    for planet in 0..2 {
        let builds = page(state, "cache_builds", planet * 289 + 144, 225)?;
        if builds[127] > 0. || builds[129] > 0. {
            return Ok(false);
        }
    }
    Ok(true)
}
fn read_power(state: &State, planet: u8) -> Result<Graph> {
    let mut graph = Graph::new();
    let name = if planet == 0 {
        "power_data"
    } else {
        "power_other"
    };
    for (page, value) in state.controller[name].values().iter().enumerate() {
        let Value::Text(text) = value else {
            anyhow::bail!("invalid power page");
        };
        for (cell, text) in text.split('|').enumerate() {
            if !text.is_empty() {
                graph.insert(
                    (page * 75 + cell).to_string(),
                    text.split(',')
                        .map(str::parse)
                        .collect::<std::result::Result<_, _>>()?,
                );
            }
        }
    }
    Ok(graph)
}
fn unlink(graph: &mut Graph, a: i64, b: i64) {
    for (node, peer) in [(a, b), (b, a)] {
        if let Some(entry) = graph.get_mut(&node.to_string()) {
            let mut kept = entry[..2].to_vec();
            kept.extend(entry[2..].iter().copied().filter(|id| *id != peer));
            *entry = kept;
        }
    }
}
fn remove_power(graph: &mut Graph, id: i64) {
    if let Some(entry) = graph.remove(&id.to_string()) {
        for peer in entry.into_iter().skip(2) {
            unlink(graph, peer, id);
        }
    }
}
