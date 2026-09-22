//! Shared Play-session UI and presentation for the native player and editor.
use crate::SceneDemo;
use anyhow::{Context, Result, ensure};
use bozzard_network::{
    Peer,
    flap::{Phase, Replica},
};
use bozzard_scene::middleware::{
    signals::{Kind, Signals},
    ui::{Control, Input, Runtime},
};
use bozzard_scene::{GameplayInput, Layer, NetworkFrame, Scene, SceneInstance};
use std::collections::BTreeMap;

const COMPONENT: &str = "steam_multiplayer";

/// Register the reference game's settings with scene serialization and the editor inspector.
pub fn register_component() -> Result<()> {
    static REGISTERED: std::sync::OnceLock<std::result::Result<(), String>> =
        std::sync::OnceLock::new();
    REGISTERED
        .get_or_init(|| {
            bozzard_scene::register_component(bozzard_scene::ComponentType {
                name: COMPONENT,
                label: "Steam Multiplayer",
                ui: bozzard_scene::Ui::Generic,
                help: "Scripted multiplayer · up to four players. 480 tests with Spacewar; set your own Steam App ID before publishing. Restart the editor after changing App ID. Gameplay lives in the attached Rhai scripts.",
                fields: || {
                    const FIELDS: &[bozzard_scene::Field] = &[
                        bozzard_scene::Field::text("app_id", "Steam App ID", "480 for Spacewar testing"),
                        bozzard_scene::Field::text("game", "Game ID", "A shared ID for this example game"),
                        bozzard_scene::Field::asset("player_script", "Player script", bozzard_scene::AssetKind::Script),
                        bozzard_scene::Field::asset("world_script", "Round script", bozzard_scene::AssetKind::Script),
                    ];
                    FIELDS
                },
                get: |object, key| {
                    let config = object.extra(COMPONENT)?;
                    Some(match key {
                        "app_id" => bozzard_scene::FieldValue::Text(config[key].as_u64()?.to_string()),
                        "game" => bozzard_scene::FieldValue::Text(config[key].as_str()?.into()),
                        "player_script" | "world_script" => bozzard_scene::FieldValue::Asset(Some(config[key].as_str()?.into())),
                        _ => return None,
                    })
                },
                set: |object, key, value| {
                    let mut config = object.extra(COMPONENT).context("missing Steam settings")?.clone();
                    match key {
                        "app_id" => {
                            let id: u32 = value.text()?.trim().parse().context("Enter a positive Steam App ID")?;
                            config[key] = id.into();
                        }
                        "game" => config[key] = value.text()?.into(),
                        "player_script" | "world_script" => {
                            let bozzard_scene::FieldValue::Asset(Some(asset)) = value else {
                                anyhow::bail!("Choose a script asset");
                            };
                            config[key] = asset.into();
                        }
                        _ => anyhow::bail!("Unknown Steam setting"),
                    }
                    validate_config(&config)?;
                    object.set_extra(COMPONENT, config);
                    Ok(())
                },
                present: |object| object.extra(COMPONENT).is_some(),
                available: |_| false,
                add: |_, _| anyhow::bail!("Open a configured multiplayer scene"),
                remove: |object, _| { object.extras.remove(COMPONENT); },
                merge: |current, old, source| {
                    if current.extra(COMPONENT) == old.extra(COMPONENT) {
                        match source.extra(COMPONENT) {
                            Some(value) => current.set_extra(COMPONENT, value.clone()),
                            None => { current.extras.remove(COMPONENT); }
                        }
                    }
                },
                load: |object, value| {
                    validate_config(&value)?;
                    object.set_extra(COMPONENT, value);
                    Ok(())
                },
                save: |object| Ok(object.extra(COMPONENT).cloned()),
            }).map_err(|error| error.to_string())?;
            register_bindings().map_err(|error| error.to_string())
        })
        .clone()
        .map_err(anyhow::Error::msg)
}

