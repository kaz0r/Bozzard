use anyhow::{Context, Result, ensure};
use bozzard_assets::LoadState;
use bozzard_editor::Editor;
use bozzard_render::{Backend, Gpu, SceneRenderer, wgpu};
use bozzard_scene::{AssetKind, Camera, Layer, Mesh, Spin, Texture, Transform};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
mod acceptance;
mod asset_browser;
mod blueprints;
mod colliders;
mod files;
mod fog;
mod framing;
mod gameplay_input;
mod gi;
mod hierarchy;
mod inspector;
mod lights;
mod loading;
mod snapping;
mod surfaces;
mod theme;
mod viewport;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Tool {
    #[default]
    Move,
    Rotate,
    Scale,
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Workspace {
    scene_path: Option<PathBuf>,
    layer_2d: bool,
    assets_visible: bool,
    settings_visible: bool,
    blueprints_visible: bool,
    stats_visible: bool,
    colliders_visible: bool,
    gi_visible: bool,
    tool: Tool,
    snapping: snapping::Snapping,
    pan: [f32; 2],
    zoom: f32,
    camera: Option<viewport::FlyCamera>,
    ortho_zoom: f32,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            scene_path: None,
            layer_2d: false,
            assets_visible: true,
            settings_visible: true,
            blueprints_visible: false,
            stats_visible: false,
            colliders_visible: true,
            gi_visible: false,
            tool: Tool::Move,
            snapping: snapping::Snapping::default(),
            pan: [0.0; 2],
            zoom: 1.0,
            camera: None,
            ortho_zoom: 1.0,
        }
    }
}

impl Workspace {
    fn restore_scene(&mut self, path: &Path) {
        if self.scene_path.as_deref() != Some(path) {
            self.pan = [0.0; 2];
            self.zoom = 1.0;
            self.camera = None;
            self.ortho_zoom = 1.0;
            self.layer_2d = false;
        }
        self.scene_path = Some(path.to_owned());
    }
}

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    id: egui::TextureId,
    size: [u32; 2],
}
struct App {
    editor: Editor,
    gpu: Gpu,
    renderer: SceneRenderer,
    residency: bozzard_render_assets::Residency,
    target: Option<Target>,
    render_state: eframe::egui_wgpu::RenderState,
    workspace: Workspace,
    status: String,
    hierarchy_search: String,
    surface_search: String,
    hierarchy_state: hierarchy::HierarchyState,
    hierarchy_frame_requested: bool,
    hierarchy_rename: Option<(String, String, bool)>,
    asset_browser: asset_browser::AssetBrowser,
    blueprint_pane: blueprints::BlueprintPane,
    loading: Option<loading::Loading>,
    import_queue: std::collections::VecDeque<PathBuf>,
    refresh: Option<loading::Refresh>,
    reload_paused: bool,
    continue_after_save: bool,
    close_after_loading: bool,
    error: bool,
    last_frame: Instant,
    last_assets: Instant,
    dialog: Option<files::Dialog>,
    pending: Option<Pending>,
    confirm_discard: bool,
    allow_close: bool,
    drag: Option<viewport::Drag>,
    navigation_button: Option<egui::PointerButton>,
    mouse_captured: bool,
    escape_deselect_requested: bool,
    fly_latched: bool,
    fly_tab_down: bool,
    viewport_rect: Option<Rect>,
    gameplay_controls: gameplay_input::GameplayControls,
    smoke: Option<PathBuf>,
    smoke_passed: Arc<AtomicBool>,
    smoke_frames: u32,
    smoke_start: Instant,
    smoke_requested: bool,
    smoke_gizmo_verified: bool,
    smoke_surface_gizmo_verified: bool,
    smoke_expected: Option<bozzard_scene::Scene>,
    smoke_selection: Option<String>,
    smoke_surface_frame: Option<u32>,
    smoke_light_frame: Option<u32>,
    smoke_gi_frame: Option<u32>,
    smoke_prefab_frame: Option<u32>,
    smoke_blueprint_frame: Option<u32>,
    smoke_object_reference_frame: Option<u32>,
}
#[derive(Clone)]
struct HierarchyDrag(String);

