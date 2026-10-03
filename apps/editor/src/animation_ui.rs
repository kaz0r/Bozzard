//! Animator controller authoring. Every edit returns through the inspector's normal Undo path.
use anyhow::{Context, Result};
use bozzard_scene::{
    Object,
    middleware::{
        animation::{Animator, Comparison, Motion, RootMotion, StateDefinition, Transition},
        curve::Repeat,
        registry,
        timeline::Marker,
    },
};
use eframe::egui;
use std::{collections::BTreeMap, sync::Arc};

mod ik;
mod layers;
mod motion;
mod preview;
mod retarget;
mod warp;
pub use preview::PreviewRequest;

pub fn component(
    ui: &mut egui::Ui,
    object: &mut Object,
    assets: &bozzard_assets::AssetStore,
    scene: &bozzard_scene::Scene,
    owner_key: &str,
    graph_layout: &mut BTreeMap<String, [f32; 2]>,
    active_state: Option<usize>,
) -> Result<Option<PreviewRequest>> {
    let Some(mut animator) = registry::get::<Animator>(object)? else {
        return Ok(None);
    };
    let before = animator.clone();
    ui.small(
        "Import an animated model, choose how clips blend, then add body layers or foot placement.",
    );
    if ui
        .button(if animator.rig.nodes.is_empty() {
            "Import skeleton and clips"
        } else {
            "Reimport skeleton and clips"
        })
        .on_hover_text("Uses the selected model. Reimport resets the controller; Undo restores it.")
        .clicked()
    {
        let bozzard_assets::AssetData::Mesh(mesh) = assets
            .handle(&animator.asset)
            .and_then(|h| assets.get(h))
            .and_then(|e| e.data())
            .context("Choose a loaded model in the Animator's Model field first")?
        else {
            anyhow::bail!("Animator requires a model asset");
        };
        let skin = mesh
            .skin
            .as_ref()
            .context("This model has no skeleton or animation")?;
        animator = Animator::from_rig(animator.asset.clone(), skin.rig.clone());
    }
    ui.small(format!(
        "{} bones · {} clips · {} states",
        animator.rig.nodes.len(),
        animator.rig.clips.len(),
        animator.states.len()
    ));
    retarget::editor(ui, &mut animator, assets, scene, owner_key)?;
    if animator.rig.clips.is_empty() {
        ui.weak("This skeleton has no clips yet. Import animations or export a model with animation from Blender.");
        if animator != before {
            registry::set(object, &animator)?;
        }
        return Ok(None);
    }
    movement_preset(ui, &mut animator);
    parameters(ui, &mut animator);
    ui.label("Starting state");
    egui::ComboBox::from_id_salt("initial-state")
        .selected_text(&animator.initial)
        .show_ui(ui, |ui| {
            for state in animator.states.iter() {
                ui.selectable_value(&mut animator.initial, state.name.clone(), &state.name);
            }
        });
    ui.collapsing("State graph", |ui| {
        state_graph(ui, &mut animator, graph_layout, active_state);
    });
    states(ui, &mut animator);
    transitions(ui, &mut animator);
    layers::editor(ui, &mut animator);
    ik::editor(ui, &mut animator, scene);
    ui.collapsing("Root motion", |ui| {
        root_motion(ui, &mut animator);
    });
    warp::editor(ui, &mut animator, scene);
    markers(ui, &mut animator);
    let request = preview::editor(ui, &animator, animator != before);
    if animator != before {
        registry::set(object, &animator)?;
    }
    Ok(request)
}

