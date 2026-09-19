//! Editor Play integration through the same UI/worker boundary used by real Steam.
use anyhow::Result;
use bozzard_demo::{
    SceneDemo,
    multiplayer::{Action, Backend, Multiplayer, Threaded, View},
};
use bozzard_editor::Editor;
use bozzard_network::{
    Message,
    flap::{Host, InputFrame, Replica},
};
use bozzard_scene::{Layer, Scene, Transform, middleware::ui::Input};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct FakeSteam {
    host: Host,
    local: u64,
    chat: bozzard_network::chat::ChatLog,
    replica: Replica,
    members: BTreeMap<u64, String>,
    lobby: Option<u64>,
    status: String,
    calls: Arc<Mutex<Vec<Action>>>,
    updates: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    sequence: u64,
}
const COUNTDOWN_TICKS: u16 = 300;
fn scene_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/flap-woods-multiplayer.json")
}
fn runtime(scene: &Scene) -> SceneDemo {
    SceneDemo::new_with_prefabs(scene, Some(&scene_path())).unwrap()
}
impl FakeSteam {
    fn new(
        calls: Arc<Mutex<Vec<Action>>>,
        updates: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    ) -> Self {
        let rules = bozzard_demo::multiplayer::rules_for(runtime(&scene()).instance()).unwrap();
        let mut host = Host::new(10, rules.clone()).unwrap();
        host.join(20).unwrap();
        Self {
            host,
            local: 10,
            chat: Default::default(),
            replica: Replica::new(rules),
            members: [(10, "Host".into()), (20, "Guest".into())].into(),
            lobby: None,
            status: "Ready".into(),
            calls,
            updates,
            dropped,
            sequence: 0,
        }
    }
}
impl Drop for FakeSteam {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl Backend for FakeSteam {
    fn chat(&self) -> bozzard_network::chat::ChatLog {
        self.chat.clone()
    }
    fn update(&mut self) -> Result<()> {
        self.updates.fetch_add(1, Ordering::SeqCst);
        self.host.step()?;
        self.replica
            .apply(10, 10, self.local, self.host.snapshot(self.local)?)?;
        Ok(())
    }
    fn action(&mut self, action: Action) -> Result<()> {
        self.calls.lock().unwrap().push(action.clone());
        match action {
            Action::Chat(text) => {
                let mut bytes = bozzard_network::chat::CHAT_PREFIX.to_vec();
                bytes.extend_from_slice(text.as_bytes());
                self.chat.receive(&self.members[&self.local], &bytes);
            }
            Action::Create => self.lobby = Some(999),
            Action::Join(id) => self.lobby = Some(id),
            Action::Start => self.host.start(10)?,
            Action::Flap => {
                self.sequence += 1;
                self.host.receive(
                    10,
                    Message::Input {
                        round: self.host.round,
                        frames: vec![InputFrame {
                            sequence: self.sequence,
                            flap: true,
                        }],
                        ack: 0,
                    },
                )?;
            }
            Action::Leave => self.lobby = None,
            Action::Invite(_) => self.status = "Invite requested".into(),
            Action::Overlay => anyhow::bail!("no overlay in this test"),
        }
        Ok(())
    }
    fn error(&mut self, message: String) {
        self.status = message;
    }
    fn friends(&self) -> Vec<(u64, String)> {
        vec![(30, "Friend outside lobby".into())]
    }
    fn view(&self) -> View<'_> {
        View {
            replica: &self.replica,
            status: &self.status,
            members: &self.members,
            lobby: self.lobby,
            owner: Some(10),
            local: self.local,
            host: self.local == 10,
            can_start: self.local == 10 && self.lobby.is_some() && !self.host.phase.round_active(),
            busy: false,
            overlay: false,
            alpha: 1.,
        }
    }
}
fn scene() -> Scene {
    Scene::from_json(include_str!(
        "../../../examples/demo/scenes/flap-woods-multiplayer.json"
    ))
    .unwrap()
}
fn click(play: &mut SceneDemo, id: &str) {
    play.ui_input(
        Layer::ThreeD,
        [1280., 720.],
        Input::ActivateObject(id.into()),
    )
    .unwrap();
}
fn wait_until(mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn host_and_guest_show_countdown_then_remove_it_when_play_begins() {
    for local in [10, 20] {
        let source = scene();
        let mut backend =
            FakeSteam::new(Default::default(), Default::default(), Default::default());
        backend.local = local;
        backend.lobby = Some(999);
        backend.host.start(10).unwrap();
        let mut play = runtime(&source);
        play.attach_multiplayer(
            Multiplayer::with_backend(play.instance(), Box::new(backend), None).unwrap(),
        )
        .unwrap();
        // Pump uses one fake host tick per update, independently of renderer timing.
        for tick in 1..COUNTDOWN_TICKS {
            let frame = play
                .instance()
                .ui_frame(&play.app.world, Layer::ThreeD, [1280., 720.])
                .unwrap();
            assert_eq!(
                frame.element("steam-countdown").unwrap().text,
                (5 - tick / 60).to_string()
            );
            assert!(frame.element("steam-start").is_none());
            assert!(frame.element("steam-chat").is_none());
            let bird = play.instance().entity("bird-0").unwrap();
            assert_eq!(
                play.app.world.get::<Transform>(bird).unwrap().translation[1],
                0.65
            );
            play.pump_multiplayer().unwrap();
        }
        let frame = play
            .instance()
            .ui_frame(&play.app.world, Layer::ThreeD, [1280., 720.])
            .unwrap();
        assert!(frame.element("steam-countdown").is_none());
        assert!(frame.element("steam-start").is_none());
    }
}

#[test]
fn scripts_present_renamed_objects_through_authored_bindings() {
    let mut source = scene();
    for object in &mut source.objects {
        if object.id == "bird-0" {
            object.id = "my-player".into();
        }
        if object.parent.as_deref() == Some("bird-0") {
            object.parent = Some("my-player".into());
        }
        if object.id == "score" {
            object.id = "my-scoreboard".into();
        }
    }
    let mut play = runtime(&source);
    let backend = FakeSteam::new(Default::default(), Default::default(), Default::default());
    play.attach_multiplayer(
        Multiplayer::with_backend(play.instance(), Box::new(backend), None).unwrap(),
    )
    .unwrap();
    let player = play.instance().entity("my-player").unwrap();
    assert_eq!(
        play.app.world.get::<Transform>(player).unwrap().translation,
        [-5., 0.65, 0.]
    );
    let score = play.instance().entity("my-scoreboard").unwrap();
    assert!(
        play.app
            .world
            .get::<bozzard_scene::TextRendering>(score)
            .unwrap()
            .text
            .contains("P1 YOU: 0")
    );
}

#[test]
fn lobby_chat_types_without_triggering_game_keys_and_clears_on_leave() {
    let source = scene();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let backend = FakeSteam::new(calls.clone(), Default::default(), Default::default());
    let mut play = runtime(&source);
    play.attach_multiplayer(
        Multiplayer::with_backend(play.instance(), Box::new(backend), None).unwrap(),
    )
    .unwrap();
    click(&mut play, "steam-create");
    click(&mut play, "steam-chat");
    assert!(play.multiplayer_chatting());
    for key in ["Q", "L", "Space"] {
        assert!(play.multiplayer_key(key));
    }
    assert!(play.multiplayer_text("Hello Q L 🌲!"));
    play.multiplayer_key("Backspace");
    play.multiplayer_key("Enter");
    play.pump_multiplayer().unwrap();
    let frame = play
        .instance()
        .ui_frame(&play.app.world, Layer::ThreeD, [1280., 720.])
        .unwrap();
    assert!(
        frame
            .element("steam-chat-log")
            .unwrap()
            .text
            .contains("Host: Hello Q L 🌲")
    );
    assert!(
        !frame
            .element("steam-chat-draft")
            .unwrap()
            .text
            .contains("Hello")
    );
    assert!(frame.element("steam-start").is_none());
    assert!(!play.multiplayer_quit());
    assert_eq!(
        *calls.lock().unwrap(),
        vec![Action::Create, Action::Chat("Hello Q L 🌲".into())]
    );
    play.multiplayer_key("Escape");
    play.pump_multiplayer().unwrap();
    assert!(!play.multiplayer_chatting());
    click(&mut play, "steam-chat");
    play.multiplayer_text("Unsent");
    play.multiplayer_key("Escape");
    play.pump_multiplayer().unwrap();
    click(&mut play, "steam-leave");
    play.pump_multiplayer().unwrap();
    click(&mut play, "steam-create");
    click(&mut play, "steam-chat");
    let frame = play
        .instance()
        .ui_frame(&play.app.world, Layer::ThreeD, [1280., 720.])
        .unwrap();
    assert!(
        !frame
            .element("steam-chat-draft")
            .unwrap()
            .text
            .contains("Unsent")
    );
}

#[test]
fn editor_routes_lobby_ui_flaps_and_overlay_free_invites_without_touching_edit_scene() {
    let source = scene();
    let mut editor = Editor::new(source.clone(), &scene_path()).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let dropped = Arc::new(AtomicUsize::new(0));
    let backend = FakeSteam::new(
        calls.clone(),
        Arc::new(AtomicUsize::new(0)),
        dropped.clone(),
    );
    let mut play = runtime(&source);
    play.attach_multiplayer(
        Multiplayer::with_backend(play.instance(), Box::new(backend), None).unwrap(),
    )
    .unwrap();
    editor.play = Some(play);
    click(editor.play.as_mut().unwrap(), "steam-create");
    click(editor.play.as_mut().unwrap(), "steam-invite");
    let play = editor.play.as_mut().unwrap();
    let frame = play
        .instance()
        .ui_frame(&play.app.world, Layer::ThreeD, [1280., 720.])
        .unwrap();
    assert!(frame.element("steam-friend-0").is_some());
    assert!(frame.element("steam-start").is_none());
    click(play, "steam-friend-0");
    click(play, "steam-friends-back");
    click(play, "steam-start");
    assert!(play.multiplayer_key("Space"));
    for _ in 0..COUNTDOWN_TICKS {
        editor.advance(Duration::from_millis(17));
    }
    assert!(editor.play.as_mut().unwrap().multiplayer_key("Space"));
    editor.advance(Duration::from_millis(17));
    let play = editor.play.as_ref().unwrap();
    let bird = play.instance().entity("bird-0").unwrap();
    assert!(play.app.world.get::<Transform>(bird).unwrap().translation[1] > 0.65);
    assert_eq!(
        play.app.ticks(),
        0,
        "editor must not run solo systems alongside the network host"
    );
    assert_eq!(editor.scene(), &source);
    assert!(calls.lock().unwrap().contains(&Action::Invite(30)));
    assert!(!calls.lock().unwrap().contains(&Action::Overlay));
    assert!(
        editor
            .play
            .as_mut()
            .unwrap()
            .debug_command(bozzard_scene::DebugCommand::StepTick)
            .is_err()
    );
    editor.stop_play();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(!editor.dirty());
    assert_eq!(editor.scene(), &source);
}

#[test]
fn network_worker_runs_without_editor_redraw_and_stop_joins_and_leaves() {
    let source = scene();
    let mut editor = Editor::new(source.clone(), &scene_path()).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let updates = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let backend = FakeSteam::new(calls.clone(), updates.clone(), dropped.clone());
        let worker = Threaded::new(Box::new(backend)).unwrap();
        let mut play = runtime(&source);
        let net = Multiplayer::with_backend(play.instance(), Box::new(worker), Some(777)).unwrap();
        play.attach_multiplayer(net).unwrap();
        editor.play = Some(play);
        let before = updates.load(Ordering::SeqCst);
        wait_until(|| updates.load(Ordering::SeqCst) >= before + 3);
        editor.stop_play();
        let stopped = updates.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(updates.load(Ordering::SeqCst), stopped);
    }
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|a| **a == Action::Leave)
            .count(),
        2
    );
    assert!(calls.lock().unwrap().contains(&Action::Join(777)));
    assert_eq!(editor.scene(), &source);
}

