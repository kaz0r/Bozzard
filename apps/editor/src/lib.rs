use anyhow::{Context, Result, ensure};
use bozzard_assets::LoadState;
use bozzard_editor::Editor;
use bozzard_render::{Backend, Gpu, SceneRenderer, wgpu};
use bozzard_scene::{AssetKind, Camera, Layer, Mesh, Spin, Texture, Transform};
pub use eframe::egui;
use eframe::egui::{Color32, Pos2, Rect, Sense, Vec2};
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
mod animation_ui;
mod asset_browser;
mod blueprint_debug;
mod blueprints;
mod cameras;
mod colliders;
mod component_ui;
mod compute_ui;
mod content;
pub mod custom_inspectors;
mod debug;
mod docking;
mod export;
mod files;
mod fog;
mod font_ui;
mod framing;
mod gameplay_input;
mod gi;
mod hierarchy;
mod inspector;
mod level_tools;
mod lights;
mod loading;
mod lod_ui;
mod material_ui;
mod motion_ui;
mod multi_inspector;
mod multi_scene;
mod navigation_ui;
mod particle_ui;
mod post_processing;
mod project_ui;
mod repaint;
mod scripts;
mod shaders;
mod snapping;
mod sprite_ui;
mod surfaces;
mod theme;
mod viewport;
mod widget_ui;

fn clipped_selectable_row(ui: &mut egui::Ui, selected: bool, label: String) -> egui::Response {
    ui.add(
        egui::Button::selectable(selected, label)
            .truncate()
            .min_size(egui::vec2(
                ui.available_width().max(1.0),
                ui.spacing().interact_size.y,
            )),
    )
}

fn left_aligned_hierarchy_row<R>(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ui.spacing().interact_size.y.max(24.0)),
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min),
        add_contents,
    )
}

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
    hierarchy_visible: bool,
    inspector_visible: bool,
    docking: docking::Layout,
    settings_visible: bool,
    effects_page: bool,
    blueprints_visible: bool,
    shaders_visible: bool,
    stats_visible: bool,
    occlusion_enabled: bool,
    debug_visible: bool,
    compute_visible: bool,
    gpu_memory_mib: u32,
    blueprint_debug: blueprint_debug::Preferences,
    colliders_visible: bool,
    gi_visible: bool,
    tool: Tool,
    snapping: snapping::Snapping,
    level: level_tools::Preferences,
    pan: [f32; 2],
    zoom: f32,
    camera: Option<viewport::FlyCamera>,
    ortho_zoom: f32,
    canvas_preview: [u32; 2],
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            scene_path: None,
            layer_2d: false,
            assets_visible: true,
            hierarchy_visible: true,
            inspector_visible: true,
            docking: Default::default(),
            settings_visible: true,
            effects_page: true,
            blueprints_visible: false,
            shaders_visible: false,
            stats_visible: false,
            occlusion_enabled: true,
            debug_visible: false,
            compute_visible: false,
            gpu_memory_mib: 512,
            blueprint_debug: Default::default(),
            colliders_visible: true,
            gi_visible: false,
            tool: Tool::Move,
            snapping: snapping::Snapping::default(),
            level: Default::default(),
            pan: [0.0; 2],
            zoom: 1.0,
            camera: None,
            ortho_zoom: 1.0,
            canvas_preview: [0, 0],
        }
    }
}

impl Workspace {
    fn select_available_view(&mut self, scene: &bozzard_scene::Scene) {
        let preferred = if self.layer_2d {
            Layer::TwoD
        } else {
            Layer::ThreeD
        };
        if !scene.views.contains_key(&preferred) {
            self.layer_2d = scene.views.contains_key(&Layer::TwoD);
        }
    }
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
    debug: debug::DebugWorkspace,
    editor: Editor,
    #[cfg(feature = "factory")]
    factory_mode: bool,
    #[cfg(feature = "factory")]
    factory_module: Option<bozz_torio::module::FactoryModule>,
    #[cfg(feature = "factory")]
    factory_cell: [u16; 2],
    #[cfg(feature = "factory")]
    factory_kind: bozz_torio::sim::Kind,
    #[cfg(feature = "factory")]
    factory_facing: bozz_torio::sim::Direction,
    open_scenes: bozzard_editor::OpenScenes,
    close_discarded: std::collections::BTreeSet<bozzard_editor::SceneId>,
    gpu: Gpu,
    renderer: SceneRenderer,
    compute: bozzard_render_assets::ComputeBridge,
    compute_pane: compute_ui::Pane,
    residency: bozzard_render_assets::Residency,
    audio: bozzard_audio::NativeAudio,
    target: Option<Target>,
    viewport_stamp: Option<repaint::ViewportStamp>,
    viewport_continuous: bool,
    viewport_draws: u64,
    viewport_reuses: u64,
    render_state: eframe::egui_wgpu::RenderState,
    workspace: Workspace,
    status: String,
    hierarchy_search: String,
    surface_search: String,
    hierarchy_state: hierarchy::HierarchyState,
    multi_inspector_cache: Option<multi_inspector::Cache>,
    hierarchy_frame_requested: bool,
    hierarchy_rename: Option<(String, String, bool)>,
    asset_browser: asset_browser::AssetBrowser,
    lod_tools: lod_ui::LodTools,
    blueprint_pane: blueprints::BlueprintPane,
    script_pane: scripts::ScriptPane,
    blueprint_debug: blueprint_debug::Workspace,
    shader_pane: shaders::ShaderPane,
    material_pane: material_ui::MaterialPane,
    level_tools: level_tools::LevelTools,
    custom_inspectors: custom_inspectors::Registry,
    dock_focus: Option<docking::Pane>,
    preview_target: Option<Target>,
    preview_time: f32,
    loading: Option<loading::Loading>,
    import_queue: std::collections::VecDeque<PathBuf>,
    refresh: Option<loading::Refresh>,
    last_refreshed_scene: bozzard_editor::SceneId,
    force_reload: bool,
    reload_paused: bool,
    continue_after_save: bool,
    close_after_loading: bool,
    error: bool,
    last_frame: Instant,
    effects_preview: Option<bozzard_editor::EffectsPreview>,
    preview_running: bool,
    preview_bypass: bool,
    last_assets: Instant,
    dialog: Option<files::Dialog>,
    file_browser: files::Browser,
    export_parent: Option<PathBuf>,
    pending: Option<Pending>,
    confirm_discard: bool,
    allow_close: bool,
    drag: Option<viewport::Drag>,
    canvas_drag: Option<viewport::CanvasDrag>,
    navigation_button: Option<egui::PointerButton>,
    mouse_captured: bool,
    escape_deselect_requested: bool,
    fly_latched: bool,
    fly_tab_down: bool,
    viewport_rect: Option<Rect>,
    viewport_layer: egui::LayerId,
    gameplay_controls: gameplay_input::GameplayControls,
    join_lobby: String,
    pending_lobby: Option<u64>,
    smoke: Option<PathBuf>,
    smoke_passed: Arc<AtomicBool>,
    smoke_frames: u32,
    smoke_repaint: acceptance::RepaintSmoke,
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
    smoke_export_started: bool,
}
#[derive(Clone)]
struct HierarchyDrag(String);

