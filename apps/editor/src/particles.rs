use super::egui;
use bozzard_scene::{ParticleEmitter, ParticleKind};
pub fn inspector(ui: &mut egui::Ui, emitter: &mut ParticleEmitter) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut emitter.enabled, "Emit particles");
        ui.menu_button("Apply preset", |ui| {
            for kind in ParticleKind::ALL {
                if ui.button(kind.name()).clicked() {
                    *emitter = ParticleEmitter::preset(kind);
                    ui.close();
                }
            }
        });
    });
    ui.label(egui::RichText::new(emitter.kind.name()).strong());
    ui.weak("Live preview is in Effects. Play runs the full scene.");
    ui.add(
        egui::Slider::new(&mut emitter.rate, 0.0..=500.0)
            .logarithmic(true)
            .text("Particles / second"),
    );
    ui.add(egui::Slider::new(&mut emitter.lifetime, 0.1..=30.0).text("Lifetime seconds"));
    ui.add(
        egui::Slider::new(&mut emitter.radius, 0.0..=20.0)
            .logarithmic(true)
            .text("Emission radius"),
    );
    ui.add(
        egui::Slider::new(&mut emitter.start_size, 0.001..=20.0)
            .logarithmic(true)
            .text("Start size"),
    );
    ui.add(
        egui::Slider::new(&mut emitter.end_size, 0.001..=40.0)
            .logarithmic(true)
            .text("End size"),
    );
    ui.horizontal(|ui| {
        ui.label("Color");
        crate::inspector::color_edit_button_rgb(ui, &mut emitter.color);
    });
    ui.add(egui::Slider::new(&mut emitter.opacity, 0.0..=1.0).text("Opacity"));
    ui.horizontal(|ui| {
        ui.label("Wind");
        for (axis, v) in emitter.wind.iter_mut().enumerate() {
            ui.add(
                egui::DragValue::new(v)
                    .speed(0.02)
                    .range(-100.0..=100.0)
                    .prefix(["X ", "Y ", "Z "][axis]),
            );
        }
    });
    ui.add(egui::Slider::new(&mut emitter.turbulence, 0.0..=10.0).text("Curl / turbulence"));
    if emitter.kind == ParticleKind::Sparks {
        ui.add(egui::Slider::new(&mut emitter.trail_length, 0.0..=1.0).text("Trail seconds"));
    }
    egui::CollapsingHeader::new("Advanced particle settings").show(ui, |ui| {
        ui.add(egui::Slider::new(&mut emitter.speed, 0.0..=50.0).text("Launch speed"));
        ui.add(egui::Slider::new(&mut emitter.spread, 0.0..=20.0).text("Spread"));
        ui.add(egui::Slider::new(&mut emitter.gravity, -30.0..=30.0).text("Vertical acceleration"));
        ui.add(egui::Slider::new(&mut emitter.drag, 0.0..=10.0).text("Drag"));
        ui.add(
            egui::Slider::new(&mut emitter.softness, 0.001..=10.0)
                .logarithmic(true)
                .text("Soft intersections"),
        );
        ui.add(egui::Slider::new(&mut emitter.max_particles, 1..=2048).text("Particle budget"));
        ui.add(egui::DragValue::new(&mut emitter.seed).prefix("Random seed "));
    });
}
