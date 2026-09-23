use crate::{
    save::SaveFile,
    scene::SceneSource,
    sim::{Direction, Game, HEIGHT, Item, Kind, Resource, WIDTH},
    sprites::Sprites,
    steam::SteamBridge,
};
use eframe::egui::{
    self, Align2, Color32, FontId, Key, Rect, RichText, Sense, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(12, 21, 31);
const PANEL: Color32 = Color32::from_rgb(20, 34, 44);
const CELL: Color32 = Color32::from_rgb(30, 48, 54);
const CELL_ALT: Color32 = Color32::from_rgb(34, 53, 59);
const EDGE: Color32 = Color32::from_rgb(58, 91, 96);
const TEAL: Color32 = Color32::from_rgb(105, 236, 201);
const COPPER: Color32 = Color32::from_rgb(250, 179, 103);
const CREAM: Color32 = Color32::from_rgb(232, 238, 224);
const MUTED: Color32 = Color32::from_rgb(150, 174, 178);

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

pub struct FactoryApp {
    screen: Screen,
    help: bool,
    game: Game,
    save: SaveFile,
    scene: SceneSource,
    sprites: Sprites,
    steam: SteamBridge,
    tool: Tool,
    facing: Direction,
    inspected: Option<(usize, usize)>,
    hover: Option<(usize, usize)>,
    last_drag_tile: Option<(usize, usize)>,
    last_tick: Instant,
    last_save: Instant,
    error: Option<String>,
    screenshot: Option<std::path::PathBuf>,
    screenshot_requested: bool,
    frames: u32,
    view_x: usize,
    view_y: usize,
    zoom: f32,
    visible_columns: usize,
    visible_rows: usize,
    show_map: bool,
}

impl FactoryApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        steam: SteamBridge,
        scene: SceneSource,
        start_playing: bool,
        screenshot: Option<std::path::PathBuf>,
    ) -> anyhow::Result<Self> {
        let save = SaveFile::default_path();
        let mut error = None;
        let fresh = scene.new_game()?;
        let game = if save.exists() {
            match save.load() {
                Ok(game) => game,
                Err(failure) => {
                    error = Some(match save.archive_invalid() {
                        Ok(archive) => format!(
                            "Save was invalid ({failure}); archived at {}",
                            archive.display()
                        ),
                        Err(archive_error) => format!(
                            "Could not load save ({failure}) or archive it ({archive_error})"
                        ),
                    });
                    fresh
                }
            }
        } else {
            fresh
        };
        let inspected = Some((game.hub[0], game.hub[1]));
        let view_x = game.hub[0].saturating_sub(18);
        let view_y = game.hub[1].saturating_sub(11);
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(CREAM);
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.widgets.inactive.bg_fill = PANEL;
        visuals.widgets.inactive.weak_bg_fill = PANEL;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, EDGE);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(41, 72, 77);
        visuals.widgets.active.bg_fill = Color32::from_rgb(43, 94, 89);
        visuals.selection.bg_fill = Color32::from_rgb(30, 116, 103);
        cc.egui_ctx.set_visuals(visuals);
        Ok(Self {
            screen: if start_playing {
                Screen::Factory
            } else {
                Screen::Menu
            },
            help: false,
            game,
            save,
            sprites: Sprites::load(&cc.egui_ctx, &scene.atlas_path)?,
            scene,
            steam,
            tool: Tool::Build(Kind::Miner),
            facing: Direction::East,
            inspected,
            hover: None,
            last_drag_tile: None,
            last_tick: Instant::now(),
            last_save: Instant::now(),
            error,
            screenshot,
            screenshot_requested: false,
            frames: 0,
            view_x,
            view_y,
            zoom: 25.0,
            visible_columns: 24,
            visible_rows: 20,
            show_map: false,
        })
    }

    fn record_error(&mut self, result: Result<(), &'static str>) {
        if let Err(message) = result {
            self.game.notice = message.into();
        }
    }

    fn persist(&mut self) {
        if let Err(error) = self.save.write(&self.game) {
            self.error = Some(format!("Save failed: {error}"));
        }
        self.last_save = Instant::now();
    }

    fn new_from_editor_scene(&mut self, ctx: &egui::Context) {
        let next = SceneSource::open(self.scene.path.clone()).and_then(|scene| {
            let game = scene.new_game()?;
            let sprites = Sprites::load(ctx, &scene.atlas_path)?;
            Ok((scene, game, sprites))
        });
        match next {
            Ok((scene, game, sprites)) => {
                self.scene = scene;
                self.game = game;
                self.sprites = sprites;
                self.inspected = Some((self.game.hub[0], self.game.hub[1]));
                self.view_x = self.game.hub[0].saturating_sub(18);
                self.view_y = self.game.hub[1].saturating_sub(11);
                self.error = None;
                self.persist();
                self.screen = Screen::Factory;
                self.last_tick = Instant::now();
            }
            Err(error) => self.error = Some(format!("Editor scene could not load: {error:#}")),
        }
    }

    fn hotkeys(&mut self, ctx: &egui::Context) {
        let keys = ctx.input(|i| {
            (
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::F1),
                i.key_pressed(Key::R),
                i.key_pressed(Key::Space),
                i.key_pressed(Key::I),
                [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5]
                    .map(|key| i.key_pressed(key)),
            )
        });
        if keys.0 {
            if self.help {
                self.help = false;
            } else if self.screen == Screen::Factory {
                self.persist();
                self.screen = Screen::Menu;
            }
        }
        if keys.1 {
            self.help = !self.help;
        }
        if self.screen != Screen::Factory {
            return;
        }
        if keys.2 {
            self.facing = self.facing.next();
        }
        if keys.3 {
            self.game.paused = !self.game.paused;
        }
        if keys.4 {
            self.tool = Tool::Inspect;
        }
        for (index, pressed) in keys.5.into_iter().enumerate() {
            if pressed {
                self.tool = Tool::Build(Kind::BUILDABLE[index]);
            }
        }
        let pan = ctx.input(|i| {
            (
                i.key_pressed(Key::A) || i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::D) || i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::W) || i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::S) || i.key_pressed(Key::ArrowDown),
            )
        });
        if pan.0 {
            self.view_x = self.view_x.saturating_sub(8);
        }
        if pan.1 {
            self.view_x = (self.view_x + 8).min(WIDTH - 1);
        }
        if pan.2 {
            self.view_y = self.view_y.saturating_sub(8);
        }
        if pan.3 {
            self.view_y = (self.view_y + 8).min(HEIGHT - 1);
        }
    }

    fn simulate(&mut self, ctx: &egui::Context) {
        self.steam.pump(&self.game);
        if self.screen == Screen::Factory {
            let elapsed = Instant::now().saturating_duration_since(self.last_tick);
            let steps = (elapsed.as_millis() / 200).min(8) as usize;
            if steps > 0 {
                for _ in 0..steps {
                    self.game.tick();
                }
                self.last_tick = Instant::now();
            }
            if self.last_save.elapsed() > Duration::from_secs(10) {
                self.persist();
            }
        } else {
            self.last_tick = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(35));
    }

    fn top_bar(&mut self, root: &mut egui::Ui) {
        let order = self.game.order();
        egui::Panel::top("factory-header")
            .exact_size(72.0)
            .frame(
                egui::Frame::NONE
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(18, 9)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("BOZZ-TORIO")
                                .color(TEAL)
                                .monospace()
                                .size(24.0)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "TIER {}  /  PHASE {}  ·  256² WORLD",
                                self.game.tier(),
                                self.game.phase()
                            ))
                            .color(MUTED)
                            .monospace()
                            .size(10.0),
                        );
                    });
                    ui.add_space(28.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "CONTRACT {:02}  ·  {}",
                                self.game.order_index + 1,
                                order.item.name().to_uppercase()
                            ))
                            .color(CREAM)
                            .monospace()
                            .strong(),
                        );
                        ui.add(
                            egui::ProgressBar::new(
                                (self.game.order_progress as f32 / order.amount as f32)
                                    .clamp(0.0, 1.0),
                            )
                            .desired_width(250.0)
                            .fill(TEAL)
                            .text(format!(
                                "{} / {} delivered",
                                self.game.order_progress, order.amount
                            )),
                        );
                    });
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("CREDITS").color(MUTED).monospace().size(10.0));
                        ui.label(
                            RichText::new(format!("¤ {}", self.game.credits))
                                .color(COPPER)
                                .monospace()
                                .size(20.0)
                                .strong(),
                        );
                    });
                    ui.add_space(18.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("ELECTRICITY")
                                .color(MUTED)
                                .monospace()
                                .size(10.0),
                        );
                        ui.label(
                            RichText::new(format!(
                                "⚡ {} / {}",
                                self.game.energy_used, self.game.energy_capacity
                            ))
                            .color(CREAM)
                            .monospace(),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("☰  MENU").clicked() {
                            self.persist();
                            self.screen = Screen::Menu;
                        }
                        if ui
                            .button(if self.game.paused {
                                "▶  RESUME"
                            } else {
                                "Ⅱ  PAUSE"
                            })
                            .clicked()
                        {
                            self.game.paused = !self.game.paused;
                        }
                    });
                });
            });
    }

    fn toolbelt(&mut self, root: &mut egui::Ui) {
        egui::Panel::bottom("factory-toolbelt")
            .exact_size(96.0)
            .frame(
                egui::Frame::NONE
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(18, 10)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("BUILD  /  1—5")
                                .color(TEAL)
                                .monospace()
                                .strong(),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            for (i, kind) in Kind::BUILDABLE.into_iter().take(5).enumerate() {
                                let (rect, response) =
                                    ui.allocate_exact_size(vec2(103.0, 57.0), Sense::click());
                                if response.clicked() {
                                    self.tool = Tool::Build(kind);
                                }
                                let active = self.tool == Tool::Build(kind);
                                let painter = ui.painter();
                                painter.rect_filled(
                                    rect,
                                    4.0,
                                    if active {
                                        Color32::from_rgb(37, 80, 76)
                                    } else {
                                        BG
                                    },
                                );
                                painter.rect_stroke(
                                    rect,
                                    4.0,
                                    Stroke::new(
                                        if active { 2.0 } else { 1.0 },
                                        if active { TEAL } else { EDGE },
                                    ),
                                    StrokeKind::Outside,
                                );
                                self.sprites.draw(
                                    painter,
                                    kind.sprite(),
                                    Rect::from_min_size(
                                        rect.min + vec2(7.0, 7.0),
                                        vec2(42.0, 42.0),
                                    ),
                                );
                                painter.text(
                                    rect.min + vec2(53.0, 8.0),
                                    Align2::LEFT_TOP,
                                    format!("{}", i + 1),
                                    FontId::monospace(10.0),
                                    COPPER,
                                );
                                painter.text(
                                    rect.min + vec2(53.0, 22.0),
                                    Align2::LEFT_TOP,
                                    kind.name(),
                                    FontId::monospace(11.0),
                                    CREAM,
                                );
                                painter.text(
                                    rect.min + vec2(53.0, 39.0),
                                    Align2::LEFT_TOP,
                                    format!("×{}", self.game.buildings[kind.index()]),
                                    FontId::monospace(11.0),
                                    TEAL,
                                );
                            }
                            let (rect, response) =
                                ui.allocate_exact_size(vec2(74.0, 57.0), Sense::click());
                            if response.clicked() {
                                self.tool = Tool::Inspect;
                            }
                            let active = self.tool == Tool::Inspect;
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                if active {
                                    Color32::from_rgb(37, 80, 76)
                                } else {
                                    BG
                                },
                            );
                            ui.painter().rect_stroke(
                                rect,
                                4.0,
                                Stroke::new(
                                    if active { 2.0 } else { 1.0 },
                                    if active { TEAL } else { EDGE },
                                ),
                                StrokeKind::Outside,
                            );
                            ui.painter().text(
                                rect.center_top() + vec2(0.0, 8.0),
                                Align2::CENTER_TOP,
                                "◎",
                                FontId::monospace(22.0),
                                TEAL,
                            );
                            ui.painter().text(
                                rect.center_bottom() - vec2(0.0, 6.0),
                                Align2::CENTER_BOTTOM,
                                "INSPECT I",
                                FontId::monospace(10.0),
                                CREAM,
                            );
                        });
                    });
                    ui.add_space(22.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("ORIENTATION")
                                .color(MUTED)
                                .monospace()
                                .size(10.0),
                        );
                        if ui
                            .add_sized(
                                [92.0, 34.0],
                                egui::Button::new(
                                    RichText::new(format!("{}  R", self.facing.glyph()))
                                        .monospace()
                                        .color(TEAL),
                                ),
                            )
                            .clicked()
                        {
                            self.facing = self.facing.next();
                        }
                        ui.label(RichText::new("Right click removes").color(MUTED).size(10.0));
                    });
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("CURRENT TOOL")
                                .color(MUTED)
                                .monospace()
                                .size(10.0),
                        );
                        let label = match self.tool {
                            Tool::Build(kind) => format!("PLACE {}", kind.name().to_uppercase()),
                            Tool::Carry(item) => format!("LOAD {}", item.name().to_uppercase()),
                            Tool::Inspect => "INSPECT".into(),
                        };
                        ui.label(RichText::new(label).color(COPPER).monospace().strong());
                        ui.label(
                            RichText::new("F1  guide  ·  Space  pause")
                                .color(MUTED)
                                .size(10.0),
                        );
                    });
                });
            });
    }

    fn inventory(&mut self, root: &mut egui::Ui) {
        egui::Panel::right("factory-inventory")
            .exact_size(302.0)
            .frame(
                egui::Frame::NONE
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(16, 15)),
            )
            .show(root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(
                    RichText::new("INVENTORY")
                        .color(TEAL)
                        .monospace()
                        .size(20.0)
                        .strong(),
                );
                ui.label(
                    RichText::new("Carry items into machines by hand.")
                        .color(MUTED)
                        .size(11.0),
                );
                if ui.small_button(if self.show_map { "HIDE WORLD MAP" } else { "SHOW WORLD MAP" }).clicked() {
                    self.show_map = !self.show_map;
                }
                ui.add_space(10.0);
                if self.show_map {
                ui.label(RichText::new("WORLD MAP · CLICK TO JUMP").color(TEAL).monospace().size(11.0));
                let (map_rect, map_response) = ui.allocate_exact_size(vec2(160.0, 160.0), Sense::click());
                ui.painter().rect_filled(map_rect, 2.0, BG);
                for (index, tile) in self.game.tiles.iter().enumerate() {
                    if let Some(resource) = tile.deposit {
                        let point = pos2(
                            map_rect.left() + (index % WIDTH) as f32 * map_rect.width() / WIDTH as f32,
                            map_rect.top() + (index / WIDTH) as f32 * map_rect.height() / HEIGHT as f32,
                        );
                        let color = match resource {
                            Resource::IronOre => Color32::from_rgb(145, 187, 202),
                            Resource::CopperOre => COPPER,
                            Resource::Coal => Color32::from_rgb(164, 154, 194),
                        };
                        ui.painter().circle_filled(point, 1.1, color);
                    }
                }
                let marker = pos2(
                    map_rect.left() + self.game.hub[0] as f32 * map_rect.width() / WIDTH as f32,
                    map_rect.top() + self.game.hub[1] as f32 * map_rect.height() / HEIGHT as f32,
                );
                ui.painter().circle_filled(marker, 3.0, TEAL);
                let viewport = Rect::from_min_size(
                    pos2(map_rect.left() + self.view_x as f32 * map_rect.width() / WIDTH as f32,
                         map_rect.top() + self.view_y as f32 * map_rect.height() / HEIGHT as f32),
                    vec2(self.visible_columns as f32 * map_rect.width() / WIDTH as f32,
                         self.visible_rows as f32 * map_rect.height() / HEIGHT as f32),
                );
                ui.painter().rect_stroke(viewport, 0.0, Stroke::new(1.0, TEAL), StrokeKind::Outside);
                if map_response.clicked() && let Some(point) = map_response.interact_pointer_pos() {
                    let x = (((point.x - map_rect.left()) / map_rect.width()) * WIDTH as f32) as usize;
                    let y = (((point.y - map_rect.top()) / map_rect.height()) * HEIGHT as f32) as usize;
                    self.view_x = x.saturating_sub(self.visible_columns / 2).min(WIDTH - 1);
                    self.view_y = y.saturating_sub(self.visible_rows / 2).min(HEIGHT - 1);
                }
                ui.add_space(8.0);
                }
                for item in Item::ALL {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
                    if response.clicked() {
                        self.tool = Tool::Carry(item);
                    }
                    let active = self.tool == Tool::Carry(item);
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        if active {
                            Color32::from_rgb(37, 80, 76)
                        } else {
                            BG
                        },
                    );
                    if active {
                        ui.painter().rect_stroke(
                            rect,
                            3.0,
                            Stroke::new(1.5, TEAL),
                            StrokeKind::Outside,
                        );
                    }
                    self.sprites.draw(
                        ui.painter(),
                        item.sprite(),
                        Rect::from_min_size(rect.min + vec2(4.0, 3.0), vec2(36.0, 36.0)),
                    );
                    ui.painter().text(
                        rect.min + vec2(47.0, 12.0),
                        Align2::LEFT_TOP,
                        item.name(),
                        FontId::monospace(12.0),
                        CREAM,
                    );
                    ui.painter().text(
                        rect.right_top() + vec2(-11.0, 12.0),
                        Align2::RIGHT_TOP,
                        format!("×{}", self.game.stock[item.index()]),
                        FontId::monospace(12.0),
                        if self.game.stock[item.index()] > 0 {
                            COPPER
                        } else {
                            MUTED
                        },
                    );
                    ui.add_space(3.0);
                }
                ui.add_space(9.0);
                ui.separator();
                ui.label(
                    RichText::new("SELECTED TILE")
                        .color(TEAL)
                        .monospace()
                        .strong(),
                );
                self.inspector(ui);
                ui.add_space(9.0);
                ui.separator();
                ui.label(RichText::new(format!("TIER {} / PHASE {}", self.game.tier(), self.game.phase())).color(TEAL).monospace().strong());
                ui.label(RichText::new(format!("Next unlock: {}", self.game.next_unlock())).color(COPPER).size(11.0));
                ui.label(RichText::new("Complete each delivery contract to advance. Upgrade machines from their inspector after unlocking them.").color(MUTED).size(11.0));
                ui.add_space(8.0);
                ui.separator();
                ui.label(
                    RichText::new("SUPPLY SHOP")
                        .color(TEAL)
                        .monospace()
                        .strong(),
                );
                ui.label(
                    RichText::new("Orders pay for expansion.")
                        .color(MUTED)
                        .size(11.0),
                );
                for kind in Kind::BUILDABLE {
                    ui.horizontal(|ui| {
                        let unlocked = self.game.order_index >= kind.unlock_after();
                        ui.label(
                            RichText::new(if unlocked { format!("{}  ·  ¤{}", kind.name(), kind.price()) } else { format!("{}  ·  LOCKED", kind.name()) })
                                .monospace()
                                .size(11.0),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    unlocked && self.game.credits >= kind.price(),
                                    egui::Button::new("BUY").min_size(vec2(48.0, 20.0)),
                                )
                                .clicked()
                            {
                                let result = self.game.buy(kind);
                                self.record_error(result);
                            }
                            if ui.add_enabled(unlocked, egui::Button::new("USE")).clicked() {
                                self.tool = Tool::Build(kind);
                            }
                        });
                    });
                }
                });
            });
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let Some((x, y)) = self.inspected else {
            ui.label(RichText::new("Click a tile to inspect it.").color(MUTED));
            return;
        };
        let tile = &self.game.tiles[y * WIDTH + x];
        let deposit = tile.deposit;
        let building = tile.building.clone();
        ui.label(
            RichText::new(format!("WORLD {:03}:{:03}", x, y))
                .color(MUTED)
                .monospace()
                .size(11.0),
        );
        if let Some(ore) = deposit {
            ui.label(
                RichText::new(format!("Deposit: {}", ore.name()))
                    .color(COPPER)
                    .size(11.0),
            );
        }
        let Some(b) = building else {
            ui.label(RichText::new("Empty floor").color(MUTED));
            return;
        };
        ui.label(
            RichText::new(format!(
                "{} MK {}  {}",
                b.kind.name().to_uppercase(),
                b.level,
                b.direction.glyph()
            ))
            .color(CREAM)
            .monospace()
            .size(15.0)
            .strong(),
        );
        if b.kind == Kind::Assembler {
            ui.label(RichText::new(format!("Recipe: {}", b.recipe.item().name())).color(COPPER));
        }
        if let Some(item) = b.output {
            ui.label(
                RichText::new(format!("Output ready: {}", item.name()))
                    .color(TEAL)
                    .size(11.0),
            );
        }
        let queued: Vec<_> = Item::ALL
            .into_iter()
            .filter(|item| b.input[item.index()] > 0)
            .map(|item| format!("{} ×{}", item.name(), b.input[item.index()]))
            .collect();
        if !queued.is_empty() {
            ui.label(
                RichText::new(format!("Buffer: {}", queued.join(", ")))
                    .color(MUTED)
                    .size(11.0),
            );
        }
        if b.level > 1
            && matches!(
                b.kind,
                Kind::Miner | Kind::Furnace | Kind::Assembler | Kind::Belt | Kind::Splitter
            )
        {
            let (powered, _) = self.game.power_network();
            ui.label(
                RichText::new(if powered[y * WIDTH + x] {
                    "⚡ Powered"
                } else {
                    "⚡ No grid connection"
                })
                .color(if powered[y * WIDTH + x] { TEAL } else { COPPER }),
            );
        }
        ui.horizontal(|ui| {
            if b.kind != Kind::Hub && ui.small_button("ROTATE").clicked() {
                let result = self.game.rotate(x, y);
                self.record_error(result);
            }
            if b.kind == Kind::Assembler && ui.small_button("RECIPE").clicked() {
                let result = self.game.set_recipe(x, y);
                self.record_error(result);
            }
            if b.output.is_some() && ui.small_button("PICK UP").clicked() {
                let result = self.game.take_output(x, y);
                self.record_error(result);
            }
        });
        if b.kind != Kind::Hub {
            let unlocked = b.level < self.game.max_level(b.kind);
            let price = b.kind.price() * u32::from(b.level + 1) + 8;
            if ui
                .add_enabled(
                    unlocked && self.game.credits >= price,
                    egui::Button::new(format!("UPGRADE TO MK {} · ¤{}", b.level + 1, price)),
                )
                .clicked()
            {
                let result = self.game.upgrade(x, y);
                self.record_error(result);
            }
            if !unlocked {
                ui.label(
                    RichText::new("Next upgrade unlocks after another phase")
                        .color(MUTED)
                        .size(10.0),
                );
            }
        }
        if b.kind != Kind::Hub && ui.small_button("RECOVER BUILDING").clicked() {
            let result = self.game.remove(x, y);
            self.record_error(result);
        }
    }

    fn click_tile(&mut self, x: usize, y: usize, secondary: bool) {
        self.inspected = Some((x, y));
        let result = if secondary {
            self.game.remove(x, y)
        } else {
            match self.tool {
                Tool::Build(kind) => self.game.place(x, y, kind, self.facing),
                Tool::Carry(item) => self.game.inject(x, y, item),
                Tool::Inspect => Ok(()),
            }
        };
        self.record_error(result);
    }

    fn board(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG).inner_margin(egui::Margin::same(18)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("FACTORY WORLD").color(TEAL).monospace().strong());
                    ui.add_space(8.0);
                    if ui.small_button("L").clicked() { self.view_x = self.view_x.saturating_sub(8); }
                    if ui.small_button("R").clicked() { self.view_x = (self.view_x + 8).min(WIDTH - 1); }
                    if ui.small_button("U").clicked() { self.view_y = self.view_y.saturating_sub(8); }
                    if ui.small_button("D").clicked() { self.view_y = (self.view_y + 8).min(HEIGHT - 1); }
                    if ui.small_button("HUB").clicked() {
                        self.view_x = self.game.hub[0].saturating_sub(18);
                        self.view_y = self.game.hub[1].saturating_sub(11);
                    }
                    if ui.small_button("-").clicked() { self.zoom = (self.zoom - 2.0).max(14.0); }
                    if ui.small_button("+").clicked() { self.zoom = (self.zoom + 2.0).min(42.0); }
                    ui.label(RichText::new(format!("{:03}:{:03} · {} × {}", self.view_x, self.view_y, WIDTH, HEIGHT)).color(MUTED).monospace().size(10.0));
                });
                ui.add_space(9.0);
                let available = ui.available_size();
                let columns = ((available.x / self.zoom).floor() as usize).clamp(1, WIDTH);
                let rows = (((available.y - 77.0).max(160.0) / self.zoom).floor() as usize).clamp(1, HEIGHT);
                self.visible_columns = columns;
                self.visible_rows = rows;
                self.view_x = self.view_x.min(WIDTH - columns);
                self.view_y = self.view_y.min(HEIGHT - rows);
                let tile = self.zoom;
                let size = vec2(tile * columns as f32, tile * rows as f32);
                let origin = ui.cursor().min;
                let board_rect = Rect::from_min_size(origin, size);
                let response = ui.interact(board_rect, ui.id().with("world-board"), Sense::click_and_drag());
                let painter = ui.painter().clone();
                painter.rect_filled(board_rect.expand(2.0), 2.0, EDGE);
                let (powered, _) = self.game.power_network();
                for row in 0..rows {
                    for col in 0..columns {
                        let x = self.view_x + col;
                        let y = self.view_y + row;
                        let rect = Rect::from_min_size(origin + vec2(col as f32 * tile, row as f32 * tile), Vec2::splat(tile));
                        let cell = rect.shrink(0.4);
                        let index = y * WIDTH + x;
                        painter.rect_filled(cell, 0.0, if (x + y).is_multiple_of(2) { CELL } else { CELL_ALT });
                        let floor_frame = self.game.terrain[index];
                        if floor_frame != 255 { self.sprites.draw(&painter, floor_frame as usize, cell); }
                        let tile_data = &self.game.tiles[index];
                        if let Some(resource) = tile_data.deposit {
                            let tint = match resource {
                                Resource::IronOre => Color32::from_rgb(45, 80, 83),
                                Resource::CopperOre => Color32::from_rgb(91, 64, 52),
                                Resource::Coal => Color32::from_rgb(56, 60, 76),
                            };
                            painter.rect_filled(cell.shrink(2.0), 2.0, tint);
                            self.sprites.draw(&painter, resource.sprite(), cell.shrink(1.0));
                        }
                        if let Some(building) = &tile_data.building {
                            if building.kind == Kind::Hub { painter.rect_filled(cell, 2.0, Color32::from_rgb(28, 103, 97)); }
                            self.sprites.draw(&painter, building.kind.sprite(), cell.shrink(1.0));
                            if building.level > 1 {
                                painter.rect_stroke(cell.shrink(1.0), 1.0, Stroke::new(1.5, if powered[index] { TEAL } else { COPPER }), StrokeKind::Inside);
                            }
                            if building.kind != Kind::Hub && !matches!(building.kind, Kind::Generator | Kind::PowerPole) && tile >= 20.0 {
                                painter.text(rect.right_bottom() - vec2(tile * 0.12, tile * 0.31), Align2::RIGHT_BOTTOM, building.direction.glyph(), FontId::monospace((tile * 0.28).max(9.0)), CREAM);
                            }
                            if let Some(item) = building.output {
                                let badge = Rect::from_min_size(rect.right_top() + vec2(-tile * 0.50, 1.0), Vec2::splat(tile * 0.43));
                                painter.rect_filled(badge, 2.0, BG);
                                self.sprites.draw(&painter, item.sprite(), badge);
                            }
                        }
                        if self.inspected == Some((x, y)) {
                            painter.rect_stroke(cell.shrink(1.0), 1.0, Stroke::new(2.0, COPPER), StrokeKind::Inside);
                        }
                    }
                }
                self.hover = response.hover_pos().filter(|p| board_rect.contains(*p)).map(|p| {
                    (self.view_x + ((p.x - origin.x) / tile).floor() as usize,
                     self.view_y + ((p.y - origin.y) / tile).floor() as usize)
                }).filter(|(x, y)| *x < WIDTH && *y < HEIGHT);
                if let Some((x, y)) = self.hover {
                    let rect = Rect::from_min_size(origin + vec2((x - self.view_x) as f32 * tile, (y - self.view_y) as f32 * tile), Vec2::splat(tile));
                    painter.rect_stroke(rect.shrink(1.0), 1.0, Stroke::new(2.0, TEAL), StrokeKind::Inside);
                }
                if response.hovered() {
                    let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                    if scroll.abs() > 1.0 { self.zoom = (self.zoom + scroll.signum() * 2.0).clamp(14.0, 42.0); }
                }
                let pointer = ctx.input(|i| (i.pointer.primary_down(), i.pointer.secondary_down()));
                if !pointer.0 && !pointer.1 { self.last_drag_tile = None; }
                if let Some((x, y)) = self.hover {
                    if pointer.1 && self.last_drag_tile != Some((x, y)) {
                        self.click_tile(x, y, true); self.last_drag_tile = Some((x, y));
                    } else if pointer.0 && self.last_drag_tile != Some((x, y)) {
                        self.click_tile(x, y, false); self.last_drag_tile = Some((x, y));
                    }
                }
                ui.add_space(size.y + 10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("◉").color(COPPER));
                    ui.label(RichText::new(&self.game.notice).color(CREAM).size(12.0));
                    if let Some(error) = &self.error { ui.label(RichText::new(error).color(COPPER).size(11.0)); }
                });
                ui.label(RichText::new(format!("WASD / arrows pan · wheel zoom · world seed {} · {} generated resource nodes", self.game.seed, self.game.tiles.iter().filter(|tile| tile.deposit.is_some()).count())).color(MUTED).size(10.0));
            });
    }

    fn menu(&mut self, root: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG))
            .show(root, |ui| {
                let available = ui.available_rect_before_wrap();
                let painter = ui.painter().clone();
                let spacing = 36.0;
                for x in 0..=((available.width() / spacing) as usize) {
                    let at = available.left() + x as f32 * spacing;
                    painter.line_segment(
                        [pos2(at, available.top()), pos2(at, available.bottom())],
                        Stroke::new(1.0, Color32::from_rgb(24, 40, 48)),
                    );
                }
                for y in 0..=((available.height() / spacing) as usize) {
                    let at = available.top() + y as f32 * spacing;
                    painter.line_segment(
                        [pos2(available.left(), at), pos2(available.right(), at)],
                        Stroke::new(1.0, Color32::from_rgb(24, 40, 48)),
                    );
                }
                let card_w = available.width().min(700.0);
                let card = Rect::from_center_size(
                    available.center(),
                    vec2(card_w, available.height().min(545.0)),
                );
                painter.rect_filled(card, 7.0, PANEL);
                painter.rect_stroke(card, 7.0, Stroke::new(2.0, EDGE), StrokeKind::Outside);
                painter.rect_filled(
                    Rect::from_min_size(card.min, vec2(card.width(), 5.0)),
                    0.0,
                    TEAL,
                );
                let mut card_ui =
                    ui.new_child(egui::UiBuilder::new().max_rect(card.shrink2(vec2(36.0, 28.0))));
                card_ui.vertical_centered(|ui| {
                    ui.add_space(26.0);
                    ui.label(
                        RichText::new("BOZZ-TORIO")
                            .color(TEAL)
                            .monospace()
                            .strong()
                            .size(52.0),
                    );
                    ui.label(
                        RichText::new("A VAST FACTORY BUILT ONE PHASE AT A TIME")
                            .color(COPPER)
                            .monospace()
                            .size(12.0),
                    );
                    ui.add_space(25.0);
                    let (strip, _) = ui.allocate_exact_size(vec2(400.0, 86.0), Sense::hover());
                    for (i, sprite) in [6, 9, 7, 9, 8, 9, 11].into_iter().enumerate() {
                        if sprite == 9 {
                            ui.painter().text(
                                strip.min + vec2(i as f32 * 55.0 + 14.0, 32.0),
                                Align2::CENTER_CENTER,
                                "→",
                                FontId::monospace(26.0),
                                COPPER,
                            );
                        } else {
                            self.sprites.draw(
                                ui.painter(),
                                sprite,
                                Rect::from_min_size(
                                    strip.min + vec2(i as f32 * 55.0, 7.0),
                                    Vec2::splat(66.0),
                                ),
                            );
                        }
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("MINE  ·  REFINE  ·  POWER  ·  EXPAND")
                            .color(MUTED)
                            .monospace()
                            .size(11.0),
                    );
                    ui.add_space(29.0);
                    if ui
                        .add_sized(
                            [300.0, 39.0],
                            egui::Button::new(
                                RichText::new(if self.save.exists() {
                                    "CONTINUE FACTORY"
                                } else {
                                    "START FACTORY"
                                })
                                .monospace()
                                .color(BG)
                                .strong(),
                            )
                            .fill(TEAL),
                        )
                        .clicked()
                    {
                        self.screen = Screen::Factory;
                        self.last_tick = Instant::now();
                    }
                    ui.add_space(7.0);
                    if ui
                        .add_sized(
                            [300.0, 34.0],
                            egui::Button::new(RichText::new("NEW FROM EDITOR SCENE").monospace()),
                        )
                        .clicked()
                    {
                        self.new_from_editor_scene(ui.ctx());
                    }
                    ui.add_space(7.0);
                    if ui
                        .add_sized(
                            [300.0, 34.0],
                            egui::Button::new(RichText::new("HOW TO PLAY").monospace()),
                        )
                        .clicked()
                    {
                        self.help = true;
                    }
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(self.steam.status())
                            .color(if self.steam.connected() { TEAL } else { MUTED })
                            .size(11.0),
                    );
                    if self.steam.connected() && ui.small_button("OPEN STEAM FRIENDS").clicked() {
                        self.steam.overlay();
                    }
                    ui.add_space(9.0);
                    if ui.small_button("QUIT TO DESKTOP").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ui.add_space(7.0);
                    ui.label(
                        RichText::new(format!("SCENE · {}", self.scene.name))
                            .color(MUTED)
                            .monospace()
                            .size(10.0),
                    )
                    .on_hover_text(format!("{}", self.scene.path.display()));
                    if let Some(error) = &self.error {
                        ui.label(RichText::new(error).color(COPPER).size(11.0));
                    }
                });
                painter.text(
                    pos2(available.left() + 18.0, available.bottom() - 18.0),
                    Align2::LEFT_BOTTOM,
                    "BOZZ INDUSTRIES   /   EST. 2026",
                    FontId::monospace(10.0),
                    MUTED,
                );
                painter.text(
                    pos2(available.right() - 18.0, available.bottom() - 18.0),
                    Align2::RIGHT_BOTTOM,
                    "2D  /  ORIGINAL PIXEL ART",
                    FontId::monospace(10.0),
                    MUTED,
                );
            });
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        if !self.help {
            return;
        }
        egui::Window::new("FIELD MANUAL  /  BOZZ-TORIO").collapsible(false).resizable(false).default_width(470.0).open(&mut self.help)
            .show(ctx,|ui| {
                ui.label(RichText::new("BUILD YOUR FIRST FACTORY").color(TEAL).monospace().strong());
                ui.add_space(8.0);
                ui.label("1. Select Miner (1) and place it on an iron deposit.");
                ui.label("2. Select Furnace (2) and place it immediately to the right.");
                ui.label("3. Select Conveyor (4) and drag from the furnace to the delivery hub.");
                ui.label(format!("4. The factory runs automatically. Deliver {} iron ingots to complete the first order.",self.game.first_order_amount));
                ui.add_space(9.0);
                ui.label(RichText::new("GO FURTHER").color(COPPER).monospace().strong());
                ui.label("Explore the 256 × 256 world with WASD or arrow keys, click SHOW WORLD MAP to jump across it, and use the mouse wheel to zoom. Every new world scatters iron, copper, and coal nodes from a saved seed. Assemblers turn two iron ingots into a gear, or iron and copper into a circuit.");
                ui.label("Complete all three phases in a tier to reach the next. Each phase unlocks another machine upgrade. Tier 2 unlocks generators and power poles: put a generator on coal, then extend its network with poles. Mk II gives a mechanical speedup; power boosts it further and enables the full speed of Mk III+ machines.");
                ui.add_space(9.0);
                ui.label(RichText::new("CONTROLS").color(COPPER).monospace().strong());
                ui.label("1–5 basic tools  ·  power tools in Supply shop  ·  I inspect  ·  R rotate  ·  right click recover  ·  Space pause  ·  F1 guide  ·  Esc menu");
                ui.label("Select an inventory item, then click a compatible machine to load it by hand. Completed output can be picked up from the inspector.");
                ui.add_space(8.0);
                ui.label(RichText::new("EDIT THE FACTORY").color(COPPER).monospace().strong());
                ui.label("The Bozzard editor contains the 22 × 15 starter district inside the generated world. Move its deposits or hub, paint its floor, change the world seed on the scene blackboard, or place machine templates. Save, then choose NEW FROM EDITOR SCENE here.");
                ui.label(RichText::new(format!("Editor scene: {}",self.scene.path.display())).color(MUTED).size(10.0));
                ui.label(RichText::new(format!("Autosave: {}",self.save.path().display())).color(MUTED).size(10.0));
            });
    }

    fn capture_if_requested(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.as_ref() else {
            return;
        };
        for event in ctx.input(|input| input.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event {
                let bytes: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                if let Err(error) = image::save_buffer(
                    path,
                    &bytes,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    self.error = Some(format!("Screenshot failed: {error}"));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        self.frames += 1;
        if self.frames >= 8 && !self.screenshot_requested {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }
}

impl eframe::App for FactoryApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.hotkeys(&ctx);
        self.simulate(&ctx);
        match self.screen {
            Screen::Menu => self.menu(root),
            Screen::Factory => {
                self.top_bar(root);
                self.toolbelt(root);
                self.inventory(root);
                self.board(root, &ctx);
            }
        }
        self.help_window(&ctx);
        self.capture_if_requested(&ctx);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.persist();
    }
}

impl Drop for FactoryApp {
    fn drop(&mut self) {
        let _ = self.save.write(&self.game);
    }
}
