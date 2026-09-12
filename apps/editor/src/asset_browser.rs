//! Asset-browser UI state. App owns dialogs, render uploads, and status display.
use bozzard_assets::{AssetData, LoadState};
use bozzard_editor::Editor;
use bozzard_scene::{AssetKind, Layer};
use eframe::egui::{self, Color32, ColorImage, Pos2, Rect, Sense, Stroke, TextureHandle, Vec2};
use std::collections::HashMap;

#[derive(Default)]
pub struct AssetBrowser {
    search: String,
    filter: AssetFilter,
    selected: Option<String>,
    selected_blueprint: Option<std::path::PathBuf>,
    focused: bool,
    pending_delete: Option<PendingDelete>,
    show_details: bool,
    thumbnails: HashMap<String, Thumbnail>,
    catalog_revision: u64,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum AssetFilter {
    #[default]
    All,
    Images,
    Models,
    Prefabs,
    Blueprints,
}

struct Thumbnail {
    asset_revision: u64,
    texture: TextureHandle,
}

#[derive(Default)]
pub struct AssetBrowserOutput {
    pub blueprint_opened: bool,
    pub blueprint_import_requested: bool,
    pub blueprint_path: Option<std::path::PathBuf>,
    pub blueprint_owner: Option<String>,
    pub prefab_requested: Option<bozzard_editor::PrefabCommand>,
    pub import_requested: bool,
    pub reload_requested: bool,
    pub added_layer: Option<Layer>,
    pub status: Option<BrowserStatus>,
}

pub struct BrowserStatus {
    pub message: String,
    pub error: bool,
}

#[derive(Clone)]
pub struct PrefabDrag(pub String);

#[derive(Clone)]
struct AssetSnapshot {
    id: String,
    kind: AssetKind,
    path: String,
    state: LoadState,
    revision: u64,
    users: usize,
    image: Option<(u32, u32)>,
    mesh: Option<MeshPreview>,
    prefab_objects: Option<usize>,
}

#[derive(Clone)]
struct MeshPreview {
    vertex_count: usize,
    index_count: usize,
    vertices: Vec<[f32; 8]>,
    indices: Vec<u32>,
    warnings: Vec<String>,
}

#[derive(Clone)]
enum DeleteTarget {
    Asset(String),
    Blueprint(std::path::PathBuf),
}
struct PendingDelete {
    target: DeleteTarget,
    revision: u64,
    assets: u64,
    scene: std::path::PathBuf,
}

enum AssetCommand {
    Delete(String),
    Add(String),
    Assign(String),
    Remove(String),
    RefreshPrefab(String),
}

impl AssetBrowser {
    pub fn confirming_delete(&self) -> bool {
        self.pending_delete.is_some()
    }
    fn request_delete(&mut self, target: DeleteTarget, editor: &Editor) {
        self.pending_delete = Some(PendingDelete {
            target,
            revision: editor.revision(),
            assets: editor.asset_revision(),
            scene: editor.path.clone(),
        });
    }
    pub fn delete_shortcut(&mut self, ctx: &egui::Context, editor: &Editor) -> bool {
        if !self.focused
            || editor.play.is_some()
            || ctx.egui_wants_keyboard_input()
            || egui::Popup::is_any_open(ctx)
        {
            return false;
        }
        if !ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || (cfg!(target_os = "macos")
                    && i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace))
        }) {
            return false;
        }
        let query = self.search.trim().to_lowercase();
        let target = if self.filter == AssetFilter::Blueprints {
            self.selected_blueprint
                .clone()
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&query))
                })
                .map(DeleteTarget::Blueprint)
        } else {
            self.selected
                .clone()
                .filter(|id| {
                    editor.scene().assets.get(id).is_some_and(|a| {
                        matches_filter(self.filter, a.kind)
                            && (id.to_lowercase().contains(&query)
                                || a.path.to_lowercase().contains(&query))
                    })
                })
                .map(DeleteTarget::Asset)
        };
        if let Some(target) = target {
            self.request_delete(target, editor);
        }
        true // Never let a browser Delete fall through to the scene selection.
    }
    pub fn reveal(&mut self, id: String) {
        self.selected = Some(id);
        self.search.clear();
        self.filter = AssetFilter::All;
    }
    /// Renders the browser and performs document operations through the editor.
    /// The caller handles the two operations that need application services.
    pub fn ui(&mut self, ui: &mut egui::Ui, editor: &mut Editor) -> AssetBrowserOutput {
        if ui.input(|i| i.pointer.any_pressed()) && self.pending_delete.is_none() {
            self.focused = ui.is_enabled()
                && ui.input(|i| {
                    i.pointer
                        .interact_pos()
                        .is_some_and(|p| ui.max_rect().contains(p))
                });
        }
        self.prune_thumbnails(editor);
        let assets = snapshots(editor);
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !assets.iter().any(|asset| &asset.id == id))
        {
            self.selected = None;
        }

        if self.selected.is_none() {
            self.selected = assets.first().map(|asset| asset.id.clone());
        }
        let mut output = AssetBrowserOutput::default();
        let editing = editor.play.is_none();
        let selected_drawable = |asset: &AssetSnapshot| {
            editor
                .selected_object()
                .is_some_and(|object| object.drawable.is_some())
                && (editor.selected_surface().is_none() || asset.kind == AssetKind::Image)
        };

        super::theme::panel_title(ui, "Content Browser");
        ui.horizontal(|ui| {
            ui.weak("Assets");
            ui.weak("/");
            egui::ComboBox::from_id_salt("asset-location")
                .width(86.0)
                .selected_text(match self.filter {
                    AssetFilter::All => "All assets",
                    AssetFilter::Images => "Textures",
                    AssetFilter::Models => "Models",
                    AssetFilter::Prefabs => "Prefabs",
                    AssetFilter::Blueprints => "Blueprints",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.filter, AssetFilter::All, "All assets");
                    ui.selectable_value(&mut self.filter, AssetFilter::Images, "Textures");
                    ui.selectable_value(&mut self.filter, AssetFilter::Models, "Models");
                    ui.selectable_value(&mut self.filter, AssetFilter::Prefabs, "Prefabs");
                    ui.selectable_value(&mut self.filter, AssetFilter::Blueprints, "Blueprints");
                });
            ui.separator();
            if ui
                .add_enabled(editing, egui::Button::new("Import…"))
                .on_hover_text("Import PNG, JPEG, OBJ, glTF, GLB, or .prefab.json")
                .clicked()
            {
                if self.filter == AssetFilter::Blueprints {
                    output.blueprint_import_requested = true;
                } else {
                    output.import_requested = true;
                }
            }
            if ui
                .add_enabled(editing, egui::Button::new("Reload"))
                .on_hover_text("Reload changed asset files")
                .clicked()
            {
                output.reload_requested = true;
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search assets")
                    .desired_width(140.0),
            );
            if ui.small_button("×").on_hover_text("Clear search").clicked() {
                self.search.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut self.show_details, "Details");
                ui.small(format!("{} assets", assets.len()));
            });
        });

        let query = self.search.trim().to_ascii_lowercase();
        let shown: Vec<_> = assets
            .iter()
            .filter(|asset| {
                matches_filter(self.filter, asset.kind)
                    && (query.is_empty()
                        || asset.id.to_ascii_lowercase().contains(&query)
                        || asset.path.to_ascii_lowercase().contains(&query))
            })
            .collect();

        let mut command = None;
        let mut available = ui.available_size();
        let sidebar = available.x > 500.0;
        if sidebar {
            available.x -= 146.0;
        }
        let side_details = available.x > 650.0;
        let selected = self
            .selected
            .as_ref()
            .filter(|_| self.show_details && self.filter != AssetFilter::Blueprints)
            .and_then(|id| assets.iter().find(|asset| &asset.id == id));
        let grid_width = if side_details && selected.is_some() {
            available.x - 276.0
        } else {
            available.x
        };
        let grid_height = if !side_details && selected.is_some() {
            (available.y * 0.55).max(90.0)
        } else {
            available.y
        };
        ui.horizontal_top(|ui| {
            if sidebar {
                ui.allocate_ui_with_layout(Vec2::new(130.0, available.y), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.small("PROJECT");
                    for (filter, name) in [(AssetFilter::All, "All assets"), (AssetFilter::Images, "Textures"), (AssetFilter::Models, "Models"), (AssetFilter::Prefabs, "Prefabs"), (AssetFilter::Blueprints, "Blueprints")] {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), Sense::hover());
                            draw_folder(ui.painter(), rect);
                            filter_button(ui, &mut self.filter, filter, name);
                        });
                    }
                    ui.separator();
                    ui.small("Scene asset library").on_hover_text("Assets referenced by the open scene, grouped by type. Import adds files to this library.");
                });
                ui.separator();
            }
            ui.allocate_ui_with_layout(
                Vec2::new(grid_width, grid_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("asset-browser-scroll")
                        .auto_shrink([false, false])
                        .max_height(grid_height)
                        .show(ui, |ui| {
                            if self.filter == AssetFilter::Blueprints {
                                self.blueprints(ui, editor, &mut output);
                                return;
                            }
                            if shown.is_empty() {
                                ui.add_space(18.0);
                                ui.weak(if assets.is_empty() {
                                    "Drop a model or texture here, or use Import to get started."
                                } else {
                                    "No assets match. Clear the search or choose All."
                                });
                                return;
                            }
                            let columns = (grid_width / 112.0).floor().max(1.0) as usize;
                            egui::Grid::new("asset-browser-grid")
                                .num_columns(columns)
                                .spacing(Vec2::new(8.0, 8.0))
                                .show(ui, |ui| {
                                    for (index, asset) in shown.iter().enumerate() {
                                        self.tile(ui, asset, editor, editing, selected_drawable(asset), &mut command);
                                        if (index + 1) % columns == 0 {
                                            ui.end_row();
                                        }
                                    }
                                });
                        });
                },
            );
            if side_details && let Some(asset) = selected {
                ui.separator();
                ui.allocate_ui_with_layout(
                    Vec2::new(260.0, available.y),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("asset-details-scroll")
                            .max_height(available.y)
                            .show(ui, |ui| {
                                self.details(ui, asset, editing, selected_drawable(asset), &mut command);
                            });
                    },
                );
            }
        });
        if !side_details && let Some(asset) = selected {
            egui::ScrollArea::vertical()
                .id_salt("asset-details-narrow")
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    self.details(ui, asset, editing, selected_drawable(asset), &mut command)
                });
        }

        if let Some(command) = command {
            match command {
                AssetCommand::Delete(id) => self.request_delete(DeleteTarget::Asset(id), editor),
                AssetCommand::RefreshPrefab(asset) => {
                    output.prefab_requested = Some(bozzard_editor::PrefabCommand::Refresh { asset })
                }
                AssetCommand::Add(id) if editor.scene().assets[&id].kind == AssetKind::Prefab => {
                    output.prefab_requested = Some(bozzard_editor::PrefabCommand::Instantiate {
                        asset: id,
                        position: None,
                    });
                }
                AssetCommand::Add(id) => match editor.add_asset_to_scene(&id) {
                    Ok(layer) => {
                        output.added_layer = Some(layer);
                        output.status = Some(BrowserStatus {
                            message: format!("Added {id} to the scene"),
                            error: false,
                        });
                    }
                    Err(error) => set_error(&mut output, error),
                },
                AssetCommand::Assign(id) => match editor.assign_asset_to_selected(&id) {
                    Ok(()) => {
                        output.status = Some(BrowserStatus {
                            message: format!("Assigned {id} to selected object"),
                            error: false,
                        });
                    }
                    Err(error) => set_error(&mut output, error),
                },
                AssetCommand::Remove(id) => match editor.remove_asset(&id) {
                    Ok(()) => {
                        self.selected = None;
                        output.status = Some(BrowserStatus {
                            message: format!("Removed {id} from this scene"),
                            error: false,
                        });
                    }
                    Err(error) => set_error(&mut output, error),
                },
            }
        }
        self.delete_confirmation(ui.ctx(), editor, &mut output, ui.is_enabled());
        output
    }

    fn delete_confirmation(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        output: &mut AssetBrowserOutput,
        enabled: bool,
    ) {
        let Some(pending) = &self.pending_delete else {
            return;
        };
        let name = match &pending.target {
            DeleteTarget::Asset(id) => id.clone(),
            DeleteTarget::Blueprint(path) => path.display().to_string(),
        };
        let mut confirm = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("delete-project-asset")).show(ctx, |ui| {
            ui.heading("Delete from project?");
            ui.label(&name);
            ui.label("Removes the asset file and all scene objects using it, including their children.");
            if matches!(pending.target, DeleteTarget::Blueprint(_)) { ui.weak("Embedded Blueprint attachments are independent copies and will remain."); }
            ui.weak("Undo restores the file and scene objects. Files are kept in .bozzard-trash for recovery. Other saved scenes may reference this file; they are not rewritten. Shared dependencies are kept.");
            ui.horizontal(|ui| {
                confirm = ui.add_enabled(enabled && editor.play.is_none(), egui::Button::new("Delete from project")).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if confirm {
            let pending = self.pending_delete.take().unwrap();
            let result = if pending.revision != editor.revision()
                || pending.assets != editor.asset_revision()
                || pending.scene != editor.path
            {
                Err(anyhow::anyhow!(
                    "Scene or assets changed; select the asset and confirm deletion again"
                ))
            } else {
                match pending.target {
                    DeleteTarget::Asset(id) => editor.delete_project_asset(&id),
                    DeleteTarget::Blueprint(path) => editor.delete_project_file(&path),
                }
            };
            match result {
                Ok(count) => {
                    self.selected = None;
                    self.selected_blueprint = None;
                    output.status = Some(BrowserStatus {
                        message: format!(
                            "Deleted project asset and {count} scene objects · Undo restores both"
                        ),
                        error: false,
                    });
                }
                Err(error) => set_error(output, error),
            }
        } else if cancel || modal.should_close() {
            self.pending_delete = None;
        }
    }

    fn blueprints(&mut self, ui: &mut egui::Ui, editor: &Editor, output: &mut AssetBrowserOutput) {
        ui.weak("Blueprints · Select an object, then double-click a graph to attach a copy.");
        let editing = editor.play.is_none()
            && editor.selected_object().is_some()
            && editor.selected_surface().is_none();
        if ui
            .add_enabled(editing, egui::Button::new("New Blueprint"))
            .clicked()
        {
            output.blueprint_opened = true;
        }
        let query = self.search.trim().to_lowercase();
        let paths = blueprint_paths(&editor.path);
        match paths {
            Err(error) => {
                ui.colored_label(Color32::LIGHT_RED, error.to_string());
            }
            Ok(mut paths) => {
                paths.sort();
                for path in paths.iter().filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                        n.ends_with(".blueprint.json") && n.to_lowercase().contains(&query)
                    })
                }) {
                    let name = path.file_name().unwrap().to_string_lossy();
                    let response = ui.add_enabled(editor.play.is_none(), egui::Button::selectable(self.selected_blueprint.as_ref() == Some(path), format!("◇ {name}")))
                        .on_hover_text(format!("{}\nDouble-click to attach an independent copy · Delete removes the project file", path.display()));
                    if response.clicked() || response.secondary_clicked() {
                        self.selected_blueprint = Some(path.clone());
                    }
                    if response.double_clicked() && editing {
                        output.blueprint_path = Some(path.clone());
                    }
                    response.context_menu(|ui| {
                        if ui
                            .add_enabled(
                                editor.play.is_none(),
                                egui::Button::new("Delete from project…"),
                            )
                            .clicked()
                        {
                            self.request_delete(DeleteTarget::Blueprint(path.clone()), editor);
                            ui.close();
                        }
                    });
                }
                if paths.is_empty() {
                    ui.weak("Save graphs here using Blueprint Editor → Save graph.");
                }
            }
        }
        ui.separator();
        ui.weak("Graphs attached in this scene");
        for object in &editor.scene().objects {
            for attachment in &object.blueprints {
                let label = format!("{} / {}", object.name, attachment.graph.name);
                if label.to_lowercase().contains(&query) && ui.button(label).clicked() {
                    self.selected_blueprint = None;
                    output.blueprint_owner = Some(object.id.clone());
                }
            }
        }
    }

    fn prune_thumbnails(&mut self, editor: &Editor) {
        if self.catalog_revision != editor.asset_revision() {
            self.thumbnails.clear();
            self.catalog_revision = editor.asset_revision();
        }
        self.thumbnails.retain(|id, thumbnail| {
            editor
                .assets
                .handle(id)
                .and_then(|handle| editor.assets.get(handle))
                .is_some_and(|entry| entry.revision() == thumbnail.asset_revision)
        });
    }

    fn tile(
        &mut self,
        ui: &mut egui::Ui,
        asset: &AssetSnapshot,
        editor: &Editor,
        editing: bool,
        selected_drawable: bool,
        command: &mut Option<AssetCommand>,
    ) {
        const TILE: Vec2 = Vec2::new(96.0, 92.0);
        let selected = self.selected.as_deref() == Some(&asset.id);
        egui::Frame::new()
            .inner_margin(4)
            .corner_radius(2)
            .stroke(Stroke::new(
                1.0,
                if selected {
                    super::theme::ACCENT
                } else {
                    Color32::TRANSPARENT
                },
            ))
            .fill(if selected {
                Color32::from_rgb(57, 51, 41)
            } else {
                Color32::TRANSPARENT
            })
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(TILE.x);
                    ui.set_min_height(TILE.y);
                    let preview =
                        ui.allocate_exact_size(Vec2::new(TILE.x, 64.0), Sense::click_and_drag());
                    let thumbnail = self.thumbnail(ui.ctx(), editor, asset);
                    draw_preview(ui, preview.0, asset, thumbnail);
                    let ready = editing && matches!(asset.state, LoadState::Ready);
                    if ready && asset.kind == AssetKind::Prefab {
                        preview.1.dnd_set_drag_payload(PrefabDrag(asset.id.clone()));
                    }
                    if preview.1.clicked() || preview.1.secondary_clicked() {
                        self.selected = Some(asset.id.clone());
                    }
                    if preview.1.double_clicked() && ready {
                        *command = Some(AssetCommand::Add(asset.id.clone()));
                    }
                    preview
                        .1
                        .on_hover_text(format!(
                            "{}\nDouble-click to add · Right-click for actions",
                            asset.path
                        ))
                        .context_menu(|ui| {
                            if ui
                                .add_enabled(ready, egui::Button::new("Add to scene"))
                                .clicked()
                            {
                                *command = Some(AssetCommand::Add(asset.id.clone()));
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    ready && selected_drawable && asset.kind != AssetKind::Prefab,
                                    egui::Button::new("Assign to selected"),
                                )
                                .clicked()
                            {
                                *command = Some(AssetCommand::Assign(asset.id.clone()));
                                ui.close();
                            }
                            if ui.button("Show details").clicked() {
                                self.show_details = true;
                                ui.close();
                            }
                            ui.separator();
                            if ui
                                .add_enabled(editing, egui::Button::new("Delete from project…"))
                                .clicked()
                            {
                                *command = Some(AssetCommand::Delete(asset.id.clone()));
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    editing && asset.users == 0,
                                    egui::Button::new("Remove from scene library"),
                                )
                                .on_hover_text("The source file stays on disk")
                                .clicked()
                            {
                                *command = Some(AssetCommand::Remove(asset.id.clone()));
                                ui.close();
                            }
                        });
                    if ui
                        .add(egui::Button::selectable(selected, &asset.id).truncate())
                        .on_hover_text(&asset.path)
                        .clicked()
                    {
                        self.selected = Some(asset.id.clone());
                    }
                    ui.small(match asset.kind {
                        AssetKind::Prefab => "Prefab",
                        AssetKind::Image => "Image",
                        AssetKind::Mesh => "Model",
                    });
                    if matches!(asset.state, LoadState::Failed(_)) {
                        ui.colored_label(Color32::LIGHT_RED, "Load failed");
                    }
                });
            });
    }

    fn thumbnail(
        &mut self,
        ctx: &egui::Context,
        editor: &Editor,
        asset: &AssetSnapshot,
    ) -> Option<TextureHandle> {
        if let Some(thumbnail) = self.thumbnails.get(&asset.id)
            && thumbnail.asset_revision == asset.revision
        {
            return Some(thumbnail.texture.clone());
        }
        let image = editor
            .assets
            .handle(&asset.id)
            .and_then(|handle| editor.assets.get(handle))
            .and_then(|entry| entry.data())
            .and_then(|data| match data {
                AssetData::Image(image) => Some(image),
                AssetData::Mesh(_) | AssetData::Prefab(_) => None,
            })?;
        let texture = ctx.load_texture(
            format!("asset-thumbnail-{}-{}", asset.id, asset.revision),
            thumbnail_image(image.width, image.height, &image.rgba),
            egui::TextureOptions::LINEAR,
        );
        self.thumbnails.insert(
            asset.id.clone(),
            Thumbnail {
                asset_revision: asset.revision,
                texture: texture.clone(),
            },
        );
        Some(texture)
    }

    fn details(
        &self,
        ui: &mut egui::Ui,
        asset: &AssetSnapshot,
        editing: bool,
        selected_drawable: bool,
        command: &mut Option<AssetCommand>,
    ) {
        ui.strong(&asset.id);
        ui.add(egui::Label::new(&asset.path).wrap());
        ui.weak(format!("Used by {} object(s)", asset.users));
        match asset.kind {
            AssetKind::Prefab => ui.label(format!(
                "Prefab · {} objects",
                asset.prefab_objects.unwrap_or(0)
            )),
            AssetKind::Image => match &asset.image {
                Some((width, height)) => ui.label(format!("Image · {width} × {height} px")),
                None => ui.label("Image"),
            },
            AssetKind::Mesh => match &asset.mesh {
                Some(mesh) => ui.label(format!(
                    "Model · {vertices} vertices · {} triangles",
                    mesh.index_count / 3,
                    vertices = mesh.vertex_count,
                )),
                None => ui.label("Model"),
            },
        };
        if let Some(mesh) = &asset.mesh {
            for warning in &mesh.warnings {
                ui.colored_label(Color32::YELLOW, warning);
            }
        }
        match &asset.state {
            LoadState::Ready => {
                ui.colored_label(Color32::LIGHT_GREEN, "Loaded");
            }
            LoadState::Pending => {
                ui.weak("Waiting to load");
            }
            LoadState::Failed(message) => {
                ui.colored_label(Color32::LIGHT_RED, message);
                if asset.image.is_some() || asset.mesh.is_some() {
                    ui.colored_label(
                        Color32::YELLOW,
                        "Showing the last successfully loaded version",
                    );
                }
            }
        };
        ui.vertical(|ui| {
            if asset.kind == AssetKind::Prefab
                && ui
                    .add_enabled(
                        editing && asset.users > 0,
                        egui::Button::new("Refresh instances"),
                    )
                    .clicked()
            {
                *command = Some(AssetCommand::RefreshPrefab(asset.id.clone()));
            }
            if ui
                .add_enabled(
                    editing && matches!(asset.state, LoadState::Ready),
                    egui::Button::new("Add to scene"),
                )
                .clicked()
            {
                *command = Some(AssetCommand::Add(asset.id.clone()));
            }
            if ui
                .add_enabled(
                    editing
                        && selected_drawable
                        && asset.kind != AssetKind::Prefab
                        && matches!(asset.state, LoadState::Ready),
                    egui::Button::new("Assign to selected"),
                )
                .clicked()
            {
                *command = Some(AssetCommand::Assign(asset.id.clone()));
            }
            if ui
                .add_enabled(editing, egui::Button::new("Delete from project…"))
                .clicked()
            {
                *command = Some(AssetCommand::Delete(asset.id.clone()));
            }
            if ui
                .add_enabled(editing && asset.users == 0, egui::Button::new("Remove"))
                .on_hover_text("Removes this asset from the scene; the source file stays on disk")
                .clicked()
            {
                *command = Some(AssetCommand::Remove(asset.id.clone()));
            }
        });
    }
}

