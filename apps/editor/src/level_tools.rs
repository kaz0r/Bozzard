//! Interactive level authoring; workers publish through the same headless editor transactions.
use anyhow::{Result, ensure};
use bozzard_assets::{
    blockout::{Blockout, BrushPrimitive},
    job::Job,
    terrain::{BrushMode, Terrain, TerrainBrush},
};
use bozzard_editor::{
    Editor, FoliageSettings, PreparedFoliage, PreparedGeometry, TerrainRequest, TerrainSource,
};
use bozzard_scene::{Layer, Transform};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use glam::{Mat4, Vec3, Vec4};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub grid: bool,
    pub grid_step: f32,
    pub grid_lines: u16,
    pub plane_y: f32,
    pub place_on_surfaces: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            grid: false,
            grid_step: 1.,
            grid_lines: 40,
            plane_y: 0.,
            place_on_surfaces: true,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Select,
    Terrain,
    Blockout,
    Measure,
}
struct TerrainDraft {
    source: TerrainSource,
    data: Terrain,
    scene: std::path::PathBuf,
    changed: bool,
    matrix: Mat4,
    matrix_revision: u64,
}
enum Work {
    Geometry(Job<PreparedGeometry>, bool),
    Foliage(Job<PreparedFoliage>),
}
enum Completed {
    Geometry(Result<PreparedGeometry>, bool),
    Foliage(Result<PreparedFoliage>),
}

