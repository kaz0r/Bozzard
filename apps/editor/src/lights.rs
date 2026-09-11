use super::*;
use bozzard_scene::{Light, LightKind};
use glam::Mat4;

pub fn inspector(ui: &mut egui::Ui, value: &mut Option<Light>) {
    let mut has_light = value.is_some();
    if ui.checkbox(&mut has_light, "Light (3D)").changed() {
        *value = has_light.then(Light::default);
    }
    if let Some(light) = value {
        ui.push_id("local-light", |ui| {
            ui.checkbox(&mut light.enabled, "Enabled");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut light.kind, LightKind::Point, "Point");
                ui.selectable_value(&mut light.kind, LightKind::Spot, "Spot");
                ui.selectable_value(&mut light.kind, LightKind::Directional, "Directional");
            });
            ui.horizontal(|ui| {
                ui.label("Color");
                inspector::color_edit_button_rgb(ui, &mut light.color);
            });
            ui.horizontal(|ui| {
                ui.label(if light.kind == LightKind::Directional {
                    "Illuminance (lux)"
                } else {
                    "Intensity (cd)"
                });
                ui.add(
                    egui::DragValue::new(&mut light.intensity)
                        .speed(1.)
                        .range(0.0..=100_000.),
                );
            });
            if light.kind != LightKind::Directional {
                ui.horizontal(|ui| {
                    ui.label("Range");
                    ui.add(
                        egui::DragValue::new(&mut light.range)
                            .speed(0.1)
                            .range(0.001..=100_000.),
                    );
                });
            } else {
                ui.weak("Rotate to aim local −Z. Position and scale do not affect illumination.");
            }
            if light.kind == LightKind::Spot {
                ui.add(
                    egui::Slider::new(&mut light.outer_angle_degrees, 0.1..=89.9)
                        .text("Outer angle °"),
                );
                light.inner_angle_degrees =
                    light.inner_angle_degrees.min(light.outer_angle_degrees);
                ui.add(
                    egui::Slider::new(
                        &mut light.inner_angle_degrees,
                        0.0..=light.outer_angle_degrees,
                    )
                    .text("Inner angle °"),
                );
                ui.weak("Angles from the center. Rotate the object to aim local −Z.");
            }
            if light.kind != LightKind::Directional {
                ui.checkbox(&mut light.shadows, "Cast shadows");
                if light.shadows {
                    ui.weak(if light.kind == LightKind::Spot {
                        "1024 px · Up to 8 shadowed spotlights per scene."
                    } else {
                        "6 × 512 px · Up to 4 shadowed point lights per scene."
                    });
                    ui.add(
                        egui::DragValue::new(&mut light.shadow_bias)
                            .speed(0.001)
                            .range(0.0..=1.0)
                            .prefix("Depth bias "),
                    )
                    .on_hover_text(
                        "World units. Increase slightly to remove surface shadow speckling.",
                    );
                    ui.add(
                        egui::DragValue::new(&mut light.shadow_normal_bias)
                            .speed(0.001)
                            .range(0.0..=1.0)
                            .prefix("Normal bias "),
                    )
                    .on_hover_text("World units. Large offsets can detach shadows from objects.");
                }
                ui.weak("Range ignores scale.");
            } else {
                ui.weak("Object-directional shadows are not available yet. Scene sun shadows remain supported.");
            }
        });
        ui.separator();
    }
}
impl App {
    /// Editor-only markers: fixed screen size, selectable through geometry like gizmos.
    pub fn light_overlay(
        &self,
        ui: &egui::Ui,
        rect: Rect,
        projection: Mat4,
        pointer: Option<Pos2>,
    ) -> Result<Option<String>> {
        if self.workspace.layer_2d || self.editor.play.is_some() {
            return Ok(None);
        }
        let demo = bozzard_demo::SceneDemo::new(self.editor.scene())?;
        let matrices = demo.instance.global_transforms(&demo.app.world)?;
        let painter = ui.painter().with_clip_rect(rect);
        let project = |v: glam::Vec4| {
            let p = v.truncate() / v.w;
            Pos2::new(
                rect.left() + (p.x + 1.) * rect.width() * 0.5,
                rect.top() + (1. - p.y) * rect.height() * 0.5,
            )
        };
        let mut picked: Option<(f32, String)> = None;
        for object in &self.editor.scene().objects {
            let Some(light) = object.light else { continue };
            let world = light.at(matrices[&object.id])?;
            let origin = Vec3::from(world.position);
            let clip = projection * origin.extend(1.);
            let selected = self.editor.selected.as_deref() == Some(&object.id);
            let color = if !light.enabled {
                Color32::GRAY
            } else if selected {
                Color32::from_rgb(255, 212, 90)
            } else {
                Color32::from_rgb(255, 243, 189)
            };
            if clip.w > 0. && (0.0..=clip.w).contains(&clip.z) {
                let center = project(clip);
                if rect.contains(center) {
                    let distance = pointer.map(|p| p.distance(center));
                    let hovered = distance.is_some_and(|d| d <= 11.);
                    painter.circle_filled(center, 5., Color32::from_black_alpha(180));
                    painter.circle_stroke(
                        center,
                        7.,
                        egui::Stroke::new(if hovered { 2.5 } else { 1.5 }, color),
                    );
                    for i in 0..8 {
                        let angle = i as f32 * std::f32::consts::TAU / 8.;
                        let axis = egui::vec2(angle.cos(), angle.sin());
                        painter
                            .line_segment([center + axis * 9., center + axis * 12.], (1., color));
                    }
                    if hovered && picked.as_ref().is_none_or(|(d, _)| distance.unwrap() < *d) {
                        picked = Some((distance.unwrap(), object.id.clone()));
                    }
                    if selected || hovered {
                        painter.text(
                            center + egui::vec2(16., -14.),
                            egui::Align2::LEFT_BOTTOM,
                            &object.name,
                            egui::FontId::proportional(12.),
                            color,
                        );
                    }
                }
            }
            if !selected {
                continue;
            }
            let edge = |a: Vec3, b: Vec3| {
                if let Some((a, b)) =
                    colliders::clip_edge(projection * a.extend(1.), projection * b.extend(1.))
                {
                    painter.line_segment(
                        [project(a), project(b)],
                        egui::Stroke::new(1., color.gamma_multiply(0.6)),
                    );
                }
            };
            if light.kind == LightKind::Point {
                for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::X)] {
                    for i in 0..64 {
                        let point = |j: usize| {
                            let t = j as f32 * std::f32::consts::TAU / 64.;
                            origin + (a * t.cos() + b * t.sin()) * light.range
                        };
                        edge(point(i), point(i + 1));
                    }
                }
            } else if light.kind == LightKind::Directional {
                let forward = Vec3::from(world.direction);
                let side = forward.any_orthonormal_vector() * 0.2;
                let tip = origin + forward * 2.;
                edge(origin, tip);
                edge(tip, tip - forward * 0.4 + side);
                edge(tip, tip - forward * 0.4 - side);
            } else {
                let forward = Vec3::from(world.direction);
                let up = if forward.y.abs() < 0.99 {
                    Vec3::Y
                } else {
                    Vec3::X
                };
                let right = forward.cross(up).normalize();
                let up = right.cross(forward);
                for angle in [light.inner_angle_degrees, light.outer_angle_degrees] {
                    let (sin, cos) = angle.to_radians().sin_cos();
                    let point = |j: usize| {
                        let t = j as f32 * std::f32::consts::TAU / 64.;
                        origin
                            + (forward * cos + (right * t.cos() + up * t.sin()) * sin) * light.range
                    };
                    for i in 0..64 {
                        edge(point(i), point(i + 1));
                    }
                    for i in [0, 16, 32, 48] {
                        edge(origin, point(i));
                    }
                }
                edge(origin, origin + forward * light.range);
            }
        }
        Ok(picked.map(|(_, id)| id))
    }
}
