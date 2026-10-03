use super::{bone_picker, motion, required_bone_picker};
use bozzard_scene::middleware::animation::{
    AnimationLayer, Animator, BoneMask, LayerBlend, data::Rig,
};
use eframe::egui;
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn editor(ui: &mut egui::Ui, animator: &mut Animator) {
    egui::CollapsingHeader::new("Body layers").show(ui, |ui| {
        ui.small("Combine actions with movement: select a spine bone for an upper-body action, leaving the legs free to run.");
        let mut remove = None;
        for (index, layer) in Arc::make_mut(&mut animator.layers).iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                egui::CollapsingHeader::new(&layer.name).default_open(true).show(ui, |ui| {
                    layer_editor(ui, layer, &animator.rig, &mut animator.parameters);
                    if ui.button("Remove layer").clicked() { remove = Some(index); }
                });
            });
        }
        if let Some(index) = remove { Arc::make_mut(&mut animator.layers).remove(index); }
        if ui.add_enabled(animator.layers.len() < 16, egui::Button::new("Add body layer")).clicked() {
            let name = (1..).map(|i| format!("Upper body {i}"))
                .find(|name| !animator.layers.iter().any(|l| &l.name == name)).unwrap();
            let root = animator.rig.nodes.iter().position(|bone| bone.name.to_lowercase().contains("spine"));
            Arc::make_mut(&mut animator.layers).push(AnimationLayer {
                name, mask: BoneMask { root, ..Default::default() }, ..Default::default()
            });
        }
    });
}

fn layer_editor(
    ui: &mut egui::Ui,
    layer: &mut AnimationLayer,
    rig: &Rig,
    parameters: &mut BTreeMap<String, f32>,
) {
    ui.text_edit_singleline(&mut layer.name);
    motion::editor(ui, &mut layer.motion, rig, parameters);
    ui.add(egui::Slider::new(&mut layer.weight, 0.0..=1.).text("Weight"));
    ui.add(
        egui::DragValue::new(&mut layer.fade)
            .speed(0.01)
            .range(0.0..=60.)
            .prefix("Fade ")
            .suffix(" s"),
    );
    motion::weight_parameter(ui, &mut layer.weight_parameter, parameters);
    blend_mode(ui, &mut layer.blend);
    ui.label("Affect this bone and its children");
    bone_picker(ui, "mask-root", &mut layer.mask.root, rig);
    mask_weights(ui, &mut layer.mask, rig);
    ui.checkbox(&mut layer.synchronized, "Follow the movement cycle");
    super::repeat(ui, &mut layer.repeat);
    ui.add(
        egui::DragValue::new(&mut layer.speed)
            .speed(0.05)
            .range(0.0..=100.)
            .prefix("Playback speed "),
    );
    if layer.blend == LayerBlend::Additive {
        let mut use_clip = layer.reference_clip.is_some();
        if ui
            .checkbox(
                &mut use_clip,
                "Use a clip's first frame as the additive reference",
            )
            .changed()
        {
            layer.reference_clip = use_clip.then_some(0);
        }
        if let Some(clip) = &mut layer.reference_clip {
            super::clip_picker(ui, "reference-clip", clip, rig);
        }
    }
}

fn blend_mode(ui: &mut egui::Ui, blend: &mut LayerBlend) {
    egui::ComboBox::from_id_salt("layer-blend")
        .selected_text(match blend {
            LayerBlend::Override => "Replace selected bones",
            LayerBlend::Additive => "Add movement to the base pose",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(blend, LayerBlend::Override, "Replace selected bones");
            ui.selectable_value(blend, LayerBlend::Additive, "Add movement to the base pose");
        });
}

fn mask_weights(ui: &mut egui::Ui, mask: &mut BoneMask, rig: &Rig) {
    ui.collapsing("Individual bone weights", |ui| {
        ui.small("Override one bone's influence. Children keep their subtree weights.");
        let mut remove = None;
        for (&bone, weight) in &mut mask.weights {
            ui.push_id(bone, |ui| {
                ui.label(&rig.nodes[bone].name);
                ui.add(egui::Slider::new(weight, 0.0..=1.).text("Influence"));
                if ui.small_button("Remove override").clicked() {
                    remove = Some(bone);
                }
            });
        }
        if let Some(bone) = remove {
            mask.weights.remove(&bone);
        }
        let id = ui.make_persistent_id("new-mask-bone");
        let mut bone = ui.data_mut(|d| d.get_temp::<usize>(id).unwrap_or(0));
        required_bone_picker(ui, "mask-bone", &mut bone, rig);
        if ui
            .add_enabled(
                bone < rig.nodes.len() && !mask.weights.contains_key(&bone),
                egui::Button::new("Add bone override"),
            )
            .clicked()
        {
            mask.weights.insert(bone, 1.);
        }
        ui.data_mut(|d| d.insert_temp(id, bone));
    });
}