pub struct LevelTools {
    pub visible: bool,
    mode: Mode,
    draft: Option<TerrainDraft>,
    work: Option<Work>,
    terrain_size: [f32; 2],
    terrain_resolution: u16,
    origin: [f32; 3],
    brush_mode: BrushMode,
    radius: f32,
    strength: f32,
    flatten_height: f32,
    primitive: BrushPrimitive,
    dimensions: [f32; 3],
    yaw: f32,
    stamp_spacing: f32,
    stamps: Vec<Transform>,
    stroking: bool,
    prototype: String,
    ground: String,
    foliage: FoliageSettings,
    measurement: Vec<Vec3>,
    message: String,
    error: bool,
}
pub struct Viewport<'a> {
    pub rect: Rect,
    pub projection: Mat4,
    pub prefs: &'a Preferences,
    pub snapping: &'a crate::snapping::Snapping,
    pub enabled: bool,
}
impl Default for LevelTools {
    fn default() -> Self {
        Self {
            visible: false,
            mode: Mode::Select,
            draft: None,
            work: None,
            terrain_size: [32.; 2],
            terrain_resolution: 65,
            origin: [0.; 3],
            brush_mode: BrushMode::Raise,
            radius: 3.,
            strength: 2.,
            flatten_height: 0.,
            primitive: BrushPrimitive::Box,
            dimensions: [2.; 3],
            yaw: 0.,
            stamp_spacing: 2.,
            stamps: Vec::new(),
            stroking: false,
            prototype: String::new(),
            ground: String::new(),
            foliage: Default::default(),
            measurement: Vec::new(),
            message: String::new(),
            error: false,
        }
    }
}
impl LevelTools {
    pub fn dirty(&self) -> bool {
        self.work.is_some()
            || !self.stamps.is_empty()
            || self.draft.as_ref().is_some_and(|d| d.changed)
    }
    fn report(&mut self, result: Result<()>) {
        if let Err(error) = result {
            self.message = format!("{error:#}");
            self.error = true;
        }
    }
    fn open_terrain(&mut self, editor: &Editor, id: &str) -> Result<()> {
        let source = editor.terrain_source(id)?;
        self.draft = Some(TerrainDraft {
            data: source.terrain.clone(),
            source,
            scene: editor.path.clone(),
            changed: false,
            matrix: editor.object_matrix(id)?,
            matrix_revision: editor.revision(),
        });
        self.mode = Mode::Terrain;
        self.error = false;
        Ok(())
    }
    fn save_terrain(&mut self, editor: &Editor) -> Result<()> {
        let Some(draft) = &self.draft else {
            return Ok(());
        };
        if !draft.changed {
            return Ok(());
        }
        ensure!(
            draft.scene == editor.path,
            "Return to the terrain's scene before applying this draft"
        );
        self.work = Some(Work::Geometry(
            editor.terrain_job(TerrainRequest::Sculpt {
                source: draft.source.clone(),
                terrain: draft.data.clone(),
            })?,
            true,
        ));
        Ok(())
    }
    fn poll(&mut self, editor: &mut Editor) {
        let completed = match &self.work {
            Some(Work::Geometry(job, terrain)) => {
                job.poll().map(|r| Completed::Geometry(r, *terrain))
            }
            Some(Work::Foliage(job)) => job.poll().map(Completed::Foliage),
            None => None,
        };
        if let Some(completed) = completed {
            // Keep the cancellation token alive through guarded publication.
            let work = self.work.take();
            let result =
                match completed {
                    Completed::Geometry(result, terrain) => result
                        .and_then(|p| editor.accept_geometry(p))
                        .and_then(|id| {
                            if terrain {
                                self.open_terrain(editor, &id)?;
                            }
                            self.message = if terrain {
                                "Terrain updated · Undo restores the previous geometry"
                            } else {
                                "Blockout placed · Undo removes this stroke"
                            }
                            .into();
                            Ok(())
                        }),
                    Completed::Foliage(result) => result
                        .and_then(|p| editor.accept_foliage(p))
                        .map(|(_, placed, requested)| {
                            self.message = format!(
                                "Placed {placed} of {requested} · Undo removes this scatter"
                            );
                        }),
                };
            drop(work);
            self.error = false;
            self.report(result);
        }
        let reload = self
            .draft
            .as_ref()
            .filter(|d| {
                self.work.is_none()
                    && !d.changed
                    && d.scene == editor.path
                    && !d.source.is_current(editor)
            })
            .map(|d| d.source.object().to_owned());
        if let Some(id) = reload {
            let result = self.open_terrain(editor, &id);
            if result.is_err() {
                self.draft = None;
                self.mode = Mode::Select;
            }
            self.report(result);
        }
    }

    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        prefs: &mut Preferences,
        busy: bool,
    ) {
        self.poll(editor);
        if self.work.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(20));
        }
        if !self.visible {
            return;
        }
        let mut visible = self.visible;
        egui::Window::new("Level tools").id(egui::Id::new("level-tools")).open(&mut visible).default_pos([340., 120.]).default_width(410.).vscroll(true).show(ctx, |ui| {
            ui.vertical(|ui| {
                if !self.message.is_empty() { ui.colored_label(if self.error { Color32::LIGHT_RED } else { ui.visuals().text_color() }, &self.message); }
                if let Some(work) = &self.work {
                    let (label, fraction) = match work { Work::Geometry(j, _) => (j.label(), j.fraction()), Work::Foliage(j) => (j.label(), j.fraction()) };
                    ui.label(label);
                    ui.add(egui::ProgressBar::new(fraction).show_percentage());
                    if ui.button("Cancel operation").clicked() { match work { Work::Geometry(j, _) => j.cancel(), Work::Foliage(j) => j.cancel() } }
                }
            });
            let available = !busy && editor.play.is_none() && self.work.is_none() && !self.stroking;
            ui.add_enabled_ui(available, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.mode, Mode::Select, "Select");
                    ui.add_enabled_ui(self.draft.is_some(), |ui| { ui.selectable_value(&mut self.mode, Mode::Terrain, "Sculpt terrain"); });
                    ui.selectable_value(&mut self.mode, Mode::Blockout, "Place brushes");
                    ui.selectable_value(&mut self.mode, Mode::Measure, "Measure");
                });
                ui.weak("Drag in the 3D viewport to sculpt or place a stroke. Release to apply; Undo restores the whole stroke.");
                egui::CollapsingHeader::new("Terrain").default_open(true).show(ui, |ui| {
                    ui.horizontal(|ui| { ui.label("Width / depth"); for value in &mut self.terrain_size { ui.add(egui::DragValue::new(value).range(0.01..=100_000.).speed(1.)); } });
                    ui.horizontal(|ui| { ui.label("Vertices per axis"); ui.add(egui::DragValue::new(&mut self.terrain_resolution).range(2..=129)); });
                    vector(ui, "Origin", &mut self.origin, -1_000_000., 1_000_000.);
                    if ui.add_enabled(!self.dirty(), egui::Button::new("Create terrain")).clicked() {
                        let result = Terrain::flat([self.terrain_resolution; 2], self.terrain_size).and_then(|terrain| editor.terrain_job(TerrainRequest::Create { terrain, position: self.origin })).map(|job| { self.work = Some(Work::Geometry(job, true)); });
                        self.report(result);
                    }
                    if ui.add_enabled(!self.dirty() && editor.selected.is_some(), egui::Button::new("Edit selected terrain")).clicked() {
                        let id = editor.selected.clone().unwrap(); let result = self.open_terrain(editor, &id); self.report(result);
                    }
                    if let Some(draft) = &self.draft { ui.label(format!("Editing {}", draft.source.object())); }
                    ui.horizontal_wrapped(|ui| {
                        for (mode, name) in [(BrushMode::Raise, "Raise"), (BrushMode::Lower, "Lower"), (BrushMode::Flatten, "Flatten"), (BrushMode::Smooth, "Smooth")] { ui.selectable_value(&mut self.brush_mode, mode, name); }
                    });
                    ui.horizontal(|ui| { ui.label("Radius"); ui.add(egui::DragValue::new(&mut self.radius).range(0.01..=10_000.).speed(0.1)); ui.label("Strength / second"); ui.add(egui::DragValue::new(&mut self.strength).range(0.01..=100.).speed(0.1)); });
                    ui.horizontal(|ui| { ui.label("Flatten height"); ui.add(egui::DragValue::new(&mut self.flatten_height).range(-10_000.0..=10_000.0).speed(0.1)); });
                    ui.weak("Brush size and height use terrain-local units. Green wire contours preview the stroke until release.");
                    ui.horizontal(|ui| {
                        if ui.add_enabled(self.draft.as_ref().is_some_and(|d| d.changed), egui::Button::new("Apply terrain draft")).clicked() { let result = self.save_terrain(editor); self.report(result); }
                        if ui.button("Discard terrain draft").clicked() { if let Some(draft) = &mut self.draft { draft.data = draft.source.terrain.clone(); draft.changed = false; } self.error = false; self.message.clear(); }
                    });
                });
                egui::CollapsingHeader::new("Blockout brushes").show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (shape, label) in [(BrushPrimitive::Box, "Box"), (BrushPrimitive::Ramp, "Ramp"), (BrushPrimitive::Stairs { steps: 8 }, "Stairs"), (BrushPrimitive::Cylinder { sides: 16 }, "Cylinder")] {
                            if ui.selectable_label(std::mem::discriminant(&self.primitive) == std::mem::discriminant(&shape), label).clicked() { self.primitive = shape; }
                        }
                    });
                    match &mut self.primitive { BrushPrimitive::Stairs { steps } => { ui.add(egui::DragValue::new(steps).range(1..=128).prefix("Steps ")); }, BrushPrimitive::Cylinder { sides } => { ui.add(egui::DragValue::new(sides).range(3..=64).prefix("Sides ")); }, _ => {} }
                    vector(ui, "Dimensions", &mut self.dimensions, 0.01, 10_000.);
                    ui.add(egui::DragValue::new(&mut self.yaw).speed(1.).prefix("Yaw ° "));
                    ui.add(egui::DragValue::new(&mut self.stamp_spacing).range(0.01..=10_000.).speed(0.1).prefix("Stroke spacing "));
                    if ui.button("Activate placement brush").clicked() { self.mode = Mode::Blockout; }
                    ui.weak("Shapes rest on their local Y=0 plane. Scene snapping controls new stamp positions; Ctrl temporarily inverts snapping.");
                });
                egui::CollapsingHeader::new("Foliage scattering").show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| { ui.label("Prototype:"); ui.label(editor.scene().objects.iter().find(|o| o.id == self.prototype).map(|o| o.name.as_str()).unwrap_or("Choose an object")); if ui.button("Use selected prototype").clicked() && let Some(id) = &editor.selected { self.prototype = id.clone(); } });
                    ui.horizontal_wrapped(|ui| { ui.label("Ground:"); ui.label(editor.scene().objects.iter().find(|o| o.id == self.ground).map(|o| o.name.as_str()).unwrap_or("Choose an object")); if ui.button("Use selected ground").clicked() && let Some(id) = &editor.selected { self.ground = id.clone(); } });
                    ui.weak("Select each object in the hierarchy, then use it here. The prototype root is its planting point; ground needs an enabled collider.");
                    ui.horizontal(|ui| { ui.label("Center X / Z"); for value in &mut self.foliage.center { ui.add(egui::DragValue::new(value).speed(0.5)); } });
                    ui.add(egui::DragValue::new(&mut self.foliage.radius).range(0.01..=10_000.).prefix("Radius "));
                    ui.add(egui::DragValue::new(&mut self.foliage.count).range(1..=2000).prefix("Count "));
                    ui.add(egui::DragValue::new(&mut self.foliage.seed).prefix("Seed "));
                    ui.add(egui::DragValue::new(&mut self.foliage.spacing).range(0.01..=10_000.).speed(0.1).prefix("Minimum spacing "));
                    ui.horizontal(|ui| { ui.label("Scale min / max"); for value in &mut self.foliage.scale { ui.add(egui::DragValue::new(value).range(0.01..=100.).speed(0.05)); } });
                    ui.add(egui::Slider::new(&mut self.foliage.max_slope_degrees, 0.0..=89.0).text("Maximum slope °"));
                    ui.checkbox(&mut self.foliage.align_to_surface, "Align to ground normal");
                    if ui.button("Scatter foliage").clicked() {
                        let result = editor.scatter_foliage_job(&self.prototype, &self.ground, self.foliage.clone()).map(|job| self.work = Some(Work::Foliage(job))); self.report(result);
                    }
                });
                egui::CollapsingHeader::new("Grid and measurement").default_open(true).show(ui, |ui| {
                    ui.checkbox(&mut prefs.grid, "Show construction grid");
                    ui.add(egui::DragValue::new(&mut prefs.grid_step).range(0.01..=10_000.).speed(0.1).prefix("Grid spacing "));
                    ui.add(egui::DragValue::new(&mut prefs.grid_lines).range(2..=100).prefix("Lines each side "));
                    ui.add(egui::DragValue::new(&mut prefs.plane_y).range(-1_000_000.0..=1_000_000.0).speed(0.1).prefix("Construction height "));
                    ui.checkbox(&mut prefs.place_on_surfaces, "Place and measure on visible surfaces");
                    if ui.button("Measure two points").clicked() { self.mode = Mode::Measure; self.measurement.clear(); }
                    if self.measurement.len() == 2 {
                        let delta = self.measurement[1] - self.measurement[0];
                        ui.label(format!("Distance {:.3} units · ΔX {:.3}, ΔY {:.3}, ΔZ {:.3}", delta.length(), delta.x, delta.y, delta.z));
                    }
                    if ui.button("Clear measurement").clicked() { self.measurement.clear(); }
                });
            });
        });
        self.visible = visible;
    }

    pub fn viewport(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        editor: &mut Editor,
        view: Viewport<'_>,
    ) -> Result<bool> {
        let Viewport {
            rect,
            projection,
            prefs,
            snapping,
            enabled,
        } = view;
        if prefs.grid {
            draw_grid(ui, rect, projection, prefs);
        }
        if self.measurement.len() == 2 {
            line(
                ui,
                rect,
                projection,
                self.measurement[0],
                self.measurement[1],
                Color32::YELLOW,
            );
            if let Some(p) = project(
                rect,
                projection,
                (self.measurement[0] + self.measurement[1]) * 0.5,
            ) {
                ui.painter().with_clip_rect(rect).text(
                    p,
                    egui::Align2::CENTER_BOTTOM,
                    format!(
                        "{:.3} units",
                        self.measurement[0].distance(self.measurement[1])
                    ),
                    egui::FontId::monospace(14.),
                    Color32::YELLOW,
                );
            }
        }
        let active = self.visible && self.mode != Mode::Select;
        let primary = ui.input(|i| i.pointer.primary_down());
        if self.stroking && !primary {
            self.stroking = false;
            if enabled && self.work.is_none() {
                let result = if self.mode == Mode::Terrain {
                    self.save_terrain(editor)
                } else if !self.stamps.is_empty() {
                    editor
                        .blockout_job(
                            Blockout {
                                version: 1,
                                primitive: self.primitive,
                            },
                            std::mem::take(&mut self.stamps),
                        )
                        .map(|job| self.work = Some(Work::Geometry(job, false)))
                } else {
                    Ok(())
                };
                self.report(result);
            }
        }
        if !active || !enabled || self.work.is_some() {
            return Ok(active);
        }
        let Some(pointer) = response.hover_pos().filter(|p| rect.contains(*p)) else {
            return Ok(active);
        };
        let ndc = [
            2. * (pointer.x - rect.left()) / rect.width() - 1.,
            1. - 2. * (pointer.y - rect.top()) / rect.height(),
        ];
        let hit = if prefs.place_on_surfaces || self.mode == Mode::Terrain {
            editor.pick_point_with_projection(Layer::ThreeD, projection, ndc)?
        } else {
            None
        };
        let fallback = plane_point(projection, ndc, prefs.plane_y);
        let Some(point) = hit.as_ref().map(|(_, p)| *p).or(fallback) else {
            return Ok(active);
        };
        match self.mode {
            Mode::Measure => {
                if response.clicked() {
                    if self.measurement.len() == 2 {
                        self.measurement.clear();
                    }
                    self.measurement.push(point);
                }
            }
            Mode::Terrain => {
                let Some(draft) = &mut self.draft else {
                    return Ok(active);
                };
                if draft.scene != editor.path
                    || !draft.source.is_current(editor)
                    || hit
                        .as_ref()
                        .is_none_or(|(p, _)| p.object != draft.source.object())
                {
                    return Ok(active);
                }
                if draft.matrix_revision != editor.revision() {
                    draft.matrix = editor.object_matrix(draft.source.object())?;
                    draft.matrix_revision = editor.revision();
                }
                let matrix = draft.matrix;
                let local = matrix.inverse().transform_point3(point);
                if primary {
                    let dt = ui.input(|i| i.stable_dt).clamp(0.001, 0.05);
                    draft.changed |= draft.data.brush(TerrainBrush {
                        mode: self.brush_mode,
                        center: [local.x, local.z],
                        radius: self.radius,
                        strength: self.strength * dt,
                        target_height: self.flatten_height,
                    })?;
                    self.stroking = true;
                    ui.ctx().request_repaint();
                }
                let radius = self.radius;
                for ring in [0.5, 1.] {
                    let mut previous = None;
                    for n in 0..=48 {
                        let angle = n as f32 * std::f32::consts::TAU / 48.;
                        let p = [
                            local.x + angle.cos() * radius * ring,
                            local.z + angle.sin() * radius * ring,
                        ];
                        if let Some((height, _)) = draft.data.sample(p) {
                            let p = matrix.transform_point3(Vec3::new(p[0], height + 0.01, p[1]));
                            if let Some(last) = previous {
                                line(ui, rect, projection, last, p, Color32::LIGHT_GREEN);
                            }
                            previous = Some(p);
                        } else {
                            previous = None;
                        }
                    }
                }
            }
            Mode::Blockout => {
                let snap = snapping.enabled ^ ui.input(|i| i.modifiers.ctrl);
                let position = snap_point(point, snapping.movement, snap);
                let transform = Transform {
                    translation: position.to_array(),
                    rotation_degrees: [0., self.yaw, 0.],
                    scale: self.dimensions,
                };
                box_preview(ui, rect, projection, transform, Color32::LIGHT_BLUE);
                if primary {
                    if self.stamps.len() < 256
                        && self.stamps.last().is_none_or(|last| {
                            Vec3::from_array(last.translation).distance(position)
                                >= self.stamp_spacing
                        })
                    {
                        self.stamps.push(transform);
                    }
                    self.stroking = true;
                }
                for stamp in &self.stamps {
                    box_preview(ui, rect, projection, *stamp, Color32::LIGHT_GREEN);
                }
            }
            Mode::Select => {}
        }
        Ok(active)
    }
}

