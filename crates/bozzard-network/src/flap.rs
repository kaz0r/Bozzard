//! Reference game rules: four independent birds, shared pipes, individual scores.
//! Only the host decides collisions and scoring; clients predict vertical motion.
use crate::*;
use bozzard_ecs::{Entity, World};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const COUNTDOWN_TICKS: u16 = 5 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Phase {
    #[default]
    Lobby,
    Countdown {
        ticks_remaining: u16,
    },
    Playing,
    Finished,
}
impl Phase {
    pub fn round_active(self) -> bool {
        matches!(self, Self::Countdown { .. } | Self::Playing)
    }
    pub fn countdown_seconds(self) -> Option<u16> {
        match self {
            Self::Countdown { ticks_remaining } => Some(ticks_remaining.div_ceil(60)),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bird {
    pub slot: u8,
    pub y: f32,
    pub velocity: f32,
    pub alive: bool,
    pub score: u32,
    pub input_ack: u64,
}
impl Bird {
    pub fn new(slot: u8) -> Self {
        Self {
            slot,
            y: 0.65,
            velocity: 0.,
            alive: true,
            score: 0,
            input_ack: 0,
        }
    }
    pub fn x(&self) -> f32 {
        -5. + f32::from(self.slot) * 0.6
    }
    pub fn predict(&mut self, flap: bool) {
        if !self.alive {
            return;
        }
        if flap {
            self.velocity = 6.5;
        }
        self.velocity -= 22. * DT;
        self.y += self.velocity * DT;
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipe {
    pub x: f32,
    pub gap: f32,
    pub cycle: u32,
}
fn pipes() -> [Pipe; 3] {
    [
        Pipe {
            x: 2.,
            gap: 0.,
            cycle: 0,
        },
        Pipe {
            x: 11.,
            gap: 1.9,
            cycle: 0,
        },
        Pipe {
            x: 20.,
            gap: -1.9,
            cycle: 0,
        },
    ]
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputFrame {
    pub sequence: u64,
    pub flap: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub round: u64,
    pub tick: u64,
    /// Every changed bird since this peer's acknowledgement, not since last send.
    pub base: u64,
    pub phase: Phase,
    /// Complete relevance set. Missing IDs are despawned even when a delta was lost.
    pub roster: Vec<Peer>,
    pub birds: BTreeMap<Peer, Bird>,
    pub pipes: [Pipe; 3],
}
#[derive(Default)]
struct Connection {
    ack: u64,
    sent: u64,
    inputs: BTreeMap<u64, bool>,
}
pub struct Host {
    pub owner: Peer,
    pub round: u64,
    pub phase: Phase,
    pub world: World,
    pub pipes: [Pipe; 3],
    entities: BTreeMap<Peer, Entity>,
    peers: BTreeMap<Peer, Connection>,
}
impl Host {
    pub fn new(owner: Peer) -> Self {
        let mut host = Self {
            owner,
            round: 0,
            phase: Phase::Lobby,
            world: World::new(),
            pipes: pipes(),
            entities: BTreeMap::new(),
            peers: BTreeMap::new(),
        };
        host.join(owner).expect("empty host accepts owner");
        host
    }
    pub fn join(&mut self, peer: Peer) -> Result<()> {
        if self.entities.contains_key(&peer) {
            return Ok(());
        }
        ensure!(
            peer != 0 && !self.phase.round_active() && self.entities.len() < MAX_PLAYERS,
            "lobby full or round in progress"
        );
        self.world.advance_change_tick();
        let slot = (0..MAX_PLAYERS as u8)
            .find(|slot| {
                self.entities
                    .values()
                    .all(|e| self.world.get::<Bird>(*e).unwrap().slot != *slot)
            })
            .unwrap();
        let e = self.world.spawn();
        self.world.insert(e, Bird::new(slot))?;
        self.entities.insert(peer, e);
        self.peers.insert(peer, Connection::default());
        Ok(())
    }
    pub fn leave(&mut self, peer: Peer) {
        self.world.advance_change_tick();
        if let Some(e) = self.entities.remove(&peer) {
            let _ = self.world.despawn(e);
        }
        self.peers.remove(&peer);
        if matches!(self.phase, Phase::Countdown { .. }) && self.entities.len() < 2 {
            self.phase = Phase::Lobby;
        }
    }
    pub fn members(&self) -> Vec<Peer> {
        self.entities.keys().copied().collect()
    }
    pub fn bird(&self, peer: Peer) -> Option<Bird> {
        self.entities
            .get(&peer)
            .and_then(|e| self.world.get::<Bird>(*e))
            .copied()
    }
    pub fn start(&mut self, actor: Peer) -> Result<()> {
        ensure!(
            actor == self.owner && self.entities.contains_key(&actor),
            "only the original lobby host can start"
        );
        ensure!(!self.phase.round_active(), "round already running");
        ensure!(self.entities.len() >= 2, "invite at least one friend first");
        self.round += 1;
        self.phase = Phase::Countdown {
            ticks_remaining: COUNTDOWN_TICKS,
        };
        self.pipes = pipes();
        self.world.advance_change_tick();
        for (_, mut bird) in self.world.query_mut::<Bird>() {
            *bird = Bird::new(bird.slot);
        }
        for peer in self.peers.values_mut() {
            *peer = Connection::default();
        }
        Ok(())
    }
    pub fn receive(&mut self, sender: Peer, message: Message) -> Result<()> {
        let bird = self
            .bird(sender)
            .ok_or_else(|| anyhow::anyhow!("sender outside lobby"))?;
        let peer = self.peers.get_mut(&sender).unwrap();
        let Message::Input { round, frames, ack } = message else {
            anyhow::bail!("clients may only send input");
        };
        ensure!(
            round == self.round && frames.len() <= 120,
            "stale round or input budget exceeded"
        );
        ensure!(
            ack <= peer.sent,
            "acknowledgement beyond last sent snapshot"
        );
        peer.ack = peer.ack.max(ack);
        if self.phase == Phase::Playing {
            for frame in frames {
                if frame.sequence > bird.input_ack
                    && frame.sequence <= bird.input_ack.saturating_add(120)
                {
                    peer.inputs.entry(frame.sequence).or_insert(frame.flap);
                }
            }
        }
        Ok(())
    }
    pub fn step(&mut self) {
        self.world.advance_change_tick();
        if let Phase::Countdown { ticks_remaining } = self.phase {
            self.phase = if ticks_remaining <= 1 {
                Phase::Playing
            } else {
                Phase::Countdown {
                    ticks_remaining: ticks_remaining - 1,
                }
            };
            return;
        }
        if self.phase != Phase::Playing {
            return;
        }
        let old = self.pipes;
        for (index, pipe) in self.pipes.iter_mut().enumerate() {
            pipe.x -= 3.2 * DT;
            if pipe.x < -13. {
                pipe.x += 27.;
                pipe.cycle += 1;
                pipe.gap = (((pipe.cycle as usize + index) % 5) as f32 - 2.) * 0.85;
            }
        }
        for (&id, &entity) in &self.entities {
            let mut bird = self.world.get_mut::<Bird>(entity).unwrap();
            let input = self.peers.get_mut(&id).unwrap().inputs.pop_first();
            if !bird.alive {
                if let Some((seq, _)) = input {
                    bird.input_ack = seq;
                }
                continue;
            }
            let flap = input.is_some_and(|(_, flap)| flap);
            if let Some((seq, _)) = input {
                bird.input_ack = seq;
            }
            bird.predict(flap);
            if bird.y.abs() > 4.65
                || self
                    .pipes
                    .iter()
                    .any(|p| (p.x - bird.x()).abs() < 1.0 && (bird.y - p.gap).abs() > 2.15)
            {
                bird.alive = false;
            }
            if bird.alive {
                for (before, after) in old.iter().zip(&self.pipes) {
                    if before.x + 0.6 >= bird.x() - 0.4 && after.x + 0.6 < bird.x() - 0.4 {
                        bird.score += 1;
                    }
                }
            }
        }
        if self.world.query::<Bird>().all(|(_, bird)| !bird.alive) {
            self.phase = Phase::Finished;
        }
    }
    pub fn snapshot(&mut self, recipient: Peer) -> Result<Snapshot> {
        let peer = self
            .peers
            .get_mut(&recipient)
            .ok_or_else(|| anyhow::anyhow!("recipient outside interest set"))?;
        let base = peer.ack;
        let tick = self.world.change_tick();
        peer.sent = tick;
        let birds = self
            .entities
            .iter()
            .filter(|(_, e)| self.world.is_changed_since::<Bird>(**e, base))
            .map(|(id, e)| (*id, *self.world.get::<Bird>(*e).unwrap()))
            .collect();
        Ok(Snapshot {
            round: self.round,
            tick,
            base,
            phase: self.phase,
            roster: self.members(),
            birds,
            pipes: self.pipes,
        })
    }
}

#[derive(Clone, Default)]
pub struct Replica {
    pub round: u64,
    pub tick: u64,
    pub phase: Phase,
    pub birds: BTreeMap<Peer, Bird>,
    pub pipes: Option<[Pipe; 3]>,
    previous: BTreeMap<Peer, Bird>,
    previous_pipes: Option<[Pipe; 3]>,
    pending: VecDeque<InputFrame>,
    sequence: u64,
    pub predicted: Option<Bird>,
}
impl Replica {
    pub fn apply(
        &mut self,
        owner: Peer,
        sender: Peer,
        local: Peer,
        snapshot: Snapshot,
    ) -> Result<bool> {
        ensure!(owner == sender, "only host snapshots are authoritative");
        if snapshot.round < self.round || snapshot.tick <= self.tick {
            return Ok(false);
        }
        ensure!(snapshot.base <= self.tick, "missing snapshot baseline");
        if let Phase::Countdown { ticks_remaining } = snapshot.phase {
            ensure!(
                (1..=COUNTDOWN_TICKS).contains(&ticks_remaining),
                "invalid countdown"
            );
        }
        let roster: BTreeSet<_> = snapshot.roster.iter().copied().collect();
        ensure!(
            !roster.contains(&0)
                && roster.len() == snapshot.roster.len()
                && roster.len() <= MAX_PLAYERS
                && roster.contains(&owner),
            "invalid roster"
        );
        ensure!(
            snapshot.birds.iter().all(|(id, b)| roster.contains(id)
                && b.slot < MAX_PLAYERS as u8
                && b.y.is_finite()
                && b.y.abs() < 100.
                && b.velocity.is_finite()
                && b.velocity.abs() < 100.),
            "invalid bird state"
        );
        ensure!(
            snapshot.pipes.iter().all(|p| p.x.is_finite()
                && (-14. ..=30.).contains(&p.x)
                && p.gap.is_finite()
                && p.gap.abs() <= 2.),
            "invalid pipe state"
        );
        let new_round = snapshot.round != self.round;
        let mut next = if snapshot.base == 0 || new_round {
            BTreeMap::new()
        } else {
            self.birds.clone()
        };
        next.retain(|id, _| roster.contains(id));
        next.extend(snapshot.birds);
        ensure!(next.len() == roster.len(), "incomplete initial snapshot");
        ensure!(
            next.values().map(|b| b.slot).collect::<BTreeSet<_>>().len() == next.len(),
            "duplicate player slot"
        );
        self.previous = if new_round {
            next.clone()
        } else {
            self.birds.clone()
        };
        self.previous_pipes = if new_round {
            Some(snapshot.pipes)
        } else {
            self.pipes
        };
        if new_round {
            self.pending.clear();
            self.sequence = 0;
        }
        self.birds = next;
        self.pipes = Some(snapshot.pipes);
        self.tick = snapshot.tick;
        self.round = snapshot.round;
        self.phase = snapshot.phase;
        self.predicted = self.birds.get(&local).copied();
        if let Some(bird) = &mut self.predicted {
            self.pending.retain(|f| f.sequence > bird.input_ack);
            for frame in &self.pending {
                if self.phase == Phase::Playing {
                    bird.predict(frame.flap);
                }
            }
        }
        Ok(true)
    }
    pub fn input(&mut self, flap: bool) {
        if self.phase != Phase::Playing || self.pending.len() >= 120 {
            return;
        }
        self.sequence += 1;
        self.pending.push_back(InputFrame {
            sequence: self.sequence,
            flap,
        });
        if let Some(bird) = &mut self.predicted {
            bird.predict(flap);
        }
    }
    pub fn message(&self) -> Message {
        Message::Input {
            round: self.round,
            frames: self.pending.iter().copied().collect(),
            ack: self.tick,
        }
    }
    /// Remote entities render one snapshot behind. Recycled pipes snap instead of
    /// interpolating backwards across the whole arena; the local bird is predicted.
    pub fn render_bird(&self, peer: Peer, local: Peer, alpha: f32) -> Option<Bird> {
        if peer == local {
            return self.predicted;
        }
        let mut bird = *self.birds.get(&peer)?;
        if let Some(before) = self.previous.get(&peer) {
            bird.y = before.y + (bird.y - before.y) * alpha.clamp(0., 1.);
        }
        Some(bird)
    }
    pub fn render_pipes(&self, alpha: f32) -> Option<[Pipe; 3]> {
        let mut result = self.pipes?;
        if let Some(before) = self.previous_pipes {
            for (p, old) in result.iter_mut().zip(before) {
                if p.cycle == old.cycle {
                    p.x = old.x + (p.x - old.x) * alpha.clamp(0., 1.);
                }
            }
        }
        Some(result)
    }
}
