use anyhow::{Context, Result, bail, ensure};
mod assets;
mod flap_woods;
mod game_flow;
mod gameplay_input;
mod presentation;
mod project;
mod smoke;
use bozzard_demo::{SceneDemo, load_document, save_document_from};
use bozzard_render::{Backend, Gpu, SceneRenderer, instance, wgpu};
use bozzard_scene::{Layer, Scene, Transform};
use presentation::extract;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, KeyCode, NamedKey, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

struct Options {
    project: Option<PathBuf>,
    game_name: Option<String>,
    export_project: Option<PathBuf>,
    export_dir: Option<PathBuf>,
    verify_first_trail: bool,
    verify_flap_woods: bool,
    backend: Backend,
    software: bool,
    hardware: bool,
    smoke: bool,
    benchmark_frames: Option<u32>,
    frames: Option<u32>,
    output: PathBuf,
    scene: Option<PathBuf>,
    write_scene: Option<PathBuf>,
    save_path: PathBuf,
    layer: Layer,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            project: None,
            game_name: None,
            export_project: None,
            export_dir: None,
            verify_first_trail: false,
            verify_flap_woods: false,
            backend: Backend::native(),
            software: false,
            hardware: false,
            smoke: false,
            benchmark_frames: None,
            frames: None,
            output: "work/gpu-smoke".into(),
            scene: None,
            write_scene: None,
            save_path: "work/saved-scene.json".into(),
            layer: Layer::ThreeD,
        }
    }
}

fn options() -> Result<Option<Options>> {
    let mut result = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--project" => {
                result.project = Some(args.next().context("--project needs a manifest")?.into())
            }
            "--export-project" => {
                result.export_project = Some(
                    args.next()
                        .context("--export-project needs a manifest")?
                        .into(),
                )
            }
            "--export-dir" => {
                result.export_dir = Some(
                    args.next()
                        .context("--export-dir needs a new folder")?
                        .into(),
                )
            }
            "--verify-first-trail" => result.verify_first_trail = true,
            "--verify-flap-woods" => result.verify_flap_woods = true,
            "--backend" => {
                result.backend = args.next().context("--backend needs a value")?.parse()?
            }
            "--software" => result.software = true,
            "--hardware" => result.hardware = true,
            "--smoke" => result.smoke = true,
            "--benchmark-frames" => {
                let frames = args
                    .next()
                    .context("--benchmark-frames needs a count")?
                    .parse()?;
                ensure!(
                    (1..=1000).contains(&frames),
                    "benchmark frames must be within 1..1000"
                );
                result.benchmark_frames = Some(frames);
            }
            "--scene" => result.scene = Some(args.next().context("--scene needs a file")?.into()),
            "--write-scene" => {
                result.write_scene = Some(args.next().context("--write-scene needs a file")?.into())
            }
            "--save-path" => {
                result.save_path = args.next().context("--save-path needs a file")?.into()
            }
            "--view" => {
                result.layer = match args.next().as_deref() {
                    Some("2d") => Layer::TwoD,
                    Some("3d") => Layer::ThreeD,
                    _ => bail!("--view expects 2d or 3d"),
                }
            }
            "--frames" => {
                let count = args.next().context("--frames needs a value")?.parse()?;
                ensure!(count > 0, "--frames must be positive");
                result.frames = Some(count);
            }
            "--output" => result.output = args.next().context("--output needs a directory")?.into(),
            "--help" => {
                println!(
                    "--project FILE starts a user game. Exported games find their project beside the executable.\n--export-project FILE --export-dir NEW_FOLDER exports a native game using this player.\n--verify-flap-woods checks start, score, pause, game over, retry and quit without graphics.\n--verify-first-trail checks the reference route without graphics; add --frames 340 to present the route."
                );
                println!(
                    "bozzard-player [--backend metal|vulkan|dx12] [--software|--hardware] [--frames N]\nbozzard-player --smoke [--backend ...] [--software|--hardware] [--output DIRECTORY]\n--benchmark-frames N compares reference/culling/cached draws during --smoke --scene.\n--scene FILE loads JSON; --write-scene FILE saves it and exits without a GPU.\n--view 2d|3d chooses the starting view; --save-path FILE sets the F5 destination.\n1/2: 2D/3D. Space: pause. Arrows: pan camera. F5: save. R: reload source. Escape: close.\nPlayer Controller scenes: WASD move, Space jump, right-drag orbit. Progress/win in title; physical R restarts."
                );
                return Ok(None);
            }
            _ => bail!("unknown argument: {arg}"),
        }
    }
    ensure!(
        !(result.software && result.hardware),
        "--software and --hardware are mutually exclusive"
    );
    ensure!(
        !(result.smoke && result.frames.is_some()),
        "--frames is for windowed runs; --smoke runs the graphics verification suite"
    );
    ensure!(
        result.write_scene.is_none() || (!result.smoke && result.frames.is_none()),
        "--write-scene is a standalone command; it cannot be combined with --smoke or --frames"
    );
    ensure!(
        result.benchmark_frames.is_none() || (result.smoke && result.scene.is_some()),
        "--benchmark-frames requires --smoke --scene FILE"
    );
    project::resolve(&mut result)?;
    Ok(Some(result))
}

