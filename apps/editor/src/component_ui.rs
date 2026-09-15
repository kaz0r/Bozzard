//! Registry-driven component UI.
//!
//! A component registered with `Ui::Generic` is drawn from its field list, so new components need
//! no editor code. Component-specific widgets that cannot come from fields — derived readouts and
//! buttons that touch scene state — live in [`extras`], which is deliberately small.
use super::*;
use bozzard_scene::{
    Camera, ComponentType, Field, FieldKind, FieldValue, Gravity, Layer, Object, ParticleEmitter,
    ParticleKind, Scene, VectorRole,
};
use std::collections::BTreeMap;

/// Draws a generic component's fields and extras. Returns whether the object changed.
pub(super) fn fields(
    ui: &mut egui::Ui,
    object: &mut Object,
    entry: &ComponentType,
    scene: &Scene,
    assets: &bozzard_assets::AssetStore,
    views: &mut BTreeMap<Layer, String>,
) -> Result<bool> {
    let mut changed = false;
    let values = match bozzard_scene::middleware::registry::field_values(object, entry.name) {
        Some(values) => values?,
        None => entry
            .visible_fields(object)
            .into_iter()
            .filter_map(|field| (entry.get)(object, field.key).map(|value| (field, value)))
            .collect(),
    };
    for (field, mut value) in values {
        if widget(ui, &field, &mut value, scene, views)? {
            (entry.set)(object, field.key, value)
                .with_context(|| format!("{} · {}", entry.label, field.label))?;
            if entry.name == "audio_source" && field.key == "asset" {
                use bozzard_scene::middleware::{audio::AudioSource, registry};
                if let Some(mut source) = registry::get::<AudioSource>(object)?
                    && let Some(bozzard_assets::AssetData::Audio(data)) = assets
                        .handle(&source.asset)
                        .and_then(|h| assets.get(h))
                        .and_then(|e| e.data())
                {
                    source.duration = data.duration;
                    source.streaming = data.duration > 10.;
                    registry::set(object, &source)?;
                }
            }
            changed = true;
        }
    }
    if !entry.help.is_empty() {
        ui.weak(entry.help);
    }
    let extra = extras(ui, object, entry, scene, assets, views)?;
    Ok(changed || extra)
}

