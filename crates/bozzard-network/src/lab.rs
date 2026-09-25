//! Deterministic packet and clock boundary for offline multiplayer scenarios.
//! Callers supply logical ticks; no Steam account or wall-clock sleep is required.
use crate::Peer;
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug)]
pub struct Faults {
    pub seed: u64,
    pub host: Peer,
    pub latency_ticks: u64,
    pub jitter_ticks: u64,
    pub loss_to_host_percent: u8,
    pub loss_from_host_percent: u8,
    pub duplicate_percent: u8,
    pub reorder_percent: u8,
}

impl Faults {
    pub fn named(name: &str, seed: u64, host: Peer) -> Result<Self> {
        let (
            latency_ticks,
            jitter_ticks,
            loss_to_host_percent,
            loss_from_host_percent,
            duplicate_percent,
            reorder_percent,
        ) = match name {
            "clean" => (0, 0, 0, 0, 0, 0),
            "cafe-wifi" => (3, 5, 8, 14, 5, 20),
            "satellite" => (12, 4, 2, 6, 2, 12),
            _ => anyhow::bail!("unknown network scenario '{name}'"),
        };
        Ok(Self {
            seed,
            host,
            latency_ticks,
            jitter_ticks,
            loss_to_host_percent,
            loss_from_host_percent,
            duplicate_percent,
            reorder_percent,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub sent_tick: u64,
    pub deliver_tick: u64,
    pub from: Peer,
    pub to: Peer,
    pub bytes: Vec<u8>,
    sequence: u64,
}

pub struct FaultTransport {
    faults: Faults,
    random: u64,
    next_sequence: u64,
    queue: Vec<Packet>,
    pub dropped: u64,
    pub duplicated: u64,
    pub peak_queue: usize,
}

impl FaultTransport {
    pub const MAX_PACKETS: usize = 4096;
    pub fn new(faults: Faults) -> Self {
        Self {
            random: faults.seed.max(1),
            faults,
            next_sequence: 0,
            queue: Vec::new(),
            dropped: 0,
            duplicated: 0,
            peak_queue: 0,
        }
    }
    fn roll(&mut self, percent: u8) -> bool {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        self.random % 100 < u64::from(percent)
    }
    fn delay(&mut self) -> u64 {
        let jitter = self.faults.jitter_ticks;
        let random = self.random;
        self.roll(0); // advance the same seeded stream even when jitter is zero
        let offset = if jitter == 0 {
            0
        } else {
            random % (jitter + 1)
        };
        if self.roll(self.faults.reorder_percent) {
            self.faults
                .latency_ticks
                .saturating_add(jitter)
                .saturating_sub(offset)
        } else {
            self.faults.latency_ticks.saturating_add(offset)
        }
    }
    pub fn pending(&self) -> usize {
        self.queue.len()
    }
    pub fn oldest_packet_age(&self, now_tick: u64) -> Option<u64> {
        self.queue
            .iter()
            .map(|packet| now_tick.saturating_sub(packet.sent_tick))
            .max()
    }
    pub fn send(&mut self, now_tick: u64, from: Peer, to: Peer, bytes: Vec<u8>) -> Result<()> {
        ensure!(
            from != to,
            "loopback packets are not part of a multiplayer session"
        );
        let loss = if to == self.faults.host {
            self.faults.loss_to_host_percent
        } else {
            self.faults.loss_from_host_percent
        };
        if self.roll(loss) {
            self.dropped += 1;
            return Ok(());
        }
        let copies = if self.roll(self.faults.duplicate_percent) {
            2
        } else {
            1
        };
        ensure!(
            self.queue.len() + copies <= Self::MAX_PACKETS,
            "network lab publication queue is full"
        );
        if copies == 2 {
            self.duplicated += 1;
        }
        for _ in 0..copies {
            self.next_sequence += 1;
            let deliver_tick = now_tick.saturating_add(self.delay());
            self.queue.push(Packet {
                sent_tick: now_tick,
                deliver_tick,
                from,
                to,
                bytes: bytes.clone(),
                sequence: self.next_sequence,
            });
        }
        self.peak_queue = self.peak_queue.max(self.queue.len());
        Ok(())
    }
    pub fn receive(&mut self, now_tick: u64) -> Vec<Packet> {
        let mut due = Vec::new();
        let mut pending = Vec::with_capacity(self.queue.len());
        for packet in self.queue.drain(..) {
            if packet.deliver_tick <= now_tick {
                due.push(packet);
            } else {
                pending.push(packet);
            }
        }
        self.queue = pending;
        due.sort_by_key(|packet| (packet.deliver_tick, packet.sequence));
        due
    }
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_faults_reproduce_packet_order_and_bound_memory() {
        let run = || {
            let mut link = FaultTransport::new(Faults::named("cafe-wifi", 17, 10).unwrap());
            let mut events = Vec::new();
            for tick in 0..200 {
                link.send(tick, 20, 10, vec![tick as u8]).unwrap();
                link.send(tick, 10, 20, vec![tick as u8]).unwrap();
                events.extend(link.receive(tick));
            }
            events.extend(link.receive(300));
            assert!(link.peak_queue < FaultTransport::MAX_PACKETS);
            assert!(link.dropped > 0 && link.duplicated > 0);
            events
        };
        assert_eq!(run(), run());
    }
}