struct View {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    gpu: Gpu,
    config: wgpu::SurfaceConfiguration,
    renderer: SceneRenderer,
    drawable: bool,
    surface_status: &'static str,
}

impl View {
    fn new(event_loop: &ActiveEventLoop, options: &Options) -> Result<Self> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(
                        "Bozzard Scene Lab — 1: 2D | 2: 3D | Space: pause | F5: save | R: reload",
                    )
                    .with_inner_size(LogicalSize::new(1024.0, 640.0)),
            )?,
        );
        let instance = instance(options.backend);
        let surface = instance.create_surface(window.clone())?;
        let gpu = pollster::block_on(Gpu::request(&instance, Some(&surface), options.software))?;
        if options.hardware {
            gpu.require_hardware()?;
        }
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .context("surface is unsupported by selected adapter")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        let renderer = SceneRenderer::new(&gpu, config.format);
        surface.configure(&gpu.device, &config);
        if options.frames.is_some() {
            window.focus_window();
        }
        Ok(Self {
            window,
            surface,
            gpu,
            config,
            renderer,
            drawable: size.width > 0 && size.height > 0,
            surface_status: "awaiting first redraw",
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.drawable = width > 0 && height > 0;
        if self.drawable {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.gpu.device, &self.config);
        }
    }

    fn draw(
        &mut self,
        demo: &SceneDemo,
        assets: &mut assets::Assets,
        layer: Layer,
    ) -> Result<bool> {
        if !self.drawable {
            self.surface_status = "window has zero size";
            return Ok(false);
        }
        self.renderer
            .set_hud_scale(self.window.scale_factor() as f32);
        assets.poll(&self.gpu, &mut self.renderer)?;
        let (frame, reconfigure) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface_status = "surface outdated";
                self.surface.configure(&self.gpu.device, &self.config);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.surface_status = "surface acquisition timed out";
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.surface_status = "window occluded; an active desktop is required";
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Lost => bail!("graphics surface lost; restart the player"),
            wgpu::CurrentSurfaceTexture::Validation => bail!("graphics surface validation failed"),
        };
        let mut scene = extract(
            demo,
            assets.store(),
            layer,
            self.config.width as f32 / self.config.height as f32,
        )?;
        if let (Some(settings), Some(session)) =
            (&demo.instance().document().game_flow, demo.game_session())
        {
            let scale = self.window.scale_factor() as f32;
            scene.items.extend(bozzard_render_assets::game_menu(
                settings,
                session,
                [
                    self.config.width as f32 / scale,
                    self.config.height as f32 / scale,
                ],
            ));
        }
        if !assets.current() {
            scene.gi = None;
        }
        self.renderer.draw(
            &self.gpu,
            &frame.texture.create_view(&Default::default()),
            [self.config.width, self.config.height],
            &scene,
        )?;
        self.window.pre_present_notify();
        self.gpu.queue.present(frame);
        self.surface_status = "presented";
        if reconfigure {
            self.surface.configure(&self.gpu.device, &self.config);
        }
        Ok(true)
    }
}

/// Everything the pointer-capture decision depends on, so it can be checked without a window.
#[derive(Clone, Copy)]
struct CursorCaptureState {
    paused: bool,
    focused: bool,
    layer: Layer,
    /// The scene uses gameplay input at all (a player controller or any enabled graph).
    gameplay: bool,
    /// The simulation is actually running: Game Flow menus, pause and game over are not.
    running: bool,
    /// Lock/Unlock Cursor request; None keeps the app policy (capture while playing).
    requested: Option<bool>,
}

