//! Optional Steam session controller. SDK callbacks and UI input stay on the
//! native main thread; HostRuntime/GuestRuntime remain in the simulation worker.
#[cfg(feature = "steam")]
use super::{Authority, Session, guest::GuestRuntime, host::HostRuntime, link::Link};
use crate::SceneDemo;
#[cfg(feature = "steam")]
use anyhow::Context;
use anyhow::Result;
#[cfg(feature = "steam")]
use bozzard_scene::NetworkOutbox;
use bozzard_scene::{
    BlueprintRuntime, NetworkFrame, SceneInstance,
    blueprint::{BlackboardValue as B, Value},
    middleware::{
        signals::{Kind, Signals},
        ui::{Control, Runtime},
    },
};
use std::collections::BTreeMap;
#[cfg(feature = "steam")]
use std::time::Instant;

pub const APP_ID: u32 = 480;
/// Separate from the Flap protocol settings: factory co-op remains optional at
/// startup, but exports still require a matching Steam-capable native player.
pub fn app_id(scene: &bozzard_scene::Scene) -> Result<Option<u32>> {
    let mut id = None;
    for config in scene.objects.iter().filter_map(|o| o.extra("steam_coop")) {
        anyhow::ensure!(
            super::is_factory(scene) && id.is_none(),
            "invalid factory Steam settings owner"
        );
        anyhow::ensure!(
            config.as_object().is_some_and(|v| v.len() == 2)
                && config["max_players"] == 4
                && config["app_id"]
                    .as_u64()
                    .is_some_and(|v| v > 0 && v <= u32::MAX as u64),
            "factory Steam settings require app_id and max_players: 4"
        );
        id = Some(config["app_id"].as_u64().unwrap() as u32);
    }
    Ok(id)
}
pub struct Multiplayer {
    #[cfg(feature = "steam")]
    lobby: Option<bozzard_network::coop_lobby::Lobby>,
    fingerprint: String,
    app_id: u32,
    initialized: bool,
    join: Option<u64>,
    bound: Option<u64>,
    names: BTreeMap<u64, String>,
    #[cfg(feature = "steam")]
    link: Link,
    #[cfg(feature = "steam")]
    began: Instant,
    #[cfg(feature = "steam")]
    next_epoch: u64,
    #[cfg(feature = "steam")]
    guest_ready: bool,
    pub chatting: bool,
    draft: String,
    open: bool,
    picking: bool,
    friends: Vec<(u64, String)>,
    friend_page: usize,
    status: String,
}
impl Multiplayer {
    pub fn new(instance: &SceneInstance, join: Option<u64>) -> Result<Self> {
        let fingerprint = format!(
            "{:016x}",
            instance.script_module("earth-factory")?.fingerprint()
        );
        Ok(Self {
            #[cfg(feature = "steam")]
            lobby: None,
            fingerprint,
            app_id: app_id(instance.document())?.unwrap_or(APP_ID),
            initialized: false,
            join,
            bound: None,
            names: Default::default(),
            #[cfg(feature = "steam")]
            link: Default::default(),
            #[cfg(feature = "steam")]
            began: Instant::now(),
            #[cfg(feature = "steam")]
            next_epoch: 1,
            #[cfg(feature = "steam")]
            guest_ready: false,
            chatting: false,
            draft: String::new(),
            open: join.is_some(),
            picking: false,
            friends: vec![],
            friend_page: 0,
            status: String::new(),
        })
    }
    pub fn active(&self) -> bool {
        self.bound.is_some()
    }
    fn initialize(&mut self) -> Result<()> {
        self.initialized = true;
        #[cfg(feature = "steam")]
        {
            if self.lobby.is_none() {
                self.lobby = Some(bozzard_network::coop_lobby::Lobby::new(
                    self.app_id,
                    &self.fingerprint,
                )?);
                self.status.clear();
            }
            Ok(())
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (&self.fingerprint, self.app_id);
            anyhow::bail!("Steam co-op is unavailable in this build. Solo play is available.")
        }
    }
    pub fn join(&mut self, id: u64) -> Result<()> {
        self.initialize()?;
        self.join = Some(id);
        self.open = true;
        Ok(())
    }
    pub fn text(&mut self, text: &str) -> bool {
        if !self.chatting {
            return false;
        }
        let left = bozzard_network::chat::MAX_CHAT_CHARS.saturating_sub(self.draft.chars().count());
        self.draft
            .push_str(&bozzard_network::chat::clean_text(text, left));
        true
    }
    pub fn key(&mut self, key: &str) -> bool {
        if self.chatting {
            match key {
                "Escape" => self.chatting = false,
                "Enter" | "NumpadEnter" => self.send_chat(),
                "Backspace" => {
                    self.draft.pop();
                }
                _ => (),
            }
            return true;
        }
        if self.active() && matches!(key, "Enter" | "NumpadEnter") {
            self.chatting = true;
            return true;
        }
        if self.open && key == "Escape" {
            self.open = false;
            self.picking = false;
            return true;
        }
        self.open
    }
    fn send_chat(&mut self) {
        #[cfg(feature = "steam")]
        if let Some(lobby) = &mut self.lobby {
            match lobby.send_chat(&self.draft) {
                Ok(()) => {
                    self.draft.clear();
                    self.chatting = false;
                }
                Err(error) => self.status = error.to_string(),
            }
        }
    }
    pub fn handle_ui(&mut self, demo: &mut SceneDemo) -> Result<()> {
        let actions: Vec<_> = demo
            .app
            .world
            .resource::<Signals>()
            .map(|signals| {
                let mut ids = vec![
                    "coop-open-title".to_string(),
                    "coop-open-menu".into(),
                    "coop-create".into(),
                    "coop-invite".into(),
                    "coop-steam-overlay".into(),
                    "coop-friends".into(),
                    "coop-leave".into(),
                    "coop-close".into(),
                    "coop-chat".into(),
                    "coop-send".into(),
                    "coop-friends-next".into(),
                ];
                ids.extend((0..4).map(|i| format!("coop-friend-{i}")));
                ids.into_iter()
                    .filter(|id| {
                        signals
                            .for_owner(id, Kind::Ui)
                            .any(|s| s.name == "activate")
                    })
                    .collect()
            })
            .unwrap_or_default();
        for action in actions {
            if let Err(error) = self.action(&action) {
                self.status = error.to_string();
            }
            demo.clear_gameplay_input();
        }
        self.present(demo)
    }
    fn action(&mut self, action: &str) -> Result<()> {
        match action {
            "coop-open-title" | "coop-open-menu" => {
                self.open = true;
                return Ok(());
            }
            "coop-close" => {
                self.open = false;
                self.picking = false;
                return Ok(());
            }
            "coop-chat" => {
                if self.active() {
                    self.chatting = true;
                }
                return Ok(());
            }
            "coop-send" => {
                self.send_chat();
                return Ok(());
            }
            _ => (),
        }
        self.initialize()?;
        self.status.clear();
        #[cfg(feature = "steam")]
        {
            let lobby = self.lobby.as_mut().unwrap();
            match action {
                "coop-steam-overlay" => bozzard_network::steam::open_overlay()?,
                "coop-create" => lobby.create()?,
                "coop-invite" => {
                    if lobby.overlay_available() {
                        lobby.invite()?;
                    } else {
                        self.friends = lobby.friends();
                        self.picking = true;
                        self.friend_page = 0;
                    }
                }
                "coop-friends" => {
                    self.friends = lobby.friends();
                    self.picking = true;
                    self.friend_page = 0;
                }
                "coop-friends-next" => {
                    self.friend_page =
                        (self.friend_page + 1) % self.friends.len().div_ceil(4).max(1)
                }
                "coop-leave" => {
                    lobby.leave();
                    self.status = "Left lobby.".into();
                    self.chatting = false;
                    self.picking = false;
                }
                _ => {
                    if let Some(index) = action
                        .strip_prefix("coop-friend-")
                        .and_then(|s| s.parse::<usize>().ok())
                        && let Some((peer, _)) = self.friends.get(self.friend_page * 4 + index)
                    {
                        lobby.invite_friend(*peer)?;
                    }
                }
            }
        }
        Ok(())
    }
    pub fn update(&mut self, demo: &mut SceneDemo) -> Result<()> {
        if demo.app.world.resource::<BlueprintRuntime>().is_none() {
            return Ok(());
        }
        if !self.initialized
            && let Err(error) = self.initialize()
        {
            self.status = error.to_string();
        }
        #[cfg(feature = "steam")]
        if let Some(mut lobby) = self.lobby.take() {
            let result = (|| -> Result<()> {
                if let Some(id) = self.join.take() {
                    lobby.join(id)?;
                }
                let packets = lobby.update()?;
                if self.bound != lobby.lobby {
                    self.detach(demo)?;
                    self.bound = lobby.lobby;
                    self.names.clear();
                    self.link = Link::default();
                    self.guest_ready = false;
                    self.open = lobby.lobby.is_some();
                }
                if let Some(owner) = lobby.owner {
                    if lobby.is_host() {
                        let title = title_open(demo);
                        if title && demo.app.world.resource::<HostRuntime>().is_some() {
                            self.detach(demo)?;
                            lobby.wait_for_world()?;
                            self.link = Link::default();
                        }
                        let session = demo
                            .app
                            .world
                            .resource_mut::<Session>()
                            .context("missing session")?;
                        session.authority = Authority::Host;
                        session.bind_local_peer(lobby.local);
                        if self.names != lobby.members
                            && let Some(host) = demo.app.world.resource_mut::<HostRuntime>()
                        {
                            host.members(lobby.members.clone())?;
                            self.names = lobby.members.clone();
                        }
                        if !title && super::session_value(&demo.app.world, 125)? == 0. {
                            if demo.app.world.resource::<HostRuntime>().is_none() {
                                HostRuntime::start(
                                    &mut demo.app.world,
                                    lobby.local,
                                    self.next_epoch,
                                    lobby.members.clone(),
                                )?;
                                self.names = lobby.members.clone();
                            }
                            if !lobby.started {
                                lobby.start()?;
                                self.open = false;
                            }
                        }
                    } else {
                        let ready = demo
                            .app
                            .world
                            .resource::<GuestRuntime>()
                            .is_some_and(|g| g.ready());
                        if ready && title_open(demo) {
                            lobby.leave();
                            self.detach(demo)?;
                            self.bound = None;
                            self.link = Link::default();
                            self.open = false;
                            return Ok(());
                        }
                        if !lobby.started
                            && demo
                                .app
                                .world
                                .resource::<GuestRuntime>()
                                .is_some_and(|g| g.ready())
                        {
                            self.detach(demo)?;
                            self.link = Link::default();
                            self.open = true;
                            self.guest_ready = false;
                        }
                        if demo.app.world.resource::<GuestRuntime>().is_none() {
                            GuestRuntime::start(&mut demo.app.world, owner, lobby.local)?;
                        }
                        if ready && !self.guest_ready {
                            self.open = false;
                            self.guest_ready = true;
                        }
                    }
                    for (peer, bytes) in packets {
                        if let Err(error) = self.link.receive(
                            &mut demo.app.world,
                            peer,
                            &bytes,
                            self.began.elapsed(),
                        ) {
                            self.link.rejected += 1;
                            self.status = format!("Rejected session packet: {error}");
                        }
                    }
                    if let Err(error) =
                        self.link
                            .pump(&mut demo.app.world, self.began.elapsed(), |peer, bytes| {
                                lobby.send(peer, bytes, true)
                            })
                    {
                        self.status = format!("Synchronizing: {error}");
                    }
                }
                Ok(())
            })();
            if let Err(error) = result {
                self.status = error.to_string();
            }
            self.lobby = Some(lobby);
        }
        self.present(demo)
    }
    #[cfg(feature = "steam")]
    fn detach(&mut self, demo: &mut SceneDemo) -> Result<()> {
        if let Some(host) = demo.app.world.remove_resource::<HostRuntime>() {
            self.next_epoch = host
                .version(host.owner)?
                .0
                .checked_add(1)
                .context("session epochs exhausted")?;
        }
        let guest = demo.app.world.remove_resource::<GuestRuntime>().is_some();
        if guest {
            let runtime = demo.app.world.resource_mut::<BlueprintRuntime>().unwrap();
            runtime.patch_blackboards(
                &Default::default(),
                &[(
                    "controller".into(),
                    [("title_open".into(), B::Scalar(Value::Bool(true)))].into(),
                )]
                .into(),
            )?;
            for (id, visible) in [
                ("title-overlay", true),
                ("game-hud", false),
                ("game-panels", false),
            ] {
                control(demo, id, Control::Visible(visible))?;
                control(demo, id, Control::Enabled(visible))?;
            }
        }
        if let Some(session) = demo.app.world.resource_mut::<Session>() {
            session.authority = Authority::Solo;
            if guest {
                session.local_peer = None;
                session.players.clear();
            }
        }
        super::set_session(&mut demo.app.world, 122, 0.)?;
        demo.app.world.insert_resource(NetworkFrame::default());
        if let Some(outbox) = demo.app.world.resource_mut::<NetworkOutbox>() {
            outbox.clear();
        }
        self.chatting = false;
        self.draft.clear();
        demo.clear_gameplay_input();
        Ok(())
    }
    fn present(&self, demo: &mut SceneDemo) -> Result<()> {
        let joined = self.active();
        #[cfg(feature = "steam")]
        let (host, busy, started, names, chat, status) = self
            .lobby
            .as_ref()
            .map(|l| {
                (
                    l.is_host(),
                    l.busy(),
                    l.started,
                    l.members.values().cloned().collect::<Vec<_>>().join("\n"),
                    l.chat.text(),
                    if self.status.is_empty() {
                        l.status.clone()
                    } else {
                        self.status.clone()
                    },
                )
            })
            .unwrap_or((
                false,
                false,
                false,
                String::new(),
                String::new(),
                self.status.clone(),
            ));
        #[cfg(not(feature = "steam"))]
        let (host, busy, started, names, chat, status) = (
            false,
            false,
            false,
            String::new(),
            String::new(),
            self.status.clone(),
        );
        let title = title_open(demo);
        for id in [
            "title-create",
            "title-load",
            "title-survival",
            "title-creative",
            "title-dev",
        ] {
            control(demo, id, Control::Enabled(!self.open && (!joined || host)))?;
        }
        for id in ["menu-save", "menu-load"] {
            control(demo, id, Control::Enabled(!joined || host))?;
        }
        control(demo, "coop-overlay", Control::Visible(self.open))?;
        control(demo, "coop-overlay", Control::Enabled(self.open))?;
        control(
            demo,
            "coop-status",
            Control::Text(if joined {
                format!(
                    "{} / 4 players · {}\n{}",
                    self.names.len().max(names.lines().count()),
                    if host {
                        "You are hosting"
                    } else {
                        "Host owns the world"
                    },
                    status
                )
            } else {
                status
            }),
        )?;
        control(demo, "coop-members", Control::Text(names))?;
        control(
            demo,
            "coop-overlay-status",
            Control::Text(
                if crate::steam_runtime::overlay_available() {
                    "Steam overlay ready · Shift+Tab (default shortcut)"
                } else {
                    "Overlay unavailable. Launch through Steam with its overlay enabled."
                }
                .into(),
            ),
        )?;
        control(demo,"coop-hint",Control::Text(if joined && host && title {"Close this panel, then create a world or load a save to start. You can start alone."}else if joined && !started {"Waiting for the host to choose a world. You can chat while you wait."}else if joined {"Invite friends anytime. The host keeps the world and saves."}else{"Steam friends-only lobby · one host and up to three guests"}.into()))?;
        for (id, enabled) in [
            ("coop-create", !joined && !busy),
            ("coop-invite", joined),
            ("coop-friends", joined),
            ("coop-leave", joined),
            ("coop-chat", joined),
            ("coop-send", joined),
        ] {
            control(demo, id, Control::Enabled(enabled))?;
        }
        let showing_chat = !self.open && self.chatting;
        control(demo, "coop-chat-overlay", Control::Visible(showing_chat))?;
        control(demo, "coop-chat-overlay", Control::Enabled(showing_chat))?;
        for id in ["coop-chat-log", "coop-lobby-log"] {
            follow_text(
                demo,
                id,
                if chat.is_empty() {
                    "No messages yet.".into()
                } else {
                    chat.clone()
                },
            )?;
        }
        for id in ["coop-chat-draft", "coop-lobby-draft"] {
            follow_text(
                demo,
                id,
                if self.chatting {
                    format!("> {}▏", self.draft)
                } else {
                    "Press Enter to chat".into()
                },
            )?;
        }
        for i in 0..4 {
            let id = format!("coop-friend-{i}");
            let friend = self.friends.get(self.friend_page * 4 + i);
            control(
                demo,
                &id,
                Control::Visible(self.open && self.picking && friend.is_some()),
            )?;
            if let Some((_, name)) = friend {
                control(demo, &id, Control::Text(format!("Invite {name}")))?;
            }
        }
        control(
            demo,
            "coop-friends-next",
            Control::Visible(self.open && self.picking && self.friends.len() > 4),
        )?;
        if let Some(frame) = demo.app.world.resource_mut::<NetworkFrame>()
            && frame.state.is_object()
        {
            frame.state["session_panel"] =
                (self.open || self.chatting || crate::steam_runtime::overlay_active()).into();
        }
        Ok(())
    }
}
fn title_open(demo: &SceneDemo) -> bool {
    matches!(
        demo.app
            .world
            .resource::<BlueprintRuntime>()
            .and_then(|r| r.object_blackboard("controller"))
            .and_then(|b| b.get("title_open")),
        Some(B::Scalar(Value::Bool(true)))
    )
}
fn control(demo: &mut SceneDemo, id: &str, value: Control) -> Result<()> {
    let unchanged = demo
        .app
        .world
        .resource::<Runtime>()
        .and_then(|r| r.widgets.get(id))
        .is_some_and(|s| match &value {
            Control::Text(t) => s.text.as_ref() == Some(t),
            Control::Visible(v) => s.visible == Some(*v),
            Control::Enabled(v) => s.enabled == Some(*v),
            _ => false,
        });
    if unchanged {
        return Ok(());
    }
    super::control(&mut demo.app.world, id, value)
}

