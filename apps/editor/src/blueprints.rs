//! An egui Scene supplies native pan/zoom; only graph nodes and typed wires are custom.
use super::*;
use bozzard_scene::{
    Blueprint, BlueprintAttachment,
    blueprint::{
        Blackboard, BlackboardValue, InputKey, Node, NodeKind, ObjectRef, PinType, Socket, Value,
        VariableScope, Wire,
    },
};

pub struct BlueprintPane {
    target: Option<(PathBuf, String)>,
    pub index: usize,
    pub(super) selected: Option<u32>,
    pub(super) breakpoints: std::collections::BTreeSet<u32>,
    pub(super) executing: Option<u32>,
    pub(super) recent: std::collections::BTreeSet<u32>,
    selection: std::collections::BTreeSet<u32>,
    clipboard: Option<Blueprint>,
    draft: Option<Blueprint>,
    draft_scene_board: Option<Blackboard>,
    draft_object_board: Option<Blackboard>,
    find: String,
    scene_file: String,
    scene_name: String,
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
            breakpoints: Default::default(),
            executing: None,
            recent: Default::default(),
            selection: Default::default(),
            clipboard: None,
            draft: None,
            draft_scene_board: None,
            draft_object_board: None,
            find: String::new(),
            scene_file: String::new(),
            scene_name: String::new(),
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
        self.selection.clear();
        self.draft = None;
        self.draft_scene_board = None;
        self.draft_object_board = None;
        self.connecting = None;
        self.fit(graph);
    }
}
impl App {
    pub(super) fn focus_diagnostic_node(&mut self, owner: &str, index: usize, node: Option<u32>) {
        let scene = self
            .editor
            .play
            .as_ref()
            .map_or_else(|| self.editor.scene(), |p| p.instance().document());
        let Some(object) = scene.objects.iter().find(|o| o.id == owner) else {
            return;
        };
        let Some(attachment) = object.blueprints.get(index) else {
            return;
        };
        self.blueprint_debug.runtime_owner = Some(owner.into());
        self.blueprint_pane
            .sync(&self.editor.path, owner, &object.blueprints);
        self.blueprint_pane.choose(index, &attachment.graph);
        if let Some(node) = node.and_then(|id| attachment.graph.nodes.iter().find(|n| n.id == id)) {
            self.blueprint_pane.selected = Some(node.id);
            self.blueprint_pane.selection.insert(node.id);
            self.blueprint_pane.view = node_rect(node).expand(180.);
        }
        self.workspace.blueprints_visible = true;
        self.workspace.shaders_visible = false;
    }
    pub fn blueprint_inspector(&mut self, ui: &mut egui::Ui, object: &mut bozzard_scene::Object) {
        if self.editor.selected_surface().is_some() {
            return;
        }
        if self.editor.play.is_some() {
            if !object.blueprints.is_empty() && ui.button("Inspect running Blueprints").clicked() {
                self.focus_diagnostic_node(&object.id, 0, None);
            }
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
                            .add_enabled(editing, egui::Button::new("Remove").small())
                            .on_hover_text("Detach blueprint (Undo restores it)")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                    ui.push_id((i, "bindings"), |ui| {
                        ui.add_enabled_ui(editing, |ui| {
                            for node in &mut attachment.graph.nodes {
                                let pins = node.input_pins();
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
                                                pins[port].0
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
                ui.weak("Top to bottom · Graph / Object / Scene state · Targets default to Self");
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
        let path = bozzard_editor::root(&self.editor.path)
            .join("assets/Blueprints/behavior.blueprint.json");
        if let Err(error) = std::fs::create_dir_all(path.parent().unwrap()) {
            self.result(Err(error.into()));
            return;
        }
        let mut dialog = files::Dialog::new(kind, &path);
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
        self.blueprint_object_picker(ui);
        let object = if let Some(play) = &self.editor.play {
            let owner = self
                .blueprint_debug
                .runtime_owner
                .as_ref()
                .or(self.editor.selected.as_ref());
            play.instance()
                .document()
                .objects
                .iter()
                .find(|o| Some(&o.id) == owner)
                .cloned()
        } else {
            self.editor.selected_object().cloned()
        };
        let Some(object) = object else {
            ui.weak(
                "Select an object or prefab member, then add or load a Blueprint in Properties.",
            );
            return;
        };
        if self.editor.play.is_none() && self.editor.selected_surface().is_some() {
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
        let mut graph = if editing {
            self.blueprint_pane
                .draft
                .take()
                .unwrap_or_else(|| object.blueprints[index].graph.clone())
        } else {
            self.blueprint_pane.draft = None;
            object.blueprints[index].graph.clone()
        };
        let mut scene_board = self
            .blueprint_pane
            .draft_scene_board
            .take()
            .unwrap_or_else(|| self.editor.scene().blackboard.clone());
        let mut object_board = self
            .blueprint_pane
            .draft_object_board
            .take()
            .unwrap_or_else(|| object.blackboard.clone());
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
        self.blueprint_debug_toolbar(ui, &object.id, &graph);
        if self.editor.play.is_none() {
            ui.add_enabled_ui(editing, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut graph.name).desired_width(170.));
                    self.blueprint_pane.add_node_menu(ui, &mut graph);
                    ui.menu_button("Blackboards", |ui| {
                        ui.label("Graph attachment");
                        board_editor(ui, &mut graph.blackboard, &mut self.blueprint_pane.variable);
                        ui.separator();
                        ui.label("Object (shared by attachments)");
                        board_editor(ui, &mut object_board, &mut self.blueprint_pane.variable);
                        ui.separator();
                        ui.label("Scene (shared by objects)");
                        board_editor(ui, &mut scene_board, &mut self.blueprint_pane.variable);
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
                            for id in std::mem::take(&mut self.blueprint_pane.selection) {
                                graph.remove_node(id);
                            }
                        }
                        self.blueprint_pane.connecting = None;
                    }
                });
            });
        }
        if let Some(node) = graph
            .nodes
            .iter_mut()
            .find(|n| Some(n.id) == self.blueprint_pane.selected && n.kind == NodeKind::SpawnPrefab)
        {
            ui.add_enabled_ui(editing, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Spawn Prefab source");
                    egui::ComboBox::from_id_salt("spawn-prefab-source")
                        .selected_text(if node.prefab.is_empty() {
                            "Choose prefab…"
                        } else {
                            &node.prefab
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut node.prefab, String::new(), "None");
                            for (id, source) in &self.editor.scene().assets {
                                if source.kind == AssetKind::Prefab {
                                    ui.selectable_value(&mut node.prefab, id.clone(), id);
                                }
                            }
                        });
                    ui.weak("Instance output → Destroy Prefab Target. Position is world space.");
                });
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.blueprint_pane.find)
                    .hint_text("Find in graph: node, ID, variable, comment")
                    .desired_width(240.),
            );
            if !self.blueprint_pane.find.trim().is_empty() {
                let query = self.blueprint_pane.find.to_lowercase();
                egui::ComboBox::from_id_salt("find-node")
                    .selected_text("Matches…")
                    .show_ui(ui, |ui| {
                        for n in &graph.nodes {
                            if format!("{} {} {} {}", n.id, n.kind.title(), n.variable, n.comment)
                                .to_lowercase()
                                .contains(&query)
                                && ui.button(format!("#{} {}", n.id, n.kind.title())).clicked()
                            {
                                self.blueprint_pane.selected = Some(n.id);
                                self.blueprint_pane.selection =
                                    std::collections::BTreeSet::from([n.id]);
                                self.blueprint_pane.view = node_rect(n).expand(120.);
                                ui.close();
                            }
                        }
                    });
            }
            if ui
                .add_enabled(editing, egui::Button::new("Select all"))
                .clicked()
            {
                self.blueprint_pane.selection = graph.nodes.iter().map(|n| n.id).collect();
                self.blueprint_pane.selected = self.blueprint_pane.selection.first().copied();
            }
            if ui.add_enabled(editing, egui::Button::new("Copy")).clicked() {
                self.blueprint_pane.copy(ui, &graph);
            }
            if ui
                .add_enabled(
                    editing && self.blueprint_pane.clipboard.is_some(),
                    egui::Button::new("Paste"),
                )
                .clicked()
                && let Err(e) = self.blueprint_pane.paste(&mut graph)
            {
                self.result(Err(e));
            }
        });
        if let Some(node) = graph
            .nodes
            .iter_mut()
            .find(|n| Some(n.id) == self.blueprint_pane.selected)
        {
            ui.add_enabled_ui(editing, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("#{} {}", node.id, node.kind.title()));
                    if node.uses_variable() {
                        egui::ComboBox::from_id_salt("variable-scope")
                            .selected_text(format!("{:?}", node.scope))
                            .show_ui(ui, |ui| {
                                for scope in [
                                    VariableScope::Graph,
                                    VariableScope::Object,
                                    VariableScope::Scene,
                                ] {
                                    ui.selectable_value(
                                        &mut node.scope,
                                        scope,
                                        format!("{scope:?}"),
                                    );
                                }
                            });
                        let board = match node.scope {
                            VariableScope::Graph => &graph.blackboard,
                            VariableScope::Object => &object_board,
                            VariableScope::Scene => &scene_board,
                        };
                        let mut choices: Vec<_> = board
                            .iter()
                            .filter(|(_, v)| {
                                matches!(v, BlackboardValue::List { .. }) == node.uses_list()
                            })
                            .map(|(name, v)| (name.clone(), v.kind()))
                            .collect();
                        if node.scope == VariableScope::Graph && !node.uses_list() {
                            choices.extend(
                                graph.variables.keys().map(|n| (n.clone(), PinType::Number)),
                            );
                        }
                        egui::ComboBox::from_id_salt("scoped-variable")
                            .selected_text(&node.variable)
                            .show_ui(ui, |ui| {
                                for (name, kind) in choices {
                                    if ui
                                        .selectable_label(
                                            node.variable == name,
                                            format!("{name} ({kind:?})"),
                                        )
                                        .clicked()
                                    {
                                        node.variable = name;
                                        if node.value_type != kind {
                                            node.value_type = kind;
                                            node.reset_inputs();
                                        }
                                    }
                                }
                            });
                    }
                    if node.kind == NodeKind::Reroute {
                        egui::ComboBox::from_id_salt("reroute-type")
                            .selected_text(format!("{:?}", node.value_type))
                            .show_ui(ui, |ui| {
                                for kind in [
                                    PinType::Exec,
                                    PinType::Number,
                                    PinType::Bool,
                                    PinType::Vector,
                                    PinType::Text,
                                    PinType::Object,
                                ] {
                                    if ui
                                        .selectable_label(
                                            node.value_type == kind,
                                            format!("{kind:?}"),
                                        )
                                        .clicked()
                                    {
                                        node.value_type = kind;
                                        node.reset_inputs();
                                    }
                                }
                            });
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut node.comment)
                            .hint_text("Comment / annotation")
                            .char_limit(4096)
                            .desired_width(260.),
                    );
                });
            });
        }
        ui.menu_button("Runtime scenes", |ui| {
            for name in self.editor.scene().runtime_scenes.keys() {
                ui.label(name);
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.blueprint_pane.scene_name)
                    .hint_text("Scene name"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.blueprint_pane.scene_file)
                    .hint_text("Scene JSON path"),
            );
            if ui
                .add_enabled(editing, egui::Button::new("Import scene into library"))
                .clicked()
            {
                let result = self.editor.import_runtime_scene(
                    &self.blueprint_pane.scene_name,
                    &PathBuf::from(&self.blueprint_pane.scene_file),
                );
                self.result(result);
            }
        });
        if let Err(error) = graph.validate() {
            ui.colored_label(Color32::LIGHT_RED, format!("Draft is invalid: {error:#}"));
            let stale = graph.stale_wires();
            for issue in &stale {
                ui.colored_label(
                    Color32::LIGHT_RED,
                    format!(
                        "− wire {}:{} → {}:{} · {}",
                        issue.wire.from.node,
                        issue.wire.from.port,
                        issue.wire.to.node,
                        issue.wire.to.port,
                        issue.reason
                    ),
                );
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        editing && !stale.is_empty(),
                        egui::Button::new("Remove stale wires"),
                    )
                    .clicked()
                {
                    let bad: std::collections::BTreeSet<_> =
                        stale.iter().map(|i| i.index).collect();
                    graph.wires = graph
                        .wires
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| !bad.contains(i))
                        .map(|(_, w)| *w)
                        .collect();
                }
                if ui.button("Discard draft").clicked() {
                    graph = object.blueprints[index].graph.clone();
                }
            });
        }
        ui.small("Drag headers to move · Output → input to connect · Right-click input to disconnect · Middle-drag / scroll to pan · Ctrl+scroll to zoom");
        if let Some(play) = &self.editor.play {
            if let Err(error) = play.check_simulation() {
                ui.colored_label(Color32::LIGHT_RED, format!("{error:#}"));
            } else {
                ui.colored_label(
                    theme::GREEN,
                    if play.app.is_paused() {
                        "Simulation paused · use Continue, Step node, or Step tick"
                    } else {
                        "Running · Switch to Scene for WASD / Space input · Stop to edit"
                    },
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
                self.blueprint_pane.selection.clear();
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
                if let Some(id) = self.blueprint_pane.selected.take() {
                    graph.remove_node(id);
                    for id in std::mem::take(&mut self.blueprint_pane.selection) {
                        graph.remove_node(id);
                    }
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
        if editing && !ui.ctx().egui_wants_keyboard_input() {
            for event in ui.input(|i| i.events.clone()) {
                match event {
                    egui::Event::Copy => self.blueprint_pane.copy(ui, &graph),
                    egui::Event::Paste(text) => {
                        if let Ok(copied) = Blueprint::from_json(&text) {
                            self.blueprint_pane.clipboard = Some(copied);
                            if let Err(e) = self.blueprint_pane.paste(&mut graph) {
                                self.result(Err(e));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        self.blueprint_debug_inspector(ui, &object.id);
        let error = self
            .blueprint_pane
            .canvas(ui, &mut graph, editing, &objects);
        if let Some(error) = error {
            self.result(Err(error));
        }
        if editing
            && (graph != object.blueprints[index].graph
                || scene_board != self.editor.scene().blackboard
                || object_board != object.blackboard)
        {
            let mut scene = self.editor.scene().clone();
            scene.blackboard = scene_board;
            let owner = scene
                .objects
                .iter_mut()
                .find(|o| o.id == object.id)
                .unwrap();
            owner.blackboard = object_board;
            owner.blueprints[index].graph = graph.clone();
            if let Err(error) = scene.validate() {
                self.blueprint_pane.draft_scene_board = Some(scene.blackboard.clone());
                self.blueprint_pane.draft_object_board = Some(
                    scene
                        .objects
                        .iter()
                        .find(|o| o.id == object.id)
                        .unwrap()
                        .blackboard
                        .clone(),
                );
                self.blueprint_pane.draft = Some(graph);
                ui.colored_label(Color32::LIGHT_RED, format!("Unapplied draft: {error:#}"));
            } else {
                self.editor.begin_gesture("Edit blueprint graph");
                let result = self.editor.apply("Edit blueprint graph", scene);
                self.result(result);
            }
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
            if node.kind == NodeKind::Comment {
                180.
            } else {
                70. + 28. * node.input_pins().len().max(node.output_pins().len()) as f32
            },
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
        PinType::Text => Color32::from_rgb(236, 157, 82),
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
                            from.output_pins()
                                .get(wire.from.port)
                                .map_or(Color32::LIGHT_RED, |p| color(p.1)),
                        );
                    }
                }
                let mut connection = None;
                let mut disconnect = None;
                let mut group_drag = None;
                for node in &mut graph.nodes {
                    let rect = node_rect(node);
                    ui.expand_to_include_rect(rect);
                    let selected =
                        self.selected == Some(node.id) || self.selection.contains(&node.id);
                    ui.painter()
                        .rect_filled(rect, 5., Color32::from_rgb(39, 43, 48));
                    ui.painter().rect_stroke(
                        rect,
                        5.,
                        egui::Stroke::new(
                            if self.executing == Some(node.id) {
                                3.
                            } else if selected {
                                2.
                            } else {
                                1.
                            },
                            if self.executing == Some(node.id) {
                                Color32::LIGHT_YELLOW
                            } else if selected {
                                theme::ACCENT
                            } else if self.recent.contains(&node.id) {
                                theme::GREEN
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
                    if self.breakpoints.contains(&node.id) {
                        ui.painter().circle_filled(
                            header.right_center() - Vec2::new(12., 0.),
                            6.,
                            Color32::LIGHT_RED,
                        );
                    }
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
                        if ui.input(|i| i.modifiers.shift) {
                            if !self.selection.insert(node.id) {
                                self.selection.remove(&node.id);
                                self.selected = self.selection.first().copied();
                            }
                        } else if !self.selection.contains(&node.id) {
                            self.selection = std::collections::BTreeSet::from([node.id]);
                        }
                    }
                    if editing && response.dragged_by(egui::PointerButton::Primary) {
                        node.position = (Pos2::from(node.position) + response.drag_delta()).into();
                        group_drag = Some((node.id, response.drag_delta()));
                    }
                    ui.push_id(node.id, |ui| {
                        ui.add_enabled_ui(editing, |ui| {
                            let config = Rect::from_min_size(
                                rect.min + Vec2::new(12., 33.),
                                Vec2::new(WIDTH - 24., 24.),
                            );
                            ui.scope_builder(egui::UiBuilder::new().max_rect(config), |ui| {
                                if node.uses_variable() {
                                    ui.weak(format!(
                                        "{:?} · {} ({:?})",
                                        node.scope, node.variable, node.value_type
                                    ));
                                } else if matches!(
                                    node.kind,
                                    NodeKind::InputPressed | NodeKind::InputHeld
                                ) {
                                    // Aliases read the engine's axes and edges; every other
                                    // name binds the button itself.
                                    egui::ComboBox::from_id_salt("key")
                                        .selected_text(node.key.name())
                                        .show_ui(ui, |ui| {
                                            for name in InputKey::authorable() {
                                                let key = InputKey::parse(name)
                                                    .expect("authorable names parse");
                                                ui.selectable_value(&mut node.key, key, name);
                                            }
                                        });
                                }
                            });
                            if node.kind == NodeKind::Comment {
                                let painter =
                                    ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                                let galley = painter.layout(
                                    node.comment.clone(),
                                    egui::FontId::proportional(12.),
                                    Color32::LIGHT_YELLOW,
                                    WIDTH - 24.,
                                );
                                painter.galley(
                                    rect.min + Vec2::new(12., 38.),
                                    galley,
                                    Color32::LIGHT_YELLOW,
                                );
                            }
                            for (port, (label, kind)) in node.input_pins().iter().enumerate() {
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
                                            Value::Text(v) => {
                                                ui.add(
                                                    egui::TextEdit::singleline(v)
                                                        .desired_width(110.)
                                                        .char_limit(4096),
                                                );
                                            }
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
                            for (port, (label, kind)) in node.output_pins().iter().enumerate() {
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
                if let Some((dragged, delta)) = group_drag {
                    for node in &mut graph.nodes {
                        if node.id != dragged && self.selection.contains(&node.id) {
                            node.position = (Pos2::from(node.position) + delta).into();
                        }
                    }
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
                                node.output_pins()
                                    .get(socket.port)
                                    .map_or(Color32::LIGHT_RED, |p| color(p.1)),
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

// The catalog is immutable. Sort and normalize it once, rather than on every menu frame.
fn node_menu_catalog() -> &'static [(&'static bozzard_scene::blueprint::NodeSpec, String)] {
    static CATALOG: std::sync::LazyLock<
        Vec<(&'static bozzard_scene::blueprint::NodeSpec, String)>,
    > = std::sync::LazyLock::new(|| {
        let mut nodes: Vec<_> = NodeKind::specs()
            .iter()
            .map(|spec| (spec, spec.title.to_lowercase()))
            .collect();
        nodes.sort_unstable_by(|a, b| a.1.cmp(&b.1));
        nodes
    });
    &CATALOG
}

impl BlueprintPane {
    fn add_node_menu(&mut self, ui: &mut egui::Ui, graph: &mut Blueprint) -> egui::Response {
        egui::containers::menu::MenuButton::new("+ Add node")
            .config(
                egui::containers::menu::MenuConfig::new()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
            )
            .ui(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search nodes…"));
                let query = self.search.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(340.)
                    .show(ui, |ui| {
                        let mut matched = false;
                        for (spec, title) in node_menu_catalog() {
                            if !title.contains(&query) {
                                continue;
                            }
                            matched = true;
                            if ui.button(spec.title).clicked() {
                                let id = graph
                                    .nodes
                                    .iter()
                                    .map(|n| n.id)
                                    .max()
                                    .unwrap_or(0)
                                    .saturating_add(1);
                                let mut node = Node::new(id, spec.kind, self.view.center().into());
                                if matches!(
                                    spec.kind,
                                    NodeKind::GetVariable | NodeKind::SetVariable
                                ) {
                                    graph.variables.entry("value".into()).or_insert(0.);
                                    node.variable = "value".into();
                                }
                                graph.nodes.push(node);
                                self.selected = Some(id);
                                self.selection = std::collections::BTreeSet::from([id]);
                                ui.close();
                            }
                        }
                        if !matched {
                            ui.weak("No matching nodes");
                        }
                    });
            })
            .0
    }
    fn copy(&mut self, ui: &egui::Ui, graph: &Blueprint) {
        let mut selected = self.selection.clone();
        if let Some(id) = self.selected {
            selected.insert(id);
        }
        if let Ok(copy) = graph.copy_subgraph(&selected) {
            if let Ok(text) = copy.to_json() {
                ui.ctx().copy_text(text);
            }
            self.clipboard = Some(copy);
        }
    }
    fn paste(&mut self, graph: &mut Blueprint) -> anyhow::Result<()> {
        if let Some(copy) = &self.clipboard {
            self.selection = graph.paste_subgraph(copy, [40., 40.])?;
            self.selected = self.selection.first().copied();
        }
        Ok(())
    }
}
fn board_editor(ui: &mut egui::Ui, board: &mut Blackboard, name: &mut String) {
    let mut remove = None;
    for (key, value) in board.iter_mut() {
        ui.push_id(key, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(key);
                match value {
                    BlackboardValue::Scalar(v) => edit_scalar(ui, v),
                    BlackboardValue::List {
                        element,
                        capacity,
                        values,
                    } => {
                        ui.label(format!("{:?} list · {} entries", element, values.len()));
                        ui.add(
                            egui::DragValue::new(capacity)
                                .range(values.len().max(1)..=1024)
                                .prefix("Capacity "),
                        );
                        if ui.button("+ element").clicked() && values.len() < *capacity {
                            values.push(element.default_value());
                        }
                        if ui.button("Pop").clicked() {
                            values.pop();
                        }
                        for (i, v) in values.iter_mut().enumerate() {
                            ui.push_id(i, |ui| edit_scalar(ui, v));
                        }
                    }
                }
                if ui.small_button("×").clicked() {
                    remove = Some(key.clone());
                }
            });
        });
    }
    if let Some(key) = remove {
        board.remove(&key);
    }
    ui.horizontal_wrapped(|ui| {
        ui.add(
            egui::TextEdit::singleline(name)
                .hint_text("Variable name")
                .desired_width(130.),
        );
        ui.menu_button("Add scalar", |ui| {
            for kind in PinType::VALUES {
                if ui.button(format!("{kind:?}")).clicked() && !name.trim().is_empty() {
                    board
                        .entry(name.trim().into())
                        .or_insert_with(|| BlackboardValue::Scalar(kind.default_value()));
                    name.clear();
                    ui.close();
                }
            }
        });
        ui.menu_button("Add list", |ui| {
            for kind in PinType::VALUES {
                if ui.button(format!("{kind:?}")).clicked() && !name.trim().is_empty() {
                    board
                        .entry(name.trim().into())
                        .or_insert_with(|| BlackboardValue::List {
                            element: kind,
                            capacity: 256,
                            values: vec![],
                        });
                    name.clear();
                    ui.close();
                }
            }
        });
    });
}
fn edit_scalar(ui: &mut egui::Ui, value: &mut Value) {
    match value {
        Value::Number(v) => {
            ui.add(egui::DragValue::new(v).speed(0.1));
        }
        Value::Bool(v) => {
            ui.checkbox(v, "");
        }
        Value::Vector(v) => {
            for axis in v {
                ui.add(egui::DragValue::new(axis).speed(0.1));
            }
        }
        Value::Text(v) => {
            ui.add(
                egui::TextEdit::singleline(v)
                    .char_limit(4096)
                    .desired_width(120.),
            );
        }
        Value::Object(reference) => {
            object_picker(ui, reference, &[]);
            if let ObjectRef::Id(id) = reference {
                ui.text_edit_singleline(id);
            }
            if ui.small_button("Bind ID").clicked() {
                *reference = ObjectRef::Id("object-id".into());
            }
        }
        Value::Exec => {}
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
    fn menu_frame(
        ctx: &egui::Context,
        pane: &mut BlueprintPane,
        graph: &mut Blueprint,
        events: Vec<egui::Event>,
    ) -> (Rect, Vec<(String, Rect)>) {
        let mut button = Rect::NOTHING;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 650.))),
                focused: true,
                events,
                ..Default::default()
            },
            |ui| {
                button = pane.add_node_menu(ui, graph).rect;
            },
        );
        fn collect(shape: &egui::Shape, clip: Rect, labels: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, clip, labels);
                    }
                }
                egui::Shape::Text(text) => {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    if clip.intersects(rect) {
                        labels.push((text.galley.text().into(), rect));
                    }
                }
                _ => {}
            }
        }
        let mut labels = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, shape.clip_rect, &mut labels);
        }
        output.textures_delta.clear();
        (button, labels)
    }
    #[test]
    fn add_node_search_stays_open_filters_sorted_nodes_and_selects_one() {
        let ctx = egui::Context::default();
        let mut pane = BlueprintPane::default();
        let mut graph = Blueprint::default();
        let button = menu_frame(&ctx, &mut pane, &mut graph, vec![]).0.center();
        menu_frame(&ctx, &mut pane, &mut graph, pointer(button, true));
        menu_frame(&ctx, &mut pane, &mut graph, pointer(button, false));
        let (_, labels) = menu_frame(&ctx, &mut pane, &mut graph, vec![]);
        let visible: Vec<_> = labels
            .iter()
            .filter(|(text, _)| NodeKind::specs().iter().any(|spec| spec.title == text))
            .map(|(text, _)| text.to_lowercase())
            .collect();
        assert!(visible.len() > 5, "the open menu shows node choices");
        assert!(visible.windows(2).all(|pair| pair[0] < pair[1]));
        let search = labels
            .iter()
            .find(|(text, _)| text == "Search nodes…")
            .unwrap()
            .1
            .center();
        menu_frame(&ctx, &mut pane, &mut graph, pointer(search, true));
        menu_frame(&ctx, &mut pane, &mut graph, pointer(search, false));
        assert!(
            egui::Popup::is_any_open(&ctx),
            "clicking search must keep the menu open"
        );
        menu_frame(
            &ctx,
            &mut pane,
            &mut graph,
            vec![egui::Event::Text("dElAy".into())],
        );
        let (_, labels) = menu_frame(&ctx, &mut pane, &mut graph, vec![]);
        assert_eq!(pane.search, "dElAy");
        assert!(!labels.iter().any(|(text, _)| text == "Abs"));
        let delay = labels
            .iter()
            .find(|(text, _)| text == "Delay / After")
            .unwrap()
            .1
            .center();
        menu_frame(&ctx, &mut pane, &mut graph, pointer(delay, true));
        menu_frame(&ctx, &mut pane, &mut graph, pointer(delay, false));
        assert_eq!(graph.nodes.len(), 3);
        assert_eq!(graph.nodes.last().unwrap().kind, NodeKind::Delay);
        assert_eq!(pane.selected, Some(graph.nodes.last().unwrap().id));
        assert!(
            !egui::Popup::is_any_open(&ctx),
            "selecting a node closes the menu"
        );

        for dismiss in [None, Some(egui::Key::Escape)] {
            menu_frame(&ctx, &mut pane, &mut graph, pointer(button, true));
            menu_frame(&ctx, &mut pane, &mut graph, pointer(button, false));
            assert!(egui::Popup::is_any_open(&ctx));
            if let Some(key) = dismiss {
                menu_frame(
                    &ctx,
                    &mut pane,
                    &mut graph,
                    vec![egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            } else {
                let outside = Pos2::new(900., 600.);
                menu_frame(&ctx, &mut pane, &mut graph, pointer(outside, true));
                menu_frame(&ctx, &mut pane, &mut graph, pointer(outside, false));
            }
            assert!(!egui::Popup::is_any_open(&ctx));
            assert_eq!(graph.nodes.len(), 3, "dismissing must not add another node");
        }
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
    #[test]
    fn canvas_handles_stale_wires_and_typed_reroutes_and_pastes_selected_subgraphs() {
        let ctx = egui::Context::default();
        let mut pane = BlueprintPane::default();
        let mut graph = Blueprint::spinning();
        graph.wires.push(Wire {
            from: Socket { node: 1, port: 99 },
            to: Socket { node: 4, port: 99 },
        });
        frame(&ctx, &mut pane, &mut graph, vec![], true);
        assert_eq!(graph.stale_wires().len(), 1);
        graph.wires.pop();
        pane.selection = std::collections::BTreeSet::from([1, 4]);
        let mut output = ctx.run_ui(Default::default(), |ui| pane.copy(ui, &graph));
        output.textures_delta.clear();
        pane.paste(&mut graph).unwrap();
        assert_eq!(pane.selection.len(), 2);
        assert_eq!(graph.nodes.len(), 6);
        graph.validate().unwrap();
        let mut reroute = Node::new(7, NodeKind::Reroute, [40., 350.]);
        reroute.value_type = PinType::Text;
        reroute.reset_inputs();
        graph.nodes.push(reroute);
        let mut comment = Node::new(8, NodeKind::Comment, [350., 350.]);
        comment.comment = "Shared state across graphs".into();
        graph.nodes.push(comment);
        frame(&ctx, &mut pane, &mut graph, vec![], true);
        graph.validate().unwrap();
    }
}
