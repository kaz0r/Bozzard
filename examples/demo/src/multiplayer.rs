//! Shared Play-session UI and presentation for the native player and editor.
use crate::SceneDemo;
use anyhow::{Context, Result, ensure};
use bozzard_network::{
    Peer,
    flap::{Phase, Replica},
};
use bozzard_scene::middleware::{
    signals::{Kind, Signals},
    ui::{Control, Input},
};
use bozzard_scene::{Layer, Scene, Transform};
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
                help: "Flap Woods Together · up to four players. 480 tests with Spacewar; set your own Steam App ID before publishing. Restart the editor after changing App ID.",
                fields: || {
                    const FIELDS: &[bozzard_scene::Field] = &[bozzard_scene::Field::text("app_id", "Steam App ID", "480 for Spacewar testing")];
                    FIELDS
                },
                get: |object, key| {
                    if key != "app_id" { return None; }
                    Some(bozzard_scene::FieldValue::Text(object.extra(COMPONENT)?["app_id"].as_u64()?.to_string()))
                },
                set: |object, key, value| {
                    ensure!(key == "app_id", "Unknown Steam setting");
                    let id: u32 = value.text()?.trim().parse().context("Enter a positive Steam App ID")?;
                    ensure!(id > 0, "Steam App ID must be positive");
                    let mut config = object.extra(COMPONENT).context("missing Steam settings")?.clone();
                    config["app_id"] = id.into();
                    object.set_extra(COMPONENT, config);
                    Ok(())
                },
                present: |object| object.extra(COMPONENT).is_some(),
                available: |_| false,
                add: |_, _| anyhow::bail!("Open the Flap Woods multiplayer example"),
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
            }).map_err(|error| error.to_string())
        })
        .clone()
        .map_err(anyhow::Error::msg)
}