/// Capture only while the game plays. A scene may opt out with Unlock Cursor, but nothing may
/// hold the pointer on a menu: the run menu, pause overlay and win screen are clicked with it.
fn cursor_capture_wanted(state: CursorCaptureState) -> bool {
    state.gameplay
        && state.running
        && state.focused
        && !state.paused
        && state.layer == Layer::ThreeD
        && state.requested.unwrap_or(true)
}

struct Player {
    options: Options,
    view: Option<View>,
    demo: SceneDemo,
    assets: assets::Assets,
    paused: bool,
    menu_input: game_flow::MenuInput,
    gameplay_controls: gameplay_input::GameplayControls,
    look: gameplay_input::Look,
    last_frame: Instant,
    last_present: Instant,
    frames: u32,
    error: Option<anyhow::Error>,
    command_error: Option<String>,
}

impl Player {
    /// The game owns the pointer only while it is actually playing: menus, dialogs,
    /// pause and other views keep a usable cursor. A grab is attempted once per
    /// transition, never per frame, so a refused platform is not polled.
    fn sync_mouse_look(&mut self) {
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
            && cursor_capture_wanted(CursorCaptureState {
                paused: self.paused,
                focused: self.gameplay_controls.focused(),
                layer: self.options.layer,
                gameplay: self.demo.accepts_gameplay_input(),
                running,
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

    fn set_look(&mut self, look: gameplay_input::Look) {
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

    fn window_title(&self) -> String {
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
        } else if self.demo.instance().has_blueprints() {
            format!(
                "{name} | {status}Blueprints running | WASD / Space: input | R: restart | F5: save"
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
    fn dispatch_keyboard(
        &mut self,
        physical: PhysicalKey,
        logical: &Key,
        state: ElementState,
        repeat: bool,
        synthetic: bool,
    ) -> Result<()> {
        if self.game_key(physical, state, repeat, synthetic)? {
            return Ok(());
        }
        if self.demo.accepts_gameplay_input() {
            if !synthetic && let PhysicalKey::Code(code) = physical {
                let input =
                    self.gameplay_controls
                        .key(code, state == ElementState::Pressed, repeat);
                if self.options.layer == Layer::ThreeD || self.demo.instance().has_blueprints() {
                    self.demo.set_gameplay_input(input);
                } else {
                    self.demo.clear_gameplay_input();
                }
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

    fn handle_key(&mut self, key: &Key, repeat: bool) -> Result<()> {
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

    fn execute_key(&mut self, key: &Key, repeat: bool) -> Result<bool> {
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
            Key::Character(value) if !repeat && value.eq_ignore_ascii_case("r") => {
                let document = load_document(self.options.scene.as_deref())?;
                let next = SceneDemo::new_with_prefabs(&document, self.options.scene.as_deref())?;
                let mut assets = assets::Assets::load(
                    next.instance().document(),
                    self.options.scene.as_deref(),
                )?;
                ensure!(
                    next.instance().has_view(self.options.layer),
                    "reloaded scene is missing the active view"
                );
                if let Some(view) = &mut self.view {
                    let mut renderer = SceneRenderer::new(&view.gpu, view.config.format);
                    assets.upload(&view.gpu, &mut renderer)?;
                    view.renderer = renderer;
                }
                self.assets = assets;
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
                let camera = self
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

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }
}

impl ApplicationHandler for Player {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_some() {
            return;
        }
        match View::new(event_loop, &self.options) {
            Ok(mut view) => {
                if let Err(error) = self.assets.upload(&view.gpu, &mut view.renderer) {
                    self.fail(event_loop, error);
                    return;
                }
                self.gameplay_controls
                    .set_scale_factor(view.window.scale_factor());
                self.view = Some(view);
                self.last_frame = Instant::now();
                self.last_present = Instant::now();
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.view = None;
        self.look = gameplay_input::Look::Off;
        self.gameplay_controls
            .set_mouse_look(gameplay_input::Look::Off);
        self.gameplay_controls.reset();
        self.demo.clear_gameplay_input();
    }

    /// Locked look reads raw device motion: the cursor does not move, so deltas never arrive
    /// through CursorMoved.
    fn device_event(&mut self, event_loop: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if event_loop.exiting() || self.view.is_none() {
            return;
        }
        let DeviceEvent::MouseMotion { delta } = event else {
            return;
        };
        if let Some(input) = self.gameplay_controls.motion([delta.0, delta.1]) {
            self.demo.set_gameplay_input(input);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if event_loop.exiting() {
            return;
        }
        if self.view.as_ref().is_none_or(|v| id != v.window.id()) {
            return;
        }
        if let Err(error) = self.game_pointer_event(&event) {
            self.fail(event_loop, error);
            return;
        }
        if self.demo.accepts_gameplay_input() {
            if let Some(input) = self.gameplay_controls.event(&event)
                && (self.options.layer == Layer::ThreeD || self.demo.instance().has_blueprints())
            {
                self.demo.set_gameplay_input(input);
            } else {
                self.demo.clear_gameplay_input();
            }
        }
        if let WindowEvent::KeyboardInput {
            event,
            is_synthetic,
            ..
        } = &event
            && let Err(error) = self.dispatch_keyboard(
                event.physical_key,
                &event.logical_key,
                event.state,
                event.repeat,
                *is_synthetic,
            )
        {
            eprintln!("scene command failed: {error:#}");
        }
        if self
            .demo
            .game_session()
            .is_some_and(|s| s.phase == bozzard_scene::GamePhase::Quit)
        {
            event_loop.exit();
            return;
        }
        let now = Instant::now();
        if matches!(event, WindowEvent::RedrawRequested) {
            if self.options.verify_first_trail {
                if let Err(error) = project::route_tick(self, self.demo.app.ticks()) {
                    self.fail(event_loop, error);
                    return;
                }
            } else if !self.paused {
                self.demo.app.advance(now.duration_since(self.last_frame));
            }
            if let Err(error) = self.demo.check_simulation() {
                self.fail(event_loop, error);
                return;
            }
            self.last_frame = now;
        }
        self.sync_mouse_look();
        let title = self.window_title();
        let Some(view) = self.view.as_mut() else {
            return;
        };
        view.window.set_title(&title);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. }
                if self.demo.game_session().is_none()
                    && event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Escape) =>
            {
                event_loop.exit()
            }
            WindowEvent::Resized(size) => view.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                match view.draw(&self.demo, &mut self.assets, self.options.layer) {
                    Ok(true) => {
                        self.frames = self.frames.saturating_add(1);
                        self.last_present = now;
                        if self
                            .options
                            .frames
                            .is_some_and(|limit| self.frames >= limit)
                        {
                            if let Err(error) = view.gpu.wait() {
                                self.fail(event_loop, error);
                                return;
                            }
                            println!("window_ok frames={}", self.frames);
                            event_loop.exit();
                        }
                    }
                    Ok(false) => {}
                    Err(error) => self.fail(event_loop, error),
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if event_loop.exiting() {
            return;
        }
        if self.options.frames.is_some() && self.last_present.elapsed() > Duration::from_secs(30) {
            self.fail(
                event_loop,
                anyhow::anyhow!(
                    "no successful presentation within 30 seconds: {}",
                    self.view
                        .as_ref()
                        .map(|v| v.surface_status)
                        .unwrap_or("no active window")
                ),
            );
            return;
        }
        if let Some(view) = &self.view {
            view.window.request_redraw();
        }
        // Avoid spinning when a window is minimized or a surface is unavailable.
        event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}

fn main() -> Result<()> {
    let Some(options) = options()? else {
        return Ok(());
    };
    if let Some(manifest) = &options.export_project {
        let (project, source) = bozzard_project::Project::load(manifest)?;
        let scene = load_document(Some(&source))?;
        let destination = bozzard_project::prepare_export(
            &project,
            &scene,
            &source,
            &std::env::current_exe()?,
            options.export_dir.as_ref().unwrap(),
            &Default::default(),
        )?
        .commit()?;
        println!("export_ok path={}", destination.display());
        return Ok(());
    }
    if options.smoke {
        return smoke::run(&options);
    }
    let document = load_document(options.scene.as_deref())?;
    if let Some(path) = &options.write_scene {
        save_document_from(&document, path, options.scene.as_deref())?;
        println!("scene_saved path={}", path.display());
        return Ok(());
    }
    let demo = SceneDemo::new_with_prefabs(&document, options.scene.as_deref())?;
    ensure!(
        demo.instance().has_view(options.layer),
        "scene has no requested view; use --view 2d or --view 3d"
    );
    let assets = assets::Assets::load(demo.instance().document(), options.scene.as_deref())?;
    let mut player = Player {
        assets,
        options,
        view: None,
        demo,
        paused: false,
        menu_input: Default::default(),
        gameplay_controls: gameplay_input::GameplayControls::default(),
        look: gameplay_input::Look::Off,
        last_frame: Instant::now(),
        last_present: Instant::now(),
        frames: 0,
        error: None,
        command_error: None,
    };
    if player.options.verify_flap_woods {
        return flap_woods::verify(&mut player);
    }
    if player.options.verify_first_trail && player.options.frames.is_none() {
        for tick in 0..340 {
            project::route_tick(&mut player, tick)?;
        }
    } else {
        EventLoop::new()?.run_app(&mut player)?;
    }
    if player.options.verify_first_trail {
        project::verify_route(&mut player)?;
    }
    if let Some(error) = player.error {
        return Err(error);
    }
    if let Some(limit) = player.options.frames {
        ensure!(
            player.frames >= limit,
            "window closed before the requested frames were presented"
        );
    }
    Ok(())
}

#[cfg(test)]
mod controls_tests {
    use super::*;
    fn authored_player() -> Player {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/first-trail.json");
        let document = load_document(Some(&path)).unwrap();
        Player {
            assets: assets::Assets::load(&document, Some(&path)).unwrap(),
            options: Options {
                scene: Some(path),
                ..Default::default()
            },
            view: None,
            demo: SceneDemo::new(&document).unwrap(),
            paused: false,
            menu_input: Default::default(),
            gameplay_controls: gameplay_input::GameplayControls::default(),
            look: gameplay_input::Look::Off,
            last_frame: Instant::now(),
            last_present: Instant::now(),
            frames: 0,
            error: None,
            command_error: None,
        }
    }

    #[test]
    fn the_pointer_is_never_captured_outside_a_running_game() {
        let base = CursorCaptureState {
            paused: false,
            focused: true,
            layer: Layer::ThreeD,
            gameplay: true,
            running: true,
            requested: None,
        };
        assert!(
            cursor_capture_wanted(base),
            "a running game owns the pointer"
        );
        assert!(
            !cursor_capture_wanted(CursorCaptureState {
                running: false,
                // Scenes with graphs report gameplay input in the run menu too.
                requested: Some(true),
                ..base
            }),
            "the run menu, pause overlay and win screen are clicked with a visible pointer"
        );
        assert!(!cursor_capture_wanted(CursorCaptureState {
            requested: Some(false),
            ..base
        }));
        assert!(!cursor_capture_wanted(CursorCaptureState {
            focused: false,
            ..base
        }));
        assert!(!cursor_capture_wanted(CursorCaptureState {
            paused: true,
            ..base
        }));
        assert!(!cursor_capture_wanted(CursorCaptureState {
            layer: Layer::TwoD,
            ..base
        }));
        assert!(!cursor_capture_wanted(CursorCaptureState {
            gameplay: false,
            ..base
        }));
    }

    #[test]
    fn game_menu_keys_do_not_repeat_or_leak_into_gameplay() {
        use bozzard_scene::{GamePhase as P, TextRendering};
        let mut player = authored_player();
        let scene = Scene::from_json(include_str!(
            "../../../examples/demo/scenes/game-flow-lab.json"
        ))
        .unwrap();
        player.demo = SceneDemo::new(&scene).unwrap();
        player.gameplay_controls.event(&WindowEvent::Focused(true));
        let key = |player: &mut Player, code, repeat, synthetic| {
            player
                .dispatch_keyboard(
                    PhysicalKey::Code(code),
                    &Key::Character("ignored".into()),
                    ElementState::Pressed,
                    repeat,
                    synthetic,
                )
                .unwrap();
        };
        key(&mut player, KeyCode::Enter, true, false);
        key(&mut player, KeyCode::Enter, false, true);
        assert_eq!(player.demo.game_session().unwrap().phase, P::Ready);
        key(&mut player, KeyCode::Space, false, false);
        key(&mut player, KeyCode::Enter, false, false);
        player.demo.app.step();
        let counter = player.demo.instance().entity("counter").unwrap();
        assert_eq!(
            player
                .demo
                .app
                .world
                .get::<TextRendering>(counter)
                .unwrap()
                .text,
            "Taps: 0"
        );
        key(&mut player, KeyCode::Escape, false, false);
        key(&mut player, KeyCode::Escape, true, false);
        assert_eq!(player.demo.game_session().unwrap().phase, P::Paused);
        key(&mut player, KeyCode::Space, false, false);
        key(&mut player, KeyCode::Enter, false, false);
        player.demo.app.step();
        assert_eq!(
            player
                .demo
                .app
                .world
                .get::<TextRendering>(counter)
                .unwrap()
                .text,
            "Taps: 0"
        );
        for _ in 0..3 {
            key(&mut player, KeyCode::Space, false, false);
            player.demo.app.step();
        }
        player.demo.check_simulation().unwrap();
        assert_eq!(player.demo.game_session().unwrap().phase, P::GameOver);
        key(&mut player, KeyCode::Enter, false, false);
        assert_eq!(player.demo.game_session().unwrap().phase, P::Playing);
        player
            .game_pointer_event(&WindowEvent::Focused(false))
            .unwrap();
        assert_eq!(player.demo.game_session().unwrap().phase, P::Paused);
        key(&mut player, KeyCode::KeyQ, false, false);
        assert_eq!(player.demo.game_session().unwrap().phase, P::Quit);
    }
    #[test]
    fn a_scene_assigned_key_reaches_gameplay_instead_of_a_menu_command() {
        use bozzard_scene::GamePhase as P;
        let mut player = authored_player();
        let scene = Scene::from_json(include_str!(
            "../../../examples/demo/scenes/game-flow-lab.json"
        ))
        .unwrap();
        player.demo = SceneDemo::new(&scene).unwrap();
        player.gameplay_controls.event(&WindowEvent::Focused(true));
        // Start the run, then press a key only a scene would use.
        player
            .dispatch_keyboard(
                PhysicalKey::Code(KeyCode::Enter),
                &Key::Named(NamedKey::Enter),
                ElementState::Pressed,
                false,
                false,
            )
            .unwrap();
        assert_eq!(player.demo.game_session().unwrap().phase, P::Playing);
        let before = player
            .demo
            .app
            .world
            .resource::<bozzard_scene::GameplayInput>()
            .copied();
        player
            .dispatch_keyboard(
                PhysicalKey::Code(KeyCode::KeyF),
                &Key::Character("f".into()),
                ElementState::Pressed,
                false,
                false,
            )
            .unwrap();
        let after = player
            .demo
            .app
            .world
            .resource::<bozzard_scene::GameplayInput>()
            .copied()
            .unwrap();
        assert!(
            after.keys & bozzard_scene::keys::bit("F") != 0,
            "an assigned key must reach the scene, not the app's command path"
        );
        assert!(before.unwrap_or_default().keys & bozzard_scene::keys::bit("F") == 0);
        // Released again, the key leaves gameplay alone.
        player
            .dispatch_keyboard(
                PhysicalKey::Code(KeyCode::KeyF),
                &Key::Character("f".into()),
                ElementState::Released,
                false,
                false,
            )
            .unwrap();
        assert_eq!(
            player
                .demo
                .app
                .world
                .resource::<bozzard_scene::GameplayInput>()
                .copied()
                .unwrap()
                .keys
                & bozzard_scene::keys::bit("F"),
            0
        );
    }

    #[test]
    fn blueprint_input_without_player_controller_toggles_rendered_mesh() {
        let mut player = authored_player();
        let scene = bozzard_scene::Scene::from_json(include_str!(
            "../../../examples/demo/scenes/blueprint-lab.json"
        ))
        .unwrap();
        player.demo = SceneDemo::new(&scene).unwrap();
        player.gameplay_controls.event(&WindowEvent::Focused(true));
        let visible = |p: &Player| {
            p.demo
                .instance()
                .view(&p.demo.app.world, Layer::ThreeD, 1.)
                .unwrap()
                .objects
                .len()
        };
        let count = visible(&player);
        for (pressed, expected) in [(true, count - 1), (false, count - 1), (true, count)] {
            player
                .dispatch_keyboard(
                    PhysicalKey::Code(KeyCode::Space),
                    &Key::Named(NamedKey::Space),
                    if pressed {
                        ElementState::Pressed
                    } else {
                        ElementState::Released
                    },
                    false,
                    false,
                )
                .unwrap();
            player.demo.app.step();
            player.demo.check_simulation().unwrap();
            assert!(!player.paused);
            assert_eq!(visible(&player), expected);
        }
    }

    #[test]
    fn combined_dispatch_reserves_gameplay_positions_and_restarts_physically() {
        let mut player = authored_player();
        player.gameplay_controls.event(&WindowEvent::Focused(true));
        player.demo.app.step();
        player.command_error = Some("retained status".into());
        for code in [
            KeyCode::KeyS,
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyD,
            KeyCode::Space,
            KeyCode::KeyP,
        ] {
            player
                .dispatch_keyboard(
                    PhysicalKey::Code(code),
                    &Key::Character("r".into()),
                    ElementState::Pressed,
                    false,
                    false,
                )
                .unwrap();
            assert_eq!(player.demo.app.ticks(), 1, "logical R must not reload");
            assert_eq!(player.command_error.as_deref(), Some("retained status"));
            let input = player
                .demo
                .app
                .world
                .resource::<bozzard_scene::GameplayInput>()
                .unwrap();
            if code == KeyCode::KeyS {
                assert_eq!(input.movement, [0.0, -1.0], "Colemak backward still moves");
            }
            if code == KeyCode::Space {
                assert!(input.jump);
                assert!(!player.paused);
            }
            player
                .dispatch_keyboard(
                    PhysicalKey::Code(code),
                    &Key::Character("r".into()),
                    ElementState::Released,
                    false,
                    false,
                )
                .unwrap();
        }
        // Reservation applies to all logical commands, not just restart.
        player.options.save_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for code in [
            KeyCode::KeyS,
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyD,
            KeyCode::Space,
        ] {
            for logical in [Key::Character("1".into()), Key::Named(NamedKey::F5)] {
                player
                    .dispatch_keyboard(
                        PhysicalKey::Code(code),
                        &logical,
                        ElementState::Pressed,
                        false,
                        false,
                    )
                    .unwrap();
                assert_eq!(player.options.layer, Layer::ThreeD);
                assert_eq!(player.command_error.as_deref(), Some("retained status"));
            }
        }
        for (state, repeat, synthetic) in [
            (ElementState::Pressed, true, false),
            (ElementState::Released, false, false),
            (ElementState::Pressed, false, true),
        ] {
            player
                .dispatch_keyboard(
                    PhysicalKey::Code(KeyCode::KeyR),
                    &Key::Character("p".into()),
                    state,
                    repeat,
                    synthetic,
                )
                .unwrap();
            assert_eq!(player.demo.app.ticks(), 1);
        }
        player
            .dispatch_keyboard(
                PhysicalKey::Code(KeyCode::KeyR),
                &Key::Character("p".into()),
                ElementState::Pressed,
                false,
                false,
            )
            .unwrap();
        assert_eq!(player.demo.app.ticks(), 0);
        assert!(player.command_error.is_none());
        assert_eq!(player.gameplay_controls.current().movement, [0.0; 2]);

        // The same physical S/logical R still reloads legacy non-controller scenes.
        player.options.scene = None;
        player.demo = SceneDemo::new(&bozzard_demo::scene_document().unwrap()).unwrap();
        player.demo.app.step();
        player
            .dispatch_keyboard(
                PhysicalKey::Code(KeyCode::KeyS),
                &Key::Character("r".into()),
                ElementState::Pressed,
                false,
                false,
            )
            .unwrap();
        assert_eq!(player.demo.app.ticks(), 0);
        player
            .dispatch_keyboard(
                PhysicalKey::Code(KeyCode::Space),
                &Key::Named(NamedKey::Space),
                ElementState::Pressed,
                false,
                false,
            )
            .unwrap();
        assert!(player.paused);
    }

    #[test]
    fn command_errors_survive_gameplay_titles_until_a_successful_command() {
        let mut player = authored_player();
        // Failed commands survive simulation/redraw titles and ordinary gameplay keys.
        let source = player.options.scene.clone();
        player.options.scene = Some(PathBuf::from("__bozzard_missing_scene__/missing.json"));
        assert!(
            player
                .handle_key(&Key::Character("r".into()), false)
                .is_err()
        );
        let failure = player.command_error.clone().unwrap();
        for _ in 0..3 {
            player.demo.app.step();
            player
                .handle_key(&Key::Character("w".into()), false)
                .unwrap();
            player
                .handle_key(&Key::Character("r".into()), true)
                .unwrap();
            assert!(player.window_title().contains(&format!("ERROR: {failure}")));
        }
        player.options.scene = source;
        player
            .handle_key(&Key::Character("r".into()), false)
            .unwrap();
        assert!(player.command_error.is_none());
        assert!(!player.window_title().contains("ERROR:"));
        // Saving to a directory reliably fails without depending on host permissions.
        player.options.save_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert!(player.handle_key(&Key::Named(NamedKey::F5), false).is_err());
        player.demo.app.step();
        assert!(player.window_title().contains("ERROR:"));
        player
            .handle_key(&Key::Character("r".into()), false)
            .unwrap();
        assert!(!player.window_title().contains("ERROR:"));
    }

    #[test]
    fn authored_player_space_does_not_pause_and_restart_clears_win_and_input() {
        let mut player = authored_player();
        let document = load_document(player.options.scene.as_deref()).unwrap();
        player
            .handle_key(&Key::Named(NamedKey::Space), false)
            .unwrap();
        assert!(!player.paused);
        let camera = player.demo.instance().camera_entity(Layer::ThreeD).unwrap();
        let pose = *player.demo.app.world.get::<Transform>(camera).unwrap();
        player
            .handle_key(&Key::Named(NamedKey::ArrowLeft), false)
            .unwrap();
        assert_eq!(
            *player.demo.app.world.get::<Transform>(camera).unwrap(),
            pose
        );
        for tick in 0..340 {
            player
                .demo
                .set_gameplay_input(bozzard_scene::GameplayInput {
                    movement: [0.0, 1.0],
                    jump: tick == 80,
                    ..Default::default()
                });
            player.demo.app.step();
        }
        player.demo.check_simulation().unwrap();
        assert!(player.demo.gameplay().unwrap().won);
        player
            .handle_key(&Key::Character("r".into()), false)
            .unwrap();
        let state = player.demo.gameplay().unwrap();
        assert!(!state.won && state.collected.is_empty() && state.checkpoint.is_none());
        assert_eq!(
            player
                .demo
                .instance()
                .capture(&player.demo.app.world)
                .unwrap(),
            document
        );
        assert_eq!(
            player
                .demo
                .app
                .world
                .resource::<bozzard_scene::GameplayInput>()
                .unwrap()
                .movement,
            [0.0; 2]
        );
    }
    #[test]
    fn view_pause_pan_and_transactional_reload_work_without_a_gpu() {
        let mut player = Player {
            assets: assets::Assets::load(&bozzard_demo::scene_document().unwrap(), None).unwrap(),
            options: Options::default(),
            view: None,
            demo: SceneDemo::new(&bozzard_demo::scene_document().unwrap()).unwrap(),
            paused: false,
            menu_input: Default::default(),
            gameplay_controls: gameplay_input::GameplayControls::default(),
            look: gameplay_input::Look::Off,
            last_frame: Instant::now(),
            last_present: Instant::now(),
            frames: 0,
            error: None,
            command_error: None,
        };
        player
            .handle_key(&Key::Named(NamedKey::Space), false)
            .unwrap();
        assert!(player.paused);
        player
            .handle_key(&Key::Named(NamedKey::Space), true)
            .unwrap();
        assert!(player.paused, "key repeat must not toggle pause");
        player
            .handle_key(&Key::Character("1".into()), false)
            .unwrap();
        assert_eq!(player.options.layer, Layer::TwoD);
        player
            .handle_key(&Key::Named(NamedKey::ArrowRight), false)
            .unwrap();
        let camera = player.demo.instance().camera_entity(Layer::TwoD).unwrap();
        assert_eq!(
            player
                .demo
                .app
                .world
                .get::<Transform>(camera)
                .unwrap()
                .translation[0],
            0.25
        );
        player.options.scene = Some(PathBuf::from("__bozzard_missing_scene__/missing.json"));
        assert!(
            player
                .handle_key(&Key::Character("r".into()), false)
                .is_err()
        );
        assert_eq!(
            player.demo.instance().camera_entity(Layer::TwoD).unwrap(),
            camera
        );
        player.options.scene = None;
        player
            .handle_key(&Key::Character("r".into()), false)
            .unwrap();
        assert_ne!(
            player.demo.instance().camera_entity(Layer::TwoD).unwrap(),
            camera
        );
        assert_eq!(player.demo.app.ticks(), 0);
    }
}
