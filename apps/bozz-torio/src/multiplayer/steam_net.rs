//! Steam lobby lifecycle and authenticated networking messages, adapted from
//! the Flap Woods Steam session for Bozz-torio's persistent factory.
use super::*;
use crate::{scene::SceneSource, steam::SteamBridge};
use anyhow::{Context, ensure};
use bozzard_network::lifecycle::{self, LobbyRequests, parse_lobby_connect, valid_members};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use steamworks::{Client, LobbyId, SteamId, networking_types::SendFlags};

const CHANNEL: u32 = 8;
const MAX_PLAYERS: u32 = 4;
const TIMEOUT: Duration = Duration::from_secs(15);

enum Event {
    Invite(LobbyId),
    Joined(u64, bool, std::result::Result<LobbyId, String>),
}

pub struct Network {
    client: Option<Client>,
    _invite: Option<steamworks::CallbackHandle>,
    _rich_invite: Option<steamworks::CallbackHandle>,
    events: mpsc::Receiver<Event>,
    sender: mpsc::SyncSender<Event>,
    allowed: Arc<Mutex<BTreeSet<u64>>>,
    game_key: String,
    local: u64,
    lobby: Option<LobbyId>,
    owner: Option<u64>,
    members: BTreeMap<u64, String>,
    sent_to: BTreeSet<u64>,
    last_command: BTreeMap<u64, u64>,
    requests: LobbyRequests,
    status: String,
    revision: u64,
    applied_revision: u64,
    assembly: Option<Assembly>,
    sequence: u64,
    last_sent: Instant,
    last_sent_tick: u64,
    last_received: Instant,
    force: bool,
}
impl Network {
    pub fn new(steam: &SteamBridge, scene: &SceneSource, join_lobby: Option<u64>) -> Result<Self> {
        let hash = Sha256::digest(std::fs::read(&scene.path)?);
        let game_key = format!("bozz-torio-v{PROTOCOL}-{hash:x}");
        let (sender, events) = mpsc::sync_channel(32);
        let allowed = Arc::new(Mutex::new(BTreeSet::new()));
        let mut client = steam.client();
        let mut invite = None;
        let mut rich_invite = None;
        let mut local = 0;
        if let Some(connected) = &client {
            if !connected.user().logged_on() {
                client = None;
            } else {
                connected.networking_utils().init_relay_network_access();
                local = connected.user().steam_id().raw();
                let tx = sender.clone();
                invite = Some(connected.register_callback(
                    move |event: steamworks::GameLobbyJoinRequested| {
                        let _ = tx.try_send(Event::Invite(event.lobby_steam_id));
                    },
                ));
                let tx = sender.clone();
                rich_invite = Some(connected.register_callback(
                    move |event: steamworks::GameRichPresenceJoinRequested| {
                        if let Some(id) = parse_lobby_connect(&event.connect) {
                            let _ = tx.try_send(Event::Invite(LobbyId::from_raw(id)));
                        }
                    },
                ));
                let access = Arc::clone(&allowed);
                connected
                    .networking_messages()
                    .session_request_callback(move |request| {
                        if request
                            .remote()
                            .steam_id()
                            .is_some_and(|id| access.lock().unwrap().contains(&id.raw()))
                        {
                            request.accept();
                        } else {
                            request.reject();
                        }
                    });
            }
        }
        let mut network = Self {
            client,
            _invite: invite,
            _rich_invite: rich_invite,
            events,
            sender,
            allowed,
            game_key,
            local,
            lobby: None,
            owner: None,
            members: BTreeMap::new(),
            sent_to: BTreeSet::new(),
            last_command: BTreeMap::new(),
            requests: LobbyRequests::default(),
            status: "Create a lobby or join a friend's lobby.".into(),
            revision: 1,
            applied_revision: 0,
            assembly: None,
            sequence: 0,
            last_sent: Instant::now(),
            last_sent_tick: 0,
            last_received: Instant::now(),
            force: false,
        };
        if network.client.is_none() {
            ensure!(join_lobby.is_none(), "Steam is required to join a lobby");
            network.status = "Steam offline. Solo factory is available.".into();
        } else if let Some(id) = join_lobby {
            network.join(id)?;
        }
        Ok(network)
    }
    pub fn connected(&self) -> bool {
        self.client.is_some()
    }
    pub fn busy(&self) -> bool {
        self.requests.busy()
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub fn lobby_id(&self) -> Option<u64> {
        self.lobby.map(|id| id.raw())
    }
    pub fn members_len(&self) -> usize {
        self.members.len()
    }
    pub fn has_state(&self) -> bool {
        !self.is_guest_or_joining() || self.applied_revision > 0
    }
    pub fn can_create_or_join(&self) -> bool {
        self.connected() && !self.requests.busy() && self.lobby.is_none()
    }
    pub fn is_host(&self) -> bool {
        self.lobby.is_some() && self.owner == Some(self.local)
    }
    pub fn is_guest_or_joining(&self) -> bool {
        self.requests.pending_guest() || self.lobby.is_some() && self.owner != Some(self.local)
    }
    pub fn create(&mut self) -> Result<()> {
        ensure!(
            !self.requests.busy() && self.lobby.is_none(),
            "leave the current lobby first"
        );
        let client = self.client.as_ref().context("Steam is offline")?;
        self.status = "Creating friends-only lobby…".into();
        lifecycle::create_friends_lobby(
            client,
            self.sender.clone(),
            &mut self.requests,
            Instant::now(),
            MAX_PLAYERS,
            Event::Joined,
        );
        Ok(())
    }
    pub fn join(&mut self, id: u64) -> Result<()> {
        ensure!(id != 0, "invalid lobby ID");
        self.leave();
        let client = self.client.as_ref().context("Steam is offline")?;
        self.status = format!("Joining lobby {id}…");
        lifecycle::join_lobby(
            client,
            self.sender.clone(),
            &mut self.requests,
            Instant::now(),
            id,
            Event::Joined,
        );
        Ok(())
    }
    pub fn invite(&mut self) -> Result<()> {
        let lobby = self.lobby.context("create or join a lobby first")?;
        let client = self.client.as_ref().context("Steam is offline")?;
        lifecycle::invite_to_lobby(
            client,
            lobby,
            "Steam overlay unavailable. Share the lobby ID or ask a friend to join with --join-lobby ID.",
        )
    }
    pub fn leave(&mut self) {
        if let (Some(client), Some(lobby)) = (&self.client, self.lobby) {
            for &member in self.members.keys().filter(|&&id| id != self.local) {
                if let Ok(bytes) = encode(lobby.raw(), Message::Goodbye) {
                    let _ = client.networking_messages().send_message_to_user(
                        SteamId::from_raw(member).into(),
                        SendFlags::RELIABLE,
                        &bytes,
                        CHANNEL,
                    );
                }
            }
            client.matchmaking().leave_lobby(lobby);
            client.friends().set_rich_presence("connect", None);
        }
        self.requests.cancel();
        self.lobby = None;
        self.owner = None;
        self.members.clear();
        self.sent_to.clear();
        self.last_command.clear();
        self.allowed.lock().unwrap().clear();
        self.assembly = None;
        self.applied_revision = 0;
        self.status = "Lobby closed. Solo factory is available.".into();
    }
    pub fn command(&mut self, action: Action) -> Result<()> {
        ensure!(
            self.is_guest_or_joining() && self.lobby.is_some(),
            "wait for the host's lobby"
        );
        self.sequence += 1;
        self.send(
            self.owner.context("host is unavailable")?,
            Message::Command {
                sequence: self.sequence,
                action,
            },
        )?;
        self.status = "Request sent to host.".into();
        Ok(())
    }
    pub fn force_publish(&mut self) {
        if self.is_host() {
            self.force = true;
        }
    }
    fn send(&self, peer: u64, message: Message) -> Result<()> {
        let lobby = self.lobby.context("not in a lobby")?;
        let client = self.client.as_ref().context("Steam is offline")?;
        let bytes = encode(lobby.raw(), message)?;
        client.networking_messages().send_message_to_user(
            SteamId::from_raw(peer).into(),
            SendFlags::RELIABLE,
            &bytes,
            CHANNEL,
        )?;
        Ok(())
    }
    fn send_state(&self, peer: u64, game: &Game) -> Result<()> {
        let lobby = self.lobby.context("not in a lobby")?;
        let client = self.client.as_ref().context("Steam is offline")?;
        for bytes in state_parts(lobby.raw(), self.revision, game)? {
            client.networking_messages().send_message_to_user(
                SteamId::from_raw(peer).into(),
                SendFlags::RELIABLE,
                &bytes,
                CHANNEL,
            )?;
        }
        Ok(())
    }
    fn joined(&mut self, lobby: LobbyId, created: bool) -> Result<()> {
        let client = self.client.as_ref().context("Steam is offline")?;
        let mm = client.matchmaking();
        let owner = mm.lobby_owner(lobby).raw();
        if created {
            ensure!(owner == self.local, "unexpected lobby owner");
            ensure!(
                mm.set_lobby_data(lobby, "bozzard-game", &self.game_key)
                    && mm.set_lobby_data(lobby, "bozzard-host", &owner.to_string()),
                "could not publish factory lobby metadata"
            );
        }
        ensure!(
            mm.lobby_data(lobby, "bozzard-game").as_deref() == Some(self.game_key.as_str())
                && mm.lobby_data(lobby, "bozzard-host").as_deref()
                    == Some(owner.to_string().as_str()),
            "incompatible Bozz-torio scene, protocol, or host"
        );
        self.lobby = Some(lobby);
        self.owner = Some(owner);
        self.last_received = Instant::now();
        client
            .friends()
            .set_rich_presence("connect", Some(&format!("+connect_lobby {}", lobby.raw())));
        self.status = if created {
            format!(
                "Lobby {} created. Invite friends; this machine hosts and saves.",
                lobby.raw()
            )
        } else {
            format!(
                "Joined lobby {}. Waiting for the host's factory…",
                lobby.raw()
            )
        };
        Ok(())
    }
    fn receive_part(
        &mut self,
        revision: u64,
        index: u16,
        count: u16,
        data: &str,
        game: &mut Game,
        change: &mut Change,
    ) -> Result<()> {
        if let Some(next) = accept_state_part(
            &mut self.assembly,
            self.applied_revision,
            revision,
            index,
            count,
            data,
            Instant::now(),
        )? {
            change.structural |= structural_difference(game, &next);
            *game = next;
            change.state = true;
            self.applied_revision = revision;
            self.last_received = Instant::now();
            self.status = format!(
                "Shared factory synced · host {}",
                self.owner.unwrap_or_default()
            );
        }
        Ok(())
    }
    pub fn update(&mut self, game: &mut Game) -> Result<Change> {
        let mut change = Change::default();
        let Some(client) = self.client.clone() else {
            return Ok(change);
        };
        client.run_callbacks();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Invite(lobby) if !self.requests.busy() && self.lobby.is_none() => {
                    if let Err(error) = self.join(lobby.raw()) {
                        self.status = error.to_string();
                    }
                }
                Event::Invite(_) => {}
                Event::Joined(generation, created, result) => {
                    if self.requests.accept(generation).is_none() {
                        if let Ok(lobby) = result {
                            client.matchmaking().leave_lobby(lobby);
                        }
                        continue;
                    }
                    match result {
                        Ok(lobby) => {
                            if let Err(error) = self.joined(lobby, created) {
                                client.matchmaking().leave_lobby(lobby);
                                self.status = error.to_string();
                                change.join_failed = !created;
                            }
                        }
                        Err(error) => {
                            self.status = error;
                            change.join_failed = !created;
                        }
                    }
                }
            }
        }
        if let Some(guest) = self.requests.expire(Instant::now(), TIMEOUT) {
            change.join_failed = guest;
            self.leave();
            self.status = "Steam lobby request timed out.".into();
        }
        let Some(lobby) = self.lobby else {
            return Ok(change);
        };
        let owner = self.owner.context("lobby has no host")?;
        let mm = client.matchmaking();
        let members: BTreeSet<_> = mm
            .lobby_members(lobby)
            .into_iter()
            .map(|id| id.raw())
            .collect();
        if !client.user().logged_on()
            || !valid_members(self.local, owner, mm.lobby_owner(lobby).raw(), &members)
        {
            change.guest_lost = !self.is_host();
            self.leave();
            self.status = "Host left or Steam disconnected. Returned to solo menu.".into();
            return Ok(change);
        }
        *self.allowed.lock().unwrap() = if self.is_host() {
            members.clone()
        } else {
            BTreeSet::from([owner])
        };
        self.members = members
            .iter()
            .map(|id| {
                (
                    *id,
                    client.friends().get_friend(SteamId::from_raw(*id)).name(),
                )
            })
            .collect();
        self.sent_to.retain(|id| members.contains(id));
        self.last_command.retain(|id, _| members.contains(id));
        if self.is_host() {
            for id in members
                .iter()
                .copied()
                .filter(|id| *id != self.local && !self.sent_to.contains(id))
                .collect::<Vec<_>>()
            {
                match self.send_state(id, game) {
                    Ok(()) => {
                        self.sent_to.insert(id);
                    }
                    Err(error) => self.status = format!("Could not sync friend: {error:#}"),
                }
            }
        }
        for packet in client
            .networking_messages()
            .receive_messages_on_channel(CHANNEL, 64)
        {
            let Some(sender) = packet.identity_peer().steam_id().map(|id| id.raw()) else {
                continue;
            };
            if sender == self.local
                || !members.contains(&sender)
                || (!self.is_host() && sender != owner)
            {
                continue;
            }
            let Ok(message) = decode(lobby.raw(), packet.data()) else {
                continue;
            };
            match message {
                Message::Goodbye if sender == owner => {
                    change.guest_lost = true;
                    self.leave();
                    self.status = "Host closed the lobby. Returned to solo menu.".into();
                    return Ok(change);
                }
                Message::Goodbye => {
                    self.sent_to.remove(&sender);
                }
                Message::Command { sequence, action } if self.is_host() => {
                    if sequence <= self.last_command.get(&sender).copied().unwrap_or(0) {
                        continue;
                    }
                    self.last_command.insert(sender, sequence);
                    let structural = action.structural();
                    match action.apply(game) {
                        Ok(()) => {
                            change.state = true;
                            change.structural |= structural;
                            self.force = true;
                        }
                        Err(reason) => {
                            let _ = self.send(
                                sender,
                                Message::Rejected {
                                    sequence,
                                    reason: reason.into(),
                                },
                            );
                        }
                    }
                }
                Message::StatePart {
                    revision,
                    index,
                    count,
                    data,
                } if !self.is_host() => {
                    if let Err(error) =
                        self.receive_part(revision, index, count, &data, game, &mut change)
                    {
                        self.assembly = None;
                        self.status = format!("Rejected invalid host snapshot: {error:#}");
                    }
                }
                Message::Rejected { reason, .. } if !self.is_host() => self.status = reason,
                _ => {}
            }
        }
        expire_assembly(&mut self.assembly, Instant::now(), TIMEOUT);
        if !self.is_host() && self.last_received.elapsed() > TIMEOUT {
            change.guest_lost = true;
            self.leave();
            self.status = "Host connection timed out. Returned to solo menu.".into();
        }
        Ok(change)
    }
    pub fn publish(&mut self, game: &Game) -> Result<()> {
        if !self.is_host() || self.members.len() < 2 {
            return Ok(());
        }
        let elapsed = self.last_sent.elapsed();
        if !self.force
            && elapsed < Duration::from_secs(3)
            && (game.ticks == self.last_sent_tick || elapsed < Duration::from_millis(400))
        {
            return Ok(());
        }
        self.revision += 1;
        for id in self
            .members
            .keys()
            .copied()
            .filter(|id| *id != self.local)
            .collect::<Vec<_>>()
        {
            if let Err(error) = self.send_state(id, game) {
                self.status = format!("Could not send factory state: {error:#}");
            }
        }
        self.force = false;
        self.last_sent = Instant::now();
        self.last_sent_tick = game.ticks;
        Ok(())
    }
}
impl Drop for Network {
    fn drop(&mut self) {
        self.leave();
        if let Some(client) = &self.client {
            client
                .networking_messages()
                .session_request_callback(|request| request.reject());
            for event in self.events.try_iter() {
                if let Event::Joined(_, _, Ok(lobby)) = event {
                    client.matchmaking().leave_lobby(lobby);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invite_payload_accepts_only_one_numeric_lobby_id() {
        assert_eq!(parse_lobby_connect("+connect_lobby 123"), Some(123));
        for bad in [
            "+connect_lobby 0",
            "+connect_lobby nope",
            "+connect_lobby 123 --scene other",
            "other 123",
        ] {
            assert_eq!(parse_lobby_connect(bad), None);
        }
    }
}
