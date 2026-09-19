//! Bounded, host-authoritative Flap Woods protocol. Steam authenticates sender IDs;
//! the protocol additionally confines messages to a lobby, round and member roster.
pub mod chat;
pub mod flap;
#[cfg(feature = "steam")]
pub mod steam;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub type Peer = u64;
pub const MAX_PLAYERS: usize = 4;
pub const PROTOCOL: u32 = 2;
pub const MAX_PACKET: usize = 16 * 1024;
pub const STEP: Duration = Duration::from_nanos(16_666_667);
pub const DT: f32 = 1. / 60.;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub protocol: u32,
    pub lobby: u64,
    pub message: Message,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Message {
    Input {
        round: u64,
        frames: Vec<flap::InputFrame>,
        ack: u64,
    },
    Snapshot(flap::Snapshot),
    Goodbye,
}
pub fn encode(lobby: u64, message: Message) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&Envelope {
        protocol: PROTOCOL,
        lobby,
        message,
    })?;
    ensure!(bytes.len() <= MAX_PACKET, "network packet exceeds budget");
    Ok(bytes)
}
pub fn decode(lobby: u64, bytes: &[u8]) -> Result<Message> {
    ensure!(bytes.len() <= MAX_PACKET, "network packet exceeds budget");
    let packet: Envelope = serde_json::from_slice(bytes)?;
    ensure!(
        packet.protocol == PROTOCOL && packet.lobby == lobby,
        "incompatible lobby/protocol"
    );
    Ok(packet.message)
}

/// Fixed 60 Hz with at most eight catch-up ticks. Discard excess whole ticks,
/// retain the fractional remainder and report overload rather than spiral.
#[derive(Default)]
pub struct Pacer {
    remainder: Duration,
    pub dropped: Duration,
    pub ticks: u64,
}
impl Pacer {
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        self.remainder = self.remainder.saturating_add(elapsed);
        let due = self.remainder.as_nanos() / STEP.as_nanos();
        let steps = due.min(8) as u32;
        let fraction = Duration::from_nanos((self.remainder.as_nanos() % STEP.as_nanos()) as u64);
        self.dropped += self.remainder - fraction - STEP * steps;
        self.remainder = fraction;
        self.ticks += u64::from(steps);
        steps
    }
}