fn parameters(ui: &mut egui::Ui, animator: &mut Animator) {
    ui.collapsing("Parameters", |ui| {
        let mut remove = None;
        for (name, value) in &mut animator.parameters {
            ui.horizontal(|ui| {
                ui.label(name);
                ui.add(egui::DragValue::new(value).speed(0.05));
                if ui.small_button("×").clicked() {
                    remove = Some(name.clone());
                }
            });
        }
        if let Some(name) = remove {
            animator.parameters.remove(&name);
            Arc::make_mut(&mut animator.transitions).retain(|t| t.parameter != name);
            for state in Arc::make_mut(&mut animator.states) {
                if state.motion.uses_parameter(&name) {
                    state.motion = Motion::Clip { clip: 0 };
                }
            }
            for layer in Arc::make_mut(&mut animator.layers) {
                if layer.motion.uses_parameter(&name) {
                    layer.motion = Motion::Clip { clip: 0 };
                }
                if layer.weight_parameter.as_ref() == Some(&name) {
                    layer.weight_parameter = None;
                }
            }
            for chain in Arc::make_mut(&mut animator.ik) {
                if chain.weight_parameter.as_ref() == Some(&name) {
                    chain.weight_parameter = None;
                }
            }
        }
        let id = ui.make_persistent_id("new-animation-parameter");
        let mut name = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_else(|| "Speed".into()));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(100.)
                    .char_limit(128),
            );
            if ui
                .add_enabled(
                    !name.trim().is_empty()
                        && !animator.parameters.contains_key(&name)
                        && animator.parameters.len() < 128,
                    egui::Button::new("Add"),
                )
                .clicked()
            {
                animator.parameters.insert(name.clone(), 0.);
            }
        });
        ui.data_mut(|d| d.insert_temp(id, name));
        ui.small("Blueprint Set Animation Parameter changes these values at runtime.");
    });
}

fn states(ui: &mut egui::Ui, animator: &mut Animator) {
    ui.collapsing("States and blend trees", |ui| {
        let mut remove = None;
        for (index, state) in Arc::make_mut(&mut animator.states).iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                let name = state.name.clone();
                egui::CollapsingHeader::new(&name).show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut state.name).char_limit(128));
                    if state.name != name {
                        if animator.initial == name {
                            animator.initial = state.name.clone();
                        }
                        for window in Arc::make_mut(&mut animator.warps) {
                            if window.state == name {
                                window.state = state.name.clone();
                            }
                        }
                        for t in Arc::make_mut(&mut animator.transitions) {
                            if t.from == name {
                                t.from = state.name.clone();
                            }
                            if t.to == name {
                                t.to = state.name.clone();
                            }
                        }
                    }
                    let warped = animator.warps.iter().any(|w| w.state == state.name);
                    ui.add_enabled_ui(!warped, |ui| repeat(ui, &mut state.repeat));
                    if warped {
                        ui.small("This state plays once because it has a motion-warp window. Remove its windows to change repetition.");
                    }
                    motion::editor(
                        ui,
                        &mut state.motion,
                        &animator.rig,
                        &mut animator.parameters,
                    );
                    if ui.small_button("Remove state").clicked() {
                        remove = Some(index);
                    }
                });
            });
        }
        if let Some(index) = remove {
            let state = Arc::make_mut(&mut animator.states).remove(index);
            Arc::make_mut(&mut animator.warps).retain(|window| window.state != state.name);
            Arc::make_mut(&mut animator.transitions)
                .retain(|t| t.from != state.name && t.to != state.name);
            if animator.initial == state.name {
                animator.initial = animator
                    .states
                    .first()
                    .map_or_else(String::new, |s| s.name.clone());
            }
        }
        if ui
            .add_enabled(animator.states.len() < 64, egui::Button::new("Add state"))
            .clicked()
        {
            let name = (1..)
                .map(|n| format!("State {n}"))
                .find(|name| !animator.states.iter().any(|s| &s.name == name))
                .unwrap();
            if animator.states.is_empty() {
                animator.initial = name.clone();
            }
            Arc::make_mut(&mut animator.states).push(StateDefinition {
                name,
                motion: Motion::Clip { clip: 0 },
                repeat: Repeat::Loop,
            });
        }
    });
}

