//! Native Bozzard component player for the factory simulation.
use crate::{
    multiplayer::{Action as FactoryAction, Network},
    save::SaveFile,
    scene::SceneSource,
    sim::{Direction, Game, HEIGHT, Item, Kind, WIDTH},
    stage::Stage,
    steam::SteamBridge,
};
use anyhow::{Context, Result};
use bozzard_render::{Backend, Gpu, SceneRenderer, capture_offscreen, instance, wgpu};
use bozzard_scene::middleware::ui::Input;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Menu,
    Factory,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Build(Kind),
    Carry(Item),
    Inspect,
}

struct View {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    gpu: Gpu,
    config: wgpu::SurfaceConfiguration,
    renderer: SceneRenderer,
}
impl View {
    fn new(loop_: &ActiveEventLoop, stage: &Stage) -> Result<Self> {
        let window = Arc::new(
            loop_.create_window(
                Window::default_attributes()
                    .with_title("BOZZ-TORIO · Pocket Factory")
                    .with_inner_size(LogicalSize::new(1320., 830.))
                    .with_min_inner_size(LogicalSize::new(960., 690.)),
            )?,
        );
        let api = instance(Backend::native());
        let surface = api.create_surface(window.clone())?;
        let gpu = pollster::block_on(Gpu::request(&api, Some(&surface), false))?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .context("graphics surface is unsupported")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&gpu.device, &config);
        let renderer = SceneRenderer::new(&gpu, config.format);
        let mut view = Self {
            window,
            surface,
            gpu,
            config,
            renderer,
        };
        view.reload_assets(stage)?;
        Ok(view)
    }
    fn reload_assets(&mut self, stage: &Stage) -> Result<()> {
        let mut renderer = SceneRenderer::new(&self.gpu, self.config.format);
        for id in stage.instance.document().assets.keys() {
            let data = stage
                .assets
                .get(stage.assets.handle(id).context("missing asset handle")?)
                .context("missing scene asset")?
                .shared_data()
                .context("scene asset is not ready")?;
            bozzard_render_assets::upload(&self.gpu, &mut renderer, id, &data)?;
        }
        self.renderer = renderer;
        Ok(())
    }
    fn logical_size(&self) -> [f32; 2] {
        let scale = self.window.scale_factor() as f32;
        [
            self.config.width as f32 / scale,
            self.config.height as f32 / scale,
        ]
    }
    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.gpu.device, &self.config);
        }
    }
    fn draw(&mut self, stage: &Stage, screenshot: Option<&PathBuf>) -> Result<bool> {
        self.renderer
            .set_hud_scale(self.window.scale_factor() as f32);
        let render = stage.render_scene(self.logical_size())?;
        if let Some(path) = screenshot {
            let size = [self.config.width, self.config.height];
            let mut capture_renderer =
                SceneRenderer::new(&self.gpu, wgpu::TextureFormat::Rgba8Unorm);
            for id in stage.instance.document().assets.keys() {
                let data = stage
                    .assets
                    .get(stage.assets.handle(id).context("missing asset handle")?)
                    .context("missing scene asset")?
                    .shared_data()
                    .context("scene asset is not ready")?;
                bozzard_render_assets::upload(&self.gpu, &mut capture_renderer, id, &data)?;
            }
            let frame = capture_offscreen(&self.gpu, size[0], size[1], |target| {
                capture_renderer.draw(&self.gpu, target, size, &render)
            })?;
            image::save_buffer(path, &frame.rgba, size[0], size[1], image::ColorType::Rgba8)?;
            return Ok(true);
        }
        let (frame, reconfigure) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.gpu.device, &self.config);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Lost => anyhow::bail!("graphics surface lost"),
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("graphics surface validation failed")
            }
        };
        self.renderer.draw(
            &self.gpu,
            &frame.texture.create_view(&Default::default()),
            [self.config.width, self.config.height],
            &render,
        )?;
        self.window.pre_present_notify();
        self.gpu.queue.present(frame);
        if reconfigure {
            self.surface.configure(&self.gpu.device, &self.config);
        }
        Ok(false)
    }
}

