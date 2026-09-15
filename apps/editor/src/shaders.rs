//! An egui Scene supplies native pan/zoom; only shader nodes and typed wires are custom.
use super::*;
use bozzard_scene::shader_graph::{
    Node, NodeKind, PinType, ShaderGraph, Socket, TextureSlot, Value, Wire,
};

pub struct ShaderPane {
    target: Option<(PathBuf, String)>,
    selected: Option<u32>,
    connecting: Option<Socket>,
    view: Rect,
    search: String,
}
impl Default for ShaderPane {
    fn default() -> Self {
        Self {
            target: None,
            selected: None,
            connecting: None,
            view: Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 560.)),
            search: String::new(),
        }
    }
}
impl ShaderPane {
    fn sync(&mut self, path: &Path, id: &str) {
        let target = (path.to_owned(), id.to_owned());
        if self.target.as_ref() != Some(&target) {
            *self = Self {
                target: Some(target),
                ..Self::default()
            };
        }
    }
    fn fit(&mut self, graph: &ShaderGraph) {
        self.view = graph
            .nodes
            .iter()
            .map(node_rect)
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::splat(500.)))
            .expand(40.);
    }
    fn choose(&mut self, graph: &ShaderGraph) {
        self.selected = None;
        self.connecting = None;
        self.fit(graph);
    }
}
impl App {
    pub fn shader_dialog(&mut self, kind: files::Kind, object_id: &str) {
        let path = bozzard_editor::root(&self.editor.path)
            .join("assets/ShaderGraphs/material.shadergraph.json");
        if let Err(error) = std::fs::create_dir_all(path.parent().unwrap()) {
            self.result(Err(error.into()));
            return;
        }
        let mut dialog = files::Dialog::new(kind, &path);
        dialog.shader_target = Some((
            self.editor.path.clone(),
            self.editor.revision(),
            object_id.to_string(),
        ));
        self.dialog = Some(dialog);
    }