enum Pending {
    Open(PathBuf),
    New,
    Close,
    CloseScene,
}
impl App {
    fn stop_play(&mut self) {
        self.editor.stop_play();
        #[cfg(feature = "factory")]
        {
            self.factory_module = None;
        }
    }
    fn new(
        cc: &eframe::CreationContext<'_>,
        editor: Editor,
        smoke: Option<PathBuf>,
        smoke_passed: Arc<AtomicBool>,
        custom_inspectors: custom_inspectors::Registry,
    ) -> Result<Self> {
        let state = cc
            .wgpu_render_state
            .clone()
            .context("editor requires native WebGPU")?;
        let gpu = Gpu::from_device(
            state.adapter.clone(),
            state.device.clone(),
            state.queue.clone(),
        );
        theme::install(&cc.egui_ctx);
        let mut workspace: Workspace = if smoke.is_none() {
            cc.storage
                .and_then(|s| eframe::get_value(s, "workspace"))
                .unwrap_or_default()
        } else {
            Workspace::default()
        };
        workspace.restore_scene(&editor.path);
        workspace.select_available_view(editor.scene());
        let renderer = SceneRenderer::new(&gpu, wgpu::TextureFormat::Rgba8UnormSrgb);
        let compute = bozzard_render_assets::ComputeBridge::new(&gpu);
        Ok(Self {
            debug: Default::default(),
            editor,
            #[cfg(feature = "factory")]
            factory_mode: false,
            #[cfg(feature = "factory")]
            factory_module: None,
            #[cfg(feature = "factory")]
            factory_cell: [
                (bozz_torio::sim::PATCH_X + 4) as u16,
                (bozz_torio::sim::PATCH_Y + 7) as u16,
            ],
            #[cfg(feature = "factory")]
            factory_kind: bozz_torio::sim::Kind::Miner,
            #[cfg(feature = "factory")]
            factory_facing: bozz_torio::sim::Direction::East,
            open_scenes: Default::default(),
            close_discarded: Default::default(),
            gpu,
            renderer,
            compute,
            compute_pane: Default::default(),
            residency: bozzard_render_assets::Residency::default(),
            audio: Default::default(),
            render_state: state,
            target: None,
            viewport_stamp: None,
            viewport_continuous: false,
            viewport_draws: 0,
            viewport_reuses: 0,
            workspace,
            status: "Ready · Select an object to begin".into(),
            hierarchy_search: String::new(),
            surface_search: String::new(),
            hierarchy_state: hierarchy::HierarchyState::default(),
            multi_inspector_cache: None,
            hierarchy_frame_requested: false,
            hierarchy_rename: None,
            asset_browser: asset_browser::AssetBrowser::default(),
            lod_tools: lod_ui::LodTools::default(),
            blueprint_pane: blueprints::BlueprintPane::default(),
            script_pane: Default::default(),
            blueprint_debug: Default::default(),
            shader_pane: shaders::ShaderPane::default(),
            material_pane: material_ui::MaterialPane::default(),
            level_tools: Default::default(),
            custom_inspectors,
            dock_focus: None,
            preview_target: None,
            preview_time: 0.,
            loading: None,
            import_queue: std::collections::VecDeque::new(),
            refresh: None,
            last_refreshed_scene: 0,
            force_reload: false,
            reload_paused: false,
            continue_after_save: false,
            close_after_loading: false,
            error: false,
            last_frame: Instant::now(),
            effects_preview: None,
            preview_running: true,
            preview_bypass: false,
            last_assets: Instant::now() - Duration::from_secs(1),
            dialog: None,
            file_browser: Default::default(),
            export_parent: None,
            pending: None,
            confirm_discard: false,
            allow_close: false,
            drag: None,
            canvas_drag: None,
            navigation_button: None,
            mouse_captured: false,
            escape_deselect_requested: false,
            fly_latched: false,
            fly_tab_down: false,
            viewport_rect: None,
            viewport_layer: egui::LayerId::background(),
            gameplay_controls: gameplay_input::GameplayControls::default(),
            join_lobby: String::new(),
            pending_lobby: None,
            smoke,
            smoke_passed,
            smoke_frames: 0,
            smoke_repaint: Default::default(),
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
            smoke_export_started: false,
        })
    }
    fn result(&mut self, result: Result<()>) {
        if let Err(error) = result {
            self.status = format!("{error:#}");
            self.error = true;
            self.debug.console.push(
                bozzard_diagnostics::Level::Error,
                "Editor",
                &self.status,
                bozzard_diagnostics::Location::default(),
                None,
            );
            self.debug.remember_status(&self.status);
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
            self.open_scenes.validate_save_path(&self.editor, &path)?;
            self.drag = None;
            self.loading = Some(loading::Loading::Save(self.editor.save_job(path)?));
            Ok(())
        })();
        self.result(result);
    }
    fn request(&mut self, pending: Pending) {
        if matches!(pending, Pending::Close) && self.level_tools.dirty() {
            self.level_tools.visible = true;
            self.status = "Apply or discard the terrain draft, or finish/cancel level preparation before quitting".into();
            return;
        }
        if matches!(pending, Pending::Close) && self.material_pane.dirty() {
            self.status = "Save or discard the open material draft before quitting".into();
            return;
        }
        if matches!(pending, Pending::Close) && self.script_pane.dirty() {
            self.dock_focus = Some(docking::Pane::Script);
            self.status = "Save or discard the open script draft before quitting".into();
            return;
        }
        if self.loading.is_some() {
            self.status = "Wait for loading to finish or cancel it first".into();
            return;
        }
        self.editor.finish_gesture();
        self.drag = None;
        self.stop_play();
        if matches!(pending, Pending::Close) {
            self.close_discarded.clear();
            let dirty = self
                .open_scenes
                .documents(&self.editor)
                .find_map(|(id, e)| e.dirty().then_some(id));
            if let Some(id) = dirty {
                let result = self.open_scenes.activate(&mut self.editor, id);
                self.result(result);
                self.scene_activated(false);
            }
        }
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
                Some(Pending::Close) => {
                    self.close_discarded.insert(self.open_scenes.active());
                    let next = self
                        .open_scenes
                        .documents(&self.editor)
                        .find_map(|(id, e)| {
                            (e.dirty() && !self.close_discarded.contains(&id)).then_some(id)
                        });
                    if let Some(id) = next {
                        self.open_scenes.activate(&mut self.editor, id)?;
                        self.scene_activated(false);
                        self.pending = Some(Pending::Close);
                        self.confirm_discard = true;
                    } else {
                        self.allow_close = true;
                    }
                }
                Some(Pending::CloseScene) => {
                    let id = self.open_scenes.active();
                    self.open_scenes.discard_and_close(&mut self.editor, id)?;
                    self.scene_activated(true);
                }
                Some(Pending::Open(path)) => {
                    ensure!(self.loading.is_none(), "Wait for loading to finish");
                    self.loading = Some(loading::Loading::Open(Editor::open_job(path)?));
                }
                Some(Pending::New) => {
                    let mut scene = bozzard_demo::scene_document()?;
                    scene.name = "Untitled level".into();
                    scene.objects.retain(|o| o.camera.is_some());
                    let path = untitled_scene_path()?;
                    self.open_scenes
                        .replace(&mut self.editor, Editor::new(scene, &path)?)?;
                    self.scene_activated(false);
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
            && (self.force_reload || self.last_assets.elapsed() > Duration::from_millis(500))
        {
            self.last_assets = Instant::now();
            let owner = if self.force_reload || self.editor.play.is_some() {
                self.open_scenes.active()
            } else {
                let ids: Vec<_> = self
                    .open_scenes
                    .documents(&self.editor)
                    .map(|(id, _)| id)
                    .collect();
                ids.iter()
                    .copied()
                    .filter(|id| *id > self.last_refreshed_scene)
                    .min()
                    .unwrap_or_else(|| *ids.iter().min().unwrap())
            };
            let document = self.open_scenes.document(&self.editor, owner).unwrap();
            self.refresh = Some(loading::Refresh {
                owner,
                workspace: self.open_scenes.revision(),
                revision: document.asset_revision(),
                job: document.assets.refresh_job_forced(self.force_reload)?,
            });
            self.force_reload = false;
            self.last_refreshed_scene = owner;
        }
        // Keep the catalog snapshot stable while an import/save is prepared.
        if self.loading.is_some() {
            return Ok(());
        }
        if let Some((owner, workspace, revision, cancelled, result)) =
            self.refresh.as_ref().and_then(|refresh| {
                refresh.job.poll().map(|result| {
                    (
                        refresh.owner,
                        refresh.workspace,
                        refresh.revision,
                        refresh.job.cancelled(),
                        result,
                    )
                })
            })
        {
            self.refresh = None;
            self.last_assets = Instant::now();
            if cancelled || workspace != self.open_scenes.revision() {
                if self.reload_paused {
                    self.status =
                        "Reload cancelled · Click Reload to resume automatic refresh".into();
                    self.error = false;
                }
                return Ok(());
            }
            let Some(document) = self.open_scenes.document_mut(&mut self.editor, owner) else {
                return Ok(());
            };
            if revision != document.asset_revision() {
                return Ok(());
            }
            let (store, changed) = result?;
            let initial_load = document
                .assets
                .entries()
                .any(|entry| entry.data().is_none());
            document.assets = store;
            document.refresh_audio_metadata()?;
            let mut reloaded = Vec::new();
            let mut failure = None;
            for handle in changed {
                let entry = document
                    .assets
                    .get(handle)
                    .context("missing asset handle")?;
                match entry.state() {
                    LoadState::Ready => {
                        if matches!(entry.data(), Some(bozzard_assets::AssetData::Audio(_))) {
                            self.audio.invalidate_assets();
                        }
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
                            if ui.button("New project…").clicked() {
                                self.show_project_wizard();
                                ui.close();
                            }
                            if ui.button("New scene").clicked() {
                                self.request(Pending::New);
                                ui.close();
                            }
                            if ui.button("Open scene…").clicked() {
                                self.dialog =
                                    Some(files::Dialog::new(files::Kind::Open, &self.editor.path));
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    self.editor.play.is_none(),
                                    egui::Button::new("Open scene additively…"),
                                )
                                .clicked()
                            {
                                self.dialog = Some(files::Dialog::new(
                                    files::Kind::OpenAdditive,
                                    &self.editor.path,
                                ));
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    self.open_scenes.len() > 1 && self.editor.play.is_none(),
                                    egui::Button::new("Close active scene"),
                                )
                                .clicked()
                            {
                                self.request(Pending::CloseScene);
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
                            if ui.button("Export game…").clicked() {
                                self.show_export_dialog();
                                ui.close();
                            }
                            if ui.button("Build content pack…").clicked() {
                                let mut dialog =
                                    files::Dialog::new(files::Kind::Bundle, &self.editor.path);
                                dialog.path.clear();
                                dialog.project_name = "Content release".into();
                                self.dialog = Some(dialog);
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
                                            let r = self.duplicate_hierarchy_selection();
                                            if r.is_ok() {
                                                self.hierarchy_search.clear();
                                            }
                                            self.result(r);
                                            ui.close();
                                        }
                                        if ui.button("Delete").clicked() {
                                            let r = self.delete_hierarchy_selection();
                                            self.result(r);
                                            ui.close();
                                        }
                                    },
                                );
                            });
                        });
                        if ui
                            .selectable_label(
                                self.workspace.settings_visible && self.workspace.effects_page,
                                "Effects",
                            )
                            .clicked()
                        {
                            self.workspace.settings_visible = true;
                            self.workspace.effects_page = true;
                            self.dock_focus = Some(docking::Pane::Settings);
                        }
                        if ui
                            .selectable_label(
                                self.workspace.debug_visible,
                                if self.debug.recording {
                                    "Debug · Recording"
                                } else {
                                    "Debug"
                                },
                            )
                            .clicked()
                        {
                            self.workspace.debug_visible = !self.workspace.debug_visible;
                            self.dock_focus = Some(docking::Pane::Debug);
                        }
                        ui.menu_button("View", |ui| {
                            if ui
                                .checkbox(&mut self.workspace.hierarchy_visible, "Hierarchy")
                                .changed()
                            {
                                self.dock_focus = Some(docking::Pane::Hierarchy);
                            }
                            if ui
                                .checkbox(&mut self.workspace.inspector_visible, "Inspector")
                                .changed()
                            {
                                self.dock_focus = Some(docking::Pane::Inspector);
                            }
                            if ui.button("Reset docking layout").clicked() {
                                self.workspace.docking.reset();
                                self.workspace.hierarchy_visible = true;
                                self.workspace.inspector_visible = true;
                                ui.close();
                            }
                            ui.checkbox(&mut self.level_tools.visible, "Level tools");
                            if ui
                                .checkbox(
                                    &mut self.workspace.blueprints_visible,
                                    "Blueprint Editor",
                                )
                                .changed()
                            {
                                self.dock_focus = Some(docking::Pane::Scene);
                            }
                            if ui
                                .checkbox(&mut self.workspace.shaders_visible, "Shader Editor")
                                .changed()
                            {
                                self.dock_focus = Some(docking::Pane::Scene);
                            }
                            if ui
                                .checkbox(&mut self.workspace.assets_visible, "Content Browser")
                                .changed()
                            {
                                self.dock_focus = Some(docking::Pane::Assets);
                            }
                            if ui
                                .checkbox(&mut self.workspace.settings_visible, "Scene Settings")
                                .changed()
                            {
                                self.dock_focus = Some(docking::Pane::Settings);
                            }
                            ui.checkbox(&mut self.workspace.stats_visible, "Renderer statistics");
                            if ui
                                .checkbox(
                                    &mut self.workspace.occlusion_enabled,
                                    "Occlusion culling",
                                )
                                .changed()
                            {
                                self.viewport_stamp = None;
                            }
                            ui.checkbox(
                                &mut self.workspace.compute_visible,
                                "Compute resources and jobs",
                            );
                            if ui
                                .checkbox(
                                    &mut self.workspace.debug_visible,
                                    "Debug · Profiler and Console",
                                )
                                .changed()
                            {
                                self.dock_focus = Some(docking::Pane::Debug);
                            }
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
                            !playing && self.loading.is_none() && !self.editor.is_prefab_source(),
                            egui::Button::new(egui::RichText::new("▶").color(theme::GREEN)),
                        )
                        .on_hover_text("Play active scene · Other open scenes remain in the editor")
                        .clicked()
                    {
                        self.start_play();
                    }
                    if ui
                        .add_enabled(playing, egui::Button::new("■"))
                        .on_hover_text("Stop · Escape")
                        .clicked()
                    {
                        self.gameplay_controls.reset();
                        self.stop_play();
                    }
                    self.blueprint_run_controls(ui);
                    if self
                        .editor
                        .play
                        .as_ref()
                        .is_some_and(|p| p.multiplayer_active())
                    {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.join_lobby)
                                .hint_text("Steam lobby ID")
                                .desired_width(150.),
                        );
                        if ui.button("Join lobby").clicked() {
                            let result = self
                                .join_lobby
                                .trim()
                                .parse::<u64>()
                                .map_err(anyhow::Error::from)
                                .and_then(|id| {
                                    self.editor.play.as_mut().unwrap().join_multiplayer(id)
                                });
                            self.result(result);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(if playing { "PLAY MODE" } else { "EDIT MODE" });
                        ui.colored_label(if playing { theme::GREEN } else { theme::ACCENT }, "●");
                    });
                });
                #[cfg(feature = "factory")]
                self.factory_controls(ui);
            });
        });
    }
    #[cfg(feature = "factory")]
    fn factory_controls(&mut self, ui: &mut egui::Ui) {
        use bozz_torio::{
            multiplayer::Action,
            sim::{Direction, HEIGHT, Kind, WIDTH},
        };
        if !self.factory_mode || self.editor.play.is_none() {
            return;
        }
        let Some(module) = self.factory_module.clone() else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong("Factory");
            ui.label("X");
            ui.add(egui::DragValue::new(&mut self.factory_cell[0]).range(0..=WIDTH as u16 - 1));
            ui.label("Y");
            ui.add(egui::DragValue::new(&mut self.factory_cell[1]).range(0..=HEIGHT as u16 - 1));
            egui::ComboBox::from_id_salt("factory-kind")
                .selected_text(self.factory_kind.name())
                .show_ui(ui, |ui| {
                    for kind in Kind::BUILDABLE {
                        ui.selectable_value(&mut self.factory_kind, kind, kind.name());
                    }
                });
            egui::ComboBox::from_id_salt("factory-facing")
                .selected_text(self.factory_facing.glyph())
                .show_ui(ui, |ui| {
                    for direction in [
                        Direction::North,
                        Direction::East,
                        Direction::South,
                        Direction::West,
                    ] {
                        ui.selectable_value(&mut self.factory_facing, direction, direction.glyph());
                    }
                });
            let [x, y] = self.factory_cell;
            if ui.button("Build").clicked() {
                self.result(module.enqueue(Action::Place {
                    x,
                    y,
                    kind: self.factory_kind,
                    direction: self.factory_facing,
                }));
            }
            if ui.button("Recover").clicked() {
                self.result(module.enqueue(Action::Remove { x, y }));
            }
            if ui.button("Rotate").clicked() {
                self.result(module.enqueue(Action::Rotate { x, y }));
            }
            let game = module.game();
            ui.weak(format!(
                "{} / {} delivered · {} credits",
                game.order_progress,
                game.order().amount,
                game.credits
            ));
            if let Some(error) = module.error() {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        });
    }
    fn assets_content(&mut self, ui: &mut egui::Ui) {
        if self.loading.is_some() {
            ui.disable();
        }
        let output = self.asset_browser.ui(ui, &mut self.editor);
        if output.material_create || output.material_variant.is_some() {
            let result = self
                .editor
                .create_material(output.material_variant.as_deref())
                .and_then(|id| self.material_pane.open(&self.editor, &id));
            self.result(result);
        }
        if let Some(id) = output.material_opened {
            let result = self.material_pane.open(&self.editor, &id);
            self.result(result);
        }
        if let Some(command) = output.prefab_requested {
            self.start_prefab(command);
        }
        if let Some(owner) = output.blueprint_owner {
            self.editor.select_object(Some(owner));
            self.open_last_blueprint();
        }
        if output.blueprint_import_requested {
            self.blueprint_dialog(files::Kind::LoadBlueprint);
        }
        if output.blueprint_opened
            && let Some(object) = self.editor.selected_object().cloned()
        {
            let mut attachments = object.blueprints;
            attachments.push(bozzard_scene::BlueprintAttachment {
                enabled: true,
                graph: Default::default(),
            });
            self.editor.finish_gesture();
            let result = self.editor.set_blueprints(&object.id, attachments);
            self.result(result);
            self.open_last_blueprint();
        }
        if let Some(path) = output.blueprint_path
            && let Some(owner) = self.editor.selected.clone()
        {
            let result = self.editor.load_blueprint(&owner, &path);
            self.result(result);
            self.open_last_blueprint();
        }
        if let Some(owner) = output.shader_owner {
            self.editor.select_object(Some(owner));
            self.workspace.shaders_visible = true;
            self.dock_focus = Some(docking::Pane::Scene);
        }
        if output.shader_import_requested
            && let Some(owner) = self.editor.selected.clone()
        {
            self.shader_dialog(files::Kind::LoadShaderGraph, &owner);
        }
        if let Some(path) = output.shader_path
            && let Some(owner) = self.editor.selected.clone()
        {
            let result = self.editor.load_shader_graph(&owner, &path);
            self.result(result);
            self.workspace.shaders_visible = true;
            self.dock_focus = Some(docking::Pane::Scene);
        }
        if output.import_requested {
            self.dialog = Some(files::Dialog::new(files::Kind::Import, &self.editor.path));
        }
        if output.reload_requested {
            self.reload_paused = false;
            self.residency.retry_failed();
            self.last_assets = Instant::now() - Duration::from_secs(1);
            self.force_reload = true;
            if let Some(refresh) = &self.refresh {
                refresh.job.cancel();
            }
        }
        if let Some(layer) = output.added_layer {
            self.workspace.layer_2d = layer == Layer::TwoD;
            self.hierarchy_search.clear();
        }
        if let Some(status) = output.status {
            self.status = status.message;
            self.error = status.error;
        }
    }
    fn settings_content(&mut self, ui: &mut egui::Ui) {
        if self.loading.is_some() {
            ui.disable();
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.workspace.effects_page, true, "Effects");
            ui.selectable_value(&mut self.workspace.effects_page, false, "Scene Settings");
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("scene-settings-scroll")
            .show(ui, |ui| {
                if self.workspace.effects_page {
                    self.effects_inspector(ui);
                } else {
                    self.lighting_inspector(ui);
                }
            });
    }
    fn scene_content(&mut self, ui: &mut egui::Ui) {
        if self.loading.is_some() {
            ui.disable();
        }
        ui.horizontal(|ui| {
            let scene = ui.selectable_label(
                !self.workspace.blueprints_visible && !self.workspace.shaders_visible,
                "Scene",
            );
            if scene.clicked() {
                self.workspace.blueprints_visible = false;
                self.workspace.shaders_visible = false;
            }
            let blueprint = ui.selectable_label(
                self.workspace.blueprints_visible && !self.workspace.shaders_visible,
                "Blueprint",
            );
            if blueprint.clicked() {
                self.workspace.blueprints_visible = true;
                self.dock_focus = Some(docking::Pane::Scene);
                self.workspace.shaders_visible = false;
            }
            let shader = ui.selectable_label(self.workspace.shaders_visible, "Shader");
            if shader.clicked() {
                self.workspace.shaders_visible = true;
                self.dock_focus = Some(docking::Pane::Scene);
                self.workspace.blueprints_visible = false;
            }
        });
        if self.workspace.shaders_visible {
            if let Err(error) = self.sync_assets() {
                self.result(Err(error));
            }
            self.shader_ui(ui);
        } else if self.workspace.blueprints_visible {
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
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.loading.is_some()
            || self.dialog.is_some()
            || self.confirm_discard
            || self.asset_browser.confirming_delete()
            || self.drag.is_some()
        {
            return;
        }
        if self.hierarchy_rename.is_some() {
            return;
        }
        if self.workspace.assets_visible
            && !self.mouse_captured
            && self.asset_browser.delete_shortcut(ctx, &self.editor)
        {
            return;
        }
        if self.editor.play.is_none()
            && !self.workspace.blueprints_visible
            && !self.workspace.shaders_visible
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
        if self
            .editor
            .play
            .as_ref()
            .is_some_and(|p| p.game_session().is_none())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.gameplay_controls.reset();
            self.stop_play();
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
                && !self.workspace.shaders_visible
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
                && !self.workspace.shaders_visible
                && self.editor.selected_object().is_some()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::D))
            {
                let r = self.duplicate_hierarchy_selection();
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
                let r = self.delete_hierarchy_selection();
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
                if let egui::Event::PointerMoved(pos) | egui::Event::PointerButton { pos, .. } =
                    event
                {
                    Some(*pos)
                } else {
                    None
                }
            })
            .or_else(|| ctx.input(|i| i.pointer.hover_pos()));
        let over_viewport = self
            .viewport_rect
            .zip(pointer)
            .is_some_and(|(rect, pos)| viewport::pointer_hits(ctx, rect, self.viewport_layer, pos));
        self.gameplay_controls.prepare(
            input,
            ctx.input(|i| i.modifiers),
            over_viewport
                && !ctx.egui_wants_keyboard_input()
                && self.loading.is_none()
                && self.dialog.is_none()
                && !self.confirm_discard
                && !self.workspace.blueprints_visible
                && !self.workspace.shaders_visible
                && (!self.workspace.layer_2d
                    || self
                        .editor
                        .play
                        .as_ref()
                        .is_some_and(|p| p.instance().has_gameplay_logic())),
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
        let debug_started = self.debug_begin_frame();
        self.poll_loading();
        self.editor.repair_surface_selection();
        let now = Instant::now();
        let debug_interval_ms = now.duration_since(self.last_frame).as_secs_f64() * 1000.;
        if let Some(play) = &mut self.editor.play {
            let refreshed = play.with_instance(|instance, _| {
                instance.set_gpu_particles(true);
                self.compute.prepare(instance);
                self.compute
                    .refresh(&self.gpu, instance, &self.editor.assets)
            });
            if let Err(error) = refreshed {
                self.status = format!("Compute: {error:#}");
                self.error = true;
            }
        }
        match self.compute.poll(&self.gpu) {
            Ok(profiles) => self.debug_compute_profiles(profiles),
            Err(error) => {
                self.status = format!("Compute: {error:#}");
                self.error = true;
            }
        }
        self.prepare_blueprint_debugger();
        let previous_assets = self.editor.asset_revision();
        self.editor.advance(now.duration_since(self.last_frame));
        if previous_assets != self.editor.asset_revision()
            && let Some(play) = &mut self.editor.play
        {
            let result = play.with_instance(|instance, _| {
                self.compute.prepare(instance);
                self.compute
                    .refresh(&self.gpu, instance, &self.editor.assets)
            });
            if let Err(error) = result {
                self.status = format!("Compute: {error:#}");
                self.error = true;
            }
        }
        if let Some(play) = &self.editor.play {
            match self.compute.submit(&self.gpu, play.instance()) {
                Ok(true) => self.viewport_stamp = None,
                Ok(false) => {}
                Err(error) => {
                    self.status = format!("Compute: {error:#}");
                    self.error = true;
                }
            }
        } else {
            self.compute.stop();
        }
        self.compute.sync_renderer(&mut self.renderer);
        self.sync_blueprint_pause();
        if let Some(play) = &self.editor.play {
            let layer = if self.workspace.layer_2d {
                Layer::TwoD
            } else {
                Layer::ThreeD
            };
            let result = play
                .instance()
                .audio_frame(&play.app.world, layer)
                .and_then(|mut frame| {
                    if play.app.is_paused() {
                        for voice in &mut frame.sources {
                            if voice.transport
                                == bozzard_scene::middleware::audio::Transport::Playing
                            {
                                voice.transport =
                                    bozzard_scene::middleware::audio::Transport::Paused;
                            }
                        }
                    }
                    self.audio.sync(
                        &frame,
                        play.instance().document(),
                        bozzard_editor::root(&self.editor.path),
                    )
                });
            if result.is_err() {
                self.result(result);
            }
        } else {
            self.audio.stop();
        }
        if self.editor.play.is_none() && self.loading.is_none() {
            let result = (|| -> Result<()> {
                self.open_scenes.sync_view(&self.editor)?;
                let view = self.open_scenes.view(&self.editor);
                self.workspace.select_available_view(view.scene());
                if self.effects_preview.is_none() {
                    self.effects_preview = Some(
                        bozzard_editor::EffectsPreview::with_gpu_particles(view, true)?,
                    );
                }
                self.effects_preview.as_mut().unwrap().advance(
                    view,
                    now.duration_since(self.last_frame),
                    self.preview_running && !self.workspace.layer_2d,
                )
            })();
            if result.is_err() {
                self.result(result);
            }
        } else {
            self.effects_preview = None;
        }

        if let Some(play) = &self.editor.play
            && let Err(error) = play.check_simulation()
        {
            self.result(Err(error));
        }
        self.last_frame = now;
        if !self.mouse_captured {
            self.blueprint_debug_shortcuts(&ctx);
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
            && (self.open_scenes.any_dirty(&self.editor)
                || self.material_pane.dirty()
                || self.level_tools.dirty()
                || self.script_pane.dirty())
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
                } else if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".material.json"))
                {
                    self.start_import(path);
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
                let memory = self.residency.stats();
                ui.menu_button("GPU assets", |ui| {
                    ui.set_max_width(330.);
                    ui.label(format!(
                        "{} resident assets · {:.1} MiB",
                        memory.resident_assets,
                        memory.resident_bytes as f64 / 1048576.
                    ));
                    ui.label(format!(
                        "Preparing: {:.1} MiB · {} evictions",
                        memory.staged_bytes as f64 / 1048576.,
                        memory.evictions
                    ));
                    ui.horizontal(|ui| {
                        ui.label("Budget (MiB)");
                        ui.add(egui::DragValue::new(&mut self.workspace.gpu_memory_mib).range(1..=32768));
                    });
                    ui.small("Unused assets are evicted first and restored when needed. Visible meshes, their materials and shadow casters stay available. This budget covers imported asset storage.");
                    if memory.over_budget_bytes > 0 {
                        ui.colored_label(
                            Color32::YELLOW,
                            format!(
                                "Assets in use or being prepared exceed the budget by {:.1} MiB. Use smaller textures or lower-detail meshes to reduce this.",
                                memory.over_budget_bytes as f64 / 1048576.
                            )
                        );
                    }
                });
                if let Some(job) = &self.loading {
                    ui.spinner();
                    ui.label(if job.cancelled() {
                        "Cancelling after the current step…".into()
                    } else {
                        job.label()
                    });
                    if job.fraction() > 0. {
                        ui.add(egui::ProgressBar::new(job.fraction()).show_percentage());
                    }
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
                if let Some(refresh) = &self.refresh
                    && (self.last_assets.elapsed() >= Duration::from_millis(500)
                        || self
                            .editor
                            .assets
                            .entries()
                            .any(|entry| entry.data().is_none()))
                {
                    let job = &refresh.job;
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
                    if ui.button("Open console").clicked() {
                        self.workspace.debug_visible = true;
            self.dock_focus = Some(docking::Pane::Debug);
                        self.debug.show_console();
                    }
                } else {
                    ui.label(&self.status);
                }
            });
        });
        let mut layout = std::mem::take(&mut self.workspace.docking);
        if let Some(pane) = self.dock_focus.take() {
            layout.focus(pane);
        }
        let visible = [
            true,
            self.workspace.hierarchy_visible,
            self.workspace.inspector_visible,
            self.workspace.assets_visible,
            self.workspace.settings_visible,
            self.workspace.debug_visible,
            self.script_pane.is_open(),
        ];
        self.viewport_rect = None;
        layout.show(ui, visible, |pane, ui| match pane {
            docking::Pane::Scene => self.scene_content(ui),
            docking::Pane::Hierarchy => self.hierarchy(ui),
            docking::Pane::Inspector => self.inspector(ui),
            docking::Pane::Assets => self.assets_content(ui),
            docking::Pane::Settings => self.settings_content(ui),
            docking::Pane::Debug => self.debug_content(ui),
            docking::Pane::Script => self.script_source_pane(ui),
        });
        self.workspace.docking = layout;
        if let Some(pane) = self.dock_focus.take() {
            self.workspace.docking.focus(pane);
        }
        self.material_pane.ui(
            &ctx,
            &mut self.editor,
            self.loading.is_some() || self.dialog.is_some(),
        );
        self.level_tools.ui(
            &ctx,
            &mut self.editor,
            &mut self.workspace.level,
            self.loading.is_some() || self.dialog.is_some(),
        );
        self.file_dialog(&ctx);
        self.compute_ui(&ctx);
        self.discard_dialog(&ctx);
        if self.smoke_start.elapsed() > Duration::from_secs(30)
            || (self.loading.is_none() && self.editor.assets.require_ready().is_ok())
        {
            self.smoke_step(&ctx);
        }
        let active = self.smoke.is_some()
            || self.compute.executor.has_pending()
            || self.editor.play.is_some()
            || self.loading.is_some()
            || self.refresh.is_some()
            || self.residency.progress().is_some()
            || self.residency.preparing().is_some()
            || self.workspace.shaders_visible
            || (self.viewport_continuous && !self.workspace.blueprints_visible)
            || self.fly_latched
            || self.mouse_captured;
        self.debug_end_frame(debug_started, debug_interval_ms);
        ctx.request_repaint_after(Duration::from_millis(if active || self.debug.recording {
            16
        } else {
            500
        }));
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

