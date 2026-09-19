//! Bounded editor docking: five resizable tab groups and detachable tool windows.
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pane {
    Scene,
    Hierarchy,
    Inspector,
    Assets,
    Settings,
    Debug,
}
impl Pane {
    const ALL: [Self; 6] = [
        Self::Scene,
        Self::Hierarchy,
        Self::Inspector,
        Self::Assets,
        Self::Settings,
        Self::Debug,
    ];
    fn title(self) -> &'static str {
        match self {
            Self::Scene => "Editor",
            Self::Hierarchy => "Hierarchy",
            Self::Inspector => "Inspector",
            Self::Assets => "Content",
            Self::Settings => "Settings",
            Self::Debug => "Debug",
        }
    }
    fn home(self) -> Dock {
        match self {
            Self::Scene => Dock::Center,
            Self::Hierarchy => Dock::Left,
            Self::Inspector => Dock::LeftLower,
            Self::Assets | Self::Debug => Dock::Bottom,
            Self::Settings => Dock::Right,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dock {
    Left,
    LeftLower,
    Right,
    Bottom,
    Center,
    Floating,
}
impl Dock {
    const ALL: [Self; 6] = [
        Self::Left,
        Self::LeftLower,
        Self::Right,
        Self::Bottom,
        Self::Center,
        Self::Floating,
    ];
    fn title(self) -> &'static str {
        match self {
            Self::Left => "Left top",
            Self::LeftLower => "Left bottom",
            Self::Right => "Right",
            Self::Bottom => "Bottom",
            Self::Center => "Center",
            Self::Floating => "Floating window",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    // One slot per pane prevents duplicate or lost panels after layout restoration.
    locations: [Dock; 6],
    selected: [Option<Pane>; 5],
    generation: u64,
    #[serde(skip)]
    tab_rects: [Option<egui::Rect>; 6],
    #[serde(skip)]
    tab_layers: [Option<egui::LayerId>; 6],
    #[serde(skip)]
    drag_candidate: Option<(Pane, egui::Pos2)>,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            locations: Pane::ALL.map(Pane::home),
            selected: [
                Some(Pane::Hierarchy),
                Some(Pane::Inspector),
                Some(Pane::Settings),
                Some(Pane::Assets),
                Some(Pane::Scene),
            ],
            generation: 0,
            tab_rects: [None; 6],
            tab_layers: [None; 6],
            drag_candidate: None,
        }
    }
}
impl Layout {
    pub fn reset(&mut self) {
        *self = Self {
            generation: self.generation.wrapping_add(1),
            ..Default::default()
        };
    }
    pub fn focus(&mut self, pane: Pane) {
        let dock = self.locations[pane as usize];
        if dock != Dock::Floating {
            self.selected[dock as usize] = Some(pane);
        }
    }
    fn move_pane(&mut self, pane: Pane, to: Dock) {
        self.locations[pane as usize] = to;
        self.focus(pane);
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        visible: [bool; 6],
        mut draw: impl FnMut(Pane, &mut egui::Ui),
    ) {
        let ctx = ui.ctx().clone();
        self.prepare_drag(&ctx);
        self.tab_rects.fill(None);
        self.tab_layers.fill(None);
        let dragging = egui::DragAndDrop::has_payload_of_type::<Pane>(&ctx);
        let occupied = |dock| {
            Pane::ALL
                .into_iter()
                .any(|p| visible[p as usize] && self.locations[p as usize] == dock)
        };
        let left = occupied(Dock::Left);
        let lower = occupied(Dock::LeftLower);
        let right = occupied(Dock::Right);
        let bottom = occupied(Dock::Bottom);
        let mut action = None;
        let generation = self.generation;
        if left || lower || dragging {
            egui::Panel::left(egui::Id::new(("dock-left", generation)))
                .default_size(290.)
                .min_size(180.)
                .max_size(600.)
                .resizable(true)
                .show(ui, |ui| {
                    if (left && lower) || dragging {
                        egui::Panel::top(egui::Id::new(("dock-left-split", generation)))
                            .default_size(280.)
                            .min_size(100.)
                            .resizable(true)
                            .show(ui, |ui| {
                                self.group(ui, Dock::Left, visible, &mut action, &mut draw);
                            });
                        egui::CentralPanel::default().show(ui, |ui| {
                            self.group(ui, Dock::LeftLower, visible, &mut action, &mut draw)
                        });
                    } else {
                        self.group(
                            ui,
                            if left { Dock::Left } else { Dock::LeftLower },
                            visible,
                            &mut action,
                            &mut draw,
                        );
                    }
                });
        }
        if bottom || dragging {
            egui::Panel::bottom(egui::Id::new(("dock-bottom", generation)))
                .default_size(220.)
                .min_size(120.)
                .max_size(650.)
                .resizable(true)
                .show(ui, |ui| {
                    self.group(ui, Dock::Bottom, visible, &mut action, &mut draw);
                });
        }
        if right || dragging {
            egui::Panel::right(egui::Id::new(("dock-right", generation)))
                .default_size(280.)
                .min_size(180.)
                .max_size(600.)
                .resizable(true)
                .show(ui, |ui| {
                    self.group(ui, Dock::Right, visible, &mut action, &mut draw);
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(20, 20, 22))
                    .inner_margin(4),
            )
            .show(ui, |ui| {
                self.group(ui, Dock::Center, visible, &mut action, &mut draw);
            });
        for pane in Pane::ALL {
            if visible[pane as usize] && self.locations[pane as usize] == Dock::Floating {
                let mut open = true;
                egui::Window::new(pane.title())
                    .id(egui::Id::new(("floating-pane", pane, generation)))
                    .open(&mut open)
                    .default_size([480., 420.])
                    .default_pos([330., 130.])
                    .show(&ctx, |ui| {
                        ui.menu_button("Dock", |ui| {
                            Self::menu(ui, pane, Dock::Floating, &mut action)
                        });
                        ui.separator();
                        draw(pane, ui);
                    });
                if !open {
                    action = Some((pane, pane.home()));
                }
            }
        }
        if let Some((pane, dock)) = action {
            self.move_pane(pane, dock);
        }
        if ctx.input(|i| i.pointer.any_released() || i.key_pressed(egui::Key::Escape)) {
            self.drag_candidate = None;
        }
    }
    fn prepare_drag(&mut self, ctx: &egui::Context) {
        // Native integrations can deliver the final motion and release in one frame,
        // before egui emits `drag_started`. Retain the tab press and recognize that
        // coalesced gesture before drawing any destination, including earlier panels.
        let (press, point, escape) = ctx.input(|input| {
            (
                input.events.iter().rev().find_map(|event| match event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } => Some(*pos),
                    _ => None,
                }),
                input.pointer.interact_pos(),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if let Some(pos) = press {
            self.drag_candidate = Pane::ALL
                .into_iter()
                .find(|p| {
                    self.tab_rects[*p as usize].is_some_and(|r| r.contains(pos))
                        && self.tab_layers[*p as usize] == ctx.layer_id_at(pos)
                })
                .map(|p| (p, pos));
        }
        if escape {
            self.drag_candidate = None;
        }
        if let Some((pane, start)) = self.drag_candidate
            && point.is_some_and(|p| p.distance_sq(start) > 36.)
        {
            egui::DragAndDrop::set_payload(ctx, pane);
        }
    }
    fn menu(ui: &mut egui::Ui, pane: Pane, location: Dock, action: &mut Option<(Pane, Dock)>) {
        for target in Dock::ALL {
            if ui
                .add_enabled(target != location, egui::Button::new(target.title()))
                .clicked()
            {
                *action = Some((pane, target));
                ui.close();
            }
        }
    }
    fn group(
        &mut self,
        ui: &mut egui::Ui,
        dock: Dock,
        visible: [bool; 6],
        action: &mut Option<(Pane, Dock)>,
        draw: &mut impl FnMut(Pane, &mut egui::Ui),
    ) {
        // Six panes are a fixed upper bound: stack storage, no per-frame panel lists.
        let present = Pane::ALL.map(|p| visible[p as usize] && self.locations[p as usize] == dock);
        let active = &mut self.selected[dock as usize];
        if active.is_none_or(|p| !present[p as usize]) {
            *active = Pane::ALL.into_iter().find(|p| present[*p as usize]);
        }
        ui.set_min_size(ui.available_size());
        let header = ui
            .horizontal_wrapped(|ui| {
                for pane in Pane::ALL {
                    if !present[pane as usize] {
                        continue;
                    }
                    let tab = ui
                        .push_id(("dock-tab", pane), |ui| {
                            ui.add(
                                egui::Button::selectable(*active == Some(pane), pane.title())
                                    .sense(egui::Sense::click_and_drag()),
                            )
                        })
                        .inner;
                    tab.dnd_set_drag_payload(pane);
                    self.tab_rects[pane as usize] = Some(tab.rect);
                    self.tab_layers[pane as usize] = Some(ui.layer_id());
                    if tab.clicked() {
                        *active = Some(pane);
                    }
                    tab.context_menu(|ui| Self::menu(ui, pane, dock, action));
                }
                if let Some(pane) = *active {
                    ui.menu_button("Dock", |ui| Self::menu(ui, pane, dock, action));
                } else {
                    ui.weak(format!("Drop a panel · {}", dock.title()));
                }
            })
            .response;
        // Empty destinations can appear during this drag. Their new egui response
        // has no prior-frame hit-test entry yet; use the current header geometry.
        let over = ui.ctx().pointer_interact_pos().is_some_and(|p| {
            header.rect.intersect(ui.clip_rect()).contains(p)
                && ui.ctx().layer_id_at(p) == Some(ui.layer_id())
        });
        if over && egui::DragAndDrop::has_payload_of_type::<Pane>(ui.ctx()) {
            ui.painter().rect_filled(
                header.rect,
                2.,
                Color32::from_rgba_unmultiplied(80, 140, 210, 50),
            );
            if ui.input(|i| i.pointer.any_released())
                && let Some(pane) = egui::DragAndDrop::take_payload::<Pane>(ui.ctx())
            {
                *action = Some((*pane, dock));
            }
        }
        ui.separator();
        if let Some(pane) = *active {
            ui.push_id(("pane-content", pane), |ui| draw(pane, ui));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_drag_moves_a_tab_between_groups() {
        let mut layout = Layout::default();
        layout.move_pane(Pane::Inspector, Dock::Right);
        let ctx = egui::Context::default();
        let run = |layout: &mut Layout, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1600., 900.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    layout.show(ui, [true; 6], |pane, ui| {
                        ui.label(pane.title());
                    })
                },
            );
            output.textures_delta.clear();
        };
        run(&mut layout, vec![]);
        let source = layout.tab_rects[Pane::Inspector as usize].unwrap().center();
        let target = layout.tab_rects[Pane::Hierarchy as usize].unwrap().center();
        run(&mut layout, vec![egui::Event::PointerMoved(source)]);
        run(
            &mut layout,
            vec![egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
        );
        run(&mut layout, vec![egui::Event::PointerMoved(target)]);
        run(
            &mut layout,
            vec![egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        assert_eq!(layout.locations[Pane::Inspector as usize], Dock::Left);
        assert_eq!(layout.selected[Dock::Left as usize], Some(Pane::Inspector));
        // A busy native frame may receive press, motion and release together.
        layout.move_pane(Pane::Inspector, Dock::Right);
        run(&mut layout, vec![]);
        let source = layout.tab_rects[Pane::Inspector as usize].unwrap().center();
        let target = layout.tab_rects[Pane::Hierarchy as usize].unwrap().center();
        run(
            &mut layout,
            vec![
                egui::Event::PointerMoved(source),
                egui::Event::PointerButton {
                    pos: source,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerMoved(target),
                egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
        );
        assert_eq!(layout.locations[Pane::Inspector as usize], Dock::Left);
    }
    #[test]
    fn moving_detaching_restoring_and_resetting_never_duplicate_or_lose_panes() {
        let mut layout = Layout::default();
        layout.move_pane(Pane::Inspector, Dock::Right);
        layout.move_pane(Pane::Assets, Dock::Floating);
        let encoded = serde_json::to_string(&layout).unwrap();
        let mut restored: Layout = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.locations[Pane::Inspector as usize], Dock::Right);
        assert_eq!(restored.locations[Pane::Assets as usize], Dock::Floating);
        let ctx = egui::Context::default();
        let mut counts = [0; 6];
        let mut output = ctx.run_ui(Default::default(), |ui| {
            restored.show(ui, [true; 6], |p, ui| {
                counts[p as usize] += 1;
                ui.label(p.title());
            })
        });
        output.textures_delta.clear();
        assert!(counts.iter().all(|n| *n <= 1));
        assert_eq!(counts[Pane::Assets as usize], 1);
        assert_eq!(counts[Pane::Inspector as usize], 1);
        restored.reset();
        assert_eq!(restored.locations, Layout::default().locations);
        assert_eq!(restored.generation, 1);
        assert_eq!(
            serde_json::from_str::<Layout>("{}").unwrap().locations,
            restored.locations
        );
    }
}
