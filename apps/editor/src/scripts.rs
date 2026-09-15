//! Script Manager: the coding alternative to a blueprint graph, binding script assets in order.
use super::*;

impl App {
    /// Draws the ordered script list of the selected object.
    ///
    /// Scripts are catalog assets, so the picker offers what the scene already imported; a `.rs`
    /// file is added to the catalog with the normal Import action.
    pub fn script_manager_inspector(
        &mut self,
        ui: &mut egui::Ui,
        object: &mut bozzard_scene::Object,
    ) {
        if object.script_manager.is_none() {
            return;
        }
        let scripts: Vec<(String, String)> = self
            .editor
            .scene()
            .assets
            .iter()
            .filter(|(_, source)| source.kind == bozzard_scene::AssetKind::Script)
            .map(|(id, source)| (id.clone(), source.path.clone()))
            .collect();
        egui::CollapsingHeader::new("SCRIPT MANAGER")
            .id_salt((&object.id, "script_manager"))
            .default_open(true)
            .show(ui, |ui| {
                let editing = self.editor.play.is_none()
                    && self.loading.is_none()
                    && self.dialog.is_none();
                let manager = object.script_manager.as_mut().expect("checked above");
                ui.add_enabled_ui(editing, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .button("+ Add script")
                            .on_hover_text("Add an attachment; pick its script below")
                            .clicked()
                        {
                            manager.scripts.push(bozzard_scene::ScriptAttachment {
                                enabled: true,
                                script: String::new(),
                            });
                        }
                        if scripts.is_empty() {
                            ui.weak("No script assets yet — import a .rs file first");
                        }
                    });
                });
                let mut remove = None;
                let mut move_to = None;
                let count = manager.scripts.len();
                for (index, attachment) in manager.scripts.iter_mut().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_enabled(
                            editing,
                            egui::Checkbox::without_text(&mut attachment.enabled),
                        )
                        .on_hover_text("Run this script during Play");
                        let selected = if attachment.script.is_empty() {
                            "(choose a script)".to_owned()
                        } else {
                            attachment.script.clone()
                        };
                        ui.push_id(index, |ui| {
                            egui::ComboBox::from_id_salt("script")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for (id, path) in &scripts {
                                        ui.selectable_value(
                                            &mut attachment.script,
                                            id.clone(),
                                            format!("{id} · {path}"),
                                        );
                                    }
                                });
                        });
                        if ui
                            .add_enabled(editing && index > 0, egui::Button::new("↑").small())
                            .on_hover_text("Run earlier")
                            .clicked()
                        {
                            move_to = Some((index, index - 1));
                        }
                        if ui
                            .add_enabled(
                                editing && index + 1 < count,
                                egui::Button::new("↓").small(),
                            )
                            .on_hover_text("Run later")
                            .clicked()
                        {
                            move_to = Some((index, index + 1));
                        }
                        if ui
                            .add_enabled(editing, egui::Button::new("Remove").small())
                            .on_hover_text("Detach script (Undo restores it)")
                            .clicked()
                        {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    manager.scripts.remove(index);
                }
                if let Some((from, to)) = move_to {
                    manager.scripts.swap(from, to);
                }
                ui.weak("Top to bottom · Rhai hooks: on_start, on_update(dt), on_enable, on_disable, \
                         on_object_enter/exit, on_overlap_enter/exit, on_collision_enter, on_destroy");
            });
    }
}
