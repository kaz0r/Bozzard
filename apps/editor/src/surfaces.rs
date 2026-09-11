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

pub(super) fn surface_matches(index: usize, part: &MeshPart, query: &str) -> bool {
    query.is_empty()
        || part.name.to_lowercase().contains(query)
        || surface_label(index, part).to_lowercase().contains(query)
}

impl App {
    /// Imported parts are inspection targets, not scene objects or reparent targets.
    pub fn hierarchy_surfaces(
        &mut self,
        ui: &mut egui::Ui,
        object: &str,
        depth: usize,
        query: &str,
    ) {
        let rows: Vec<_> = self
            .editor
            .object_mesh(object)
            .into_iter()
            .flat_map(|mesh| mesh.parts.iter().enumerate())
            .filter(|(index, part)| surface_matches(*index, part, query))
            .map(|(index, part)| {
                (
                    index,
                    format!("{} · {}", part.name, surface_label(index, part)),
                )
            })
            .collect();
        let enabled = self.editor.play.is_none()
            && self.drag.is_none()
            && !self.mouse_captured
            && self.hierarchy_rename.is_none()
            && self.dialog.is_none()
            && !self.confirm_discard;
        ui.add_enabled_ui(enabled, |ui| {
            for (index, label) in rows {
                ui.push_id(("hierarchy-surface", object, index), |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space((depth.min(12) * 12 + 18) as f32);
                        let selected = self.editor.selected.as_deref() == Some(object)
                            && self.editor.selected_surface().is_some_and(|s| s.index == index);
                        let row = ui.selectable_label(selected, label)
                            .on_hover_text("Editable surface · W/E/R transforms · Double-click to frame · Select the model row for components");
                        if row.clicked() || row.double_clicked() {
                            self.editor.finish_gesture();
                            let result = self.editor.select_pick(Some(bozzard_editor::Pick {
                                object: object.to_owned(), surface: Some(index),
                            }));
                            self.result(result);
                        }
                        if row.double_clicked() {
                            self.hierarchy_frame_requested = true;
                        }
                    });
                });
            }
        });
    }

    pub fn surface_graphics_ready(&self) -> bool {
        self.editor
            .selected_object()
            .and_then(|o| o.drawable.as_ref())
            .is_none_or(|d| match &d.mesh {
                Mesh::Asset(id) => self.residency.is_current(&self.editor.assets, id),
                _ => true,
            })
    }
    /// Returns true while editing an imported surface instead of its owner's components.
    pub fn surface_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(mesh) = self.editor.selected_mesh() else {
            return false;
        };
        if mesh.parts.is_empty() || self.editor.play.is_some() {
            return false;
        }
        let selected = self.editor.selected_surface().map(|s| s.index);
        let original_override = self.editor.selected_material_override().ok();
        let saved = &self
            .editor
            .selected_object()
            .unwrap()
            .drawable
            .as_ref()
            .unwrap()
            .material_overrides;
        let inactive = saved
            .iter()
            .filter(|value| {
                mesh.parts
                    .get(value.surface as usize)
                    .is_none_or(|part| part.source_key != value.source)
            })
            .count();
        let mut material_edit = None;
        let mut reset_material = false;
        let mut choose = None;
        let mut frame = false;
        let mut whole = false;
        egui::CollapsingHeader::new(format!("Imported surfaces ({})", mesh.parts.len()))
            .id_salt("imported-surfaces")
            .default_open(false)
            .show(ui, |ui| {
                if inactive > 0 {
                    ui.colored_label(
                        Color32::YELLOW,
                        format!("{inactive} saved overrides no longer match this source."),
                    );
                }
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
                    .filter(|(index, part)| surface_matches(*index, part, &query))
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
                                        format!(
                                            "{}{}",
                                            surface_label(index, part),
                                            if saved.iter().any(|v| v.surface as usize == index
                                                && v.source == part.source_key)
                                            {
                                                " •"
                                            } else {
                                                ""
                                            }
                                        ),
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
            ui.weak("Edit this surface here or with W / E / R gizmos. Components and physics belong to the whole model.");
            if let Some(original) = &original_override {
                ui.separator();
                let mut value = original.clone();
                let has_saved = saved.iter().any(|v| v.surface as usize == index);
                ui.push_id(index, |ui| {
                    egui::CollapsingHeader::new("TRANSFORM")
                        .default_open(true)
                        .show(ui, |ui| {
                            inspector::vector(ui, "Offset", &mut value.transform.translation, 0.05);
                            inspector::vector(
                                ui,
                                "Rotation °",
                                &mut value.transform.rotation_degrees,
                                0.5,
                            );
                            inspector::vector(ui, "Scale", &mut value.transform.scale, 0.02);
                            if ui.small_button("Reset transform").clicked() {
                                value.transform = Transform::default();
                            }
                            ui.small(
                                "Model-space offset; rotate/scale about this surface’s center.",
                            );
                        });
                    ui.separator();
                    reset_material =
                        material_controls(ui, &mut value, part, has_saved, self.editor.scene());
                });
                if value != *original {
                    material_edit = Some(value);
                }
            }
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
        if reset_material {
            self.editor.finish_gesture();
            if let Some(original) = original_override {
                let mut value = bozzard_scene::SurfaceMaterialOverride::inherited(
                    original.surface,
                    original.source,
                );
                value.transform = original.transform;
                let result = self.editor.set_selected_material_override(value);
                self.result(result);
            }
        } else if let Some(value) = material_edit {
            self.editor.begin_gesture("Edit surface");
            let result = self.editor.set_selected_material_override(value);
            self.result(result);
        }
        if whole {
            self.editor.finish_gesture();
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

fn material_controls(
    ui: &mut egui::Ui,
    value: &mut bozzard_scene::SurfaceMaterialOverride,
    part: &MeshPart,
    has_saved: bool,
    scene: &bozzard_scene::Scene,
) -> bool {
    let mut reset = false;
    ui.horizontal(|ui| {
        ui.strong("Material override");
        reset = ui
            .add_enabled(has_saved, egui::Button::new("Reset override"))
            .on_hover_text("Return this surface to its source material")
            .clicked();
    });
    let mut replace = value.texture.is_some();
    if ui.checkbox(&mut replace, "Override texture / effect")
        .on_hover_text("Uncheck to inherit. Import an image in Content Browser, then choose it here or Assign to selected.").changed() {
        value.texture = replace.then_some(Texture::White);
    }
    if let Some(texture) = &mut value.texture {
        inspector::texture_control(ui, texture, scene);
    }
    ui.horizontal(|ui| {
        ui.label("UV repeat");
        for uv in &mut value.uv_scale {
            ui.add(egui::DragValue::new(uv).speed(0.05).range(0.001..=1000.0));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Tint");
        inspector::color_edit_button_rgb(ui, &mut value.tint);
    });
    if let Some(shading) = &part.shading {
        factor_control(
            ui,
            "Metallic",
            &mut value.metallic,
            shading.material.metallic,
        );
        factor_control(
            ui,
            "Roughness",
            &mut value.roughness,
            shading.material.roughness,
        );
    } else {
        ui.weak("Diffuse surface: texture and tint");
    }
    ui.weak("Only this object's surface. Tint and factors multiply the existing maps.");
    reset
}

fn factor_control(ui: &mut egui::Ui, label: &str, factor: &mut Option<f32>, source: f32) {
    ui.horizontal(|ui| {
        let mut enabled = factor.is_some();
        if ui.checkbox(&mut enabled, label)
            .on_hover_text("Override this factor for this object's surface. Uncheck to inherit the source material.")
            .changed()
        {
            *factor = enabled.then_some(source);
        }
        let mut value = factor.unwrap_or(source);
        ui.spacing_mut().slider_width = (ui.available_width() - 55.0).clamp(50.0, 120.0);
        if ui.add_enabled(enabled, egui::Slider::new(&mut value, 0.0..=1.0)).changed() {
            *factor = Some(value);
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_and_inspector_search_source_mesh_or_material() {
        let mut part = MeshPart {
            source_key: "source".into(),
            name: "Car / Wheel / Surface 1".into(),
            material_name: Some("Rubber".into()),
            start: 0,
            count: 3,
            color: [1.0; 4],
            image: None,
            alpha_cutoff: None,
            shading: None,
        };
        for query in ["", "wheel", "rubber", "surface 1"] {
            assert!(surface_matches(0, &part, query));
        }
        assert!(!surface_matches(0, &part, "glass"));
        part.material_name = None;
        assert!(surface_matches(0, &part, "wheel"));
        assert!(!surface_matches(0, &part, "rubber"));
    }
}