enum Pending {
    Open(PathBuf),
    New,
    Close,
}
impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        editor: Editor,
        smoke: Option<PathBuf>,
        smoke_passed: Arc<AtomicBool>,
    ) -> Result<Self> {
        let state = cc
            .wgpu_render_state
            .clone()
            .context("editor requires native WebGPU")?;
        let gpu = Gpu {
            adapter: state.adapter.clone(),
            device: state.device.clone(),
            queue: state.queue.clone(),
        };
        theme::install(&cc.egui_ctx);
        let mut workspace: Workspace = if smoke.is_none() {
            cc.storage
                .and_then(|s| eframe::get_value(s, "workspace"))
                .unwrap_or_default()
        } else {
            Workspace::default()
        };
        workspace.restore_scene(&editor.path);
        let renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        Ok(Self {
            editor,
            gpu,
            renderer,
            residency: bozzard_render_assets::Residency::default(),
            render_state: state,
            target: None,
            workspace,
            status: "Ready · Select an object to begin".into(),
            hierarchy_search: String::new(),
            surface_search: String::new(),
            hierarchy_state: hierarchy::HierarchyState::default(),
            hierarchy_frame_requested: false,
            hierarchy_rename: None,
            asset_browser: asset_browser::AssetBrowser::default(),
            blueprint_pane: blueprints::BlueprintPane::default(),
            loading: None,
            import_queue: std::collections::VecDeque::new(),
            refresh: None,
            reload_paused: false,
            continue_after_save: false,
            close_after_loading: false,
            error: false,
            last_frame: Instant::now(),
            last_assets: Instant::now() - Duration::from_secs(1),
            dialog: None,
            pending: None,
            confirm_discard: false,
            allow_close: false,
            drag: None,
            navigation_button: None,
            mouse_captured: false,
            escape_deselect_requested: false,
            fly_latched: false,
            fly_tab_down: false,
            viewport_rect: None,
            gameplay_controls: gameplay_input::GameplayControls::default(),
            smoke,
            smoke_passed,
            smoke_frames: 0,
            smoke_start: Instant::now(),
            smoke_requested: false,
            smoke_gizmo_verified: false,
            smoke_surface_gizmo_verified: false,
            smoke_expected: None,
            smoke_selection: None,
            smoke_surface_frame: None,
            smoke_light_frame: None,
            smoke_gi_frame: None,
            smoke_prefab_frame: None,
            smoke_blueprint_frame: None,
            smoke_object_reference_frame: None,
        })
    }
    fn result(&mut self, result: Result<()>) {
        if let Err(error) = result {
            self.status = format!("{error:#}");
            self.error = true;
        } else {
            self.error = false;
        }
    }
    fn layer(&self) -> Layer {
        if self.workspace.layer_2d {
            Layer::TwoD
        } else {
            Layer::ThreeD
        }
    }
    fn save_scene(&mut self, path: PathBuf) {
        let result = (|| {
            ensure!(self.loading.is_none(), "Wait for loading to finish");
            self.drag = None;
            self.loading = Some(loading::Loading::Save(self.editor.save_job(path)?));
            Ok(())
        })();
        self.result(result);
    }
    fn request(&mut self, pending: Pending) {
        if self.loading.is_some() {
            self.status = "Wait for loading to finish or cancel it first".into();
            return;
        }
        self.editor.finish_gesture();
        self.drag = None;
        self.pending = Some(pending);
        if self.editor.dirty() {
            self.confirm_discard = true;
        } else {
            self.perform_pending();
        }
    }
    fn perform_pending(&mut self) {
        self.confirm_discard = false;
        let result = (|| -> Result<()> {
            match self.pending.take() {
                Some(Pending::Close) => self.allow_close = true,
                Some(Pending::Open(path)) => {
                    ensure!(self.loading.is_none(), "Wait for loading to finish");
                    self.loading = Some(loading::Loading::Open(Editor::open_job(path)?));
                }
                Some(Pending::New) => {
                    let mut scene = bozzard_demo::scene_document()?;
                    scene.name = "Untitled level".into();
                    scene.objects.retain(|o| o.camera.is_some());
                    let path = untitled_scene_path()?;
                    self.editor = Editor::new(scene, &path)?;
                    self.hierarchy_state = hierarchy::HierarchyState::default();
                    self.refresh = None;
                    self.reload_paused = false;
                    self.workspace.camera = None;
                    self.workspace.ortho_zoom = 1.0;
                    self.residency.retry_failed();
                    self.status = "New level · Add a cube or sprite".into();
                }
                None => {}
            }
            Ok(())
        })();
        self.result(result);
    }
    fn sync_assets(&mut self) -> Result<()> {
        if self.refresh.is_none()
            && self.loading.is_none()
            && !self.reload_paused
            && self.last_assets.elapsed() > Duration::from_millis(500)
        {
            self.last_assets = Instant::now();
            self.refresh = Some((
                self.editor.asset_revision(),
                self.editor.assets.refresh_job()?,
            ));
        }
        // Keep the catalog snapshot stable while an import/save is prepared.
        if self.loading.is_some() {
            return Ok(());
        }
        if let Some((revision, cancelled, result)) =
            self.refresh.as_ref().and_then(|(revision, job)| {
                job.poll()
                    .map(|result| (*revision, job.cancelled(), result))
            })
        {
            self.refresh = None;
            self.last_assets = Instant::now();
            if cancelled {
                self.status = "Reload cancelled · Click Reload to resume automatic refresh".into();
                self.error = false;
                return Ok(());
            }
            if revision != self.editor.asset_revision() {
                return Ok(());
            }
            let (store, changed) = result?;
            let initial_load = self
                .editor
                .assets
                .entries()
                .any(|entry| entry.data().is_none());
            self.editor.assets = store;
            let mut reloaded = Vec::new();
            let mut failure = None;
            for handle in changed {
                let entry = self
                    .editor
                    .assets
                    .get(handle)
                    .context("missing asset handle")?;
                match entry.state() {
                    LoadState::Ready => {
                        reloaded.push(entry.id.clone());
                    }
                    LoadState::Failed(message) => {
                        failure = Some(format!(
                            "{message} · {}",
                            if entry.data().is_some() {
                                "Keeping last good asset"
                            } else {
                                "Repair the file and reload"
                            }
                        ));
                    }
                    LoadState::Pending => {}
                }
            }
            if let Some(message) = failure {
                self.status = message;
                self.error = true;
            } else if !reloaded.is_empty() {
                self.status = if initial_load {
                    "Scene assets ready".into()
                } else if reloaded.len() == 1 {
                    format!("Reloaded {}", reloaded[0])
                } else {
                    format!("Reloaded {} assets", reloaded.len())
                };
                self.error = false;
            }
        }
        Ok(())
    }
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu-bar")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(22, 22, 24))
                    .inner_margin(4),
            )
            .show(ui, |ui| {
                ui.add_enabled_ui(self.loading.is_none(), |ui| {
                    egui::MenuBar::new().ui(ui, |ui| {
                        ui.label(
                            egui::RichText::new("B")
                                .size(20.0)
                                .strong()
                                .color(theme::GREEN),
                        );
                        ui.menu_button("File", |ui| {
                            if ui.button("New scene").clicked() {
                                self.request(Pending::New);
                                ui.close();
                            }
                            if ui.button("Open scene…").clicked() {
                                self.dialog =
                                    Some(files::Dialog::new(files::Kind::Open, &self.editor.path));
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .add(egui::Button::new("Save").shortcut_text("Ctrl/Cmd+S"))
                                .clicked()
                            {
                                self.save_scene(self.editor.path.clone());
                                ui.close();
                            }
                            if ui.button("Save as…").clicked() {
                                self.dialog =
                                    Some(files::Dialog::new(files::Kind::Save, &self.editor.path));
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .add_enabled(
                                    self.editor.play.is_none(),
                                    egui::Button::new("Import asset…"),
                                )
                                .clicked()
                            {
                                self.dialog = Some(files::Dialog::new(
                                    files::Kind::Import,
                                    &self.editor.path,
                                ));
                                ui.close();
                            }
                        });
                        ui.menu_button("Edit", |ui| {
                            ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                                if ui
                                    .add_enabled(
                                        self.editor.undo_label().is_some(),
                                        egui::Button::new("Undo").shortcut_text("Ctrl/Cmd+Z"),
                                    )
                                    .on_hover_text(
                                        self.editor.undo_label().unwrap_or("Nothing to undo"),
                                    )
                                    .clicked()
                                {
                                    let r = self.editor.undo();
                                    self.result(r);
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(
                                        self.editor.redo_label().is_some(),
                                        egui::Button::new("Redo").shortcut_text("Ctrl/Cmd+Shift+Z"),
                                    )
                                    .clicked()
                                {
                                    let r = self.editor.redo();
                                    self.result(r);
                                    ui.close();
                                }
                                ui.separator();
                                ui.add_enabled_ui(
                                    self.editor.selected_object().is_some()
                                        && self.editor.selected_surface().is_none(),
                                    |ui| {
                                        if ui.button("Rename").clicked() {
                                            self.begin_hierarchy_rename();
                                            ui.close();
                                        }
                                        if ui.button("Duplicate").clicked() {
                                            let r = self.editor.duplicate();
                                            if r.is_ok() {
                                                self.hierarchy_search.clear();
                                            }
                                            self.result(r);
                                            ui.close();
                                        }
                                        if ui.button("Delete").clicked() {
                                            let r = self.editor.delete();
                                            self.result(r);
                                            ui.close();
                                        }
                                    },
                                );
                            });
                        });
                        ui.menu_button("View", |ui| {
                            ui.checkbox(&mut self.workspace.blueprints_visible, "Blueprint Editor");
                            ui.checkbox(&mut self.workspace.assets_visible, "Content Browser");
                            ui.checkbox(&mut self.workspace.settings_visible, "Scene Settings");
                            ui.checkbox(&mut self.workspace.stats_visible, "Renderer statistics");
                        });
                        ui.menu_button("Help", |ui| {
                            ui.label("Bozzard · Native scene editor");
                            ui.separator();
                            ui.label("W / E / R   Move / Rotate / Scale");
                            ui.label("F / Shift+F   Frame selection / all");
                            ui.label("Right-drag or Tab   Fly camera");
                            ui.label("WASD   Fly movement · Shift   Faster");
                            ui.label("Space / Ctrl   Fly up / down");
                            ui.label("Middle-drag   Pan · Scroll   Dolly");
                            ui.label("Click   Inspect imported surface");
                            ui.label("Alt-click   Select whole model");
                            ui.label("Escape   Release / cancel / deselect");
                            ui.label("Drag files into the editor to import");
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak("BOZZARD  /  EDITOR");
                        });
                    });
                });
            });
        egui::Panel::top("scene-bar").show(ui, |ui| {
            ui.add_enabled_ui(self.loading.is_none(), |ui| {
                ui.horizontal(|ui| {
                    let playing = self.editor.play.is_some();
                    let left_width = (ui.available_width() * 0.5 - 46.0).max(0.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(left_width, 24.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(left_width);
                            ui.colored_label(theme::ACCENT, "◇");
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "{}{}",
                                        self.editor.scene().name,
                                        if self.editor.dirty() { " *" } else { "" }
                                    ))
                                    .strong(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(self.editor.path.display().to_string());
                        },
                    );
                    if ui
                        .add_enabled(
                            !playing,
                            egui::Button::new(egui::RichText::new("▶").color(theme::GREEN)),
                        )
                        .on_hover_text("Play · Authored scene stays protected")
                        .clicked()
                    {
                        self.gameplay_controls.reset();
                        let r = self.editor.start_play();
                        self.result(r);
                    }
                    if ui
                        .add_enabled(playing, egui::Button::new("■"))
                        .on_hover_text("Stop · Escape")
                        .clicked()
                    {
                        self.gameplay_controls.reset();
                        self.editor.stop_play();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(if playing { "PLAY MODE" } else { "EDIT MODE" });
                        ui.colored_label(if playing { theme::GREEN } else { theme::ACCENT }, "●");
                    });
                });
            });
        });
    }
    fn begin_hierarchy_rename(&mut self) {
        if self.editor.selected_surface().is_some() {
            self.status = "Select the whole model before renaming it".into();
            return;
        }
        if let Some(object) = self.editor.selected_object() {
            self.hierarchy_rename = Some((object.id.clone(), object.name.clone(), true));
            self.hierarchy_search.clear();
            self.editor.finish_gesture();
            self.hierarchy_state
                .reveal(self.editor.scene(), self.editor.selected.as_deref());
        }
    }

    fn hierarchy(&mut self, ui: &mut egui::Ui) {
        if self.loading.is_some()
            || self.editor.play.is_some()
            || self.dialog.is_some()
            || self.confirm_discard
            || self.mouse_captured
            || self
                .hierarchy_rename
                .as_ref()
                .is_some_and(|(id, _, _)| self.editor.selected.as_ref() != Some(id))
        {
            self.hierarchy_rename = None;
        }
        if self.loading.is_some() {
            ui.disable();
        }
        theme::panel_title(ui, "Scene Hierarchy");
        ui.horizontal(|ui| {
            ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                ui.menu_button("+ Create", |ui| {
                    if ui.button("+ Cube").clicked() {
                        let r = self.editor.create(Mesh::Cube, Layer::ThreeD);
                        if r.is_ok() {
                            self.hierarchy_search.clear();
                            self.workspace.layer_2d = false;
                        }
                        self.result(r);
                        ui.close();
                    }
                    if ui.button("+ Sprite").clicked() {
                        let r = self.editor.create(Mesh::Quad, Layer::TwoD);
                        if r.is_ok() {
                            self.hierarchy_search.clear();
                            self.workspace.layer_2d = true;
                        }
                        self.result(r);
                        ui.close();
                    }
                    ui.separator();
                    for (kind, label) in [
                        (bozzard_scene::LightKind::Point, "Point light"),
                        (bozzard_scene::LightKind::Spot, "Spot light"),
                        (bozzard_scene::LightKind::Directional, "Directional light"),
                    ] {
                        if ui.button(label).clicked() {
                            let result = self.editor.create_light(kind);
                            if result.is_ok() {
                                self.workspace.layer_2d = false;
                                self.hierarchy_search.clear();
                            }
                            self.result(result);
                            ui.close();
                        }
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("···", |ui| {
                    ui.add_enabled_ui(self.hierarchy_search.trim().is_empty(), |ui| {
                        if ui.button("Expand all").clicked() {
                            self.hierarchy_state.expand_all();
                            ui.close();
                        }
                        if ui.button("Collapse all").clicked() {
                            self.hierarchy_state.collapse_all(self.editor.scene());
                            ui.close();
                        }
                    });
                })
                .response
                .on_hover_text("Hierarchy options");
                ui.weak(format!("{} entities", self.editor.scene().objects.len()));
            });
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.hierarchy_search)
                    .hint_text("Search entities or materials…")
                    .desired_width(ui.available_width() - 28.0),
            );
            if ui.small_button("×").on_hover_text("Clear search").clicked() {
                self.hierarchy_search.clear();
            }
        });
        let query = self.hierarchy_search.trim().to_lowercase();
        let scene = self.editor.scene().clone();
        self.hierarchy_state
            .sync_selection(&scene, self.editor.selected.as_deref());
        self.hierarchy_state.sync_surface_selection(
            self.editor
                .selected_surface()
                .and_then(|s| self.editor.selected.as_deref().map(|id| (id, s.index))),
        );

        let mut stack: Vec<_> = scene
            .objects
            .iter()
            .filter(|o| o.parent.is_none())
            .rev()
            .map(|o| (o, 0usize))
            .collect();
        let mut matches = 0usize;
        let can_reparent = ui.is_enabled()
            && self.editor.play.is_none()
            && self.drag.is_none()
            && !self.mouse_captured
            && self.hierarchy_rename.is_none()
            && self.dialog.is_none()
            && !self.confirm_discard;
        let mut reparent_request: Option<(String, Option<String>)> = None;
        if can_reparent {
            let root = ui.small("▼  Scene Collection").on_hover_text(
                "Scene root · Drop here to unparent, preserving world transform · Undo to restore",
            );
            if root.dnd_hover_payload::<HierarchyDrag>().is_some() {
                ui.painter().rect_stroke(
                    root.rect,
                    2.0,
                    egui::Stroke::new(1.5, Color32::LIGHT_BLUE),
                    egui::StrokeKind::Inside,
                );
            }
            if let Some(id) = root.dnd_release_payload::<HierarchyDrag>() {
                reparent_request = Some((id.0.clone(), None));
            }
        }
        egui::ScrollArea::vertical().id_salt("hierarchy-tree")
                    .max_height((ui.available_height() - 24.0).max(1.0)).show(ui, |ui| {
                    while let Some((object, depth)) = stack.pop() {
                        let object_matches = query.is_empty()
                            || object.name.to_lowercase().contains(&query)
                            || object.id.to_lowercase().contains(&query);
                        let mesh = self.editor.object_mesh(&object.id);
                        let has_surfaces = mesh.is_some_and(|m| !m.parts.is_empty());
                        let surface_matches = mesh.is_some_and(|m| m.parts.iter().enumerate()
                            .any(|(index, part)| surfaces::surface_matches(index, part, &query)));
                        if object_matches || surface_matches {
                            matches += 1;
                            ui.horizontal(|ui| {
                                if query.is_empty() {
                                    ui.add_space((depth.min(12) * 12) as f32);
                                    if has_surfaces || scene.objects.iter().any(|o| o.parent.as_deref() == Some(&object.id)) {
                                        let collapsed = self.hierarchy_state.is_collapsed(&object.id);
                                        if ui.add_sized([18.0, 18.0], egui::Button::new(if collapsed { "▶" } else { "▼" }).frame(false))
                                            .on_hover_text(if collapsed { "Expand children" } else { "Collapse children" }).clicked() {
                                            self.hierarchy_state.toggle(&object.id);
                                        }
                                    } else {
                                        ui.add_space(18.0);
                                    }
                                }
                                if let Some((id, name, focus)) = &mut self.hierarchy_rename
                                    && id == &object.id
                                {
                                    let response = ui.add(egui::TextEdit::singleline(name)
                                        .id_salt(("hierarchy-rename", id.as_str()))
                                        .desired_width(ui.available_width()));
                                    let first = std::mem::take(focus);
                                    if first {
                                        response.request_focus();
                                        response.scroll_to_me(Some(egui::Align::Center));
                                    }
                                    let cancel = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                                    let apply = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                                    if cancel || apply || (!first && response.lost_focus()) {
                                        let (id, name, _) = self.hierarchy_rename.take().unwrap();
                                        response.surrender_focus();
                                        if apply && !cancel {
                                            let mut scene = self.editor.scene().clone();
                                            if let Some(target) = scene.objects.iter_mut().find(|o| o.id == id) {
                                                target.name = name;
                                                let result = self.editor.apply("Rename object", scene);
                                                self.result(result);
                                            }
                                        }
                                    }
                                    return;
                                }
                                let kind = if object.camera.is_some() {
                                    "◉"
                                } else if object.drawable.is_some() {
                                    "◇"
                                } else {
                                    "·"
                                };
                                let response = ui
                                    .selectable_label(
                                        self.editor.selected.as_ref() == Some(&object.id) && self.editor.selected_surface().is_none(),
                                        format!("{kind} {}", object.name),
                                    )
                                    .interact(if can_reparent { Sense::click_and_drag() } else { Sense::click() })
                                    .on_hover_text(format!("{} · Double-click to frame · Drag onto an object to reparent", object.id));
                                if can_reparent {
                                    response.dnd_set_drag_payload(HierarchyDrag(object.id.clone()));
                                    if response.dnd_hover_payload::<HierarchyDrag>().is_some() {
                                        ui.painter().rect_stroke(response.rect, 2.0, egui::Stroke::new(1.5, Color32::LIGHT_BLUE), egui::StrokeKind::Inside);
                                    }
                                    if let Some(id) = response.dnd_release_payload::<HierarchyDrag>() {
                                        reparent_request = Some((id.0.clone(), Some(object.id.clone())));
                                    }
                                }
                                if response.clicked() || response.double_clicked() || response.secondary_clicked() {
                                    self.editor.finish_gesture();
                                    self.editor.select_object(Some(object.id.clone()));
                                }
                                if response.double_clicked()
                                    && self.editor.play.is_none()
                                    && self.drag.is_none()
                                    && !self.mouse_captured
                                {
                                    self.hierarchy_frame_requested = true;
                                }
                                response.context_menu(|ui| {
                                    ui.set_min_width(290.0);
                                    ui.spacing_mut().item_spacing.y = 6.0;
                                    let mac = cfg!(target_os = "macos");
                                    ui.add_enabled_ui(self.editor.play.is_none() && self.loading.is_none()
                                        && self.drag.is_none() && !self.mouse_captured, |ui| {
                                        if ui.add(egui::Button::new("Rename").shortcut_text(if mac { "Cmd+Return" } else { "F2" })).clicked() {
                                            self.editor.select_object(Some(object.id.clone()));
                                            self.begin_hierarchy_rename();
                                            ui.close();
                                        }
                                        if ui.add(egui::Button::new("Duplicate").shortcut_text(if mac { "Cmd+D" } else { "Ctrl+D" })).clicked() {
                                            self.editor.finish_gesture();
                                            self.editor.select_object(Some(object.id.clone()));
                                            let result = self.editor.duplicate();
                                            if result.is_ok() { self.hierarchy_search.clear(); }
                                            self.result(result);
                                            ui.close();
                                        }
                                        if ui.add(egui::Button::new("Frame Selection").shortcut_text(if mac { "Cmd+Shift+F" } else { "Ctrl+Shift+F" })).clicked() {
                                            self.editor.select_object(Some(object.id.clone()));
                                            self.hierarchy_frame_requested = true;
                                            ui.close();
                                        }
                                        if ui.add_enabled(object.parent.is_some(), egui::Button::new("Unparent"))
                                            .on_hover_text("Move to Scene root, preserving world transform · Undo to restore")
                                            .clicked()
                                        {
                                            reparent_request = Some((object.id.clone(), None));
                                            ui.close();
                                        }
                                        ui.separator();
                                        if ui.add(egui::Button::new("Delete").shortcut_text(if mac { "Cmd+Backspace" } else { "Delete" })).clicked() {
                                            self.editor.finish_gesture();
                                            self.editor.select_object(Some(object.id.clone()));
                                            let result = self.editor.delete();
                                            self.result(result);
                                            ui.close();
                                        }
                                    });
                                });
                            });
                        }
                        if !self.hierarchy_state.visit_children(&object.id, !query.is_empty()) {
                            continue;
                        }
                        self.hierarchy_surfaces(ui, &object.id, depth + 1, if object_matches { "" } else { &query });
                        for child in scene
                            .objects
                            .iter()
                            .filter(|o| o.parent.as_deref() == Some(&object.id))
                            .rev()
                        {
                            stack.push((child, depth + 1));
                        }
                    }
                });
        // Use only the blank region below the rows, never the row gaps
        // or toolbar, so dropping near a child cannot accidentally unparent it.
        if can_reparent {
            let size = Vec2::new(
                ui.available_width(),
                (ui.available_height() - 26.0).max(0.0),
            );
            let (_, blank) = ui.allocate_exact_size(size, Sense::hover());
            if blank.dnd_hover_payload::<HierarchyDrag>().is_some() {
                ui.painter().rect_stroke(
                    blank.rect,
                    2.0,
                    egui::Stroke::new(1.5, Color32::LIGHT_BLUE),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    blank.rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Drop to unparent",
                    egui::FontId::proportional(12.0),
                    Color32::LIGHT_BLUE,
                );
            }
            if let Some(id) = blank.dnd_release_payload::<HierarchyDrag>() {
                reparent_request = Some((id.0.clone(), None));
            }
        }
        if let Some((id, parent)) = reparent_request {
            let result = self.editor.reparent(&id, parent.as_deref());
            if result.is_ok() {
                self.editor.select_object(Some(id));
                self.hierarchy_state
                    .reveal(self.editor.scene(), self.editor.selected.as_deref());
                self.status = "Parent updated · World transform preserved".into();
                self.error = false;
            }
            self.result(result);
        }
        if matches == 0 {
            ui.weak(if query.is_empty() {
                "Scene is empty. Add a Cube or Sprite above."
            } else {
                "No matching objects. Clear search to see all."
            });
        } else if !query.is_empty() {
            ui.weak(format!("{matches} of {} objects", scene.objects.len()));
        }
    }
    fn assets_panel(&mut self, ui: &mut egui::Ui) {
        if !self.workspace.assets_visible {
            return;
        }
        egui::Panel::bottom("content-browser")
            .default_size(220.0)
            .min_size(150.0)
            .max_size(520.0)
            .resizable(true)
            .show(ui, |ui| {
                if self.loading.is_some() {
                    ui.disable();
                }
                let output = self.asset_browser.ui(ui, &mut self.editor);
                if let Some(command) = output.prefab_requested {
                    self.start_prefab(command);
                }
                if output.import_requested {
                    self.dialog = Some(files::Dialog::new(files::Kind::Import, &self.editor.path));
                }
                if output.reload_requested {
                    self.reload_paused = false;
                    self.residency.retry_failed();
                    self.last_assets = Instant::now() - Duration::from_secs(1);
                    let result = self.sync_assets();
                    self.result(result);
                }
                if let Some(layer) = output.added_layer {
                    self.workspace.layer_2d = layer == Layer::TwoD;
                    self.hierarchy_search.clear();
                }
                if let Some(status) = output.status {
                    self.status = status.message;
                    self.error = status.error;
                }
            });
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.loading.is_some()
            || self.dialog.is_some()
            || self.confirm_discard
            || self.drag.is_some()
        {
            return;
        }
        if self.hierarchy_rename.is_some() {
            return;
        }
        if self.editor.play.is_none()
            && !self.workspace.blueprints_visible
            && !self.mouse_captured
            && !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::F2)
                    || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
            })
        {
            self.begin_hierarchy_rename();
            return;
        }
        if self.editor.play.is_some()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.gameplay_controls.reset();
            self.editor.stop_play();
            self.status = "Simulation stopped · Back to editing".into();
            self.error = false;
            return;
        }
        if !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.save_scene(self.editor.path.clone());
        }
        if !ctx.egui_wants_keyboard_input() && self.editor.play.is_none() && !self.mouse_captured {
            // Active navigation and popup editors own Escape before selection does.
            // Drag/rename/dialog handling is already guarded above.
            if self.escape_deselect_requested
                && !self.workspace.blueprints_visible
                && self.editor.selected.is_some()
                && !self.fly_latched
                && self.navigation_button.is_none()
                && !egui::Popup::is_any_open(ctx)
                && ctx.input(|i| i.focused && !i.pointer.any_down())
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                self.escape_deselect_requested = false;
                self.editor.finish_gesture();
                self.editor.select_object(None);
                self.hierarchy_frame_requested = false;
                self.status = "Selection cleared".into();
                self.error = false;
                return;
            }
            if ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::F,
                )
            }) {
                self.hierarchy_frame_requested = self.editor.selected_object().is_some();
            }
            // egui accepts extra Shift for Cmd+Z: consume Redo before Undo.
            if ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                )
            }) {
                let r = self.editor.redo();
                self.result(r);
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                let r = self.editor.undo();
                self.result(r);
            }
            if !self.workspace.blueprints_visible
                && self.editor.selected_object().is_some()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::D))
            {
                let r = self.editor.duplicate();
                if r.is_ok() {
                    self.hierarchy_search.clear();
                }
                self.result(r);
            }
            if !self.workspace.blueprints_visible
                && self.editor.selected_object().is_some()
                && ctx.input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                        || (cfg!(target_os = "macos")
                            && i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace))
                })
            {
                let r = self.editor.delete();
                self.result(r);
            }
        }
    }
}
fn requests_escape_deselect(ctx: &egui::Context, input: &egui::RawInput) -> bool {
    // Read focus before egui's begin_pass clears it on Escape. A held key must not
    // deselect on a later repeat after first cancelling navigation or another edit.
    !ctx.egui_wants_keyboard_input() && input.events.iter().any(|event| {
        matches!(event,
            egui::Event::Key { key: egui::Key::Escape, pressed: true, repeat: false, modifiers, .. }
                if *modifiers == egui::Modifiers::NONE
        )
    })
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        self.escape_deselect_requested = requests_escape_deselect(ctx, input);
        let pointer = input
            .events
            .iter()
            .rev()
            .find_map(|event| {
                if let egui::Event::PointerMoved(pos) = event {
                    Some(*pos)
                } else {
                    None
                }
            })
            .or_else(|| ctx.input(|i| i.pointer.hover_pos()));
        let over_viewport = self
            .viewport_rect
            .zip(pointer)
            .is_some_and(|(rect, pos)| rect.contains(pos));
        self.gameplay_controls.prepare(
            input,
            ctx.input(|i| i.modifiers),
            over_viewport
                && !ctx.egui_wants_keyboard_input()
                && self.loading.is_none()
                && self.dialog.is_none()
                && !self.confirm_discard
                && !self.workspace.blueprints_visible
                && (!self.workspace.layer_2d
                    || self
                        .editor
                        .play
                        .as_ref()
                        .is_some_and(|p| p.instance.has_blueprints())),
            self.editor.play.as_mut(),
        );
        let eligible = input.focused
            && self.editor.play.is_none()
            && !self.workspace.layer_2d
            && self.loading.is_none()
            && self.dialog.is_none()
            && !self.confirm_discard
            && self.drag.is_none()
            && (self.fly_latched || (over_viewport && !ctx.egui_wants_keyboard_input()));
        if viewport::filter_fly_tab(
            input,
            eligible,
            &mut self.fly_latched,
            &mut self.fly_tab_down,
        ) {
            self.navigation_button = None;
            self.status = if self.fly_latched {
                "Fly mode · Tab or Esc to release"
            } else {
                "Camera released"
            }
            .into();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_loading();
        self.editor.repair_surface_selection();
        let now = Instant::now();
        self.editor.advance(now.duration_since(self.last_frame));
        if let Some(play) = &self.editor.play
            && let Err(error) = play.check_simulation()
        {
            self.result(Err(error));
        }
        self.last_frame = now;
        if !self.mouse_captured {
            self.shortcuts(&ctx);
        }
        if self.drag.is_none()
            && !ctx.input(|i| i.pointer.any_down())
            && !ctx.egui_wants_keyboard_input()
        {
            self.editor.finish_gesture();
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && let Some(job) = &self.loading
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            job.cancel();
            self.import_queue.clear();
            self.close_after_loading = true;
        } else if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.editor.dirty()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(Pending::Close);
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let drops = ctx.input(|i| i.raw.dropped_files.clone());
        for file in drops {
            {
                let path = file.path().to_path_buf();
                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".blueprint.json"))
                {
                    let result = (|| {
                        ensure!(
                            self.loading.is_none()
                                && self.dialog.is_none()
                                && !self.confirm_discard,
                            "Finish the current operation before loading a blueprint"
                        );
                        ensure!(
                            self.editor.selected_surface().is_none(),
                            "Select the mesh owner first"
                        );
                        let id = self
                            .editor
                            .selected
                            .clone()
                            .context("Select an object before loading a blueprint")?;
                        self.editor.load_blueprint(&id, &path)
                    })();
                    if result.is_ok() {
                        self.open_last_blueprint();
                    }
                    self.result(result);
                } else if path.extension().is_some_and(|e| e == "json") {
                    self.request(Pending::Open(path));
                } else {
                    self.start_import(path);
                }
            }
        }
        self.toolbar(ui);
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(job) = &self.loading {
                    ui.spinner();
                    ui.label(if job.cancelled() {
                        "Cancelling after the current decoding step…".into()
                    } else {
                        job.label()
                    });
                    if !self.import_queue.is_empty() {
                        ui.label(format!("{} imports queued", self.import_queue.len()));
                    }
                    if ui
                        .add_enabled(!job.cancelled(), egui::Button::new("Cancel loading"))
                        .clicked()
                    {
                        job.cancel();
                        self.import_queue.clear();
                    }
                    return;
                }
                // Fast automatic checks are housekeeping, not a new user operation.
                if let Some((id, progress)) = self.residency.progress() {
                    ui.spinner();
                    ui.label(format!(
                        "Uploading {id} · {:.0}%",
                        100.0 * progress.bytes_done as f64 / progress.bytes_total.max(1) as f64
                    ));
                    if ui.button("Cancel this upload").clicked() {
                        self.residency.cancel();
                        self.reload_paused = true;
                        self.status = "GPU upload cancelled · Reload to retry".into();
                    }
                    return;
                }
                if let Some((id, cancelled)) = self.residency.preparing() {
                    ui.spinner();
                    ui.label(format!(
                        "{} GPU resources · {id}",
                        if cancelled { "Cancelling" } else { "Preparing" }
                    ));
                    if ui
                        .add_enabled(!cancelled, egui::Button::new("Cancel this upload"))
                        .clicked()
                    {
                        self.residency.cancel();
                        self.reload_paused = true;
                        self.status = "GPU upload cancelled · Reload to retry".into();
                    }
                    return;
                }
                // Show progress for initial loading or a genuinely slow check only.
                if let Some((_, job)) = &self.refresh
                    && (self.last_assets.elapsed() >= Duration::from_millis(500)
                        || self
                            .editor
                            .assets
                            .entries()
                            .any(|entry| entry.data().is_none()))
                {
                    ui.spinner();
                    ui.label(job.label());
                    if ui
                        .add_enabled(!job.cancelled(), egui::Button::new("Cancel reload"))
                        .clicked()
                    {
                        job.cancel();
                        self.reload_paused = true;
                    }
                    return;
                }
                if self.error {
                    ui.colored_label(Color32::LIGHT_RED, &self.status);
                } else {
                    ui.label(&self.status);
                }
            });
        });
        egui::Panel::left("entity-workspace")
            .default_size(290.0)
            .min_size(260.0)
            .max_size(420.0)
            .resizable(true)
            .show(ui, |ui| {
                egui::Panel::top("entity-tree")
                    .default_size(280.0)
                    .min_size(140.0)
                    .max_size((ui.available_height() * 0.65).max(140.0))
                    .resizable(true)
                    .show(ui, |ui| {
                        // Panels size to their contents; fill the requested height so the
                        // scroll area's reserved footer cannot shrink the split each frame.
                        ui.set_min_height(ui.available_height());
                        self.hierarchy(ui);
                    });
                egui::CentralPanel::default().show(ui, |ui| self.inspector(ui));
            });
        self.assets_panel(ui);
        if self.workspace.settings_visible {
            egui::Panel::right("scene-settings")
                .default_size(280.0)
                .min_size(250.0)
                .max_size(400.0)
                .resizable(true)
                .show(ui, |ui| {
                    if self.loading.is_some() {
                        ui.disable();
                    }
                    theme::panel_title(ui, "Scene Settings");
                    egui::ScrollArea::vertical()
                        .id_salt("scene-settings-scroll")
                        .show(ui, |ui| {
                            self.lighting_inspector(ui);
                        });
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(20, 20, 22))
                    .inner_margin(4),
            )
            .show(ui, |ui| {
                if self.loading.is_some() {
                    ui.disable();
                }
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.workspace.blueprints_visible, false, "Scene");
                    ui.selectable_value(&mut self.workspace.blueprints_visible, true, "Blueprint");
                });
                if self.workspace.blueprints_visible {
                    if let Err(error) = self.sync_assets() {
                        self.result(Err(error));
                    }
                    self.blueprint_ui(ui);
                } else if let Err(error) = self.viewport(ui) {
                    self.result(Err(error));
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        "Viewport unavailable. Check the status message.",
                    );
                }
            });
        self.file_dialog(&ctx);
        self.discard_dialog(&ctx);
        if self.smoke_start.elapsed() > Duration::from_secs(30)
            || (self.loading.is_none() && self.editor.assets.require_ready().is_ok())
        {
            self.smoke_step(&ctx);
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.workspace.scene_path = Some(self.editor.path.clone());
        eframe::set_value(storage, "workspace", &self.workspace);
    }
}