fn transitions(ui: &mut egui::Ui, animator: &mut Animator) {
    ui.collapsing("Transitions (top to bottom priority)", |ui| {
        let mut remove = None;
        for (index, t) in Arc::make_mut(&mut animator.transitions)
            .iter_mut()
            .enumerate()
        {
            ui.push_id(index, |ui| {
                egui::CollapsingHeader::new(format!("{} → {}", t.from, t.to)).show(ui, |ui| {
                    for (key, label, value) in
                        [("from", "From", &mut t.from), ("to", "To", &mut t.to)]
                    {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(key)
                            .selected_text(value.as_str())
                            .show_ui(ui, |ui| {
                                if key == "from" {
                                    ui.selectable_value(value, "*".into(), "Any state");
                                }
                                for s in animator.states.iter() {
                                    ui.selectable_value(value, s.name.clone(), &s.name);
                                }
                            });
                    }
                    egui::ComboBox::from_id_salt("parameter")
                        .selected_text(&t.parameter)
                        .show_ui(ui, |ui| {
                            for name in animator.parameters.keys() {
                                ui.selectable_value(&mut t.parameter, name.clone(), name);
                            }
                        });
                    egui::ComboBox::from_id_salt("comparison")
                        .selected_text(format!("{:?}", t.comparison))
                        .show_ui(ui, |ui| {
                            for op in [
                                Comparison::Above,
                                Comparison::Below,
                                Comparison::Equal,
                                Comparison::NotEqual,
                            ] {
                                ui.selectable_value(&mut t.comparison, op, format!("{op:?}"));
                            }
                        });
                    ui.add(
                        egui::DragValue::new(&mut t.threshold)
                            .prefix("Threshold ")
                            .speed(0.05),
                    );
                    ui.add(
                        egui::DragValue::new(&mut t.fade)
                            .prefix("Fade ")
                            .suffix(" s")
                            .range(0.0..=60.)
                            .speed(0.01),
                    );
                    let mut exit = t.exit_time.is_some();
                    if ui.checkbox(&mut exit, "Wait for exit time").changed() {
                        t.exit_time = exit.then_some(1.);
                    }
                    if let Some(time) = &mut t.exit_time {
                        ui.add(egui::Slider::new(time, 0.0..=1.).text("Progress"));
                    }
                    if ui.small_button("Remove transition").clicked() {
                        remove = Some(index);
                    }
                });
            });
        }
        if let Some(index) = remove {
            Arc::make_mut(&mut animator.transitions).remove(index);
        }
        if ui
            .add_enabled(
                !animator.parameters.is_empty()
                    && !animator.states.is_empty()
                    && animator.transitions.len() < 128,
                egui::Button::new("Add transition"),
            )
            .clicked()
        {
            Arc::make_mut(&mut animator.transitions).push(Transition {
                from: "*".into(),
                to: animator.states[0].name.clone(),
                parameter: animator.parameters.keys().next().unwrap().clone(),
                comparison: Comparison::Above,
                threshold: 0.5,
                fade: 0.2,
                exit_time: None,
            });
        }
    });
}

fn markers(ui: &mut egui::Ui, animator: &mut Animator) {
    ui.collapsing("Clip events → On Animation Event", |ui| {
        for index in 0..animator.rig.clips.len() {
            let clip = &animator.rig.clips[index];
            let mut edited = None;
            ui.push_id(index, |ui| {
                ui.collapsing(&clip.name, |ui| {
                    let mut events = clip.events.clone();
                    ui.label(format!("{:.3} seconds", clip.duration));
                    let mut remove = None;
                    for (i, event) in events.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut event.time)
                                    .range(0.0..=clip.duration)
                                    .speed(0.01),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut event.name)
                                    .desired_width(100.)
                                    .char_limit(256),
                            );
                            if ui.small_button("×").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        events.remove(i);
                    }
                    if ui
                        .add_enabled(events.len() < 1024, egui::Button::new("Add event"))
                        .clicked()
                    {
                        events.push(Marker {
                            time: clip.duration * 0.5,
                            name: "Event".into(),
                        });
                    }
                    events.sort_by(|a, b| a.time.total_cmp(&b.time));
                    if events != clip.events {
                        edited = Some(events);
                    }
                });
            });
            if let Some(events) = edited {
                Arc::make_mut(&mut animator.rig).clips[index].events = events;
            }
        }
    });
}

fn root_motion(ui: &mut egui::Ui, animator: &mut Animator) {
    let mut root = animator.root_motion.is_some();
    if ui
        .checkbox(&mut root, "Move the character using the animation")
        .changed()
    {
        if !root {
            Arc::make_mut(&mut animator.warps).clear();
        }
        animator.root_motion = root.then_some(RootMotion {
            node: 0,
            translation: [true, false, true],
            yaw: false,
        });
    }
    if let Some(root) = &mut animator.root_motion {
        egui::ComboBox::from_id_salt("root-node")
            .selected_text(&animator.rig.nodes[root.node].name)
            .show_ui(ui, |ui| {
                for (index, node) in animator.rig.nodes.iter().enumerate() {
                    ui.selectable_value(&mut root.node, index, &node.name);
                }
            });
        ui.horizontal(|ui| {
            for (axis, label) in root.translation.iter_mut().zip(["X", "Y", "Z"]) {
                ui.checkbox(axis, label);
            }
            ui.checkbox(&mut root.yaw, "Yaw");
        });
    }
}

