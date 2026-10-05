use bozzard_scene::{
    Scene,
    middleware::{
        animation::{Animator, MotionWarp, WarpTarget},
        curve::Repeat,
    },
};
use eframe::egui;
use std::sync::Arc;

pub(super) fn editor(ui: &mut egui::Ui, animator: &mut Animator, scene: &Scene) {
    ui.collapsing("Align root motion with a target", |ui| {
        ui.small("Use this for reaching a ledge, sitting, or approaching an interaction. The selected state must play once and root motion must be enabled.");
        let mut remove = None;
        for (index, window) in Arc::make_mut(&mut animator.warps).iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                egui::CollapsingHeader::new(&window.name).default_open(true).show(ui, |ui| {
                    ui.text_edit_singleline(&mut window.name);
                    egui::ComboBox::from_id_salt("warp-state").selected_text(&window.state).show_ui(ui, |ui| {
                        for state in animator.states.iter().filter(|s| s.repeat == Repeat::Once) {
                            ui.selectable_value(&mut window.state, state.name.clone(), &state.name);
                        }
                    });
                    ui.add(egui::Slider::new(&mut window.start, 0.0..=1.).text("Window starts"));
                    ui.add(egui::Slider::new(&mut window.end, 0.0..=1.).text("Window ends"));
                    ui.horizontal_wrapped(|ui| {
                        for (axis, label) in window.translation.iter_mut().zip(["Position X", "Position Y", "Position Z"]) {
                            ui.checkbox(axis, label);
                        }
                        ui.checkbox(&mut window.yaw, "Facing direction");
                    });
                    target(ui, &mut window.target, scene);
                    if ui.button("Remove alignment window").clicked() { remove = Some(index); }
                });
            });
        }
        if let Some(index) = remove { Arc::make_mut(&mut animator.warps).remove(index); }
        let state = animator.states.iter().find(|s| s.repeat == Repeat::Once);
        if ui.add_enabled(animator.root_motion.is_some() && state.is_some() && animator.warps.len() < 64,
            egui::Button::new("Add alignment window")).clicked() {
            let name = (1..).map(|i| format!("Align {i}")).find(|name| !animator.warps.iter().any(|w| &w.name == name)).unwrap();
            Arc::make_mut(&mut animator.warps).push(MotionWarp { name, state: state.unwrap().name.clone(),
                start: 0., end: 1., translation: [true, false, true], yaw: true,
                target: WarpTarget::Point { position: [0.; 3], yaw_degrees: 0. } });
        }
    });
}
fn target(ui: &mut egui::Ui, target: &mut WarpTarget, scene: &Scene) {
    let mut object = matches!(target, WarpTarget::Object { .. });
    if ui
        .checkbox(&mut object, "Use a scene object as the target")
        .changed()
    {
        *target = if object {
            WarpTarget::Object {
                object: scene
                    .objects
                    .first()
                    .map_or_else(String::new, |o| o.id.clone()),
                offset: [0.; 3],
                yaw_degrees: 0.,
            }
        } else {
            WarpTarget::Point {
                position: [0.; 3],
                yaw_degrees: 0.,
            }
        };
    }
    match target {
        WarpTarget::Point {
            position,
            yaw_degrees,
        } => {
            super::vector(ui, "World position", position);
            ui.add(
                egui::DragValue::new(yaw_degrees)
                    .speed(1.)
                    .suffix("° facing"),
            );
        }
        WarpTarget::Object {
            object,
            offset,
            yaw_degrees,
        } => {
            super::object_picker(ui, object, scene);
            super::vector(ui, "Local offset", offset);
            ui.add(
                egui::DragValue::new(yaw_degrees)
                    .speed(1.)
                    .suffix("° facing offset"),
            );
        }
    }
}