fn blueprint_paths(scene: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    // ponytail: two shallow directory listings; cache if projects accumulate thousands of graphs.
    let assets = bozzard_editor::root(scene).join("assets");
    let mut paths = Vec::new();
    for directory in [assets.clone(), assets.join("Blueprints")] {
        match std::fs::read_dir(directory) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    if entry.file_type()?.is_file()
                        && entry
                            .file_name()
                            .to_string_lossy()
                            .ends_with(".blueprint.json")
                    {
                        paths.push(entry.path());
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(paths)
}

fn set_error(output: &mut AssetBrowserOutput, error: anyhow::Error) {
    output.status = Some(BrowserStatus {
        message: format!("{error:#}"),
        error: true,
    });
}

fn snapshots(editor: &Editor) -> Vec<AssetSnapshot> {
    let users = editor.scene().asset_users();
    editor
        .assets
        .entries()
        .filter_map(|entry| {
            let source = editor.scene().assets.get(&entry.id)?;
            let (image, mesh) = match entry.data() {
                Some(AssetData::Image(image)) => (Some((image.width, image.height)), None),
                Some(AssetData::Mesh(mesh)) => (None, Some(sample_mesh(mesh))),
                Some(AssetData::Prefab(_)) | None => (None, None),
            };
            Some(AssetSnapshot {
                id: entry.id.clone(),
                kind: source.kind,
                path: source.path.clone(),
                state: entry.state().clone(),
                revision: entry.revision(),
                users: users.get(&entry.id).map_or(0, Vec::len),
                image,
                mesh,
                prefab_objects: match entry.data() {
                    Some(AssetData::Prefab(p)) => Some(p.objects.len()),
                    _ => None,
                },
            })
        })
        .collect()
}

/// Re-index a bounded sample, so thumbnails never retain a second full copy of
/// an imported mesh. Invalid triangles are ignored here and remain reported by
/// the asset loader itself.
fn sample_mesh(mesh: &bozzard_assets::MeshData) -> MeshPreview {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut remap = HashMap::new();
    for triangle in mesh.indices.chunks_exact(3).take(160) {
        let mut local = [0; 3];
        let mut valid = true;
        for (slot, &index) in triangle.iter().enumerate() {
            let Some(vertex) = mesh.vertices.get(index as usize) else {
                valid = false;
                break;
            };
            let mapped = *remap.entry(index).or_insert_with(|| {
                let next = vertices.len() as u32;
                vertices.push(*vertex);
                next
            });
            local[slot] = mapped;
        }
        if valid {
            indices.extend(local);
        }
    }
    MeshPreview {
        vertex_count: mesh.vertices.len(),
        index_count: mesh.indices.len(),
        vertices,
        indices,
        warnings: mesh.warnings.clone(),
    }
}

/// Keep thumbnail texture memory predictable even when a source image is 4K.
/// This runs only when an image enters or changes in the texture cache.
fn thumbnail_image(width: u32, height: u32, rgba: &[u8]) -> ColorImage {
    const MAX_EDGE: u32 = 192;
    let scale = (MAX_EDGE as f32 / width.max(height) as f32).min(1.0);
    let output_width = (width as f32 * scale).round().max(1.0) as u32;
    let output_height = (height as f32 * scale).round().max(1.0) as u32;
    if output_width == width && output_height == height {
        return ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba);
    }
    let mut pixels = vec![0; output_width as usize * output_height as usize * 4];
    for y in 0..output_height {
        for x in 0..output_width {
            let source_x = x * width / output_width;
            let source_y = y * height / output_height;
            let source = ((source_y * width + source_x) * 4) as usize;
            let target = ((y * output_width + x) * 4) as usize;
            pixels[target..target + 4].copy_from_slice(&rgba[source..source + 4]);
        }
    }
    ColorImage::from_rgba_unmultiplied([output_width as usize, output_height as usize], &pixels)
}

fn draw_folder(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(
        Rect::from_min_size(rect.min + Vec2::new(1.0, 2.0), Vec2::new(7.0, 5.0)),
        1.0,
        Color32::from_rgb(94, 150, 154),
    );
    painter.rect_filled(
        Rect::from_min_max(
            rect.min + Vec2::new(1.0, 5.0),
            rect.max - Vec2::new(1.0, 1.0),
        ),
        1.0,
        Color32::from_rgb(171, 160, 132),
    );
}

fn filter_button(ui: &mut egui::Ui, filter: &mut AssetFilter, value: AssetFilter, label: &str) {
    if ui.selectable_label(*filter == value, label).clicked() {
        *filter = value;
    }
}

fn matches_filter(filter: AssetFilter, kind: AssetKind) -> bool {
    matches!(filter, AssetFilter::All)
        || matches!((filter, kind), (AssetFilter::Images, AssetKind::Image))
        || matches!((filter, kind), (AssetFilter::Models, AssetKind::Mesh))
        || matches!((filter, kind), (AssetFilter::Prefabs, AssetKind::Prefab))
}

fn draw_preview(
    ui: &egui::Ui,
    rect: Rect,
    asset: &AssetSnapshot,
    thumbnail: Option<TextureHandle>,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_gray(25));
    match (asset.kind, thumbnail, asset.mesh.as_ref()) {
        (AssetKind::Prefab, _, _) => {
            let c = rect.center();
            let color = Color32::from_rgb(178, 155, 244);
            for (dx, dy, size) in [(0.0, -12.0, 22.0), (-22.0, 18.0, 14.0), (22.0, 18.0, 14.0)] {
                let center = c + Vec2::new(dx, dy);
                if dy > 0.0 {
                    painter.line_segment([c, center], egui::Stroke::new(1.5, color));
                }
                painter.rect_filled(
                    Rect::from_center_size(center, Vec2::splat(size)),
                    3.0,
                    color,
                );
            }
        }
        (AssetKind::Image, Some(texture), _) => {
            let size = texture.size_vec2();
            let scale = (rect.width() / size.x).min(rect.height() / size.y).min(1.0);
            let image_rect = Rect::from_center_size(rect.center(), size * scale);
            painter.image(
                texture.id(),
                image_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        (AssetKind::Mesh, _, Some(mesh)) => {
            draw_mesh_preview(&painter, rect.shrink(8.0), &mesh.vertices, &mesh.indices);
        }
        (AssetKind::Mesh, _, _) => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "◇",
                egui::FontId::proportional(34.0),
                Color32::from_gray(150),
            );
        }
        _ => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Image",
                egui::FontId::proportional(13.0),
                Color32::from_gray(150),
            );
        }
    }
}

