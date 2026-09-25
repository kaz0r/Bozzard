//! Docked timeline over the authored Timeline component and an isolated Edit preview world.
use super::*;
use bozzard_scene::middleware::tween::{Property, Track};
use bozzard_scene::middleware::{registry, timeline::Timeline};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum ItemKind {
    Marker,
    Camera,
}
#[derive(Clone, Copy)]
struct TimelineDrag {
    kind: ItemKind,
    index: usize,
}

impl App {
    pub(super) fn timeline_pane(&mut self, ui: &mut egui::Ui) {
        theme::panel_title(ui, "Timeline");
        let Some(object) = self.editor.selected_object().cloned() else {
            ui.weak("Select an object with a Timeline component.");
            return;
        };
        let mut timeline = match registry::get::<Timeline>(&object) {
            Ok(Some(timeline)) => timeline,
            Ok(None) => {
                ui.weak("Add a Timeline component to this object to author tracks, markers and camera cuts.");
                return;
            }
            Err(error) => {
                self.result(Err(error));
                return;
            }
        };
        if self
            .timeline_scrub
            .as_ref()
            .is_some_and(|(id, _)| id != &object.id)
        {
            self.editor.clear_timeline_preview();
            self.timeline_scrub = None;
            self.viewport_stamp = None;
        }
        ui.horizontal_wrapped(|ui| {
            ui.strong(&object.name);
            ui.label(format!(
                "{:.2} s · {} tracks · {} markers · {} cuts",
                timeline.motion.duration,
                timeline.motion.tracks.len(),
                timeline.markers.len(),
                timeline.cameras.len()
            ));
            ui.add(
                egui::Slider::new(&mut self.workspace.timeline_zoom, 20.0..=300.0)
                    .text("Pixels / second"),
            );
            if ui.button("Clear preview").clicked() {
                self.editor.clear_timeline_preview();
                self.timeline_scrub = None;
                self.viewport_stamp = None;
            }
            ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                if ui.button("+ Track").clicked() && timeline.motion.tracks.len() < 128 {
                    Arc::make_mut(&mut timeline.motion.tracks)
                        .push(Track::new(Property::Translation));
                }
                if ui.button("+ Marker").clicked() && timeline.markers.len() < 1024 {
                    let time = self.timeline_scrub.as_ref().map_or(0., |(_, t)| *t);
                    let name = format!("Marker {}", timeline.markers.len() + 1);
                    Arc::make_mut(&mut timeline.markers)
                        .push(bozzard_scene::middleware::timeline::Marker { time, name });
                    Arc::make_mut(&mut timeline.markers).sort_by(|a, b| a.time.total_cmp(&b.time));
                }
                if let Some((layer, camera)) = self.editor.scene().views.iter().next()
                    && ui.button("+ Camera cut").clicked()
                    && timeline.cameras.len() < 1024
                {
                    let time = self.timeline_scrub.as_ref().map_or(0., |(_, t)| *t);
                    Arc::make_mut(&mut timeline.cameras).push(
                        bozzard_scene::middleware::timeline::CameraCut {
                            time,
                            camera: camera.clone(),
                            layer: *layer,
                        },
                    );
                    Arc::make_mut(&mut timeline.cameras).sort_by(|a, b| a.time.total_cmp(&b.time));
                }
            });
        });
        if self.editor.play.is_some() {
            ui.weak("Stop Play to scrub or edit the authoring timeline.");
        }
        let duration = timeline.motion.duration.max(0.001);
        let mut keyboard_time = self
            .timeline_scrub
            .as_ref()
            .filter(|(id, _)| id == &object.id)
            .map_or(0., |(_, time)| *time);
        let keyboard_seek = ui
            .add_enabled(
                self.editor.play.is_none(),
                egui::Slider::new(&mut keyboard_time, 0.0..=duration).text("Preview time"),
            )
            .changed();
        let width = (duration * self.workspace.timeline_zoom)
            .clamp(ui.available_width().max(200.), 50_000.);
        let pixels_per_second = width / duration;
        let height = 118. + timeline.motion.tracks.len() as f32 * 25.;
        let owner = object.id.clone();
        let mut requested_time = None;
        if keyboard_seek {
            requested_time = Some(keyboard_time);
        }
        let mut new_drag = None;
        let drag_id = ui.make_persistent_id(("timeline-drag", &owner));
        let mut drag = ui.data_mut(|d| d.get_temp::<TimelineDrag>(drag_id));
        egui::ScrollArea::horizontal()
            .id_salt(("timeline-scroll", &owner))
            .show(ui, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());
                let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
                painter.rect_filled(rect, 0., Color32::from_rgb(29, 33, 42));
                let x = |time: f32| rect.left() + time / duration * width;
                let clip = ui.clip_rect().intersect(rect);
                let first_second = ((clip.left() - rect.left()) / pixels_per_second)
                    .floor()
                    .max(0.) as usize;
                let last_second = ((clip.right() - rect.left()) / pixels_per_second)
                    .ceil()
                    .min(duration) as usize;
                let tick_step = if pixels_per_second >= 30. {
                    1
                } else if pixels_per_second >= 8. {
                    5
                } else {
                    10
                };
                for second in (first_second..=last_second).step_by(tick_step) {
                    let at = x(second as f32);
                    painter.line_segment(
                        [Pos2::new(at, rect.top()), Pos2::new(at, rect.bottom())],
                        egui::Stroke::new(1., Color32::from_gray(65)),
                    );
                    painter.text(
                        Pos2::new(at + 3., rect.top() + 3.),
                        egui::Align2::LEFT_TOP,
                        format!("{second}s"),
                        egui::FontId::monospace(10.),
                        Color32::LIGHT_GRAY,
                    );
                }
                for (row, label) in [(0., "Ruler"), (32., "Markers"), (61., "Camera cuts")] {
                    painter.text(
                        Pos2::new(clip.left() + 5., rect.top() + row),
                        egui::Align2::LEFT_TOP,
                        label,
                        egui::FontId::proportional(11.),
                        Color32::LIGHT_BLUE,
                    );
                }
                for (index, track) in timeline.motion.tracks.iter().enumerate() {
                    let y = rect.top() + 102. + index as f32 * 25.;
                    painter.line_segment(
                        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                        egui::Stroke::new(3., Color32::from_rgb(55, 112, 139)),
                    );
                    painter.text(
                        Pos2::new(clip.left() + 5., y - 12.),
                        egui::Align2::LEFT_TOP,
                        format!("{} · {}", index + 1, track.property.name()),
                        egui::FontId::proportional(11.),
                        Color32::WHITE,
                    );
                    for curve in &track.channels {
                        for key in &curve.keys {
                            let at = x(key.time);
                            painter.circle_filled(
                                Pos2::new(at, y),
                                3.,
                                Color32::from_rgb(153, 216, 240),
                            );
                        }
                    }
                }
                for marker in timeline.markers.iter() {
                    painter.circle_filled(
                        Pos2::new(x(marker.time), rect.top() + 49.),
                        6.,
                        Color32::YELLOW,
                    );
                }
                for cut in timeline.cameras.iter() {
                    painter.rect_filled(
                        Rect::from_center_size(
                            Pos2::new(x(cut.time), rect.top() + 78.),
                            Vec2::splat(10.),
                        ),
                        1.,
                        Color32::from_rgb(207, 158, 255),
                    );
                }
                if let Some((_, time)) = self.timeline_scrub.as_ref().filter(|(id, _)| id == &owner)
                {
                    let at = x(*time);
                    painter.line_segment(
                        [Pos2::new(at, rect.top()), Pos2::new(at, rect.bottom())],
                        egui::Stroke::new(2., Color32::LIGHT_GREEN),
                    );
                }
                if self.editor.play.is_none() {
                    if response.drag_started()
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        let mut hits = Vec::new();
                        for (index, item) in timeline.markers.iter().enumerate() {
                            hits.push((
                                (pointer - Pos2::new(x(item.time), rect.top() + 49.)).length(),
                                TimelineDrag {
                                    kind: ItemKind::Marker,
                                    index,
                                },
                            ));
                        }
                        for (index, item) in timeline.cameras.iter().enumerate() {
                            hits.push((
                                (pointer - Pos2::new(x(item.time), rect.top() + 78.)).length(),
                                TimelineDrag {
                                    kind: ItemKind::Camera,
                                    index,
                                },
                            ));
                        }
                        if let Some((distance, candidate)) =
                            hits.into_iter().min_by(|a, b| a.0.total_cmp(&b.0))
                            && distance < 12.
                        {
                            new_drag = Some(candidate);
                        }
                    }
                    if response.clicked()
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        requested_time = Some(
                            ((pointer.x - rect.left()) / width * duration).clamp(0., duration),
                        );
                    }
                    if response.dragged()
                        && drag.is_none()
                        && new_drag.is_none()
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        requested_time = Some(
                            ((pointer.x - rect.left()) / width * duration).clamp(0., duration),
                        );
                    }
                    if response.dragged()
                        && let Some(pointer) = response.interact_pointer_pos()
                        && let Some(active) = drag.or(new_drag)
                    {
                        let time =
                            ((pointer.x - rect.left()) / width * duration).clamp(0., duration);
                        match active.kind {
                            ItemKind::Marker => {
                                let markers = Arc::make_mut(&mut timeline.markers);
                                let min = if active.index > 0 {
                                    markers[active.index - 1].time
                                } else {
                                    0.
                                };
                                let max = if active.index + 1 < markers.len() {
                                    markers[active.index + 1].time
                                } else {
                                    duration
                                };
                                markers[active.index].time = time.clamp(min, max);
                            }
                            ItemKind::Camera => {
                                let cuts = Arc::make_mut(&mut timeline.cameras);
                                let min = if active.index > 0 {
                                    cuts[active.index - 1].time
                                } else {
                                    0.
                                };
                                let max = if active.index + 1 < cuts.len() {
                                    cuts[active.index + 1].time
                                } else {
                                    duration
                                };
                                cuts[active.index].time = time.clamp(min, max);
                            }
                        }
                    }
                    if response.drag_stopped() {
                        drag = None;
                        self.editor.finish_gesture();
                    }
                }
            });
        if let Some(active) = new_drag {
            self.editor.begin_gesture("Move timeline item");
            drag = Some(active);
        }
        if let Some(active) = drag {
            ui.data_mut(|d| d.insert_temp(drag_id, active));
        } else {
            ui.data_mut(|d| d.remove::<TimelineDrag>(drag_id));
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            && drag.is_some()
        {
            let result = self.editor.cancel_gesture();
            self.result(result);
            ui.data_mut(|d| d.remove::<TimelineDrag>(drag_id));
            return;
        }
        if let Some(time) = requested_time {
            match self.editor.scrub_timeline_preview(&owner, time) {
                Ok(()) => {
                    self.timeline_scrub = Some((owner.clone(), time));
                    self.viewport_stamp = None;
                }
                Err(error) => self.result(Err(error)),
            }
        }
        let original = registry::get::<Timeline>(&object).ok().flatten().unwrap();
        if timeline != original && self.editor.play.is_none() {
            let mut scene = self.editor.scene().clone();
            let target = scene.objects.iter_mut().find(|o| o.id == owner).unwrap();
            let result = registry::set(target, &timeline)
                .and_then(|_| self.editor.apply("Move timeline item", scene));
            let applied = result.is_ok();
            self.result(result);
            if applied
                && let Some((id, time)) = self.timeline_scrub.clone().filter(|(id, _)| id == &owner)
                && let Err(error) = self.editor.scrub_timeline_preview(&id, time)
            {
                self.result(Err(error));
            }
            self.viewport_stamp = None;
        }
        if let Some((_, time)) = self.timeline_scrub.as_ref().filter(|(id, _)| id == &owner) {
            let sample_time = timeline.motion.ease.sample(*time / duration) * duration;
            ui.label(format!(
                "Edit preview: {time:.3}s · sampled at {sample_time:.3}s"
            ));
            for track in timeline.motion.tracks.iter() {
                ui.monospace(format!(
                    "{} on {} = {:?}",
                    track.property.name(),
                    track.target(&owner),
                    track.sample(sample_time)
                ));
            }
            ui.weak("Preview uses the runtime timeline sampler in the Edit world. Markers and gameplay events do not run; no preview pose is saved.");
        }
    }
}
