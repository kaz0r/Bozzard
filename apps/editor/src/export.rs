//! The export flow chooses a parent folder, creates a fresh game folder, then offers launch actions.
use super::*;

pub fn platform_label() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" if cfg!(target_os = "macos") => "Apple Silicon",
        "aarch64" => "ARM64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{os} · {arch}")
}

fn folder_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim_matches([' ', '.', '-']);
    if name.is_empty() {
        return "Game".into();
    }
    // Also keep names portable when someone moves the export onto another filesystem.
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        format!("Game - {name}")
    } else {
        name.into()
    }
}

pub fn destination(parent: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        parent.is_dir(),
        "Choose an existing folder for your export."
    );
    let name = folder_name(name);
    for number in 1..=10_000 {
        let path = parent.join(if number == 1 {
            name.clone()
        } else {
            format!("{name} ({number})")
        });
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(path),
            Ok(_) => continue,
            Err(e) => return Err(e).context("Checking the export location"),
        }
    }
    anyhow::bail!(
        "This location has too many exports with this name. Choose another location or game name."
    )
}

fn validate_name(name: &str) -> Result<()> {
    bozzard_project::Project {
        version: 1,
        name: name.into(),
        start_scene: "scene.json".into(),
        view: Layer::ThreeD,
        runtime_modules: Vec::new(),
        cook: Default::default(),
    }
    .validate()
}

impl App {
    pub fn show_export_dialog(&mut self) {
        let mut dialog = files::Dialog::new(files::Kind::Export, &self.editor.path);
        dialog.project_name = self.editor.scene().name.clone();
        if let Some(parent) = &self.export_parent
            && parent.is_dir()
        {
            dialog.directory = parent.clone();
        }
        self.dialog = Some(dialog);
    }

    pub fn export_dialog(&mut self, ctx: &egui::Context, dialog: &mut files::Dialog) -> bool {
        if matches!(dialog.kind, files::Kind::Exported) {
            return self.exported_dialog(ctx, dialog);
        }
        let mut keep = true;
        egui::Window::new("Export game")
            .id(egui::Id::new("export-game"))
            .collapsible(false)
            .resizable(false)
            .default_width(500.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Create a game you can open and play on its own.");
                ui.add_space(12.0);
                ui.label("Game name");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut dialog.project_name)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    dialog.export_error = None;
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("Export to");
                    if ui.button("Choose folder…").clicked() {
                        if let Some(parent) = rfd::FileDialog::new()
                            .set_title("Choose where to export your game")
                            .set_directory(&dialog.directory)
                            .set_can_create_directories(true)
                            .pick_folder()
                        {
                            dialog.directory = parent;
                            dialog.export_error = None;
                        }
                        // Time spent in the native modal dialog must not advance Play on return.
                        self.last_frame = Instant::now();
                        ctx.request_repaint();
                    }
                });
                ui.label(dialog.directory.display().to_string());
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("Build for");
                    ui.strong(platform_label());
                });
                ui.weak("A standalone game for this platform.");
                egui::ComboBox::from_label("Asset cooking")
                    .selected_text(dialog.cook_target.label()).show_ui(ui, |ui| {
                        for target in bozzard_project::CookTarget::ALL {
                            ui.selectable_value(&mut dialog.cook_target, target, target.label());
                        }
                    });
                ui.weak("Compressed targets keep lossless fallback pixels. Unchanged assets reuse the cook cache.");
                ui.add_space(10.0);
                let output = validate_name(&dialog.project_name)
                    .and_then(|()| destination(&dialog.directory, &dialog.project_name));
                match &output {
                    Ok(path) => {
                        ui.label(format!(
                            "Game folder: {}",
                            path.file_name().unwrap().to_string_lossy()
                        ));
                        ui.weak("Created automatically. Previous exports are kept.");
                    }
                    Err(error) => {
                        ui.colored_label(Color32::LIGHT_RED, error.to_string());
                    }
                }
                ui.weak("Includes current scene changes, even if you haven't saved them.");
                if let Ok(Some(id)) = bozzard_demo::multiplayer::app_id(self.editor.scene()) {
                    ui.label(format!("Steam multiplayer · App ID {id}"));
                    ui.weak(if id == 480 {
                        "Includes Steam API files and Spacewar development settings. Open the exported game with Steam running."
                    } else {
                        "Includes Steam API files. Launch the exported build through Steam; use editor Play for local testing."
                    });
                }
                let runtime = std::env::current_exe()
                    .map_err(anyhow::Error::from)
                    .and_then(|path| bozzard_project::companion_player(&path));
                if runtime.is_err() {
                    ui.add_space(8.0);
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        "The player needed for export is missing.",
                    );
                    ui.label("Use the full Bozzard editor bundle, which includes the player.");
                }
                if let Some(error) = &dialog.export_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            output.is_ok() && runtime.is_ok() && self.loading.is_none(),
                            egui::Button::new(
                                egui::RichText::new("Export game")
                                    .color(Color32::from_rgb(16, 32, 26)),
                            )
                            .fill(theme::GREEN),
                        )
                        .clicked()
                        && let Ok(path) = output
                    {
                        if self.start_export(path, dialog.project_name.clone(), dialog.cook_target) {
                            self.export_parent = Some(dialog.directory.clone());
                            keep = false;
                        } else {
                            dialog.export_error = Some(self.status.clone());
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        keep
    }

    fn exported_dialog(&mut self, ctx: &egui::Context, dialog: &mut files::Dialog) -> bool {
        let mut keep = true;
        let folder = PathBuf::from(&dialog.path);
        egui::Window::new("Game exported")
            .id(egui::Id::new("game-exported"))
            .collapsible(false)
            .resizable(false)
            .default_width(500.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.heading("Your game is ready to play");
                ui.label(platform_label());
                ui.add_space(10.0);
                ui.label(folder.display().to_string());
                ui.weak("Keep this folder's contents together when sharing or moving it.");
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.button("Play game").clicked() {
                        dialog.export_error = play_game(&folder)
                            .err()
                            .map(|e| format!("Couldn't start the game: {e:#}"));
                    }
                    if ui.button("Open folder").clicked() {
                        dialog.export_error = open_folder(&folder)
                            .err()
                            .map(|e| format!("Couldn't open the folder: {e:#}"));
                    }
                    if ui.button("Done").clicked() {
                        keep = false;
                    }
                });
                if let Some(error) = &dialog.export_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
            });
        keep
    }
}

