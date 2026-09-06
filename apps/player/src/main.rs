use anyhow::{Context, Result, bail, ensure};
mod presentation;
mod smoke;
use bozzard_demo::{SceneDemo, load_document, save_document};
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
    keyboard::{Key, NamedKey},
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
                    "bozzard-player [--backend metal|vulkan|dx12] [--software|--hardware] [--frames N]\nbozzard-player --smoke [--backend ...] [--software|--hardware] [--output DIRECTORY]\n--scene FILE loads JSON; --write-scene FILE saves it and exits without a GPU.\n--view 2d|3d chooses the starting view; --save-path FILE sets the F5 destination.\n1/2: 2D/3D. Space: pause. Arrows: pan camera. F5: save. R: reload source. Escape: close."
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

    fn draw(&mut self, demo: &SceneDemo, layer: Layer) -> Result<bool> {
        if !self.drawable {
            self.surface_status = "window has zero size";
            return Ok(false);
        }
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
    paused: bool,
    last_frame: Instant,
    last_present: Instant,
    frames: u32,
    error: Option<anyhow::Error>,
}

impl Player {
    fn handle_key(&mut self, key: &Key, repeat: bool) -> Result<()> {
        match key {
            Key::Named(NamedKey::Space) if !repeat => self.paused = !self.paused,
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
            }
            Key::Character(value) if !repeat && value.eq_ignore_ascii_case("r") => {
                let next = SceneDemo::new(&load_document(self.options.scene.as_deref())?)?;
                ensure!(
                    next.instance.has_view(self.options.layer),
                    "reloaded scene is missing the active view"
                );
                self.demo = next;
                self.last_frame = Instant::now();
                println!("scene_reloaded");
            }
            Key::Named(NamedKey::F5) if !repeat => {
                save_document(
                    &self.demo.instance.capture(&self.demo.app.world)?,
                    &self.options.save_path,
                )?;
                println!("scene_saved path={}", self.options.save_path.display());
            }
            Key::Named(
                direction @ (NamedKey::ArrowLeft
                | NamedKey::ArrowRight
                | NamedKey::ArrowUp
                | NamedKey::ArrowDown),
            ) => {
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
            _ => return Ok(()),
        }
        if let Some(view) = &self.view {
            let layer = if self.options.layer == Layer::TwoD {
                "2D"
            } else {
                "3D"
            };
            let state = if self.paused { "paused" } else { "playing" };
            view.window.set_title(&format!("Bozzard — {layer} / {state} | 1/2: view | Space: pause | Arrows: pan | F5: save | R: reload"));
        }
        Ok(())
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
            Ok(view) => {
                self.view = Some(view);
                self.last_frame = Instant::now();
                self.last_present = Instant::now();
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.view = None;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if event_loop.exiting() {
            return;
        }
        if self.view.as_ref().is_none_or(|v| id != v.window.id()) {
            return;
        }
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && event.state == ElementState::Pressed
            && let Err(error) = self.handle_key(&event.logical_key, event.repeat)
        {
            eprintln!("scene command failed: {error:#}");
            if let Some(view) = &self.view {
                view.window.set_title(&format!("Bozzard — {error}"));
            }
        }
        let Some(view) = self.view.as_mut() else {
            return;
        };
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
                let now = Instant::now();
                if !self.paused {
                    self.demo.app.advance(now.duration_since(self.last_frame));
                }
                self.last_frame = now;
                match view.draw(&self.demo, self.options.layer) {
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
        save_document(&document, path)?;
        println!("scene_saved path={}", path.display());
        return Ok(());
    }
    let demo = SceneDemo::new(&document)?;
    ensure!(
        demo.instance.has_view(options.layer),
        "scene has no requested view; use --view 2d or --view 3d"
    );
    let mut player = Player {
        options,
        view: None,
        demo,
        paused: false,
        last_frame: Instant::now(),
        last_present: Instant::now(),
        frames: 0,
        error: None,
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
    #[test]
    fn view_pause_pan_and_transactional_reload_work_without_a_gpu() {
        let mut player = Player {
            options: Options::default(),
            view: None,
            demo: SceneDemo::new(&bozzard_demo::scene_document().unwrap()).unwrap(),
            paused: false,
            last_frame: Instant::now(),
            last_present: Instant::now(),
            frames: 0,
            error: None,
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