fn vector(ui: &mut egui::Ui, name: &str, values: &mut [f32; 3], min: f32, max: f32) {
    ui.horizontal(|ui| {
        ui.label(name);
        for (axis, value) in ["X ", "Y ", "Z "].into_iter().zip(values) {
            ui.add(
                egui::DragValue::new(value)
                    .range(min..=max)
                    .speed(0.1)
                    .prefix(axis),
            );
        }
    });
}
fn snap_point(p: Vec3, step: f32, enabled: bool) -> Vec3 {
    if enabled && step.is_finite() && step > 0. {
        let snapped = (p / step).round() * step;
        if snapped.is_finite() { snapped } else { p }
    } else {
        p
    }
}
fn plane_point(projection: Mat4, ndc: [f32; 2], height: f32) -> Option<Vec3> {
    let inv = projection.inverse();
    let near = inv.project_point3(Vec3::new(ndc[0], ndc[1], 0.));
    let far = inv.project_point3(Vec3::new(ndc[0], ndc[1], 1.));
    let direction = far - near;
    if !direction.is_finite() || direction.y.abs() < 1e-6 {
        return None;
    }
    let t = (height - near.y) / direction.y;
    let p = near + t * direction;
    (t >= 0. && p.is_finite()).then_some(p)
}
fn project(rect: Rect, matrix: Mat4, p: Vec3) -> Option<Pos2> {
    let p = matrix * p.extend(1.);
    (p.is_finite() && p.w > 1e-7 && p.z >= 0. && p.z <= p.w).then(|| screen(rect, p))
}
fn screen(rect: Rect, p: Vec4) -> Pos2 {
    Pos2::new(
        rect.left() + (p.x / p.w + 1.) * rect.width() * 0.5,
        rect.top() + (1. - p.y / p.w) * rect.height() * 0.5,
    )
}
fn clipped_line(rect: Rect, matrix: Mat4, a: Vec3, b: Vec3) -> Option<[Pos2; 2]> {
    let [a, b] = [a, b].map(|p| matrix * p.extend(1.));
    if !a.is_finite() || !b.is_finite() {
        return None;
    }
    let planes = |p: Vec4| [p.w + p.x, p.w - p.x, p.w + p.y, p.w - p.y, p.z, p.w - p.z];
    let mut lo = 0_f32;
    let mut hi = 1_f32;
    for (x, y) in planes(a).into_iter().zip(planes(b)) {
        if x < 0. && y < 0. {
            return None;
        }
        if x < 0. {
            lo = lo.max(x / (x - y));
        } else if y < 0. {
            hi = hi.min(x / (x - y));
        }
    }
    if lo > hi {
        return None;
    }
    let [a, b] = [a.lerp(b, lo), a.lerp(b, hi)];
    (a.w > 1e-7 && b.w > 1e-7).then(|| [screen(rect, a), screen(rect, b)])
}
fn line(ui: &egui::Ui, rect: Rect, matrix: Mat4, a: Vec3, b: Vec3, color: Color32) {
    if let Some(points) = clipped_line(rect, matrix, a, b) {
        ui.painter()
            .with_clip_rect(rect)
            .line_segment(points, Stroke::new(1., color));
    }
}
fn draw_grid(ui: &egui::Ui, rect: Rect, projection: Mat4, prefs: &Preferences) {
    if !prefs.grid_step.is_finite() || prefs.grid_step <= 0. || !prefs.plane_y.is_finite() {
        return;
    }
    let step = prefs.grid_step.clamp(0.01, 10_000.);
    let center = snap_point(
        plane_point(projection, [0., 0.], prefs.plane_y).unwrap_or(Vec3::ZERO),
        step,
        true,
    );
    let count = i32::from(prefs.grid_lines.min(100));
    let extent = count as f32 * step;
    for index in -count..=count {
        let x = center.x + index as f32 * step;
        let z = center.z + index as f32 * step;
        line(
            ui,
            rect,
            projection,
            Vec3::new(x, prefs.plane_y, center.z - extent),
            Vec3::new(x, prefs.plane_y, center.z + extent),
            if x.abs() < step * 0.01 {
                Color32::from_rgb(70, 100, 180)
            } else {
                Color32::from_white_alpha(45)
            },
        );
        line(
            ui,
            rect,
            projection,
            Vec3::new(center.x - extent, prefs.plane_y, z),
            Vec3::new(center.x + extent, prefs.plane_y, z),
            if z.abs() < step * 0.01 {
                Color32::from_rgb(180, 80, 70)
            } else {
                Color32::from_white_alpha(45)
            },
        );
    }
}
fn box_preview(ui: &egui::Ui, rect: Rect, projection: Mat4, transform: Transform, color: Color32) {
    let m = transform.matrix();
    let corners: [_; 8] = std::array::from_fn(|i| {
        m.transform_point3(Vec3::new(
            if i & 1 == 0 { -0.5 } else { 0.5 },
            if i & 2 == 0 { 0. } else { 1. },
            if i & 4 == 0 { -0.5 } else { 0.5 },
        ))
    });
    for i in 0..8 {
        for axis in [1, 2, 4] {
            if i & axis == 0 {
                line(ui, rect, projection, corners[i], corners[i | axis], color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn grid_clips_camera_crossings_and_snapping_handles_invalid_preferences() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(500., 500.));
        let projection = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 100.);
        let segment = clipped_line(
            rect,
            projection,
            Vec3::new(0., 0., 1.),
            Vec3::new(0.5, 0., -2.),
        )
        .unwrap();
        assert!(
            segment
                .iter()
                .all(|p| p.is_finite() && rect.expand(0.001).contains(*p))
        );
        assert!(clipped_line(rect, projection, Vec3::Z, Vec3::Z * 2.).is_none());
        assert!(clipped_line(rect, Mat4::ZERO, Vec3::ZERO, Vec3::ONE).is_none());
        let point = Vec3::new(0.26, -0.26, 1.1);
        assert_eq!(snap_point(point, 0.5, true), Vec3::new(0.5, -0.5, 1.));
        for step in [0., -1., f32::NAN, f32::INFINITY] {
            assert_eq!(snap_point(point, step, true), point);
        }
        assert!(snap_point(Vec3::splat(f32::MAX), f32::MIN_POSITIVE, true).is_finite());
    }

    #[test]
    fn viewport_sculpt_release_publishes_collision_and_one_undo_restores_draft() -> Result<()> {
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let temp =
            Temp(std::env::temp_dir().join(format!("bozzard-sculpt-ui-{}", std::process::id())));
        std::fs::create_dir(&temp.0)?;
        let mut editor = Editor::new(
            bozzard_scene::Scene::from_json(
                r#"{"version":1,"name":"Sculpt","views":{},"objects":[]}"#,
            )?,
            &temp.0.join("scene.json"),
        )?;
        let job = editor.terrain_job(TerrainRequest::Create {
            terrain: Terrain::flat([9, 9], [8.; 2])?,
            position: [0.; 3],
        })?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let prepared = loop {
            if let Some(result) = job.poll() {
                break result?;
            }
            ensure!(Instant::now() < deadline, "terrain creation timed out");
            std::thread::sleep(Duration::from_millis(1));
        };
        let id = editor.accept_geometry(prepared)?;
        let mut tools = LevelTools {
            visible: true,
            radius: 1.,
            ..Default::default()
        };
        tools.open_terrain(&editor, &id)?;
        // Move the terrain after opening the tool. The world-space stroke must
        // follow its new transform without rebuilding that matrix on idle frames.
        let mut moved = editor.scene().clone();
        moved
            .objects
            .iter_mut()
            .find(|o| o.id == id)
            .unwrap()
            .transform
            .translation = [2., 0., 0.];
        editor.apply("Move terrain", moved)?;
        let before = editor.scene().clone();
        let ctx = egui::Context::default();
        let projection = glam::camera::rh::proj::directx::orthographic(-4., 4., -4., 4., 0.1, 100.)
            * glam::camera::rh::view::look_at_mat4(Vec3::Y * 10., Vec3::ZERO, Vec3::NEG_Z);
        let prefs = Preferences::default();
        let snapping = crate::snapping::Snapping::default();
        for frame in 0..4 {
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(516., 516.))),
                time: Some(frame as f64 / 60.),
                ..Default::default()
            };
            input
                .events
                .push(egui::Event::PointerMoved(Pos2::new(258., 258.)));
            if frame == 1 || frame == 3 {
                input.events.push(egui::Event::PointerButton {
                    pos: Pos2::new(258., 258.),
                    button: egui::PointerButton::Primary,
                    pressed: frame == 1,
                    modifiers: Default::default(),
                });
            }
            let mut result = Ok(());
            let mut output = ctx.run_ui(input, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(500., 500.), egui::Sense::click_and_drag());
                result = tools
                    .viewport(
                        ui,
                        &response,
                        &mut editor,
                        Viewport {
                            rect,
                            projection,
                            prefs: &prefs,
                            snapping: &snapping,
                            enabled: true,
                        },
                    )
                    .map(|_| ());
            });
            output.textures_delta.clear();
            result?;
        }
        assert!(
            tools.work.is_some(),
            "mouse release did not submit the sculpt stroke"
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while tools.work.is_some() {
            ensure!(Instant::now() < deadline, "sculpt publication timed out");
            tools.poll(&mut editor);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!tools.error, "{}", tools.message);
        assert!(!tools.dirty());
        let height = tools
            .draft
            .as_ref()
            .unwrap()
            .data
            .sample([-2., 0.])
            .unwrap()
            .0;
        assert!(height > 0.);
        let collision = editor
            .collisions()?
            .raycast(Vec3::Y * 10., Vec3::NEG_Y, 20., None)?
            .unwrap();
        assert!((collision.position.y - height).abs() < 1e-5);
        editor.undo()?;
        tools.poll(&mut editor);
        assert_eq!(editor.scene(), &before);
        assert_eq!(
            tools
                .draft
                .as_ref()
                .unwrap()
                .data
                .sample([-2., 0.])
                .unwrap()
                .0,
            0.
        );
        Ok(())
    }
}
