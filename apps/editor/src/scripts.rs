//! Script Manager: the coding alternative to a blueprint graph, binding script assets in order.
use super::*;

#[derive(Default)]
pub(crate) struct ScriptPane {
    asset: Option<String>,
    scene_path: PathBuf,
    path: PathBuf,
    source: String,
    saved: String,
    pending: Option<Option<String>>,
    external: Option<String>,
    last_check: Option<Instant>,
    requested: Option<String>,
    help_query: String,
    api_signatures: Vec<String>,
    jump_line: Option<usize>,
    cursor: usize,
    pending_cursor: Option<usize>,
}
impl ScriptPane {
    pub fn is_open(&self) -> bool {
        self.asset.is_some()
    }
    pub(super) fn dirty(&self) -> bool {
        self.source != self.saved
    }
}

impl App {
    fn open_script_source(&mut self, asset: String) {
        if self.script_pane.asset.as_deref() == Some(&asset)
            && self.script_pane.scene_path == self.editor.path
        {
            self.dock_focus = Some(docking::Pane::Script);
            return;
        }
        if self.script_pane.dirty() {
            self.script_pane.pending = Some(Some(asset));
            self.dock_focus = Some(docking::Pane::Script);
            return;
        }
        self.load_script_source(Some(asset));
    }

