use anyhow::{Context, Result, bail, ensure};
mod assets;
mod gameplay_input;
mod presentation;
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
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, KeyCode, NamedKey, PhysicalKey},
    window::{Window, WindowId},
};

struct Options {
    backend: Backend,
    software: bool,
    hardware: bool,
    smoke: bool,
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
            backend: Backend::native(),
            software: false,
            hardware: false,
            smoke: false,
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
            "--backend" => {
                result.backend = args.next().context("--backend needs a value")?.parse()?
            }
            "--software" => result.software = true,
            "--hardware" => result.hardware = true,
            "--smoke" => result.smoke = true,
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
                    "bozzard-player [--backend metal|vulkan|dx12] [--software|--hardware] [--frames N]\nbozzard-player --smoke [--backend ...] [--software|--hardware] [--output DIRECTORY]\n--scene FILE loads JSON; --write-scene FILE saves it and exits without a GPU.\n--view 2d|3d chooses the starting view; --save-path FILE sets the F5 destination.\n1/2: 2D/3D. Space: pause. Arrows: pan camera. F5: save. R: reload source. Escape: close.\nPlayer Controller scenes: WASD move, Space jump, right-drag orbit. Progress/win in title; physical R restarts."
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
        self.renderer.draw(
            &self.gpu,
            &frame.texture.create_view(&Default::default()),
            [self.config.width, self.config.height],
            &extract(
                demo,
                layer,
                self.config.width as f32 / self.config.height as f32,
            )?,
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

struct Player {
    options: Options,
    view: Option<View>,
    demo: SceneDemo,
    assets: assets::Assets,
    paused: bool,
    gameplay_controls: gameplay_input::GameplayControls,
    last_frame: Instant,
    last_present: Instant,
    frames: u32,
    error: Option<anyhow::Error>,
    command_error: Option<String>,
}

impl Player {
    fn window_title(&self) -> String {
        let status = if let Some(error) = &self.command_error {
            format!("ERROR: {error} | ")
        } else {
            String::new()
        };
        if let Some(state) = self.demo.gameplay() {
            format!(
                "Bozzard | {status}{} {}/{} | CP: {} | falls: {} | WASD move, Space jump, RMB orbit, physical R restart",
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
        } else {
            let layer = if self.options.layer == Layer::TwoD {
                "2D"
            } else {
                "3D"
            };
            let state = if self.paused { "paused" } else { "playing" };
            format!(
                "Bozzard — {status}{layer} / {state} | 1/2: view | Space: pause | Arrows: pan | F5: save | R: reload"
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
        if self.demo.gameplay().is_some() {
            if !synthetic && let PhysicalKey::Code(code) = physical {
                let input =
                    self.gameplay_controls
                        .key(code, state == ElementState::Pressed, repeat);
                if self.options.layer == Layer::ThreeD {
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
        if state == ElementState::Pressed && (!synthetic || self.demo.gameplay().is_none()) {
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
        match key {
            Key::Named(NamedKey::Space) if !repeat && self.demo.gameplay().is_none() => {
                self.paused = !self.paused
            }
            Key::Character(value) if !repeat && (value == "1" || value == "2") => {
                let layer = if value == "1" {
                    Layer::TwoD
                } else {
                    Layer::ThreeD
                };
                ensure!(
                    self.demo.instance.has_view(layer),
                    "scene has no {layer:?} view"
                );
                self.options.layer = layer;
                self.gameplay_controls.reset();
                self.demo.clear_gameplay_input();
            }
            Key::Character(value) if !repeat && value.eq_ignore_ascii_case("r") => {
                let document = load_document(self.options.scene.as_deref())?;
                let next = SceneDemo::new(&document)?;
                let assets = assets::Assets::load(&document, self.options.scene.as_deref())?;
                ensure!(
                    next.instance.has_view(self.options.layer),
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
                if self.demo.gameplay().is_some() {
                    self.paused = false;
                }
                self.last_frame = Instant::now();
                println!("scene_reloaded");
            }
            Key::Named(NamedKey::F5) if !repeat => {
                save_document_from(
                    &self.demo.instance.capture(&self.demo.app.world)?,
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
                let entity = self.demo.instance.camera_entity(self.options.layer)?;
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
        self.gameplay_controls.reset();
        self.demo.clear_gameplay_input();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if event_loop.exiting() {
            return;
        }
        if self.view.as_ref().is_none_or(|v| id != v.window.id()) {
            return;
        }
        if self.demo.gameplay().is_some() {
            if let Some(input) = self.gameplay_controls.event(&event)
                && self.options.layer == Layer::ThreeD
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
        let now = Instant::now();
        if matches!(event, WindowEvent::RedrawRequested) {
            if !self.paused {
                self.demo.app.advance(now.duration_since(self.last_frame));
            }
            self.last_frame = now;
        }
        let title = self.window_title();
        let Some(view) = self.view.as_mut() else {
            return;
        };
        view.window.set_title(&title);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
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
    if options.smoke {
        return smoke::run(&options);
    }
    let document = load_document(options.scene.as_deref())?;
    if let Some(path) = &options.write_scene {
        save_document_from(&document, path, options.scene.as_deref())?;
        println!("scene_saved path={}", path.display());
        return Ok(());
    }
    let demo = SceneDemo::new(&document)?;
    ensure!(
        demo.instance.has_view(options.layer),
        "scene has no requested view; use --view 2d or --view 3d"
    );
    let assets = assets::Assets::load(&document, options.scene.as_deref())?;
    let mut player = Player {
        assets,
        options,
        view: None,
        demo,
        paused: false,
        gameplay_controls: gameplay_input::GameplayControls::default(),
        last_frame: Instant::now(),
        last_present: Instant::now(),
        frames: 0,
        error: None,
        command_error: None,
    };
    EventLoop::new()?.run_app(&mut player)?;
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
            gameplay_controls: gameplay_input::GameplayControls::default(),
            last_frame: Instant::now(),
            last_present: Instant::now(),
            frames: 0,
            error: None,
            command_error: None,
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
        let camera = player.demo.instance.camera_entity(Layer::ThreeD).unwrap();
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
                .instance
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
            gameplay_controls: gameplay_input::GameplayControls::default(),
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
        let camera = player.demo.instance.camera_entity(Layer::TwoD).unwrap();
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
            player.demo.instance.camera_entity(Layer::TwoD).unwrap(),
            camera
        );
        player.options.scene = None;
        player
            .handle_key(&Key::Character("r".into()), false)
            .unwrap();
        assert_ne!(
            player.demo.instance.camera_entity(Layer::TwoD).unwrap(),
            camera
        );
        assert_eq!(player.demo.app.ticks(), 0);
    }
}
