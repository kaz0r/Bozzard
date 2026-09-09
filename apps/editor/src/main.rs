use anyhow::{Context, Result, ensure};
use bozzard_assets::{AssetData, LoadState};
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
mod colliders;
mod files;
mod framing;
mod inspector;
mod loading;
mod snapping;
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
    layer_2d: bool,
    assets_visible: bool,
    colliders_visible: bool,
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
            layer_2d: false,
            assets_visible: true,
            colliders_visible: true,
            tool: Tool::Move,
            snapping: snapping::Snapping::default(),
            pan: [0.0; 2],
            zoom: 1.0,
            camera: None,
            ortho_zoom: 1.0,
        }
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
    target: Option<Target>,
    render_state: eframe::egui_wgpu::RenderState,
    workspace: Workspace,
    status: String,
    hierarchy_search: String,
    asset_browser: asset_browser::AssetBrowser,
    loading: Option<loading::Loading>,
    import_queue: std::collections::VecDeque<PathBuf>,
    refresh: Option<loading::Refresh>,
    reload_paused: bool,
    continue_after_save: bool,
    close_after_loading: bool,
    error: bool,
    last_frame: Instant,
    last_assets: Instant,
    uploaded_revision: u64,
    dialog: Option<files::Dialog>,
    pending: Option<Pending>,
    confirm_discard: bool,
    allow_close: bool,
    drag: Option<viewport::Drag>,
    navigation_button: Option<egui::PointerButton>,
    mouse_captured: bool,
    fly_latched: bool,
    fly_tab_down: bool,
    viewport_rect: Option<Rect>,
    smoke: Option<PathBuf>,
    smoke_passed: Arc<AtomicBool>,
    smoke_frames: u32,
    smoke_start: Instant,
    smoke_requested: bool,
    smoke_expected: Option<bozzard_scene::Scene>,
    smoke_selection: Option<String>,
}
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
        let mut style = (*cc.egui_ctx.style_of(egui::Theme::Dark)).clone();
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.visuals = egui::Visuals::dark();
        style.visuals.selection.bg_fill = Color32::from_rgb(28, 101, 119);
        cc.egui_ctx.set_style_of(egui::Theme::Dark, style);
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        let workspace = if smoke.is_none() {
            cc.storage
                .and_then(|s| eframe::get_value(s, "workspace"))
                .unwrap_or_default()
        } else {
            Workspace::default()
        };
        let renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        Ok(Self {
            editor,
            gpu,
            renderer,
            render_state: state,
            target: None,
            workspace,
            status: "Ready · Select an object to begin".into(),
            hierarchy_search: String::new(),
            asset_browser: asset_browser::AssetBrowser::default(),
            loading: None,
            import_queue: std::collections::VecDeque::new(),
            refresh: None,
            reload_paused: false,
            continue_after_save: false,
            close_after_loading: false,
            error: false,
            last_frame: Instant::now(),
            last_assets: Instant::now() - Duration::from_secs(1),
            uploaded_revision: 0,
            dialog: None,
            pending: None,
            confirm_discard: false,
            allow_close: false,
            drag: None,
            navigation_button: None,
            mouse_captured: false,
            fly_latched: false,
            fly_tab_down: false,
            viewport_rect: None,
            smoke,
            smoke_passed,
            smoke_frames: 0,
            smoke_start: Instant::now(),
            smoke_requested: false,
            smoke_expected: None,
            smoke_selection: None,
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
                    self.refresh = None;
                    self.reload_paused = false;
                    self.workspace.camera = None;
                    self.workspace.ortho_zoom = 1.0;
                    self.uploaded_revision = 0;
                    self.status = "New level · Add a cube or sprite".into();
                }
                None => {}
            }
            Ok(())
        })();
        self.result(result);
    }
    fn sync_assets(&mut self) -> Result<()> {
        if self.uploaded_revision != self.editor.asset_revision() {
            // Document edits need no re-import. Re-uploading the small scene cache also handles undo/catalog changes.
            self.renderer.clear_imported();
            for entry in self.editor.assets.entries() {
                if let Some(data) = entry.data() {
                    upload(&self.gpu, &mut self.renderer, &entry.id, data)?;
                }
            }
            self.uploaded_revision = self.editor.asset_revision();
        }
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
                        upload(
                            &self.gpu,
                            &mut self.renderer,
                            &entry.id,
                            entry.data().context("asset missing data")?,
                        )?;
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
        egui::Panel::top("toolbar").show(ui, |ui| {
            if self.loading.is_some() {
                ui.disable();
            }
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("BOZZARD")
                        .strong()
                        .color(Color32::from_rgb(80, 218, 198)),
                );
                ui.separator();
                if ui.button("New").clicked() {
                    self.request(Pending::New);
                }
                if ui.button("Open…").clicked() {
                    self.dialog = Some(files::Dialog::new(files::Kind::Open, &self.editor.path));
                }
                if ui.button("Save").clicked() {
                    self.save_scene(self.editor.path.clone());
                }
                if ui.button("Save As…").clicked() {
                    self.dialog = Some(files::Dialog::new(files::Kind::Save, &self.editor.path));
                }
                ui.separator();
                let editable = self.editor.play.is_none();
                if ui
                    .add_enabled(
                        editable && self.editor.undo_label().is_some(),
                        egui::Button::new("Undo"),
                    )
                    .on_hover_text(self.editor.undo_label().unwrap_or("Nothing to undo"))
                    .clicked()
                {
                    let r = self.editor.undo();
                    self.result(r);
                }
                if ui
                    .add_enabled(
                        editable && self.editor.redo_label().is_some(),
                        egui::Button::new("Redo"),
                    )
                    .clicked()
                {
                    let r = self.editor.redo();
                    self.result(r);
                }
                ui.separator();
                if editable {
                    if ui.button("▶ Play").clicked() {
                        let r = self.editor.start_play();
                        self.result(r);
                    }
                } else if ui.button("■ Stop").clicked() {
                    self.editor.stop_play();
                }
                ui.separator();
                ui.selectable_value(&mut self.workspace.layer_2d, false, "3D");
                ui.selectable_value(&mut self.workspace.layer_2d, true, "2D");
                ui.checkbox(&mut self.workspace.assets_visible, "Assets");
            });
            ui.horizontal(|ui| {
                let modified = if self.editor.dirty() {
                    " • modified"
                } else {
                    ""
                };
                ui.label(format!("{}{}", self.editor.scene().name, modified));
                ui.weak(self.editor.path.display().to_string());
                if self.editor.play.is_some() {
                    ui.colored_label(
                        Color32::from_rgb(100, 220, 160),
                        "PLAY MODE · authored scene protected",
                    );
                }
            });
        });
    }
    fn hierarchy(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("hierarchy")
            .default_size(225.0)
            .min_size(160.0)
            .max_size(420.0)
            .resizable(true)
            .show(ui, |ui| {
                if self.loading.is_some() { ui.disable(); }
                ui.heading("Hierarchy");
                ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("+ Cube").clicked() {
                            let r = self.editor.create(Mesh::Cube, Layer::ThreeD);
                            if r.is_ok() {
                                self.hierarchy_search.clear();
                                self.workspace.layer_2d = false;
                            }
                            self.result(r);
                        }
                        if ui.button("+ Sprite").clicked() {
                            let r = self.editor.create(Mesh::Quad, Layer::TwoD);
                            if r.is_ok() {
                                self.hierarchy_search.clear();
                                self.workspace.layer_2d = true;
                            }
                            self.result(r);
                        }
                    });
                    ui.horizontal(|ui| {
                        let selected = self.editor.selected_object().is_some();
                        if ui.add_enabled(selected, egui::Button::new("Duplicate"))
                            .on_hover_text("Duplicate selected object and its children · Cmd/Ctrl+D")
                            .clicked() {
                            let r = self.editor.duplicate();
                            if r.is_ok() { self.hierarchy_search.clear(); }
                            self.result(r);
                        }
                        if ui.add_enabled(selected, egui::Button::new("Delete"))
                            .on_hover_text("Delete selected object and its children · Delete key · Undo to restore")
                            .clicked() {
                            let r = self.editor.delete();
                            self.result(r);
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.hierarchy_search)
                            .hint_text("Search name or ID…")
                            .desired_width(ui.available_width() - 28.0),
                    );
                    if ui.small_button("×").on_hover_text("Clear search").clicked() {
                        self.hierarchy_search.clear();
                    }
                });
                let query = self.hierarchy_search.trim().to_lowercase();
                let scene = self.editor.scene().clone();
                let mut stack: Vec<_> = scene
                    .objects
                    .iter()
                    .filter(|o| o.parent.is_none())
                    .rev()
                    .map(|o| (o, 0usize))
                    .collect();
                let mut matches = 0usize;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    while let Some((object, depth)) = stack.pop() {
                        if query.is_empty()
                            || object.name.to_lowercase().contains(&query)
                            || object.id.to_lowercase().contains(&query)
                        {
                            matches += 1;
                            ui.horizontal(|ui| {
                                if query.is_empty() {
                                    ui.add_space((depth.min(12) * 12) as f32);
                                }
                                let kind = if object.camera.is_some() {
                                    "◉"
                                } else if object.drawable.is_some() {
                                    "◇"
                                } else {
                                    "·"
                                };
                                if ui
                                    .selectable_label(
                                        self.editor.selected.as_ref() == Some(&object.id),
                                        format!("{kind} {}", object.name),
                                    )
                                    .on_hover_text(&object.id)
                                    .clicked()
                                {
                                    self.editor.finish_gesture();
                                    self.editor.selected = Some(object.id.clone());
                                }
                            });
                        }
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
                if matches == 0 {
                    ui.weak(if query.is_empty() {
                        "Scene is empty. Add a Cube or Sprite above."
                    } else {
                        "No matching objects. Clear search to see all."
                    });
                } else if !query.is_empty() {
                    ui.weak(format!("{matches} of {} objects", scene.objects.len()));
                }
            });
    }
    fn assets_panel(&mut self, ui: &mut egui::Ui) {
        if !self.workspace.assets_visible {
            return;
        }
        egui::Panel::bottom("assets")
            .default_size(260.0)
            .min_size(180.0)
            .max_size(520.0)
            .resizable(true)
            .show(ui, |ui| {
                if self.loading.is_some() {
                    ui.disable();
                }
                let output = self.asset_browser.ui(ui, &mut self.editor);
                if output.import_requested {
                    self.dialog = Some(files::Dialog::new(files::Kind::Import, &self.editor.path));
                }
                if output.reload_requested {
                    self.reload_paused = false;
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
        if self.editor.play.is_some()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
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
        if !ctx.egui_wants_keyboard_input() && self.editor.play.is_none() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                let r = self.editor.undo();
                self.result(r);
            }
            if ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                )
            }) {
                let r = self.editor.redo();
                self.result(r);
            }
            if self.editor.selected_object().is_some()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::D))
            {
                let r = self.editor.duplicate();
                if r.is_ok() {
                    self.hierarchy_search.clear();
                }
                self.result(r);
            }
            if self.editor.selected_object().is_some()
                && ctx.input(|i| i.key_pressed(egui::Key::Delete))
            {
                let r = self.editor.delete();
                self.result(r);
            }
        }
    }
}
impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
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
        let now = Instant::now();
        self.editor.advance(now.duration_since(self.last_frame));
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
                if path.extension().is_some_and(|e| e == "json") {
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
        self.assets_panel(ui);
        self.hierarchy(ui);
        self.inspector(ui);
        egui::CentralPanel::default().show(ui, |ui| {
            if self.loading.is_some() {
                ui.disable();
            }
            if let Err(error) = self.viewport(ui) {
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
        eframe::set_value(storage, "workspace", &self.workspace);
    }
}
fn upload(gpu: &Gpu, renderer: &mut SceneRenderer, id: &str, data: &AssetData) -> Result<()> {
    match data {
        AssetData::Image(i) => renderer.upload_image(gpu, id, i.width, i.height, &i.rgba),
        AssetData::Mesh(m) => {
            let parts: Vec<_> = m
                .parts
                .iter()
                .map(|part| bozzard_render::ModelPart {
                    start: part.start,
                    count: part.count,
                    color: part.color,
                    alpha_cutoff: part.alpha_cutoff,
                    image: part.image.as_ref().map(|image| bozzard_render::ModelImage {
                        width: image.width,
                        height: image.height,
                        rgba: &image.rgba,
                    }),
                })
                .collect();
            renderer.upload_model(gpu, id, &m.vertices, &m.indices, &parts)
        }
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
