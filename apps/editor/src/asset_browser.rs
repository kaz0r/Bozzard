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
}

struct Thumbnail {
    asset_revision: u64,
    texture: TextureHandle,
}

#[derive(Default)]
pub struct AssetBrowserOutput {
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

enum AssetCommand {
    Add(String),
    Assign(String),
    Remove(String),
    RefreshPrefab(String),
}

impl AssetBrowser {
    pub fn reveal(&mut self, id: String) {
        self.selected = Some(id);
        self.search.clear();
        self.filter = AssetFilter::All;
    }
    /// Renders the browser and performs document operations through the editor.
    /// The caller handles the two operations that need application services.
    pub fn ui(&mut self, ui: &mut egui::Ui, editor: &mut Editor) -> AssetBrowserOutput {
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
        let selected_drawable = editor
            .selected_object()
            .is_some_and(|object| object.drawable.is_some())
            && editor.selected_surface().is_none();

        ui.horizontal(|ui| {
            ui.heading("Assets");
            if ui
                .add_enabled(editing, egui::Button::new("Import…"))
                .on_hover_text("Import PNG, JPEG, OBJ, glTF, GLB, or .prefab.json")
                .clicked()
            {
                output.import_requested = true;
            }
            if ui
                .add_enabled(editing, egui::Button::new("Reload"))
                .on_hover_text("Reload changed asset files")
                .clicked()
            {
                output.reload_requested = true;
            }
            if !editing {
                ui.weak("Stop Play to edit assets");
            }
        });
        ui.horizontal(|ui| {
            ui.label("Find");
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search assets")
                    .desired_width(180.0),
            );
            if ui.small_button("×").on_hover_text("Clear search").clicked() {
                self.search.clear();
            }
            filter_button(ui, &mut self.filter, AssetFilter::All, "All");
            filter_button(ui, &mut self.filter, AssetFilter::Images, "Images");
            filter_button(ui, &mut self.filter, AssetFilter::Models, "Models");
            filter_button(ui, &mut self.filter, AssetFilter::Prefabs, "Prefabs");
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
        let available = ui.available_size();
        let side_details = available.x > 650.0;
        let selected = self
            .selected
            .as_ref()
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
            ui.allocate_ui_with_layout(
                Vec2::new(grid_width, grid_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("asset-browser-scroll")
                        .auto_shrink([false, false])
                        .max_height(grid_height)
                        .show(ui, |ui| {
                            if shown.is_empty() {
                                ui.add_space(18.0);
                                ui.weak(if assets.is_empty() {
                                    "Drop a model or texture here, or use Import to get started."
                                } else {
                                    "No assets match. Clear the search or choose All."
                                });
                                return;
                            }
                            let columns = (grid_width / 150.0).floor().max(1.0) as usize;
                            egui::Grid::new("asset-browser-grid")
                                .num_columns(columns)
                                .spacing(Vec2::new(8.0, 8.0))
                                .show(ui, |ui| {
                                    for (index, asset) in shown.iter().enumerate() {
                                        self.tile(ui, asset, editor, editing, &mut command);
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
                                self.details(ui, asset, editing, selected_drawable, &mut command);
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
                    self.details(ui, asset, editing, selected_drawable, &mut command)
                });
        }

        if let Some(command) = command {
            match command {
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
        output
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
        command: &mut Option<AssetCommand>,
    ) {
        const TILE: Vec2 = Vec2::new(132.0, 128.0);
        let selected = self.selected.as_deref() == Some(&asset.id);
        egui::Frame::group(ui.style())
            .fill(if selected {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().faint_bg_color
            })
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(TILE.x);
                    ui.set_min_height(TILE.y);
                    let preview =
                        ui.allocate_exact_size(Vec2::new(120.0, 82.0), Sense::click_and_drag());
                    let thumbnail = self.thumbnail(ui.ctx(), editor, asset);
                    draw_preview(ui, preview.0, asset, thumbnail);
                    if editing
                        && asset.kind == AssetKind::Prefab
                        && matches!(asset.state, LoadState::Ready)
                    {
                        preview.1.dnd_set_drag_payload(PrefabDrag(asset.id.clone()));
                    }
                    if preview.1.clicked() {
                        self.selected = Some(asset.id.clone());
                    }
                    if ui
                        .add(egui::Button::selectable(selected, &asset.id).truncate())
                        .on_hover_text(&asset.path)
                        .clicked()
                    {
                        self.selected = Some(asset.id.clone());
                    }
                    ui.weak(match asset.kind {
                        AssetKind::Prefab => "Prefab",
                        AssetKind::Image => "Image",
                        AssetKind::Mesh => "Model",
                    });
                    if matches!(asset.state, LoadState::Failed(_)) {
                        ui.colored_label(Color32::LIGHT_RED, "Load failed");
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
                .add_enabled(editing && asset.users == 0, egui::Button::new("Remove"))
                .on_hover_text("Removes this asset from the scene; the source file stays on disk")
                .clicked()
            {
                *command = Some(AssetCommand::Remove(asset.id.clone()));
            }
        });
    }
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
    painter.rect_filled(rect, 4.0, Color32::from_gray(28));
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
                (66.0 * light) as u8,
                (157.0 * light) as u8,
                (172.0 * light) as u8,
            ),
            Stroke::new(0.35, Color32::from_rgb(99, 184, 196)),
        ));
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
                        browser.tile(ui, &asset, &editor, editing, &mut None);
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