fn state_graph(
    ui: &mut egui::Ui,
    animator: &mut Animator,
    positions: &mut BTreeMap<String, [f32; 2]>,
    active_state: Option<usize>,
) {
    ui.small("Click a source state, then a destination to add a transition. Drag states to arrange the graph; the numbered transition priority stays unchanged.");
    let width = ui.available_width().max(1.);
    let columns = (((width - 100.) / 145.).floor() as usize).max(1);
    let height = (80. + animator.states.len().div_ceil(columns) as f32 * 70.).max(180.);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let node_size = egui::vec2(116., 38.);
    let names: std::collections::BTreeSet<_> =
        animator.states.iter().map(|s| s.name.clone()).collect();
    positions.retain(|name, _| names.contains(name));
    for (index, state) in animator.states.iter().enumerate() {
        let point = positions.entry(state.name.clone()).or_insert_with(|| {
            [
                100. + (index % columns) as f32 * 145.,
                20. + (index / columns) as f32 * 70.,
            ]
        });
        point[0] = point[0].clamp(0., (width - node_size.x).max(0.));
        point[1] = point[1].clamp(0., (height - node_size.y).max(0.));
    }
    let node_rect = |name: &str, positions: &BTreeMap<String, [f32; 2]>| {
        let p = positions[name];
        egui::Rect::from_min_size(rect.min + egui::vec2(p[0], p[1]), node_size)
    };
    let wildcard = egui::Rect::from_min_size(rect.min + egui::vec2(8., 20.), egui::vec2(70., 32.));
    let selected_id = ui.make_persistent_id("graph-source");
    let selected = ui.data_mut(|d| d.get_temp::<String>(selected_id));
    let transition_id = ui.make_persistent_id("graph-transition");
    let selected_transition = ui.data_mut(|d| d.get_temp::<usize>(transition_id));
    for (index, transition) in animator.transitions.iter().enumerate() {
        if !names.contains(&transition.to) {
            continue;
        }
        let from = if transition.from == "*" {
            wildcard
        } else if names.contains(&transition.from) {
            node_rect(&transition.from, positions)
        } else {
            continue;
        };
        let to = node_rect(&transition.to, positions);
        let start = from.right_center();
        let end = to.left_center();
        let tint = if selected_transition == Some(index) {
            egui::Color32::YELLOW
        } else {
            egui::Color32::from_rgb(130, 160, 195)
        };
        ui.painter()
            .line_segment([start, end], egui::Stroke::new(1.5, tint));
        let dir = (end - start).normalized();
        if dir.length_sq() > 0. {
            let tip = end - dir * 6.;
            let side = egui::vec2(-dir.y, dir.x) * 4.;
            ui.painter()
                .line_segment([tip - dir * 7. + side, tip], egui::Stroke::new(1.5, tint));
            ui.painter()
                .line_segment([tip - dir * 7. - side, tip], egui::Stroke::new(1.5, tint));
        }
        let badge = egui::Rect::from_center_size(start + (end - start) * 0.5, egui::vec2(23., 23.));
        let response = ui.interact(
            badge,
            ui.id().with(("transition", index)),
            egui::Sense::click(),
        );
        if response.clicked() && ui.is_enabled() {
            ui.data_mut(|d| d.insert_temp(transition_id, index));
        }
        ui.painter()
            .circle_filled(badge.center(), 11., egui::Color32::from_rgb(40, 48, 64));
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            format!("{}", index + 1),
            egui::FontId::monospace(11.),
            tint,
        );
    }
    let mut clicked = None;
    for (index, state) in animator.states.iter().enumerate() {
        let node = node_rect(&state.name, positions);
        let response = ui.interact(
            node,
            ui.id().with(("state", &state.name)),
            egui::Sense::click_and_drag(),
        );
        if response.clicked() && ui.is_enabled() {
            clicked = Some(state.name.clone());
        }
        if response.dragged() && ui.is_enabled() {
            let delta = ui.input(|i| i.pointer.delta());
            let p = positions.get_mut(&state.name).unwrap();
            p[0] = (p[0] + delta.x).clamp(0., (width - node_size.x).max(0.));
            p[1] = (p[1] + delta.y).clamp(0., (height - node_size.y).max(0.));
        }
        let color = if active_state == Some(index) {
            egui::Color32::from_rgb(28, 105, 65)
        } else if selected.as_deref() == Some(&state.name) {
            egui::Color32::from_rgb(75, 72, 120)
        } else {
            egui::Color32::from_rgb(48, 57, 72)
        };
        ui.painter().rect_filled(node, 6., color);
        ui.painter().rect_stroke(
            node,
            6.,
            egui::Stroke::new(
                1.5,
                if state.name == animator.initial {
                    egui::Color32::YELLOW
                } else {
                    egui::Color32::GRAY
                },
            ),
            egui::StrokeKind::Outside,
        );
        ui.painter().text(
            node.center(),
            egui::Align2::CENTER_CENTER,
            &state.name,
            egui::FontId::proportional(12.),
            egui::Color32::WHITE,
        );
    }
    let wildcard_response = ui.interact(wildcard, ui.id().with("wildcard"), egui::Sense::click());
    if wildcard_response.clicked() && ui.is_enabled() {
        clicked = Some("*".into());
    }
    ui.painter()
        .rect_filled(wildcard, 6., egui::Color32::from_rgb(68, 55, 85));
    ui.painter().text(
        wildcard.center(),
        egui::Align2::CENTER_CENTER,
        "Any *",
        egui::FontId::proportional(12.),
        egui::Color32::WHITE,
    );
    if let Some(target) = clicked {
        if let Some(source) = selected.filter(|source| source != &target)
            && target != "*"
            && !animator.parameters.is_empty()
            && animator.transitions.len() < 128
        {
            Arc::make_mut(&mut animator.transitions).push(Transition {
                from: source,
                to: target,
                parameter: animator.parameters.keys().next().unwrap().clone(),
                comparison: Comparison::Above,
                threshold: 0.5,
                fade: 0.2,
                exit_time: None,
            });
            ui.data_mut(|d| d.remove::<String>(selected_id));
            ui.data_mut(|d| d.insert_temp(transition_id, animator.transitions.len() - 1));
        } else {
            ui.data_mut(|d| d.insert_temp(selected_id, target));
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.weak(format!(
            "Initial: {} · * = any state · lower number = higher priority",
            animator.initial
        ));
        if let Some(index) =
            selected_transition.and_then(|i| animator.transitions.get(i).map(|_| i))
        {
            ui.label(format!(
                "Selected #{}: {} → {}",
                index + 1,
                animator.transitions[index].from,
                animator.transitions[index].to
            ));
        }
    });
}
fn movement_preset(ui: &mut egui::Ui, animator: &mut Animator) {
    let find = |word: &str| {
        animator
            .rig
            .clips
            .iter()
            .position(|clip| clip.name.to_lowercase().contains(word))
    };
    let (idle, walk, run) = (find("idle"), find("walk"), find("run"));
    if ui.add_enabled(idle.is_some() && walk.is_some(), egui::Button::new("Create idle / walk / run state"))
        .on_hover_text("Adds a movement state driven by Speed, keeping your existing states. Clips are matched by name.").clicked() {
        let name = (1..).map(|i| format!("Movement {i}")).find(|name| !animator.states.iter().any(|s| &s.name == name)).unwrap();
        let mut samples = vec![
            bozzard_scene::middleware::animation::BlendSample { threshold: 0., clip: idle.unwrap() },
            bozzard_scene::middleware::animation::BlendSample { threshold: 1., clip: walk.unwrap() },
        ];
        if let Some(clip) = run { samples.push(bozzard_scene::middleware::animation::BlendSample { threshold: 2., clip }); }
        animator.parameters.entry("Speed".into()).or_insert(0.);
        Arc::make_mut(&mut animator.states).push(StateDefinition { name: name.clone(), repeat: Repeat::Loop,
            motion: Motion::Blend1d { parameter: "Speed".into(), samples } });
        animator.initial = name;
    }
}
fn bone_picker(
    ui: &mut egui::Ui,
    id: &str,
    bone: &mut Option<usize>,
    rig: &bozzard_scene::middleware::animation::data::Rig,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(
            bone.and_then(|i| rig.nodes.get(i))
                .map_or("Whole skeleton", |b| b.name.as_str()),
        )
        .show_ui(ui, |ui| {
            ui.selectable_value(bone, None, "Whole skeleton");
            for (index, joint) in rig.nodes.iter().enumerate() {
                ui.selectable_value(bone, Some(index), &joint.name);
            }
        });
}
fn required_bone_picker(
    ui: &mut egui::Ui,
    id: &str,
    bone: &mut usize,
    rig: &bozzard_scene::middleware::animation::data::Rig,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(
            rig.nodes
                .get(*bone)
                .map_or("Choose bone", |b| b.name.as_str()),
        )
        .show_ui(ui, |ui| {
            for (index, joint) in rig.nodes.iter().enumerate() {
                ui.selectable_value(bone, index, &joint.name);
            }
        });
}
fn object_picker(ui: &mut egui::Ui, object: &mut String, scene: &bozzard_scene::Scene) {
    egui::ComboBox::from_id_salt("animation-target-object")
        .selected_text(
            scene
                .objects
                .iter()
                .find(|o| &o.id == object)
                .map_or(object.as_str(), |o| o.name.as_str()),
        )
        .show_ui(ui, |ui| {
            for target in &scene.objects {
                ui.selectable_value(object, target.id.clone(), &target.name);
            }
        });
}
fn vector(ui: &mut egui::Ui, label: &str, value: &mut [f32; 3]) {
    ui.label(label);
    ui.horizontal_wrapped(|ui| {
        for (axis, name) in value.iter_mut().zip(["X", "Y", "Z"]) {
            ui.add(
                egui::DragValue::new(axis)
                    .speed(0.01)
                    .prefix(format!("{name} ")),
            );
        }
    });
}
fn clip_picker(
    ui: &mut egui::Ui,
    id: &str,
    clip: &mut usize,
    rig: &bozzard_scene::middleware::animation::data::Rig,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(
            rig.clips
                .get(*clip)
                .map_or("Missing clip", |c| c.name.as_str()),
        )
        .show_ui(ui, |ui| {
            for (i, c) in rig.clips.iter().enumerate() {
                ui.selectable_value(clip, i, &c.name);
            }
        });
}
fn repeat(ui: &mut egui::Ui, value: &mut Repeat) {
    egui::ComboBox::from_id_salt("repeat")
        .selected_text(format!("{value:?}"))
        .show_ui(ui, |ui| {
            for mode in [Repeat::Once, Repeat::Loop, Repeat::PingPong] {
                ui.selectable_value(value, mode, format!("{mode:?}"));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_layout_does_not_reorder_large_controller_transitions() {
        let mut animator = Animator {
            initial: "State 0".into(),
            states: Arc::new(
                (0..64)
                    .map(|i| StateDefinition {
                        name: format!("State {i}"),
                        motion: Motion::Clip { clip: 0 },
                        repeat: Repeat::Loop,
                    })
                    .collect(),
            ),
            transitions: Arc::new(
                (0..128)
                    .map(|i| Transition {
                        from: format!("State {}", i % 64),
                        to: format!("State {}", (i + 1) % 64),
                        parameter: "Speed".into(),
                        comparison: Comparison::Above,
                        threshold: i as f32,
                        fade: 0.2,
                        exit_time: None,
                    })
                    .collect(),
            ),
            ..Default::default()
        };
        let expected = animator.clone();
        let mut positions = BTreeMap::new();
        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            state_graph(ui, &mut animator, &mut positions, None);
        });
        output.textures_delta.clear();
        assert_eq!(positions.len(), 64);
        positions.get_mut("State 0").unwrap()[0] += 10.;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            state_graph(ui, &mut animator, &mut positions, Some(1));
        });
        output.textures_delta.clear();
        assert_eq!(animator, expected);
    }
}
