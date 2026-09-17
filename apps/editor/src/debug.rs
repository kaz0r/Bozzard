//! The Debug workspace: bounded captures and a source-aware console.
use super::*;
use bozzard_diagnostics::{Console, CpuSpan, Diagnostics, Level, Location};
use std::collections::VecDeque;

const HISTORY: usize = 240;
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Page {
    #[default]
    Profiler,
    Console,
}
#[derive(Default, Serialize)]
struct Memory {
    buffers: u64,
    textures: u64,
    buffer_bytes: u64,
    texture_bytes: u64,
}
#[derive(Serialize)]
struct Frame {
    id: u64,
    scene: String,
    interval_ms: f64,
    editor_cpu_ms: f64,
    spans: Vec<CpuSpan>,
    omitted_spans: u64,
    render: Option<bozzard_render::FrameStats>,
    gpu: Option<bozzard_render::GpuFrameTiming>,
    compute_gpu: Option<bozzard_render::GpuFrameTiming>,
    compute_frame: Option<u64>,
    compute: Option<bozzard_scene::compute::Statistics>,
    compute_executor: bozzard_render::compute::ExecutorStats,
    entities: usize,
    assets: usize,
}
pub(super) struct DebugWorkspace {
    page: Page,
    pub recording: bool,
    frames: VecDeque<Frame>,
    selected_frame: Option<u64>,
    next_frame: u64,
    pub console: Console,
    last_status: String,
    filter: String,
    source: String,
    levels: [bool; 3],
    rows: Vec<usize>,
    filter_revision: Option<u64>,
    selected_event: Option<u64>,
    follow: bool,
    target_ms: f32,
    memory: Memory,
    memory_read: Instant,
    gpu_supported: bool,
    gpu_skipped: u64,
}
impl Default for DebugWorkspace {
    fn default() -> Self {
        Self {
            page: Page::Profiler,
            recording: false,
            frames: VecDeque::new(),
            selected_frame: None,
            next_frame: 0,
            console: Console::default(),
            last_status: String::new(),
            filter: String::new(),
            source: String::new(),
            levels: [true; 3],
            rows: Vec::new(),
            filter_revision: None,
            selected_event: None,
            follow: true,
            target_ms: 1000. / 60.,
            memory: Memory::default(),
            memory_read: Instant::now(),
            gpu_supported: false,
            gpu_skipped: 0,
        }
    }
}
fn level_index(level: Level) -> usize {
    match level {
        Level::Info => 0,
        Level::Warning => 1,
        Level::Error => 2,
    }
}
fn level_color(level: Level) -> Color32 {
    match level {
        Level::Info => Color32::LIGHT_GRAY,
        Level::Warning => Color32::YELLOW,
        Level::Error => Color32::LIGHT_RED,
    }
}
impl DebugWorkspace {
    pub(super) fn remember_status(&mut self, status: &str) {
        self.last_status.clear();
        self.last_status.push_str(status);
    }
    pub(super) fn show_console(&mut self) {
        self.page = Page::Console;
    }
    fn filter_rows(&mut self) {
        if self.filter_revision == Some(self.console.revision) {
            return;
        }
        self.rows.clear();
        let needle = self.filter.to_lowercase();
        self.rows.extend(
            self.console
                .events
                .iter()
                .enumerate()
                .filter(|(_, event)| {
                    self.levels[level_index(event.level)]
                        && (self.source.is_empty() || event.source == self.source)
                        && event.matches(&needle)
                })
                .map(|(i, _)| i),
        );
        self.filter_revision = Some(self.console.revision);
    }
    fn cpu_chart(&mut self, ui: &mut egui::Ui) {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 76.), Sense::click());
        let ceiling = self
            .frames
            .iter()
            .map(|f| f.editor_cpu_ms as f32)
            .fold(self.target_ms * 1.5, f32::max)
            .max(1.);
        let width = rect.width() / HISTORY as f32;
        let offset = HISTORY.saturating_sub(self.frames.len());
        for (i, frame) in self.frames.iter().enumerate() {
            let selected = self.selected_frame == Some(frame.id);
            let x = rect.left() + (offset + i) as f32 * width;
            let height = (frame.editor_cpu_ms as f32 / ceiling * rect.height()).max(1.);
            ui.painter().rect_filled(
                Rect::from_min_max(
                    Pos2::new(x, rect.bottom() - height),
                    Pos2::new(x + width.max(1.), rect.bottom()),
                ),
                0.,
                if selected {
                    Color32::WHITE
                } else if frame.editor_cpu_ms > f64::from(self.target_ms) {
                    Color32::LIGHT_RED
                } else {
                    theme::GREEN
                },
            );
        }
        let y = rect.bottom() - self.target_ms / ceiling * rect.height();
        ui.painter().line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            egui::Stroke::new(1., Color32::YELLOW),
        );
        if let Some(point) = response.hover_pos() {
            let index = ((point.x - rect.left()) / width) as usize;
            if let Some(frame) = index.checked_sub(offset).and_then(|i| self.frames.get(i)) {
                response.clone().on_hover_text(format!(
                    "Frame {} · {:.2} ms editor CPU · click to freeze and inspect",
                    frame.id, frame.editor_cpu_ms
                ));
                if response.clicked() {
                    self.selected_frame = Some(frame.id);
                    self.recording = false;
                }
            }
        }
    }
    fn profiler_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if self.recording {
                    "Pause capture"
                } else {
                    "Record"
                })
                .on_hover_text(
                    "Capture timings without pausing the game. Click a bar to inspect a frame.",
                )
                .clicked()
            {
                self.recording = !self.recording;
                if self.recording {
                    self.selected_frame = None;
                }
            }
            if ui.button("Clear capture").clicked() {
                self.frames.clear();
                self.selected_frame = None;
            }
            egui::ComboBox::from_id_salt("debug-frame-budget")
                .selected_text(format!("{:.0} FPS budget", 1000. / self.target_ms))
                .show_ui(ui, |ui| {
                    for fps in [30., 60., 120.] {
                        ui.selectable_value(
                            &mut self.target_ms,
                            1000. / fps,
                            format!("{fps:.0} FPS ({:.2} ms)", 1000. / fps),
                        );
                    }
                });
            ui.weak(format!("{} / {HISTORY} frames", self.frames.len()));
        });
        if self.frames.is_empty() {
            ui.label(
                "Press Record, then Play or move around the scene. Click a frame to inspect it.",
            );
            ui.weak("CPU work, GPU passes and frame intervals are different measurements. GPU results arrive a few frames later.");
            return;
        }
        self.cpu_chart(ui);
        let selected = self
            .selected_frame
            .and_then(|id| self.frames.iter().find(|f| f.id == id))
            .or_else(|| self.frames.back())
            .unwrap();
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("Frame {}", selected.id));
            ui.label(format!("Editor CPU {:.2} ms", selected.editor_cpu_ms)).on_hover_text("Time spent in the editor update, including simulation, viewport preparation and these panels. Excludes window presentation and egui's GPU rendering.");
            ui.label(format!("Interval {:.2} ms", selected.interval_ms)).on_hover_text("Wall time between editor updates, including waiting, scheduling and presentation. This is not CPU work.");
            ui.label(format!("{} entities · {} assets", selected.entities, selected.assets));
        });
        egui::ScrollArea::vertical().id_salt("profiler-details").show(ui, |ui| {
            egui::CollapsingHeader::new("CPU · simulation stages").default_open(true).show(ui, |ui| {
                if selected.spans.is_empty() { ui.weak("No simulation tick in this frame. Press Play to profile gameplay."); }
                let mut depths = [0usize; bozzard_diagnostics::MAX_SPANS];
                egui::Grid::new("cpu-spans").striped(true).show(ui, |ui| {
                    for (index, span) in selected.spans.iter().enumerate() {
                        let depth = span.parent.filter(|p| *p < index).map_or(0, |p| depths[p] + 1);
                        depths[index] = depth;
                        ui.horizontal(|ui| { ui.add_space(depth as f32 * 12.); ui.label(span.name); });
                        ui.monospace(format!("{:.3} ms", span.duration_ms));
                        ui.weak(span.tick.map_or(String::new(), |tick| format!("tick {tick}")));
                        ui.end_row();
                    }
                });
                ui.weak("Parent times include their children; do not add both. Each fixed tick is shown separately.");
                if selected.omitted_spans > 0 { ui.colored_label(Color32::YELLOW, format!("{} additional CPU spans omitted (capture limit).", selected.omitted_spans)); }
            });
            egui::CollapsingHeader::new("CPU · viewport rendering").default_open(true).show(ui, |ui| {
                if let Some(render) = selected.render {
                    ui.label(format!("Prepare {:.3} ms · Encode {:.3} ms · Submit {:.3} ms", render.prepare_ms, render.encode_ms, render.submit_ms));
                    ui.label(format!("{} visible mesh draws · {} culled surfaces · {} shadow draws · {} particles", render.visible_surfaces, render.culled_surfaces, render.shadow_draws, render.particles));
                    ui.label(format!("{} color triangles · {} shadow triangles · {} particle dispatches", render.color_triangles, render.shadow_triangles, render.particle_compute_dispatches));
                    ui.weak("Mesh/shadow counters exclude full-screen effects, text overlays and particle draw batches.");
                } else { ui.weak("Viewport reused or not drawn. No new renderer work is attributed to this frame."); }
            });
            egui::CollapsingHeader::new("GPU · measured passes").default_open(true).show(ui, |ui| {
                if !self.gpu_supported { ui.weak("Timestamp queries are unavailable on this graphics device. CPU measurements still work."); }
                else if let Some(gpu) = &selected.gpu {
                    if gpu.failed { ui.colored_label(Color32::YELLOW, "GPU readback failed for this frame."); }
                    let total: f64 = gpu.passes.iter().filter_map(|p| p.milliseconds).sum();
                    let measured = gpu.passes.iter().filter(|p| p.milliseconds.is_some()).count();
                    if measured > 0 { ui.label(format!("Sum of {measured} measured passes: {total:.3} ms")); }
                    if measured < gpu.passes.len() {
                        ui.colored_label(Color32::YELLOW, "Some GPU timestamps are unavailable. The graphics driver returned invalid samples; these are excluded from the sum.");
                    }
                    egui::Grid::new("gpu-spans").striped(true).show(ui, |ui| {
                        for pass in &gpu.passes { ui.label(&pass.name); ui.monospace(pass.milliseconds.map_or_else(|| "Unavailable".into(), |ms| format!("{ms:.3} ms"))); ui.end_row(); }
                    });
                    if gpu.omitted > 0 { ui.colored_label(Color32::YELLOW, format!("{} passes omitted (capture limit).", gpu.omitted)); }
                    ui.weak("Scene render and compute passes only. Excludes uploads, egui and presentation; GPU passes can overlap.");
                } else if selected.render.is_some() { ui.weak("GPU sample pending or skipped because all readback slots were busy."); }
                else { ui.weak("No new viewport draw in this frame."); }
                if self.gpu_skipped > 0 { ui.weak(format!("{} GPU samples skipped to keep rendering responsive.", self.gpu_skipped)); }
            });
            egui::CollapsingHeader::new("Memory · graphics resources").show(ui, |ui| {
                if self.memory.buffers + self.memory.textures == 0 { ui.weak("Graphics allocation counters are unavailable on this backend."); }
                else {
                    ui.label(format!("Buffers: {:.2} MiB in {} resources", self.memory.buffer_bytes as f64 / 1048576., self.memory.buffers));
                    ui.label(format!("Textures: {:.2} MiB in {} resources", self.memory.texture_bytes as f64 / 1048576., self.memory.textures));
                    ui.weak("Live backend counters, sampled once per second. Includes editor graphics; not total process RAM or total VRAM use.");
                }
            });
            egui::CollapsingHeader::new("Compute · scripts and Rust").default_open(true).show(ui, |ui| {
                if let Some(compute) = selected.compute {
                    ui.label(format!("{} queued commands · {} jobs · {} resources · {:.2} MiB", compute.queued_commands, compute.jobs, compute.resources, compute.resource_bytes as f64 / 1048576.));
                    ui.weak(format!("Cumulative uploads {:.2} MiB · Readbacks {:.2} MiB", compute.uploaded_bytes as f64 / 1048576., compute.readback_bytes as f64 / 1048576.));
                }
                if selected.compute_frame.is_some() {
                    ui.label(format!("Encode {:.3} ms · Submit {:.3} ms", selected.compute_executor.encode_ms, selected.compute_executor.submit_ms));
                    if let Some(gpu) = &selected.compute_gpu {
                        for pass in &gpu.passes { ui.label(format!("{} · {}", pass.name, pass.milliseconds.map_or_else(|| "Timing unavailable".into(), |ms| format!("{ms:.3} ms")))); }
                        if gpu.omitted > 0 { ui.weak(format!("{} additional dispatch timings omitted", gpu.omitted)); }
                    } else { ui.weak("GPU timing pending, unsupported, or the timestamp pool was busy."); }
                } else { ui.weak("No new authored compute submission in this frame."); }
            });
        });
    }
    fn console_ui(&mut self, ui: &mut egui::Ui) -> Option<Location> {
        let mut jump = None;
        ui.horizontal_wrapped(|ui| {
            for (index, name) in ["Info", "Warnings", "Errors"].into_iter().enumerate() {
                if ui.checkbox(&mut self.levels[index], name).changed() {
                    self.filter_revision = None;
                }
            }
            egui::ComboBox::from_id_salt("console-source")
                .selected_text(if self.source.is_empty() {
                    "All sources"
                } else {
                    &self.source
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(&mut self.source, String::new(), "All sources")
                        .changed()
                    {
                        self.filter_revision = None;
                    }
                    let sources: std::collections::BTreeSet<_> = self
                        .console
                        .events
                        .iter()
                        .map(|e| e.source.as_str())
                        .collect();
                    for source in sources {
                        if ui
                            .selectable_value(&mut self.source, source.to_owned(), source)
                            .changed()
                        {
                            self.filter_revision = None;
                        }
                    }
                });
            if ui
                .add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("Search messages, objects or assets…")
                        .desired_width(230.),
                )
                .changed()
            {
                self.filter_revision = None;
            }
            ui.checkbox(&mut self.follow, "Follow newest");
            if ui.button("Clear console").clicked() {
                self.console.clear();
                self.selected_event = None;
            }
        });
        self.filter_rows();
        ui.weak(format!(
            "{} matching messages · {} older messages discarded · repeated messages show a count",
            self.rows.len(),
            self.console.discarded
        ));
        if self.console.events.is_empty() {
            ui.label("No messages yet. Blueprint Print, script print(), and editor/runtime errors appear here.");
            return None;
        }
        let details_height = if self.selected_event.is_some() {
            98.
        } else {
            0.
        };
        egui::ScrollArea::vertical()
            .id_salt("debug-console-rows")
            .max_height((ui.available_height() - details_height).max(48.))
            .stick_to_bottom(self.follow)
            .show_rows(ui, 22., self.rows.len(), |ui, range| {
                for index in range {
                    let event = &self.console.events[self.rows[index]];
                    ui.horizontal(|ui| {
                        ui.weak(format!("{:7.2}s", event.seconds));
                        ui.colored_label(level_color(event.level), format!("{:?}", event.level));
                        ui.weak(&event.source);
                        if event.repetitions > 1 {
                            ui.strong(format!("×{}", event.repetitions));
                        }
                        let label = event.message.lines().next().unwrap_or("");
                        if ui
                            .add(
                                egui::Button::selectable(
                                    self.selected_event == Some(event.id),
                                    egui::RichText::new(label).color(level_color(event.level)),
                                )
                                .truncate(),
                            )
                            .clicked()
                        {
                            self.selected_event = Some(event.id);
                            self.follow = false;
                        }
                    });
                }
            });
        if let Some(event) = self
            .selected_event
            .and_then(|id| self.console.events.iter().find(|e| e.id == id))
        {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if let Some(object) = &event.location.object
                    && ui.button(format!("Go to {object}")).clicked()
                {
                    jump = Some(event.location.clone());
                }
                if let Some(node) = event.location.node {
                    ui.label(format!("Node {node}"));
                }
                if let Some(tick) = event.tick {
                    ui.label(format!("Tick {tick}"));
                }
                if let Some(scene) = &event.location.scene {
                    let name = Path::new(scene)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    ui.label(format!("Scene: {name}")).on_hover_text(scene);
                }
                if let Some(asset) = &event.location.asset {
                    ui.label(format!("Asset: {asset}"));
                }
                if ui.button("Copy message").clicked() {
                    ui.ctx().copy_text(format!(
                        "{:?} · {}\n{}\n{:?}",
                        event.level, event.source, event.message, event.location
                    ));
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("console-message-details")
                .max_height(68.)
                .show(ui, |ui| {
                    ui.add(egui::Label::new(&event.message).wrap().selectable(true));
                });
        }
        jump
    }
}
impl App {
    pub(super) fn debug_begin_frame(&mut self) -> Option<(Instant, u64, u64)> {
        if let Some(play) = &mut self.editor.play
            && let Some(d) = play.app.world.resource_mut::<Diagnostics>()
        {
            d.profiler.recording = self.debug.recording;
            d.profiler.begin_frame();
        }
        self.renderer.set_profiling_enabled(self.debug.recording);
        self.compute.executor.set_profiling(self.debug.recording);
        self.debug.gpu_supported = self
            .gpu
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY);
        self.debug.recording.then(|| {
            (
                Instant::now(),
                self.viewport_draws,
                self.compute.executor.statistics().submissions,
            )
        })
    }
    pub(super) fn debug_compute_profiles(&mut self, profiles: Vec<bozzard_render::GpuFrameTiming>) {
        for profile in profiles {
            if let Some(frame) = self
                .debug
                .frames
                .iter_mut()
                .rev()
                .find(|frame| frame.compute_frame == Some(profile.frame))
            {
                frame.compute_gpu = Some(profile);
            }
        }
    }
    pub(super) fn debug_end_frame(
        &mut self,
        started: Option<(Instant, u64, u64)>,
        interval_ms: f64,
    ) {
        if let Some(play) = &mut self.editor.play
            && let Some(d) = play.app.world.resource_mut::<Diagnostics>()
            && !d.console.events.is_empty()
        {
            let scene = self.editor.path.to_string_lossy();
            for event in &mut d.console.events {
                event
                    .location
                    .scene
                    .get_or_insert_with(|| scene.to_string());
            }
            d.console.drain_into(&mut self.debug.console);
        }
        if self.debug.last_status != self.status {
            self.debug.console.push(
                if self.error {
                    Level::Error
                } else {
                    Level::Info
                },
                "Editor",
                &self.status,
                Location::default(),
                None,
            );
            self.debug.last_status.clone_from(&self.status);
        }
        if let Some((start, draws, submissions)) = started
            && self.debug.recording
        {
            let mut spare = if self.debug.frames.len() == HISTORY {
                self.debug.frames.pop_front().unwrap().spans
            } else {
                Vec::new()
            };
            let mut omitted = 0;
            if let Some(play) = &mut self.editor.play
                && let Some(d) = play.app.world.resource_mut::<Diagnostics>()
            {
                spare.clear();
                std::mem::swap(&mut spare, &mut d.profiler.spans);
                omitted = d.profiler.dropped;
            } else {
                spare.clear();
            }
            self.debug.next_frame += 1;
            self.debug.frames.push_back(Frame {
                id: self.debug.next_frame,
                scene: self.editor.path.to_string_lossy().into_owned(),
                interval_ms,
                editor_cpu_ms: start.elapsed().as_secs_f64() * 1000.,
                spans: spare,
                omitted_spans: omitted,
                render: (self.viewport_draws != draws).then(|| self.renderer.frame_stats()),
                gpu: None,
                compute_gpu: None,
                compute_frame: (self.compute.executor.statistics().submissions != submissions)
                    .then_some(self.compute.executor.statistics().profile_frame),
                compute: self.editor.play.as_ref().and_then(|play| {
                    play.instance()
                        .compute_if_initialized()
                        .map(|state| state.runtime.statistics())
                }),
                compute_executor: self.compute.executor.statistics(),
                entities: self
                    .editor
                    .play
                    .as_ref()
                    .map_or(self.editor.scene().objects.len(), |p| p.app.world.len()),
                assets: self.editor.assets.entries().count(),
            });
        }
        match self.renderer.poll_gpu_profiles(&self.gpu) {
            Ok(completed) => {
                for gpu in completed {
                    if let Some(frame) = self
                        .debug
                        .frames
                        .iter_mut()
                        .find(|f| f.render.is_some_and(|r| r.frame_id == gpu.frame))
                    {
                        frame.gpu = Some(gpu);
                    }
                }
            }
            Err(error) => self.debug.console.push(
                Level::Warning,
                "GPU profiler",
                &format!("{error:#}"),
                Location::default(),
                None,
            ),
        }
        self.debug.gpu_skipped = self.renderer.skipped_gpu_profiles();
        if self.workspace.debug_visible
            && self.debug.memory_read.elapsed() >= Duration::from_secs(1)
        {
            let counters = self.gpu.device.get_internal_counters().hal;
            self.debug.memory = Memory {
                buffers: counters.buffers.read().max(0) as u64,
                textures: counters.textures.read().max(0) as u64,
                buffer_bytes: counters.buffer_memory.read().max(0) as u64,
                texture_bytes: counters.texture_memory.read().max(0) as u64,
            };
            self.debug.memory_read = Instant::now();
        }
    }
    pub(super) fn debug_panel(&mut self, ui: &mut egui::Ui) {
        if !self.workspace.debug_visible {
            return;
        }
        let mut jump = None;
        let mut export = false;
        egui::Panel::bottom("debug-workspace").default_size(340.).min_size(180.).max_size(650.).resizable(true).show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                ui.strong("Debug");
                ui.selectable_value(&mut self.debug.page, Page::Profiler, "Profiler");
                let errors = self.debug.console.events.iter().filter(|e| e.level == Level::Error).count();
                ui.selectable_value(&mut self.debug.page, Page::Console, format!("Console{suffix}", suffix = if errors > 0 { format!(" ({errors} errors)") } else { String::new() }));
                export = ui.button("Export JSON…").on_hover_text("Save the captured frames and console messages, including source references.").clicked();
                if ui.button("Hide").clicked() { self.workspace.debug_visible = false; }
            });
            ui.separator();
            match self.debug.page { Page::Profiler => self.debug.profiler_ui(ui), Page::Console => jump = self.debug.console_ui(ui) }
        });
        if let Some(location) = jump {
            self.debug_jump(&location);
        }
        if export
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Debug capture", &["json"])
                .set_file_name("bozzard-debug-capture.json")
                .save_file()
        {
            let result = {
                #[derive(Serialize)]
                struct Capture<'a> {
                    version: u32,
                    scene: &'a std::path::Path,
                    frames: &'a VecDeque<Frame>,
                    console: &'a VecDeque<bozzard_diagnostics::Event>,
                    discarded_messages: u64,
                    gpu_samples_skipped: u64,
                    live_gpu_memory: &'a Memory,
                }
                use std::io::Write;
                bozzard_demo::save_atomic(&path, |file| {
                    let mut writer = std::io::BufWriter::new(file);
                    serde_json::to_writer_pretty(
                        &mut writer,
                        &Capture {
                            version: 1,
                            scene: &self.editor.path,
                            frames: &self.debug.frames,
                            console: &self.debug.console.events,
                            discarded_messages: self.debug.console.discarded,
                            gpu_samples_skipped: self.debug.gpu_skipped,
                            live_gpu_memory: &self.debug.memory,
                        },
                    )?;
                    writer.flush()?;
                    Ok(())
                })
            };
            if result.is_ok() {
                self.status = format!("Debug capture saved · {}", path.display());
            }
            self.result(result);
        }
    }
    fn debug_jump(&mut self, location: &Location) {
        if location
            .scene
            .as_ref()
            .is_some_and(|scene| Path::new(scene) != self.editor.path)
        {
            self.status = format!(
                "Open '{}' to inspect this message's source.",
                location.scene.as_deref().unwrap()
            );
            return;
        }
        let Some(owner) = &location.object else {
            return;
        };
        let scene = self
            .editor
            .play
            .as_ref()
            .map_or_else(|| self.editor.scene(), |p| p.instance().document());
        if !scene.objects.iter().any(|o| &o.id == owner) {
            self.status = format!(
                "'{owner}' is a runtime-only or removed object. Its recorded source remains in the console."
            );
            return;
        }
        if self.editor.play.is_none() {
            self.editor.select_object(Some(owner.clone()));
        }
        self.hierarchy_search.clear();
        if location.node.is_some() {
            self.focus_diagnostic_node(owner, location.attachment.unwrap_or(0), location.node);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_update_for_repeated_messages_and_clear_does_not_reuse_selection() {
        let mut debug = DebugWorkspace::default();
        debug.console.push(
            Level::Error,
            "Blueprint",
            "Bad door target",
            Location::default(),
            Some(7),
        );
        debug
            .console
            .push(Level::Info, "Script", "Hello", Location::default(), None);
        debug.filter = "DOOR".into();
        debug.filter_rows();
        assert_eq!(debug.rows, vec![0]);
        debug.levels[2] = false;
        debug.filter_revision = None;
        debug.filter_rows();
        assert!(debug.rows.is_empty());
        let id = debug.console.events[0].id;
        debug.console.clear();
        debug.console.push(
            Level::Error,
            "Editor",
            "New failure",
            Location::default(),
            None,
        );
        assert!(debug.console.events[0].id > id);
    }
}
