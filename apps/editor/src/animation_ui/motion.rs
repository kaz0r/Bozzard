//! Clip and blend-space widgets shared by base states and body layers.
use super::clip_picker;
use bozzard_scene::middleware::animation::{BlendPoint, BlendSample, Motion, data::Rig};
use eframe::egui;
use std::collections::BTreeMap;

pub(super) fn parameter(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut String,
    parameters: &BTreeMap<String, f32>,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.as_str())
        .show_ui(ui, |ui| {
            for name in parameters.keys() {
                ui.selectable_value(value, name.clone(), name);
            }
        });
}
pub(super) fn weight_parameter(
    ui: &mut egui::Ui,
    value: &mut Option<String>,
    parameters: &BTreeMap<String, f32>,
) {
    egui::ComboBox::from_id_salt("weight-parameter")
        .selected_text(value.as_deref().unwrap_or("Fixed weight"))
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "Fixed weight");
            for name in parameters.keys() {
                ui.selectable_value(value, Some(name.clone()), name);
            }
        });
}
pub(super) fn editor(
    ui: &mut egui::Ui,
    motion: &mut Motion,
    rig: &Rig,
    parameters: &mut BTreeMap<String, f32>,
) {
    let mut kind = match motion {
        Motion::Clip { .. } => 0,
        Motion::Blend1d { .. } => 1,
        Motion::Blend2d { .. } => 2,
    };
    let before = kind;
    egui::ComboBox::from_id_salt("motion-kind")
        .selected_text(["Single clip", "Blend by speed", "Blend by direction"][kind])
        .show_ui(ui, |ui| {
            for (index, label) in ["Single clip", "Blend by speed", "Blend by direction"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut kind, index, *label);
            }
        });
    if kind != before {
        *motion = match kind {
            1 => {
                parameters.entry("Speed".into()).or_insert(0.);
                Motion::Blend1d {
                    parameter: "Speed".into(),
                    samples: vec![BlendSample {
                        threshold: 0.,
                        clip: 0,
                    }],
                }
            }
            2 => {
                for name in ["Move X", "Move Y"] {
                    parameters.entry(name.into()).or_insert(0.);
                }
                Motion::Blend2d {
                    parameters: ["Move X".into(), "Move Y".into()],
                    samples: vec![
                        BlendPoint {
                            position: [0., 0.],
                            clip: 0,
                        },
                        BlendPoint {
                            position: [1., 0.],
                            clip: 0,
                        },
                        BlendPoint {
                            position: [0., 1.],
                            clip: 0,
                        },
                    ],
                }
            }
            _ => Motion::Clip { clip: 0 },
        };
    }
    match motion {
        Motion::Clip { clip } => clip_picker(ui, "clip", clip, rig),
        Motion::Blend1d {
            parameter: axis,
            samples,
        } => {
            parameter(ui, "blend-axis", axis, parameters);
            blend_1d(ui, samples, rig);
        }
        Motion::Blend2d {
            parameters: axes,
            samples,
        } => {
            let before = axes.clone();
            ui.horizontal_wrapped(|ui| {
                ui.label("Horizontal");
                parameter(ui, "blend-x", &mut axes[0], parameters);
                ui.label("Forward");
                parameter(ui, "blend-y", &mut axes[1], parameters);
            });
            if axes[0] == axes[1] {
                if axes[0] != before[0] {
                    axes[1] = before[0].clone();
                } else {
                    axes[0] = before[1].clone();
                }
            }
            if let Some(value) = blend_2d(
                ui,
                samples,
                rig,
                [parameters[&axes[0]], parameters[&axes[1]]],
            ) {
                for (axis, value) in axes.iter().zip(value) {
                    parameters.insert(axis.clone(), value);
                }
            }
        }
    }
}
fn blend_1d(ui: &mut egui::Ui, samples: &mut Vec<BlendSample>, rig: &Rig) {
    ui.small("Samples share one cycle phase, so changing speed keeps footsteps in step.");
    let mut remove = None;
    let count = samples.len();
    for (index, sample) in samples.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::DragValue::new(&mut sample.threshold)
                        .speed(0.05)
                        .prefix("At "),
                );
                clip_picker(ui, "clip", &mut sample.clip, rig);
                if ui
                    .add_enabled(count > 1, egui::Button::new("Remove"))
                    .clicked()
                {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        samples.remove(index);
    }
    if ui
        .add_enabled(samples.len() < 64, egui::Button::new("Add speed sample"))
        .clicked()
    {
        samples.push(BlendSample {
            threshold: samples.last().map_or(0., |s| s.threshold + 1.),
            clip: 0,
        });
    }
    samples.sort_by(|a, b| a.threshold.total_cmp(&b.threshold));
}
fn blend_2d(
    ui: &mut egui::Ui,
    samples: &mut Vec<BlendPoint>,
    rig: &Rig,
    current: [f32; 2],
) -> Option<[f32; 2]> {
    ui.small("Place clips at their movement values. The engine blends between nearby points and clamps at the boundary.");
    ui.small("Drag in the diagram to preview a direction. Numbered points match the clip rows.");
    let value = diagram(ui, samples, current);
    let mut remove = None;
    let count = samples.len();
    for (index, sample) in samples.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.horizontal_wrapped(|ui| {
                clip_picker(ui, "clip", &mut sample.clip, rig);
                ui.add(
                    egui::DragValue::new(&mut sample.position[0])
                        .speed(0.05)
                        .prefix("X "),
                );
                ui.add(
                    egui::DragValue::new(&mut sample.position[1])
                        .speed(0.05)
                        .prefix("Y "),
                );
                if ui
                    .add_enabled(count > 3, egui::Button::new("Remove"))
                    .clicked()
                {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        samples.remove(index);
    }
    if ui
        .add_enabled(
            samples.len() < 64,
            egui::Button::new("Add direction sample"),
        )
        .clicked()
    {
        let x = samples.iter().map(|s| s.position[0]).fold(0., f32::max) + 1.;
        samples.push(BlendPoint {
            position: [x, 0.],
            clip: 0,
        });
    }
    value
}
fn diagram(ui: &mut egui::Ui, samples: &[BlendPoint], current: [f32; 2]) -> Option<[f32; 2]> {
    let width = ui.available_width().clamp(1., 320.);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 160.), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 5., ui.visuals().extreme_bg_color);
    let extent = samples
        .iter()
        .flat_map(|s| s.position)
        .map(f32::abs)
        .fold(1., f32::max)
        * 1.2;
    let scale = rect.height().min(rect.width()) * 0.45 / extent;
    let screen = |point: [f32; 2]| rect.center() + egui::vec2(point[0], -point[1]) * scale;
    let stroke = egui::Stroke::new(1., ui.visuals().weak_text_color());
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.center().y),
            egui::pos2(rect.right(), rect.center().y),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(rect.center().x, rect.top()),
            egui::pos2(rect.center().x, rect.bottom()),
        ],
        stroke,
    );
    for (index, sample) in samples.iter().enumerate() {
        let point = screen(sample.position);
        painter.circle_filled(point, 4., ui.visuals().text_color());
        painter.text(
            point + egui::vec2(6., -4.),
            egui::Align2::LEFT_BOTTOM,
            (index + 1).to_string(),
            egui::FontId::proportional(11.),
            ui.visuals().text_color(),
        );
    }
    painter.circle_stroke(
        screen(current),
        6.,
        egui::Stroke::new(2., egui::Color32::from_rgb(80, 210, 170)),
    );
    response
        .interact_pointer_pos()
        .filter(|_| response.clicked() || response.dragged())
        .map(|p| {
            let delta = (rect.clamp(p) - rect.center()) / scale;
            [delta.x, -delta.y]
        })
}