#[test]
fn quit_stops_play_instead_of_closing_the_editor() {
    let source = scene();
    let mut editor = Editor::new(source.clone(), &scene_path()).unwrap();
    let backend = FakeSteam::new(Default::default(), Default::default(), Default::default());
    let mut play = runtime(&source);
    play.attach_multiplayer(
        Multiplayer::with_backend(play.instance(), Box::new(backend), None).unwrap(),
    )
    .unwrap();
    editor.play = Some(play);
    click(editor.play.as_mut().unwrap(), "steam-quit");
    editor.advance(Duration::from_millis(17));
    assert!(editor.play.is_none());
    assert_eq!(editor.scene(), &source);
}

#[test]
fn solo_scene_plays_through_both_editor_entry_points_without_a_steam_session() {
    let source = Scene::from_json(include_str!(
        "../../../examples/demo/scenes/flap-woods.json"
    ))
    .unwrap();
    let mut editor = Editor::new(
        source.clone(),
        &std::env::temp_dir().join("solo-editor-steam-regression.json"),
    )
    .unwrap();
    // Opening legacy solo scenes materializes their standard game-menu widgets.
    let authored = editor.scene().clone();
    editor.start_play().unwrap();
    assert!(!editor.play.as_ref().unwrap().multiplayer_active());
    editor.advance(Duration::from_millis(34));
    assert!(editor.play.as_ref().unwrap().app.ticks() > 0);
    editor.stop_play();
    let job = editor.play_job().unwrap();
    let mut prepared = None;
    wait_until(|| {
        prepared = job.poll();
        prepared.is_some()
    });
    editor.accept_play(prepared.unwrap().unwrap()).unwrap();
    assert!(!editor.play.as_ref().unwrap().multiplayer_active());
    editor.advance(Duration::from_millis(34));
    assert!(editor.play.as_ref().unwrap().app.ticks() > 0);
    editor.stop_play();
    assert_eq!(editor.scene(), &authored);
}

#[cfg(not(feature = "steam"))]
#[test]
fn ordinary_editor_rejects_network_play_transactionally_with_launch_instructions() {
    let source = scene();
    let mut editor = Editor::new(source.clone(), &scene_path()).unwrap();
    let revision = editor.asset_revision();
    assert!(
        editor
            .start_play()
            .unwrap_err()
            .to_string()
            .contains("standard editor/player build")
    );
    assert!(editor.play.is_none());
    assert_eq!(editor.asset_revision(), revision);
    assert_eq!(editor.scene(), &source);
    let job = editor.play_job().unwrap();
    let mut completion = None;
    wait_until(|| {
        completion = job.poll();
        completion.is_some()
    });
    assert!(editor.accept_play(completion.unwrap().unwrap()).is_err());
    assert!(editor.play.is_none());
    assert_eq!(editor.asset_revision(), revision);
}