/// Start the stock editor, parsing its ordinary command-line options.
pub fn run() -> Result<()> {
    run_with_inspectors(custom_inspectors::Registry::default())
}

/// Start an editor build with registered component inspectors.
pub fn run_with_inspectors(custom_inspectors: custom_inspectors::Registry) -> Result<()> {
    run_with_mode(custom_inspectors, false)
}

#[cfg(feature = "factory")]
pub fn run_factory() -> Result<()> {
    run_with_mode(custom_inspectors::Registry::default(), true)
}

fn run_with_mode(custom_inspectors: custom_inspectors::Registry, factory_mode: bool) -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--runtime-info") {
        println!("{}", bozzard_project::runtime::description());
        return Ok(());
    }
    let mut source = None;
    let mut join_lobby: Option<u64> = None;
    let mut project = None;
    let mut smoke = None;
    let mut backend = Backend::native();
    let mut software = false;
    let mut hardware = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--join-lobby" | "+connect_lobby" => {
                join_lobby = Some(args.next().context("--join-lobby needs an ID")?.parse()?)
            }
            "--scene" => source = Some(PathBuf::from(args.next().context("--scene needs a path")?)),
            "--project" => {
                project = Some(PathBuf::from(
                    args.next().context("--project needs a manifest")?,
                ))
            }
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
                    "bozzard-editor [--scene FILE] [--backend metal|vulkan|dx12] [--software|--hardware] [--smoke DIRECTORY]\nSteam-enabled builds: --join-lobby ID joins on the next Play; Stop leaves the lobby.\nNative scene editor. --project FILE opens a game project. Import assets, edit, Play/Stop, and File > Export game."
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
    ensure!(
        source.is_none() || project.is_none(),
        "--project and --scene are mutually exclusive"
    );
    #[cfg(feature = "factory")]
    if factory_mode && source.is_none() && project.is_none() {
        project = Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bozz-torio/bozzard.project.json"),
        );
    }
    if let Some(path) = project {
        let (project, scene) = bozzard_project::Project::load(&path)?;
        #[cfg(feature = "factory")]
        let available = if factory_mode {
            &[bozz_torio::module::NAME][..]
        } else {
            &[][..]
        };
        #[cfg(not(feature = "factory"))]
        let available: &[&str] = &[];
        project.require_runtime_modules(available)?;
        project.validate_scene(&bozzard_demo::load_document(Some(&scene))?)?;
        source = Some(scene);
    }
    let editor = if let Some(path) = source {
        let path = std::path::absolute(path)?;
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".prefab.json"))
        {
            Editor::open(&path)?
        } else {
            Editor::new_pending(bozzard_demo::load_document(Some(&path))?, &path)?
        }
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
    #[cfg(feature = "steam")]
    if let Err(error) = bozzard_demo::steam_runtime::initialize_editor(editor.scene()) {
        eprintln!("Steam: {error:#}. Editing is available; Play will retry.");
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
        Box::new(move |cc| {
            let mut app = App::new(cc, editor, smoke, passed, custom_inspectors)?;
            #[cfg(feature = "factory")]
            {
                app.factory_mode = factory_mode;
            }
            #[cfg(not(feature = "factory"))]
            let _ = factory_mode;
            app.pending_lobby = join_lobby;
            app.join_lobby = join_lobby.map(|id| id.to_string()).unwrap_or_default();
            Ok(Box::new(app))
        }),
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
    fn clipped_hierarchy_rows_stay_left_aligned_and_within_the_pane() {
        fn first_text_x(shape: &egui::Shape) -> Option<f32> {
            match shape {
                egui::Shape::Text(text) => Some(text.pos.x),
                egui::Shape::Vec(shapes) => shapes.iter().find_map(first_text_x),
                _ => None,
            }
        }

        let ctx = egui::Context::default();
        let long_name = "A very long imported object name ".repeat(20);
        for label in ["Player", long_name.as_str()] {
            let mut row = Rect::NOTHING;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(240.0, 100.0))),
                    ..Default::default()
                },
                |ui| {
                    left_aligned_hierarchy_row(ui, |ui| {
                        ui.add_space(18.0);
                        row = clipped_selectable_row(ui, true, label.to_owned()).rect;
                    });
                },
            );
            let text_x = output
                .shapes
                .iter()
                .find_map(|shape| first_text_x(&shape.shape));
            output.textures_delta.clear();
            let text_x = text_x.expect("row text should be painted");
            assert!(text_x - row.min.x < 20.0, "text was centered in {row:?}");
            assert!(row.max.x <= 240.0, "row exceeded the pane: {row:?}");
        }
    }

    #[test]
    fn hierarchy_eye_aligns_across_leaf_and_parent_rows() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut rows = Vec::new();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(240.0, 100.0))),
                ..Default::default()
            },
            |ui| {
                for (expandable, visible, selected) in [(true, true, true), (false, false, false)] {
                    let row = left_aligned_hierarchy_row(ui, |ui| {
                        let disclosure = hierarchy::disclosure_slot(ui, expandable, false);
                        let eye = ui
                            .add_enabled_ui(true, |ui| hierarchy::visibility_eye(ui, visible))
                            .inner;
                        let label = clipped_selectable_row(ui, selected, "Selected object".into());
                        (disclosure.rect, eye.rect, label.rect)
                    });
                    rows.push((row.response.rect, row.inner));
                }
            },
        );
        output.textures_delta.clear();
        assert_eq!(rows.len(), 2);
        let (parent_row, (parent_disclosure, parent_eye, parent_label)) = rows[0];
        let (leaf_row, (_, leaf_eye, leaf_label)) = rows[1];
        assert_eq!(parent_disclosure.width(), 18.0);
        assert_eq!(
            parent_eye.min.x, leaf_eye.min.x,
            "leaf eye shifted from parent eye"
        );
        for (eye, label) in [(parent_eye, parent_label), (leaf_eye, leaf_label)] {
            assert_eq!(eye.size(), Vec2::splat(24.0), "eye hit target changed size");
            assert_eq!(
                eye.center().y,
                label.center().y,
                "eye is not centered on row label"
            );
        }
        assert!(parent_row.height() >= 24.0);
        assert_eq!(parent_row.height(), leaf_row.height());
        assert!(!output.shapes.is_empty(), "hierarchy rows should paint");

        let mut eye_hovered = false;
        let mut label_hovered = false;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(240.0, 100.0))),
                events: vec![egui::Event::PointerMoved(parent_eye.center())],
                ..Default::default()
            },
            |ui| {
                left_aligned_hierarchy_row(ui, |ui| {
                    hierarchy::disclosure_slot(ui, true, false);
                    eye_hovered = ui
                        .add_enabled_ui(true, |ui| hierarchy::visibility_eye(ui, true))
                        .inner
                        .hovered();
                    label_hovered =
                        clipped_selectable_row(ui, true, "Selected object".into()).hovered();
                });
            },
        );
        output.textures_delta.clear();
        assert!(eye_hovered, "eye should own its hover target");
        assert!(
            !label_hovered,
            "eye hover must not overlap the selectable label"
        );
        let mut eye_clicks = 0;
        let mut label_clicks = 0;
        for pressed in [true, false] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(240.0, 100.0))),
                    events: vec![egui::Event::PointerButton {
                        pos: parent_eye.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    }],
                    ..Default::default()
                },
                |ui| {
                    left_aligned_hierarchy_row(ui, |ui| {
                        hierarchy::disclosure_slot(ui, true, false);
                        eye_clicks += ui
                            .add_enabled_ui(true, |ui| hierarchy::visibility_eye(ui, true))
                            .inner
                            .clicked() as usize;
                        label_clicks += clipped_selectable_row(ui, true, "Selected object".into())
                            .clicked() as usize;
                    });
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(eye_clicks, 1, "one click must toggle visibility once");
        assert_eq!(label_clicks, 0, "the eye must not select or drag the row");
    }

    #[test]
    fn scene_open_selects_an_existing_view_and_preserves_valid_preferences() {
        let mut scene = bozzard_demo::scene_document().unwrap();
        let mut workspace = Workspace::default();
        scene.views.remove(&Layer::ThreeD);
        workspace.select_available_view(&scene);
        assert!(workspace.layer_2d);
        scene = bozzard_demo::scene_document().unwrap();
        workspace.select_available_view(&scene);
        assert!(workspace.layer_2d);
        scene.views.remove(&Layer::TwoD);
        workspace.select_available_view(&scene);
        assert!(!workspace.layer_2d);
    }

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