fn draw_mesh_preview(painter: &egui::Painter, rect: Rect, vertices: &[[f32; 8]], indices: &[u32]) {
    if vertices.is_empty() {
        return;
    }
    let rotation = glam::Mat4::from_rotation_x(-0.25) * glam::Mat4::from_rotation_y(0.55);
    let projected: Vec<_> = vertices
        .iter()
        .map(|v| rotation.transform_point3(glam::Vec3::from_slice(&v[..3])))
        .collect();
    let min = projected
        .iter()
        .fold(glam::Vec3::splat(f32::INFINITY), |min, p| min.min(*p));
    let max = projected
        .iter()
        .fold(glam::Vec3::splat(f32::NEG_INFINITY), |max, p| max.max(*p));
    let center = min * 0.5 + max * 0.5;
    let scale = (rect.width() / (max.x - min.x).max(0.01))
        .min(rect.height() / (max.y - min.y).max(0.01))
        * 0.88;
    let mut triangles: Vec<_> = indices
        .chunks_exact(3)
        .take(160)
        .filter_map(|t| {
            let p = [
                *projected.get(t[0] as usize)?,
                *projected.get(t[1] as usize)?,
                *projected.get(t[2] as usize)?,
            ];
            Some(p)
        })
        .collect();
    triangles.sort_by(|a, b| (a[0].z + a[1].z + a[2].z).total_cmp(&(b[0].z + b[1].z + b[2].z)));
    for triangle in triangles {
        let normal = (triangle[1] - triangle[0])
            .cross(triangle[2] - triangle[0])
            .normalize_or_zero();
        let light =
            (0.45 + 0.55 * normal.dot(glam::Vec3::new(0.4, 0.7, 0.6).normalize()).abs()) as f64;
        let points = triangle.map(|p| {
            Pos2::new(
                rect.center().x + (p.x - center.x) * scale,
                rect.center().y - (p.y - center.y) * scale,
            )
        });
        painter.add(egui::Shape::convex_polygon(
            points.to_vec(),
            Color32::from_rgb(
                (188.0 * light) as u8,
                (177.0 * light) as u8,
                (152.0 * light) as u8,
            ),
            Stroke::new(0.35, Color32::from_rgb(210, 197, 168)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blueprints_folder_lists_saved_graphs_without_mutating_scene() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/model-lab.json");
        let mut editor = Editor::open(&path).unwrap();
        let original = editor.scene().clone();
        let paths = blueprint_paths(&path).unwrap();
        assert!(paths.iter().any(|p| p.ends_with("spin.blueprint.json")));
        let ctx = egui::Context::default();
        let mut browser = AssetBrowser {
            filter: AssetFilter::Blueprints,
            show_details: true,
            ..Default::default()
        };
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let result = browser.ui(ui, &mut editor);
            assert!(!result.blueprint_opened && result.blueprint_path.is_none());
        });
        output.textures_delta.clear();
        assert_eq!(editor.scene(), &original);
    }

    #[test]
    fn delete_belongs_to_browser_focus_and_requires_confirmation_of_visible_asset() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/model-lab.json");
        let editor = Editor::open(&path).unwrap();
        let original = editor.scene().clone();
        for (focused, query, pending) in [
            (false, "", false),
            (true, "", true),
            (true, "no-such-asset", false),
        ] {
            let ctx = egui::Context::default();
            let mut browser = AssetBrowser {
                focused,
                selected: editor.scene().assets.keys().next().cloned(),
                search: query.into(),
                ..Default::default()
            };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::Delete,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ui| {
                    assert_eq!(browser.delete_shortcut(ui.ctx(), &editor), focused);
                },
            );
            output.textures_delta.clear();
            assert_eq!(browser.confirming_delete(), pending);
            assert_eq!(editor.scene(), &original);
        }
    }

    #[test]
    fn compact_browser_fits_and_double_click_add_is_edit_only() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/model-lab.json");
        let mut editor = Editor::open(&path).unwrap();
        let original = editor.scene().clone();
        for width in [640.0, 1100.0] {
            for details in [false, true] {
                let ctx = egui::Context::default();
                super::super::theme::install(&ctx);
                let mut browser = AssetBrowser {
                    show_details: details,
                    ..Default::default()
                };
                for frame in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                Pos2::ZERO,
                                Vec2::new(width, 240.0),
                            )),
                            time: Some(frame as f64),
                            ..Default::default()
                        },
                        |ui| {
                            let available = ui.max_rect();
                            browser.ui(ui, &mut editor);
                            assert!(
                                ui.min_rect().right() <= available.right() + 1.0,
                                "browser overflow at width {width}, details={details}: {:?}",
                                ui.min_rect()
                            );
                        },
                    );
                    output.textures_delta.clear();
                }
                assert_eq!(
                    editor.scene(),
                    &original,
                    "idle browser changed authored data"
                );
            }
        }
        let asset = snapshots(&editor).remove(0);
        assert!(matches!(asset.state, LoadState::Ready));
        for editing in [false, true] {
            let ctx = egui::Context::default();
            let mut browser = AssetBrowser::default();
            let mut command = None;
            for (frame, pressed) in [None, Some(true), Some(false), Some(true), Some(false)]
                .into_iter()
                .enumerate()
            {
                let pos = Pos2::new(35.0, 35.0);
                let mut events = vec![egui::Event::PointerMoved(pos)];
                if let Some(pressed) = pressed {
                    events.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 200.0))),
                        time: Some(frame as f64 * 0.05),
                        events,
                        ..Default::default()
                    },
                    |ui| browser.tile(ui, &asset, &editor, editing, false, &mut command),
                );
                output.textures_delta.clear();
            }
            assert_eq!(browser.selected.as_deref(), Some(asset.id.as_str()));
            if editing {
                assert!(matches!(command, Some(AssetCommand::Add(ref id)) if id == &asset.id));
            } else {
                assert!(command.is_none(), "Play must not permit adding assets");
            }
        }
    }
}

