use super::*;
#[derive(Clone, Copy)]
pub enum Kind {
    Open,
    Save,
    Import,
}
pub struct Dialog {
    pub kind: Kind,
    directory: PathBuf,
    path: String,
    overwrite: bool,
}
impl Dialog {
    pub fn new(kind: Kind, path: &Path) -> Self {
        let directory = bozzard_editor::root(path).to_path_buf();
        let directory = if directory.is_dir() {
            directory
        } else {
            std::env::current_dir().unwrap_or_default()
        };
        Self {
            kind,
            directory,
            path: path.display().to_string(),
            overwrite: false,
        }
    }
}
impl App {
    pub fn file_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let mut chosen = None;
        let title = match dialog.kind {
            Kind::Open => "Open scene",
            Kind::Save => "Save scene as",
            Kind::Import => "Import image or model",
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
                        let p = PathBuf::from(&dialog.path);
                        if p.is_dir() {
                            dialog.directory = p;
                        }
                    }
                    if ui.button("Parent folder").clicked()
                        && let Some(parent) = dialog.directory.parent()
                    {
                        dialog.directory = parent.to_path_buf();
                    }
                    ui.weak(dialog.directory.display().to_string());
                });
                egui::ScrollArea::vertical().max_height(280.0).show(
                    ui,
                    |ui| match std::fs::read_dir(&dialog.directory) {
                        Ok(entries) => {
                            let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                            entries.sort_by_key(|e| (!e.path().is_dir(), e.file_name()));
                            for entry in entries {
                                let path = entry.path();
                                let folder = path.is_dir();
                                let ext = path
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or("")
                                    .to_ascii_lowercase();
                                let valid = folder
                                    || match dialog.kind {
                                        Kind::Import => matches!(
                                            ext.as_str(),
                                            "png" | "jpg" | "jpeg" | "obj" | "gltf" | "glb"
                                        ),
                                        _ => ext == "json",
                                    };
                                if !valid {
                                    continue;
                                }
                                let label = format!(
                                    "{} {}",
                                    if folder { "▸" } else { "  " },
                                    entry.file_name().to_string_lossy()
                                );
                                let response = ui.selectable_label(
                                    dialog.path == path.display().to_string(),
                                    label,
                                );
                                if response.clicked() {
                                    dialog.path = path.display().to_string();
                                    dialog.overwrite = false;
                                }
                                if response.double_clicked() && folder {
                                    dialog.directory = path;
                                }
                            }
                        }
                        Err(e) => {
                            ui.colored_label(Color32::LIGHT_RED, e.to_string());
                        }
                    },
                );
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
                        if matches!(dialog.kind, Kind::Save) && path.exists() && !dialog.overwrite {
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
                    Kind::Open => self.request(Pending::Open(path)),
                    Kind::Save => self.save_scene(path),
                    Kind::Import => {
                        let r = self.editor.import(&path).map(|id| {
                            self.status = format!("Imported {id}");
                            self.asset_browser.reveal(id);
                            self.workspace.assets_visible = true;
                        });
                        self.result(r);
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
                ui.label("Save your changes before continuing?");
                ui.horizontal(|ui| {
                    if ui.button("Save and continue").clicked() {
                        let r = self.editor.save(&self.editor.path.clone());
                        if r.is_ok() {
                            self.perform_pending();
                        } else {
                            self.result(r);
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
