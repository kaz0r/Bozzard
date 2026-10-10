//! Host-owned factory replicas. Steam supplies authenticated member identities;
//! neither a guest nor a wire packet chooses its owner, slot, name or inventory.
//! This layer handles state delivery. Gameplay requests still need to pass the
//! authoritative Rhai rules before the host publishes their resulting state.
pub mod requests;
pub mod stream;
use super::shared::{MAX_SAVED_PLAYERS, Player, Position, World};
use anyhow::{Context, Result, ensure};
use bozzard_network::{MAX_PLAYERS, Peer, chat::clean_text};
use bozzard_scene::blueprint::{BlackboardValue as B, Value};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub peer: Peer,
    pub slot: u8,
    pub name: String,
    pub position: Position,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replica {
    pub epoch: u64,
    pub revision: u64,
    pub connection: u64,
    pub acknowledged: u64,
    pub world: World,
    pub members: Vec<Member>,
    /// Only the recipient's private backpack/tool selection is sent.
    pub player: Player,
    pub effects: Effects,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effects {
    pub rotations: Vec<super::authority::RotationView>,
    pub flights: Vec<super::authority::FlightView>,
    pub feedback: Option<Feedback>,
    pub items: Vec<super::transport::ItemMotion>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feedback {
    pub sequence: u64,
    pub accepted: bool,
    pub message: String,
}
impl Replica {
    pub fn validate(&self, owner: Peer, local: Peer) -> Result<()> {
        ensure!(
            owner != 0 && local != 0 && self.epoch > 0 && self.revision > 0 && self.connection > 0,
            "invalid replica identity"
        );
        ensure!(
            !self.members.is_empty() && self.members.len() <= MAX_PLAYERS,
            "invalid player count"
        );
        let mut peers = BTreeSet::new();
        let mut slots = BTreeSet::new();
        for member in &self.members {
            ensure!(
                member.peer > 0
                    && peers.insert(member.peer)
                    && member.slot < 4
                    && slots.insert(member.slot),
                "duplicate or invalid player slot"
            );
            ensure!(
                (member.slot == 0) == (member.peer == owner),
                "host must own blue slot"
            );
            ensure!(
                !member.name.trim().is_empty() && member.name == clean_text(&member.name, 64),
                "invalid player name"
            );
            ensure!(
                self.world.discovered(member.position)?,
                "undiscovered player position"
            );
        }
        ensure!(peers.contains(&owner), "replica has no host");
        let me = self
            .members
            .iter()
            .find(|m| m.peer == local)
            .context("replica is for another player")?;
        self.player.validate()?;
        ensure!(
            self.effects.rotations.len() <= 225 * 578 && self.effects.flights.len() <= 4,
            "too many effects"
        );
        let mut turning = BTreeSet::new();
        for turn in &self.effects.rotations {
            ensure!(
                self.world.discovered(turn.at)?
                    && turning.insert(turn.at)
                    && (1..=8).contains(&turn.turns)
                    && turn.progress.is_finite()
                    && (0. ..=1.).contains(&turn.progress),
                "invalid rotation"
            );
        }
        let mut flying = BTreeSet::new();
        for flight in &self.effects.flights {
            ensure!(
                peers.contains(&flight.peer)
                    && flying.insert(flight.peer)
                    && flight.destination < 2
                    && flight.elapsed.is_finite()
                    && (0. ..=4.4).contains(&flight.elapsed),
                "invalid flight"
            );
        }
        ensure!(
            self.effects.items.len() <= 289 * 225,
            "too many item transfers"
        );
        let mut sources = BTreeSet::new();
        for item in &self.effects.items {
            item.validate()?;
            ensure!(
                item.planet == self.player.position.planet
                    && sources.insert(item.from)
                    && self.world.discovered(item.position(item.from)?)?
                    && self.world.discovered(item.position(item.to)?)?,
                "invalid transfer world"
            );
        }
        if let Some(feedback) = &self.effects.feedback {
            ensure!(
                feedback.sequence > 0
                    && feedback.sequence <= self.acknowledged
                    && feedback.message.len() <= 2048,
                "invalid feedback"
            );
        }
        ensure!(
            me.position == self.player.position,
            "private and public player positions disagree"
        );
        self.world.validate()
    }
}

pub struct Host {
    owner: Peer,
    epoch: u64,
    revision: u64,
    world: World,
    players: BTreeMap<Peer, Player>,
    members: BTreeMap<Peer, (u8, String)>,
    requests: requests::Gate,
}
impl Host {
    pub fn new(
        epoch: u64,
        owner: Peer,
        world: World,
        player: Player,
        mut saved: BTreeMap<Peer, Player>,
    ) -> Result<Self> {
        ensure!(owner > 0 && epoch > 0, "invalid host identity");
        world.validate()?;
        saved.insert(owner, player);
        ensure!(saved.len() <= MAX_SAVED_PLAYERS, "too many saved players");
        for (peer, player) in &saved {
            ensure!(*peer != 0, "invalid saved peer");
            player.validate()?;
            ensure!(
                world.discovered(player.position)?,
                "saved player is outside discovered world"
            );
        }
        Ok(Self {
            owner,
            epoch,
            revision: 1,
            world,
            players: saved,
            members: BTreeMap::new(),
            requests: requests::Gate::new(epoch, owner)?,
        })
    }
    /// Take the roster from Steam, not a peer's message. Existing guests retain
    /// their colors when someone leaves. Joining again restores their backpack.
    pub fn members(&mut self, names: &BTreeMap<Peer, String>) -> Result<()> {
        ensure!(
            names.contains_key(&self.owner)
                && names.len() <= MAX_PLAYERS
                && !names.contains_key(&0),
            "invalid authenticated lobby roster"
        );
        let new_players = names
            .keys()
            .filter(|peer| !self.players.contains_key(peer))
            .count();
        ensure!(
            self.players.len() + new_players <= MAX_SAVED_PLAYERS,
            "saved player limit reached"
        );
        self.requests.members(names.keys().copied().collect())?;
        let mut next = self.members.clone();
        next.retain(|peer, _| names.contains_key(peer));
        next.insert(self.owner, (0, String::new()));
        for (peer, name) in names {
            let slot = next.get(peer).map(|v| v.0).unwrap_or_else(|| {
                (1..4)
                    .find(|s| !next.values().any(|v| v.0 == *s))
                    .expect("bounded roster")
            });
            let name = clean_text(name, 64);
            next.insert(
                *peer,
                (
                    slot,
                    if name.trim().is_empty() {
                        "Player".into()
                    } else {
                        name
                    },
                ),
            );
            self.players.entry(*peer).or_default();
        }
        if next != self.members {
            self.revision = self.revision.checked_add(1).context("revision exhausted")?;
        }
        self.members = next;
        Ok(())
    }
    pub fn publish(&mut self, world: World, players: BTreeMap<Peer, Player>) -> Result<()> {
        world.validate()?;
        ensure!(
            players.keys().eq(self.players.keys()),
            "publication changed authenticated identities"
        );
        for player in players.values() {
            player.validate()?;
            ensure!(
                world.discovered(player.position)?,
                "publication contains an undiscovered location"
            );
        }
        if world != self.world || players != self.players {
            let revision = self.revision.checked_add(1).context("revision exhausted")?;
            self.world = world;
            self.players = players;
            self.revision = revision;
        }
        Ok(())
    }
    pub fn players(&self) -> &BTreeMap<Peer, Player> {
        &self.players
    }
    pub fn receive_request(
        &mut self,
        peer: Peer,
        bytes: &[u8],
        now: std::time::Duration,
    ) -> Result<bool> {
        self.requests.receive(peer, bytes, now)
    }
    pub fn drain_requests(&mut self) -> impl Iterator<Item = requests::Authorized> + '_ {
        self.requests.drain_tick().into_iter()
    }
    pub fn complete_request(&mut self, request: &requests::Authorized) -> Result<()> {
        let revision = self.revision.checked_add(1).context("revision exhausted")?;
        self.requests.complete(request)?;
        self.revision = revision;
        Ok(())
    }
    pub fn version(&self, peer: Peer) -> Result<(u64, u64, u64)> {
        Ok((self.epoch, self.revision, self.requests.connection(peer)?))
    }
    pub fn snapshot(&self, peer: Peer) -> Result<Replica> {
        ensure!(self.members.contains_key(&peer), "recipient left the lobby");
        let replica = Replica {
            epoch: self.epoch,
            revision: self.revision,
            connection: self.requests.connection(peer)?,
            acknowledged: self.requests.acknowledged(peer)?,
            world: self.world.clone(),
            player: self.players[&peer].clone(),
            members: self.public_members(),
            effects: Effects::default(),
        };
        replica.validate(self.owner, peer)?;
        Ok(replica)
    }
    pub fn public_members(&self) -> Vec<Member> {
        self.members
            .iter()
            .map(|(peer, (slot, name))| Member {
                peer: *peer,
                slot: *slot,
                name: name.clone(),
                position: self.players[peer].position,
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    controller: bool,
    field: String,
    /// Scalars use None. Lists change individual archive pages or numeric cells.
    index: Option<usize>,
    value: Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Update {
    Full(Replica),
    Delta {
        epoch: u64,
        base: u64,
        revision: u64,
        connection: u64,
        acknowledged: u64,
        changes: Vec<Change>,
        members: Vec<Member>,
        player: Player,
        effects: Effects,
    },
}
impl Update {
    pub fn between(previous: &Replica, next: &Replica) -> Result<Self> {
        if previous.epoch != next.epoch || previous.connection != next.connection {
            return Ok(Self::Full(next.clone()));
        }
        ensure!(
            next.revision > previous.revision,
            "world revision did not advance"
        );
        let mut changes = Vec::new();
        for (controller, before, after) in [
            (
                false,
                &previous.world.state().scene,
                &next.world.state().scene,
            ),
            (
                true,
                &previous.world.state().controller,
                &next.world.state().controller,
            ),
        ] {
            ensure!(before.keys().eq(after.keys()), "world schema changed");
            for (field, b) in after {
                let a = &before[field];
                match (a, b) {
                    (B::Scalar(a), B::Scalar(b)) => {
                        if a != b {
                            changes.push(Change {
                                controller,
                                field: field.clone(),
                                index: None,
                                value: b.clone(),
                            });
                        }
                    }
                    (
                        B::List {
                            element: ae,
                            capacity: ac,
                            values: a,
                        },
                        B::List {
                            element: be,
                            capacity: bc,
                            values: b,
                        },
                    ) => {
                        ensure!(
                            ae == be && ac == bc && a.len() == b.len(),
                            "world list schema changed"
                        );
                        for (i, (a, b)) in a.iter().zip(b).enumerate() {
                            if a != b {
                                changes.push(Change {
                                    controller,
                                    field: field.clone(),
                                    index: Some(i),
                                    value: b.clone(),
                                });
                            }
                        }
                    }
                    _ => anyhow::bail!("world field type changed"),
                }
            }
        }
        Ok(Self::Delta {
            epoch: next.epoch,
            base: previous.revision,
            revision: next.revision,
            connection: next.connection,
            acknowledged: next.acknowledged,
            changes,
            members: next.members.clone(),
            player: next.player.clone(),
            effects: next.effects.clone(),
        })
    }
    pub fn version(&self) -> (u64, u64) {
        match self {
            Self::Full(r) => (r.epoch, r.revision),
            Self::Delta {
                epoch, revision, ..
            } => (*epoch, *revision),
        }
    }
    /// Validate the complete candidate before publishing any part of it. A gap
    /// returns an error so the caller requests a fresh snapshot, never a partial world.
    pub fn apply(self, current: Option<&Replica>, owner: Peer, local: Peer) -> Result<Replica> {
        if let Some(current) = current {
            ensure!(
                self.version() > (current.epoch, current.revision),
                "stale world update"
            );
        }
        let replica = match self {
            Self::Full(replica) => replica,
            Self::Delta {
                epoch,
                base,
                revision,
                connection,
                acknowledged,
                changes,
                members,
                player,
                effects,
            } => {
                let current = current.context("delta before initial snapshot")?;
                ensure!(
                    current.epoch == epoch
                        && current.revision == base
                        && revision > base
                        && connection == current.connection
                        && acknowledged >= current.acknowledged,
                    "missing world revision; request a snapshot"
                );
                ensure!(changes.len() <= 32_768, "too many world changes");
                let mut seen = BTreeSet::new();
                let mut state = current.world.state().clone();
                for change in changes {
                    ensure!(
                        seen.insert((change.controller, change.field.clone(), change.index)),
                        "duplicate world change"
                    );
                    let board = if change.controller {
                        &mut state.controller
                    } else {
                        &mut state.scene
                    };
                    let field = board
                        .get_mut(&change.field)
                        .context("unknown world field")?;
                    let value = match (field, change.index) {
                        (B::Scalar(value), None) => value,
                        (B::List { values, .. }, Some(index)) => values
                            .get_mut(index)
                            .context("world list index out of range")?,
                        _ => anyhow::bail!("world container type changed"),
                    };
                    ensure!(
                        value.kind() == change.value.kind(),
                        "world value type changed"
                    );
                    *value = change.value;
                }
                Replica {
                    epoch,
                    revision,
                    connection,
                    acknowledged,
                    world: World::from_canonical(state)?,
                    members,
                    player,
                    effects,
                }
            }
        };
        replica.validate(owner, local)?;
        if let Some(current) = current.filter(|r| r.epoch == replica.epoch) {
            ensure!(
                replica.connection >= current.connection,
                "stale player connection"
            );
            if replica.connection == current.connection {
                ensure!(
                    replica.acknowledged >= current.acknowledged,
                    "action acknowledgement went backwards"
                );
            }
        }
        Ok(replica)
    }
}
