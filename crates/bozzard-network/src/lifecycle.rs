//! Shared, clock-injected lobby request lifecycle for reference games.
use crate::Peer;
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct LobbyRequests {
    generation: u64,
    pending: Option<(Instant, bool)>,
}
impl LobbyRequests {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn pending_guest(&self) -> bool {
        self.pending.is_some_and(|(_, guest)| guest)
    }
    pub fn begin(&mut self, now: Instant, guest: bool) -> u64 {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("lobby generation exhausted");
        self.pending = Some((now, guest));
        self.generation
    }
    pub fn accept(&mut self, generation: u64) -> Option<bool> {
        if generation != self.generation {
            return None;
        }
        self.pending.take().map(|(_, guest)| guest)
    }
    pub fn expire(&mut self, now: Instant, timeout: Duration) -> Option<bool> {
        if self
            .pending
            .is_some_and(|(started, _)| now.duration_since(started) > timeout)
        {
            let guest = self.pending_guest();
            self.cancel();
            Some(guest)
        } else {
            None
        }
    }
    pub fn cancel(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("lobby generation exhausted");
        self.pending = None;
    }
}

pub fn valid_members(
    local: Peer,
    owner: Peer,
    current_owner: Peer,
    members: &BTreeSet<Peer>,
) -> bool {
    current_owner == owner && members.contains(&owner) && members.contains(&local)
}

/// Invite payloads are data, never shell commands or arbitrary launch arguments.
pub fn parse_lobby_connect(connect: &str) -> Option<u64> {
    let mut words = connect.split_whitespace();
    if words.next()? != "+connect_lobby" {
        return None;
    }
    let id: u64 = words.next()?.parse().ok()?;
    (id != 0 && words.next().is_none()).then_some(id)
}

#[cfg(feature = "steam")]
pub fn deliver_join<E>(
    client: &steamworks::Client,
    sender: &std::sync::mpsc::SyncSender<E>,
    event: E,
    lobby: impl Fn(&E) -> Option<steamworks::LobbyId>,
) {
    let joined = lobby(&event);
    if sender.try_send(event).is_err()
        && let Some(lobby) = joined
    {
        client.matchmaking().leave_lobby(lobby);
    }
}

#[cfg(feature = "steam")]
pub fn create_friends_lobby<E: Send + 'static>(
    client: &steamworks::Client,
    sender: std::sync::mpsc::SyncSender<E>,
    requests: &mut LobbyRequests,
    now: Instant,
    capacity: u32,
    event: impl FnOnce(u64, bool, Result<steamworks::LobbyId, String>) -> E + Send + 'static,
) {
    let generation = requests.begin(now, false);
    let cleanup = client.clone();
    client.matchmaking().create_lobby(
        steamworks::LobbyType::FriendsOnly,
        capacity,
        move |result| {
            let joined = result.as_ref().ok().copied();
            deliver_join(
                &cleanup,
                &sender,
                event(
                    generation,
                    true,
                    result.map_err(|error| format!("{error:?}")),
                ),
                |_| joined,
            );
        },
    );
}

#[cfg(feature = "steam")]
pub fn join_lobby<E: Send + 'static>(
    client: &steamworks::Client,
    sender: std::sync::mpsc::SyncSender<E>,
    requests: &mut LobbyRequests,
    now: Instant,
    id: u64,
    event: impl FnOnce(u64, bool, Result<steamworks::LobbyId, String>) -> E + Send + 'static,
) {
    let generation = requests.begin(now, true);
    let cleanup = client.clone();
    client
        .matchmaking()
        .join_lobby(steamworks::LobbyId::from_raw(id), move |result| {
            let joined = result.as_ref().ok().copied();
            deliver_join(
                &cleanup,
                &sender,
                event(
                    generation,
                    false,
                    result.map_err(|()| "Lobby unavailable or full".into()),
                ),
                |_| joined,
            );
        });
}

#[cfg(feature = "steam")]
pub fn invite_to_lobby(
    client: &steamworks::Client,
    lobby: steamworks::LobbyId,
    fallback: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(client.utils().is_overlay_enabled(), "{fallback}");
    client.friends().activate_invite_dialog(lobby);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_callback_timeout_and_owner_change_are_rejected() {
        let now = Instant::now();
        let mut requests = LobbyRequests::default();
        let first = requests.begin(now, true);
        assert!(requests.busy() && requests.pending_guest());
        assert_eq!(
            requests.expire(now + Duration::from_secs(16), Duration::from_secs(15)),
            Some(true)
        );
        let second = requests.begin(now + Duration::from_secs(17), false);
        assert_eq!(requests.accept(first), None);
        assert_eq!(requests.accept(second), Some(false));
        requests.cancel();
        assert_eq!(requests.accept(second), None);
        let members = BTreeSet::from([10, 20]);
        assert!(valid_members(20, 10, 10, &members));
        assert!(!valid_members(20, 10, 20, &members));
        assert!(!valid_members(30, 10, 10, &members));
        assert_eq!(parse_lobby_connect("+connect_lobby 123"), Some(123));
        assert_eq!(parse_lobby_connect("+connect_lobby 123 --scene x"), None);
    }
}
