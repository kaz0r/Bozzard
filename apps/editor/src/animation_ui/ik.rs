use super::motion;
use bozzard_scene::{
    Scene,
    middleware::animation::{Animator, FootPlacement, IkConstraint, IkTarget, data::Rig},
};
use eframe::egui;
use std::sync::Arc;

pub(super) fn editor(ui: &mut egui::Ui, animator: &mut Animator, scene: &Scene) {
    egui::CollapsingHeader::new("Feet and inverse kinematics").show(ui, |ui| {
        ui.small("Keep feet on stairs and slopes, or guide a hand to a point. Choose upper limb → lower limb → foot/hand.");
        let mut remove = None;
        for (index, constraint) in Arc::make_mut(&mut animator.ik).iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                egui::CollapsingHeader::new(&constraint.name).default_open(true).show(ui, |ui| {
                    ui.text_edit_singleline(&mut constraint.name);
                    chain_picker(ui, constraint, &animator.rig);
                    ui.add(egui::Slider::new(&mut constraint.weight, 0.0..=1.).text("Weight"));
                    motion::weight_parameter(ui, &mut constraint.weight_parameter, &animator.parameters);
                    target(ui, &mut constraint.target, scene);
                    ui.add(egui::DragValue::new(&mut constraint.smoothing).speed(0.5).range(0.0..=100.).prefix("Contact smoothing "));
                    ui.collapsing("Bend direction", |ui| {
                        for (value, name) in constraint.pole.iter_mut().zip(["X", "Y", "Z"]) {
                            ui.add(egui::DragValue::new(value).speed(0.05).prefix(format!("{name} ")));
                        }
                        ui.small("Use the character's forward direction for knees; reverse it for elbows.");
                    });
                    if ui.button("Remove IK chain").clicked() { remove = Some(index); }
                });
            });
        }
        if let Some(index) = remove { Arc::make_mut(&mut animator.ik).remove(index); }
        let first = first_chain(&animator.rig, None);
        if ui.add_enabled(animator.ik.len() < 16 && first.is_some(), egui::Button::new("Add IK chain")).clicked()
            && let Some((root, middle, tip)) = first {
            let name = (1..).map(|i| format!("IK chain {i}")).find(|name| !animator.ik.iter().any(|c| &c.name == name)).unwrap();
            Arc::make_mut(&mut animator.ik).push(IkConstraint { name, root, middle, tip, ..Default::default() });
        }
        let feet: Vec<_> = ["left", "right"].into_iter().filter_map(|side| {
            foot_chain(&animator.rig, side).map(|chain| (side, chain))
        }).filter(|(_, (_, _, tip))| !animator.ik.iter().any(|chain| chain.tip == *tip)).collect();
        if ui.add_enabled(!feet.is_empty() && animator.ik.len() + feet.len() <= 16, egui::Button::new("Set up humanoid feet"))
            .on_hover_text("Matches left/right upper-leg or thigh bones and their children.").clicked() {
            for (side, (root, middle, tip)) in feet {
                    Arc::make_mut(&mut animator.ik).push(IkConstraint { name: format!("{side} foot"),
                        root, middle, tip, ..Default::default() });
            }
            if animator.foot_placement.is_none() && let Some(pelvis) = animator.rig.nodes.iter()
                .position(|b| matches!(b.name.to_lowercase().as_str(), "hips" | "pelvis")) {
                animator.foot_placement = Some(FootPlacement { pelvis, ..Default::default() });
            }
        }
        pelvis(ui, animator);
    });
}
fn pelvis(ui: &mut egui::Ui, animator: &mut Animator) {
    let mut enabled = animator.foot_placement.is_some();
    if ui
        .checkbox(&mut enabled, "Adjust body height to reach the ground")
        .changed()
    {
        animator.foot_placement = enabled.then(|| FootPlacement {
            pelvis: animator
                .rig
                .nodes
                .iter()
                .position(|b| b.name.eq_ignore_ascii_case("hips"))
                .unwrap_or(0),
            ..Default::default()
        });
    }
    if let Some(settings) = &mut animator.foot_placement {
        select_bone(ui, "Pelvis", &mut settings.pelvis, &animator.rig, |_| true);
        for (label, value) in [
            ("Raise at most ", &mut settings.max_up),
            ("Lower at most ", &mut settings.max_down),
        ] {
            ui.add(
                egui::DragValue::new(value)
                    .speed(0.01)
                    .range(0.0..=2.)
                    .prefix(label)
                    .suffix(" m"),
            );
        }
        ui.add(
            egui::DragValue::new(&mut settings.smoothing)
                .speed(0.5)
                .range(0.0..=100.)
                .prefix("Body smoothing "),
        );
    }
}
fn first_chain(rig: &Rig, root: Option<usize>) -> Option<(usize, usize, usize)> {
    rig.nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| root.is_none_or(|root| *index == root))
        .find_map(|(root, _)| {
            rig.nodes
                .iter()
                .enumerate()
                .filter(|(_, b)| b.parent == Some(root as u32))
                .find_map(|(middle, _)| {
                    rig.nodes
                        .iter()
                        .position(|b| b.parent == Some(middle as u32))
                        .map(|tip| (root, middle, tip))
                })
        })
}
fn foot_chain(rig: &Rig, side: &str) -> Option<(usize, usize, usize)> {
    let prefix = if side == "left" { "l_" } else { "r_" };
    rig.nodes.iter().enumerate().find_map(|(root, bone)| {
        let name = bone.name.to_lowercase();
        ((name.contains(side) || name.starts_with(prefix))
            && (name.contains("upperleg") || name.contains("upleg") || name.contains("thigh")))
        .then(|| first_chain(rig, Some(root)))
        .flatten()
    })
}
fn chain_picker(ui: &mut egui::Ui, chain: &mut IkConstraint, rig: &Rig) {
    let old = chain.root;
    select_bone(ui, "Upper limb", &mut chain.root, rig, |i| {
        first_chain(rig, Some(i)).is_some()
    });
    if chain.root != old {
        let (_, middle, tip) = first_chain(rig, Some(chain.root)).unwrap();
        chain.middle = middle;
        chain.tip = tip;
    }
    let old = chain.middle;
    select_bone(ui, "Lower limb", &mut chain.middle, rig, |i| {
        rig.nodes[i].parent == Some(chain.root as u32)
            && rig.nodes.iter().any(|b| b.parent == Some(i as u32))
    });
    if chain.middle != old {
        chain.tip = rig
            .nodes
            .iter()
            .position(|b| b.parent == Some(chain.middle as u32))
            .unwrap();
    }
    select_bone(ui, "Foot / hand", &mut chain.tip, rig, |i| {
        rig.nodes[i].parent == Some(chain.middle as u32)
    });
}
fn select_bone(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut usize,
    rig: &Rig,
    eligible: impl Fn(usize) -> bool,
) {
    ui.label(label);
    egui::ComboBox::from_id_salt(label)
        .selected_text(
            rig.nodes
                .get(*value)
                .map_or("Choose bone", |b| b.name.as_str()),
        )
        .show_ui(ui, |ui| {
            for (i, bone) in rig.nodes.iter().enumerate().filter(|(i, _)| eligible(*i)) {
                ui.selectable_value(value, i, &bone.name);
            }
        });
}
fn target(ui: &mut egui::Ui, target: &mut IkTarget, scene: &Scene) {
    let mut kind = match target {
        IkTarget::Ground { .. } => 0,
        IkTarget::Point { .. } => 1,
        IkTarget::Object { .. } => 2,
    };
    let before = kind;
    egui::ComboBox::from_id_salt("ik-target-kind")
        .selected_text(["Ground beneath the foot", "World position", "Scene object"][kind])
        .show_ui(ui, |ui| {
            for (index, label) in ["Ground beneath the foot", "World position", "Scene object"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut kind, index, *label);
            }
        });
    if kind != before {
        *target = match kind {
            1 => IkTarget::Point { position: [0.; 3] },
            2 => IkTarget::Object {
                object: scene
                    .objects
                    .first()
                    .map_or_else(String::new, |o| o.id.clone()),
                offset: [0.; 3],
            },
            _ => IkConstraint::default().target,
        };
    }
    match target {
        IkTarget::Ground {
            sole_height,
            ray_up,
            ray_down,
            release_height,
            plant,
            align_normal,
            ..
        } => {
            for (label, value) in [
                ("Sole height", sole_height),
                ("Probe above", ray_up),
                ("Probe below", ray_down),
                ("Release when lifted", release_height),
            ] {
                ui.add(
                    egui::DragValue::new(value)
                        .speed(0.01)
                        .range(0.001..=10.)
                        .prefix(format!("{label} "))
                        .suffix(" m"),
                );
            }
            ui.checkbox(plant, "Hold planted feet in place");
            ui.checkbox(align_normal, "Align soles with the surface");
        }
        IkTarget::Point { position } => super::vector(ui, "Position", position),
        IkTarget::Object { object, offset } => {
            super::object_picker(ui, object, scene);
            super::vector(ui, "Offset", offset);
        }
    }
}
