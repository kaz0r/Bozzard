//! Player state outside the event loop: mouse look, the window title and keyboard commands.
use super::*;

/// Capture only while the game plays. Interactive UI keeps the default pointer free;
/// scenes can explicitly request mouse-look with Lock Cursor while playing.
/// Run, pause and game-over menus always release it, even with an explicit request.
pub(crate) fn cursor_capture_wanted(state: CursorCaptureState) -> bool {
    state.gameplay
        && state.running
        && state.focused
        && !state.paused
        && state.layer == Layer::ThreeD
        && state.requested.unwrap_or(!state.ui_wants_pointer)
}

pub(crate) fn print_frame_percentiles(label: &str, samples: &VecDeque<f64>) {
    if samples.is_empty() {
        return;
    }
    let mut sorted: Vec<_> = samples.iter().copied().collect();
    sorted.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        sorted[((sorted.len() as f64 * p).ceil() as usize)
            .saturating_sub(1)
            .min(sorted.len() - 1)]
    };
    println!(
        "{label} n={} median={:.3}ms p95={:.3}ms p99={:.3}ms",
        sorted.len(),
        percentile(0.5),
        percentile(0.95),
        percentile(0.99)
    );
}

impl Player {
    /// A player for `demo` before its window opens, with idle input and empty frame statistics.
    pub(crate) fn new(options: Options, demo: SceneDemo, assets: assets::Assets) -> Self {
        Self {
            options,
            view: None,
            demo,
            assets,
            audio: Default::default(),
            paused: false,
            menu_input: Default::default(),
            gameplay_controls: gameplay_input::GameplayControls::default(),
            look: gameplay_input::Look::Off,
            last_frame: Instant::now(),
            last_present: Instant::now(),
            frames: 0,
            cpu_frame_ms: VecDeque::new(),
            presentation_interval_ms: VecDeque::new(),
            fault_injected: false,
            error: None,
            command_error: None,
        }
    }
    pub(crate) fn simulation_elapsed(&self, now: Instant) -> Option<Duration> {
        (!self.paused
            && !self.demo.multiplayer_drives_simulation()
            && !self.options.verify_first_trail)
            .then(|| now.duration_since(self.last_frame))
    }
    /// The game owns the pointer only while it is actually playing: menus, dialogs,
    /// pause and other views keep a usable cursor. A grab is attempted once per
    /// transition, never per frame, so a refused platform is not polled.
    pub(crate) fn sync_mouse_look(&mut self) {
        let running = self
            .demo
            .game_session()
            .is_none_or(|session| session.phase == bozzard_scene::GamePhase::Playing);
        let requested = self
            .demo
            .app
            .world
            .resource::<bozzard_scene::CursorCapture>()
            .and_then(|capture| capture.requested);
        let playing = self.view.is_some()
            && !self.demo.steam_overlay_active()
            && cursor_capture_wanted(CursorCaptureState {
                paused: self.paused,
                focused: self.gameplay_controls.focused(),
                layer: self.options.layer,
                gameplay: self.demo.accepts_gameplay_input(),
                running,
                ui_wants_pointer: self.view.as_ref().is_none_or(|view| view.ui_wants_pointer),
                requested,
            });
        if self.look != gameplay_input::Look::Off {
            if playing {
                return;
            }
            self.set_look(gameplay_input::Look::Off);
            return;
        }
        if !playing {
            return;
        }
        let window = &self.view.as_ref().unwrap().window;
        let look = if window.set_cursor_grab(CursorGrabMode::Locked).is_ok() {
            gameplay_input::Look::Locked
        } else {
            // Confined keeps the pointer on the game view; a platform that refuses both
            // grabs still gets no-button look from ordinary pointer motion.
            if window.set_cursor_grab(CursorGrabMode::Confined).is_err() {
                let _ = window.set_cursor_grab(CursorGrabMode::None);
            }
            gameplay_input::Look::Cursor
        };
        self.set_look(look);
    }

    pub(crate) fn set_look(&mut self, look: gameplay_input::Look) {
        self.look = look;
        self.gameplay_controls.set_mouse_look(look);
        let Some(view) = &self.view else {
            return;
        };
        if look == gameplay_input::Look::Off {
            let _ = view.window.set_cursor_grab(CursorGrabMode::None);
            view.window.set_cursor_visible(true);
        } else {
            // A locked pointer is invisible by definition; a moving one stays findable.
            view.window
                .set_cursor_visible(look == gameplay_input::Look::Cursor);
        }
    }

