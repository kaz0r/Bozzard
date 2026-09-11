use super::*;
use glam::Mat4;
impl App {
    pub fn gi_overlay(&self, ui: &egui::Ui, rect: Rect, projection: Mat4) {
        if !self.workspace.gi_visible || self.workspace.layer_2d || self.editor.play.is_some() {
            return;
        }
        let gi = &self.editor.scene().gi;
        let volume = gi.volume;
        if volume.validate().is_err() {
            return;
        }
        let painter = ui.painter().with_clip_rect(rect);
        let current = self.editor.gi_current();
        let color = if current {
            Color32::from_rgb(96, 215, 215)
        } else {
            Color32::from_rgb(235, 177, 95)
        };
        let project = |v: glam::Vec4| {
            let p = v.truncate() / v.w;
            Pos2::new(
                rect.left() + (p.x + 1.) * rect.width() * 0.5,
                rect.top() + (1. - p.y) * rect.height() * 0.5,
            )
        };
        let corner = |i: usize| {
            Vec3::new(
                if i & 1 == 0 {
                    volume.min[0]
                } else {
                    volume.max[0]
                },
                if i & 2 == 0 {
                    volume.min[1]
                } else {
                    volume.max[1]
                },
                if i & 4 == 0 {
                    volume.min[2]
                } else {
                    volume.max[2]
                },
            )
        };
        for i in 0..8 {
            for axis in [1, 2, 4] {
                if i & axis != 0 {
                    continue;
                }
                if let Some((a, b)) = colliders::clip_edge(
                    projection * corner(i).extend(1.),
                    projection * corner(i | axis).extend(1.),
                ) {
                    painter.line_segment([project(a), project(b)], (1.5, color));
                }
            }
        }
        for i in 0..volume.probe_count() {
            let clip = projection * volume.position(i).extend(1.);
            if clip.w <= 0. || !(0.0..=clip.w).contains(&clip.z) {
                continue;
            }
            let point = project(clip);
            if !rect.contains(point) {
                continue;
            }
            let valid = gi
                .baked
                .as_ref()
                .filter(|b| b.volume == volume)
                .is_none_or(|b| b.probes[i * bozzard_scene::GI_PROBE_STRIDE][3] >= 0.5);
            painter.circle_filled(point, 3., Color32::from_black_alpha(150));
            painter.circle_filled(point, 2., if valid { color } else { Color32::GRAY });
        }
    }
}

/// Fit and Bake are commands; all numeric/color edits use the scene's normal history.
pub fn controls(
    ui: &mut egui::Ui,
    gi: &mut bozzard_scene::GiSettings,
    visible: &mut bool,
    current: bool,
    smoke: bool,
) -> (bool, bool) {
    let mut actions = (false, false);
    egui::CollapsingHeader::new("Baked global illumination")
        .open(smoke.then_some(true)).show(ui, |ui| {
            ui.checkbox(&mut gi.enabled, "Enable baked GI");
            ui.checkbox(visible, "Show volume and probes");
            if let Some(baked) = &gi.baked {
                ui.label(if current { "Bake is current" } else { "Bake is out of date — rebuild to apply" });
                ui.weak(format!("{} probes · {:.1} KiB", baked.volume.probe_count(), baked.probes.len() as f32 * 16. / 1024.));
            } else { ui.weak("No bake yet. Fit the volume, then Bake."); }
            ui.horizontal(|ui| {
                actions.0 = ui.button("Fit scene").clicked();
                actions.1 = ui.button("Bake GI").clicked();
                if ui.add_enabled(gi.baked.is_some(), egui::Button::new("Clear bake")).clicked() {
                    gi.baked = None;
                    gi.enabled = false;
                }
            });
            ui.add(egui::Slider::new(&mut gi.intensity, 0.0..=10.0).text("GI intensity"));
            ui.add(egui::Slider::new(&mut gi.normal_bias, 0.0..=1.0).text("Normal bias"));
            inspector::vector(ui, "Volume min", &mut gi.volume.min, 0.1);
            inspector::vector(ui, "Volume max", &mut gi.volume.max, 0.1);
            ui.horizontal(|ui| {
                ui.label("Grid XYZ");
                for n in &mut gi.volume.resolution { ui.add(egui::DragValue::new(n).range(2..=16)); }
            });
            egui::ComboBox::from_id_salt("gi-samples")
                .selected_text(format!("{} rays/probe", gi.volume.samples)).show_ui(ui, |ui| {
                    for n in [64,128,256,512,1024] { ui.selectable_value(&mut gi.volume.samples,n,n.to_string()); }
                });
            ui.add(egui::Slider::new(&mut gi.volume.bounces,1..=4).text("Diffuse bounces"));
            ui.weak("Include receiving surfaces inside the volume. More probes improve detail near walls.");
            ui.weak("Bakes static geometry and lights. Includes diffuse color bounce; no glossy transport. Save stores the bake with the scene.");
        });
    actions
}
