//! Author a pack specification in JSON, then cook it with the same cancellable builder as the CLI.
use super::*;
use crate::loading::Loading;
use bozzard_assets::job::Job;

impl App {
    pub fn bundle_dialog(&mut self, ctx: &egui::Context, dialog: &mut files::Dialog) -> bool {
        let mut keep = true;
        egui::Window::new("Build content pack")
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Bundle scenes and assets under reusable addresses.");
                ui.weak("Choose a pack specification that lists the content and cooking target.");
                ui.add_space(10.0);
                ui.label("Pack specification");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut dialog.path).desired_width(380.0));
                    if ui.button("Choose file…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_directory(&dialog.directory)
                            .add_filter("Content specification", &["json"])
                            .pick_file()
                        {
                            dialog.path = path.display().to_string();
                        }
                        self.last_frame = Instant::now();
                    }
                });
                ui.label("Release folder name");
                ui.text_edit_singleline(&mut dialog.project_name);
                ui.horizontal(|ui| {
                    ui.label("Build into");
                    if ui.button("Choose folder…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_directory(&dialog.directory)
                            .set_can_create_directories(true)
                            .pick_folder()
                        {
                            dialog.directory = path;
                        }
                        self.last_frame = Instant::now();
                    }
                });
                ui.label(dialog.directory.display().to_string());
                ui.weak("Uses saved source files. Save scene changes before building.");
                let destination = export::destination(&dialog.directory, &dialog.project_name);
                if let Ok(path) = &destination {
                    ui.label(format!(
                        "New release: {}",
                        path.file_name().unwrap().to_string_lossy()
                    ));
                }
                if let Some(error) = &dialog.export_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.loading.is_none()
                                && !dialog.path.trim().is_empty()
                                && destination.is_ok(),
                            egui::Button::new("Build pack"),
                        )
                        .clicked()
                    {
                        let spec = PathBuf::from(&dialog.path);
                        let result = destination.and_then(|destination| {
                            Job::start("Preparing content pack", move |progress| {
                                bozzard_project::content::prepare_pack(
                                    &spec,
                                    &destination,
                                    &progress,
                                )
                            })
                        });
                        match result {
                            Ok(job) => {
                                self.loading = Some(Loading::Bundle(job));
                                keep = false;
                            }
                            Err(error) => dialog.export_error = Some(format!("{error:#}")),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        keep
    }
}
