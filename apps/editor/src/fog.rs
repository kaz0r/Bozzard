use eframe::egui;

pub(super) fn controls(ui: &mut egui::Ui, fog: &mut bozzard_scene::FogSettings) {
    ui.collapsing("Fog (3D)", |ui| {
        ui.checkbox(&mut fog.enabled, "Enable fog");
        ui.add_enabled_ui(fog.enabled, |ui| {
            ui.label("Fog color (linear RGB)");
            egui::color_picker::color_edit_button_rgb(ui, &mut fog.color);
            for (label, value, range, speed) in [
                ("Distance density ", &mut fog.distance_density, 0.0..=1000.0, 0.001),
                ("Start distance ", &mut fog.start_distance, 0.0..=100000.0, 0.1),
                ("Height density ", &mut fog.height_density, 0.0..=1000.0, 0.001),
                ("Base height ", &mut fog.base_height, -100000.0..=100000.0, 0.1),
                ("Height falloff ", &mut fog.height_falloff, 0.0..=1000.0, 0.01),
            ] {
                ui.add(egui::DragValue::new(value).range(range).speed(speed).prefix(label));
            }
            ui.weak("World units from the near plane. Height density decreases above base height; zero falloff is uniform.");
        });
        if ui.button("Reset fog").clicked() {
            *fog = Default::default();
        }
    });
}
