//! Guest simulation-worker bridge. Steam pumps fragments on the main thread;
//! complete validated replicas are projected into this player's local view here.
use super::{
    Authority, Session, live,
    replication::{
        Replica,
        requests::{Action, Request},
        stream::Receiver,
    },
    shared::{Player, Position},
    state::{self, State},
};
use anyhow::{Context, Result, ensure};
use bozzard_app::World;
use bozzard_network::Peer;
use bozzard_scene::{BlueprintRuntime, NetworkFrame, NetworkOutbox, blueprint::Value};
use std::{collections::VecDeque, time::Duration};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Waiting,
    Clear,
    Restore,
    Live,
}
pub struct GuestRuntime {
    pub owner: Peer,
    pub local: Peer,
    receiver: Receiver,
    phase: Phase,
    reset: Option<Replica>,
    shown: Option<Replica>,
    pending: VecDeque<Request>,
    sequence: u64,
    sent: u64,
    presentation: u32,
    changes: Vec<live::Change>,
    resident: Vec<bool>,
    destination: Option<super::shared::Position>,
    pub error: Option<String>,
}
impl GuestRuntime {
    pub fn start(world: &mut World, owner: Peer, local: Peer) -> Result<()> {
        ensure!(
            world.resource::<Self>().is_none()
                && world.resource::<super::host::HostRuntime>().is_none(),
            "factory session already active"
        );
        let receiver = Receiver::new(owner, local)?;
        let session = world
            .resource_mut::<Session>()
            .context("missing factory session")?;
        session.authority = Authority::Guest;
        session.local_peer = Some(local);
        session.pending = None;
        if let Some(job) = session.job.take() {
            job.into_inner().unwrap().cancel();
        }
        session.players.clear();
        super::set_session(world, 122, 2.)?;
        super::set_session(world, 116, 0.)?;
        super::set_session(world, 125, 0.)?;
        if let Some(outbox) = world.resource_mut::<NetworkOutbox>() {
            outbox.clear();
        }
        let service = Self {
            owner,
            local,
            receiver,
            phase: Phase::Waiting,
            reset: None,
            shown: None,
            pending: VecDeque::new(),
            sequence: 0,
            sent: 0,
            presentation: 0,
            changes: Vec::new(),
            resident: vec![false; 289],
            destination: None,
            error: None,
        };
        service.present(world)?;
        world.insert_resource(service);
        Ok(())
    }
    pub fn receive(&mut self, peer: Peer, bytes: &[u8], now: Duration) -> Result<bool> {
        self.receiver.receive(peer, bytes, now)
    }
    pub fn expire(&mut self, now: Duration) -> bool {
        self.receiver.expire(now)
    }
    pub fn current(&self) -> Option<&Replica> {
        self.receiver.current()
    }
    pub fn next_request(&self) -> Result<Option<Vec<u8>>> {
        self.pending
            .iter()
            .find(|r| r.sequence > self.sent)
            .map(Request::encode)
            .transpose()
    }
    pub fn mark_sent(&mut self, sequence: u64) -> Result<()> {
        ensure!(
            self.pending
                .iter()
                .find(|r| r.sequence > self.sent)
                .is_some_and(|r| r.sequence == sequence),
            "out-of-order send completion"
        );
        self.sent = sequence;
        Ok(())
    }
    pub fn retry_pending(&mut self) {
        if let Some(first) = self.pending.front() {
            self.sent = first.sequence - 1;
        }
    }
    pub fn has_pending_requests(&self) -> bool {
        !self.pending.is_empty()
    }
    pub fn ready(&self) -> bool {
        self.phase == Phase::Live && self.error.is_none()
    }
    fn update(&mut self, world: &mut World) -> Result<()> {
        // Rendering ticks outnumber network updates. Keep the validated world in
        // the receiver instead of cloning its archive pages every local frame.
        let reference = self.reset.as_ref().or(self.shown.as_ref());
        let incoming = self
            .receiver
            .current()
            .filter(|next| {
                reference.is_none_or(|old| {
                    old.epoch != next.epoch
                        || old.connection != next.connection
                        || (self.phase == Phase::Live && old.revision != next.revision)
                })
            })
            .cloned();
        if let Some(next) = &incoming {
            let reference = self.reset.as_ref().or(self.shown.as_ref());
            if reference
                .is_none_or(|old| old.epoch != next.epoch || old.connection != next.connection)
            {
                self.pending.clear();
                self.sequence = next.acknowledged;
                self.sent = next.acknowledged;
                self.reset = Some(next.clone());
                self.phase = Phase::Clear;
                self.changes.clear();
                self.destination = None;
                self.resident.fill(false);
                super::set_session(world, 119, 0.)?;
                if let Some(outbox) = world.resource_mut::<NetworkOutbox>() {
                    outbox.clear();
                }
            }
        }
        match self.phase {
            Phase::Waiting => (),
            Phase::Clear if super::session_value(world, 119)? == 1. => {
                let replica = self.reset.as_ref().context("missing guest reset")?;
                let mut state = replica.world.project(&replica.player)?;
                let generation = super::session_value(world, 126)?;
                let session = state.controller.get_mut("session").unwrap().values_mut();
                session[119] = Value::Number(2.);
                session[122] = Value::Number(2.);
                session[126] = Value::Number(generation);
                world
                    .resource_mut::<BlueprintRuntime>()
                    .context("missing gameplay")?
                    .patch_blackboards(
                        &state.scene,
                        &[("controller".into(), state.controller)].into(),
                    )?;
                self.phase = Phase::Restore;
            }
            Phase::Restore if super::session_value(world, 119)? == 3. => {
                self.shown = self.reset.take();
                self.phase = Phase::Live;
            }
            Phase::Live => {
                if super::session_value(world, 118)? as u32 == self.presentation {
                    self.changes.clear();
                    self.destination = None;
                }
                if let Some(next) = incoming.filter(|next| {
                    self.shown
                        .as_ref()
                        .is_none_or(|old| old.revision != next.revision || old.epoch != next.epoch)
                }) {
                    self.pending.retain(|r| r.sequence > next.acknowledged);
                    let runtime = world
                        .resource::<BlueprintRuntime>()
                        .context("missing gameplay")?;
                    let before = State::capture_live(runtime)?;
                    let local = Player::capture(&before)?;
                    if local.position.planet != next.player.position.planet {
                        self.reset = Some(next);
                        self.phase = Phase::Clear;
                        self.changes.clear();
                        self.destination = None;
                        self.resident.fill(false);
                        super::set_session(world, 119, 0.)?;
                    } else {
                        let mut player = next.player.clone();
                        // Tool choice is local presentation; an older snapshot must
                        // not reset a freshly selected action bar while typing/building.
                        player.selected = local.selected;
                        player.direction = local.direction;
                        player.bar = local.bar;
                        player.bar_slots = local.bar_slots;
                        let destination = player.position;
                        player.position = local.position;
                        self.changes.extend(live::apply(
                            world.resource_mut::<BlueprintRuntime>().unwrap(),
                            &next.world,
                            &player,
                        )?);
                        self.destination = Some(destination);
                        if let Some(feedback) = &next.effects.feedback
                            && self
                                .shown
                                .as_ref()
                                .and_then(|r| r.effects.feedback.as_ref())
                                != Some(feedback)
                        {
                            super::notice(world, &feedback.message)?;
                        }
                        super::set_session(world, 124, 0.)?;
                        self.shown = Some(next);
                    }
                }
            }
            _ => (),
        }
        let requests = world
            .resource_mut::<NetworkOutbox>()
            .map(|outbox| outbox.drain().collect::<Vec<_>>())
            .unwrap_or_default();
        if self.phase == Phase::Live {
            let replica = self.shown.as_ref().context("guest has no world")?;
            let mut position_requested = false;
            for request in requests {
                if !request.kind.starts_with("factory.") {
                    continue;
                }
                let action = Action::from_script(request)?;
                position_requested |= matches!(action, Action::Move { .. } | Action::Point { .. });
                if self.pending.len() >= 32 {
                    super::notice(world, "Waiting for the host to process your actions…")?;
                    break;
                }
                self.sequence = self
                    .sequence
                    .checked_add(1)
                    .context("request sequence exhausted")?;
                self.pending.push_back(Request {
                    epoch: replica.epoch,
                    connection: replica.connection,
                    sequence: self.sequence,
                    action,
                });
            }
            if self.destination.is_some() || position_requested {
                // Replay only unacknowledged positions on the newest host base.
                // This also handles a partial acknowledgement without snapping
                // backward or applying an accepted movement twice.
                self.destination = Some(predicted_position(replica, self.local, &self.pending)?);
            }
            let runtime = world.resource::<BlueprintRuntime>().unwrap();
            let resident =
                state::values(runtime.object_blackboard("controller").unwrap(), "resident")?
                    .iter()
                    .map(|v| state::numeric(v).map(|n| n > 0.))
                    .collect::<Result<Vec<_>>>()?;
            let added: Vec<_> = resident
                .iter()
                .zip(&self.resident)
                .enumerate()
                .filter_map(|(i, (new, old))| (*new && !*old).then_some(i))
                .collect();
            self.changes.extend(live::resident_items(runtime, &added)?);
            self.resident = resident;
            if !self.changes.is_empty() || self.destination.is_some() {
                self.presentation = self.presentation % 16_000_000 + 1;
            }
        }
        self.present(world)
    }
    fn present(&self, world: &mut World) -> Result<()> {
        let replica = self.reset.as_ref().or(self.shown.as_ref());
        world.insert_resource(NetworkFrame{active:true,objects:Default::default(),state:serde_json::json!({
            "factory_guest":true,"local":self.local,"owner":self.owner,
            "phase":match self.phase {Phase::Waiting=>"waiting",Phase::Clear=>"clear",Phase::Restore=>"restore",Phase::Live=>"live"},
            "presentation":self.presentation,"changes":self.changes,"destination":self.destination,
            "rotations":replica.map(|r|&r.effects.rotations).cloned().unwrap_or_default(),
            "items":replica.map(|r|&r.effects.items).cloned().unwrap_or_default(),
            "flights":replica.map(|r|&r.effects.flights).cloned().unwrap_or_default(),
            "members":replica.map(|r|&r.members).cloned().unwrap_or_default()
        })});
        Ok(())
    }
}