#[cfg(test)]
mod prefab_tests {
    use super::*;
    #[test]
    fn prefab_thumbnail_drag_delivers_one_payload_and_is_disabled_in_play() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/prefab-lab.json");
        let mut editor = Editor::open(&path).unwrap();
        let asset = snapshots(&editor)
            .into_iter()
            .find(|a| a.kind == AssetKind::Prefab)
            .unwrap();
        assert_eq!(asset.prefab_objects, Some(5));
        assert!(matches_filter(AssetFilter::Prefabs, asset.kind));
        assert!(!matches_filter(AssetFilter::Models, asset.kind));
        let ctx = egui::Context::default();
        let mut browser = AssetBrowser::default();
        let mut delivered = Vec::new();
        let mut source = Pos2::ZERO;
        let mut target = Pos2::ZERO;
        let mut frame = |events: Vec<egui::Event>, editing: bool| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(640.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.horizontal_top(|ui| {
                        source = ui.cursor().min + Vec2::new(30.0, 30.0);
                        browser.tile(ui, &asset, &editor, editing, false, &mut None);
                        let (_, response) =
                            ui.allocate_exact_size(Vec2::splat(180.0), Sense::click_and_drag());
                        target = response.rect.center();
                        if let Some(payload) = response.dnd_release_payload::<PrefabDrag>() {
                            delivered.push(payload.0.clone());
                        }
                    });
                },
            );
            output.textures_delta.clear();
            (source, target)
        };
        let (source, target) = frame(vec![], true);
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            vec![egui::Event::PointerMoved(source), press(source, true)],
            true,
        );
        frame(
            vec![egui::Event::PointerMoved(source + Vec2::new(15.0, 15.0))],
            true,
        );
        frame(vec![egui::Event::PointerMoved(target)], true);
        frame(vec![press(target, false)], true);
        frame(
            vec![egui::Event::PointerMoved(source), press(source, true)],
            false,
        );
        frame(vec![egui::Event::PointerMoved(target)], false);
        frame(vec![press(target, false)], false);
        assert_eq!(delivered, vec![asset.id]);
        editor.start_play().unwrap();
    }
}
