//! Steamworks lobby lifecycle and Steam Networking Messages transport.
//! App 480 is Valve's Spacewar development example, never a shipping App ID.
use crate::{
    flap::{Host, Phase, Replica},
    *,
};
use anyhow::Context;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, mpsc},
    time::Instant,
};
use steamworks::{Client, LobbyId, LobbyType, SteamId, networking_types::SendFlags};
const GAME: &str = "bozzard-flap-woods-v2";
const CHANNEL: u32 = 7;
const TIMEOUT: Duration = Duration::from_secs(15);
static CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
enum Event {
    Invite(LobbyId),
    Joined(u64, bool, std::result::Result<LobbyId, String>),
    Chat(LobbyId, Peer, Vec<u8>),
}

pub struct Session {
    client: Client,
    _invite: steamworks::CallbackHandle,
    _rich_invite: steamworks::CallbackHandle,
    _chat: steamworks::CallbackHandle,
    events: mpsc::Receiver<Event>,
    sender: mpsc::SyncSender<Event>,
    allowed: Arc<Mutex<BTreeSet<Peer>>>,
    pub local: Peer,
    pub lobby: Option<LobbyId>,
    pub owner: Option<Peer>,
    pub members: BTreeMap<Peer, String>,
    pub replica: Replica,
    pub chat: chat::ChatLog,
    last_chat: Option<Instant>,
    host: Option<Host>,
    pub status: String,
    pub pacer: Pacer,
    pub rejected: u64,
    pub sent_bytes: u64,
    pub received_bytes: u64,
    pub send_errors: u64,
    generation: u64,
    pending: Option<Instant>,
    last_update: Instant,
    last_snapshot: Instant,
    seen: BTreeMap<Peer, Instant>,
    ready: BTreeSet<Peer>,
    timed_out: BTreeSet<Peer>,
    flap: bool,
    diagnostics_at: Instant,
}
impl Session {
    pub fn new(app_id: u32) -> Result<Self> {
        let client = initialize(app_id)?;
        ensure!(client.user().logged_on(), "Steam must be online");
        client.networking_utils().init_relay_network_access();
        let local = client.user().steam_id().raw();
        let (sender, events) = mpsc::sync_channel(32);
        let tx = sender.clone();
        let invite = client.register_callback(move |e: steamworks::GameLobbyJoinRequested| {
            let _ = tx.try_send(Event::Invite(e.lobby_steam_id));
        });
        let tx = sender.clone();
        let rich_invite =
            client.register_callback(move |e: steamworks::GameRichPresenceJoinRequested| {
                if let Some(id) = parse_lobby_connect(&e.connect) {
                    let _ = tx.try_send(Event::Invite(LobbyId::from_raw(id)));
                }
            });
        let tx = sender.clone();
        let chat_client = client.clone();
        let chat = client.register_callback(move |e: steamworks::LobbyChatMsg| {
            if e.chat_entry_type != steamworks::ChatEntryType::ChatMsg {
                return;
            }
            // Steam's chat ID is only valid inside this callback.
            let mut buffer = [0; 4096];
            let bytes = chat_client
                .matchmaking()
                .get_lobby_chat_entry(e.lobby, e.chat_id, &mut buffer)
                .to_vec();
            let _ = tx.try_send(Event::Chat(e.lobby, e.user.raw(), bytes));
        });
        let allowed = Arc::new(Mutex::new(BTreeSet::new()));
        let access = Arc::clone(&allowed);
        client
            .networking_messages()
            .session_request_callback(move |request| {
                let accept = request
                    .remote()
                    .steam_id()
                    .is_some_and(|id| access.lock().unwrap().contains(&id.raw()));
                if accept {
                    request.accept();
                } else {
                    request.reject();
                }
            });
        Ok(Self {
            client,
            _invite: invite,
            _rich_invite: rich_invite,
            _chat: chat,
            events,
            sender,
            allowed,
            local,
            lobby: None,
            owner: None,
            members: BTreeMap::new(),
            replica: Replica::default(),
            chat: chat::ChatLog::default(),
            last_chat: None,
            host: None,
            status: "Create a lobby or accept a friend's Steam invite.".into(),
            pacer: Pacer::default(),
            rejected: 0,
            sent_bytes: 0,
            received_bytes: 0,
            send_errors: 0,
            generation: 0,
            pending: None,
            last_update: Instant::now(),
            last_snapshot: Instant::now(),
            seen: BTreeMap::new(),
            ready: BTreeSet::new(),
            timed_out: BTreeSet::new(),
            flap: false,
            diagnostics_at: Instant::now(),
        })
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn is_host(&self) -> bool {
        self.owner == Some(self.local) && self.host.is_some()
    }
    pub fn can_start(&self) -> bool {
        self.is_host()
            && !self.replica.phase.round_active()
            && self.members.len() >= 2
            && self
                .members
                .keys()
                .all(|id| *id == self.local || self.ready.contains(id))
    }
    pub fn create(&mut self) -> Result<()> {
        ensure!(
            !self.busy() && self.lobby.is_none(),
            "leave the current lobby first"
        );
        self.generation += 1;
        let generation = self.generation;
        let tx = self.sender.clone();
        let cleanup = self.client.clone();
        self.pending = Some(Instant::now());
        self.status = "Creating Steam friends-only lobby…".into();
        self.client.matchmaking().create_lobby(
            LobbyType::FriendsOnly,
            MAX_PLAYERS as u32,
            move |result| {
                deliver_join(
                    &cleanup,
                    &tx,
                    Event::Joined(generation, true, result.map_err(|e| format!("{e:?}"))),
                );
            },
        );
        Ok(())
    }
    pub fn join(&mut self, lobby: u64) -> Result<()> {
        ensure!(lobby != 0, "invalid lobby ID");
        self.leave();
        let generation = self.generation;
        let tx = self.sender.clone();
        let cleanup = self.client.clone();
        self.pending = Some(Instant::now());
        self.status = "Joining friend's lobby…".into();
        self.client
            .matchmaking()
            .join_lobby(LobbyId::from_raw(lobby), move |result| {
                deliver_join(
                    &cleanup,
                    &tx,
                    Event::Joined(
                        generation,
                        false,
                        result.map_err(|()| "Lobby unavailable or full".into()),
                    ),
                );
            });
        Ok(())
    }
    pub fn overlay_available(&self) -> bool {
        self.client.utils().is_overlay_enabled()
    }
    pub fn friends(&self) -> Vec<(Peer, String)> {
        let mut friends: Vec<_> = self
            .client
            .friends()
            .get_friends(steamworks::FriendFlags::IMMEDIATE)
            .into_iter()
            .filter(|f| !self.members.contains_key(&f.id().raw()))
            .map(|f| (f.id().raw(), f.name()))
            .collect();
        friends.sort_by(|a, b| {
            a.1.to_lowercase()
                .cmp(&b.1.to_lowercase())
                .then(a.0.cmp(&b.0))
        });
        friends
    }
    pub fn invite_friend(&mut self, id: Peer) -> Result<()> {
        let lobby = self.lobby.context("create or join a lobby first")?;
        ensure!(
            !self.replica.phase.round_active(),
            "wait for the round to finish before inviting"
        );
        let friend = self.client.friends().get_friend(SteamId::from_raw(id));
        ensure!(
            friend.has_friend(steamworks::FriendFlags::IMMEDIATE),
            "choose a current Steam friend"
        );
        friend.invite_user_to_game(&format!("+connect_lobby {}", lobby.raw()));
        // steamworks-rs does not expose Valve's boolean return; don't claim delivery.
        self.status = format!(
            "Invite requested for {}. Your friend can accept in Steam.",
            friend.name()
        );
        Ok(())
    }
    pub fn invite(&self) -> Result<()> {
        let lobby = self.lobby.context("create or join a lobby first")?;
        ensure!(
            self.client.utils().is_overlay_enabled(),
            "Steam overlay unavailable. Use Invite without overlay, or share this lobby ID."
        );
        self.client.friends().activate_invite_dialog(lobby);
        Ok(())
    }
    pub fn start(&mut self) -> Result<()> {
        ensure!(
            self.can_start(),
            "only the host can start, after at least one friend connects"
        );
        let host = self.host.as_mut().context("not hosting")?;
        host.start(self.local)?;
        let snapshot = host.snapshot(self.local)?;
        self.replica
            .apply(self.local, self.local, self.local, snapshot)?;
        self.client
            .matchmaking()
            .set_lobby_joinable(self.lobby.unwrap(), false);
        self.flap = false;
        Ok(())
    }
    pub fn flap(&mut self) {
        if self.replica.phase == Phase::Playing {
            self.flap = true;
        }
    }
    pub fn send_chat(&mut self, text: &str) -> Result<()> {
        let lobby = self.lobby.context("join a lobby before chatting")?;
        ensure!(
            self.last_chat
                .is_none_or(|sent| sent.elapsed() >= Duration::from_millis(500)),
            "Please wait a moment before sending again."
        );
        let text = chat::clean_text(text, chat::MAX_CHAT_CHARS);
        ensure!(!text.trim().is_empty(), "Enter a message first.");
        let mut bytes = chat::CHAT_PREFIX.to_vec();
        bytes.extend_from_slice(text.trim().as_bytes());
        self.client
            .matchmaking()
            .send_lobby_chat_message(lobby, &bytes)?;
        self.last_chat = Some(Instant::now());
        self.status = "Message sent.".into();
        Ok(())
    }
    pub fn leave(&mut self) {
        if let Some(lobby) = self.lobby {
            let recipients: Vec<_> = self
                .members
                .keys()
                .copied()
                .filter(|id| *id != self.local)
                .collect();
            for id in recipients {
                let _ = self.send(id, Message::Goodbye, true);
            }
            self.client.matchmaking().leave_lobby(lobby);
        }
        self.generation += 1;
        self.pending = None;
        self.lobby = None;
        self.owner = None;
        self.host = None;
        self.replica = Replica::default();
        self.chat = chat::ChatLog::default();
        self.last_chat = None;
        self.members.clear();
        self.seen.clear();
        self.ready.clear();
        self.timed_out.clear();
        self.allowed.lock().unwrap().clear();
        self.flap = false;
        self.status = "Lobby closed. Create a lobby or accept an invite.".into();
    }
    fn joined(&mut self, lobby: LobbyId, created: bool) -> Result<()> {
        let mm = self.client.matchmaking();
        let owner = mm.lobby_owner(lobby).raw();
        if created {
            ensure!(owner == self.local, "unexpected lobby owner");
            ensure!(
                mm.set_lobby_data(lobby, "bozzard-game", GAME)
                    && mm.set_lobby_data(lobby, "bozzard-host", &owner.to_string()),
                "could not publish lobby metadata"
            );
        }
        ensure!(
            mm.lobby_data(lobby, "bozzard-game").as_deref() == Some(GAME)
                && mm.lobby_data(lobby, "bozzard-host").as_deref()
                    == Some(owner.to_string().as_str()),
            "incompatible Spacewar lobby or host has left"
        );
        self.lobby = Some(lobby);
        self.owner = Some(owner);
        self.host = created.then(|| Host::new(self.local));
        self.last_snapshot = Instant::now();
        self.status = if created {
            "Lobby created. Invite friends, then Start game."
        } else {
            "Connected. Waiting for the host to start."
        }
        .into();
        Ok(())
    }
    fn send(&mut self, peer: Peer, message: Message, reliable: bool) -> Result<()> {
        let bytes = encode(self.lobby.context("no lobby")?.raw(), message)?;
        let flags = if reliable {
            SendFlags::RELIABLE
        } else {
            SendFlags::UNRELIABLE | SendFlags::NO_NAGLE
        };
        self.client.networking_messages().send_message_to_user(
            SteamId::from_raw(peer).into(),
            flags,
            &bytes,
            CHANNEL,
        )?;
        self.sent_bytes += bytes.len() as u64;
        Ok(())
    }
    pub fn interpolation(&self) -> f32 {
        (self.last_snapshot.elapsed().as_secs_f32() * 20.).clamp(0., 1.)
    }
    pub fn update(&mut self) -> Result<()> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update);
        self.last_update = now;
        self.client.run_callbacks();
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Chat(lobby, sender, bytes) => {
                    if self.lobby == Some(lobby)
                        && self
                            .client
                            .matchmaking()
                            .lobby_members(lobby)
                            .contains(&SteamId::from_raw(sender))
                    {
                        let name = self
                            .client
                            .friends()
                            .get_friend(SteamId::from_raw(sender))
                            .name();
                        self.chat.receive(&name, &bytes);
                    }
                }
                Event::Invite(id) => {
                    if !self.busy() && self.lobby != Some(id) {
                        self.join(id.raw())?;
                    }
                }
                Event::Joined(generation, created, result) => {
                    if generation != self.generation || self.pending.is_none() {
                        if let Ok(lobby) = result {
                            self.client.matchmaking().leave_lobby(lobby);
                        }
                        continue;
                    }
                    self.pending = None;
                    match result {
                        Ok(lobby) => {
                            if let Err(error) = self.joined(lobby, created) {
                                self.client.matchmaking().leave_lobby(lobby);
                                self.status = error.to_string();
                            }
                        }
                        Err(error) => self.status = error,
                    }
                }
            }
        }
        if self
            .pending
            .is_some_and(|started| now.duration_since(started) > TIMEOUT)
        {
            self.pending = None;
            self.status = "Steam lobby request timed out. Try again.".into();
        }
        let Some(lobby) = self.lobby else {
            return Ok(());
        };
        let owner = self.owner.unwrap();
        let mm = self.client.matchmaking();
        let members: BTreeSet<_> = mm
            .lobby_members(lobby)
            .into_iter()
            .map(|id| id.raw())
            .collect();
        if !self.client.user().logged_on()
            || mm.lobby_owner(lobby).raw() != owner
            || !members.contains(&owner)
            || !members.contains(&self.local)
        {
            self.leave();
            self.status = "Host left or Steam disconnected. Create a new lobby.".into();
            return Ok(());
        }
        self.timed_out.retain(|id| members.contains(id));
        let allowed: BTreeSet<_> = members.difference(&self.timed_out).copied().collect();
        *self.allowed.lock().unwrap() = if self.is_host() {
            allowed.clone()
        } else {
            BTreeSet::from([owner])
        };
        self.members = allowed
            .iter()
            .map(|id| {
                (
                    *id,
                    self.client
                        .friends()
                        .get_friend(SteamId::from_raw(*id))
                        .name(),
                )
            })
            .collect();
        self.seen.retain(|id, _| allowed.contains(id));
        self.ready.retain(|id| allowed.contains(id));
        for id in &allowed {
            self.seen.entry(*id).or_insert(now);
        }
        if let Some(host) = &mut self.host {
            for id in host.members() {
                if !allowed.contains(&id) {
                    host.leave(id);
                }
            }
            for id in &allowed {
                let _ = host.join(*id);
            }
        }
        for packet in self
            .client
            .networking_messages()
            .receive_messages_on_channel(CHANNEL, 64)
        {
            let Some(sender) = packet.identity_peer().steam_id().map(|id| id.raw()) else {
                self.rejected += 1;
                continue;
            };
            if !allowed.contains(&sender) || (!self.is_host() && sender != owner) {
                self.rejected += 1;
                continue;
            }
            self.received_bytes += packet.data().len() as u64;
            let Ok(message) = decode(lobby.raw(), packet.data()) else {
                self.rejected += 1;
                continue;
            };
            let result = match message {
                Message::Goodbye if sender == owner => {
                    self.leave();
                    self.status = "Host closed the lobby.".into();
                    return Ok(());
                }
                Message::Goodbye => {
                    self.timed_out.insert(sender);
                    if let Some(host) = &mut self.host {
                        host.leave(sender);
                    }
                    Ok(())
                }
                message if self.is_host() => self.host.as_mut().unwrap().receive(sender, message),
                Message::Snapshot(snapshot) => self
                    .replica
                    .apply(owner, sender, self.local, snapshot)
                    .map(|applied| {
                        if applied {
                            self.last_snapshot = now;
                        }
                    }),
                _ => Err(anyhow::anyhow!("unexpected peer message")),
            };
            if result.is_ok() {
                self.seen.insert(sender, now);
                self.ready.insert(sender);
            } else {
                self.rejected += 1;
            }
        }
        if !self.is_host() && now.duration_since(self.last_snapshot) > TIMEOUT {
            self.leave();
            self.status = "Host connection timed out. Rejoin or create a lobby.".into();
            return Ok(());
        }
        let stale: Vec<_> = self
            .seen
            .iter()
            .filter(|(id, seen)| {
                self.is_host() && **id != self.local && now.duration_since(**seen) > TIMEOUT
            })
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.timed_out.insert(id);
            self.seen.remove(&id);
            self.ready.remove(&id);
            if let Some(host) = &mut self.host {
                host.leave(id);
            }
        }
        let steps = self.pacer.advance(elapsed);
        for _ in 0..steps {
            self.replica.input(std::mem::take(&mut self.flap));
            if let Some(host) = &mut self.host {
                let _ = host.receive(self.local, self.replica.message());
                host.step();
                let snapshot = host.snapshot(self.local)?;
                self.replica.apply(owner, owner, self.local, snapshot)?;
                self.last_snapshot = now;
            } else if self.send(owner, self.replica.message(), false).is_err() {
                self.send_errors += 1;
            }
        }
        // 20 Hz snapshots; also send in idle lobbies so guests acknowledge readiness.
        if self.is_host() && steps > 0 && self.pacer.ticks % 3 < u64::from(steps) {
            let host = self.host.as_mut().unwrap();
            let snapshots: Vec<_> = host
                .members()
                .into_iter()
                .filter(|id| *id != self.local)
                .map(|id| (id, host.snapshot(id)))
                .collect();
            for (id, snapshot) in snapshots {
                if self.send(id, Message::Snapshot(snapshot?), false).is_err() {
                    self.send_errors += 1;
                }
            }
            mm.set_lobby_joinable(lobby, !self.replica.phase.round_active());
        }
        if now.duration_since(self.diagnostics_at) >= Duration::from_secs(5) {
            eprintln!(
                "steam_net lobby={} host={} members={} tick={} sent_bytes={} received_bytes={} rejected={} send_errors={} dropped_ms={}",
                lobby.raw(),
                owner,
                self.members.len(),
                self.replica.tick,
                self.sent_bytes,
                self.received_bytes,
                self.rejected,
                self.send_errors,
                self.pacer.dropped.as_millis()
            );
            self.diagnostics_at = now;
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.leave();
        // The wrapper retains this callback on Client. Replace it to release Play's state.
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

fn deliver_join(client: &Client, sender: &mpsc::SyncSender<Event>, event: Event) {
    // Async Steam operations can finish after Stop has destroyed their Play session.
    if let Err(
        mpsc::TrySendError::Disconnected(Event::Joined(_, _, Ok(lobby)))
        | mpsc::TrySendError::Full(Event::Joined(_, _, Ok(lobby))),
    ) = sender.try_send(event)
    {
        client.matchmaking().leave_lobby(lobby);
    }
}

/// Pump only while no Play worker exists, so late lobby results can clean up after Stop.
pub fn pump_idle_callbacks() {
    if let Some(client) = CLIENT.get() {
        client.run_callbacks();
    }
}

/// Keep the SDK alive across editor Play/Stop cycles, initialized before GPU creation.
/// Per-Play callbacks and lobbies still belong to Session and are dropped on Stop.
pub fn initialize(app_id: u32) -> Result<Client> {
    initialize_mode(app_id, app_id == 480)
}

/// Editor Play may initialize its configured App ID directly; store builds use Steam's launch context.
pub fn initialize_editor(app_id: u32) -> Result<Client> {
    initialize_mode(app_id, true)
}

fn initialize_mode(app_id: u32, development: bool) -> Result<Client> {
    if let Some(client) = CLIENT.get() {
        ensure!(
            client.utils().app_id().0 == app_id,
            "Steam already initialized with a different App ID. Restart the editor with this scene."
        );
        return Ok(client.clone());
    }
    let client = if development { Client::init_app(app_id) } else { Client::init() }
        .context("Steam initialization failed: start Steam and sign in. Published games must be launched through their Steam library entry.")?;
    ensure!(
        client.utils().app_id().0 == app_id,
        "Steam launched a different App ID; use this game's Steam library entry"
    );
    let _ = CLIENT.set(client.clone());
    Ok(client)
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
#[cfg(test)]
mod invite_tests {
    use super::*;
    #[test]
    fn only_a_single_lobby_argument_is_accepted() {
        assert_eq!(parse_lobby_connect("+connect_lobby 123"), Some(123));
        for text in [
            "+connect_lobby 0",
            "+connect_lobby nope",
            "+connect_lobby 123 --scene evil",
            "other 123",
        ] {
            assert_eq!(parse_lobby_connect(text), None);
        }
    }
}
