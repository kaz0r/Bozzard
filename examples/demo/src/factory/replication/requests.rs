//! Authenticated gameplay intent. This gate bounds, deduplicates and orders input;
//! the game executor must still check range, unlocks, cost, power and capacity.
//! No request can replace a world, inventory, peer identity or save file.
use super::super::shared::Position;
use anyhow::{Context, Result, ensure};
use bozzard_network::{MAX_PLAYERS, Peer};
use bozzard_scene::NetworkRequest;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Duration,
};

pub const MAX_PACKET: usize = 4096;
const MAX_PENDING: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum Action {
    Point {
        at: Position,
    },
    InventoryClear,
    Move {
        x: i8,
        z: i8,
    },
    Discover {
        planet: u8,
        x: i8,
        z: i8,
    },
    Place {
        kind: u8,
        direction: u8,
    },
    Remove,
    Rotate,
    Gather {
        active: bool,
    },
    Craft {
        recipe: u8,
        count: u16,
    },
    Deliver,
    Travel,
    Configure {
        at: Position,
        recipe: u8,
    },
    Collect {
        at: Position,
    },
    Feed {
        at: Position,
    },
    TakeStorage {
        at: Position,
    },
    StorageMove {
        at: Position,
        from: u8,
        to: u8,
    },
    StorageSplit {
        at: Position,
        slot: u8,
    },
    StorageDeleteKind {
        at: Position,
        slot: u8,
    },
    Insert {
        at: Position,
        slot: u8,
        amount: u16,
    },
    StorageTransfer {
        at: Position,
        from_storage: bool,
        slot: u8,
        amount: u16,
    },
    InventoryMove {
        from: u8,
        to: u8,
        amount: u16,
    },
    InventoryDiscard {
        slot: u8,
        amount: u16,
    },
    InventorySplit {
        slot: u8,
    },
    Wire {
        from: Position,
        to: Position,
    },
    Unwire {
        from: Position,
        to: Option<Position>,
    },
}
impl Action {
    pub fn validate(&self) -> Result<()> {
        let stack = |slot: u8, count: u8, amount: u16| -> Result<()> {
            ensure!(
                slot < count && (1..=100).contains(&amount),
                "invalid inventory transfer"
            );
            Ok(())
        };
        let recipe = |recipe: u8| -> Result<()> {
            ensure!((1..=53).contains(&recipe), "invalid recipe");
            Ok(())
        };
        match self {
            Self::Point { at } => at.validate()?,
            Self::Move { x, z } => ensure!(
                (-1..=1).contains(x) && (-1..=1).contains(z) && (*x != 0 || *z != 0),
                "invalid movement step"
            ),
            Self::Discover { planet, x, z } => {
                ensure!(*planet < 2, "invalid planet");
                let edge = if *planet == 0 { 8 } else { 6 };
                ensure!(
                    (-edge..=edge).contains(x) && (-edge..=edge).contains(z),
                    "invalid discovery coordinates"
                );
            }
            Self::Place { kind, direction } => ensure!(
                (1..=29).contains(kind) && *kind != 10 && *direction < 4,
                "invalid machine"
            ),
            Self::Craft {
                recipe: kind,
                count,
            } => {
                recipe(*kind)?;
                ensure!((1..=100).contains(count), "invalid craft count");
            }
            Self::Configure { at, recipe: kind } => {
                at.validate()?;
                recipe(*kind)?;
            }
            Self::Collect { at } | Self::Feed { at } | Self::TakeStorage { at } => at.validate()?,
            Self::StorageMove { at, from, to } => {
                at.validate()?;
                ensure!(
                    *from < 16 && *to < 16 && from != to,
                    "invalid storage slots"
                );
            }
            Self::StorageSplit { at, slot } | Self::StorageDeleteKind { at, slot } => {
                at.validate()?;
                ensure!(*slot < 16, "invalid storage slot");
            }
            Self::Insert { at, slot, amount } => {
                at.validate()?;
                stack(*slot, 25, *amount)?;
            }
            Self::StorageTransfer {
                at,
                from_storage,
                slot,
                amount,
            } => {
                at.validate()?;
                stack(*slot, if *from_storage { 16 } else { 25 }, *amount)?;
            }
            Self::InventoryMove { from, to, amount } => {
                stack(*from, 25, *amount)?;
                stack(*to, 25, *amount)?;
                ensure!(from != to, "same inventory slot");
            }
            Self::InventoryDiscard { slot, amount } => stack(*slot, 25, *amount)?,
            Self::InventorySplit { slot } => ensure!(*slot < 25, "invalid inventory slot"),
            Self::Wire { from, to } => {
                from.validate()?;
                to.validate()?;
                ensure!(
                    from.planet == to.planet && from != to,
                    "invalid wire endpoints"
                );
            }
            Self::Unwire { from, to } => {
                from.validate()?;
                if let Some(to) = to {
                    to.validate()?;
                    ensure!(
                        from.planet == to.planet && from != to,
                        "invalid wire endpoints"
                    );
                }
            }
            Self::Remove
            | Self::Rotate
            | Self::Gather { .. }
            | Self::Deliver
            | Self::Travel
            | Self::InventoryClear => (),
        }
        Ok(())
    }
    pub fn from_script(request: NetworkRequest) -> Result<Self> {
        ensure!(
            request.owner == "controller",
            "only the factory controller can issue gameplay requests"
        );
        let kind = request
            .kind
            .strip_prefix("factory.")
            .context("unknown game request")?;
        let mut payload = request.payload;
        if ["remove", "rotate", "deliver", "travel", "inventory_clear"].contains(&kind) {
            ensure!(
                payload.as_object().is_some_and(|v| v.is_empty()),
                "unexpected action fields"
            );
            payload = serde_json::Value::Null;
        }
        let action: Self = serde_json::from_value(serde_json::json!({kind:payload}))?;
        action.validate()?;
        Ok(action)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub epoch: u64,
    pub connection: u64,
    pub sequence: u64,
    pub action: Action,
}
impl Request {
    pub fn encode(&self) -> Result<Vec<u8>> {
        ensure!(
            self.epoch > 0 && self.connection > 0 && self.sequence > 0,
            "request has no active connection"
        );
        self.action.validate()?;
        let bytes = serde_json::to_vec(self)?;
        ensure!(bytes.len() <= MAX_PACKET, "request too large");
        Ok(bytes)
    }
}
#[derive(Clone, Debug)]
pub struct Authorized {
    pub peer: Peer,
    pub request: Request,
}
#[derive(Clone)]
struct Input {
    connection: u64,
    queued: u64,
    completed: u64,
    tokens: f64,
    refill: Duration,
}
pub struct Gate {
    epoch: u64,
    owner: Peer,
    next_connection: u64,
    peers: BTreeMap<Peer, Input>,
    pending: VecDeque<Authorized>,
}
impl Gate {
    pub fn new(epoch: u64, owner: Peer) -> Result<Self> {
        ensure!(epoch != 0 && owner != 0, "invalid request gate");
        Ok(Self {
            epoch,
            owner,
            next_connection: 1,
            peers: BTreeMap::new(),
            pending: VecDeque::new(),
        })
    }
    pub fn members(&mut self, members: BTreeSet<Peer>) -> Result<()> {
        ensure!(
            members.contains(&self.owner) && !members.contains(&0) && members.len() <= MAX_PLAYERS,
            "invalid request roster"
        );
        let added = members
            .iter()
            .filter(|p| !self.peers.contains_key(p))
            .count();
        self.next_connection
            .checked_add(added as u64)
            .context("connection identifiers exhausted")?;
        self.peers.retain(|peer, _| members.contains(peer));
        self.pending.retain(|p| members.contains(&p.peer));
        for peer in members {
            self.peers.entry(peer).or_insert_with(|| {
                let connection = self.next_connection;
                self.next_connection += 1;
                Input {
                    connection,
                    queued: 0,
                    completed: 0,
                    tokens: 32.,
                    refill: Duration::ZERO,
                }
            });
        }
        Ok(())
    }
    pub fn connection(&self, peer: Peer) -> Result<u64> {
        Ok(self.peers.get(&peer).context("peer left lobby")?.connection)
    }
    pub fn acknowledged(&self, peer: Peer) -> Result<u64> {
        Ok(self.peers.get(&peer).context("peer left lobby")?.completed)
    }
    pub fn receive(&mut self, peer: Peer, bytes: &[u8], now: Duration) -> Result<bool> {
        let input = self
            .peers
            .get_mut(&peer)
            .context("sender is not a lobby member")?;
        ensure!(bytes.len() <= MAX_PACKET, "request too large");
        let request: Request = serde_json::from_slice(bytes)?;
        ensure!(
            request.epoch == self.epoch
                && request.connection == input.connection
                && request.sequence > 0,
            "stale world or player connection"
        );
        if request.sequence <= input.queued {
            return Ok(false);
        }
        ensure!(
            input.queued.checked_add(1) == Some(request.sequence),
            "missing input sequence"
        );
        request.action.validate()?;
        ensure!(
            input.queued - input.completed < MAX_PENDING as u64,
            "too many unprocessed actions"
        );
        input.tokens =
            (input.tokens + now.saturating_sub(input.refill).as_secs_f64() * 120.).min(32.);
        input.refill = now.max(input.refill);
        ensure!(input.tokens >= 1., "gameplay request rate exceeded");
        input.tokens -= 1.;
        input.queued = request.sequence;
        self.pending.push_back(Authorized { peer, request });
        Ok(true)
    }
    pub fn drain(&mut self) -> impl Iterator<Item = Authorized> + '_ {
        self.pending.drain(..)
    }
    /// Packet batching must not turn valid consecutive steps into same-tick
    /// speed-limit rejections. Process one move per player per simulation tick,
    /// retaining that player's subsequent commands in their original order.
    pub fn drain_tick(&mut self) -> Vec<Authorized> {
        let mut moved = BTreeSet::new();
        let mut deferred = BTreeSet::new();
        let mut ready = Vec::new();
        self.pending.retain(|action| {
            if deferred.contains(&action.peer) {
                return true;
            }
            if matches!(action.request.action, Action::Move { .. }) && !moved.insert(action.peer) {
                deferred.insert(action.peer);
                return true;
            }
            ready.push(action.clone());
            false
        });
        ready
    }
    /// Called after the game has accepted or rejected an action. Rejection also
    /// completes its sequence, so a failed craft cannot stall all later movement.
    pub fn complete(&mut self, action: &Authorized) -> Result<()> {
        let input = self
            .peers
            .get_mut(&action.peer)
            .context("peer left lobby")?;
        ensure!(
            action.request.epoch == self.epoch
                && action.request.connection == input.connection
                && action.request.sequence <= input.queued
                && input.completed.checked_add(1) == Some(action.request.sequence),
            "out-of-order action result"
        );
        input.completed = action.request.sequence;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batched_moves_preserve_each_players_order_without_blocking_other_players() {
        let mut gate = Gate::new(1, 10).unwrap();
        gate.members([10, 20, 30].into()).unwrap();
        for (peer, sequence, action) in [
            (20, 1, Action::Move { x: 1, z: 0 }),
            (20, 2, Action::Move { x: 1, z: 0 }),
            (
                20,
                3,
                Action::Place {
                    kind: 2,
                    direction: 0,
                },
            ),
            (30, 1, Action::Move { x: 0, z: 1 }),
            (30, 2, Action::Move { x: 0, z: 1 }),
        ] {
            let bytes = Request {
                epoch: 1,
                connection: gate.connection(peer).unwrap(),
                sequence,
                action,
            }
            .encode()
            .unwrap();
            assert!(gate.receive(peer, &bytes, Duration::ZERO).unwrap());
            assert!(
                !gate.receive(peer, &bytes, Duration::ZERO).unwrap(),
                "retry duplicated a queued step"
            );
        }
        for expected in [vec![(20, 1), (30, 1)], vec![(20, 2), (20, 3), (30, 2)]] {
            let ready = gate.drain_tick();
            assert_eq!(
                ready
                    .iter()
                    .map(|a| (a.peer, a.request.sequence))
                    .collect::<Vec<_>>(),
                expected
            );
            for action in ready {
                gate.complete(&action).unwrap();
            }
        }
        assert_eq!(gate.acknowledged(20).unwrap(), 3);
        assert_eq!(gate.acknowledged(30).unwrap(), 2);
        assert!(gate.drain_tick().is_empty());
    }

    #[test]
    fn requests_are_authenticated_ordered_bounded_and_reconnect_safe() {
        let mut gate = Gate::new(1, 10).unwrap();
        gate.members([10, 20, 30, 40].into()).unwrap();
        let mut request = Request {
            epoch: 1,
            connection: gate.connection(20).unwrap(),
            sequence: 1,
            action: Action::Move { x: 1, z: 0 },
        };
        let old = request.encode().unwrap();
        assert!(gate.receive(99, &old, Duration::ZERO).is_err());
        assert!(gate.receive(30, &old, Duration::ZERO).is_err());
        assert!(gate.receive(20, &old, Duration::ZERO).unwrap());
        assert!(!gate.receive(20, &old, Duration::ZERO).unwrap());
        request.sequence = 3;
        assert!(
            gate.receive(20, &request.encode().unwrap(), Duration::ZERO)
                .is_err()
        );
        for i in 2..=32 {
            request.sequence = i;
            assert!(
                gate.receive(20, &request.encode().unwrap(), Duration::ZERO)
                    .unwrap()
            );
        }
        request.sequence = 33;
        assert!(
            gate.receive(20, &request.encode().unwrap(), Duration::ZERO)
                .is_err()
        );
        let actions: Vec<_> = gate.drain().collect();
        assert!(gate.complete(&actions[1]).is_err());
        for action in actions {
            gate.complete(&action).unwrap();
        }
        assert_eq!(gate.acknowledged(20).unwrap(), 32);
        assert!(
            gate.receive(20, &request.encode().unwrap(), Duration::ZERO)
                .is_err(),
            "draining does not bypass rate limit"
        );
        assert!(
            gate.receive(20, &request.encode().unwrap(), Duration::from_secs(1))
                .unwrap()
        );
        gate.members([10, 30, 40].into()).unwrap();
        assert_eq!(
            gate.drain().count(),
            0,
            "departed player's queued action must not execute"
        );
        gate.members([10, 20, 30, 40].into()).unwrap();
        assert!(gate.receive(20, &old, Duration::from_secs(2)).is_err());
        request.connection = gate.connection(20).unwrap();
        request.sequence = 1;
        assert!(
            gate.receive(20, &request.encode().unwrap(), Duration::from_secs(2))
                .unwrap()
        );
        request.epoch = 2;
        request.sequence = 2;
        assert!(
            gate.receive(20, &request.encode().unwrap(), Duration::from_secs(2))
                .is_err()
        );
    }
    #[test]
    fn script_actions_cannot_supply_world_state_or_identity() {
        let script = |kind: &str, payload| NetworkRequest {
            owner: "controller".into(),
            kind: kind.into(),
            payload,
        };
        assert_eq!(
            Action::from_script(script("factory.move", serde_json::json!({"x":1,"z":0}))).unwrap(),
            Action::Move { x: 1, z: 0 }
        );
        assert_eq!(
            Action::from_script(script("factory.travel", serde_json::json!({}))).unwrap(),
            Action::Travel
        );
        for (kind, payload) in [
            ("factory.move", serde_json::json!({"x":127,"z":0})),
            ("factory.move", serde_json::json!({"x":1,"z":0,"peer":10})),
            ("factory.load", serde_json::json!({"slot":1})),
            ("factory.inventory", serde_json::json!({"items":[100]})),
            (
                "factory.discover",
                serde_json::json!({"planet":1,"x":7,"z":0}),
            ),
            (
                "factory.collect",
                serde_json::json!({"at":{"planet":0,"x":-32768,"z":0}}),
            ),
        ] {
            assert!(Action::from_script(script(kind, payload)).is_err());
        }
    }
}