const ACTIONS: [&str; 12] = [
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
    backend: Box<dyn Backend>,
    pub quit: bool,
    friends: Vec<(Peer, String)>,
    friend_page: usize,
    picking: bool,
}
impl Multiplayer {
    pub fn new(scene: &Scene, join: Option<u64>) -> Result<Self> {
        validate(scene)?;
        #[cfg(feature = "steam")]
        {
            Self::with_backend(
                scene,
                Box::new(Threaded::new(Box::new(
                    bozzard_network::steam::Session::new(
                        app_id(scene)?.context("missing Steam settings")?,
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
        scene: &Scene,
        mut backend: Box<dyn Backend>,
        join: Option<u64>,
    ) -> Result<Self> {
        validate(scene)?;
        if let Some(id) = join {
            backend.action(Action::Join(id))?;
        }
        Ok(Self {
            backend,
            quit: false,
            friends: Vec::new(),
            friend_page: 0,
            picking: false,
        })
    }
    pub fn key(&mut self, key: &str) -> bool {
        let action = match key {
            "Space" => Action::Flap,
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
            "Flap Woods Together | {} | {:?}",
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
        }
        self.present(demo)
    }
    fn open_friends(&mut self) {
        self.friends = self.backend.friends();
        self.friend_page = 0;
        self.picking = true;
    }
    fn present(&self, demo: &mut SceneDemo) -> Result<()> {
        let net = self.backend.view();
        let playing = net.replica.phase == Phase::Playing;
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
        Self::control(demo, "steam-status", Control::Visible(!playing))?;
        for (id, enabled) in [
            ("steam-create", !joined && !net.busy),
            ("steam-invite", joined),
            ("steam-friends", joined),
            ("steam-start", net.can_start),
            ("steam-leave", joined),
            ("steam-quit", true),
        ] {
            Self::control(demo, id, Control::Enabled(enabled))?;
            Self::control(demo, id, Control::Visible(!playing && !self.picking))?;
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
        // Leave/Quit remain available as keyboard shortcuts through explicit keys below;
        // the playing canvas is entirely clear so Space always controls your bird.
        for slot in 0..4 {
            let id = format!("bird-{slot}");
            let entity = demo
                .instance()
                .entity(&id)
                .context("missing multiplayer bird")?;
            let bird = net
                .replica
                .birds
                .iter()
                .find(|(_, b)| b.slot == slot)
                .and_then(|(id, _)| {
                    net.replica
                        .render_bird(*id, net.local, if net.host { 1. } else { net.alpha })
                });
            let mut transform = demo
                .app
                .world
                .get_mut::<Transform>(entity)
                .context("missing bird transform")?;
            if let Some(bird) = bird {
                transform.translation = [bird.x(), bird.y, f32::from(slot) * 0.05];
                transform.rotation_degrees[2] = bird.velocity * 4.;
                transform.scale = if bird.alive { [0.8; 3] } else { [0.45; 3] };
            } else {
                transform.translation[1] = 100.;
            }
        }
        if let Some(pipes) = net
            .replica
            .render_pipes(if net.host { 1. } else { net.alpha })
        {
            for (i, pipe) in pipes.iter().enumerate() {
                let id = format!("pipe-{}", i + 1);
                let entity = demo.instance().entity(&id).context("missing pipe")?;
                demo.app
                    .world
                    .get_mut::<Transform>(entity)
                    .unwrap()
                    .translation[0] = pipe.x;
                for (part, offset) in [("bottom", -9.05), ("top", 9.05)] {
                    let entity = demo
                        .instance()
                        .entity(&format!("{id}-{part}"))
                        .context("missing pipe half")?;
                    demo.app
                        .world
                        .get_mut::<Transform>(entity)
                        .unwrap()
                        .translation[1] = pipe.gap + offset;
                }
            }
        }
        let scores = net
            .replica
            .birds
            .iter()
            .map(|(id, b)| {
                format!(
                    "P{}{}: {}{}",
                    b.slot + 1,
                    if *id == net.local { " YOU" } else { "" },
                    b.score,
                    if b.alive { "" } else { " OUT" }
                )
            })
            .collect::<Vec<_>>()
            .join("   ");
        let entity = demo
            .instance()
            .entity("score")
            .context("missing score HUD")?;
        demo.app
            .world
            .get_mut::<bozzard_scene::TextRendering>(entity)
            .unwrap()
            .text = if scores.is_empty() {
            "FLAP WOODS TOGETHER".into()
        } else {
            scores
        };
        Ok(())
    }
}

fn validate(scene: &Scene) -> Result<()> {
    app_id(scene)?.context("missing Steam settings")?;
    Ok(())
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
        config.as_object().is_some_and(|value| value.len() == 4)
            && config["game"] == "flap_woods"
            && config["protocol"] == 1
            && config["max_players"] == 4
            && config["app_id"]
                .as_u64()
                .is_some_and(|id| id > 0 && id <= u64::from(u32::MAX)),
        "unsupported Steam reference game settings"
    );
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Create,
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
    fn update(&mut self) -> Result<()> {
        self.update()
    }
    fn action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Create => self.create(),
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
    shared: std::sync::Arc<std::sync::Mutex<OwnedView>>,
    cached: OwnedView,
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
    friends: Vec<(Peer, String)>,
}
impl OwnedView {
    fn capture(backend: &dyn Backend, friends: Vec<(Peer, String)>) -> Self {
        let v = backend.view();
        Self {
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
        let cached = OwnedView::capture(&*backend, backend.friends());
        let shared = Arc::new(Mutex::new(cached.clone()));
        let output = Arc::clone(&shared);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let (commands, receive) = mpsc::sync_channel(64);
        let worker = std::thread::Builder::new()
            .name("steam-play".into())
            .spawn(move || {
                let mut friends = backend.friends();
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
                        friends = backend.friends();
                        refresh = std::time::Instant::now();
                    }
                    *output.lock().unwrap() = OwnedView::capture(&*backend, friends.clone());
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
    fn update(&mut self) -> Result<()> {
        ensure!(
            !self.worker.as_ref().is_some_and(|w| w.is_finished()),
            "Steam Play worker stopped"
        );
        self.cached = self.shared.lock().unwrap().clone();
        Ok(())
    }
    fn action(&mut self, action: Action) -> Result<()> {
        self.commands
            .try_send(action)
            .map_err(|e| anyhow::anyhow!("Steam command queue unavailable: {e}"))
    }
    fn error(&mut self, message: String) {
        self.cached.status = message;
    }
    fn view(&self) -> View<'_> {
        self.cached.view()
    }
    fn friends(&self) -> Vec<(Peer, String)> {
        self.cached.friends.clone()
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