fn register_bindings() -> Result<()> {
    macro_rules! binding {
        ($name:literal, $label:literal, $key:literal, $max:literal) => {
            bozzard_scene::register_component(bozzard_scene::ComponentType {
                name: $name, label: $label, ui: bozzard_scene::Ui::Generic,
                help: "Binds a locally authored object to replicated state. Attach a Script Manager script to present it.",
                fields: || {
                    const FIELDS: &[bozzard_scene::Field] = &[
                        bozzard_scene::Field::integer_range($key, "Slot (zero-based)", 1., 0., $max as f32),
                    ];
                    FIELDS
                },
                get: |object, key| {
                    if key != $key { return None; }
                    Some(bozzard_scene::FieldValue::Number(object.extra($name)?[$key].as_u64()? as f32))
                },
                set: |object, key, value| {
                    ensure!(key == $key, "unknown binding field");
                    let value = value.number()?;
                    ensure!(value.is_finite() && value.fract() == 0. && (0. ..=$max as f32).contains(&value), "binding index out of bounds");
                    object.set_extra($name, serde_json::json!({$key: value as u8}));
                    Ok(())
                },
                present: |object| object.extra($name).is_some(),
                available: |_| true,
                add: |object, _| { object.set_extra($name, serde_json::json!({$key: 0})); Ok(()) },
                remove: |object, _| { object.extras.remove($name); },
                merge: |current, old, source| {
                    if current.extra($name) == old.extra($name) {
                        match source.extra($name) {
                            Some(value) => current.set_extra($name, value.clone()),
                            None => { current.extras.remove($name); }
                        }
                    }
                },
                load: |object, value| {
                    ensure!(value.as_object().is_some_and(|map| map.len() == 1)
                        && value[$key].as_u64().is_some_and(|index| index <= $max), "invalid network binding");
                    object.set_extra($name, value);
                    Ok(())
                },
                save: |object| Ok(object.extra($name).cloned()),
            })?;
        };
    }
    binding!("network_player", "Network Player", "slot", 3);
    binding!("network_obstacle", "Network Obstacle", "index", 2);
    Ok(())
}