fn widget(
    ui: &mut egui::Ui,
    field: &Field,
    value: &mut FieldValue,
    scene: &Scene,
    views: &mut BTreeMap<Layer, String>,
) -> Result<bool> {
    let mut changed = false;
    ui.push_id(field.key, |ui| {
        let hover = |ui: egui::Response| {
            if field.help.is_empty() {
                ui
            } else {
                ui.on_hover_text(field.help)
            }
        };
        match field.kind {
            FieldKind::Bool => {
                let mut checked = value.bool().unwrap_or_default();
                if hover(ui.checkbox(&mut checked, field.label)).changed() {
                    *value = FieldValue::Bool(checked);
                    changed = true;
                }
            }
            FieldKind::Number { speed, min, max } => {
                let mut number = value.number().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(field.label);
                    let mut drag = egui::DragValue::new(&mut number)
                        .speed(f64::from(speed))
                        .max_decimals(4);
                    if let (Some(min), Some(max)) = (min, max) {
                        drag = drag.range(min..=max);
                    } else if let Some(min) = min {
                        drag = drag.range(min..=f32::MAX);
                    }
                    if hover(ui.add(drag)).changed() {
                        changed = true;
                    }
                });
                if changed {
                    *value = FieldValue::Number(number);
                }
            }
            FieldKind::Integer { speed, min, max } => {
                let mut number = value.number().unwrap_or_default().round();
                ui.horizontal(|ui| {
                    ui.label(field.label);
                    let mut drag = egui::DragValue::new(&mut number)
                        .speed(f64::from(speed))
                        .max_decimals(0);
                    if let (Some(min), Some(max)) = (min, max) {
                        drag = drag.range(min..=max);
                    } else if let Some(min) = min {
                        drag = drag.range(min..=f32::MAX);
                    }
                    if hover(ui.add(drag)).changed() {
                        changed = true;
                    }
                });
                if changed {
                    *value = FieldValue::Number(number.round());
                }
            }
            FieldKind::Vector {
                role,
                speed,
                min,
                max,
                axes,
            } => {
                let mut vector = value.vector().unwrap_or_default();
                if role == VectorRole::Color {
                    ui.horizontal(|ui| {
                        ui.label(field.label);
                        if hover(ui.color_edit_button_rgb(&mut vector)).changed() {
                            changed = true;
                        }
                    });
                } else {
                    hover(ui.label(field.label));
                    changed =
                        vector_row(ui, &mut vector[..axes as usize], f64::from(speed), min, max);
                }
                if changed {
                    *value = FieldValue::Vector(vector);
                }
            }
            FieldKind::Text { hint, multiline } => {
                let mut text = value.text().unwrap_or_default().to_owned();
                ui.label(field.label);
                let edit = if multiline {
                    egui::TextEdit::multiline(&mut text)
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .char_limit(4096)
                } else {
                    egui::TextEdit::singleline(&mut text)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY)
                };
                if hover(ui.add(edit)).changed() {
                    *value = FieldValue::Text(text);
                    changed = true;
                }
            }
            FieldKind::Mesh => {
                let selected = value.text().unwrap_or_default().to_owned();
                let label = if selected == "surface" {
                    "Imported surface".to_owned()
                } else if selected == "cube" {
                    "Cube".to_owned()
                } else if selected == "quad" {
                    "Quad".to_owned()
                } else {
                    selected.clone()
                };
                ui.horizontal(|ui| {
                    ui.label(field.label);
                    egui::ComboBox::from_id_salt(field.key)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (id, name) in [("cube", "Cube"), ("quad", "Quad")] {
                                if ui.selectable_label(selected == id, name).clicked()
                                    && selected != id
                                {
                                    *value = FieldValue::Text(id.to_string());
                                    changed = true;
                                }
                            }
                            for (id, source) in &scene.assets {
                                if source.kind == AssetKind::Mesh
                                    && ui.selectable_label(selected == *id, id).clicked()
                                    && selected != *id
                                {
                                    *value = FieldValue::Text(id.to_string());
                                    changed = true;
                                }
                            }
                        });
                });
                if selected == "surface" {
                    ui.weak("Imported surface stays unless another mesh is chosen.");
                }
            }
            FieldKind::Texture => {
                let selected = value.text().unwrap_or_default().to_owned();
                let label = texture_label(&selected);
                ui.horizontal(|ui| {
                    ui.label(field.label);
                    egui::ComboBox::from_id_salt(field.key)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (id, name) in TEXTURES {
                                if ui.selectable_label(selected == *id, *name).clicked()
                                    && selected != *id
                                {
                                    *value = FieldValue::Text((*id).to_string());
                                    changed = true;
                                }
                            }
                            for (id, source) in &scene.assets {
                                if source.kind == AssetKind::Image
                                    && ui.selectable_label(selected == *id, id).clicked()
                                    && selected != *id
                                {
                                    *value = FieldValue::Text(id.to_string());
                                    changed = true;
                                }
                            }
                        });
                });
            }
            FieldKind::Options { options } => {
                let selected = value.index().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label(field.label);
                    egui::ComboBox::from_id_salt(field.key)
                        .selected_text(options.get(selected).copied().unwrap_or("—"))
                        .show_ui(ui, |ui| {
                            for (index, option) in options.iter().enumerate() {
                                if ui.selectable_label(index == selected, *option).clicked()
                                    && index != selected
                                {
                                    *value = FieldValue::Index(index);
                                    changed = true;
                                }
                            }
                        });
                });
            }
            FieldKind::Object { filter, activates } => {
                let selected = value.object().unwrap_or_default().to_owned();
                let label = scene
                    .objects
                    .iter()
                    .find(|candidate| candidate.id == selected)
                    .map_or_else(|| "—".to_owned(), |candidate| candidate.name.clone());
                ui.horizontal(|ui| {
                    ui.label(field.label);
                    egui::ComboBox::from_id_salt(field.key)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for candidate in scene.objects.iter().filter(|candidate| {
                                candidate.id == selected
                                    || filter.is_none_or(|filter| filter(candidate))
                            }) {
                                if ui
                                    .selectable_label(candidate.id == selected, &candidate.name)
                                    .clicked()
                                    && candidate.id != selected
                                {
                                    *value = FieldValue::Object(candidate.id.clone());
                                    // A follow camera is also the active view camera.
                                    if let Some(layer) = activates {
                                        views.insert(layer, candidate.id.clone());
                                    }
                                    changed = true;
                                }
                            }
                        });
                });
            }
            FieldKind::Asset(kind) => {
                let selected = match value {
                    FieldValue::Asset(selected) => selected.clone(),
                    _ => None,
                };
                ui.horizontal(|ui| {
                    ui.label(field.label);
                    egui::ComboBox::from_id_salt(field.key)
                        .selected_text(selected.clone().unwrap_or_else(|| "—".into()))
                        .show_ui(ui, |ui| {
                            for (id, source) in &scene.assets {
                                if source.kind == kind
                                    && ui
                                        .selectable_label(selected.as_deref() == Some(id), id)
                                        .clicked()
                                    && selected.as_deref() != Some(id)
                                {
                                    *value = FieldValue::Asset(Some(id.to_string()));
                                    changed = true;
                                }
                            }
                        });
                });
            }
        }
    });
    Ok(changed)
}

