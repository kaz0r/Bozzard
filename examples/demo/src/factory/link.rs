//! Main-thread delivery state shared by Steam and integration tests. A delta base
//! advances only after the recipient acknowledges a fully validated replica.
use super::{
    guest::GuestRuntime,
    host::HostRuntime,
    replication::{Replica, Update, stream::Sender},
};
use anyhow::{Context, Result, ensure};
use bozzard_app::World;
use bozzard_network::Peer;
use std::{collections::BTreeMap, time::Duration};

const ACK: &[u8; 4] = b"SA01";
const RESYNC: &[u8; 4] = b"SR01";
const INTERVAL: Duration = Duration::from_millis(100);
const STALLED: Duration = Duration::from_secs(15);
struct Transfer {
    replica: Replica,
    sender: Sender,
    index: usize,
    progress: Duration,
}
#[derive(Default)]
struct Recipient {
    base: Option<Replica>,
    transfer: Option<Transfer>,
    updated: Duration,
}
#[derive(Default)]
pub struct Link {
    recipients: BTreeMap<Peer, Recipient>,
    acknowledgement: Option<Vec<u8>>,
    resync: bool,
    retry_actions: Option<Duration>,
    pub rejected: u64,
}
fn acknowledgement(replica: &Replica) -> Vec<u8> {
    let mut bytes = ACK.to_vec();
    for number in [replica.epoch, replica.revision, replica.connection] {
        bytes.extend_from_slice(&number.to_le_bytes());
    }
    bytes
}
impl Link {
    /// `peer` is supplied by authenticated lobby membership, never packet text.
    pub fn receive(
        &mut self,
        world: &mut World,
        peer: Peer,
        bytes: &[u8],
        now: Duration,
    ) -> Result<()> {
        if let Some(host) = world.resource_mut::<HostRuntime>() {
            host.version(peer)?; // validates connected membership without cloning a world
            if bytes.starts_with(ACK) {
                ensure!(bytes.len() == 28, "invalid world acknowledgement");
                if let Some(recipient) = self.recipients.get_mut(&peer)
                    && recipient.transfer.as_ref().is_some_and(|t| {
                        acknowledgement(&t.replica) == bytes && t.index == t.sender.packets()
                    })
                {
                    recipient.base = recipient.transfer.take().map(|t| t.replica);
                    recipient.updated = now;
                }
            } else if bytes == RESYNC {
                self.recipients.remove(&peer);
            } else {
                ensure!(
                    !bytes.starts_with(b"SF01"),
                    "guest tried to publish world state"
                );
                host.receive(peer, bytes)?;
            }
        } else if let Some(guest) = world.resource_mut::<GuestRuntime>() {
            // Even a duplicate completed snapshot receives an acknowledgement:
            // the host may be retrying after an interrupted acknowledgement.
            if let Err(error) = guest.receive(peer, bytes, now) {
                if peer == guest.owner {
                    self.resync = true;
                }
                return Err(error);
            }
            if let Some(replica) = guest.current() {
                self.acknowledgement = Some(acknowledgement(replica));
            }
        } else {
            anyhow::bail!("no active factory world");
        }
        Ok(())
    }
    /// Queue at most eight fragments per recipient per frame. Failed sends keep
    /// the exact fragment/request for the next frame; the caller reports errors.
    pub fn pump(
        &mut self,
        world: &mut World,
        now: Duration,
        mut send: impl FnMut(Peer, &[u8]) -> Result<()>,
    ) -> Result<()> {
        if let Some(host) = world.resource::<HostRuntime>() {
            ensure!(host.error.is_none(), "host simulation failed");
            let members = host.peers();
            self.recipients
                .retain(|peer, _| members.contains(peer) && *peer != host.owner);
            let mut failure = None;
            for peer in members.into_iter().filter(|peer| *peer != host.owner) {
                let version = host.version(peer)?;
                let recipient = self.recipients.entry(peer).or_default();
                if recipient.transfer.as_ref().is_some_and(|t| {
                    t.replica.epoch != version.0
                        || t.replica.connection != version.2
                        || now.saturating_sub(t.progress) >= STALLED
                }) {
                    recipient.transfer = None;
                    recipient.base = None;
                }
                if recipient.transfer.is_none()
                    && (recipient.base.is_none()
                        || now.saturating_sub(recipient.updated) >= INTERVAL)
                    && recipient
                        .base
                        .as_ref()
                        .is_none_or(|base| (base.epoch, base.revision, base.connection) != version)
                {
                    let next = host.snapshot(peer)?;
                    let update = if let Some(base) = &recipient.base {
                        Update::between(base, &next)?
                    } else {
                        Update::Full(next.clone())
                    };
                    recipient.transfer = Some(Transfer {
                        sender: Sender::new(&update)?,
                        replica: next,
                        index: 0,
                        progress: now,
                    });
                }
                if let Some(transfer) = &mut recipient.transfer {
                    for _ in 0..8 {
                        let Some(packet) = transfer.sender.packet(transfer.index) else {
                            break;
                        };
                        if let Err(error) = send(peer, &packet) {
                            failure = Some(error);
                            break;
                        }
                        transfer.index += 1;
                        transfer.progress = now;
                    }
                }
            }
            if let Some(error) = failure {
                return Err(error);
            }
        } else if let Some(guest) = world.resource_mut::<GuestRuntime>() {
            ensure!(guest.error.is_none(), "guest simulation failed");
            if !guest.has_pending_requests() {
                self.retry_actions = None;
            }
            if self.retry_actions.is_some_and(|deadline| now >= deadline) {
                guest.retry_pending();
                self.retry_actions = Some(now + Duration::from_secs(1));
            }
            self.resync |= guest.expire(now);
            if self.resync {
                send(guest.owner, RESYNC)?;
                self.resync = false;
            }
            if let Some(ack) = &self.acknowledgement {
                send(guest.owner, ack)?;
                self.acknowledgement = None;
            }
            for _ in 0..8 {
                let Some(bytes) = guest.next_request()? else {
                    break;
                };
                let request: super::replication::requests::Request =
                    serde_json::from_slice(&bytes).context("invalid queued action")?;
                send(guest.owner, &bytes)?;
                guest.mark_sent(request.sequence)?;
                self.retry_actions
                    .get_or_insert(now + Duration::from_secs(1));
            }
        }
        Ok(())
    }
}