// Finder launches with an unrelated working directory. Use a user-owned location
// and a fresh filename so a new session never silently replaces a saved level.
fn untitled_scene_path() -> Result<PathBuf> {
    let home_key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var_os(home_key).context("user home directory is unavailable")?;
    let directory = PathBuf::from(home)
        .join("Documents")
        .join("Bozzard Projects");
    std::fs::create_dir_all(&directory)?;
    for index in 1u64.. {
        let path = directory.join(format!("untitled-{index}.json"));
        if !path.exists() {
            return Ok(path);
        }
    }
    anyhow::bail!("no available untitled scene filename")
}

fn main() -> Result<()> {
    let mut source = None;
    let mut smoke = None;
    let mut backend = Backend::native();
    let mut software = false;
    let mut hardware = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scene" => source = Some(PathBuf::from(args.next().context("--scene needs a path")?)),
            "--smoke" => {
                smoke = Some(PathBuf::from(
                    args.next().context("--smoke needs an output directory")?,
                ))
            }
            "--backend" => backend = args.next().context("--backend needs a value")?.parse()?,
            "--software" => software = true,
            "--hardware" => hardware = true,
            "--help" => {
                println!(
                    "bozzard-editor [--scene FILE] [--backend metal|vulkan|dx12] [--software|--hardware] [--smoke DIRECTORY]\nNative scene editor. Import PNG/JPEG/OBJ/glTF/GLB, edit objects, save, and use Play/Stop."
                );
                return Ok(());
            }
            _ => anyhow::bail!("unknown argument {arg}"),
        }
    }
    ensure!(
        !(software && hardware),
        "choose software or hardware, not both"
    );
    let editor = if let Some(path) = source {
        let path = std::path::absolute(path)?;
        Editor::new_pending(
            bozzard_scene::Scene::from_json(&std::fs::read_to_string(&path)?)?,
            &path,
        )?
    } else {
        let path = match &smoke {
            Some(directory) => std::path::absolute(directory.join("initial-scene.json"))?,
            None => untitled_scene_path()?,
        };
        Editor::new(bozzard_demo::scene_document()?, &path)?
    };
    if let Some(dir) = &smoke {
        std::fs::create_dir_all(dir)?;
    }
    let instance = bozzard_render::instance(backend);
    let gpu = pollster::block_on(Gpu::request(&instance, None, software))?;
    if hardware {
        gpu.require_hardware()?;
    }
    let existing = eframe::egui_wgpu::WgpuSetupExisting {
        instance,
        adapter: gpu.adapter,
        device: gpu.device,
        queue: gpu.queue,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([960.0, 640.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: existing.into(),
            ..Default::default()
        },
        // eframe chooses the platform application-data directory for normal launches.
        persistence_path: smoke.as_ref().map(|p| p.join("workspace")),
        persist_window: smoke.is_none(),
        ..Default::default()
    };
    let passed = Arc::new(AtomicBool::new(false));
    let result = passed.clone();
    let is_smoke = smoke.is_some();
    eframe::run_native(
        "Bozzard Editor",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, editor, smoke, passed)?))),
    )
    .map_err(|e| anyhow::anyhow!("editor: {e}"))?;
    ensure!(
        !is_smoke || result.load(Ordering::Relaxed),
        "editor smoke run did not complete"
    );
    Ok(())
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    #[test]
    fn workspace_navigation_is_restored_only_for_the_same_scene() {
        let mut workspace: Workspace =
            serde_json::from_str(r#"{"zoom":4.0,"layer_2d":true}"#).unwrap();
        workspace.restore_scene(Path::new("sponza.json"));
        assert_eq!(workspace.zoom, 1.0);
        assert!(!workspace.layer_2d);
        workspace.zoom = 2.0;
        workspace.restore_scene(Path::new("sponza.json"));
        assert_eq!(workspace.zoom, 2.0);
        workspace.restore_scene(Path::new("another.json"));
        assert_eq!(workspace.zoom, 1.0);
        assert!(workspace.camera.is_none());
    }

    #[test]
    fn escape_respects_focus_before_egui_clears_it_and_ignores_repeats() {
        let ctx = egui::Context::default();
        let mut text = String::from("Editing a material value");
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.text_edit_singleline(&mut text).request_focus();
        });
        output.textures_delta.clear();
        assert!(ctx.egui_wants_keyboard_input());
        let escape = |repeat| egui::RawInput {
            focused: true,
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        assert!(!requests_escape_deselect(&ctx, &escape(false)));
        ctx.begin_pass(escape(false));
        assert!(
            !ctx.egui_wants_keyboard_input(),
            "egui drops focus before App::shortcuts runs"
        );
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
        assert!(!requests_escape_deselect(&ctx, &escape(true)));
        assert!(
            requests_escape_deselect(&ctx, &escape(false)),
            "a new press can deselect after editing ends"
        );
        assert_eq!(text, "Editing a material value");
    }
}
