//! Curve and cinematic authoring within the normal validated, undoable inspector transaction.
use anyhow::Result;
use bozzard_scene::{
    Object, Scene,
    blueprint::ObjectRef,
    middleware::{
        curve::{Curve, Interpolation, Key, MAX_KEYS},
        registry,
        timeline::{CameraCut, Marker, Timeline},
        tween::{Property, Track, Tween},
    },
};
use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};
use std::sync::Arc;

pub fn component(ui: &mut egui::Ui, object: &mut Object, name: &str, scene: &Scene) -> Result<()> {
    match name {
        "tween" => {
            if let Some(mut tween) = registry::get::<Tween>(object)? {
                let before = tween.clone();
                tracks(ui, &mut tween, scene);
                if tween != before {
                    registry::set(object, &tween)?;
                }
            }
        }
        "timeline" => {
            if let Some(mut timeline) = registry::get::<Timeline>(object)? {
                let before = timeline.clone();
                tracks(ui, &mut timeline.motion, scene);
                let duration = timeline.motion.duration;
                ui.collapsing("Markers → On Timeline Event", |ui| {
                    let mut remove = None;
                    for (index, marker) in
                        Arc::make_mut(&mut timeline.markers).iter_mut().enumerate()
                    {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::DragValue::new(&mut marker.time)
                                        .range(0.0..=duration)
                                        .speed(0.01)
                                        .suffix(" s"),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut marker.name)
                                        .desired_width(110.)
                                        .char_limit(256),
                                );
                                if ui.small_button("×").clicked() {
                                    remove = Some(index);
                                }
                            });
                        });
                    }
                    if let Some(index) = remove {
                        Arc::make_mut(&mut timeline.markers).remove(index);
                    }
                    if ui
                        .add_enabled(
                            timeline.markers.len() < 1024,
                            egui::Button::new("Add marker"),
                        )
                        .clicked()
                    {
                        Arc::make_mut(&mut timeline.markers).push(Marker {
                            time: duration,
                            name: "Event".into(),
                        });
                    }
                    Arc::make_mut(&mut timeline.markers).sort_by(|a, b| a.time.total_cmp(&b.time));
                });
                ui.collapsing("Camera cuts", |ui| {
                    let mut remove = None;
                    for (index, cut) in Arc::make_mut(&mut timeline.cameras).iter_mut().enumerate()
                    {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::DragValue::new(&mut cut.time)
                                        .range(0.0..=duration)
                                        .speed(0.01)
                                        .suffix(" s"),
                                );
                                if ui.small_button("×").clicked() {
                                    remove = Some(index);
                                }
                            });
                            egui::ComboBox::from_id_salt("camera")
                                .selected_text(&cut.camera)
                                .show_ui(ui, |ui| {
                                    for object in
                                        scene.objects.iter().filter(|o| o.camera.is_some())
                                    {
                                        ui.selectable_value(
                                            &mut cut.camera,
                                            object.id.clone(),
                                            &object.name,
                                        );
                                    }
                                });
                            egui::ComboBox::from_id_salt("view")
                                .selected_text(format!("{:?}", cut.layer))
                                .show_ui(ui, |ui| {
                                    for &layer in scene.views.keys() {
                                        ui.selectable_value(
                                            &mut cut.layer,
                                            layer,
                                            format!("{layer:?}"),
                                        );
                                    }
                                });
                        });
                    }
                    if let Some(index) = remove {
                        Arc::make_mut(&mut timeline.cameras).remove(index);
                    }
                    if let Some((&layer, camera)) = scene.views.first_key_value()
                        && ui
                            .add_enabled(
                                timeline.cameras.len() < 1024,
                                egui::Button::new("Add camera cut"),
                            )
                            .clicked()
                    {
                        Arc::make_mut(&mut timeline.cameras).push(CameraCut {
                            time: duration,
                            camera: camera.clone(),
                            layer,
                        });
                    }
                    Arc::make_mut(&mut timeline.cameras).sort_by(|a, b| a.time.total_cmp(&b.time));
                });
                if timeline != before {
                    registry::set(object, &timeline)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
fn tracks(ui: &mut egui::Ui, tween: &mut Tween, scene: &Scene) {
    ui.label("Motion tracks");
    let mut remove = None;
    for index in 0..tween.tracks.len() {
        let source = &tween.tracks[index];
        let mut edited = None;
        ui.push_id(index, |ui| {
            egui::CollapsingHeader::new(format!("{} · {}", index + 1, source.property.name()))
                .show(ui, |ui| {
                    let mut track = source.clone();
                    let before = track.property;
                    egui::ComboBox::from_id_salt("property")
                        .selected_text(track.property.name())
                        .show_ui(ui, |ui| {
                            for property in Property::ALL {
                                ui.selectable_value(&mut track.property, property, property.name());
                            }
                        });
                    if track.property != before {
                        track.channels = Track::new(track.property).channels;
                    }
                    egui::ComboBox::from_id_salt("target")
                        .selected_text(match &track.target {
                            ObjectRef::Id(id) => id,
                            _ => "Self",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut track.target, ObjectRef::SelfObject, "Self");
                            for object in &scene.objects {
                                ui.selectable_value(
                                    &mut track.target,
                                    ObjectRef::Id(object.id.clone()),
                                    &object.name,
                                );
                            }
                        });
                    for (index, channel) in track.channels.iter_mut().enumerate() {
                        ui.push_id(index, |ui| {
                            let label = match track.property {
                                Property::Color => ["Red", "Green", "Blue"][index],
                                _ if track.property.channels() == 3 => ["X", "Y", "Z"][index],
                                _ => "Value",
                            };
                            ui.collapsing(label, |ui| {
                                curve(ui, channel, tween.duration);
                            });
                        });
                    }
                    if &track != source {
                        edited = Some(track);
                    }
                    if ui.small_button("Remove track").clicked() {
                        remove = Some(index);
                    }
                });
        });
        if let Some(track) = edited {
            Arc::make_mut(&mut tween.tracks)[index] = track;
        }
    }
    if let Some(index) = remove {
        Arc::make_mut(&mut tween.tracks).remove(index);
    }
    if ui
        .add_enabled(
            tween.tracks.len() < 128,
            egui::Button::new("Add motion track"),
        )
        .clicked()
    {
        Arc::make_mut(&mut tween.tracks).push(Track::new(Property::Translation));
    }
    ui.small("Times are seconds. Use Blueprint Play / Pause / Seek nodes to preview in Play mode.");
}
/// Shared editor for tween, animation and particle scalar curves.
pub fn curve(ui: &mut egui::Ui, curve: &mut Curve, duration: f32) {
    curve_with_limit(ui, curve, duration, MAX_KEYS)
}
pub fn curve_with_limit(ui: &mut egui::Ui, curve: &mut Curve, duration: f32, limit: usize) {
    egui::ComboBox::from_id_salt("interpolation")
        .selected_text(format!("{:?}", curve.interpolation))
        .show_ui(ui, |ui| {
            for mode in [
                Interpolation::Step,
                Interpolation::Linear,
                Interpolation::Cubic,
            ] {
                ui.selectable_value(&mut curve.interpolation, mode, format!("{mode:?}"));
            }
        });
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(60.), 64.),
        Sense::hover(),
    );
    let points: Vec<_> = (0..=64)
        .map(|i| curve.sample(duration * i as f32 / 64.))
        .collect();
    let min = points.iter().copied().fold(f32::INFINITY, f32::min);
    let max = points.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let span = (max - min).max(0.01);
    let points: Vec<_> = points
        .iter()
        .enumerate()
        .map(|(i, &y)| {
            Pos2::new(
                rect.left() + rect.width() * i as f32 / 64.,
                rect.bottom() - 4. - (y - min) / span * (rect.height() - 8.),
            )
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        points,
        Stroke::new(1.5, Color32::from_rgb(100, 190, 245)),
    ));
    let mut remove = None;
    let count = curve.keys.len();
    egui::Grid::new("keys").striped(true).show(ui, |ui| {
        ui.label("Time");
        ui.label("Value");
        if curve.interpolation == Interpolation::Cubic {
            ui.label("In / out tangent");
        }
        ui.end_row();
        for (index, key) in curve.keys.iter_mut().enumerate() {
            ui.add(
                egui::DragValue::new(&mut key.time)
                    .range(0.0..=duration)
                    .speed(0.01),
            );
            ui.add(egui::DragValue::new(&mut key.value).speed(0.01));
            if curve.interpolation == Interpolation::Cubic {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut key.incoming).speed(0.01));
                    ui.add(egui::DragValue::new(&mut key.outgoing).speed(0.01));
                });
            }
            if ui.add_enabled(count > 1, egui::Button::new("×")).clicked() {
                remove = Some(index);
            }
            ui.end_row();
        }
    });
    if let Some(index) = remove {
        curve.keys.remove(index);
    }
    if ui
        .add_enabled(count < limit, egui::Button::new("Add key"))
        .clicked()
    {
        // Split the widest open interval, so a new key never silently replaces an existing key.
        let mut intervals: Vec<_> = curve
            .keys
            .windows(2)
            .map(|w| (w[0].time, w[1].time))
            .collect();
        if let Some(last) = curve.keys.last() {
            intervals.push((last.time, duration));
        }
        if let Some(first) = curve.keys.first() {
            intervals.push((0., first.time));
        }
        if let Some((a, b)) = intervals
            .into_iter()
            .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
        {
            let time = (a + b) * 0.5;
            if time > a && time < b {
                curve.keys.push(Key::new(time, curve.sample(time)));
            }
        }
    }
    curve.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
}