/// Three labelled axes, optionally clamped to a lower bound.
fn vector_row(
    ui: &mut egui::Ui,
    value: &mut [f32],
    speed: f64,
    min: Option<f32>,
    max: Option<f32>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let count = value.len() as f32;
        let width = ((ui.available_width() - 12.0) / count - 18.0).max(24.0);
        for (index, axis) in value.iter_mut().enumerate() {
            ui.label(
                egui::RichText::new([" X ", " Y ", " Z "][index])
                    .background_color(theme::AXES[index])
                    .color(Color32::WHITE),
            );
            let mut drag = egui::DragValue::new(axis).speed(speed).max_decimals(3);
            if let Some(min) = min {
                drag = drag.range(min..=max.unwrap_or(f32::MAX));
            }
            if ui.add_sized([width, 20.0], drag).changed() {
                changed = true;
            }
        }
    });
    changed
}

/// The built-in texture patterns, in the order the picker shows them.
const TEXTURES: &[(&str, &str)] = &[
    ("white", "White / no map"),
    ("checker", "Checker"),
    ("normals", "World normals"),
    ("procedural_checker", "Procedural checker"),
    ("toon", "Toon (3 bands)"),
];

fn texture_label(id: &str) -> String {
    TEXTURES
        .iter()
        .find(|(name, _)| *name == id)
        .map_or_else(|| id.to_owned(), |(_, label)| (*label).to_owned())
}

