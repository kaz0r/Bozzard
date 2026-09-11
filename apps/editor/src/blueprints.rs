//! An egui Scene supplies native pan/zoom; only graph nodes and typed wires are custom.
use super::*;
use bozzard_scene::{
    Blueprint, BlueprintAttachment,
    blueprint::{InputKey, Node, NodeKind, ObjectRef, PinType, Socket, Value, Wire},
};

pub struct BlueprintPane {
    target: Option<(PathBuf, String)>,
    pub index: usize,
    selected: Option<u32>,
    connecting: Option<Socket>,
    view: Rect,
    search: String,
    variable: String,
}
impl Default for BlueprintPane {
    fn default() -> Self {
        Self {
            target: None,
            index: 0,
            selected: None,
            connecting: None,
            view: Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 560.)),
            search: String::new(),
            variable: String::new(),
        }
    }
}
impl BlueprintPane {
    fn sync(&mut self, path: &Path, id: &str, attachments: &[BlueprintAttachment]) {
        let target = (path.to_owned(), id.to_owned());
        if self.target.as_ref() != Some(&target) {
            *self = Self {
                target: Some(target),
                ..Self::default()
            };
            if let Some(attachment) = attachments.first() {
                self.fit(&attachment.graph);
            }
        }
    }
    fn fit(&mut self, graph: &Blueprint) {
        self.view = graph
            .nodes
            .iter()
            .map(node_rect)
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::splat(500.)))
            .expand(40.);
    }
    fn choose(&mut self, index: usize, graph: &Blueprint) {
        self.index = index;
        self.selected = None;
        self.connecting = None;
        self.fit(graph);
    }
}
impl App {
    pub fn blueprint_inspector(&mut self, ui: &mut egui::Ui, object: &mut bozzard_scene::Object) {
        if self.editor.selected_surface().is_some() {
            return;
        }
        self.blueprint_pane
            .sync(&self.editor.path, &object.id, &object.blueprints);
        let objects: Vec<_> = self
            .editor
            .scene()
            .objects
            .iter()
            .map(|o| (o.id.clone(), o.name.clone()))
            .collect();
        egui::CollapsingHeader::new("BLUEPRINTS")
            .id_salt((&object.id, "blueprints"))
            .default_open(!object.blueprints.is_empty())
            .show(ui, |ui| {
                let mut attachments = object.blueprints.clone();
                let editing =
                    self.editor.play.is_none() && self.loading.is_none() && self.dialog.is_none();
                let mut remove = None;
                let mut move_to = None;
                ui.add_enabled_ui(editing, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (title, graph) in [
                            ("+ New", Blueprint::default()),
                            ("+ Spin example", Blueprint::spinning()),
                        ] {
                            if ui.button(title).clicked() {
                                self.blueprint_pane.choose(attachments.len(), &graph);
                                attachments.push(BlueprintAttachment {
                                    enabled: true,
                                    graph,
                                });
                                self.workspace.blueprints_visible = true;
                            }
                        }
                        if ui.button("Load…").clicked() {
                            self.blueprint_dialog(files::Kind::LoadBlueprint);
                        }
                    });
                });
                for (i, attachment) in attachments.iter_mut().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_enabled(
                            editing,
                            egui::Checkbox::without_text(&mut attachment.enabled),
                        )
                        .on_hover_text("Run this blueprint during Play");
                        if ui
                            .selectable_label(
                                self.workspace.blueprints_visible && self.blueprint_pane.index == i,
                                format!("{}. {}", i + 1, attachment.graph.name),
                            )
                            .clicked()
                        {
                            self.blueprint_pane.choose(i, &attachment.graph);
                            self.workspace.blueprints_visible = true;
                        }
                        if ui
                            .add_enabled(editing && i > 0, egui::Button::new("↑").small())
                            .on_hover_text("Run earlier")
                            .clicked()
                        {
                            move_to = Some((i, i - 1));
                        }
                        if ui
                            .add_enabled(editing, egui::Button::new("×").small())
                            .on_hover_text("Detach blueprint (Undo restores it)")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                    ui.push_id((i, "bindings"), |ui| {
                        ui.add_enabled_ui(editing, |ui| {
                            for node in &mut attachment.graph.nodes {
                                for (port, value) in node.inputs.iter_mut().enumerate() {
                                    if attachment.graph.wires.iter().any(|w| {
                                        w.to == (Socket {
                                            node: node.id,
                                            port,
                                        })
                                    }) {
                                        continue;
                                    }
                                    if let Value::Object(reference) = value {
                                        ui.push_id((node.id, port), |ui| {
                                            ui.label(format!(
                                                "{} · {}",
                                                node.kind.title(),
                                                node.kind.inputs()[port].0
                                            ));
                                            object_picker(ui, reference, &objects);
                                        });
                                    }
                                }
                            }
                        });
                    });
                }
                if let Some(i) = remove {
                    attachments.remove(i);
                    self.blueprint_pane.connecting = None;
                }
                if let Some((a, b)) = move_to {
                    attachments.swap(a, b);
                    self.blueprint_pane.index = b;
                    self.blueprint_pane.connecting = None;
                }
                object.blueprints = attachments;
                ui.weak("Top to bottom · Independent state · Targets default to Self");
            });
    }
    pub fn open_last_blueprint(&mut self) {
        if let Some(object) = self.editor.selected_object()
            && let Some(attachment) = object.blueprints.last()
        {
            self.blueprint_pane
                .sync(&self.editor.path, &object.id, &object.blueprints);
            self.blueprint_pane
                .choose(object.blueprints.len() - 1, &attachment.graph);
            self.workspace.blueprints_visible = true;
        }
    }
    pub fn blueprint_dialog(&mut self, kind: files::Kind) {
        let Some(object) = self.editor.selected_object() else {
            return;
        };
        let mut dialog = files::Dialog::new(
            kind,
            &bozzard_editor::root(&self.editor.path).join("behavior.blueprint.json"),
        );
        dialog.blueprint_target = Some((
            self.editor.path.clone(),
            self.editor.revision(),
            object.id.clone(),
            self.blueprint_pane.index,
        ));
        self.dialog = Some(dialog);
    }
    pub fn blueprint_ui(&mut self, ui: &mut egui::Ui) {
        self.viewport_rect = None;
        self.mouse_captured = false;
        self.fly_latched = false;
        self.navigation_button = None;
        self.gameplay_controls.reset();
        if let Some(play) = &mut self.editor.play {
            play.clear_gameplay_input();
        }
        let Some(object) = self.editor.selected_object().cloned() else {
            ui.weak(
                "Select an object or prefab member, then add or load a Blueprint in Properties.",
            );
            return;
        };
        if self.editor.selected_surface().is_some() {
            ui.weak("Select the mesh owner in Hierarchy to edit its Blueprints (or Alt-click the model).");
            return;
        }
        self.blueprint_pane
            .sync(&self.editor.path, &object.id, &object.blueprints);
        if object.blueprints.is_empty() {
            ui.heading("Blueprint Editor");
            ui.label("No coding required. Add a graph, connect event → action, then Play.");
            ui.add_enabled_ui(
                self.editor.play.is_none() && self.loading.is_none() && self.dialog.is_none(),
                |ui| {
                    ui.horizontal(|ui| {
                        for (title, graph) in [
                            ("New Blueprint", Blueprint::default()),
                            ("Spin example", Blueprint::spinning()),
                        ] {
                            if ui.button(title).clicked() {
                                self.blueprint_pane.choose(0, &graph);
                                let result = self.editor.set_blueprints(
                                    &object.id,
                                    vec![BlueprintAttachment {
                                        enabled: true,
                                        graph,
                                    }],
                                );
                                self.result(result);
                            }
                        }
                        if ui.button("Load Blueprint…").clicked() {
                            self.blueprint_dialog(files::Kind::LoadBlueprint);
                        }
                    });
                },
            );
            return;
        }
        self.blueprint_pane.index = self.blueprint_pane.index.min(object.blueprints.len() - 1);
        let editing = self.editor.play.is_none()
            && self.loading.is_none()
            && self.dialog.is_none()
            && !self.confirm_discard;
        let index = self.blueprint_pane.index;
        let mut graph = object.blueprints[index].graph.clone();
        ui.horizontal_wrapped(|ui| {
            ui.strong("Blueprint Editor");
            ui.label(&object.name);
            egui::ComboBox::from_id_salt("active-blueprint")
                .selected_text(&graph.name)
                .show_ui(ui, |ui| {
                    for (i, a) in object.blueprints.iter().enumerate() {
                        if ui.selectable_label(i == index, &a.graph.name).clicked() {
                            self.blueprint_pane.choose(i, &a.graph);
                        }
                    }
                });
            if ui.button("Fit graph").clicked() {
                self.blueprint_pane.fit(&graph);
            }
            ui.add_enabled_ui(editing, |ui| {
                if ui.button("Load copy…").clicked() {
                    self.blueprint_dialog(files::Kind::LoadBlueprint);
                }
                if ui.button("Save graph…").clicked() {
                    self.blueprint_dialog(files::Kind::SaveBlueprint);
                }
            });
        });
        if self.blueprint_pane.index != index {
            return;
        }
        ui.add_enabled_ui(editing, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut graph.name).desired_width(170.));
                ui.menu_button("+ Add node", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.blueprint_pane.search)
                            .hint_text("Search nodes…"),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(340.)
                        .show(ui, |ui| {
                            for kind in NodeKind::ALL {
                                if kind
                                    .title()
                                    .to_lowercase()
                                    .contains(&self.blueprint_pane.search.to_lowercase())
                                    && ui.button(kind.title()).clicked()
                                {
                                    let id = graph
                                        .nodes
                                        .iter()
                                        .map(|n| n.id)
                                        .max()
                                        .unwrap_or(0)
                                        .saturating_add(1);
                                    let mut node = Node::new(
                                        id,
                                        kind,
                                        self.blueprint_pane.view.center().into(),
                                    );
                                    if matches!(kind, NodeKind::GetVariable | NodeKind::SetVariable)
                                    {
                                        graph.variables.entry("value".into()).or_insert(0.);
                                        node.variable = "value".into();
                                    }
                                    graph.nodes.push(node);
                                    self.blueprint_pane.selected = Some(id);
                                    ui.close();
                                }
                            }
                        });
                });
                ui.menu_button("Variables", |ui| {
                    ui.weak("Number variables · Reset to defaults on each Play");
                    let mut remove = None;
                    for (name, value) in &mut graph.variables {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            ui.add(egui::DragValue::new(value).speed(0.1));
                            if ui
                                .add_enabled(
                                    !graph.nodes.iter().any(|n| {
                                        matches!(
                                            n.kind,
                                            NodeKind::GetVariable | NodeKind::SetVariable
                                        ) && n.variable == *name
                                    }),
                                    egui::Button::new("×"),
                                )
                                .clicked()
                            {
                                remove = Some(name.clone());
                            }
                        });
                    }
                    if let Some(name) = remove {
                        graph.variables.remove(&name);
                    }
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.blueprint_pane.variable)
                                .hint_text("New variable")
                                .desired_width(120.),
                        );
                        if ui.button("Add").clicked() {
                            graph
                                .variables
                                .entry(self.blueprint_pane.variable.trim().to_owned())
                                .or_insert(0.);
                            self.blueprint_pane.variable.clear();
                        }
                    });
                });
                if ui
                    .add_enabled(
                        self.blueprint_pane.selected.is_some(),
                        egui::Button::new("Delete node"),
                    )
                    .clicked()
                {
                    if let Some(id) = self.blueprint_pane.selected.take() {
                        graph.remove_node(id);
                    }
                    self.blueprint_pane.connecting = None;
                }
            });
        });
        ui.small("Drag headers to move · Output → input to connect · Right-click input to disconnect · Middle-drag / scroll to pan · Ctrl+scroll to zoom");
        if let Some(play) = &self.editor.play {
            if let Err(error) = play.check_simulation() {
                ui.colored_label(Color32::LIGHT_RED, format!("{error:#}"));
            } else {
                ui.colored_label(
                    theme::GREEN,
                    "Running · Switch to Scene for WASD / Space input · Stop to edit",
                );
            }
            if let Some(runtime) = play.app.world.resource::<bozzard_scene::BlueprintRuntime>()
                && let Some(message) = runtime.messages.back()
            {
                ui.label(message);
            }
        }
        if editing && !ui.ctx().egui_wants_keyboard_input() {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.blueprint_pane.connecting = None;
                self.blueprint_pane.selected = None;
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
                if let Some(id) = self.blueprint_pane.selected.take() {
                    graph.remove_node(id);
                }
                self.blueprint_pane.connecting = None;
            }
        }
        let objects: Vec<_> = self
            .editor
            .scene()
            .objects
            .iter()
            .map(|o| (o.id.clone(), o.name.clone()))
            .collect();
        let error = self
            .blueprint_pane
            .canvas(ui, &mut graph, editing, &objects);
        if let Some(error) = error {
            self.result(Err(error));
        }
        if editing && graph != object.blueprints[index].graph {
            let mut attachments = object.blueprints;
            attachments[index].graph = graph;
            self.editor.begin_gesture("Edit blueprint graph");
            let result = self.editor.set_blueprints(&object.id, attachments);
            self.result(result);
        }
    }
}
fn object_picker(ui: &mut egui::Ui, reference: &mut ObjectRef, objects: &[(String, String)]) {
    let label = match reference {
        ObjectRef::SelfObject => "Self".to_owned(),
        ObjectRef::None => "None".to_owned(),
        ObjectRef::Id(id) => objects.iter().find(|(key, _)| key == id).map_or_else(
            || format!("Missing: {id}"),
            |(_, name)| format!("{name} ({id})"),
        ),
    };
    egui::ComboBox::from_id_salt("object-reference")
        .width(110.)
        .truncate()
        .selected_text(label)
        .show_ui(ui, |ui| {
            ui.selectable_value(reference, ObjectRef::SelfObject, "Self");
            ui.selectable_value(reference, ObjectRef::None, "None");
            let search_id = ui.id().with("object-search");
            let mut search = ui.data_mut(|d| d.get_temp::<String>(search_id).unwrap_or_default());
            ui.add(
                egui::TextEdit::singleline(&mut search)
                    .hint_text("Find object…")
                    .desired_width(180.),
            );
            let query = search.to_lowercase();
            ui.data_mut(|d| d.insert_temp(search_id, search));
            for (id, name) in objects.iter().filter(|(id, name)| {
                query.is_empty()
                    || id.to_lowercase().contains(&query)
                    || name.to_lowercase().contains(&query)
            }) {
                ui.selectable_value(
                    reference,
                    ObjectRef::Id(id.clone()),
                    format!("{name} ({id})"),
                );
            }
        });
}
const WIDTH: f32 = 260.;
fn node_rect(node: &Node) -> Rect {
    Rect::from_min_size(
        Pos2::from(node.position),
        Vec2::new(
            WIDTH,
            70. + 28. * node.kind.inputs().len().max(node.kind.outputs().len()) as f32,
        ),
    )
}
fn pin(node: &Node, port: usize, output: bool) -> Pos2 {
    Pos2::new(
        node.position[0] + if output { WIDTH - 8. } else { 8. },
        node.position[1] + 66. + port as f32 * 28.,
    )
}
fn color(kind: PinType) -> Color32 {
    match kind {
        PinType::Exec => Color32::from_rgb(220, 220, 220),
        PinType::Number => Color32::from_rgb(132, 206, 71),
        PinType::Bool => Color32::from_rgb(210, 73, 91),
        PinType::Vector => Color32::from_rgb(88, 183, 225),
        PinType::Object => Color32::from_rgb(193, 143, 245),
    }
}
fn curve(painter: &egui::Painter, from: Pos2, to: Pos2, tint: Color32) {
    let offset = ((to.x - from.x).abs() * 0.5).max(45.);
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
        [
            from,
            from + Vec2::new(offset, 0.),
            to - Vec2::new(offset, 0.),
            to,
        ],
        false,
        Color32::TRANSPARENT,
        egui::Stroke::new(2., tint),
    ));
}
impl BlueprintPane {
    fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        graph: &mut Blueprint,
        editing: bool,
        objects: &[(String, String)],
    ) -> Option<anyhow::Error> {
        let mut error = None;
        let mut view = self.view;
        egui::Scene::new()
            .zoom_range(0.35..=1.5)
            .drag_pan_buttons(egui::DragPanButtons::MIDDLE | egui::DragPanButtons::SECONDARY)
            .show(ui, &mut view, |ui| {
                let clip = ui.clip_rect();
                ui.painter()
                    .rect_filled(clip, 0., Color32::from_rgb(25, 27, 30));
                for axis in 0..2 {
                    let start = (clip.min[axis] / 32.).floor() as i32;
                    let end = (clip.max[axis] / 32.).ceil() as i32;
                    for i in start..=end {
                        let mut a = clip.min;
                        let mut b = clip.max;
                        a[axis] = i as f32 * 32.;
                        b[axis] = a[axis];
                        ui.painter().line_segment(
                            [a, b],
                            egui::Stroke::new(
                                1.,
                                Color32::from_gray(if i % 4 == 0 { 43 } else { 33 }),
                            ),
                        );
                    }
                }
                for wire in &graph.wires {
                    if let (Ok(from), Ok(to)) =
                        (graph.node(wire.from.node), graph.node(wire.to.node))
                    {
                        curve(
                            ui.painter(),
                            pin(from, wire.from.port, true),
                            pin(to, wire.to.port, false),
                            color(from.kind.outputs()[wire.from.port].1),
                        );
                    }
                }
                let mut connection = None;
                let mut disconnect = None;
                for node in &mut graph.nodes {
                    let rect = node_rect(node);
                    ui.expand_to_include_rect(rect);
                    let selected = self.selected == Some(node.id);
                    ui.painter()
                        .rect_filled(rect, 5., Color32::from_rgb(39, 43, 48));
                    ui.painter().rect_stroke(
                        rect,
                        5.,
                        egui::Stroke::new(
                            if selected { 2. } else { 1. },
                            if selected {
                                theme::ACCENT
                            } else {
                                Color32::from_gray(65)
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    let header = Rect::from_min_size(rect.min, Vec2::new(WIDTH, 30.));
                    ui.painter().rect_filled(
                        header,
                        4.,
                        if node.kind.event() {
                            Color32::from_rgb(109, 48, 57)
                        } else if node.kind.action() {
                            Color32::from_rgb(103, 96, 41)
                        } else {
                            Color32::from_rgb(38, 76, 100)
                        },
                    );
                    ui.painter().text(
                        header.left_center() + Vec2::new(10., 0.),
                        egui::Align2::LEFT_CENTER,
                        node.kind.title(),
                        egui::FontId::proportional(14.),
                        Color32::WHITE,
                    );
                    let response = ui.interact(
                        header,
                        ui.id().with((node.id, "header")),
                        if editing {
                            Sense::click_and_drag()
                        } else {
                            Sense::click()
                        },
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            node.kind.title(),
                        )
                    });
                    if response.clicked() || response.drag_started() {
                        self.selected = Some(node.id);
                    }
                    if editing && response.dragged_by(egui::PointerButton::Primary) {
                        node.position = (Pos2::from(node.position) + response.drag_delta()).into();
                    }
                    ui.push_id(node.id, |ui| {
                        ui.add_enabled_ui(editing, |ui| {
                            let config = Rect::from_min_size(
                                rect.min + Vec2::new(12., 33.),
                                Vec2::new(WIDTH - 24., 24.),
                            );
                            ui.scope_builder(egui::UiBuilder::new().max_rect(config), |ui| {
                                if matches!(
                                    node.kind,
                                    NodeKind::GetVariable | NodeKind::SetVariable
                                ) {
                                    egui::ComboBox::from_id_salt("variable")
                                        .selected_text(&node.variable)
                                        .show_ui(ui, |ui| {
                                            for name in graph.variables.keys() {
                                                ui.selectable_value(
                                                    &mut node.variable,
                                                    name.clone(),
                                                    name,
                                                );
                                            }
                                        });
                                } else if matches!(
                                    node.kind,
                                    NodeKind::InputPressed | NodeKind::InputHeld
                                ) {
                                    egui::ComboBox::from_id_salt("key")
                                        .selected_text(format!("{:?}", node.key))
                                        .show_ui(ui, |ui| {
                                            for key in InputKey::ALL {
                                                ui.selectable_value(
                                                    &mut node.key,
                                                    key,
                                                    format!("{key:?}"),
                                                );
                                            }
                                        });
                                }
                            });
                            for (port, (label, kind)) in node.kind.inputs().iter().enumerate() {
                                let p = pin(node, port, false);
                                let socket = Socket {
                                    node: node.id,
                                    port,
                                };
                                let linked = graph.wires.iter().any(|w| w.to == socket);
                                ui.painter().circle(
                                    p,
                                    5.,
                                    if linked {
                                        color(*kind)
                                    } else {
                                        Color32::from_gray(25)
                                    },
                                    egui::Stroke::new(1.5, color(*kind)),
                                );
                                ui.painter().text(
                                    p + Vec2::new(12., 0.),
                                    egui::Align2::LEFT_CENTER,
                                    label,
                                    egui::FontId::proportional(11.),
                                    Color32::LIGHT_GRAY,
                                );
                                let hit = ui.interact(
                                    Rect::from_center_size(p, Vec2::splat(18.)),
                                    ui.id().with((port, "in")),
                                    Sense::click(),
                                );
                                hit.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        editing,
                                        format!("{} input {label}: {kind:?}", node.kind.title()),
                                    )
                                });
                                if hit.has_focus() {
                                    ui.painter().circle_stroke(
                                        p,
                                        9.,
                                        egui::Stroke::new(1., theme::ACCENT),
                                    );
                                }
                                if hit.secondary_clicked() {
                                    disconnect = Some(socket);
                                }
                                if (hit.clicked()
                                    || (hit.contains_pointer()
                                        && ui.input(|i| i.pointer.primary_released())))
                                    && let Some(from) = self.connecting.take()
                                {
                                    connection = Some(Wire { from, to: socket });
                                }
                                if !linked && *kind != PinType::Exec {
                                    let input_rect = Rect::from_min_size(
                                        p + Vec2::new(70., -10.),
                                        Vec2::new(120., 22.),
                                    );
                                    ui.scope_builder(
                                        egui::UiBuilder::new()
                                            .id_salt((port, "value"))
                                            .max_rect(input_rect),
                                        |ui| match &mut node.inputs[port] {
                                            Value::Number(v) => {
                                                ui.add_sized(
                                                    [100., 20.],
                                                    egui::DragValue::new(v).speed(0.1),
                                                );
                                            }
                                            Value::Bool(v) => {
                                                ui.checkbox(v, "");
                                            }
                                            Value::Vector(v) => {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 1.;
                                                    for value in v {
                                                        ui.add_sized(
                                                            [36., 20.],
                                                            egui::DragValue::new(value).speed(0.1),
                                                        );
                                                    }
                                                });
                                            }
                                            Value::Object(v) => {
                                                object_picker(ui, v, objects);
                                            }
                                            Value::Exec => {}
                                        },
                                    );
                                }
                            }
                            for (port, (label, kind)) in node.kind.outputs().iter().enumerate() {
                                let p = pin(node, port, true);
                                ui.painter().circle_filled(p, 5., color(*kind));
                                ui.painter().text(
                                    p - Vec2::new(12., 0.),
                                    egui::Align2::RIGHT_CENTER,
                                    label,
                                    egui::FontId::proportional(11.),
                                    Color32::LIGHT_GRAY,
                                );
                                let hit = ui.interact(
                                    Rect::from_center_size(p, Vec2::splat(18.)),
                                    ui.id().with((port, "out")),
                                    Sense::click_and_drag(),
                                );
                                hit.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        editing,
                                        format!("{} output {label}: {kind:?}", node.kind.title()),
                                    )
                                });
                                if hit.has_focus() {
                                    ui.painter().circle_stroke(
                                        p,
                                        9.,
                                        egui::Stroke::new(1., theme::ACCENT),
                                    );
                                }
                                if hit.clicked() || hit.drag_started() {
                                    self.connecting = Some(Socket {
                                        node: node.id,
                                        port,
                                    });
                                }
                            }
                        });
                    });
                }
                if let Some(socket) = self.connecting {
                    if let Ok(node) = graph.node(socket.node) {
                        if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                            let p = ui
                                .ctx()
                                .layer_transform_from_global(ui.layer_id())
                                .map_or(pointer, |t| t * pointer);
                            curve(
                                ui.painter(),
                                pin(node, socket.port, true),
                                p,
                                color(node.kind.outputs()[socket.port].1),
                            );
                        }
                    } else {
                        self.connecting = None;
                    }
                }
                if editing {
                    if let Some(socket) = disconnect {
                        graph.wires.retain(|w| w.to != socket);
                    }
                    if let Some(wire) = connection {
                        error = graph.connect(wire).err();
                    }
                }
            });
        self.view = view;
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        pane: &mut BlueprintPane,
        graph: &mut Blueprint,
        events: Vec<egui::Event>,
        editing: bool,
    ) -> egui::emath::TSTransform {
        let mut transform = egui::emath::TSTransform::IDENTITY;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 650.))),
                focused: true,
                events,
                ..Default::default()
            },
            |ui| {
                pane.canvas(ui, graph, editing, &[]);
                transform = ui
                    .ctx()
                    .layer_transform_to_global(egui::LayerId::new(
                        ui.layer_id().order,
                        ui.id().with("scene_area"),
                    ))
                    .unwrap();
            },
        );
        output.textures_delta.clear();
        transform
    }
    fn pointer(pos: Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn canvas_connects_other_object_to_action_target() {
        let ctx = egui::Context::default();
        let mut pane = BlueprintPane::default();
        let mut graph = Blueprint {
            nodes: vec![
                Node::new(1, NodeKind::BodyEnter, [30., 30.]),
                Node::new(2, NodeKind::SetPosition, [380., 30.]),
            ],
            ..Default::default()
        };
        let transform = frame(&ctx, &mut pane, &mut graph, vec![], true);
        let from = transform * pin(graph.node(1).unwrap(), 1, true);
        let to = transform * pin(graph.node(2).unwrap(), 2, false);
        frame(&ctx, &mut pane, &mut graph, pointer(from, true), true);
        frame(&ctx, &mut pane, &mut graph, pointer(from, false), true);
        frame(&ctx, &mut pane, &mut graph, pointer(to, true), true);
        frame(&ctx, &mut pane, &mut graph, pointer(to, false), true);
        assert_eq!(
            graph.wires,
            vec![Wire {
                from: Socket { node: 1, port: 1 },
                to: Socket { node: 2, port: 2 }
            }]
        );
        graph.validate().unwrap();
    }
    #[test]
    fn canvas_connects_typed_pins_moves_nodes_and_protects_play() {
        let ctx = egui::Context::default();
        let mut pane = BlueprintPane::default();
        let mut graph = Blueprint::spinning();
        graph.wires.remove(0);
        let transform = frame(&ctx, &mut pane, &mut graph, vec![], true);
        let from = transform * pin(graph.node(1).unwrap(), 0, true);
        let to = transform * pin(graph.node(4).unwrap(), 0, false);
        frame(&ctx, &mut pane, &mut graph, pointer(from, true), true);
        frame(&ctx, &mut pane, &mut graph, pointer(from, false), true);
        assert_eq!(pane.connecting, Some(Socket { node: 1, port: 0 }));
        frame(&ctx, &mut pane, &mut graph, pointer(to, true), true);
        frame(&ctx, &mut pane, &mut graph, pointer(to, false), true);
        assert_eq!(graph.wires.len(), 3);
        graph.validate().unwrap();
        // A number cannot drive an execution pin, and failed connections preserve the graph.
        let before = graph.clone();
        pane.connecting = Some(Socket { node: 2, port: 0 });
        frame(&ctx, &mut pane, &mut graph, pointer(to, true), true);
        frame(&ctx, &mut pane, &mut graph, pointer(to, false), true);
        assert_eq!(graph, before);
        let header = transform * (Pos2::from(graph.nodes[0].position) + Vec2::new(50., 15.));
        frame(&ctx, &mut pane, &mut graph, pointer(header, true), true);
        frame(
            &ctx,
            &mut pane,
            &mut graph,
            vec![egui::Event::PointerMoved(header + Vec2::new(80., 0.))],
            true,
        );
        frame(
            &ctx,
            &mut pane,
            &mut graph,
            pointer(header + Vec2::new(80., 0.), false),
            true,
        );
        assert!(graph.nodes[0].position[0] > before.nodes[0].position[0] + 40.);
        let before = graph.clone();
        frame(&ctx, &mut pane, &mut graph, pointer(to, true), false);
        frame(
            &ctx,
            &mut pane,
            &mut graph,
            pointer(to + Vec2::new(80., 0.), false),
            false,
        );
        assert_eq!(graph, before);
    }
}
