//! Bounded framing for reliable Steam messages. World snapshots can exceed one
//! packet; assembly is authenticated, timed, ordered by epoch/revision and atomic.
use super::{Replica, Update};
use anyhow::{Context, Result, ensure};
use bozzard_network::Peer;
use std::time::Duration;

pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PACKET: usize = 16 * 1024;
const HEADER: usize = 36;
const DATA: usize = MAX_PACKET - HEADER;
const MAGIC: &[u8; 4] = b"SF01";
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(300);

pub struct Sender {
    bytes: Vec<u8>,
    version: (u64, u64),
    checksum: u64,
}
impl Sender {
    pub fn new(update: &Update) -> Result<Self> {
        let bytes = serde_json::to_vec(update)?;
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_BYTES,
            "world transfer exceeds size limit"
        );
        let checksum = checksum(&bytes);
        Ok(Self {
            bytes,
            version: update.version(),
            checksum,
        })
    }
    pub fn packets(&self) -> usize {
        self.bytes.len().div_ceil(DATA)
    }
    /// The caller advances its index only after Steam accepts this packet. A
    /// full outgoing queue must not silently discard a fragment or block a frame.
    pub fn packet(&self, index: usize) -> Option<Vec<u8>> {
        if index >= self.packets() {
            return None;
        }
        let mut packet = Vec::with_capacity(MAX_PACKET);
        packet.extend_from_slice(MAGIC);
        packet.extend_from_slice(&self.version.0.to_le_bytes());
        packet.extend_from_slice(&self.version.1.to_le_bytes());
        packet.extend_from_slice(&(self.bytes.len() as u32).to_le_bytes());
        packet.extend_from_slice(&(index as u32).to_le_bytes());
        packet.extend_from_slice(&self.checksum.to_le_bytes());
        packet.extend_from_slice(
            &self.bytes[index * DATA..((index + 1) * DATA).min(self.bytes.len())],
        );
        Some(packet)
    }
}
struct Assembly {
    version: (u64, u64),
    length: usize,
    checksum: u64,
    pieces: Vec<Option<Vec<u8>>>,
    received: usize,
    began: Duration,
    progress: Duration,
}
pub struct Receiver {
    owner: Peer,
    local: Peer,
    current: Option<Replica>,
    pending: Option<Assembly>,
}
impl Receiver {
    pub fn new(owner: Peer, local: Peer) -> Result<Self> {
        ensure!(
            owner != 0 && local != 0 && owner != local,
            "invalid replica peers"
        );
        Ok(Self {
            owner,
            local,
            current: None,
            pending: None,
        })
    }
    pub fn current(&self) -> Option<&Replica> {
        self.current.as_ref()
    }
    pub fn buffered_bytes(&self) -> usize {
        self.pending
            .as_ref()
            .map_or(0, |p| p.pieces.iter().flatten().map(Vec::len).sum())
    }
    pub fn expire(&mut self, now: Duration) -> bool {
        if self.pending.as_ref().is_some_and(|p| {
            now.saturating_sub(p.progress) >= IDLE_TIMEOUT
                || now.saturating_sub(p.began) >= TOTAL_TIMEOUT
        }) {
            self.pending = None;
            return true;
        }
        false
    }
    /// True means a complete, validated world replaced the previous replica.
    /// False means another piece is needed, or this was an old/duplicate piece.
    pub fn receive(&mut self, sender: Peer, packet: &[u8], now: Duration) -> Result<bool> {
        ensure!(
            sender == self.owner,
            "only the lobby host may publish world state"
        );
        ensure!(
            packet.len() > HEADER && packet.len() <= MAX_PACKET && packet.starts_with(MAGIC),
            "invalid world fragment"
        );
        let version = (
            u64::from_le_bytes(packet[4..12].try_into()?),
            u64::from_le_bytes(packet[12..20].try_into()?),
        );
        let length = u32::from_le_bytes(packet[20..24].try_into()?) as usize;
        let index = u32::from_le_bytes(packet[24..28].try_into()?) as usize;
        let checksum = u64::from_le_bytes(packet[28..36].try_into()?);
        ensure!(
            version.0 > 0 && version.1 > 0 && length > 0 && length <= MAX_BYTES,
            "invalid world transfer bounds"
        );
        let count = length.div_ceil(DATA);
        ensure!(
            index < count && packet.len() - HEADER == (length - index * DATA).min(DATA),
            "invalid fragment length/index"
        );
        if self
            .current
            .as_ref()
            .is_some_and(|r| version <= (r.epoch, r.revision))
        {
            return Ok(false);
        }
        self.expire(now);
        if self.pending.as_ref().is_some_and(|p| version < p.version) {
            return Ok(false);
        }
        if self.pending.as_ref().is_none_or(|p| version > p.version) {
            self.pending = Some(Assembly {
                version,
                length,
                checksum,
                pieces: vec![None; count],
                received: 0,
                began: now,
                progress: now,
            });
        }
        let pending = self.pending.as_mut().unwrap();
        ensure!(
            pending.length == length && pending.checksum == checksum,
            "conflicting transfer headers"
        );
        let bytes = &packet[HEADER..];
        if let Some(previous) = &pending.pieces[index] {
            ensure!(previous == bytes, "conflicting duplicate fragment");
            return Ok(false);
        }
        pending.pieces[index] = Some(bytes.to_vec());
        pending.received += 1;
        pending.progress = now;
        if pending.received != count {
            return Ok(false);
        }
        let pending = self.pending.take().unwrap();
        let bytes: Vec<u8> = pending.pieces.into_iter().flatten().flatten().collect();
        ensure!(
            self::checksum(&bytes) == pending.checksum,
            "corrupt world transfer"
        );
        let update: Update = serde_json::from_slice(&bytes).context("invalid world update")?;
        ensure!(
            update.version() == pending.version,
            "world version differs from fragment headers"
        );
        let replica = update.apply(self.current.as_ref(), self.owner, self.local)?;
        self.current = Some(replica);
        Ok(true)
    }
}
// Detect framing/assembly corruption; this is not authentication. Sender identity
// comes from Steam and is checked before even allocating an assembly.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