    pub fn shader_graph_inspector(
        &mut self,
        ui: &mut egui::Ui,
        object: &mut bozzard_scene::Object,
    ) {
        egui::CollapsingHeader::new("SHADER GRAPH")
            .id_salt((&object.id, "shader-graph"))
            .default_open(object.shader_graph.is_some())
            .show(ui, |ui| {
                let editing =
                    self.editor.play.is_none() && self.loading.is_none() && self.dialog.is_none();
                ui.add_enabled_ui(editing, |ui| {
                    let mut remove = false;
                    if let Some(graph) = &mut object.shader_graph {
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .button("Edit")
                                .on_hover_text("Open the node editor pane")
                                .clicked()
                            {
                                self.open_shader_editor();
                            }
                            if ui.button("Load…").clicked() {
                                self.shader_dialog(files::Kind::LoadShaderGraph, &object.id);
                            }
                            if ui.button("Save…").clicked() {
                                self.shader_dialog(files::Kind::SaveShaderGraph, &object.id);
                            }
                            if ui
                                .button("Remove")
                                .on_hover_text("Detach graph (Undo restores it)")
                                .clicked()
                            {
                                remove = true;
                            }
                        });
                        ui.weak(format!(
                            "{} nodes · {} wires · {}",
                            graph.nodes.len(),
                            graph.wires.len(),
                            graph.name
                        ));
                        if let Err(error) = graph.validate() {
                            ui.colored_label(egui::Color32::LIGHT_RED, format!("{error:#}"));
                        }
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .button("+ New")
                                .on_hover_text(
                                    "Start an empty graph (Base Color etc. stay stock until wired)",
                                )
                                .clicked()
                            {
                                object.shader_graph = Some(Default::default());
                                self.open_shader_editor();
                            }
                            if ui.button("Load…").clicked() {
                                self.shader_dialog(files::Kind::LoadShaderGraph, &object.id);
                            }
                        });
                        ui.weak("Overrides the object's material surface channels.");
                    }
                    if remove {
                        object.shader_graph = None;
                    }
                });
            });
    }

    pub fn open_shader_editor(&mut self) {
        self.workspace.shaders_visible = true;
        if let Some(object) = self.editor.selected_object() {
            self.shader_pane.sync(&self.editor.path, &object.id);
            if let Some(graph) = &object.shader_graph {
                self.shader_pane.fit(graph);
            }
        }
    }
    pub fn shader_ui(&mut self, ui: &mut egui::Ui) {
        self.viewport_rect = None;
        self.mouse_captured = false;
        self.fly_latched = false;
        self.navigation_button = None;
        self.gameplay_controls.reset();
        if let Some(play) = &mut self.editor.play {
            play.clear_gameplay_input();
        }
        let Some(object) = self.editor.selected_object().cloned() else {
            ui.weak("Select an object with a Mesh Renderer, then attach a graph in Properties.");
            return;
        };
        if self.editor.selected_surface().is_some() {
            ui.weak("Select the mesh owner in Hierarchy to edit its Shader Graph.");
            return;
        }
        self.shader_pane.sync(&self.editor.path, &object.id);
        let Some(graph) = object.shader_graph.clone() else {
            ui.heading("Shader Editor");
            ui.label("No coding required. Wire parameters into Master channels, then check the Scene tab.");
            ui.add_enabled_ui(
                self.editor.play.is_none() && self.loading.is_none() && self.dialog.is_none(),
                |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("New Shader Graph").clicked() {
                            let graph = ShaderGraph::default();
                            self.shader_pane.choose(&graph);
                            let result = self.editor.set_shader_graph(&object.id, Some(graph));
                            self.result(result);
                        }
                        if ui.button("Load graph…").clicked() {
                            self.shader_dialog(files::Kind::LoadShaderGraph, &object.id);
                        }
                    });
                },
            );
            return;
        };
        let editing = self.editor.play.is_none()
            && self.loading.is_none()
            && self.dialog.is_none()
            && !self.confirm_discard;
        let mut graph = graph;
        ui.horizontal_wrapped(|ui| {
            ui.strong("Shader Editor");
            ui.label(&object.name);
            if ui.button("Fit graph").clicked() {
                self.shader_pane.fit(&graph);
            }
            ui.add_enabled_ui(editing, |ui| {
                if ui.button("Load copy…").clicked() {
                    self.shader_dialog(files::Kind::LoadShaderGraph, &object.id);
                }
                if ui.button("Save graph…").clicked() {
                    self.shader_dialog(files::Kind::SaveShaderGraph, &object.id);
                }
            });
        });
        ui.add_enabled_ui(editing, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Name");
                ui.add(
                    egui::TextEdit::singleline(&mut graph.name)
                        .desired_width(170.)
                        .char_limit(128),
                );
                ui.menu_button("+ Add node", |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.shader_pane.search)
                            .hint_text("Search nodes…"),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(340.)
                        .show(ui, |ui| {
                            for kind in NodeKind::ALL {
                                if kind
                                    .label()
                                    .to_lowercase()
                                    .contains(&self.shader_pane.search.to_lowercase())
                                    && ui.button(kind.label()).clicked()
                                {
                                    let id = graph
                                        .nodes
                                        .iter()
                                        .map(|n| n.id)
                                        .max()
                                        .unwrap_or(0)
                                        .saturating_add(1);
                                    graph.nodes.push(Node::new(
                                        id,
                                        kind,
                                        self.shader_pane.view.center().into(),
                                    ));
                                    self.shader_pane.selected = Some(id);
                                    ui.close();
                                }
                            }
                        });
                });
                if ui
                    .add_enabled(
                        self.shader_pane.selected.is_some_and(|id| {
                            graph.node(id).is_ok_and(|n| n.kind != NodeKind::Master)
                        }),
                        egui::Button::new("Delete node"),
                    )
                    .clicked()
                {
                    if let Some(id) = self.shader_pane.selected.take() {
                        graph.remove_node(id);
                    }
                    self.shader_pane.connecting = None;
                }
            });
        });
        ui.small("Drag headers to move · Output → input to connect · Right-click input to disconnect · Middle-drag / scroll to pan · Ctrl+scroll to zoom · Live preview on the right");
        ui.horizontal_top(|ui| {
            let preview_width = (ui.available_width() * 0.34).clamp(240., 480.);
            let canvas_width = (ui.available_width() - preview_width - 10.).max(320.);
            ui.allocate_ui_with_layout(
                Vec2::new(canvas_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if editing && !ui.ctx().egui_wants_keyboard_input() {
                        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                        {
                            self.shader_pane.connecting = None;
                            self.shader_pane.selected = None;
                        }
                        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
                            && let Some(id) = self.shader_pane.selected.take()
                            && graph.node(id).is_ok_and(|n| n.kind != NodeKind::Master)
                        {
                            graph.remove_node(id);
                            self.shader_pane.connecting = None;
                        }
                    }
                    let error = self.shader_pane.canvas(ui, &mut graph, editing);
                    if let Some(error) = error {
                        self.result(Err(error));
                    }
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                Vec2::new(preview_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if let Err(error) = self.shader_preview(ui) {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("Preview: {error:#}"));
                    }
                },
            );
        });
        if editing && Some(&graph) != object.shader_graph.as_ref() {
            self.editor.begin_gesture("Edit shader graph");
            let result = self.editor.set_shader_graph(&object.id, Some(graph));
            self.result(result);
        }
    }
}
impl App {
    /// Live material preview: a preview-only unit sphere carries the graph, so
    /// the pane shows the shader itself rather than the whole scene. A
    /// pane-owned clock advances display time so Time nodes animate without
    /// pressing Play.
    fn shader_preview(&mut self, ui: &mut egui::Ui) -> Result<()> {
        let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let Some(graph) = self
            .editor
            .selected_object()
            .and_then(|o| o.shader_graph.clone())
        else {
            return Ok(());
        };
        let shader = Some(bozzard_render_assets::shader_source(&graph)?);
        let ppp = ui.ctx().pixels_per_point();
        let limit = self.gpu.device.limits().max_texture_dimension_2d.min(4096);
        let size = [
            (rect.width() * ppp).round().clamp(1.0, limit as f32) as u32,
            (rect.height() * ppp).round().clamp(1.0, limit as f32) as u32,
        ];
        if self.preview_target.as_ref().is_none_or(|t| t.size != size) {
            let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Shader graph preview"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
            });
            let view = texture.create_view(&Default::default());
            // egui samples gamma colors; reinterpret the sRGB render target as UNORM for the UI.
            let sampled = texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            });
            let mut renderer = self.render_state.renderer.write();
            let id = if let Some(target) = &self.preview_target {
                renderer.update_egui_texture_from_wgpu_texture(
                    &self.gpu.device,
                    &sampled,
                    wgpu::FilterMode::Linear,
                    target.id,
                );
                target.id
            } else {
                renderer.register_native_texture(
                    &self.gpu.device,
                    &sampled,
                    wgpu::FilterMode::Linear,
                )
            };
            self.preview_target = Some(Target {
                texture,
                view,
                id,
                size,
            });
        }
        let aspect = size[0] as f32 / size[1] as f32;
        self.preview_time = (self.preview_time + ui.input(|i| i.stable_dt.min(0.05))) % 4096.;
        let radius = 0.5_f32;
        let eye = Vec3::new(1., 0.55, 1.).normalize() * (radius * 2.4 + 0.4);
        let lens = glam::camera::rh::proj::directx::perspective(
            50f32.to_radians(),
            aspect,
            0.05,
            radius * 8. + 20.,
        );
        let scene = bozzard_render::RenderScene {
            skin_poses: Default::default(),
            shader_time: self.preview_time,
            particles: Vec::new(),
            view_projection: lens * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y),
            items: vec![bozzard_render::DrawItem {
                motion_id: 0,
                mesh: bozzard_render::MeshKind::Sphere,
                model: glam::Mat4::IDENTITY,
                material: bozzard_render::Material {
                    metallic: None,
                    roughness: None,
                    tint: [1.; 3],
                    lit: true,
                    texture: bozzard_render::TextureKind::White,
                    uv_scale: [1.; 2],
                    surface_overrides: Default::default(),
                    shader,
                },
            }],
            lighting: bozzard_render::Lighting {
                shadows: false,
                sun_direction: [-0.45, -0.8, -0.4],
                sun_intensity: 3.5 * std::f32::consts::PI,
                ambient_intensity: 0.35,
                ..Default::default()
            },
            lights: Vec::new(),
            environment: bozzard_render::EnvironmentSettings::disabled(),
            fog: Default::default(),
            gi: None,
            display: bozzard_render::DisplaySettings {
                tone_mapping: false,
                time_seconds: self.preview_time,
                ..Default::default()
            },
        };
        let target = self.preview_target.as_ref().unwrap();
        self.renderer.draw(&self.gpu, &target.view, size, &scene)?;
        ui.painter().image(
            target.id,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        ui.ctx().request_repaint();
        Ok(())
    }
}
const WIDTH: f32 = 260.;
fn node_rect(node: &Node) -> Rect {
    Rect::from_min_size(
        Pos2::from(node.position),
        Vec2::new(
            WIDTH,
            70. + 28. * node.kind.inputs().len().max(node.kind.outputs().len()) as f32
                + if node.kind == NodeKind::TextureSample {
                    26.
                } else {
                    0.
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
        PinType::Float => Color32::from_rgb(132, 206, 71),
        PinType::Vector => Color32::from_rgb(88, 183, 225),
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
impl ShaderPane {
    fn canvas(
        &mut self,
        ui: &mut egui::Ui,
        graph: &mut ShaderGraph,
        editing: bool,
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
                        if node.kind == NodeKind::Master {
                            Color32::from_rgb(48, 92, 60)
                        } else if node.kind.inputs().is_empty() {
                            Color32::from_rgb(109, 48, 57)
                        } else {
                            Color32::from_rgb(38, 76, 100)
                        },
                    );
                    ui.painter().text(
                        header.left_center() + Vec2::new(10., 0.),
                        egui::Align2::LEFT_CENTER,
                        node.kind.label(),
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
                            node.kind.label(),
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
                            if node.kind == NodeKind::TextureSample {
                                let config = Rect::from_min_size(
                                    rect.min
                                        + Vec2::new(
                                            12.,
                                            59. + 28. * node.kind.inputs().len() as f32,
                                        ),
                                    Vec2::new(WIDTH - 24., 24.),
                                );
                                ui.scope_builder(egui::UiBuilder::new().max_rect(config), |ui| {
                                    egui::ComboBox::from_id_salt("texture-slot")
                                        .selected_text(format!("{:?}", node.slot))
                                        .show_ui(ui, |ui| {
                                            for slot in TextureSlot::ALL {
                                                ui.selectable_value(
                                                    &mut node.slot,
                                                    slot,
                                                    format!("{slot:?}"),
                                                );
                                            }
                                        });
                                });
                            }
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
                                        format!("{} input {label}: {kind:?}", node.kind.label()),
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
                                if !linked && node.kind != NodeKind::Master {
                                    let input_rect = Rect::from_min_size(
                                        p + Vec2::new(70., -10.),
                                        Vec2::new(120., 22.),
                                    );
                                    ui.scope_builder(
                                        egui::UiBuilder::new()
                                            .id_salt((port, "value"))
                                            .max_rect(input_rect),
                                        |ui| match &mut node.inputs[port] {
                                            Value::Float(v) => {
                                                ui.add_sized(
                                                    [100., 20.],
                                                    egui::DragValue::new(v).speed(0.1),
                                                );
                                            }
                                            Value::Vector(v) if node.kind == NodeKind::Color => {
                                                ui.color_edit_button_rgb(v);
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
                                        format!("{} output {label}: {kind:?}", node.kind.label()),
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
        pane: &mut ShaderPane,
        graph: &mut ShaderGraph,
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
                pane.canvas(ui, graph, editing);
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
    fn canvas_connects_typed_pins_moves_nodes_and_protects_play() {
        let ctx = egui::Context::default();
        let mut pane = ShaderPane::default();
        let mut graph = ShaderGraph::default();
        graph.nodes.push(Node::new(2, NodeKind::Time, [30., 30.]));
        let transform = frame(&ctx, &mut pane, &mut graph, vec![], true);
        let from = transform * pin(graph.node(2).unwrap(), 0, true);
        let to = transform * pin(graph.node(1).unwrap(), 1, false);
        frame(&ctx, &mut pane, &mut graph, pointer(from, true), true);
        frame(&ctx, &mut pane, &mut graph, pointer(from, false), true);
        assert_eq!(pane.connecting, Some(Socket { node: 2, port: 0 }));
        frame(&ctx, &mut pane, &mut graph, pointer(to, true), true);
        frame(&ctx, &mut pane, &mut graph, pointer(to, false), true);
        assert_eq!(
            graph.wires,
            vec![Wire {
                from: Socket { node: 2, port: 0 },
                to: Socket { node: 1, port: 1 }
            }]
        );
        graph.validate().unwrap();
        // A Float cannot drive a Vector channel, and failed connections preserve the graph.
        let before = graph.clone();
        pane.connecting = Some(Socket { node: 2, port: 0 });
        frame(&ctx, &mut pane, &mut graph, pointer(to, true), true);
        frame(&ctx, &mut pane, &mut graph, pointer(to, false), true);
        assert_eq!(graph, before);
        // Drag a node header to move it.
        let header = transform * (Pos2::from(graph.nodes[1].position) + Vec2::new(50., 15.));
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
        assert!(graph.nodes[1].position[0] > before.nodes[1].position[0] + 40.);
        // Without editing, nothing connects.
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
