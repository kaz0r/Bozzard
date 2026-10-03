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
            .title_bar(false)
            .frame(theme::dialog_frame())
            .collapsible(false)
            .resizable(false)
            .fixed_size(Vec2::new(540.0, 0.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                // Header band with the target platform.
                let header = egui::Frame::new()
                    .fill(theme::glass(12))
                    .corner_radius(egui::CornerRadius { nw: theme::RADIUS, ne: theme::RADIUS, sw: 0, se: 0 })
                    .inner_margin(egui::Margin::symmetric(18, 14))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new("Export game").font(theme::bold(ctx, 19.0)));
                                ui.weak("Build a standalone game you can open and play on its own.");
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                                theme::chip(ui, &platform_label(), theme::ACCENT);
                            });
                        });
                    });
                ui.painter().hline(
                    header.response.rect.x_range(),
                    header.response.rect.bottom(),
                    egui::Stroke::new(1.0, theme::glass(24)),
                );
                egui::Frame::new().inner_margin(egui::Margin::same(18)).show(ui, |ui| {
                    theme::section(ui, "Game", |ui| {
                        ui.label("Name");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut dialog.project_name)
                                    .desired_width(f32::INFINITY)
                                    .margin(Vec2::new(8.0, 6.0)),
                            )
                            .changed()
                        {
                            dialog.export_error = None;
                        }
                    });
                    theme::section(ui, "Location", |ui| {
                        ui.horizontal(|ui| {
                            let browse = ui.button("Browse…");
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(dialog.directory.display().to_string()).monospace(),
                                )
                                .truncate(),
                            );
                            if browse.clicked() {
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
                    });
                    theme::section(ui, "Build", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Asset cooking");
                            egui::ComboBox::from_id_salt("export-cook")
                                .selected_text(dialog.cook_target.label())
                                .show_ui(ui, |ui| {
                                    for target in bozzard_project::CookTarget::ALL {
                                        ui.selectable_value(&mut dialog.cook_target, target, target.label());
                                    }
                                });
                        });
                        ui.weak("Compressed targets keep lossless fallback pixels. Unchanged assets reuse the cook cache.");
                    });
                    let output = validate_name(&dialog.project_name)
                        .and_then(|()| destination(&dialog.directory, &dialog.project_name));
                    theme::section(ui, "Result", |ui| {
                        match &output {
                            Ok(path) => {
                                ui.horizontal(|ui| {
                                    ui.colored_label(theme::GREEN, "✓");
                                    ui.label(format!(
                                        "Creates the folder {}",
                                        path.file_name().unwrap().to_string_lossy()
                                    ));
                                });
                                ui.weak("Previous exports are kept.");
                            }
                            Err(error) => {
                                ui.colored_label(theme::CORAL, error.to_string());
                            }
                        }
                        ui.weak("Includes current scene changes, even if you haven't saved them.");
                        if let Ok(Some(id)) = bozzard_demo::multiplayer::app_id(self.editor.scene()) {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                theme::chip(ui, &format!("Steam · App ID {id}"), theme::SKY);
                            });
                            ui.weak(if id == 480 {
                                "Includes Steam API files and Spacewar development settings. Open the exported game with Steam running."
                            } else {
                                "Includes Steam API files. Launch the exported build through Steam; use editor Play for local testing."
                            });
                        }
                    });
                    let runtime = std::env::current_exe()
                        .map_err(anyhow::Error::from)
                        .and_then(|path| bozzard_project::companion_player(&path));
                    if runtime.is_err() {
                        ui.colored_label(theme::CORAL, "The player needed for export is missing.");
                        ui.label("Use the full Bozzard editor bundle, which includes the player.");
                        ui.add_space(6.0);
                    }
                    if cfg!(debug_assertions) {
                        ui.weak("Development editor: export uses a compatible release player when available. A development player may run slower.");
                    }
                    if let Some(error) = &dialog.export_error {
                        ui.colored_label(theme::CORAL, error);
                        ui.add_space(6.0);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let go = ui.add_enabled(
                            output.is_ok() && runtime.is_ok() && self.loading.is_none(),
                            egui::Button::new(
                                egui::RichText::new("Export game")
                                    .font(theme::bold(ctx, 13.0))
                                    .color(Color32::from_rgb(16, 32, 26)),
                            )
                            .fill(theme::GREEN)
                            .min_size(Vec2::new(130.0, 30.0)),
                        );
                        if go.clicked()
                            && let Ok(path) = output
                        {
                            if self.start_export(path, dialog.project_name.clone(), dialog.cook_target) {
                                self.export_parent = Some(dialog.directory.clone());
                                keep = false;
                            } else {
                                dialog.export_error = Some(self.status.clone());
                            }
                        }
                        if ui.add(egui::Button::new("Cancel").min_size(Vec2::new(80.0, 30.0))).clicked() {
                            keep = false;
                        }
                    });
                });
            });
        keep
    }

    fn exported_dialog(&mut self, ctx: &egui::Context, dialog: &mut files::Dialog) -> bool {
        let mut keep = true;
        let folder = PathBuf::from(&dialog.path);
        egui::Window::new("Game exported")
            .id(egui::Id::new("game-exported"))
            .title_bar(false)
            .frame(theme::dialog_frame())
            .collapsible(false)
            .resizable(false)
            .fixed_size(Vec2::new(480.0, 0.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(18))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("✓")
                                    .font(theme::bold(ctx, 22.0))
                                    .color(theme::GREEN),
                            );
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new("Your game is ready to play")
                                        .font(theme::bold(ctx, 17.0)),
                                );
                                theme::chip(ui, &platform_label(), theme::ACCENT);
                            });
                        });
                        ui.add_space(12.0);
                        theme::section(ui, "Location", |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(folder.display().to_string()).monospace(),
                                )
                                .wrap(),
                            );
                            ui.weak(
                                "Keep this folder's contents together when sharing or moving it.",
                            );
                        });
                        if let Some(error) = &dialog.export_error {
                            ui.colored_label(theme::CORAL, error);
                            ui.add_space(6.0);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Play game")
                                            .font(theme::bold(ctx, 13.0))
                                            .color(Color32::from_rgb(16, 32, 26)),
                                    )
                                    .fill(theme::GREEN)
                                    .min_size(Vec2::new(110.0, 30.0)),
                                )
                                .clicked()
                            {
                                dialog.export_error = play_game(&folder)
                                    .err()
                                    .map(|e| format!("Couldn't start the game: {e:#}"));
                            }
                            if ui
                                .add(
                                    egui::Button::new("Open folder")
                                        .min_size(Vec2::new(100.0, 30.0)),
                                )
                                .clicked()
                            {
                                dialog.export_error = open_folder(&folder)
                                    .err()
                                    .map(|e| format!("Couldn't open the folder: {e:#}"));
                            }
                            if ui
                                .add(egui::Button::new("Done").min_size(Vec2::new(70.0, 30.0)))
                                .clicked()
                            {
                                keep = false;
                            }
                        });
                    });
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
