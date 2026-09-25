use super::*;

impl App {
    /// Invalidate state keyed by a scene-local object ID or document revision.
    pub(super) fn scene_activated(&mut self, retain_camera: bool) {
        if !retain_camera {
            self.workspace.restore_scene(&self.editor.path);
        }
        self.workspace.scene_path = Some(self.editor.path.clone());
        self.workspace.select_available_view(self.editor.scene());
        self.viewport_stamp = None;
        self.hierarchy_state = Default::default();
        self.multi_inspector_cache = None;
        self.hierarchy_rename = None;
        self.hierarchy_search.clear();
        self.surface_search.clear();
        self.blueprint_pane = Default::default();
        self.shader_pane = Default::default();
        self.asset_browser = Default::default();
        self.effects_preview = None;
        self.drag = None;
        self.canvas_drag = None;
        self.gameplay_controls.reset();
        if let Some(refresh) = &self.refresh {
            refresh.job.cancel();
        }
        self.reload_paused = false;
        self.residency.retry_failed();
    }

    pub(super) fn open_additive(&mut self, path: PathBuf) {
        let result = (|| {
            ensure!(
                self.loading.is_none() && self.editor.play.is_none(),
                "Stop Play and wait for loading to finish"
            );
            if let Some(id) = self.open_scenes.find_path(&self.editor, &path) {
                self.open_scenes.activate(&mut self.editor, id)?;
                self.scene_activated(true);
            } else {
                ensure!(
                    self.open_scenes.len() < bozzard_editor::OpenScenes::LIMIT,
                    "open scene limit: 16"
                );
                self.loading = Some(loading::Loading::OpenAdditive(Editor::open_job(path)?));
            }
            Ok(())
        })();
        self.result(result);
    }

    pub(super) fn scene_documents(&mut self, ui: &mut egui::Ui) {
        if self.open_scenes.len() < 2 {
            return;
        }
        let mut activate = None;
        let mut visibility = None;
        ui.add_enabled_ui(
            self.loading.is_none()
                && self.editor.play.is_none()
                && self.dialog.is_none()
                && !self.confirm_discard
                && !self.mouse_captured,
            |ui| {
                ui.strong("Open scenes");
                for (id, editor) in self.open_scenes.documents(&self.editor) {
                    ui.horizontal(|ui| {
                        let mut visible = self.open_scenes.visible(id);
                        if !editor.is_prefab_source()
                            && ui
                                .checkbox(&mut visible, "")
                                .on_hover_text("Visible in the shared viewport")
                                .changed()
                        {
                            visibility = Some((id, visible));
                        }
                        if ui
                            .selectable_label(
                                id == self.open_scenes.active(),
                                format!(
                                    "{}{}",
                                    editor.scene().name,
                                    if editor.dirty() { " *" } else { "" }
                                ),
                            )
                            .on_hover_text(editor.path.display().to_string())
                            .clicked()
                        {
                            activate = Some(id);
                        }
                    });
                }
                ui.weak(
                    "Select a scene to edit its hierarchy. Save and Play use the active scene.",
                );
                if self.editor.is_prefab_source() {
                    ui.weak("Prefab source · Isolated preview · Save writes the source hierarchy.");
                }
                ui.separator();
            },
        );
        if let Some((id, visible)) = visibility {
            self.open_scenes.set_visible(id, visible);
        }
        if let Some(id) = activate {
            let result = self.open_scenes.activate(&mut self.editor, id);
            if result.is_ok() {
                self.scene_activated(true);
            }
            self.result(result);
        }
    }
}
