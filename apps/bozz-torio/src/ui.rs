use crate::{
    save::SaveFile,
    sim::{Direction, Game, HEIGHT, Item, Kind, WIDTH},
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
}

impl FactoryApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        steam: SteamBridge,
        start_playing: bool,
        screenshot: Option<std::path::PathBuf>,
    ) -> anyhow::Result<Self> {
        let save = SaveFile::default_path();
        let mut error = None;
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
                    Game::new()
                }
            }
        } else {
            Game::new()
        };
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
            sprites: Sprites::load(&cc.egui_ctx)?,
            steam,
            tool: Tool::Build(Kind::Miner),
            facing: Direction::East,
            inspected: Some((4, 7)),
            hover: None,
            last_drag_tile: None,
            last_tick: Instant::now(),
            last_save: Instant::now(),
            error,
            screenshot,
            screenshot_requested: false,
            frames: 0,
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
                            RichText::new("POCKET FACTORY  /  SECTOR 01")
                                .color(MUTED)
                                .monospace()
                                .size(10.0),
                        );
                    });
                    ui.add_space(28.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "ORDER {:02}  ·  {}",
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
                            RichText::new("PRODUCTION")
                                .color(MUTED)
                                .monospace()
                                .size(10.0),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} items",
                                self.game.produced.iter().sum::<u32>()
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
                            for (i, kind) in Kind::BUILDABLE.into_iter().enumerate() {
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
                ui.add_space(10.0);
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
                        ui.label(
                            RichText::new(format!("{}  ·  ¤{}", kind.name(), kind.price()))
                                .monospace()
                                .size(11.0),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    self.game.credits >= kind.price(),
                                    egui::Button::new("BUY").min_size(vec2(48.0, 20.0)),
                                )
                                .clicked()
                            {
                                let result = self.game.buy(kind);
                                self.record_error(result);
                            }
                        });
                    });
                }
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
            RichText::new(format!("GRID {:02}:{:02}", x, y))
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
                "{}  {}",
                b.kind.name().to_uppercase(),
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
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG).inner_margin(egui::Margin::same(18)))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("FACTORY FLOOR").color(TEAL).monospace().strong());
                    ui.add_space(8.0);
                    ui.label(RichText::new("ore → refine → assemble → deliver").color(MUTED).monospace().size(11.0));
                });
                ui.add_space(9.0);
                let available=ui.available_size();
                let tile=(available.x/WIDTH as f32).min((available.y-47.0)/HEIGHT as f32).floor().max(16.0);
                let size=vec2(tile*WIDTH as f32,tile*HEIGHT as f32);
                let origin=pos2(ui.min_rect().min.x+(available.x-size.x)/2.0,ui.cursor().min.y);
                let board_rect=Rect::from_min_size(origin,size);
                let response=ui.interact(board_rect,ui.id().with("board"),Sense::click_and_drag());
                let painter=ui.painter();
                painter.rect_filled(board_rect.expand(3.0),3.0,EDGE);
                for y in 0..HEIGHT { for x in 0..WIDTH {
                    let rect=Rect::from_min_size(origin+vec2(x as f32*tile,y as f32*tile),Vec2::splat(tile));
                    let cell=rect.shrink(0.5);
                    painter.rect_filled(cell,0.0,if (x+y)%2==0 {CELL} else {CELL_ALT});
                    let tile_data=&self.game.tiles[y*WIDTH+x];
                    if let Some(deposit)=tile_data.deposit {
                        let tint=if deposit==Item::IronOre {Color32::from_rgb(45,80,83)} else {Color32::from_rgb(91,64,52)};
                        painter.rect_filled(cell.shrink(2.0),2.0,tint);
                        self.sprites.draw(painter,if deposit==Item::IronOre {12} else {13},cell.shrink(1.0));
                    }
                    if let Some(building)=&tile_data.building {
                        if building.kind==Kind::Hub { painter.rect_filled(cell,2.0,Color32::from_rgb(28,103,97)); }
                        self.sprites.draw(painter,building.kind.sprite(),cell.shrink(1.0));
                        if building.kind!=Kind::Hub {
                            painter.text(rect.right_bottom()-vec2(tile*0.14,tile*0.33),Align2::RIGHT_BOTTOM,building.direction.glyph(),FontId::monospace((tile*0.30).max(10.0)),CREAM);
                        }
                        if let Some(item)=building.output {
                            let badge=Rect::from_min_size(rect.right_top()+vec2(-tile*0.50,1.0),Vec2::splat(tile*0.43));
                            painter.rect_filled(badge,2.0,BG);
                            self.sprites.draw(painter,item.sprite(),badge);
                        }
                    }
                    if self.inspected==Some((x,y)) { painter.rect_stroke(cell.shrink(1.0),1.0,Stroke::new(2.0,COPPER),StrokeKind::Inside); }
                }}
                self.hover=response.hover_pos().filter(|p|board_rect.contains(*p)).map(|p| {
                    (((p.x-origin.x)/tile).floor() as usize,((p.y-origin.y)/tile).floor() as usize)
                }).filter(|(x,y)|*x<WIDTH && *y<HEIGHT);
                if let Some((x,y))=self.hover {
                    let rect=Rect::from_min_size(origin+vec2(x as f32*tile,y as f32*tile),Vec2::splat(tile));
                    painter.rect_stroke(rect.shrink(1.0),1.0,Stroke::new(2.0,TEAL),StrokeKind::Inside);
                }
                let pointer=ctx.input(|i| (i.pointer.primary_down(),i.pointer.secondary_down()));
                if !pointer.0 && !pointer.1 { self.last_drag_tile=None; }
                if let Some((x,y))=self.hover {
                    if pointer.1 && self.last_drag_tile!=Some((x,y)) {
                        self.click_tile(x,y,true);self.last_drag_tile=Some((x,y));
                    } else if pointer.0 && self.last_drag_tile!=Some((x,y)) {
                        self.click_tile(x,y,false);self.last_drag_tile=Some((x,y));
                    }
                }
                ui.add_space(size.y+12.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("◉").color(COPPER));
                    ui.label(RichText::new(&self.game.notice).color(CREAM).size(12.0));
                    if let Some(error)=&self.error { ui.label(RichText::new(error).color(COPPER).size(11.0)); }
                });
                ui.label(RichText::new("Drag to lay conveyors  ·  Select an item in Inventory, then click a machine to hand-load it  ·  Inspect a machine to change its recipe").color(MUTED).size(10.0));
                ui.add_space(25.0);
                ui.label(RichText::new("PRODUCTION NOTES  /  FIELD GUIDE").color(TEAL).monospace().size(12.0).strong());
                ui.add_space(8.0);
                let width=(ui.available_width()-16.0)/3.0;
                let start=ui.cursor().min;
                for (i,(title,formula,detail,sprite)) in [
                    ("01  IRON INGOT","IRON ORE → FURNACE","First delivery order",Item::IronBar.sprite()),
                    ("02  GEAR","2 IRON INGOTS","Assembler recipe",Item::Gear.sprite()),
                    ("03  CIRCUIT","IRON + COPPER","Assembler recipe",Item::Circuit.sprite()),
                ].into_iter().enumerate() {
                    let rect=Rect::from_min_size(start+vec2(i as f32*(width+8.0),0.0),vec2(width,107.0));
                    ui.painter().rect_filled(rect,4.0,PANEL);
                    ui.painter().rect_stroke(rect,4.0,Stroke::new(1.0,EDGE),StrokeKind::Inside);
                    self.sprites.draw(ui.painter(),sprite,Rect::from_min_size(rect.min+vec2(8.0,9.0),Vec2::splat(40.0)));
                    ui.painter().text(rect.min+vec2(53.0,14.0),Align2::LEFT_TOP,title,FontId::monospace(11.0),COPPER);
                    ui.painter().text(rect.min+vec2(9.0,62.0),Align2::LEFT_TOP,formula,FontId::monospace(10.0),CREAM);
                    ui.painter().text(rect.min+vec2(9.0,83.0),Align2::LEFT_TOP,detail,FontId::monospace(10.0),MUTED);
                }
                ui.add_space(112.0);
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
                        RichText::new("A POCKET-SIZED FACTORY ABOUT BIG IDEAS")
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
                        RichText::new("MINE  ·  REFINE  ·  ASSEMBLE  ·  DELIVER")
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
                            egui::Button::new(RichText::new("NEW FACTORY").monospace()),
                        )
                        .clicked()
                    {
                        self.game = Game::new();
                        self.error = None;
                        self.persist();
                        self.screen = Screen::Factory;
                        self.last_tick = Instant::now();
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
                ui.label("1. Select Miner (1) and place it on the iron deposit at 04:07.");
                ui.label("2. Select Furnace (2) and place it immediately to the right.");
                ui.label("3. Select Conveyor (4) and drag from the furnace to the delivery hub.");
                ui.label("4. The factory runs automatically. Deliver 8 iron ingots to complete the first order.");
                ui.add_space(9.0);
                ui.label(RichText::new("GO FURTHER").color(COPPER).monospace().strong());
                ui.label("Mine copper on the lower deposits. Assemblers turn two iron ingots into a gear, or one iron and one copper ingot into a circuit. Click an assembler to change recipes. Splitters divide a line across two directions.");
                ui.add_space(9.0);
                ui.label(RichText::new("CONTROLS").color(COPPER).monospace().strong());
                ui.label("1–5 tools  ·  I inspect  ·  R rotate placement  ·  right click recover  ·  Space pause  ·  F1 guide  ·  Esc menu");
                ui.label("Select an inventory item, then click a compatible machine to load it by hand. Completed output can be picked up from the inspector.");
                ui.add_space(8.0);
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