pub struct Factory {
    source: SceneSource,
    stage: Stage,
    game: Game,
    save: SaveFile,
    steam: SteamBridge,
    network: Network,
    view: Option<View>,
    screenshot: Option<PathBuf>,
    screenshot_after: Duration,
    screenshot_started: Option<Instant>,
    screen: Screen,
    tool: Tool,
    facing: Direction,
    inspected: Option<(usize, usize)>,
    pointer: [f32; 2],
    primary_down: bool,
    secondary_down: bool,
    last_drag: Option<(usize, usize)>,
    view_x: usize,
    view_y: usize,
    zoom: f32,
    help: bool,
    show_map: bool,
    error: Option<String>,
    last_tick: Instant,
    last_frame: Instant,
    frame_dt: f32,
    anim_fraction: f32,
    last_save: Instant,
    failure: Option<anyhow::Error>,
    join_entry: bool,
    join_code: String,
    guest_placeholder: bool,
}
impl Factory {
    pub fn new(
        source: SceneSource,
        steam: SteamBridge,
        start_playing: bool,
        screenshot: Option<PathBuf>,
        screenshot_after: Duration,
        join_lobby: Option<u64>,
    ) -> Result<Self> {
        let save = SaveFile::default_path();
        let fresh = source.new_game()?;
        let network = Network::new(&steam, &source, join_lobby)?;
        let mut error = None;
        let game = if join_lobby.is_some() {
            fresh
        } else if save.exists() {
            match save.load() {
                Ok(game) => game,
                Err(problem) => {
                    let archived = save.archive_invalid()?;
                    error = Some(format!(
                        "Invalid save archived at {}: {problem:#}",
                        archived.display()
                    ));
                    fresh
                }
            }
        } else {
            fresh
        };
        let stage = Stage::new(&source, &game)?;
        Ok(Self {
            view_x: game.hub[0].saturating_sub(18),
            view_y: game.hub[1].saturating_sub(11),
            inspected: Some((game.hub[0], game.hub[1])),
            source,
            stage,
            game,
            save,
            steam,
            network,
            view: None,
            screenshot,
            screenshot_after,
            screenshot_started: None,
            screen: if start_playing {
                Screen::Factory
            } else {
                Screen::Menu
            },
            tool: Tool::Build(Kind::Miner),
            facing: Direction::East,
            pointer: [0.; 2],
            primary_down: false,
            secondary_down: false,
            last_drag: None,
            zoom: 32.,
            help: false,
            show_map: false,
            error,
            last_tick: Instant::now(),
            last_frame: Instant::now(),
            frame_dt: 0.,
            anim_fraction: 0.,
            last_save: Instant::now(),
            failure: None,
            join_entry: false,
            join_code: String::new(),
            guest_placeholder: join_lobby.is_some(),
        })
    }
    fn persist(&mut self) {
        if self.network.is_guest_or_joining() {
            return;
        }
        match self.save.write(&self.game) {
            Ok(()) => self.last_save = Instant::now(),
            Err(error) => self.error = Some(format!("Save failed: {error:#}")),
        }
    }
    fn record(&mut self, result: Result<(), &'static str>, world_change: bool) -> Result<()> {
        match result {
            Ok(()) => {
                self.error = None;
                if world_change {
                    self.stage.rebuild(&self.game)?;
                } else {
                    self.stage.sync_outputs(&self.game)?;
                }
            }
            Err(message) => self.error = Some(message.into()),
        }
        Ok(())
    }
    fn board_rect(size: [f32; 2]) -> [f32; 4] {
        [
            18.,
            255.,
            (size[0] - 318.).max(20.),
            (size[1] - 90.).max(280.),
        ]
    }
    fn board_tile(&self, size: [f32; 2], p: [f32; 2]) -> Option<(usize, usize)> {
        let [left, top, right, bottom] = Self::board_rect(size);
        if p[0] < left || p[0] >= right || p[1] < top || p[1] >= bottom {
            return None;
        }
        let x = self.view_x + ((p[0] - left) / self.zoom) as usize;
        let y = self.view_y + ((p[1] - top) / self.zoom) as usize;
        (x < WIDTH && y < HEIGHT).then_some((x, y))
    }
    fn apply_board(&mut self, size: [f32; 2], secondary: bool) -> Result<()> {
        if self.screen != Screen::Factory || self.help || self.show_map {
            return Ok(());
        }
        if self.stage.ui_frame(size)?.blocks_pointer(self.pointer) {
            return Ok(());
        }
        let Some((x, y)) = self.board_tile(size, self.pointer) else {
            return Ok(());
        };
        if self.last_drag == Some((x, y)) {
            return Ok(());
        }
        self.last_drag = Some((x, y));
        self.inspected = Some((x, y));
        if secondary {
            self.perform(FactoryAction::Remove {
                x: x as u16,
                y: y as u16,
            })
        } else {
            match self.tool {
                Tool::Build(kind) => self.perform(FactoryAction::Place {
                    x: x as u16,
                    y: y as u16,
                    kind,
                    direction: self.facing,
                }),
                Tool::Carry(item) => self.perform(FactoryAction::Inject {
                    x: x as u16,
                    y: y as u16,
                    item,
                }),
                Tool::Inspect => Ok(()),
            }
        }
    }
    fn jump(&mut self, x: usize, y: usize) {
        self.view_x = x.saturating_sub(14).min(WIDTH - 1);
        self.view_y = y.saturating_sub(8).min(HEIGHT - 1);
    }
    fn perform(&mut self, action: FactoryAction) -> Result<()> {
        if self.network.is_guest_or_joining() {
            match self.network.command(action) {
                Ok(()) => self.error = None,
                Err(error) => self.error = Some(error.to_string()),
            }
            return Ok(());
        }
        let structural = action.structural();
        let pause = matches!(action, FactoryAction::TogglePause);
        let result = action.apply(&mut self.game);
        let succeeded = result.is_ok();
        if pause {
            if let Err(message) = result {
                self.error = Some(message.into());
            }
        } else {
            self.record(result, structural)?;
        }
        if succeeded {
            self.network.force_publish();
        }
        if pause && succeeded && !self.game.paused {
            self.last_tick =
                Instant::now() - Duration::from_secs_f32(self.anim_fraction.clamp(0., 1.) * 0.2);
        }
        Ok(())
    }
    fn restore_local_factory(&mut self) -> Result<()> {
        self.game = if self.save.exists() {
            self.save.load()?
        } else {
            self.source.new_game()?
        };
        self.stage.rebuild(&self.game)?;
        self.inspected = Some((self.game.hub[0], self.game.hub[1]));
        self.jump(self.game.hub[0], self.game.hub[1]);
        self.screen = Screen::Menu;
        self.last_tick = Instant::now();
        self.anim_fraction = 0.;
        Ok(())
    }
    fn action(&mut self, name: &str, loop_: &ActiveEventLoop) -> Result<()> {
        if let Some(i) = name
            .strip_prefix("tool-")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let Some(kind) = Kind::BUILDABLE.get(i) {
                self.tool = Tool::Build(*kind);
            }
            return Ok(());
        }
        if let Some(i) = name
            .strip_prefix("carry-")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let Some(item) = Item::ALL.get(i) {
                self.tool = Tool::Carry(*item);
            }
            return Ok(());
        }
        if let Some(i) = name
            .strip_prefix("use-")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let Some(kind) = Kind::BUILDABLE.get(i) {
                self.tool = Tool::Build(*kind);
            }
            return Ok(());
        }
        if let Some(i) = name
            .strip_prefix("buy-")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let Some(kind) = Kind::BUILDABLE.get(i) {
                self.perform(FactoryAction::Buy { kind: *kind })?;
            }
            return Ok(());
        }
        match name {
            "pause" => self.perform(FactoryAction::TogglePause)?,
            "menu-button" => {
                self.persist();
                self.screen = Screen::Menu;
            }
            "map-button" => {
                self.show_map = true;
                self.help = false;
            }
            "rotate-selected" => {
                if let Some((x, y)) = self.inspected {
                    self.perform(FactoryAction::Rotate {
                        x: x as u16,
                        y: y as u16,
                    })?;
                }
            }
            "upgrade" => {
                if let Some((x, y)) = self.inspected {
                    self.perform(FactoryAction::Upgrade {
                        x: x as u16,
                        y: y as u16,
                    })?;
                }
            }
            "recipe" => {
                if let Some((x, y)) = self.inspected {
                    self.perform(FactoryAction::Recipe {
                        x: x as u16,
                        y: y as u16,
                    })?;
                }
            }
            "pick-up" => {
                if let Some((x, y)) = self.inspected {
                    self.perform(FactoryAction::TakeOutput {
                        x: x as u16,
                        y: y as u16,
                    })?;
                }
            }
            "recover" => {
                if let Some((x, y)) = self.inspected {
                    self.perform(FactoryAction::Remove {
                        x: x as u16,
                        y: y as u16,
                    })?;
                }
            }
            "inspect" => self.tool = Tool::Inspect,
            "orientation" => self.facing = self.facing.next(),
            "pan-left" => self.view_x = self.view_x.saturating_sub(8),
            "pan-right" => self.view_x = (self.view_x + 8).min(WIDTH - 1),
            "pan-up" => self.view_y = self.view_y.saturating_sub(8),
            "pan-down" => self.view_y = (self.view_y + 8).min(HEIGHT - 1),
            "hub" | "map-hub" => {
                self.jump(self.game.hub[0], self.game.hub[1]);
                self.show_map = false;
            }
            "zoom-out" => self.zoom = (self.zoom - 2.).max(14.),
            "zoom-in" => self.zoom = (self.zoom + 2.).min(42.),
            "help-button" | "menu-help" => {
                self.help = true;
                self.show_map = false;
            }
            "close-help" => self.help = false,
            "continue" => {
                if self.network.is_guest_or_joining() && !self.network.has_state() {
                    self.error = Some("Waiting for the host's factory state".into());
                    return Ok(());
                }
                self.screen = Screen::Factory;
                self.last_tick = Instant::now();
                self.anim_fraction = 0.;
            }
            "new" => {
                if self.network.lobby_id().is_some() || self.network.busy() {
                    self.error = Some("Leave the lobby before loading a new editor scene".into());
                    return Ok(());
                }
                let source = SceneSource::open(self.source.path.clone())?;
                let game = source.new_game()?;
                self.stage.reload(&source, &game)?;
                if let Some(view) = &mut self.view {
                    view.reload_assets(&self.stage)?;
                }
                self.game = game;
                self.source = source;
                self.inspected = Some((self.game.hub[0], self.game.hub[1]));
                self.jump(self.game.hub[0], self.game.hub[1]);
                self.screen = Screen::Factory;
                self.last_tick = Instant::now();
                self.anim_fraction = 0.;
                self.persist();
            }
            "quit" => {
                self.persist();
                loop_.exit();
            }
            "steam-friends" => self.steam.overlay(),
            "create-lobby" => {
                if let Err(error) = self.network.create() {
                    self.error = Some(error.to_string());
                }
            }
            "join-lobby" => {
                if self.join_code.is_empty() {
                    self.join_entry = true;
                } else if let Ok(id) = self.join_code.parse::<u64>() {
                    if let Err(error) = self.network.join(id) {
                        self.error = Some(error.to_string());
                    }
                    self.join_entry = false;
                }
            }
            "invite-lobby" => {
                if let Err(error) = self.network.invite() {
                    self.error = Some(error.to_string());
                }
            }
            "leave-lobby" => {
                let guest = self.network.is_guest_or_joining();
                if self.network.is_host() {
                    self.persist();
                }
                self.network.leave();
                self.join_entry = false;
                self.join_code.clear();
                if guest {
                    self.restore_local_factory()?;
                }
            }
            "close-map" => self.show_map = false,
            "map-nw" | "map-ne" | "map-sw" | "map-se" => {
                let x = if name.ends_with('e') { 192 } else { 64 };
                let y = if name.contains("-s") { 192 } else { 64 };
                self.jump(x, y);
                self.show_map = false;
            }
            _ => {}
        }
        Ok(())
    }
    fn ui_input(&mut self, size: [f32; 2], input: Input, loop_: &ActiveEventLoop) -> Result<()> {
        for action in self.stage.input(size, input)? {
            self.action(action, loop_)?;
        }
        Ok(())
    }
    fn sync_ui(&mut self, size: [f32; 2]) -> Result<()> {
        self.stage.canvas("hud", self.screen == Screen::Factory)?;
        self.stage.canvas("menu", self.screen == Screen::Menu)?;
        self.stage.canvas("help", self.help)?;
        self.stage.canvas("map", self.show_map)?;
        self.stage.text(
            "phase",
            format!(
                "TIER {} / PHASE {} · 256² WORLD",
                self.game.tier(),
                self.game.phase()
            ),
        )?;
        let order = self.game.order();
        self.stage.text(
            "contract",
            format!(
                "CONTRACT {:02} · {}",
                self.game.order_index + 1,
                order.item.name().to_uppercase()
            ),
        )?;
        self.stage.text(
            "progress",
            format!("{} / {} DELIVERED", self.game.order_progress, order.amount),
        )?;
        self.stage
            .text("credits", format!("CREDITS  ¤{}", self.game.credits))?;
        let (power_mask, capacity) = self.game.power_network();
        self.stage.text(
            "electricity",
            format!("POWER {} / {capacity}", self.game.energy_used),
        )?;
        self.stage
            .text("pause", if self.game.paused { "RESUME" } else { "PAUSE" })?;
        self.stage.text(
            "coordinates",
            format!(
                "{:03}:{:03} · {}×{}",
                self.view_x, self.view_y, WIDTH, HEIGHT
            ),
        )?;
        self.stage.text(
            "objective-title",
            format!(
                "OBJECTIVE · TIER {} / PHASE {}",
                self.game.tier(),
                self.game.phase()
            ),
        )?;
        let remaining = order.amount.saturating_sub(self.game.order_progress);
        let hint = if self.game.order_index == 0 {
            "Place Miner on ore, Furnace beside it, then Conveyor to Hub."
        } else {
            "Complete this contract to advance and unlock the next upgrade."
        };
        self.stage.text(
            "objective-body",
            format!(
                "Deliver {remaining} × {} to the Hub.\n{hint}",
                order.item.name()
            ),
        )?;
        self.stage.text(
            "objective-unlock",
            format!("NEXT UNLOCK: {}", self.game.next_unlock()),
        )?;
        self.stage.text(
            "notice",
            self.error
                .as_deref()
                .unwrap_or(&self.game.notice)
                .to_owned(),
        )?;
        self.stage
            .text("steam-status", self.steam.status().to_owned())?;
        self.stage
            .enabled("steam-friends", self.steam.connected())?;
        let lobby_id = self.network.lobby_id();
        self.stage.text(
            "lobby-id",
            lobby_id.map_or_else(
                || "NO LOBBY".into(),
                |id| format!("LOBBY {id} · {} PLAYER(S)", self.network.members_len()),
            ),
        )?;
        self.stage.text(
            "lobby-status",
            self.error
                .as_deref()
                .unwrap_or(self.network.status())
                .to_owned(),
        )?;
        self.stage.text(
            "lobby-hud",
            if let Some(id) = lobby_id {
                if self.network.is_host() {
                    format!("HOST · {id}\nSAVING THIS FACTORY")
                } else {
                    format!("GUEST · {id}\nHOST SAVES THE FACTORY")
                }
            } else {
                "SOLO FACTORY".into()
            },
        )?;
        self.stage
            .enabled("create-lobby", self.network.can_create_or_join())?;
        self.stage
            .enabled("join-lobby", self.network.can_create_or_join())?;
        self.stage.enabled("invite-lobby", lobby_id.is_some())?;
        self.stage.enabled(
            "leave-lobby",
            lobby_id.is_some() || self.network.is_guest_or_joining(),
        )?;
        self.stage.text(
            "join-lobby",
            if self.join_entry {
                format!("ID: {}_", self.join_code)
            } else if self.join_code.is_empty() {
                "JOIN BY ID".into()
            } else {
                format!("JOIN {}", self.join_code)
            },
        )?;
        self.stage.enabled("continue", self.network.has_state())?;
        self.stage
            .enabled("new", lobby_id.is_none() && !self.network.busy())?;
        self.stage
            .text("orientation", format!("{}  R", direction_name(self.facing)))?;
        let tool = match self.tool {
            Tool::Build(kind) => format!(
                "PLACE {} {}",
                kind.name().to_uppercase(),
                direction_name(self.facing)
            ),
            Tool::Carry(item) => format!("CARRY {}", item.name().to_uppercase()),
            Tool::Inspect => "INSPECT".into(),
        };
        self.stage.text("current-tool", tool)?;
        for (i, item) in Item::ALL.iter().enumerate() {
            self.stage.text(
                &format!("carry-{i}"),
                format!("{}  ×{}", item.name(), self.game.stock[item.index()]),
            )?;
        }
        for (i, kind) in Kind::BUILDABLE.iter().enumerate() {
            let unlocked = self.game.order_index >= kind.unlock_after();
            self.stage.text(
                &format!("shop-{i}"),
                if unlocked {
                    format!("{} · ¤{}", kind.name(), kind.price())
                } else {
                    format!("{} · LOCKED", kind.name())
                },
            )?;
            self.stage.enabled(&format!("use-{i}"), unlocked)?;
            self.stage.enabled(
                &format!("buy-{i}"),
                unlocked && self.game.credits >= kind.price(),
            )?;
            if i < 5 {
                self.stage.text(
                    &format!("tool-{i}"),
                    format!(
                        "{}\n{}\n×{}",
                        i + 1,
                        kind.name(),
                        self.game.buildings[kind.index()]
                    ),
                )?;
            }
        }
        if let Some((x, y)) = self.inspected {
            let tile = &self.game.tiles[y * WIDTH + x];
            let building = tile.building.as_ref();
            self.stage.text(
                "selected",
                if let Some(b) = building {
                    format!(
                        "WORLD {x}:{y}  {} MK {}  {}",
                        b.kind.name().to_uppercase(),
                        b.level,
                        direction_name(b.direction)
                    )
                } else if let Some(resource) = tile.deposit {
                    format!("WORLD {x}:{y}  {}", resource.name())
                } else {
                    format!("WORLD {x}:{y}  EMPTY")
                },
            )?;
            let can_rotate = building.is_some_and(|b| b.kind != Kind::Hub);
            self.stage.enabled("rotate-selected", can_rotate)?;
            self.stage.enabled("recover", can_rotate)?;
            self.stage.enabled(
                "recipe",
                building.is_some_and(|b| b.kind == Kind::Assembler),
            )?;
            self.stage
                .enabled("pick-up", building.is_some_and(|b| b.output.is_some()))?;
            let upgrade = building.is_some_and(|b| {
                b.kind != Kind::Hub
                    && b.level < self.game.max_level(b.kind)
                    && self.game.credits >= b.kind.price() * u32::from(b.level + 1) + 8
            });
            self.stage.enabled("upgrade", upgrade)?;
            self.stage.text(
                "upgrade",
                if let Some(b) = building {
                    format!("UPGRADE MK {}", b.level + 1)
                } else {
                    "UPGRADE".into()
                },
            )?;
        }
        let board = Self::board_rect(size);
        self.stage
            .sync_terrain(&self.game, [self.view_x, self.view_y])?;
        self.stage.conveyor_preview(self.facing)?;
        self.stage.camera(
            size,
            [board[0], board[1]],
            [self.view_x, self.view_y],
            self.zoom,
        )?;
        self.stage
            .sync_progress(&self.game, self.anim_fraction, &power_mask)?;
        self.stage.animate(self.frame_dt, self.anim_fraction)?;
        Ok(())
    }
    fn tick(&mut self) -> Result<()> {
        let now = Instant::now();
        self.frame_dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        let was_paused = self.game.paused;
        let was_host = self.network.is_host();
        let change = self.network.update(&mut self.game)?;
        if was_host && !self.network.is_host() {
            self.persist();
        }
        if change.guest_lost || change.join_failed && self.guest_placeholder {
            self.restore_local_factory()?;
            self.guest_placeholder = false;
        } else if change.join_failed {
            self.last_tick = now;
        } else if change.state {
            if change.structural {
                self.stage.rebuild(&self.game)?;
            } else {
                self.stage.sync_outputs(&self.game)?;
            }
            self.last_tick = now;
            self.anim_fraction = 0.;
        }
        if was_paused && !self.game.paused {
            self.last_tick = now;
        }
        if !self.network.is_guest_or_joining() {
            self.steam.pump(&self.game);
        }
        if self.screen == Screen::Factory || self.network.is_host() {
            if !self.game.paused
                && (!self.network.is_guest_or_joining() || self.network.has_state())
            {
                let steps = (now.duration_since(self.last_tick).as_millis() / 200).min(8) as usize;
                if steps > 0 {
                    for _ in 0..steps {
                        self.game.tick();
                    }
                    self.last_tick = now;
                    self.stage.sync_outputs(&self.game)?;
                }
                self.anim_fraction =
                    (now.duration_since(self.last_tick).as_secs_f32() / 0.2).min(1.);
            } else {
                self.frame_dt = 0.;
            }
            if self.last_save.elapsed() > Duration::from_secs(10) {
                self.persist();
            }
        } else {
            self.last_tick = now;
            self.anim_fraction = 0.;
        }
        self.network.publish(&self.game)?;
        Ok(())
    }
    fn key(&mut self, key: &Key, loop_: &ActiveEventLoop) -> Result<()> {
        if self.join_entry {
            match key {
                Key::Named(NamedKey::Escape) => self.join_entry = false,
                Key::Named(NamedKey::Backspace) => {
                    self.join_code.pop();
                }
                Key::Named(NamedKey::Enter) => {
                    if let Ok(id) = self.join_code.parse::<u64>() {
                        if let Err(error) = self.network.join(id) {
                            self.error = Some(error.to_string());
                        }
                        self.join_entry = false;
                    }
                }
                Key::Character(value) if value.chars().all(|c| c.is_ascii_digit()) => {
                    for digit in value.chars() {
                        if self.join_code.len() < 20 {
                            self.join_code.push(digit);
                        }
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        match key {
            Key::Named(NamedKey::Tab) => self.ui_input(
                self.view.as_ref().map_or([1320., 830.], View::logical_size),
                Input::FocusNext { reverse: false },
                loop_,
            )?,
            Key::Named(NamedKey::Enter) => self.ui_input(
                self.view.as_ref().map_or([1320., 830.], View::logical_size),
                Input::Activate,
                loop_,
            )?,
            Key::Named(NamedKey::Escape) => {
                if self.help {
                    self.help = false;
                } else if self.show_map {
                    self.show_map = false;
                } else if self.screen == Screen::Factory {
                    self.persist();
                    self.screen = Screen::Menu;
                }
            }
            Key::Named(NamedKey::F1) => self.help = !self.help,
            Key::Named(NamedKey::Space) if self.screen == Screen::Factory => {
                self.perform(FactoryAction::TogglePause)?;
            }
            Key::Named(NamedKey::ArrowLeft) => self.view_x = self.view_x.saturating_sub(8),
            Key::Named(NamedKey::ArrowRight) => self.view_x = (self.view_x + 8).min(WIDTH - 1),
            Key::Named(NamedKey::ArrowUp) => self.view_y = self.view_y.saturating_sub(8),
            Key::Named(NamedKey::ArrowDown) => self.view_y = (self.view_y + 8).min(HEIGHT - 1),
            Key::Character(value) if value.eq_ignore_ascii_case("r") => {
                self.facing = self.facing.next()
            }
            Key::Character(value) if value.eq_ignore_ascii_case("i") => self.tool = Tool::Inspect,
            Key::Character(value) if value.eq_ignore_ascii_case("a") => {
                self.view_x = self.view_x.saturating_sub(8)
            }
            Key::Character(value) if value.eq_ignore_ascii_case("d") => {
                self.view_x = (self.view_x + 8).min(WIDTH - 1)
            }
            Key::Character(value) if value.eq_ignore_ascii_case("w") => {
                self.view_y = self.view_y.saturating_sub(8)
            }
            Key::Character(value) if value.eq_ignore_ascii_case("s") => {
                self.view_y = (self.view_y + 8).min(HEIGHT - 1)
            }
            Key::Character(value)
                if value.len() == 1 && matches!(value.as_str(), "1" | "2" | "3" | "4" | "5") =>
            {
                let index = value.parse::<usize>()? - 1;
                self.tool = Tool::Build(Kind::BUILDABLE[index]);
            }
            _ => {}
        }
        Ok(())
    }
    fn fail(&mut self, loop_: &ActiveEventLoop, error: anyhow::Error) {
        self.failure = Some(error);
        loop_.exit();
    }
}

impl ApplicationHandler for Factory {
    fn resumed(&mut self, loop_: &ActiveEventLoop) {
        if self.view.is_none() {
            match View::new(loop_, &self.stage) {
                Ok(view) => {
                    self.view = Some(view);
                    let now = Instant::now();
                    self.last_tick = now;
                    self.last_frame = now;
                    self.screenshot_started = Some(now);
                }
                Err(error) => self.fail(loop_, error),
            }
        }
    }
    fn suspended(&mut self, _loop_: &ActiveEventLoop) {
        self.view = None;
    }
    fn window_event(&mut self, loop_: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.view.as_ref().is_none_or(|view| view.window.id() != id) {
            return;
        }
        let result = (|| -> Result<()> {
            match event {
                WindowEvent::CloseRequested => {
                    self.persist();
                    loop_.exit();
                }
                WindowEvent::Resized(size) => {
                    self.view.as_mut().unwrap().resize(size.width, size.height)
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let scale = self.view.as_ref().unwrap().window.scale_factor() as f32;
                    self.pointer = [position.x as f32 / scale, position.y as f32 / scale];
                    let size = self.view.as_ref().unwrap().logical_size();
                    self.ui_input(size, Input::PointerMove(self.pointer), loop_)?;
                    if self.primary_down && matches!(self.tool, Tool::Build(Kind::Belt)) {
                        self.apply_board(size, false)?;
                    }
                    if self.secondary_down {
                        self.apply_board(size, true)?;
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    let size = self.view.as_ref().unwrap().logical_size();
                    match (button, state) {
                        (MouseButton::Left, ElementState::Pressed) => {
                            self.primary_down = true;
                            self.last_drag = None;
                            self.ui_input(size, Input::PointerDown(self.pointer), loop_)?;
                            self.apply_board(size, false)?;
                        }
                        (MouseButton::Left, ElementState::Released) => {
                            self.primary_down = false;
                            self.last_drag = None;
                            self.ui_input(size, Input::PointerUp(self.pointer), loop_)?;
                        }
                        (MouseButton::Right, ElementState::Pressed) => {
                            self.secondary_down = true;
                            self.last_drag = None;
                            self.apply_board(size, true)?;
                        }
                        (MouseButton::Right, ElementState::Released) => {
                            self.secondary_down = false;
                            self.last_drag = None;
                        }
                        _ => {}
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let d = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 24.,
                    };
                    let size = self.view.as_ref().unwrap().logical_size();
                    if self.screen == Screen::Factory
                        && self.pointer[0] >= size[0] - 300.
                        && self.pointer[1] >= 76.
                        && self.pointer[1] < size[1] - 90.
                    {
                        self.ui_input(
                            size,
                            Input::ScrollAt {
                                point: self.pointer,
                                delta: -d * 30.,
                            },
                            loop_,
                        )?;
                    } else {
                        self.zoom = (self.zoom + d.signum() * 2.).clamp(14., 42.);
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed && !event.repeat =>
                {
                    self.key(&event.logical_key, loop_)?
                }
                WindowEvent::RedrawRequested => {
                    self.tick()?;
                    let size = self.view.as_ref().unwrap().logical_size();
                    self.sync_ui(size)?;
                    let capture = self
                        .screenshot_started
                        .is_some_and(|start| start.elapsed() >= self.screenshot_after);
                    let done = self
                        .view
                        .as_mut()
                        .unwrap()
                        .draw(&self.stage, self.screenshot.as_ref().filter(|_| capture))?;
                    if done {
                        loop_.exit();
                    }
                }
                _ => {}
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.fail(loop_, error);
        }
    }
    fn about_to_wait(&mut self, loop_: &ActiveEventLoop) {
        if loop_.exiting() {
            return;
        }
        if let Some(view) = &self.view {
            view.window.request_redraw();
        }
        loop_.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(35),
        ));
    }
}

pub fn run(mut factory: Factory) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut factory)?;
    if let Some(error) = factory.failure {
        return Err(error);
    }
    Ok(())
}

fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::North => "N",
        Direction::East => "E",
        Direction::South => "S",
        Direction::West => "W",
    }
}
