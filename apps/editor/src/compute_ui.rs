//! Runtime inspection uses the normal bounded command API. No live storage mapping or GPU waits.
use super::*;
use bozzard_scene::compute::{Handle, JobState, Owner, ResourceKind, Shape, Ticket};

pub(super) struct Pane {
    selected: Option<Handle>,
    preview: Option<(Handle, egui::TextureId)>,
    pending: Option<(Owner, Ticket)>,
    values: String,
    first: u32,
    count: u32,
    completed: bool,
}
impl Default for Pane {
    fn default() -> Self {
        Self {
            selected: None,
            preview: None,
            pending: None,
            values: String::new(),
            first: 0,
            count: 64,
            completed: false,
        }
    }
}
impl App {
    pub(super) fn compute_ui(&mut self, ctx: &egui::Context) {
        let pane = &mut self.compute_pane;
        if pane.preview.is_some_and(|(handle, _)| {
            !self.workspace.compute_visible || self.compute.executor.texture_view(handle).is_none()
        }) {
            let (_, id) = pane.preview.take().unwrap();
            self.render_state.renderer.write().free_texture(&id);
        }
        let mut state = self
            .editor
            .play
            .as_ref()
            .and_then(|play| play.instance().compute_if_initialized());
        if let Some((owner, ticket)) = &pane.pending {
            if let Some(state) = &mut state {
                let result = match state.runtime.job(owner, *ticket).map(|job| &job.state) {
                    Ok(JobState::Complete) => Some(
                        state
                            .runtime
                            .take_result(owner, *ticket, 4096)
                            .and_then(|values| Ok(serde_json::to_string_pretty(&values)?)),
                    ),
                    Ok(JobState::Failed(error)) => Some(Err(anyhow::anyhow!(error.clone()))),
                    Ok(JobState::Cancelled) | Err(_) => Some(Err(anyhow::anyhow!(
                        "Inspection cancelled or scene replaced"
                    ))),
                    _ => None,
                };
                if let Some(result) = result {
                    pane.values = result.unwrap_or_else(|error| format!("{error:#}"));
                    let end = pane
                        .values
                        .floor_char_boundary(pane.values.len().min(16_384));
                    if end < pane.values.len() {
                        pane.values.truncate(end);
                        pane.values.push_str("\n…preview limited to 16 KiB");
                    }
                    let _ = state.runtime.cancel(owner, *ticket);
                    let _ = state.runtime.forget(owner, *ticket);
                    pane.pending = None;
                } else if !self.workspace.compute_visible {
                    let _ = state.runtime.cancel(owner, *ticket);
                    let _ = state.runtime.forget(owner, *ticket);
                    pane.pending = None;
                }
            } else {
                pane.pending = None;
                pane.values.clear();
            }
        }
        if !self.workspace.compute_visible {
            return;
        }
        let mut visible = true;
        egui::Window::new("Compute resources and jobs").open(&mut visible).default_size([620., 620.]).vscroll(true).show(ctx, |ui| {
            let gpu = self.compute.executor.statistics();
            ui.label(format!("{} pipelines · {} binding sets · {} GPU resources", gpu.pipelines, gpu.bind_groups, gpu.allocations));
            ui.weak(format!("Uploads {:.2} MiB · Readback staging {:.2} MiB", gpu.staging_bytes as f64 / 1048576., gpu.readback_bytes as f64 / 1048576.));
            for (asset, error) in self.compute.diagnostics() {
                ui.colored_label(Color32::LIGHT_RED, format!("{asset}: {error}"));
            }
            let Some(state) = &mut state else { ui.weak("Press Play to inspect resources created by scripts or Rust systems."); return; };
            let stats = state.runtime.statistics();
            ui.label(format!("{} queued commands · {} in-flight submissions · {} readback slots occupied", stats.queued_commands, stats.in_flight_submissions, stats.pending_readbacks));
            ui.label(format!("Resources {:.2} MiB · Uploaded {:.2} MiB · Read back {:.2} MiB", stats.resource_bytes as f64 / 1048576., stats.uploaded_bytes as f64 / 1048576., stats.readback_bytes as f64 / 1048576.));
            let resources: Vec<_> = state.runtime.resources().filter(|(_, retiring, _)| !retiring).map(|(r, _, error)| (r.clone(), error.map(str::to_owned))).collect();
            let previous = pane.selected;
            if !resources.iter().any(|(r, _)| Some(r.handle) == pane.selected) {
                pane.selected = resources.first().map(|(r, _)| r.handle);
                pane.values.clear();
            }
            egui::ComboBox::from_id_salt("compute-resource").width(320.).selected_text(resources.iter().find(|(r, _)| Some(r.handle) == pane.selected).map_or("No resources".into(), |(r, _)| format!("{} · {} #{}", r.name, r.owner.object, r.owner.attachment))).show_ui(ui, |ui| {
                for (r, _) in &resources {
                    if ui.selectable_value(&mut pane.selected, Some(r.handle), format!("{} · {} #{} · {:?}", r.name, r.owner.object, r.owner.attachment, r.scope)).changed() { pane.first = 0; pane.values.clear(); }
                }
            });
            if pane.selected != previous {
                if let Some((owner, ticket)) = pane.pending.take() {
                    let _ = state.runtime.cancel(&owner, ticket);
                    let _ = state.runtime.forget(&owner, ticket);
                }
                if let Some((_, id)) = pane.preview.take() {
                    self.render_state.renderer.write().free_texture(&id);
                }
                pane.values.clear();
            }
            if let Some((resource, error)) = resources.iter().find(|(r, _)| Some(r.handle) == pane.selected) {
                if let Some(error) = error { ui.colored_label(Color32::LIGHT_RED, error); }
                match &resource.kind {
                    ResourceKind::Texture { width, height, format } => {
                        ui.label(format!("{width} × {height} · {} · linear color · straight alpha", format.name()));
                        if pane.preview.is_some_and(|(handle, _)| handle != resource.handle) {
                            let (_, id) = pane.preview.take().unwrap(); self.render_state.renderer.write().free_texture(&id);
                        }
                        if pane.preview.is_none() && let Some(view) = self.compute.executor.texture_view(resource.handle) {
                            let id = self.render_state.renderer.write().register_native_texture(&self.gpu.device, view, wgpu::FilterMode::Linear);
                            pane.preview = Some((resource.handle, id));
                        }
                        if let Some((_, id)) = pane.preview {
                            let w = ui.available_width().min(512.).min(256. * *width as f32 / *height as f32);
                            ui.image((id, Vec2::new(w, w * *height as f32 / *width as f32)));
                        } else { ui.weak("Texture creation is queued."); }
                    }
                    ResourceKind::Buffer { layout, elements, bytes } => {
                        ui.label(format!("{bytes} bytes · {} byte alignment", layout.alignment()));
                        let array = if let Shape::Array { count, .. } = layout.shape() { Some(count.unwrap_or(*elements)) } else { None };
                        if let Some(capacity) = array {
                            pane.first = pane.first.min(capacity - 1);
                            pane.count = pane.count.clamp(1, (capacity - pane.first).min(1024));
                            ui.horizontal(|ui| {
                                ui.label("First"); ui.add(egui::DragValue::new(&mut pane.first).range(0..=capacity - 1));
                                ui.label("Count"); ui.add(egui::DragValue::new(&mut pane.count).range(1..=(capacity - pane.first).min(1024)));
                            });
                        }
                        if ui.add_enabled(pane.pending.is_none() && error.is_none(), egui::Button::new("Request buffer values")).clicked() {
                            let result = if array.is_some() { state.runtime.readback_range(&resource.owner, resource.handle, pane.first, pane.count) } else { state.runtime.readback(&resource.owner, resource.handle) };
                            match result {
                                Ok(ticket) => { pane.pending = Some((resource.owner.clone(), ticket)); pane.values.clear(); }
                                Err(error) => pane.values = format!("{error:#}"),
                            }
                        }
                        if pane.pending.is_some() { ui.weak("Pending. Results arrive on a simulation tick; step or resume if paused."); }
                        if !pane.values.is_empty() { egui::ScrollArea::both().max_height(200.).show(ui, |ui| { ui.monospace(&pane.values); }); }
                    }
                    ResourceKind::Sampler { linear } => { ui.label(if *linear { "Linear filtering · clamp to edge" } else { "Nearest filtering · clamp to edge" }); }
                }
            }
            ui.separator();
            ui.horizontal(|ui| { ui.strong("Jobs"); ui.checkbox(&mut pane.completed, "Show completed"); });
            let jobs: Vec<_> = state.runtime.jobs().filter(|job| pane.completed || !job.state.terminal()).collect();
            egui::Grid::new("compute-jobs").striped(true).show(ui, |ui| {
                for job in jobs.into_iter().rev().take(64) {
                    ui.monospace(format!("#{}", job.ticket.serial()));
                    ui.label(&job.label).on_hover_text(format!("{} attachment {} · tick {}", job.owner.object, job.owner.attachment, job.tick));
                    let response = ui.label(job.state.name());
                    if let JobState::Failed(error) = &job.state { response.on_hover_text(error); }
                    ui.end_row();
                }
            });
            ui.weak("Latest 64 matching jobs. GPU timings are available while recording in Debug → Profiler.");
        });
        self.workspace.compute_visible = visible;
        if !visible {
            if let Some((owner, ticket)) = pane.pending.take()
                && let Some(state) = &mut state
            {
                let _ = state.runtime.cancel(&owner, ticket);
                let _ = state.runtime.forget(&owner, ticket);
            }
            if let Some((_, id)) = pane.preview.take() {
                self.render_state.renderer.write().free_texture(&id);
            }
        }
    }
}
