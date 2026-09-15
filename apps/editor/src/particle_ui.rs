use anyhow::Result;
use bozzard_scene::{
    Object,
    middleware::{particle::Modules, registry},
};
use eframe::egui;
use std::sync::Arc;
pub fn component(ui: &mut egui::Ui, object: &mut Object, name: &str) -> Result<()> {
    if name == "particle_emitter" {
        ui.horizontal_wrapped(|ui| {
            ui.label("Apply preset");
            for kind in bozzard_scene::ParticleKind::ALL {
                if ui.small_button(kind.name()).clicked() {
                    object.particle_emitter = Some(bozzard_scene::ParticleEmitter::preset(kind));
                }
            }
        });
        if !object.extras.contains_key("particle_modules")
            && ui.button("Add lifetime curves").clicked()
        {
            registry::set(object, &Modules::default())?;
        }
    } else if name == "particle_modules" {
        let Some(mut modules) = registry::get::<Modules>(object)? else {
            return Ok(());
        };
        let mut changed = false;
        for index in 0..6 {
            let name = [
                "Size multiplier",
                "Opacity multiplier",
                "Red multiplier",
                "Green multiplier",
                "Blue multiplier",
                "Speed multiplier",
            ][index];
            egui::CollapsingHeader::new(name)
                .id_salt(("particle_curve", index))
                .show(ui, |ui| {
                    let source = match index {
                        0 => &modules.curves.size,
                        1 => &modules.curves.opacity,
                        2..=4 => &modules.curves.color[index - 2],
                        _ => &modules.curves.speed,
                    };
                    let mut curve = source.clone();
                    crate::motion_ui::curve_with_limit(ui, &mut curve, 1., 64);
                    for key in &mut curve.keys {
                        key.value = key.value.clamp(0., 8.);
                        key.incoming = key.incoming.clamp(-100., 100.);
                        key.outgoing = key.outgoing.clamp(-100., 100.);
                    }
                    if &curve != source {
                        let curves = Arc::make_mut(&mut modules.curves);
                        match index {
                            0 => curves.size = curve,
                            1 => curves.opacity = curve,
                            2..=4 => curves.color[index - 2] = curve,
                            _ => curves.speed = curve,
                        }
                        changed = true;
                    }
                });
        }
        ui.small("Age 0 is birth; age 1 is the end of each particle's life. Curves multiply the emitter preset. Opacity/color are clamped to 0–1 after multiplication.");
        if changed {
            registry::set(object, &modules)?;
        }
    }
    Ok(())
}
