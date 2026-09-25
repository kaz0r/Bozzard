//! Selection-wide component edits. The core command validates and publishes one document change.
use super::*;
use bozzard_editor::{BulkTransformAxis, BulkTransformMode};
use bozzard_scene::{FieldKind, FieldValue};

#[derive(Clone)]
pub(crate) struct Cache {
    revision: u64,
    path: PathBuf,
    ids: Vec<String>,
    indices: Vec<usize>,
    common: Vec<&'static str>,
    presence: Vec<(&'static str, usize)>,
}

impl App {
    pub(super) fn multi_inspector(&mut self, ui: &mut egui::Ui) {
        let ids = self.hierarchy_state.selected_objects();
        let snapshot = self.editor.scene_snapshot();
        if self.multi_inspector_cache.as_ref().is_none_or(|cache| {
            cache.revision != self.editor.revision()
                || cache.path != self.editor.path
                || cache.ids != ids
        }) {
            let indices: Vec<_> = ids
                .iter()
                .filter_map(|id| snapshot.objects.iter().position(|o| &o.id == id))
                .collect();
            let objects: Vec<_> = indices.iter().map(|&i| &snapshot.objects[i]).collect();
            let presence: Vec<_> = bozzard_scene::components()
                .map(|entry| {
                    (
                        entry.name,
                        objects.iter().filter(|o| (entry.present)(o)).count(),
                    )
                })
                .collect();
            let common = presence
                .iter()
                .filter(|(_, count)| *count == objects.len())
                .map(|(name, _)| *name)
                .collect();
            self.multi_inspector_cache = Some(Cache {
                revision: self.editor.revision(),
                path: self.editor.path.clone(),
                ids: ids.clone(),
                indices,
                common,
                presence,
            });
        }
        let cache = self.multi_inspector_cache.as_ref().unwrap().clone();
        let objects: Vec<_> = cache
            .indices
            .iter()
            .map(|&i| &snapshot.objects[i])
            .collect();
        if objects.len() != ids.len() || ids.len() < 2 {
            ui.weak("Selection changed; choose the objects again.");
            return;
        }
        ui.strong(format!("{} objects selected", ids.len()));
        ui.small("Changes apply to every selected object in this scene. A failed validation leaves all objects unchanged.");
        ui.add_enabled_ui(self.editor.play.is_none() && self.loading.is_none(), |ui| {
            egui::ScrollArea::vertical().id_salt("multi-properties").show(ui, |ui| {
                self.multi_transforms(ui, &ids);
                let common: Vec<_> = cache.common.iter().filter_map(|name| bozzard_scene::component_type(name)).collect();
                ui.separator();
                ui.strong(format!("Common components · {}", common.len()));
                for entry in bozzard_scene::components() {
                    let count = cache.presence.iter().find(|(name, _)| *name == entry.name).map_or(0, |(_, count)| *count);
                    if count > 0 && count < objects.len() {
                        ui.weak(format!("{} · on {count}/{} objects", entry.label, objects.len()));
                    }
                }
                for entry in &common {
                    ui.collapsing(entry.label, |ui| {
                        for field in (entry.fields)() {
                            if !matches!(field.kind, FieldKind::Bool | FieldKind::Number { .. } | FieldKind::Integer { .. } | FieldKind::Vector { .. })
                                || !objects.iter().all(|o| field.visible.is_none_or(|show| show(o))) { continue; }
                            let Some(first) = (entry.get)(objects[0], field.key) else { continue; };
                            let values: Option<Vec<_>> = objects.iter().map(|o| (entry.get)(o, field.key)).collect();
                            let Some(values) = values else { continue; };
                            let mixed = values.iter().any(|v| *v != first);
                            ui.push_id((&ids, field.key), |ui| {
                                if mixed { ui.weak("Mixed values"); }
                                if matches!(field.kind, FieldKind::Vector { .. }) {
                                    let id = ui.make_persistent_id("bulk-vector-draft");
                                    let mut draft = ui.data_mut(|d| d.get_temp::<[f32; 3]>(id)).unwrap_or_else(|| first.vector().unwrap_or_default());
                                    let axes = match field.kind { FieldKind::Vector { axes, .. } => axes as usize, _ => 3 };
                                    ui.label(field.label);
                                    ui.horizontal(|ui| {
                                        for i in 0..axes {
                                            ui.add(egui::DragValue::new(&mut draft[i]).speed(0.05).prefix(["X ", "Y ", "Z "][i]));
                                        }
                                    });
                                    if ui.button("Set on selection").clicked() {
                                        let result = self.editor.bulk_set_field(&ids, entry.name, field.key, FieldValue::Vector(draft));
                                        self.result(result);
                                    }
                                    ui.data_mut(|d| d.insert_temp(id, draft));
                                } else {
                                    let mut edited = first.clone();
                                    let mut views = snapshot.views.clone();
                                    match crate::component_ui::widget(ui, field, &mut edited, &snapshot, &mut views) {
                                        Ok(true) => {
                                            self.editor.begin_gesture("Edit selection");
                                            let result = self.editor.bulk_set_field(&ids, entry.name, field.key, edited);
                                            if result.is_err() { let _ = self.editor.cancel_gesture(); }
                                            self.result(result);
                                        }
                                        Ok(false) => {}
                                        Err(error) => self.result(Err(error)),
                                    }
                                }
                            });
                        }
                    });
                }
                ui.separator();
                ui.menu_button("Add component to selection", |ui| {
                    for entry in bozzard_scene::components() {
                        let present = cache.presence.iter().find(|(name, _)| *name == entry.name).map_or(0, |(_, count)| *count);
                        let missing = objects.len() - present;
                        if missing > 0 && objects.iter().all(|o| (entry.present)(o) || (entry.available)(o))
                            && ui.button(format!("{} · add to {missing} missing", entry.label)).clicked() {
                            let result = self.editor.bulk_add_component(&ids, entry.name, self.layer());
                            self.result(result);
                            ui.close();
                        }
                    }
                });
                let confirmation = ui.make_persistent_id("bulk-remove-confirm");
                let mut pending = ui.data_mut(|d| d.get_temp::<String>(confirmation));
                ui.menu_button("Remove component from selection…", |ui| {
                    for entry in bozzard_scene::components() {
                        let present = cache.presence.iter().find(|(name, _)| *name == entry.name).map_or(0, |(_, count)| *count);
                        if present > 0 && ui.button(format!("{} · remove from {present}", entry.label)).clicked() { pending = Some(entry.name.to_owned()); ui.close(); }
                    }
                });
                if let Some(name) = pending.clone() {
                    let label = bozzard_scene::component_type(&name).map_or(name.as_str(), |e| e.label);
                    let present = cache.presence.iter().find(|(entry, _)| *entry == name).map_or(0, |(_, count)| *count);
                    ui.colored_label(Color32::YELLOW, format!("Remove {label} from {present} selected object(s)? Dependent components may also be removed. Scene Undo restores them."));
                    ui.horizontal(|ui| {
                        if ui.button("Remove from all").clicked() {
                            let result = self.editor.bulk_remove_component(&ids, &name);
                            self.result(result);
                            pending = None;
                        }
                        if ui.button("Cancel").clicked() { pending = None; }
                    });
                }
                if let Some(name) = pending { ui.data_mut(|d| d.insert_temp(confirmation, name)); }
                else { ui.data_mut(|d| d.remove::<String>(confirmation)); }
            });
        });
    }

