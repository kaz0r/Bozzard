//! Simulation-worker side of a Steam host. Main-thread transport supplies only
//! authenticated roster/packets; gameplay and publication happen at tick boundaries.
use super::{
    Authority, Session,
    authority::{Executor, Outcome},
    live,
    replication::{Host, Replica, requests::Action},
    shared, state,
};
use anyhow::{Context, Result, ensure};
use bozzard_app::World;
use bozzard_network::Peer;
use bozzard_scene::{
    BlueprintRuntime, NetworkFrame, SceneInstance,
    blueprint::{BlackboardValue as B, Value},
};
use std::{collections::BTreeMap, time::Duration};

pub struct HostRuntime {
    pub owner: Peer,
    host: Host,
    executor: Executor,
    now: Duration,
    members: BTreeMap<Peer, String>,
    changes: Vec<live::Change>,
    presentation: u32,
    epoch: u64,
    generation: f32,
    world_revision: u64,
    local_flight: Option<super::authority::FlightView>,
    transport_tick: Option<f32>,
    transports: Vec<super::transport::ItemMotion>,
    pub outcomes: BTreeMap<Peer, Outcome>,
    pub error: Option<String>,
}
impl HostRuntime {
    /// Called on the main thread after the chosen save/new world is ready.
    /// It neither initializes Steam nor disables the game's simulation worker.
    pub fn start(
        world: &mut World,
        owner: Peer,
        epoch: u64,
        members: BTreeMap<Peer, String>,
    ) -> Result<()> {
        ensure!(
            world.resource::<Self>().is_none(),
            "factory host already running"
        );
        let runtime = world
            .resource::<BlueprintRuntime>()
            .context("missing gameplay")?;
        ensure!(
            !matches!(
                runtime
                    .object_blackboard("controller")
                    .and_then(|b| b.get("title_open")),
                Some(B::Scalar(Value::Bool(true)))
            ),
            "Choose a world before starting the session."
        );
        let (state, player) = shared::World::from_local(state::State::capture_live(runtime)?)?;
        let session = world
            .resource::<Session>()
            .context("missing factory session")?;
        ensure!(
            session.authority != Authority::Guest,
            "a guest cannot become this world's host"
        );
        let mut saved_players = session.players.clone();
        if let Some(previous) = session.local_peer.filter(|previous| *previous != owner) {
            saved_players.remove(&previous);
        }
        let mut host = Host::new(epoch, owner, state, player, saved_players)?;
        host.members(&members)?;
        let executor = Executor::new(world.resource::<SceneInstance>().context("missing scene")?)?;
        let mut service = Self {
            owner,
            host,
            executor,
            now: Duration::ZERO,
            members,
            changes: Vec::new(),
            presentation: 0,
            epoch,
            generation: super::session_value(world, 126)?,
            world_revision: session.world_revision,
            local_flight: None,
            transport_tick: None,
            transports: Vec::new(),
            outcomes: BTreeMap::new(),
            error: None,
        };
        service.present(world)?;
        let session = world.resource_mut::<Session>().unwrap();
        session.authority = Authority::Host;
        session.bind_local_peer(owner);
        session.players = service.host.players().clone();
        world.insert_resource(service);
        Ok(())
    }
    pub fn members(&mut self, names: BTreeMap<Peer, String>) -> Result<()> {
        self.host.members(&names)?;
        self.executor
            .retain_members(&names.keys().copied().collect());
        self.outcomes.retain(|peer, _| names.contains_key(peer));
        self.members = names;
        Ok(())
    }
    pub fn receive(&mut self, peer: Peer, bytes: &[u8]) -> Result<bool> {
        ensure!(peer != self.owner, "host input uses the local game loop");
        self.host.receive_request(peer, bytes, self.now)
    }
    pub fn peers(&self) -> Vec<Peer> {
        self.members.keys().copied().collect()
    }
    pub fn version(&self, peer: Peer) -> Result<(u64, u64, u64)> {
        self.host.version(peer)
    }
    pub fn snapshot(&self, peer: Peer) -> Result<Replica> {
        let mut replica = self.host.snapshot(peer)?;
        replica.effects.rotations = self.executor.rotation_views(self.now);
        replica.effects.flights = self.flights();
        replica.effects.items = self
            .transports
            .iter()
            .filter(|item| item.planet == replica.player.position.planet)
            .cloned()
            .collect();
        replica.effects.feedback = self
            .outcomes
            .get(&peer)
            .filter(|_| replica.acknowledged > 0)
            .map(|outcome| super::replication::Feedback {
                sequence: replica.acknowledged,
                accepted: outcome.accepted,
                message: outcome.message.clone(),
            });
        replica.validate(self.owner, peer)?;
        Ok(replica)
    }
    fn update(&mut self, world: &mut World, delta: Duration) -> Result<()> {
        // A load clears rendering before publishing its validated data. Never
        // capture that intermediate empty scene or run a guest action against it.
        if [3., 4., 5.].contains(&super::session_value(world, 116)?) {
            return Ok(());
        }
        if matches!(
            world
                .resource::<BlueprintRuntime>()
                .and_then(|r| r.object_blackboard("controller"))
                .and_then(|b| b.get("title_open")),
            Some(B::Scalar(Value::Bool(true)))
        ) {
            return Ok(());
        }
        self.now += delta;
        let flight = super::session_value(world, 44)?;
        let planet = super::session_value(world, 7)? as u8;
        self.local_flight = (flight > 0.).then(|| super::authority::FlightView {
            peer: self.owner,
            destination: if flight == 1. { 1 - planet } else { planet },
            elapsed: super::session_value(world, 45).unwrap_or_default()
                + if flight == 2. { 2. } else { 0. },
        });
        let runtime = world
            .resource::<BlueprintRuntime>()
            .context("missing gameplay")?;
        let acknowledged = super::session_value(world, 118)? as u32;
        if acknowledged == self.presentation {
            self.changes.clear();
        }
        let (mut next, local) = shared::World::from_local(state::State::capture_live(runtime)?)?;
        let generation = super::session_value(world, 126)?;
        let session = world
            .resource::<Session>()
            .context("missing factory session")?;
        if generation != self.generation || session.world_revision != self.world_revision {
            self.epoch = self
                .epoch
                .checked_add(1)
                .context("world epochs exhausted")?;
            let saved = if generation != self.generation {
                BTreeMap::new()
            } else {
                session.players.clone()
            };
            self.host = Host::new(self.epoch, self.owner, next.clone(), local.clone(), saved)?;
            self.host.members(&self.members)?;
            self.executor =
                Executor::new(world.resource::<SceneInstance>().context("missing scene")?)?;
            self.outcomes.clear();
            self.changes.clear();
            self.generation = generation;
            self.world_revision = session.world_revision;
            self.transport_tick = None;
            self.transports.clear();
        }
        let tick = state::number(runtime.scene_blackboard(), "ticks")?;
        if self.transport_tick != Some(tick) {
            self.transports = super::transport::capture(runtime, tick)?;
            self.transport_tick = Some(tick);
        }
        let mut players = self.host.players().clone();
        players.insert(self.owner, local.clone());
        let current = next.clone();
        let actions: Vec<_> = self.host.drain_requests().collect();
        for action in &actions {
            // Local rotations remain controlled by Rhai; a guest may remove the
            // machine but cannot simultaneously rotate/configure that same cell.
            let player = players.get_mut(&action.peer).context("actor left lobby")?;
            let at = match &action.request.action {
                Action::Configure { at, .. }
                | Action::Collect { at }
                | Action::Feed { at }
                | Action::TakeStorage { at }
                | Action::StorageMove { at, .. }
                | Action::StorageSplit { at, .. }
                | Action::StorageDeleteKind { at, .. }
                | Action::Insert { at, .. }
                | Action::StorageTransfer { at, .. } => *at,
                _ => player.position,
            };
            let local_rotation = at.planet == local.position.planet
                && at.chunk() == local.position.chunk()
                && state::numeric(
                    &runtime.object_blackboard("controller").unwrap()["rotation_turns"].values()
                        [((at.cell().1 + 7) * 15 + at.cell().0 + 7) as usize],
                )? > 0.;
            let result = if local_rotation
                && !matches!(
                    action.request.action,
                    Action::Remove | Action::Move { .. } | Action::Gather { active: false }
                ) {
                Err(anyhow::anyhow!("Wait for the machine to finish turning."))
            } else {
                self.executor.apply(
                    &mut next,
                    action.peer,
                    player,
                    &action.request.action,
                    self.now,
                )
            };
            self.outcomes.insert(
                action.peer,
                result.unwrap_or_else(|error| Outcome {
                    accepted: false,
                    message: error.to_string(),
                }),
            );
        }
        self.executor.advance(&mut next, &mut players, self.now)?;
        // Validate/patch before acknowledging requests. A rejected gameplay action
        // still completes its sequence, but an internal publication error does not.
        if next != current {
            self.changes.extend(live::apply(
                world.resource_mut::<BlueprintRuntime>().unwrap(),
                &next,
                &local,
            )?);
        }
        self.host.publish(next, players)?;
        for action in &actions {
            self.host.complete_request(action)?;
        }
        world
            .resource_mut::<Session>()
            .context("missing factory session")?
            .players = self.host.players().clone();
        self.present(world)
    }
    fn flights(&self) -> Vec<super::authority::FlightView> {
        let mut flights = self.executor.flight_views(self.now);
        flights.extend(self.local_flight.clone());
        flights
    }
    fn present(&mut self, world: &mut World) -> Result<()> {
        if !self.changes.is_empty() {
            self.presentation = self.presentation % 16_000_000 + 1;
        }
        world.insert_resource(NetworkFrame {
            active: true,
            state: serde_json::json!({"factory_host":true,"local":self.owner,"presentation":self.presentation,
                "changes":self.changes,"rotations":self.executor.rotation_views(self.now),
                "flights":self.flights(),"members":self.host.public_members()}),
            objects: Default::default(),
        });
        Ok(())
    }
}

pub(crate) fn step(world: &mut World, delta: Duration) {
    if world
        .resource::<crate::SimulationStatus>()
        .is_some_and(|s| s.error.is_some())
    {
        return;
    }
    let Some(mut host) = world.remove_resource::<HostRuntime>() else {
        return;
    };
    if host.error.is_none() {
        if let Err(error) = host.update(world, delta) {
            host.error = Some(format!("{error:#}"));
            world.insert_resource(crate::SimulationStatus {
                error: Some(format!("Factory co-op host: {error:#}")),
            });
        }
    }
    world.insert_resource(host);
}
