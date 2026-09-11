use super::*;
use bozzard_assets::{ImageData, MeshPart, Sampler};
use glam::Mat4;

fn surface_label(index: usize, part: &MeshPart) -> String {
    format!(
        "{} · {}",
        index + 1,
        part.material_name.as_deref().unwrap_or(&part.name)
    )
}

impl App {
    pub fn surface_graphics_ready(&self) -> bool {
        self.editor
            .selected_object()
            .and_then(|o| o.drawable.as_ref())
            .is_none_or(|d| match &d.mesh {
                Mesh::Asset(id) => self.residency.is_current(&self.editor.assets, id),
                _ => true,
            })
    }
    /// Returns true while inspecting an imported surface instead of editing its owner.
    pub fn surface_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(mesh) = self.editor.selected_mesh() else {
            return false;
        };
        if mesh.parts.is_empty() || self.editor.play.is_some() {
            return false;
        }
        let selected = self.editor.selected_surface().map(|s| s.index);
        let mut choose = None;
        let mut frame = false;
        let mut whole = false;
        egui::CollapsingHeader::new(format!("Imported surfaces ({})", mesh.parts.len()))
            .id_salt("imported-surfaces")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.surface_search)
                            .hint_text("Find surface or material…")
                            .desired_width(ui.available_width() - 30.0),
                    );
                    if ui.small_button("×").on_hover_text("Clear search").clicked() {
                        self.surface_search.clear();
                    }
                });
                let query = self.surface_search.trim().to_lowercase();
                let visible: Vec<_> = mesh
                    .parts
                    .iter()
                    .enumerate()
                    .filter(|(index, part)| {
                        query.is_empty()
                            || part.name.to_lowercase().contains(&query)
                            || surface_label(*index, part).to_lowercase().contains(&query)
                    })
                    .collect();
                if visible.is_empty() {
                    ui.weak("No matching surfaces");
                }
                egui::ScrollArea::vertical()
                    .id_salt("surface-list")
                    .max_height(160.0)
                    .show_rows(
                        ui,
                        ui.text_style_height(&egui::TextStyle::Body),
                        visible.len(),
                        |ui, range| {
                            for &(index, part) in &visible[range] {
                                let row = ui
                                    .selectable_label(
                                        selected == Some(index),
                                        surface_label(index, part),
                                    )
                                    .on_hover_text(&part.name);
                                if row.clicked() || row.double_clicked() {
                                    choose = Some(index);
                                }
                                if row.double_clicked() {
                                    frame = true;
                                }
                            }
                        },
                    );
            });
        if let Some(index) = selected {
            let part = &mesh.parts[index];
            ui.separator();
            ui.strong(surface_label(index, part));
            ui.weak(&part.name);
            ui.label(format!("{} triangles", part.count / 3));
            ui.horizontal(|ui| {
                if ui
                    .button("Frame surface")
                    .on_hover_text("F over viewport · double-click a surface row")
                    .clicked()
                {
                    frame = true;
                }
                whole = ui.button("Select whole model").clicked();
            });
            ui.weak(
                "Inspecting imported geometry. Select the whole model to transform or edit it.",
            );
            ui.separator();
            ui.strong("Source material");
            ui.label(format!("Base color: {:.3?}", part.color));
            if let Some(cutoff) = part.alpha_cutoff {
                ui.label(format!("Alpha cutoff: {cutoff:.3}"));
            }
            let pbr = part.shading.as_ref().map(|s| &s.material);
            if let Some(material) = pbr {
                ui.label(format!(
                    "Metallic: {:.3} · Roughness: {:.3}",
                    material.metallic, material.roughness
                ));
                ui.label(format!(
                    "Normal scale: {:.3} · Occlusion: {:.3}",
                    material.normal_scale, material.occlusion_strength
                ));
                ui.label(format!("Emissive: {:.3?}", material.emissive_factor));
                ui.label(if material.double_sided {
                    "Double-sided"
                } else {
                    "Single-sided"
                });
            } else {
                ui.weak("Diffuse material");
            }
            map_details(
                ui,
                "Base color (sRGB)",
                part.image.as_deref(),
                pbr.map(|m| m.base_color_sampler),
            );
            if let Some(material) = pbr {
                for (label, map) in [
                    ("Normal (linear)", &material.normal),
                    (
                        "Metallic / roughness (linear)",
                        &material.metallic_roughness,
                    ),
                    ("Occlusion (linear)", &material.occlusion),
                    ("Emissive (sRGB)", &material.emissive),
                ] {
                    map_details(
                        ui,
                        label,
                        map.as_ref().map(|m| m.image.as_ref()),
                        map.as_ref().map(|m| m.sampler),
                    );
                }
            }
        }
        if whole {
            self.editor.select_object(self.editor.selected.clone());
        } else if let Some(index) = choose {
            self.editor.finish_gesture();
            let result = self.editor.select_surface(index);
            self.result(result);
        }
        if frame {
            self.hierarchy_frame_requested = true;
        }
        self.editor.selected_surface().is_some()
    }

    pub fn surface_overlay(&self, ui: &egui::Ui, rect: Rect, projection: Mat4) -> Result<()> {
        if !self.surface_graphics_ready() {
            return Ok(());
        }
        let Some(corners) = self.editor.selected_surface_corners(self.layer())? else {
            return Ok(());
        };
        let painter = ui.painter().with_clip_rect(rect);
        let color = Color32::from_rgb(255, 205, 90);
        let project = |clip: glam::Vec4| {
            let ndc = clip.truncate() / clip.w;
            Pos2::new(
                rect.left() + (ndc.x + 1.0) * rect.width() * 0.5,
                rect.top() + (1.0 - ndc.y) * rect.height() * 0.5,
            )
        };
        for i in 0..8 {
            for axis in 0..3 {
                let j = i ^ (1 << axis);
                if j > i
                    && let Some((a, b)) = colliders::clip_edge(
                        projection * corners[i].extend(1.0),
                        projection * corners[j].extend(1.0),
                    )
                {
                    painter.line_segment([project(a), project(b)], egui::Stroke::new(1.5, color));
                }
            }
        }
        let selected = self.editor.selected_surface().unwrap();
        painter.text(
            rect.left_bottom() + egui::vec2(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            format!("Surface {} · F to frame", selected.index + 1),
            egui::FontId::proportional(13.0),
            color,
        );
        Ok(())
    }
}

fn map_details(
    ui: &mut egui::Ui,
    label: &str,
    image: Option<&ImageData>,
    sampler: Option<Sampler>,
) {
    if let Some(image) = image {
        let response = ui.label(format!("{label}: {} × {}", image.width, image.height));
        if let Some(s) = sampler {
            response.on_hover_text(format!(
                "U: {:?} · V: {:?}\nMin: {:?} · Mag: {:?} · Mips: {:?}",
                s.wrap_u, s.wrap_v, s.min, s.mag, s.mip
            ));
        }
    } else {
        ui.weak(format!("{label}: none"));
    }
}
