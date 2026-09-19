use super::*;
mod browser;
pub use browser::Browser;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    NewProject,
    Open,
    OpenAdditive,
    Save,
    Import,
    Export,
    Exported,
    Bundle,
    LoadBlueprint,
    SaveBlueprint,
    LoadShaderGraph,
    SaveShaderGraph,
}
pub struct Dialog {
    pub kind: Kind,
    pub project_name: String,
    pub cook_target: bozzard_project::CookTarget,
    pub project_template: bozzard_project::ProjectTemplate,
    pub export_error: Option<String>,
    pub blueprint_target: Option<(PathBuf, u64, String, usize)>,
    pub shader_target: Option<(PathBuf, u64, String)>,
    pub(super) directory: PathBuf,
    pub(super) path: String,
    overwrite: bool,
}
impl Dialog {
    pub fn new(kind: Kind, path: &Path) -> Self {
        let directory = bozzard_editor::root(path).to_path_buf();
        Self {
            kind,
            project_name: String::new(),
            cook_target: bozzard_project::CookTarget::Universal,
            project_template: bozzard_project::ProjectTemplate::ThirdPerson,
            export_error: None,
            blueprint_target: None,
            shader_target: None,
            directory,
            path: path.display().to_string(),
            overwrite: false,
        }
    }
}
impl App {
    pub fn file_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            self.file_browser.clear();
            return;
        };
        if matches!(dialog.kind, Kind::NewProject) {
            if self.project_dialog(ctx, &mut dialog) {
                self.dialog = Some(dialog);
            }
            return;
        }
        if matches!(dialog.kind, Kind::Export | Kind::Exported) {
            if self.export_dialog(ctx, &mut dialog) {
                self.dialog = Some(dialog);
            }
            return;
        }
        if matches!(dialog.kind, Kind::Bundle) {
            if self.bundle_dialog(ctx, &mut dialog) {
                self.dialog = Some(dialog);
            }
            return;
        }
        let mut keep = true;
        let mut chosen = None;
        let title = match dialog.kind {
            Kind::NewProject => unreachable!("project wizard uses its own dialog"),
            Kind::Open => "Open scene",
            Kind::OpenAdditive => "Open scene additively",
            Kind::Save => "Save scene as",
            Kind::Import => "Import image, model or prefab",
            Kind::Export | Kind::Exported => unreachable!("export uses its own dialog"),
            Kind::Bundle => unreachable!("bundle uses its own dialog"),
            Kind::LoadBlueprint => "Load and attach Blueprint copy",
            Kind::SaveBlueprint => "Save Blueprint graph",
            Kind::LoadShaderGraph => "Load shader graph",
            Kind::SaveShaderGraph => "Save shader graph",
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(true)
            .default_width(650.0)
            .show(ctx, |ui| {
                ui.label("Path");
                if ui.text_edit_singleline(&mut dialog.path).changed() {
                    dialog.overwrite = false;
                }
                ui.horizontal(|ui| {
                    if ui.button("Go to folder").clicked() {
                        dialog.directory = PathBuf::from(&dialog.path);
                    }
                    if ui.button("Parent folder").clicked()
                        && let Some(parent) = dialog.directory.parent()
                    {
                        dialog.directory = parent.to_path_buf();
                    }
                    ui.weak(dialog.directory.display().to_string());
                    if ui.button("Refresh").clicked() {
                        self.file_browser.clear();
                    }
                });
                self.file_browser.request(&dialog.directory, dialog.kind);
                match self.file_browser.entries() {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Reading folder… You can still enter a file path or cancel.");
                        });
                        ctx.request_repaint_after(Duration::from_millis(50));
                    }
                    Some(Err(error)) => {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    }
                    Some(Ok(entries)) => {
                        egui::ScrollArea::vertical().max_height(280.0).show_rows(
                            ui,
                            ui.spacing()
                                .interact_size
                                .y
                                .max(ui.text_style_height(&egui::TextStyle::Body)),
                            entries.len(),
                            |ui, rows| {
                                for entry in &entries[rows] {
                                    let response = ui.selectable_label(
                                        dialog.path == entry.path_text,
                                        &entry.label,
                                    );
                                    if response.clicked() {
                                        dialog.path.clone_from(&entry.path_text);
                                        dialog.overwrite = false;
                                    }
                                    if response.double_clicked() && entry.folder {
                                        dialog.directory.clone_from(&entry.path);
                                    }
                                }
                            },
                        );
                    }
                }
                if dialog.overwrite {
                    ui.colored_label(
                        Color32::YELLOW,
                        "This file exists. Click Replace to overwrite it.",
                    );
                }
                ui.horizontal(|ui| {
                    if ui
                        .button(if dialog.overwrite { "Replace" } else { title })
                        .clicked()
                    {
                        let path = PathBuf::from(&dialog.path);
                        if matches!(
                            dialog.kind,
                            Kind::Save | Kind::SaveBlueprint | Kind::SaveShaderGraph
                        ) && path.exists()
                            && !dialog.overwrite
                        {
                            dialog.overwrite = true;
                        } else {
                            chosen = Some(path);
                            keep = false;
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        if let Some(path) = chosen {
            let path = std::path::absolute(path);
            match path {
                Err(e) => self.result(Err(e.into())),
                Ok(path) => match dialog.kind {
                    Kind::NewProject => unreachable!(),
                    Kind::Open => self.request(Pending::Open(path)),
                    Kind::OpenAdditive => self.open_additive(path),
                    Kind::Save => self.save_scene(path),
                    Kind::Export | Kind::Exported => unreachable!(),
                    Kind::Bundle => unreachable!(),
                    Kind::Import => {
                        self.start_import(path);
                    }
                    Kind::LoadBlueprint | Kind::SaveBlueprint => {
                        let result = (|| {
                            let (scene_path, revision, object, index) = dialog
                                .blueprint_target
                                .as_ref()
                                .context("missing blueprint target")?;
                            ensure!(
                                *scene_path == self.editor.path
                                    && *revision == self.editor.revision(),
                                "Scene changed while choosing a blueprint file; try again"
                            );
                            ensure!(self.loading.is_none(), "Wait for loading to finish");
                            if matches!(dialog.kind, Kind::SaveBlueprint) {
                                self.editor.save_blueprint(object, *index, &path)
                            } else {
                                self.editor.load_blueprint(object, &path)
                            }
                        })();
                        if result.is_ok() {
                            if matches!(dialog.kind, Kind::LoadBlueprint)
                                && let Some((_, _, object, _)) = &dialog.blueprint_target
                            {
                                self.editor.select_object(Some(object.clone()));
                                self.open_last_blueprint();
                            }
                            self.workspace.blueprints_visible = true;
                            self.dock_focus = Some(docking::Pane::Scene);
                            self.status =
                                "Blueprint file ready · Attachments are independent copies".into();
                        }
                        self.result(result);
                    }
                    Kind::LoadShaderGraph | Kind::SaveShaderGraph => {
                        let result = (|| {
                            let (scene_path, revision, object) = dialog
                                .shader_target
                                .as_ref()
                                .context("missing shader graph target")?;
                            ensure!(
                                *scene_path == self.editor.path
                                    && *revision == self.editor.revision(),
                                "Scene changed while choosing a shader graph file; try again"
                            );
                            ensure!(self.loading.is_none(), "Wait for loading to finish");
                            if matches!(dialog.kind, Kind::SaveShaderGraph) {
                                self.editor.save_shader_graph(object, &path)
                            } else {
                                self.editor.load_shader_graph(object, &path)
                            }
                        })();
                        if result.is_ok() && matches!(dialog.kind, Kind::LoadShaderGraph) {
                            if let Some((_, _, object)) = &dialog.shader_target {
                                self.editor.select_object(Some(object.clone()));
                            }
                            self.workspace.shaders_visible = true;
                            self.dock_focus = Some(docking::Pane::Scene);
                            self.status =
                                "Shader graph ready · Attachments are independent copies".into();
                        }
                        self.result(result);
                    }
                },
            }
        }
        if keep {
            self.dialog = Some(dialog);
        }
    }
    pub fn discard_dialog(&mut self, ctx: &egui::Context) {
        if !self.confirm_discard {
            return;
        }
        egui::Window::new("Unsaved scene changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Save changes to {} before continuing?",
                    self.editor.scene().name
                ));
                ui.small(self.editor.path.display().to_string());
                ui.horizontal(|ui| {
                    if ui.button("Save and continue").clicked() {
                        self.save_scene(self.editor.path.clone());
                        if self.loading.is_some() {
                            self.continue_after_save = true;
                            self.confirm_discard = false;
                        }
                    }
                    if ui.button("Discard changes").clicked() {
                        self.perform_pending();
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending = None;
                        self.confirm_discard = false;
                    }
                });
            });
    }
}