/// Readouts and buttons a field list cannot express.
fn extras(
    ui: &mut egui::Ui,
    object: &mut Object,
    entry: &ComponentType,
    scene: &Scene,
    assets: &bozzard_assets::AssetStore,
    views: &mut BTreeMap<Layer, String>,
) -> Result<bool> {
    let mut changed = false;
    match entry.name {
        "mesh_collider" => {
            let Some(collider) = &mut object.mesh_collider else {
                return Ok(false);
            };
            ui.weak(format!(
                "{} source triangles",
                collider.mesh.triangles().len()
            ));
            ui.weak(if object.gravity.is_some_and(|gravity| gravity.enabled) {
                "Dynamic convex hull: holes and concavities are filled."
            } else {
                "Static / two-sided triangle surfaces."
            });
            if ui
                .add_enabled(
                    object.drawable.is_some(),
                    egui::Button::new("Rebuild from Mesh Renderer"),
                )
                .clicked()
            {
                let drawable = object.drawable.as_ref().unwrap();
                let mut rebuilt = assets.cook_mesh_collider(drawable)?;
                rebuilt.enabled = collider.enabled;
                *collider = rebuilt;
                changed = true;
            }
        }
        "particle_emitter" => {
            let Some(emitter) = &mut object.particle_emitter else {
                return Ok(false);
            };
            ui.menu_button("Apply preset", |ui| {
                for kind in ParticleKind::ALL {
                    if ui.button(kind.name()).clicked() {
                        *emitter = ParticleEmitter::preset(kind);
                        changed = true;
                        ui.close();
                    }
                }
            });
        }
        "gravity" => {
            let player = object.player_controller.is_some();
            let Some(gravity) = &mut object.gravity else {
                return Ok(false);
            };
            ui.weak(if player {
                "Player Controller keeps kinematic movement and locked rotation."
            } else {
                "Dynamic body: contacts can rotate, topple and push it. Spin sets initial angular velocity."
            });
            // Continuous-motion estimate; fixed-step integration differs slightly.
            let acceleration = f64::from(gravity.acceleration);
            let launch = f64::from(gravity.jump_speed);
            let fall_limit = f64::from(gravity.max_speed);
            let height = launch * launch / (2.0 * acceleration);
            let ascent_time = launch / acceleration;
            let descent_time = if launch <= fall_limit {
                ascent_time
            } else {
                fall_limit / acceleration
                    + (height - fall_limit * fall_limit / (2.0 * acceleration)) / fall_limit
            };
            ui.weak(format!(
                "Estimated jump: {height:.2} m high {:.2} s airtime",
                ascent_time + descent_time
            ))
            .on_hover_text(
                "Assumes enabled gravity, no obstacles, and landing at the starting height. \
                 Includes the fall speed limit; fixed-step motion may differ slightly.",
            );
            if ui
                .small_button("Reset gravity defaults")
                .on_hover_text(
                    "Reset acceleration, fall speed and jump speed. Keeps Enabled unchanged. \
                     Supports Undo.",
                )
                .clicked()
            {
                let enabled = gravity.enabled;
                *gravity = Gravity {
                    enabled,
                    ..Gravity::default()
                };
                changed = true;
            }
        }
        "trigger" => {
            let Some(trigger) = &object.trigger else {
                return Ok(false);
            };
            let checkpoint = matches!(
                trigger.action,
                bozzard_scene::TriggerAction::Checkpoint { .. }
            );
            if checkpoint
                && ui
                    .small_button("Use the safe respawn")
                    .on_hover_text(
                        "The player start, or this object's world position when the scene has no \
                         player. Place the trigger above a safe floor, clear of solids and above \
                         Fall Y.",
                    )
                    .clicked()
            {
                let safe = crate::inspector::checkpoint_respawn(scene, object);
                if let Some(trigger) = &mut object.trigger {
                    trigger.action = bozzard_scene::TriggerAction::Checkpoint { respawn: safe };
                }
                changed = true;
            }
        }
        "camera" => {
            let Some(camera) = &object.camera else {
                return Ok(false);
            };
            let orthographic = matches!(camera, Camera::Orthographic { .. });
            let follow = bozzard_scene::eligible_follow_camera(object);
            ui.horizontal(|ui| {
                for (layer, name, offered) in [
                    (Layer::ThreeD, "3D", follow),
                    (Layer::TwoD, "2D", orthographic),
                ] {
                    if !offered {
                        continue;
                    }
                    let active = views.get(&layer) == Some(&object.id);
                    if ui
                        .add_enabled(!active, egui::Button::new(format!("Use for {name}")))
                        .clicked()
                    {
                        views.insert(layer, object.id.clone());
                    }
                }
            });
            if !follow && !orthographic {
                ui.weak(
                    "The 3D view follows the player: only a root perspective camera without Spin, \
                     Gravity, collider or trigger can serve it.",
                );
            }
        }
        _ => {}
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bozzard_scene::{AddContext, COMPONENTS, Ui};

    /// Draws every generic component's section with no input. The scene-side round trip proves
    /// `get`/`set` agree; this proves the widget code for every field kind runs, and that an
    /// untouched field never reports a change.
    #[test]
    fn every_generic_component_draws_and_reports_no_change() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/first-trail.json");
        let editor = bozzard_editor::Editor::open(&path).unwrap();
        let scene = editor.scene().clone();
        let origin = scene
            .objects
            .iter()
            .find(|object| object.drawable.is_some())
            .cloned()
            .unwrap_or_default();
        let ctx = egui::Context::default();
        let mut drawn = 0;
        let mut skipped = 0;
        for entry in COMPONENTS.iter().filter(|entry| entry.ui == Ui::Generic) {
            let mut object = origin.clone();
            // Some components need an asset store to add (a cooked Mesh Collider, for instance);
            // those are covered by the scene-side round-trip test instead.
            if !(entry.present)(&object)
                && (entry.add)(
                    &mut object,
                    &AddContext {
                        layer: bozzard_scene::Layer::ThreeD,
                        scene: &scene,
                        bounds: None,
                        cooked: None,
                    },
                )
                .is_err()
            {
                skipped += 1;
                continue;
            }
            let mut views = Default::default();
            let mut output = ctx.run_ui(Default::default(), |ui| {
                let changed = fields(ui, &mut object, entry, &scene, &editor.assets, &mut views)
                    .unwrap_or_else(|error| panic!("{}: {error}", entry.label));
                assert!(!changed, "{} changed with no input", entry.label);
            });
            output.textures_delta.clear();
            drawn += 1;
        }
        assert_eq!(
            drawn + skipped,
            COMPONENTS
                .iter()
                .filter(|entry| entry.ui == Ui::Generic)
                .count(),
            "every generic component is either drawn or reported as skipped"
        );
        assert!(drawn >= 9, "only {drawn} component sections drew");

        // The loop reaches a Trigger's default Sensor action, so the Checkpoint fields and the
        // safe-respawn hook need their own draw.
        let mut checkpoint = scene
            .objects
            .iter()
            .find(|object| {
                object.trigger.as_ref().is_some_and(|trigger| {
                    matches!(
                        trigger.action,
                        bozzard_scene::TriggerAction::Checkpoint { .. }
                    )
                })
            })
            .cloned()
            .expect("first-trail authors a checkpoint");
        let trigger = bozzard_scene::component_type("trigger").expect("trigger row");
        let mut views = Default::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let changed = fields(
                ui,
                &mut checkpoint,
                trigger,
                &scene,
                &editor.assets,
                &mut views,
            )
            .unwrap_or_else(|error| panic!("Trigger: {error}"));
            assert!(!changed, "Trigger changed with no input");
        });
        output.textures_delta.clear();
    }
}