    fn multi_transforms(&mut self, ui: &mut egui::Ui, ids: &[String]) {
        ui.collapsing("TRANSFORM · selection", |ui| {
            ui.small("Absolute sets the entered XYZ on each object. Relative adds XYZ to each object's current local transform. Parent and child values are each edited in local space.");
            let mode_id = ui.make_persistent_id("bulk-transform-mode");
            let mut relative = ui.data_mut(|d| d.get_temp::<bool>(mode_id)).unwrap_or(false);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut relative, false, "Absolute");
                ui.selectable_value(&mut relative, true, "Relative");
            });
            ui.data_mut(|d| d.insert_temp(mode_id, relative));
            for (name, axis) in [("Position", BulkTransformAxis::Position), ("Rotation °", BulkTransformAxis::Rotation), ("Scale", BulkTransformAxis::Scale)] {
                ui.push_id((ids, name), |ui| {
                    let id = ui.make_persistent_id("draft");
                    let mut value = ui.data_mut(|d| d.get_temp::<[f32; 3]>(id)).unwrap_or([0.; 3]);
                    ui.label(name);
                    ui.horizontal(|ui| {
                        for i in 0..3 { ui.add(egui::DragValue::new(&mut value[i]).speed(0.05).prefix(["X ", "Y ", "Z "][i])); }
                        if ui.button("Apply").clicked() {
                            let mode = if relative { BulkTransformMode::Relative } else { BulkTransformMode::Absolute };
                            let result = self.editor.bulk_transform(ids, axis, mode, value);
                            self.result(result);
                        }
                    });
                    ui.data_mut(|d| d.insert_temp(id, value));
                });
            }
        });
    }
}