    pub(crate) fn window_title(&self) -> String {
        if let Some(title) = self.demo.multiplayer_title() {
            return title;
        }
        let name = self.options.game_name.as_deref().unwrap_or("Bozzard");
        let status = if let Some(error) = &self.command_error {
            format!("ERROR: {error} | ")
        } else {
            String::new()
        };
        if let Some(session) = self.demo.game_session() {
            return format!("{name} | {status}{:?}", session.phase);
        }
        if let Some(state) = self.demo.gameplay() {
            format!(
                "{name} | {status}{} {}/{} | CP: {} | falls: {} | WASD move, Space jump, RMB orbit, physical R restart",
                if state.won {
                    "YOU WIN!"
                } else {
                    "Gold → goal"
                },
                state.collected.len(),
                state.total,
                state.checkpoint.as_deref().unwrap_or("start"),
                state.respawns
            )
        } else if self.options.game_name.is_some() {
            format!("{name} | {status}Playing | R: restart | Escape: quit")
        } else if self.demo.instance().has_gameplay_logic() {
            format!(
                "{name} | {status}Gameplay logic running | Scene keys: input | F5: save | F6: reload"
            )
        } else {
            let layer = if self.options.layer == Layer::TwoD {
                "2D"
            } else {
                "3D"
            };
            let state = if self.paused { "paused" } else { "playing" };
            format!(
                "{name} — {status}{layer} / {state} | 1/2: view | Space: pause | Arrows: pan | F5: save | R: reload"
            )
        }
    }

    /// The native handler and regression tests share movement/command dispatch.
    pub(crate) fn dispatch_keyboard(
        &mut self,
        physical: PhysicalKey,
        logical: &Key,
        state: ElementState,
        repeat: bool,
        synthetic: bool,
    ) -> Result<()> {
        if self.demo.steam_overlay_active() {
            self.gameplay_controls.reset();
            self.menu_input = Default::default();
            return Ok(());
        }
        self.gameplay_controls.set_scene_keyboard(
            self.demo.gameplay().is_none() && self.demo.instance().has_gameplay_logic(),
        );
        if self.demo.multiplayer_chatting() {
            self.gameplay_controls.reset();
            if state == ElementState::Pressed
                && !synthetic
                && let PhysicalKey::Code(code) = physical
            {
                let name = format!("{code:?}");
                if !repeat || code == KeyCode::Backspace {
                    self.demo
                        .multiplayer_key(bozzard_scene::keys::canonical(&name).unwrap_or(&name));
                }
            }
            return Ok(());
        }
        if self.demo.multiplayer_active() {
            if !repeat
                && !synthetic
                && state == ElementState::Pressed
                && let PhysicalKey::Code(code) = physical
                && let Some(key) = bozzard_scene::keys::canonical(&format!("{code:?}"))
                && self.demo.multiplayer_key(key)
            {
                self.gameplay_controls.reset();
                return Ok(());
            }
            if self.demo.multiplayer_drives_simulation() {
                self.game_key(physical, state, repeat, synthetic)?;
                return Ok(());
            }
        }
        if self.game_key(physical, state, repeat, synthetic)? {
            return Ok(());
        }
        if self.demo.accepts_gameplay_input() {
            if !synthetic && let PhysicalKey::Code(code) = physical {
                let input =
                    self.gameplay_controls
                        .key(code, state == ElementState::Pressed, repeat);
                if self.options.layer == Layer::ThreeD || self.demo.instance().has_gameplay_logic()
                {
                    self.demo.set_gameplay_input(input);
                } else {
                    self.demo.clear_gameplay_input();
                }
            }
            // A script or Blueprint scene without a Player Controller owns its entire key
            // layout. The viewer's view/reload shortcuts must not run on those same presses.
            if self.demo.gameplay().is_none() && self.demo.instance().has_gameplay_logic() {
                if state == ElementState::Pressed
                    && matches!(logical, Key::Named(NamedKey::F5 | NamedKey::F6))
                {
                    return self.handle_key(logical, repeat);
                }
                return Ok(());
            }
            // Gameplay positions must never also invoke logical commands on another layout.
            if matches!(
                physical,
                PhysicalKey::Code(
                    KeyCode::KeyA | KeyCode::KeyD | KeyCode::KeyW | KeyCode::KeyS | KeyCode::Space
                )
            ) {
                return Ok(());
            }
            if physical == PhysicalKey::Code(KeyCode::KeyR) {
                return if state == ElementState::Pressed && !synthetic {
                    self.handle_key(&Key::Character("r".into()), repeat)
                } else {
                    Ok(())
                };
            }
            if matches!(logical, Key::Character(value) if value.eq_ignore_ascii_case("r")) {
                return Ok(());
            }
        }
        if state == ElementState::Pressed && (!synthetic || !self.demo.accepts_gameplay_input()) {
            self.handle_key(logical, repeat)?;
        }
        Ok(())
    }