/// Predict presentation only. Machines, inventory, discovery and rocket travel
/// still wait for the host. Unknown terrain is shown once its host data arrives.
fn predicted_position(
    replica: &Replica,
    local: Peer,
    pending: &VecDeque<Request>,
) -> Result<Position> {
    let mut position = replica.player.position;
    if replica
        .effects
        .flights
        .iter()
        .any(|flight| flight.peer == local)
    {
        return Ok(position);
    }
    let mut visible = position;
    for request in pending {
        match request.action {
            Action::Move { x, z } => {
                let next = Position {
                    x: position.x + i16::from(x),
                    z: position.z + i16::from(z),
                    ..position
                };
                if next.validate().is_ok() {
                    position = next;
                }
            }
            Action::Point { at } if position.near(at, 90) && replica.world.discovered(at)? => {
                position = at
            }
            // Don't predict commands past a launch until the host accepts or
            // rejects it; a confirmed flight locks movement on either planet.
            Action::Travel => break,
            _ => (),
        }
        if replica.world.discovered(position)? {
            visible = position;
        }
    }
    Ok(visible)
}

pub(crate) fn step(world: &mut World) {
    if world
        .resource::<crate::SimulationStatus>()
        .is_some_and(|s| s.error.is_some())
    {
        return;
    }
    let Some(mut service) = world.remove_resource::<GuestRuntime>() else {
        return;
    };
    if service.error.is_none()
        && let Err(error) = service.update(world)
    {
        service.error = Some(format!("{error:#}"));
        world.insert_resource(crate::SimulationStatus {
            error: Some(format!("Factory guest: {error:#}")),
        });
    }
    world.insert_resource(service);
}
