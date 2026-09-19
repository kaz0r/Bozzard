use super::*;

impl App {
    pub fn show_project_wizard(&mut self) {
        let mut dialog = files::Dialog::new(files::Kind::NewProject, &self.editor.path);
        dialog.project_name = "My Game".into();
        dialog.path = dialog.directory.join("My Game").display().to_string();
        self.dialog = Some(dialog);
    }

    pub fn project_dialog(&mut self, ctx: &egui::Context, dialog: &mut files::Dialog) -> bool {
        let mut keep = true;
        let mut create = false;
        egui::Window::new("New project").collapsible(false).default_width(520.).show(ctx, |ui| {
            ui.label("Project name");
            ui.text_edit_singleline(&mut dialog.project_name);
            ui.label("New project folder");
            ui.text_edit_singleline(&mut dialog.path);
            ui.weak("Choose a new folder inside an existing directory.");
            egui::ComboBox::from_label("Template")
                .selected_text(dialog.project_template.label()).show_ui(ui, |ui| {
                    for template in bozzard_project::ProjectTemplate::ALL {
                        ui.selectable_value(&mut dialog.project_template, template, template.label());
                    }
                });
            ui.label(match dialog.project_template {
                bozzard_project::ProjectTemplate::ThirdPerson => "A playable 3D course with a character controller, platforms, collectibles and a goal.",
                bozzard_project::ProjectTemplate::Collect2d => "A playable 2D collection game with movement, scoring and an editable gameplay script.",
            });
            if let Some(error) = &dialog.export_error { ui.colored_label(Color32::LIGHT_RED, error); }
            ui.horizontal(|ui| {
                create = ui.button("Create project").clicked();
                if ui.button("Cancel").clicked() { keep = false; }
            });
        });
        if create {
            let result = bozzard_project::create_project(
                Path::new(&dialog.path),
                &dialog.project_name,
                dialog.project_template,
            )
            .and_then(|manifest| bozzard_project::Project::load(&manifest).map(|(_, scene)| scene));
            match result {
                Ok(scene) => {
                    self.workspace.blueprints_visible = false;
                    self.workspace.shaders_visible = false;
                    self.request(Pending::Open(scene));
                    keep = false;
                }
                Err(error) => dialog.export_error = Some(format!("{error:#}")),
            }
        }
        keep
    }
}