    fn load_script_source(&mut self, asset: Option<String>) {
        let Some(asset) = asset else {
            self.script_pane = ScriptPane::default();
            return;
        };
        let Some(source) = self
            .editor
            .scene()
            .assets
            .get(&asset)
            .filter(|s| s.kind == AssetKind::Script)
        else {
            self.result(Err(anyhow::anyhow!(
                "Script asset '{asset}' is missing from the active scene"
            )));
            return;
        };
        let path = bozzard_editor::root(&self.editor.path).join(&source.path);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.script_pane = ScriptPane {
                    asset: Some(asset),
                    scene_path: self.editor.path.clone(),
                    path,
                    source: text.clone(),
                    saved: text,
                    api_signatures: bozzard_scene::script_function_descriptions(),
                    ..Default::default()
                };
                self.dock_focus = Some(docking::Pane::Script);
            }
            Err(error) => self.result(Err(error.into())),
        }
    }

    fn save_script_source(&mut self, overwrite_external: bool) -> Result<()> {
        let pane = &mut self.script_pane;
        let disk = std::fs::read_to_string(&pane.path)?;
        ensure!(
            overwrite_external || disk == pane.saved,
            "Script changed outside the editor; review the external version before saving"
        );
        std::fs::write(&pane.path, &pane.source)?;
        pane.saved = pane.source.clone();
        pane.external = None;
        Ok(())
    }

    pub(super) fn script_source_pane(&mut self, ui: &mut egui::Ui) {
        let Some(asset) = self.script_pane.asset.clone() else {
            return;
        };
        if self
            .script_pane
            .last_check
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(1))
        {
            self.script_pane.last_check = Some(Instant::now());
            if let Ok(disk) = std::fs::read_to_string(&self.script_pane.path)
                && disk != self.script_pane.saved
            {
                self.script_pane.external = Some(disk);
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!(
                "{asset}{}",
                if self.script_pane.dirty() { " ●" } else { "" }
            ));
            ui.weak(self.script_pane.path.display().to_string());
            if ui.button("Close").clicked() {
                if self.script_pane.dirty() {
                    self.script_pane.pending = Some(None);
                } else {
                    self.load_script_source(None);
                }
            }
        });
        let current_scene = self.script_pane.scene_path == self.editor.path;
        if !current_scene {
            ui.colored_label(
                Color32::YELLOW,
                "This source belongs to another open scene. Save or discard its draft before applying a script to the current scene.",
            );
        }
        if let Some(disk) = self.script_pane.external.clone() {
            ui.colored_label(Color32::YELLOW, "This file changed outside the editor.");
            ui.horizontal(|ui| {
                if ui.button("Load external version").clicked() {
                    self.script_pane.source = disk.clone();
                    self.script_pane.saved = disk.clone();
                    self.script_pane.external = None;
                }
                if ui.button("Keep editor draft").clicked() {
                    self.script_pane.saved = disk;
                    self.script_pane.external = None;
                }
            });
        }
        if let Some(target) = self.script_pane.pending.clone() {
            ui.colored_label(
                Color32::YELLOW,
                "Unsaved script changes. Save, discard, or cancel before switching.",
            );
            ui.horizontal(|ui| {
                if ui.button("Save and continue").clicked() {
                    let result = self.save_script_source(false);
                    if result.is_ok() {
                        self.load_script_source(target.clone());
                    }
                    self.result(result);
                }
                if ui.button("Discard and continue").clicked() {
                    self.load_script_source(target.clone());
                }
                if ui.button("Cancel").clicked() {
                    self.script_pane.pending = None;
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.script_pane.dirty() && self.script_pane.external.is_none(),
                    egui::Button::new("Save file · Ctrl+S"),
                )
                .clicked()
            {
                let result = self.save_script_source(false);
                self.result(result);
            }
            if self.script_pane.external.is_some()
                && ui.button("Overwrite external file…").clicked()
            {
                let result = self.save_script_source(true);
                self.result(result);
            }
            if self.editor.play.is_some() {
                let network = self.editor.play.as_ref().is_some_and(|play| play.multiplayer_active());
                if ui.add_enabled(current_scene && !network, egui::Button::new("Apply to running Play"))
                    .on_hover_text(if network { "Active network sessions require all peers to stop and restart with the same script revision" } else { "Compile this draft and replace the running Play script without saving the source file" })
                    .clicked() {
                    let result = self
                        .editor
                        .request_script_reload(&asset, self.script_pane.source.clone());
                    if result.is_ok() {
                        self.script_pane.requested = Some(self.script_pane.source.clone());
                    }
                    self.result(result.map(|_| ()));
                }
            }
        });
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
            && self.script_pane.dirty()
        {
            let result = self.save_script_source(false);
            self.result(result);
        }
        if current_scene && self.editor.play.is_some() {
            let message = if self.script_pane.requested.as_deref() != Some(&self.script_pane.source)
            {
                "Draft has not been applied to Play".to_owned()
            } else {
                match self.editor.script_reload_feedback(&asset) {
                    Some(bozzard_editor::ScriptReloadFeedback::Compiling { revision }) => {
                        format!("Compiling revision {revision}…")
                    }
                    Some(bozzard_editor::ScriptReloadFeedback::Applied { revision }) => {
                        format!("Applied revision {revision} to Play")
                    }
                    Some(bozzard_editor::ScriptReloadFeedback::Stale { reason }) => {
                        format!("Reload skipped: {reason}")
                    }
                    Some(bozzard_editor::ScriptReloadFeedback::Failed { message }) => {
                        format!("Reload failed: {message}. Last valid script is still running.")
                    }
                    None => "Draft has not been applied to Play".into(),
                }
            };
            ui.label(message);
            if let Some(bozzard_editor::ScriptReloadFeedback::Failed { message }) =
                self.editor.script_reload_feedback(&asset)
                && let Some(line) = diagnostic_line(message)
                && ui
                    .link(format!("{}:{line}", self.script_pane.path.display()))
                    .on_hover_text("Open diagnostic location in this source pane")
                    .clicked()
            {
                self.script_pane.jump_line = Some(line);
            }
        } else {
            ui.weak("Save writes the source file. Start Play, then apply edits separately to the running scene.");
        }
        let lines = self.script_pane.source.lines().count().max(1);
        egui::ScrollArea::both()
            .id_salt("script-code")
            .max_height(ui.available_height().max(160.) * 0.65)
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.monospace(
                        (1..=lines)
                            .map(|n| format!("{n:>3}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                    let edit = egui::TextEdit::multiline(&mut self.script_pane.source)
                        .code_editor()
                        .desired_rows(lines.max(12))
                        .desired_width(f32::INFINITY)
                        .id_source(("script-source", &asset));
                    let mut output = edit.show(ui);
                    if let Some(range) = output.cursor_range {
                        self.script_pane.cursor = range.primary.index.0;
                    }
                    if let Some(line) = self.script_pane.jump_line.take() {
                        let cursor = self
                            .script_pane
                            .source
                            .lines()
                            .take(line.saturating_sub(1))
                            .map(|s| s.chars().count() + 1)
                            .sum();
                        self.script_pane.pending_cursor = Some(cursor);
                    }
                    if let Some(cursor) = self.script_pane.pending_cursor.take() {
                        use egui::text::{CCursor, CCursorRange};
                        output
                            .state
                            .cursor
                            .set_char_range(Some(CCursorRange::one(CCursor::new(cursor))));
                        output.state.store(ui.ctx(), output.response.id);
                        output.response.request_focus();
                        self.script_pane.cursor = cursor;
                    }
                });
            });
        ui.collapsing("Engine API and hooks", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.script_pane.help_query)
                    .hint_text("Search signatures…"),
            );
            let prefix = script_prefix(&self.script_pane.source, self.script_pane.cursor);
            let query = if self.script_pane.help_query.is_empty() {
                prefix.to_lowercase()
            } else {
                self.script_pane.help_query.to_lowercase()
            };
            egui::ScrollArea::vertical()
                .max_height(180.)
                .show(ui, |ui| {
                    for (name, parameters) in bozzard_scene::script_hook_signatures()
                        .iter()
                        .filter(|(name, _)| name.contains(&query))
                        .take(30)
                    {
                        let args = parameters.join(", ");
                        if ui
                            .selectable_label(false, format!("fn {name}({args})"))
                            .on_hover_text("Insert hook signature at the source cursor")
                            .clicked()
                        {
                            let skeleton = format!("fn {name}({args}) {{\n    \n}}");
                            let cursor = complete_script_name(
                                &mut self.script_pane.source,
                                self.script_pane.cursor,
                                &skeleton,
                            );
                            self.script_pane.pending_cursor = Some(cursor);
                        }
                    }
                    for signature in self
                        .script_pane
                        .api_signatures
                        .iter()
                        .filter(|s| s.to_lowercase().contains(&query))
                        .take(60)
                    {
                        if ui
                            .selectable_label(false, signature.as_str())
                            .on_hover_text("Complete at the source cursor")
                            .clicked()
                        {
                            let name = signature
                                .split('(')
                                .next()
                                .unwrap_or(signature)
                                .trim()
                                .trim_start_matches("fn ");
                            let cursor = complete_script_name(
                                &mut self.script_pane.source,
                                self.script_pane.cursor,
                                name,
                            );
                            self.script_pane.pending_cursor = Some(cursor);
                        }
                    }
                });
        });
    }
}

