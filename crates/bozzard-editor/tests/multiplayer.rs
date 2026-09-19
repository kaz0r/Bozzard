//! Editor Play integration through the same UI/worker boundary used by real Steam.
use anyhow::Result;
use bozzard_demo::{
    SceneDemo,
    multiplayer::{Action, Backend, Multiplayer, Threaded, View},
};
use bozzard_editor::Editor;
use bozzard_network::{
    Message,
    flap::{Host, InputFrame, Phase, Replica},
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
    replica: Replica,
    members: BTreeMap<u64, String>,
    lobby: Option<u64>,
    status: String,
    calls: Arc<Mutex<Vec<Action>>>,
    updates: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
    sequence: u64,
}
impl FakeSteam {
    fn new(
        calls: Arc<Mutex<Vec<Action>>>,
        updates: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    ) -> Self {
        let mut host = Host::new(10);
        host.join(20).unwrap();
        Self {
            host,
            replica: Replica::default(),
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
    fn update(&mut self) -> Result<()> {
        self.updates.fetch_add(1, Ordering::SeqCst);
        self.host.step();
        self.replica.apply(10, 10, 10, self.host.snapshot(10)?)?;
        Ok(())
    }
    fn action(&mut self, action: Action) -> Result<()> {
        self.calls.lock().unwrap().push(action);
        match action {
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
            local: 10,
            host: true,
            can_start: self.lobby.is_some() && self.host.phase != Phase::Playing,
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
fn editor_routes_lobby_ui_flaps_and_overlay_free_invites_without_touching_edit_scene() {
    let source = scene();
    let mut editor = Editor::new(
        source.clone(),
        &std::env::temp_dir().join("steam-editor-scene.json"),
    )
    .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let dropped = Arc::new(AtomicUsize::new(0));
    let backend = FakeSteam::new(
        calls.clone(),
        Arc::new(AtomicUsize::new(0)),
        dropped.clone(),
    );
    let mut play = SceneDemo::new(&source).unwrap();
    play.attach_multiplayer(Multiplayer::with_backend(&source, Box::new(backend), None).unwrap())
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
    let mut editor = Editor::new(
        source.clone(),
        &std::env::temp_dir().join("steam-editor-worker.json"),
    )
    .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let updates = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let backend = FakeSteam::new(calls.clone(), updates.clone(), dropped.clone());
        let worker = Threaded::new(Box::new(backend)).unwrap();
        let net = Multiplayer::with_backend(&source, Box::new(worker), Some(777)).unwrap();
        let mut play = SceneDemo::new(&source).unwrap();
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
    let mut editor = Editor::new(
        source.clone(),
        &std::env::temp_dir().join("steam-editor-quit.json"),
    )
    .unwrap();
    let backend = FakeSteam::new(Default::default(), Default::default(), Default::default());
    let mut play = SceneDemo::new(&source).unwrap();
    play.attach_multiplayer(Multiplayer::with_backend(&source, Box::new(backend), None).unwrap())
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
    let mut editor = Editor::new(
        source.clone(),
        &std::env::temp_dir().join("steam-editor-disabled.json"),
    )
    .unwrap();
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
