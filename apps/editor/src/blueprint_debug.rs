//! Editor preferences and inspection UI for the headless Blueprint debugger.
use super::*;
use bozzard_scene::{
    Blueprint, BlueprintDebugger, Breakpoint, DebugCommand, DebugPause, NodeSnapshot,
    WatchSnapshot, blueprint::NodeKind,
};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    enabled: bool,
    breakpoints: Vec<SavedBreakpoint>,
}
#[derive(Serialize, Deserialize)]
struct SavedBreakpoint {
    path: PathBuf,
    location: Breakpoint,
    graph: String,
    kind: NodeKind,
}
impl SavedBreakpoint {
    fn matches(&self, scene: &bozzard_scene::Scene) -> bool {
        scene.name == self.location.scene
            && scene
                .objects
                .iter()
                .find(|o| o.id == self.location.object)
                .and_then(|o| o.blueprints.get(self.location.attachment))
                .is_some_and(|a| {
                    a.graph.name == self.graph
                        && a.graph
                            .nodes
                            .iter()
                            .any(|n| n.id == self.location.node && n.kind == self.kind)
                })
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Values,
    Trace,
    Breakpoints,
}
pub(super) struct Workspace {
    pub runtime_owner: Option<String>,
    pub visible: bool,
    revision: u64,
    page: Page,
    tracing: bool,
    watch: Option<WatchSnapshot>,
    watch_key: Option<(String, String, usize, Option<u32>, u64)>,
    watch_updated: Instant,
    recorded: Option<NodeSnapshot>,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            runtime_owner: None,
            visible: true,
            revision: 0,
            page: Page::Values,
            tracing: true,
            watch: None,
            watch_key: None,
            watch_updated: Instant::now(),
            recorded: None,
        }
    }
}
impl App {
    /// Install before the first fixed tick, so On Start breakpoints cannot be missed.
    pub(super) fn prepare_blueprint_debugger(&mut self) {
        let Some(play) = &mut self.editor.play else {
            self.blueprint_debug.runtime_owner = None;
            self.blueprint_debug.revision = 0;
            self.blueprint_debug.watch_key = None;
            self.blueprint_debug.watch = None;
            self.blueprint_debug.recorded = None;
            return;
        };
        if play.app.world.resource::<BlueprintDebugger>().is_none() {
            let saved: Vec<_> = self
                .workspace
                .blueprint_debug
                .breakpoints
                .iter()
                .filter(|b| b.path == self.editor.path)
                .collect();
            let mut debugger = BlueprintDebugger::new(saved.iter().map(|b| b.location.clone()));
            debugger.breakpoint_guards = saved
                .iter()
                .map(|b| (b.location.clone(), (b.graph.clone(), b.kind)))
                .collect();
            debugger.enabled = self.workspace.blueprint_debug.enabled;
            debugger.tracing = self.blueprint_debug.tracing;
            play.app.world.insert_resource(debugger);
            self.blueprint_debug.revision = 0;
        }
    }
    pub(super) fn sync_blueprint_pause(&mut self) {
        let next = self
            .editor
            .play
            .as_ref()
            .and_then(|p| p.app.world.resource::<BlueprintDebugger>())
            .filter(|d| d.revision != self.blueprint_debug.revision)
            .map(|d| (d.revision, d.current.clone()));
        if let Some((revision, current)) = next {
            self.blueprint_debug.revision = revision;
            self.blueprint_debug.watch_key = None;
            self.blueprint_debug.recorded = None;
            self.gameplay_controls.reset();
            self.mouse_captured = false;
            self.fly_latched = false;
            if let Some(current) = current {
                self.blueprint_debug.visible = true;
                self.blueprint_debug.page = Page::Values;
                self.jump_blueprint(&current);
            }
        }
    }
    fn jump_blueprint(&mut self, snapshot: &NodeSnapshot) {
        let location = &snapshot.location;
        if self
            .editor
            .play
            .as_ref()
            .is_some_and(|p| p.instance().document().name != location.scene)
        {
            self.status = format!(
                "Recorded in scene '{}'. Values remain available in the trace.",
                location.scene
            );
            return;
        }
        self.focus_diagnostic_node(&location.object, location.attachment, Some(location.node));
    }
    fn blueprint_command(&mut self, command: DebugCommand) {
        self.workspace.blueprint_debug.enabled = true;
        if let Some(play) = &mut self.editor.play {
            let result = play.debug_command(command);
            self.result(result);
        }
        self.sync_blueprint_pause();
    }
    pub(super) fn blueprint_run_controls(&mut self, ui: &mut egui::Ui) {
        let Some(play) = &self.editor.play else {
            return;
        };
        if play.multiplayer_active() {
            ui.small("Steam session · Stop to disconnect");
            return;
        }
        let paused = play.app.is_paused();
        let failed = play
            .app
            .world
            .resource::<BlueprintDebugger>()
            .is_some_and(|d| d.error.is_some());
        if ui
            .add_enabled(
                !failed,
                egui::Button::new(if paused { "Continue" } else { "Pause" }),
            )
            .on_hover_text("Pause or resume the simulation. No wall-clock catch-up after a pause.")
            .clicked()
        {
            self.blueprint_command(if paused {
                DebugCommand::Continue
            } else {
                DebugCommand::Pause
            });
        }
        if ui.add_enabled(paused && !failed, egui::Button::new("Step node"))
            .on_hover_text("Execute the highlighted event/action, then stop before the next. Pure data nodes are evaluated on demand. F10.").clicked() {
            self.blueprint_command(DebugCommand::StepNode);
        }
        if ui
            .add_enabled(paused && !failed, egui::Button::new("Step tick"))
            .on_hover_text(
                "Finish this fixed tick, ignoring breakpoints until its boundary. Shift+F10.",
            )
            .clicked()
        {
            self.blueprint_command(DebugCommand::StepTick);
        }
    }
    pub(super) fn blueprint_debug_shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some()
            || self.loading.is_some()
            || self.confirm_discard
            || ctx.egui_wants_keyboard_input()
        {
            return;
        }
        if self.editor.play.as_ref().is_some_and(|p| p.app.is_paused()) {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::F10)) {
                self.blueprint_command(DebugCommand::StepTick);
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F10)) {
                self.blueprint_command(DebugCommand::StepNode);
            }
        }
    }
    pub(super) fn blueprint_object_picker(&mut self, ui: &mut egui::Ui) {
        let scene = self
            .editor
            .play
            .as_ref()
            .map_or_else(|| self.editor.scene(), |p| p.instance().document());
        let mut owners: Vec<_> = scene
            .objects
            .iter()
            .filter(|o| !o.blueprints.is_empty())
            .collect();
        owners.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        let playing = self.editor.play.is_some();
        let current = if playing {
            self.blueprint_debug
                .runtime_owner
                .as_ref()
                .or(self.editor.selected.as_ref())
        } else {
            self.editor.selected.as_ref()
        };
        let label = owners
            .iter()
            .find(|o| Some(&o.id) == current)
            .map_or("Choose an object…", |o| o.name.as_str());
        let mut select = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(if playing {
                "Runtime object"
            } else {
                "Blueprint object"
            });
            egui::ComboBox::from_id_salt("blueprint-debug-owner")
                .selected_text(label)
                .width(220.)
                .show_ui(ui, |ui| {
                    for object in owners {
                        if ui
                            .selectable_label(
                                Some(&object.id) == current,
                                format!("{} · {}", object.name, object.id),
                            )
                            .clicked()
                        {
                            select = Some(object.id.clone());
                        }
                    }
                });
            if playing {
                ui.weak("Includes spawned instances · Stop to edit");
            }
        });
        if let Some(owner) = select {
            if playing {
                self.blueprint_debug.runtime_owner = Some(owner);
            } else {
                self.editor.select_object(Some(owner));
            }
            self.blueprint_debug.recorded = None;
            self.blueprint_debug.watch_key = None;
        }
    }
    pub(super) fn blueprint_debug_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        owner: &str,
        graph: &Blueprint,
    ) {
        let playing = self.editor.play.is_some();
        let paused = self.editor.play.as_ref().is_some_and(|p| p.app.is_paused());
        let scene = self
            .editor
            .play
            .as_ref()
            .map_or_else(|| self.editor.scene(), |p| p.instance().document())
            .name
            .clone();
        let selected = self
            .blueprint_pane
            .selected
            .and_then(|id| graph.nodes.iter().find(|n| n.id == id));
        let location = selected.map(|n| Breakpoint {
            scene,
            object: owner.into(),
            attachment: self.blueprint_pane.index,
            node: n.id,
        });
        let has = location.as_ref().is_some_and(|l| {
            self.workspace
                .blueprint_debug
                .breakpoints
                .iter()
                .any(|b| b.path == self.editor.path && &b.location == l)
        });
        let breakable = selected.is_some_and(|n| n.kind.event() || n.kind.action());
        let shortcut = !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F9));
        let mut toggle = false;
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!paused, egui::Checkbox::new(&mut self.workspace.blueprint_debug.enabled, "Debug Blueprints"))
                .on_hover_text("Enable breakpoints and execution tracing. Continue or Stop before disabling.").changed()
                && let Some(play) = &mut self.editor.play && let Some(d) = play.app.world.resource_mut::<BlueprintDebugger>() {
                d.enabled = self.workspace.blueprint_debug.enabled;
            }
            toggle = ui.add_enabled(breakable, egui::Button::new(if has { "● Remove breakpoint" } else { "○ Add breakpoint" }))
                .on_hover_text("Select an event or action node, then toggle its breakpoint. F9. Pure data nodes are inspected through Values.").clicked() || (shortcut && breakable);
            ui.checkbox(&mut self.blueprint_debug.visible, "Inspector");
            if !playing { ui.weak("Set a breakpoint, then Play. Red dot = breakpoint."); }
        });
        if toggle
            && let Some(location) = location
            && let Some(node) = selected
        {
            self.workspace
                .blueprint_debug
                .breakpoints
                .retain(|b| b.path != self.editor.path || b.location != location);
            if !has {
                self.workspace.blueprint_debug.enabled = true;
                self.workspace
                    .blueprint_debug
                    .breakpoints
                    .push(SavedBreakpoint {
                        path: self.editor.path.clone(),
                        location: location.clone(),
                        graph: graph.name.clone(),
                        kind: node.kind,
                    });
            }
            if let Some(play) = &mut self.editor.play
                && let Some(d) = play.app.world.resource_mut::<BlueprintDebugger>()
            {
                if has {
                    d.breakpoints.remove(&location);
                    d.breakpoint_guards.remove(&location);
                } else {
                    d.enabled = true;
                    d.breakpoint_guards
                        .insert(location.clone(), (graph.name.clone(), node.kind));
                    d.breakpoints.insert(location);
                }
            }
        }
        self.blueprint_pane.breakpoints = self
            .workspace
            .blueprint_debug
            .breakpoints
            .iter()
            .filter(|b| {
                b.location.scene
                    == self
                        .editor
                        .play
                        .as_ref()
                        .map_or_else(|| self.editor.scene(), |p| p.instance().document())
                        .name
                    && b.path == self.editor.path
                    && b.location.object == owner
                    && b.location.attachment == self.blueprint_pane.index
                    && b.graph == graph.name
                    && graph
                        .nodes
                        .iter()
                        .any(|n| n.id == b.location.node && n.kind == b.kind)
            })
            .map(|b| b.location.node)
            .collect();
        self.blueprint_pane.executing = None;
        self.blueprint_pane.recent.clear();
        if let Some(play) = &self.editor.play
            && let Some(d) = play.app.world.resource::<BlueprintDebugger>()
        {
            let same = |s: &NodeSnapshot| {
                s.location.scene == play.instance().document().name
                    && s.location.object == owner
                    && s.location.attachment == self.blueprint_pane.index
            };
            self.blueprint_pane.executing = d
                .current
                .as_ref()
                .filter(|s| same(s) && paused)
                .map(|s| s.location.node);
            self.blueprint_pane.recent.extend(
                d.trace
                    .iter()
                    .rev()
                    .take(24)
                    .filter(|s| same(s))
                    .map(|s| s.location.node),
            );
            let reason = match d.paused {
                Some(DebugPause::Breakpoint) => "Paused at breakpoint · highlighted node runs next",
                Some(DebugPause::NodeStep) if d.current.is_none() => {
                    "Paused at tick boundary after node step"
                }
                Some(DebugPause::NodeStep) => "Paused after node step · highlighted node runs next",
                Some(DebugPause::TickStep) => "Paused at tick boundary",
                Some(DebugPause::Requested) => "Paused by you",
                Some(DebugPause::Error) => {
                    "Execution failed · inspect values, then Stop and fix the graph"
                }
                None => {
                    if d.enabled {
                        "Running · green outline = recently executed"
                    } else {
                        "Debugger disabled"
                    }
                }
            };
            ui.colored_label(
                if paused {
                    Color32::LIGHT_YELLOW
                } else {
                    theme::GREEN
                },
                reason,
            );
            if let Some(e) = &d.error {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
        }
    }
    pub(super) fn blueprint_debug_inspector(&mut self, ui: &mut egui::Ui, owner: &str) {
        if !self.blueprint_debug.visible {
            return;
        }
        let selected = self.blueprint_pane.selected;
        let index = self.blueprint_pane.index;
        if let Some(play) = &self.editor.play {
            let key = (
                play.instance().document().name.clone(),
                owner.to_owned(),
                index,
                selected,
                self.blueprint_debug.revision,
            );
            if self.blueprint_debug.watch_key.as_ref() != Some(&key)
                || (!play.app.is_paused()
                    && self.blueprint_debug.watch_updated.elapsed() >= Duration::from_millis(200))
            {
                self.blueprint_debug.watch =
                    play.instance()
                        .inspect_blueprint(&play.app.world, owner, index, selected);
                self.blueprint_debug.watch_key = Some(key);
                self.blueprint_debug.watch_updated = Instant::now();
            }
        }
        let mut jump = None;
        let mut remove = None;
        let mut export = false;
        egui::Panel::right("blueprint-debug-inspector").default_size(300.).min_size(230.).max_size(550.).resizable(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Blueprint Inspector");
                if ui.small_button("×").on_hover_text("Hide inspector").clicked() { self.blueprint_debug.visible = false; }
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.blueprint_debug.page, Page::Values, "Values");
                ui.selectable_value(&mut self.blueprint_debug.page, Page::Trace, "Trace");
                ui.selectable_value(&mut self.blueprint_debug.page, Page::Breakpoints, "Breakpoints");
            });
            ui.separator();
            match self.blueprint_debug.page {
                Page::Values => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(recorded) = &self.blueprint_debug.recorded {
                            ui.colored_label(Color32::LIGHT_YELLOW, "Recorded values · before execution");
                            if ui.button("Return to live values").clicked() { self.blueprint_debug.recorded = None; }
                            else { snapshot_ui(ui, recorded); return; }
                        }
                        if self.editor.play.is_none() { ui.label("Play to inspect live values. Select any node to see its pins."); return; }
                        ui.weak("Values refresh while running. Pause to inspect a stable state.");
                        if let Some(watch) = &self.blueprint_debug.watch {
                            if let Some(node) = &watch.node { snapshot_ui(ui, node); } else { ui.label("Select a node to inspect pins."); }
                            ui.separator();
                            ui.strong("Variables");
                            if watch.variables.is_empty() { ui.weak("No variables declared."); }
                            for scope in ["Graph", "Object", "Scene"] {
                                let values: Vec<_> = watch.variables.iter().filter(|v| v.scope == scope).collect();
                                if !values.is_empty() {
                                    egui::CollapsingHeader::new(scope).default_open(true).show(ui, |ui| {
                                        for v in values { ui.label(&v.name); ui.monospace(&v.value); }
                                    });
                                }
                            }
                            if !watch.timers.is_empty() { ui.separator(); ui.strong("Pending timers"); for timer in &watch.timers { ui.label(timer); } }
                        } else { ui.label("This graph has not run yet. Step a node or tick to initialize it."); }
                    });
                }
                Page::Trace => {
                    if let Some(play) = &mut self.editor.play && let Some(d) = play.app.world.resource_mut::<BlueprintDebugger>() {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.blueprint_debug.tracing, "Record execution");
                            d.tracing = self.blueprint_debug.tracing;
                            if ui.button("Clear").clicked() { d.clear_trace(); self.blueprint_debug.recorded = None; }
                        });
                        ui.label(format!("{} retained · {} older entries discarded", d.trace.len(), d.discarded));
                        ui.weak("Latest first · snapshots are taken before execution. Select an entry to inspect its recorded pins.");
                        export = ui.button("Export trace JSON…").clicked();
                        egui::ScrollArea::vertical().show_rows(ui, 42., d.trace.len(), |ui, rows| {
                            for i in rows {
                                let snapshot = &d.trace[d.trace.len() - 1 - i];
                                let label = format!("{} · {} #{}\n{} · graph {}", snapshot.tick.map_or("UI".into(), |t| format!("Tick {t}")), snapshot.node_name, snapshot.location.node, snapshot.location.object, snapshot.location.attachment + 1);
                                if ui.selectable_label(false, label).clicked() { jump = Some(snapshot.clone()); }
                            }
                        });
                    } else { ui.label("Play with Debug Blueprints enabled to record execution."); }
                }
                Page::Breakpoints => {
                    ui.weak("Saved with your workspace, separate from scene/game data. F9 toggles the selected event or action.");
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let scene = self.editor.play.as_ref().map_or_else(|| self.editor.scene(), |p| p.instance().document());
                        let mut count = 0;
                        for (i, saved) in self.workspace.blueprint_debug.breakpoints.iter().enumerate().filter(|(_, b)| b.path == self.editor.path) {
                            count += 1;
                            let valid = saved.matches(scene) || scene.runtime_scenes.values().any(|s| saved.matches(s));
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(if valid { Color32::LIGHT_RED } else { Color32::GRAY }, "●");
                                ui.label(format!("{} · {} #{}", saved.location.object, saved.kind.title(), saved.location.node));
                                if ui.small_button("Remove").clicked() { remove = Some(i); }
                            });
                            ui.weak(format!("{} · {}", saved.location.scene, saved.graph));
                            if !valid { ui.colored_label(Color32::LIGHT_YELLOW, "Unavailable: object, graph, or node changed. Remove and set it again, or wait for the instance to spawn."); }
                            ui.separator();
                        }
                        if count == 0 { ui.label("No breakpoints. Select an event or action node and click Add breakpoint."); }
                    });
                }
            }
        });
        if let Some(snapshot) = jump {
            self.jump_blueprint(&snapshot);
            self.blueprint_debug.recorded = Some(snapshot);
            self.blueprint_debug.page = Page::Values;
        }
        if let Some(i) = remove {
            let saved = self.workspace.blueprint_debug.breakpoints.remove(i);
            if let Some(play) = &mut self.editor.play
                && let Some(d) = play.app.world.resource_mut::<BlueprintDebugger>()
            {
                d.breakpoints.remove(&saved.location);
                d.breakpoint_guards.remove(&saved.location);
            }
        }
        if export
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Blueprint trace", &["json"])
                .set_file_name("bozzard-blueprint-trace.json")
                .save_file()
        {
            let result = (|| -> Result<()> {
                let play = self.editor.play.as_ref().context("Play has stopped")?;
                let debugger = play
                    .app
                    .world
                    .resource::<BlueprintDebugger>()
                    .context("Debugger not enabled")?;
                #[derive(Serialize)]
                struct Capture<'a> {
                    version: u32,
                    scene: &'a Path,
                    discarded: u64,
                    trace: &'a std::collections::VecDeque<NodeSnapshot>,
                    paused: bool,
                    paused_node: Option<&'a NodeSnapshot>,
                    error: Option<&'a str>,
                }
                use std::io::Write;
                bozzard_demo::save_atomic(&path, |file| {
                    let mut writer = std::io::BufWriter::new(file);
                    serde_json::to_writer_pretty(
                        &mut writer,
                        &Capture {
                            version: 1,
                            scene: &self.editor.path,
                            discarded: debugger.discarded,
                            trace: &debugger.trace,
                            paused: play.app.is_paused(),
                            paused_node: debugger.current.as_ref().filter(|_| play.app.is_paused()),
                            error: debugger.error.as_deref(),
                        },
                    )?;
                    writer.flush()?;
                    Ok(())
                })
            })();
            if result.is_ok() {
                self.status = format!("Blueprint trace saved · {}", path.display());
            }
            self.result(result);
        }
    }
}
fn snapshot_ui(ui: &mut egui::Ui, snapshot: &NodeSnapshot) {
    if snapshot.atomic {
        ui.colored_label(
            Color32::LIGHT_YELLOW,
            "Atomic teardown · cannot pause inside scene/host cleanup",
        );
    }
    ui.strong(format!(
        "#{} {}",
        snapshot.location.node, snapshot.node_name
    ));
    ui.label(format!("{} · {}", snapshot.location.object, snapshot.graph));
    ui.weak(format!(
        "{} · {}",
        snapshot.event,
        snapshot
            .tick
            .map_or("UI dispatch".into(), |t| format!("tick {t}"))
    ));
    egui::CollapsingHeader::new("Event context").show(ui, |ui| {
        ui.label(&snapshot.context);
    });
    for pin in &snapshot.pins {
        if pin.kind == bozzard_scene::blueprint::PinType::Exec {
            continue;
        }
        ui.separator();
        ui.label(format!("{} · {} ({:?})", pin.direction, pin.name, pin.kind));
        ui.monospace(&pin.value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_breakpoints_are_scene_and_graph_specific_and_round_trip_outside_gameplay() {
        let mut scene = bozzard_scene::Scene::from_json(
            r#"{"version":1,"name":"debug","views":{},"objects":[{"id":"owner","name":"Owner","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}]}"#,
        )
        .unwrap();
        let graph = Blueprint::spinning();
        let node = graph.nodes.iter().find(|n| n.kind.action()).unwrap();
        let saved = SavedBreakpoint {
            path: PathBuf::from("demo.json"),
            location: Breakpoint {
                scene: scene.name.clone(),
                object: "owner".into(),
                attachment: 0,
                node: node.id,
            },
            graph: graph.name.clone(),
            kind: node.kind,
        };
        scene.objects[0]
            .blueprints
            .push(bozzard_scene::BlueprintAttachment {
                enabled: true,
                graph,
            });
        assert!(saved.matches(&scene));
        let json = serde_json::to_string(&Preferences {
            enabled: true,
            breakpoints: vec![saved],
        })
        .unwrap();
        let prefs: Preferences = serde_json::from_str(&json).unwrap();
        assert!(prefs.enabled);
        assert!(prefs.breakpoints[0].matches(&scene));
        scene.name = "other".into();
        assert!(!prefs.breakpoints[0].matches(&scene));
        scene.name = "debug".into();
        scene.objects[0].blueprints[0].graph.name = "replacement".into();
        assert!(!prefs.breakpoints[0].matches(&scene));
        assert!(
            serde_json::from_str::<Preferences>("{}")
                .unwrap()
                .breakpoints
                .is_empty()
        );
    }
}