fn diagnostic_line(message: &str) -> Option<usize> {
    let (_, tail) = message.split_once("line ")?;
    tail.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

fn script_prefix(source: &str, cursor: usize) -> String {
    let chars: Vec<_> = source.chars().collect();
    let end = cursor.min(chars.len());
    let start = (0..end)
        .rev()
        .take_while(|&i| chars[i].is_ascii_alphanumeric() || chars[i] == '_')
        .last()
        .unwrap_or(end);
    chars[start..end].iter().collect()
}

fn complete_script_name(source: &mut String, cursor: usize, name: &str) -> usize {
    let mut chars: Vec<_> = source.chars().collect();
    let end = cursor.min(chars.len());
    let start = (0..end)
        .rev()
        .take_while(|&i| chars[i].is_ascii_alphanumeric() || chars[i] == '_')
        .last()
        .unwrap_or(end);
    chars.splice(start..end, name.chars());
    *source = chars.into_iter().collect();
    start + name.chars().count()
}

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
        let mut open_asset = None;
        egui::CollapsingHeader::new("SCRIPT MANAGER")
            .id_salt((&object.id, "script_manager"))
            .default_open(true)
            .show(ui, |ui| {
                let editing =
                    self.editor.play.is_none() && self.loading.is_none() && self.dialog.is_none();
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
                        if !attachment.script.is_empty() && ui.button("Open source").clicked() {
                            open_asset = Some(attachment.script.clone());
                        }
                        let stats = self
                            .editor
                            .play
                            .as_ref()
                            .and_then(|p| p.app.world.resource::<bozzard_scene::ScriptRuntime>())
                            .and_then(|runtime| {
                                runtime
                                    .stats
                                    .attachments
                                    .get(&(object.id.clone(), index))
                                    .copied()
                            });
                        if self.editor.play.is_some() {
                            let stats = stats.unwrap_or_default();
                            ui.weak(format!(
                                "{} hooks · {} commands",
                                stats.hooks, stats.commands
                            ));
                        }
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
                ui.weak(
                    "Top to bottom · open source for generated hook signatures and engine API help",
                );
            });
        if let Some(asset) = open_asset {
            self.open_script_source(asset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_replaces_the_identifier_at_the_character_cursor() {
        let mut source = "let label = \"å\"; set_pos".to_owned();
        let cursor = source.chars().count();
        let end = complete_script_name(&mut source, cursor, "set_position");
        assert_eq!(source, "let label = \"å\"; set_position");
        assert_eq!(end, source.chars().count());
        assert_eq!(script_prefix(&source, end), "set_position");
        assert_eq!(diagnostic_line("script 'x' line 12: bad hook"), Some(12));
    }
}