fn follow_text(demo: &mut SceneDemo, id: &str, text: String) -> Result<()> {
    if demo
        .app
        .world
        .resource::<Runtime>()
        .and_then(|r| r.widgets.get(id))
        .is_some_and(|s| s.text.as_ref() == Some(&text))
    {
        return Ok(());
    }
    control(demo, id, Control::Text(text))?;
    let viewport = format!("{id}-scroll");
    if demo.instance().entity(&viewport).is_some() {
        // Layout clamps to the measured content height. Only a changed message
        // or draft follows the end, so idle frames preserve manual scrollback.
        if let Some(ui) = demo.app.world.resource_mut::<Runtime>() {
            ui.widgets.entry(viewport).or_default().scroll = 1_000_000.;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chat_follows_changed_text_without_overriding_manual_scrollback() -> Result<()> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../earth-factory/scenes/earth.json");
        let scene = bozzard_scene::Scene::from_json(&std::fs::read_to_string(&path)?)?;
        let mut demo = SceneDemo::new_with_prefabs(&scene, Some(&path))?;
        for id in [
            "coop-chat-log",
            "coop-lobby-log",
            "coop-chat-draft",
            "coop-lobby-draft",
        ] {
            let viewport = format!("{id}-scroll");
            follow_text(&mut demo, id, "W".repeat(160))?;
            assert_eq!(
                demo.app.world.resource::<Runtime>().unwrap().widgets[&viewport].scroll,
                1_000_000.
            );
            demo.app
                .world
                .resource_mut::<Runtime>()
                .unwrap()
                .widgets
                .get_mut(&viewport)
                .unwrap()
                .scroll = 0.;
            follow_text(&mut demo, id, "W".repeat(160))?;
            assert_eq!(
                demo.app.world.resource::<Runtime>().unwrap().widgets[&viewport].scroll,
                0.
            );
            follow_text(&mut demo, id, "Another message".into())?;
            assert_eq!(
                demo.app.world.resource::<Runtime>().unwrap().widgets[&viewport].scroll,
                1_000_000.
            );
        }
        Ok(())
    }
    #[test]
    fn factory_steam_settings_are_exported_and_reject_invalid_or_mixed_protocols() {
        let mut scene=bozzard_scene::Scene::from_json(r#"{"version":1,"name":"Factory settings","views":{},
            "assets":{"earth-factory":{"kind":"script","path":"game.rhai"}},
            "objects":[{"id":"controller","name":"Controller","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
                "steam_coop":{"app_id":480,"max_players":4}}]}"#).unwrap();
        for id in [480, 123456] {
            scene.objects[0].extras.get_mut("steam_coop").unwrap()["app_id"] = id.into();
            assert_eq!(crate::multiplayer::app_id(&scene).unwrap(), Some(id));
        }
        scene.objects[0].extras.get_mut("steam_coop").unwrap()["max_players"] = 5.into();
        assert!(app_id(&scene).is_err());
        scene.objects[0].extras.get_mut("steam_coop").unwrap()["max_players"] = 4.into();
        scene.objects[0]
            .extras
            .insert("steam_multiplayer".into(), serde_json::json!({}));
        assert!(crate::multiplayer::app_id(&scene).is_err());
    }
    #[test]
    fn chat_captures_game_keys_limits_unicode_and_escape_preserves_draft() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../earth-factory/scenes/earth.json");
        let scene =
            bozzard_scene::Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let demo = SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
        let mut net = Multiplayer::new(demo.instance(), None).unwrap();
        net.bound = Some(10);
        assert!(net.key("Enter"));
        assert!(net.chatting);
        assert!(net.key("J"));
        assert!(net.key("Space"));
        assert!(net.text(&"å\n".repeat(200)));
        assert_eq!(net.draft.chars().count(), 160);
        assert!(!net.draft.contains('\n'));
        assert!(net.key("Backspace"));
        assert_eq!(net.draft.chars().count(), 159);
        assert!(net.key("Escape"));
        assert!(!net.chatting);
        assert!(!net.key("W"));
        assert!(!net.text("ignored"));
        assert_eq!(net.draft.chars().count(), 159);
        net.open = true;
        assert!(net.key("I"));
        assert!(net.key("Escape"));
        assert!(!net.open);
    }
    #[test]
    #[cfg(feature = "steam")]
    #[ignore = "requires a signed-in Steam client; creates and leaves a private test lobby"]
    fn steam_host_starts_alone_and_returns_to_world_selection() -> Result<()> {
        let _steam_shutdown = crate::steam_runtime::ShutdownGuard;
        use bozzard_scene::{Layer, middleware::ui::Input};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../earth-factory/scenes/earth.json");
        let scene = bozzard_scene::Scene::from_json(&std::fs::read_to_string(&path)?)?;
        let mut demo = SceneDemo::new_with_prefabs(&scene, Some(&path))?;
        demo.app.step();
        demo.check_simulation()?;
        demo.enable_multiplayer(None)?;
        demo.pump_multiplayer()?;
        let adapter = demo.factory_multiplayer.as_ref().unwrap();
        anyhow::ensure!(
            adapter.lobby.is_some(),
            "Steam unavailable: {}",
            adapter.status
        );
        fn click(demo: &mut SceneDemo, id: &str) -> Result<()> {
            let frame = demo
                .instance()
                .ui_frame(&demo.app.world, Layer::ThreeD, [1080., 600.])?;
            let r = frame
                .element(id)
                .with_context(|| format!("missing {id}"))?
                .rect;
            let p = [r.min[0] + r.size[0] * 0.5, r.min[1] + r.size[1] * 0.5];
            for input in [Input::PointerDown(p), Input::PointerUp(p)] {
                demo.ui_input(Layer::ThreeD, [1080., 600.], input)?;
            }
            demo.app.step();
            demo.check_simulation()?;
            demo.pump_multiplayer()
        }
        click(&mut demo, "coop-open-title")?;
        click(&mut demo, "coop-create")?;
        let deadline = Instant::now() + std::time::Duration::from_secs(20);
        while !demo.factory_multiplayer.as_ref().unwrap().active() {
            anyhow::ensure!(Instant::now() < deadline, "Steam lobby creation timed out");
            std::thread::sleep(std::time::Duration::from_millis(20));
            demo.pump_multiplayer()?;
        }
        click(&mut demo, "coop-close")?;
        click(&mut demo, "title-create")?;
        let lobby = demo
            .factory_multiplayer
            .as_ref()
            .unwrap()
            .lobby
            .as_ref()
            .unwrap();
        anyhow::ensure!(
            lobby.started && lobby.is_host() && lobby.members.len() == 1,
            "solo Steam lobby did not start"
        );
        let lobby_id = lobby.lobby;
        anyhow::ensure!(
            demo.app.world.resource::<HostRuntime>().is_some(),
            "host worker missing"
        );
        demo.set_threaded_simulation(true)?;
        for _ in 0..24 {
            demo.advance_with_frame(demo.app.timestep(), || ())?;
            demo.pump_multiplayer()?;
        }
        // Stop the world through its existing menu, retaining lobby membership.
        demo.set_gameplay_input(bozzard_scene::GameplayInput {
            keys: bozzard_scene::keys::bit("Escape"),
            ..Default::default()
        });
        demo.app.step();
        demo.clear_gameplay_input();
        click(&mut demo, "menu-main-menu")?;
        let lobby = demo
            .factory_multiplayer
            .as_ref()
            .unwrap()
            .lobby
            .as_ref()
            .unwrap();
        anyhow::ensure!(
            lobby.lobby == lobby_id && !lobby.started,
            "lobby did not return to world selection"
        );
        anyhow::ensure!(
            demo.app.world.resource::<HostRuntime>().is_none(),
            "old host worker survived main menu"
        );
        // Drop leaves the test lobby even if any assertion above failed.
        Ok(())
    }
}