fn open_folder(folder: &Path) -> Result<()> {
    ensure!(
        folder.is_dir(),
        "The exported folder has been moved or removed."
    );
    open::that(folder).context("Opening the exported folder")
}

fn play_game(folder: &Path) -> Result<()> {
    let steam = folder.join("steam-runtime.json");
    if steam.is_file() {
        let runtime: serde_json::Value = serde_json::from_slice(&std::fs::read(steam)?)?;
        ensure!(
            runtime["mode"] != "steam-store",
            "Launch this build through its Steam library entry (App ID {}). Use editor Play for local testing.",
            runtime["app_id"]
        );
    }
    #[cfg(target_os = "macos")]
    {
        let app = folder.join("Game.app");
        ensure!(
            app.join("Contents/MacOS/Game").is_file(),
            "The exported game has been moved or removed."
        );
        // Each export shares the engine's bundle identifier. Force a fresh instance
        // so macOS does not focus a different, older exported build that is running.
        let output = std::process::Command::new("/usr/bin/open")
            .arg("-n")
            .arg(app)
            .output()
            .context("Opening Game.app")?;
        ensure!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let binary = folder.join(if cfg!(windows) { "Game.exe" } else { "Game" });
        ensure!(
            binary.is_file(),
            "The exported game has been moved or removed."
        );
        let mut child = std::process::Command::new(binary)
            .current_dir(folder)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("Starting the exported game")?;
        std::thread::Builder::new()
            .name("exported-game".into())
            .spawn(move || {
                let _ = child.wait();
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_names_stay_inside_the_chosen_folder() {
        for (name, expected) in [
            ("First Trail", "First Trail"),
            ("../My:Game?", "My-Game"),
            ("...", "Game"),
            ("CON", "Game - CON"),
            ("LPT9.txt", "Game - LPT9.txt"),
            ("  Café  ", "Café"),
        ] {
            assert_eq!(folder_name(name), expected);
        }
    }
    #[test]
    fn repeated_exports_choose_new_folders_without_touching_existing_files() {
        let parent =
            std::env::temp_dir().join(format!("bozzard-export-location-{}", std::process::id()));
        std::fs::create_dir(&parent).unwrap();
        let first = destination(&parent, "First Trail").unwrap();
        std::fs::create_dir(&first).unwrap();
        let second = destination(&parent, "First Trail").unwrap();
        assert_eq!(second, parent.join("First Trail (2)"));
        std::fs::write(&second, "keep me").unwrap();
        assert_eq!(
            destination(&parent, "First Trail").unwrap(),
            parent.join("First Trail (3)")
        );
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "keep me");
        assert!(destination(&parent.join("missing"), "First Trail").is_err());
        std::fs::remove_dir_all(parent).unwrap();
    }
}