const ACTIONS: [&str; 15] = [
    "steam-chat",
    "steam-chat-send",
    "steam-chat-back",
    "steam-create",
    "steam-invite",
    "steam-start",
    "steam-leave",
    "steam-quit",
    "steam-friends",
    "steam-friends-next",
    "steam-friends-back",
    "steam-friend-0",
    "steam-friend-1",
    "steam-friend-2",
    "steam-friend-3",
];
pub struct Multiplayer {
    rules: std::sync::Arc<bozzard_network::rules::Rules>,
    title: String,
    players: Vec<(String, u8)>,
    obstacles: Vec<(String, usize)>,
    backend: Box<dyn Backend>,
    pub quit: bool,
    friends: std::sync::Arc<Vec<(Peer, String)>>,
    friend_page: usize,
    picking: bool,
    pub chatting: bool,
    draft: String,
}
impl Multiplayer {
    pub fn new(instance: &SceneInstance, join: Option<u64>) -> Result<Self> {
        let scene = instance.document();
        validate(scene)?;
        #[cfg(feature = "steam")]
        {
            Self::with_backend(
                instance,
                Box::new(Threaded::new(Box::new(
                    bozzard_network::steam::Session::new(
                        app_id(scene)?.context("missing Steam settings")?,
                        config(scene)?["game"].as_str().unwrap(),
                        rules_for(instance)?,
                    )?,
                ))?),
                join,
            )
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = join;
            anyhow::bail!(
                "Steam multiplayer is disabled in this build. Use the standard editor/player build (cargo build -p bozzard-editor-app -p bozzard-player)."
            );
        }
    }
    /// Attach a transport to a prepared runtime, on the application's main thread.
    pub fn with_backend(
        instance: &SceneInstance,
        mut backend: Box<dyn Backend>,
        join: Option<u64>,
    ) -> Result<Self> {
        let scene = instance.document();
        validate(scene)?;
        let rules = rules_for(instance)?;
        let (players, obstacles) = bindings(scene)?;
        if let Some(id) = join {
            backend.action(Action::Join(id))?;
        }
        Ok(Self {
            rules,
            title: scene.name.clone(),
            players,
            obstacles,
            backend,
            quit: false,
            friends: std::sync::Arc::new(Vec::new()),
            friend_page: 0,
            picking: false,
            chatting: false,
            draft: String::new(),
        })
    }
    pub fn text(&mut self, text: &str) -> bool {
        if !self.chatting {
            return false;
        }
        let available =
            bozzard_network::chat::MAX_CHAT_CHARS.saturating_sub(self.draft.chars().count());
        self.draft
            .push_str(&bozzard_network::chat::clean_text(text, available));
        true
    }
    fn send_chat(&mut self) {
        if self.draft.trim().is_empty() {
            return;
        }
        match self.backend.action(Action::Chat(self.draft.clone())) {
            Ok(()) => self.draft.clear(),
            Err(error) => self.backend.error(error.to_string()),
        }
    }
    pub fn key(&mut self, key: &str) -> bool {
        if self.chatting {
            match key {
                "Escape" => self.chatting = false,
                "Enter" | "NumpadEnter" => self.send_chat(),
                "Backspace" => {
                    self.draft.pop();
                }
                _ => {}
            }
            return true;
        }
        let pressed = match self.rules.input(key) {
            Ok(pressed) => pressed,
            Err(error) => {
                let _ = self.backend.action(Action::Leave);
                self.backend.error(error.to_string());
                return true;
            }
        };
        let action = match key {
            _ if pressed => Action::Flap,
            "L" => Action::Leave,
            "Q" | "Escape" => {
                self.quit = true;
                Action::Leave
            }
            _ => return false,
        };
        self.command(action);
        true
    }
    fn command(&mut self, action: Action) {
        if matches!(action, Action::Leave | Action::Join(_)) {
            self.picking = false;
            self.chatting = false;
            self.draft.clear();
        }
        if let Err(error) = self.backend.action(action) {
            self.backend.error(error.to_string());
        }
    }
    pub fn join(&mut self, id: u64) -> Result<()> {
        self.backend.action(Action::Join(id))
    }
    pub fn title(&self) -> String {
        format!(
            "{} | {} | {:?}",
            self.title,
            if self.backend.view().host {
                "HOST"
            } else {
                "GUEST"
            },
            self.backend.view().replica.phase
        )
    }
    fn control(demo: &mut SceneDemo, id: &str, control: Control) -> Result<()> {
        if demo.instance().entity(id).is_none() {
            return Ok(());
        }
        let unchanged = demo
            .app
            .world
            .resource::<Runtime>()
            .and_then(|runtime| runtime.widgets.get(id))
            .is_some_and(|state| match &control {
                Control::Text(text) => state.text.as_deref() == Some(text.as_str()),
                Control::Value(value) => state.value == Some(*value),
                Control::Visible(visible) => state.visible == Some(*visible),
                Control::Enabled(enabled) => state.enabled == Some(*enabled),
                Control::Focus => false,
            });
        if unchanged {
            return Ok(());
        }
        demo.with_instance(|instance, world| instance.control_ui(world, id, control))
    }
    pub fn ui_input(
        &mut self,
        demo: &mut SceneDemo,
        layer: Layer,
        size: [f32; 2],
        input: Input,
    ) -> Result<bool> {
        // Dispatch only the authored lobby signals. The solo Start/Retry actions never run.
        let consumed =
            demo.with_instance(|instance, world| instance.ui_input(world, layer, size, input))?;
        let actions: Vec<_> = demo
            .app
            .world
            .resource::<Signals>()
            .map(|s| {
                ACTIONS
                    .iter()
                    .filter(|id| s.for_owner(id, Kind::Ui).next().is_some())
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        if let Some(signals) = demo.app.world.resource_mut::<Signals>() {
            signals.begin(Kind::Ui);
        }
        for action in actions {
            let result = match action {
                "steam-chat" => {
                    self.chatting = true;
                    self.picking = false;
                    Ok(())
                }
                "steam-chat-send" => {
                    self.send_chat();
                    Ok(())
                }
                "steam-chat-back" => {
                    self.chatting = false;
                    Ok(())
                }
                "steam-create" => self.backend.action(Action::Create),
                "steam-invite" => {
                    if self.backend.view().overlay {
                        self.backend.action(Action::Overlay)
                    } else {
                        self.open_friends();
                        Ok(())
                    }
                }
                "steam-friends" => {
                    self.open_friends();
                    Ok(())
                }
                "steam-friends-back" => {
                    self.picking = false;
                    Ok(())
                }
                "steam-friends-next" => {
                    self.friend_page =
                        (self.friend_page + 1) % self.friends.len().div_ceil(4).max(1);
                    Ok(())
                }
                "steam-start" => self.backend.action(Action::Start),
                "steam-leave" => {
                    self.command(Action::Leave);
                    Ok(())
                }
                "steam-quit" => {
                    self.command(Action::Leave);
                    self.quit = true;
                    Ok(())
                }
                action if action.starts_with("steam-friend-") => {
                    if let Ok(slot) = action.trim_start_matches("steam-friend-").parse::<usize>()
                        && let Some((id, _)) = self.friends.get(self.friend_page * 4 + slot)
                    {
                        self.command(Action::Invite(*id));
                    }
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(error) = result {
                self.backend.error(error.to_string());
            }
        }
        self.present(demo)?;
        Ok(consumed)
    }
    pub fn update(&mut self, demo: &mut SceneDemo) -> Result<()> {
        self.backend.update()?;
        if self.backend.view().lobby.is_none() {
            self.picking = false;
            self.draft.clear();
        }
        if self.backend.view().lobby.is_none() || self.backend.view().replica.phase.round_active() {
            self.picking = false;
            self.chatting = false;
        }
        self.present(demo)
    }
    fn open_friends(&mut self) {
        self.friends = std::sync::Arc::new(self.backend.friends());
        self.friend_page = 0;
        self.picking = true;
    }
    fn present(&self, demo: &mut SceneDemo) -> Result<()> {
        let net = self.backend.view();
        let playing = net.replica.phase.round_active();
        Self::control(
            demo,
            "steam-countdown",
            Control::Visible(net.replica.phase.countdown_seconds().is_some()),
        )?;
        if let Some(seconds) = net.replica.phase.countdown_seconds() {
            Self::control(demo, "steam-countdown", Control::Text(seconds.to_string()))?;
        }
        let joined = net.lobby.is_some();
        let mut status = net.status.to_owned();
        if let Some(lobby) = net.lobby {
            status = format!(
                "Lobby {} · {}\n",
                lobby,
                if net.host {
                    "You are the host"
                } else {
                    "Waiting for host / host controls rounds"
                }
            );
            for (id, name) in net.members {
                let name: String = name.chars().filter(|c| !c.is_control()).take(32).collect();
                status.push_str(&format!(
                    "{}{}{}\n",
                    name,
                    if *id == net.local { " (you)" } else { "" },
                    if Some(*id) == net.owner {
                        " · host"
                    } else {
                        ""
                    }
                ));
            }
            if net.replica.phase == Phase::Finished {
                status.push_str("Round over. Host can start again.\n");
            }
            status.push_str(net.status);
        }
        Self::control(demo, "steam-status", Control::Text(status))?;
        Self::control(
            demo,
            "steam-status",
            Control::Visible(!playing && !self.chatting),
        )?;
        for (id, enabled) in [
            ("steam-chat", joined),
            ("steam-create", !joined && !net.busy),
            ("steam-invite", joined),
            ("steam-friends", joined),
            ("steam-start", net.can_start),
            ("steam-leave", joined),
            ("steam-quit", true),
        ] {
            Self::control(demo, id, Control::Enabled(enabled))?;
            Self::control(
                demo,
                id,
                Control::Visible(!playing && !self.picking && !self.chatting),
            )?;
        }
        let chatting = self.chatting && joined && !playing;
        for id in [
            "steam-chat-log",
            "steam-chat-draft",
            "steam-chat-send",
            "steam-chat-back",
        ] {
            Self::control(demo, id, Control::Visible(chatting))?;
        }
        if chatting {
            let history = self.backend.chat().text();
            let history = format!(
                "{}\n\n{}",
                net.status,
                if history.is_empty() {
                    "Say hello to your friends.".into()
                } else {
                    history
                }
            );
            Self::control(demo, "steam-chat-log", Control::Text(history))?;
            Self::control(
                demo,
                "steam-chat-draft",
                Control::Text(format!("> {}▏", self.draft)),
            )?;
        }
        // Picker buttons are optional for older authored copies of the scene.
        if self.picking && !playing {
            let title = if self.friends.is_empty() {
                "No Steam friends found. Add friends in Steam, or share the lobby ID.".to_owned()
            } else {
                format!(
                    "Invite a Steam friend · page {} of {}\n{}",
                    self.friend_page + 1,
                    self.friends.len().div_ceil(4),
                    net.status
                )
            };
            Self::control(demo, "steam-status", Control::Text(title))?;
        }
        for slot in 0..4 {
            let id = format!("steam-friend-{slot}");
            if let Some((_, name)) = self.friends.get(self.friend_page * 4 + slot) {
                let name: String = name.chars().filter(|c| !c.is_control()).take(32).collect();
                Self::control(demo, &id, Control::Text(format!("Invite {name}")))?;
            }
            Self::control(
                demo,
                &id,
                Control::Visible(
                    self.picking
                        && !playing
                        && self.friends.get(self.friend_page * 4 + slot).is_some(),
                ),
            )?;
        }
        for id in ["steam-friends-next", "steam-friends-back"] {
            Self::control(demo, id, Control::Visible(self.picking && !playing))?;
        }
        // Bind bounded transport state to locally authored objects. Script Manager
        // owns all game transforms, tilt, elimination visuals and HUD formatting.
        let alpha = if net.host { 1. } else { net.alpha };
        let mut frame = NetworkFrame {
            active: true,
            ..Default::default()
        };
        let mut players = Vec::new();
        for (peer, bird) in &net.replica.birds {
            let mut state = serde_json::to_value(bird)?;
            state["local"] = (*peer == net.local).into();
            players.push(state);
        }
        frame.state = serde_json::json!({"players": players});
        for (object, slot) in &self.players {
            if let Some((peer, _)) = net
                .replica
                .birds
                .iter()
                .find(|(_, bird)| bird.slot == *slot)
                && let Some(bird) = net.replica.render_bird(*peer, net.local, alpha)
            {
                frame
                    .objects
                    .insert(object.clone(), serde_json::to_value(bird)?);
            }
        }
        if let Some(pipes) = net.replica.render_pipes(alpha) {
            for (object, index) in &self.obstacles {
                frame
                    .objects
                    .insert(object.clone(), serde_json::to_value(pipes[*index])?);
            }
        }
        demo.app.world.insert_resource(frame);
        demo.with_instance(|instance, world| {
            instance.step_scripts(world, bozzard_network::DT, GameplayInput::default())
        })?;
        Ok(())
    }
}

fn validate(scene: &Scene) -> Result<()> {
    app_id(scene)?.context("missing Steam settings")?;
    bindings(scene)?;
    Ok(())
}

fn config(scene: &Scene) -> Result<&serde_json::Value> {
    scene
        .objects
        .iter()
        .find_map(|object| object.extra(COMPONENT))
        .context("missing Steam settings")
}

/// Select loaded catalog scripts. No engine-owned source or game-name switch.
pub fn rules_for(
    instance: &SceneInstance,
) -> Result<std::sync::Arc<bozzard_network::rules::Rules>> {
    let scene = instance.document();
    validate(scene)?;
    let config = config(scene)?;
    let player = config["player_script"].as_str().unwrap();
    let world = config["world_script"].as_str().unwrap();
    for object in scene
        .objects
        .iter()
        .filter(|object| object.extra("network_player").is_some())
    {
        ensure!(
            object.script_manager.as_ref().is_some_and(|manager| manager
                .scripts
                .iter()
                .any(|a| a.enabled && a.script == player)),
            "network player '{}' needs the enabled '{player}' Script Manager attachment",
            object.id
        );
    }
    for asset in [player, world] {
        ensure!(
            scene
                .objects
                .iter()
                .any(
                    |object| object.script_manager.as_ref().is_some_and(|manager| manager
                        .scripts
                        .iter()
                        .any(|a| a.enabled && a.script == asset))
                ),
            "network script '{asset}' must be enabled in a Script Manager"
        );
    }
    bozzard_network::rules::Rules::new(
        instance.script_module(player)?,
        instance.script_module(world)?,
    )
}

type Bindings = (Vec<(String, u8)>, Vec<(String, usize)>);
fn bindings(scene: &Scene) -> Result<Bindings> {
    let mut players = Vec::new();
    let mut obstacles = Vec::new();
    for object in &scene.objects {
        if let Some(binding) = object.extra("network_player") {
            let slot = binding["slot"]
                .as_u64()
                .context("network player needs an integer slot")?;
            ensure!(
                slot < bozzard_network::MAX_PLAYERS as u64,
                "network slot out of bounds"
            );
            players.push((object.id.clone(), slot as u8));
        }
        if let Some(binding) = object.extra("network_obstacle") {
            let index = binding["index"]
                .as_u64()
                .context("network obstacle needs an integer index")?;
            ensure!(index < 3, "network obstacle index out of bounds");
            obstacles.push((object.id.clone(), index as usize));
        }
    }
    ensure!(
        players.len() == bozzard_network::MAX_PLAYERS
            && players
                .iter()
                .map(|(_, slot)| slot)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == players.len(),
        "network scene needs one binding for each player slot"
    );
    ensure!(
        obstacles.len() == 3
            && obstacles
                .iter()
                .map(|(_, index)| index)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == obstacles.len(),
        "network scene needs one binding for each obstacle"
    );
    Ok((players, obstacles))
}

pub fn app_id(scene: &Scene) -> Result<Option<u32>> {
    let mut id = None;
    for config in scene
        .objects
        .iter()
        .filter_map(|object| object.extra(COMPONENT))
    {
        ensure!(
            id.is_none(),
            "A scene must have only one Steam multiplayer settings component"
        );
        validate_config(config)?;
        id = Some(config["app_id"].as_u64().unwrap() as u32);
    }
    Ok(id)
}

fn validate_config(config: &serde_json::Value) -> Result<()> {
    ensure!(
        config.as_object().is_some_and(|value| value.len() == 6)
            && config["game"].as_str().is_some_and(|name| !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
            && ["player_script", "world_script"]
                .iter()
                .all(|key| config[*key]
                    .as_str()
                    .is_some_and(|asset| !asset.trim().is_empty() && asset.len() <= 256))
            && config["protocol"] == bozzard_network::PROTOCOL
            && config["max_players"] == 4
            && config["app_id"]
                .as_u64()
                .is_some_and(|id| id > 0 && id <= u64::from(u32::MAX)),
        "invalid scripted Steam settings; expected game, protocol, app_id, max_players, player_script and world_script"
    );
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Create,
    Chat(String),
    Join(u64),
    Overlay,
    Invite(Peer),
    Start,
    Flap,
    Leave,
}
/// Narrow transport boundary also used by headless editor lifecycle tests.
pub trait Backend: Send {
    fn update(&mut self) -> Result<()>;
    fn action(&mut self, action: Action) -> Result<()>;
    fn error(&mut self, message: String);
    fn view(&self) -> View<'_>;
    fn chat(&self) -> bozzard_network::chat::ChatLog {
        Default::default()
    }
    fn friends(&self) -> Vec<(Peer, String)> {
        Vec::new()
    }
}
pub struct View<'a> {
    pub replica: &'a Replica,
    pub status: &'a str,
    pub members: &'a BTreeMap<Peer, String>,
    pub lobby: Option<u64>,
    pub owner: Option<Peer>,
    pub local: Peer,
    pub host: bool,
    pub can_start: bool,
    pub busy: bool,
    pub overlay: bool,
    pub alpha: f32,
}
#[cfg(feature = "steam")]
impl Backend for bozzard_network::steam::Session {
    fn chat(&self) -> bozzard_network::chat::ChatLog {
        self.chat.clone()
    }
    fn update(&mut self) -> Result<()> {
        self.update()
    }
    fn action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Create => self.create(),
            Action::Chat(text) => self.send_chat(&text),
            Action::Join(id) => self.join(id),
            Action::Overlay => self.invite(),
            Action::Invite(id) => self.invite_friend(id),
            Action::Start => self.start(),
            Action::Flap => {
                self.flap();
                Ok(())
            }
            Action::Leave => {
                self.leave();
                Ok(())
            }
        }
    }
    fn error(&mut self, message: String) {
        self.status = message;
    }
    fn friends(&self) -> Vec<(Peer, String)> {
        self.friends()
    }
    fn view(&self) -> View<'_> {
        View {
            replica: &self.replica,
            status: &self.status,
            members: &self.members,
            lobby: self.lobby.map(|id| id.raw()),
            owner: self.owner,
            local: self.local,
            host: self.is_host(),
            can_start: self.can_start(),
            busy: self.busy(),
            overlay: self.overlay_available(),
            alpha: self.interpolation(),
        }
    }
}

/// Network pumping belongs to the Play session, independent of editor redraw/minimize.
/// Only copied presentation state crosses back to the UI; the scene world stays on the UI thread.
pub struct Threaded {
    commands: std::sync::mpsc::SyncSender<Action>,
    shared: std::sync::Arc<std::sync::Mutex<std::sync::Arc<OwnedView>>>,
    cached: std::sync::Arc<OwnedView>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
#[derive(Clone)]
struct OwnedView {
    replica: Replica,
    status: String,
    members: BTreeMap<Peer, String>,
    lobby: Option<u64>,
    owner: Option<Peer>,
    local: Peer,
    host: bool,
    can_start: bool,
    busy: bool,
    overlay: bool,
    alpha: f32,
    friends: std::sync::Arc<Vec<(Peer, String)>>,
    chat: bozzard_network::chat::ChatLog,
}
impl OwnedView {
    fn capture(backend: &dyn Backend, friends: std::sync::Arc<Vec<(Peer, String)>>) -> Self {
        let v = backend.view();
        Self {
            chat: backend.chat(),
            replica: v.replica.clone(),
            status: v.status.into(),
            members: v.members.clone(),
            lobby: v.lobby,
            owner: v.owner,
            local: v.local,
            host: v.host,
            can_start: v.can_start,
            busy: v.busy,
            overlay: v.overlay,
            alpha: v.alpha,
            friends,
        }
    }
    fn view(&self) -> View<'_> {
        View {
            replica: &self.replica,
            status: &self.status,
            members: &self.members,
            lobby: self.lobby,
            owner: self.owner,
            local: self.local,
            host: self.host,
            can_start: self.can_start,
            busy: self.busy,
            overlay: self.overlay,
            alpha: self.alpha,
        }
    }
}
impl Threaded {
    pub fn new(mut backend: Box<dyn Backend>) -> Result<Self> {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        let cached = Arc::new(OwnedView::capture(&*backend, Arc::new(backend.friends())));
        let shared = Arc::new(Mutex::new(Arc::clone(&cached)));
        let output = Arc::clone(&shared);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let (commands, receive) = mpsc::sync_channel(64);
        let worker = std::thread::Builder::new()
            .name("steam-play".into())
            .spawn(move || {
                let mut friends = Arc::new(backend.friends());
                let mut refresh = std::time::Instant::now();
                while !stopped.load(Ordering::Acquire) {
                    for action in receive.try_iter().take(64) {
                        if let Err(error) = backend.action(action) {
                            backend.error(error.to_string());
                        }
                    }
                    if let Err(error) = backend.update() {
                        let _ = backend.action(Action::Leave);
                        backend.error(error.to_string());
                    }
                    if refresh.elapsed() >= std::time::Duration::from_secs(1) {
                        friends = Arc::new(backend.friends());
                        refresh = std::time::Instant::now();
                    }
                    *output.lock().unwrap() =
                        Arc::new(OwnedView::capture(&*backend, Arc::clone(&friends)));
                    std::thread::sleep(std::time::Duration::from_millis(8));
                }
                let _ = backend.action(Action::Leave);
            })?;
        Ok(Self {
            commands,
            shared,
            cached,
            stop,
            worker: Some(worker),
        })
    }
}
impl Backend for Threaded {
    fn chat(&self) -> bozzard_network::chat::ChatLog {
        self.cached.chat.clone()
    }
    fn update(&mut self) -> Result<()> {
        ensure!(
            !self.worker.as_ref().is_some_and(|w| w.is_finished()),
            "Steam Play worker stopped"
        );
        self.cached = std::sync::Arc::clone(&self.shared.lock().unwrap());
        Ok(())
    }
    fn action(&mut self, action: Action) -> Result<()> {
        self.commands
            .try_send(action)
            .map_err(|e| anyhow::anyhow!("Steam command queue unavailable: {e}"))
    }
    fn error(&mut self, message: String) {
        std::sync::Arc::make_mut(&mut self.cached).status = message;
    }
    fn view(&self) -> View<'_> {
        self.cached.view()
    }
    fn friends(&self) -> Vec<(Peer, String)> {
        self.cached.friends.as_ref().clone()
    }
}
impl Drop for Threaded {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
