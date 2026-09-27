//! Persistent four-member Steam lobby for a co-op world. Unlike a competitive
//! round, starting alone is valid and invitations/joins remain open during play.
//! Game payloads belong to the host-authoritative protocol above this transport.
use crate::{
    MAX_PLAYERS, Peer, chat,
    lifecycle::{self, LobbyRequests},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use steamworks::{Client, LobbyId, SteamId, networking_types::SendFlags};

const CHANNEL: u32 = 8;
pub const MAX_PAYLOAD: usize = 48 * 1024;
const MAGIC: &[u8] = b"STIX\x01";
enum Event {
    Invite(LobbyId),
    Joined(u64, bool, std::result::Result<LobbyId, String>),
    Chat(LobbyId, Peer, Vec<u8>),
}

pub struct Lobby {
    client: Client,
    game: String,
    _invite: steamworks::CallbackHandle,
    _rich_invite: steamworks::CallbackHandle,
    _chat: steamworks::CallbackHandle,
    sender: mpsc::SyncSender<Event>,
    events: mpsc::Receiver<Event>,
    allowed: Arc<Mutex<BTreeSet<Peer>>>,
    requests: LobbyRequests,
    last_chat: Option<Instant>,
    pub local: Peer,
    pub lobby: Option<u64>,
    pub owner: Option<Peer>,
    pub members: BTreeMap<Peer, String>,
    pub started: bool,
    pub chat: chat::ChatLog,
    pub status: String,
    pub rejected: u64,
}
impl Lobby {
    pub fn new(app_id: u32, fingerprint: &str) -> Result<Self> {
        ensure!(
            !fingerprint.is_empty() && fingerprint.len() <= 128,
            "invalid game fingerprint"
        );
        let client = crate::steam::initialize(app_id)?;
        ensure!(
            client.user().logged_on(),
            "Steam must be online to host or join."
        );
        client.networking_utils().init_relay_network_access();
        let (sender, events) = mpsc::sync_channel(64);
        let tx = sender.clone();
        let invite = client.register_callback(move |event: steamworks::GameLobbyJoinRequested| {
            let _ = tx.try_send(Event::Invite(event.lobby_steam_id));
        });
        let tx = sender.clone();
        let rich_invite =
            client.register_callback(move |event: steamworks::GameRichPresenceJoinRequested| {
                if let Some(id) = lifecycle::parse_lobby_connect(&event.connect) {
                    let _ = tx.try_send(Event::Invite(LobbyId::from_raw(id)));
                }
            });
        let tx = sender.clone();
        let chat_client = client.clone();
        let chat = client.register_callback(move |event: steamworks::LobbyChatMsg| {
            if event.chat_entry_type != steamworks::ChatEntryType::ChatMsg {
                return;
            }
            let mut buffer = [0; 4096];
            let bytes = chat_client
                .matchmaking()
                .get_lobby_chat_entry(event.lobby, event.chat_id, &mut buffer)
                .to_vec();
            let _ = tx.try_send(Event::Chat(event.lobby, event.user.raw(), bytes));
        });
        let allowed = Arc::new(Mutex::new(BTreeSet::new()));
        let access = allowed.clone();
        client
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
        Ok(Self {
            local: client.user().steam_id().raw(),
            client,
            game: format!("stellar-ix-v1-{fingerprint}"),
            _invite: invite,
            _rich_invite: rich_invite,
            _chat: chat,
            sender,
            events,
            allowed,
            requests: LobbyRequests::default(),
            last_chat: None,
            lobby: None,
            owner: None,
            members: BTreeMap::new(),
            started: false,
            chat: chat::ChatLog::default(),
            status: "Create a lobby or accept a friend's invitation.".into(),
            rejected: 0,
        })
    }
    pub fn busy(&self) -> bool {
        self.requests.busy()
    }
    pub fn is_host(&self) -> bool {
        self.lobby.is_some() && self.owner == Some(self.local)
    }
    pub fn create(&mut self) -> Result<()> {
        ensure!(
            !self.busy() && self.lobby.is_none(),
            "Leave the current lobby first."
        );
        lifecycle::create_friends_lobby(
            &self.client,
            self.sender.clone(),
            &mut self.requests,
            Instant::now(),
            MAX_PLAYERS as u32,
            Event::Joined,
        );
        self.status = "Creating Steam lobby…".into();
        Ok(())
    }
    pub fn join(&mut self, id: u64) -> Result<()> {
        ensure!(id != 0, "invalid lobby ID");
        self.leave();
        lifecycle::join_lobby(
            &self.client,
            self.sender.clone(),
            &mut self.requests,
            Instant::now(),
            id,
            Event::Joined,
        );
        self.status = "Joining Steam lobby…".into();
        Ok(())
    }
    fn joined(&mut self, id: LobbyId, created: bool) -> Result<()> {
        let mm = self.client.matchmaking();
        let owner = mm.lobby_owner(id).raw();
        if created {
            ensure!(owner == self.local, "unexpected lobby owner");
            ensure!(
                mm.set_lobby_data(id, "bozzard-game", &self.game)
                    && mm.set_lobby_data(id, "bozzard-host", &owner.to_string())
                    && mm.set_lobby_data(id, "stellar-started", "0"),
                "Could not publish lobby settings."
            );
            mm.set_lobby_joinable(id, true);
        }
        ensure!(
            mm.lobby_data(id, "bozzard-game").as_deref() == Some(self.game.as_str())
                && mm.lobby_data(id, "bozzard-host").as_deref() == Some(owner.to_string().as_str()),
            "Incompatible game version, or the original host left."
        );
        ensure!(mm.lobby_members(id).len() <= MAX_PLAYERS, "Lobby is full.");
        self.lobby = Some(id.raw());
        self.owner = Some(owner);
        self.started = mm.lobby_data(id, "stellar-started").as_deref() == Some("1");
        self.status = if created {
            "Lobby created. Invite friends or start now."
        } else {
            "Joined. Synchronizing with the host…"
        }
        .into();
        Ok(())
    }
    pub fn start(&mut self) -> Result<()> {
        ensure!(self.is_host(), "Only the host can start the world.");
        let id = LobbyId::from_raw(self.lobby.unwrap());
        let mm = self.client.matchmaking();
        ensure!(
            mm.set_lobby_data(id, "stellar-started", "1"),
            "Could not start the lobby."
        );
        // Keep membership alive for chat, authenticated identity and late joins.
        mm.set_lobby_joinable(id, true);
        self.started = true;
        Ok(())
    }
    pub fn wait_for_world(&mut self) -> Result<()> {
        ensure!(self.is_host(), "Only the host can choose another world.");
        ensure!(
            self.client.matchmaking().set_lobby_data(
                LobbyId::from_raw(self.lobby.unwrap()),
                "stellar-started",
                "0"
            ),
            "Could not return lobby to world selection."
        );
        self.started = false;
        Ok(())
    }
    pub fn overlay_available(&self) -> bool {
        self.client.utils().is_overlay_enabled()
    }
    pub fn invite(&self) -> Result<()> {
        lifecycle::invite_to_lobby(
            &self.client,
            LobbyId::from_raw(self.lobby.context("Create a lobby first.")?),
            "Steam overlay unavailable. Use the friend picker.",
        )
    }
    pub fn friends(&self) -> Vec<(Peer, String)> {
        let mut friends: Vec<_> = self
            .client
            .friends()
            .get_friends(steamworks::FriendFlags::IMMEDIATE)
            .into_iter()
            .filter(|f| !self.members.contains_key(&f.id().raw()))
            .map(|f| (f.id().raw(), chat::clean_text(&f.name(), 64)))
            .collect();
        friends.sort_by(|a, b| {
            a.1.to_lowercase()
                .cmp(&b.1.to_lowercase())
                .then(a.0.cmp(&b.0))
        });
        friends
    }
    pub fn invite_friend(&mut self, peer: Peer) -> Result<()> {
        let lobby = self.lobby.context("Create a lobby first.")?;
        let friend = self.client.friends().get_friend(SteamId::from_raw(peer));
        ensure!(
            friend.has_friend(steamworks::FriendFlags::IMMEDIATE),
            "Choose a current Steam friend."
        );
        friend.invite_user_to_game(&format!("+connect_lobby {lobby}"));
        self.status = format!(
            "Invite requested for {}.",
            chat::clean_text(&friend.name(), 64)
        );
        Ok(())
    }
    pub fn send_chat(&mut self, text: &str) -> Result<()> {
        let lobby = self.lobby.context("Join a lobby to chat.")?;
        ensure!(
            self.last_chat
                .is_none_or(|at| at.elapsed() >= Duration::from_millis(500)),
            "Wait a moment before sending again."
        );
        let text = chat::clean_text(text, chat::MAX_CHAT_CHARS);
        ensure!(!text.trim().is_empty(), "Enter a message first.");
        let mut bytes = chat::CHAT_PREFIX.to_vec();
        bytes.extend_from_slice(text.trim().as_bytes());
        self.client
            .matchmaking()
            .send_lobby_chat_message(LobbyId::from_raw(lobby), &bytes)?;
        self.last_chat = Some(Instant::now());
        Ok(())
    }
    pub fn send(&self, peer: Peer, payload: &[u8], reliable: bool) -> Result<()> {
        ensure!(
            peer != self.local
                && self.members.contains_key(&peer)
                && (self.is_host() || self.owner == Some(peer)),
            "recipient is not an authorized peer"
        );
        let bytes = encode(self.lobby.context("not in a lobby")?, payload)?;
        self.client.networking_messages().send_message_to_user(
            SteamId::from_raw(peer).into(),
            if reliable {
                SendFlags::RELIABLE | SendFlags::AUTO_RESTART_BROKEN_SESSION
            } else {
                SendFlags::UNRELIABLE | SendFlags::NO_NAGLE | SendFlags::AUTO_RESTART_BROKEN_SESSION
            },
            &bytes,
            CHANNEL,
        )?;
        Ok(())
    }
    pub fn update(&mut self) -> Result<Vec<(Peer, Vec<u8>)>> {
        self.client.run_callbacks();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Invite(id) => {
                    if !self.busy() && self.lobby != Some(id.raw()) {
                        self.join(id.raw())?;
                    }
                }
                Event::Joined(generation, created, result) => {
                    if self.requests.accept(generation).is_none() {
                        if let Ok(id) = result {
                            self.client.matchmaking().leave_lobby(id);
                        }
                        continue;
                    }
                    match result {
                        Ok(id) => {
                            if let Err(error) = self.joined(id, created) {
                                self.client.matchmaking().leave_lobby(id);
                                self.status = error.to_string();
                            }
                        }
                        Err(error) => self.status = error,
                    }
                }
                Event::Chat(id, peer, bytes) => {
                    if self.lobby == Some(id.raw())
                        && self
                            .client
                            .matchmaking()
                            .lobby_members(id)
                            .contains(&SteamId::from_raw(peer))
                    {
                        self.chat.receive(
                            &self
                                .client
                                .friends()
                                .get_friend(SteamId::from_raw(peer))
                                .name(),
                            &bytes,
                        );
                    }
                }
            }
        }
        if self
            .requests
            .expire(Instant::now(), Duration::from_secs(15))
            .is_some()
        {
            self.status = "Lobby request timed out. Try again.".into();
        }
        let Some(lobby) = self.lobby else {
            return Ok(Vec::new());
        };
        let id = LobbyId::from_raw(lobby);
        let mm = self.client.matchmaking();
        let owner = self.owner.unwrap();
        let members: BTreeSet<_> = mm
            .lobby_members(id)
            .into_iter()
            .map(|peer| peer.raw())
            .collect();
        if !self.client.user().logged_on()
            || members.len() > MAX_PLAYERS
            || !lifecycle::valid_members(self.local, owner, mm.lobby_owner(id).raw(), &members)
        {
            self.leave();
            self.status = "Host left or Steam disconnected.".into();
            return Ok(Vec::new());
        }
        *self.allowed.lock().unwrap() = if self.is_host() {
            members.clone()
        } else {
            [owner].into()
        };
        self.members = members
            .iter()
            .map(|peer| {
                (
                    *peer,
                    chat::clean_text(
                        &self
                            .client
                            .friends()
                            .get_friend(SteamId::from_raw(*peer))
                            .name(),
                        64,
                    ),
                )
            })
            .collect();
        self.started = mm.lobby_data(id, "stellar-started").as_deref() == Some("1");
        let mut incoming = Vec::new();
        for packet in self
            .client
            .networking_messages()
            .receive_messages_on_channel(CHANNEL, 64)
        {
            let sender = packet.identity_peer().steam_id().map(|p| p.raw());
            if let Some(peer) = sender.filter(|peer| {
                *peer != self.local && members.contains(peer) && (self.is_host() || *peer == owner)
            }) && let Ok(payload) = decode(lobby, packet.data())
            {
                incoming.push((peer, payload.to_vec()));
                continue;
            }
            self.rejected += 1;
        }
        Ok(incoming)
    }
    pub fn leave(&mut self) {
        self.allowed.lock().unwrap().clear();
        self.requests.cancel();
        if let Some(lobby) = self.lobby.take() {
            self.client
                .matchmaking()
                .leave_lobby(LobbyId::from_raw(lobby));
        }
        self.owner = None;
        self.members.clear();
        self.started = false;
        self.chat = chat::ChatLog::default();
        self.last_chat = None;
    }
}
impl Drop for Lobby {
    fn drop(&mut self) {
        self.leave();
        self.client
            .networking_messages()
            .session_request_callback(|request| request.reject());
        for event in self.events.try_iter() {
            if let Event::Joined(_, _, Ok(lobby)) = event {
                self.client.matchmaking().leave_lobby(lobby);
            }
        }
    }
}
fn encode(lobby: u64, payload: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        lobby != 0 && !payload.is_empty() && payload.len() <= MAX_PAYLOAD,
        "invalid co-op packet size"
    );
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&lobby.to_le_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}
fn decode(lobby: u64, bytes: &[u8]) -> Result<&[u8]> {
    ensure!(
        bytes.len() > MAGIC.len() + 8
            && bytes.len() <= MAGIC.len() + 8 + MAX_PAYLOAD
            && bytes.starts_with(MAGIC),
        "invalid co-op packet"
    );
    ensure!(
        u64::from_le_bytes(bytes[MAGIC.len()..MAGIC.len() + 8].try_into()?) == lobby,
        "packet from another lobby"
    );
    Ok(&bytes[MAGIC.len() + 8..])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coop_packets_are_bounded_and_confined_to_their_lobby_and_protocol() {
        let packet = encode(42, b"payload").unwrap();
        assert_eq!(decode(42, &packet).unwrap(), b"payload");
        assert!(decode(43, &packet).is_err());
        assert!(encode(42, &vec![0; MAX_PAYLOAD + 1]).is_err());
        assert!(decode(42, b"STIX").is_err());
        assert!(encode(0, b"payload").is_err());
        let mut wrong = packet;
        wrong[4] = 2;
        assert!(decode(42, &wrong).is_err());
    }
}