    pub(crate) fn handle_key(&mut self, key: &Key, repeat: bool) -> Result<()> {
        let result = self.execute_key(key, repeat);
        match &result {
            Ok(true) => self.command_error = None,
            Err(error) => self.command_error = Some(format!("{error:#}")),
            Ok(false) => {}
        }
        if let Some(view) = &self.view {
            view.window.set_title(&self.window_title());
        }
        result.map(|_| ())
    }

    pub(crate) fn execute_key(&mut self, key: &Key, repeat: bool) -> Result<bool> {
        if self.options.game_name.is_some()
            && (matches!(key, Key::Named(NamedKey::F5))
                || matches!(key, Key::Character(value) if value == "1" || value == "2"))
        {
            return Ok(false);
        }
        match key {
            Key::Named(NamedKey::Space) if !repeat && !self.demo.accepts_gameplay_input() => {
                self.paused = !self.paused
            }
            Key::Character(value) if !repeat && (value == "1" || value == "2") => {
                let layer = if value == "1" {
                    Layer::TwoD
                } else {
                    Layer::ThreeD
                };
                ensure!(
                    self.demo.instance().has_view(layer),
                    "scene has no {layer:?} view"
                );
                self.options.layer = layer;
                self.gameplay_controls.reset();
                self.demo.clear_gameplay_input();
            }
            key if !repeat
                && (matches!(key, Key::Named(NamedKey::F6))
                    || matches!(key, Key::Character(value) if value.eq_ignore_ascii_case("r"))) =>
            {
                let document = load_document(self.options.scene.as_deref())?;
                let mut next =
                    SceneDemo::new_with_prefabs(&document, self.options.scene.as_deref())?;
                next.set_threaded_simulation(self.options.threaded_simulation)?;
                let mut assets = assets::Assets::load(
                    next.instance().document(),
                    self.options.scene.as_deref(),
                )?;
                bozzard_project::streaming::install(
                    &mut next.app.world,
                    self.options
                        .scene
                        .as_deref()
                        .unwrap_or(Path::new("scene.json")),
                    assets.store(),
                )?;
                ensure!(
                    next.instance().has_view(self.options.layer),
                    "reloaded scene is missing the active view"
                );
                if let Some(view) = &mut self.view {
                    let mut renderer = SceneRenderer::new(&view.gpu, view.config.format);
                    renderer.set_occlusion_enabled(self.options.occlusion_enabled);
                    let render = extract(
                        &next,
                        assets.store(),
                        self.options.layer,
                        view.config.width as f32 / view.config.height as f32,
                    )?;
                    assets.upload_required(
                        &view.gpu,
                        &mut renderer,
                        &render,
                        self.options.gpu_memory_mib * 1024 * 1024,
                    )?;
                    view.renderer = renderer;
                }
                self.assets = assets;
                self.audio.stop();
                self.demo = next;
                self.gameplay_controls.reset();
                if self.demo.accepts_gameplay_input() {
                    self.paused = false;
                }
                self.last_frame = Instant::now();
                println!("scene_reloaded");
            }
            Key::Named(NamedKey::F5) if !repeat => {
                save_document_from(
                    &self.demo.instance().capture(&self.demo.app.world)?,
                    &self.options.save_path,
                    self.options.scene.as_deref(),
                )?;
                println!("scene_saved path={}", self.options.save_path.display());
            }
            Key::Named(
                direction @ (NamedKey::ArrowLeft
                | NamedKey::ArrowRight
                | NamedKey::ArrowUp
                | NamedKey::ArrowDown),
            ) if self.demo.gameplay().is_none() => {
                let entity = self.demo.instance().camera_entity(self.options.layer)?;
                let mut camera = self
                    .demo
                    .app
                    .world
                    .get_mut::<Transform>(entity)
                    .context("missing camera transform")?;
                match direction {
                    NamedKey::ArrowLeft => camera.translation[0] -= 0.25,
                    NamedKey::ArrowRight => camera.translation[0] += 0.25,
                    NamedKey::ArrowUp => camera.translation[1] += 0.25,
                    _ => camera.translation[1] -= 0.25,
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    pub(crate) fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}
