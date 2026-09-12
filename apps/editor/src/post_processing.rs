use super::*;
use bozzard_scene::{DisplayPreset, DisplaySettings, PostProcessVolume, ToneMapper};

fn presets(ui: &mut egui::Ui, display: &mut DisplaySettings) {
    ui.menu_button("Apply preset", |ui| {
        for preset in DisplayPreset::ALL {
            if ui.button(preset.name()).clicked() {
                *display = DisplaySettings::preset(preset);
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(
        "Replace these display settings with a named look. Undo restores the previous settings.",
    );
}
fn slider(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, label: &str) {
    ui.add(egui::Slider::new(value, range).text(label));
}
pub fn controls(ui: &mut egui::Ui, display: &mut DisplaySettings) {
    egui::CollapsingHeader::new("POST PROCESSING")
        .default_open(true)
        .show(ui, |ui| {
            presets(ui, display);
            slider(ui, &mut display.exposure_ev, -16.0..=16.0, "Exposure EV");
            ui.checkbox(&mut display.tone_mapping, "Tone mapping");
            ui.add_enabled_ui(display.tone_mapping, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut display.tone_mapper, ToneMapper::Reinhard, "Reinhard");
                    ui.selectable_value(&mut display.tone_mapper, ToneMapper::Filmic, "Filmic");
                });
            });
            egui::CollapsingHeader::new("Color grading").show(ui, |ui| {
                let grade = &mut display.color_grading;
                slider(ui, &mut grade.temperature, -1.0..=1.0, "Warmth");
                slider(ui, &mut grade.tint, -1.0..=1.0, "Green / magenta");
                slider(ui, &mut grade.saturation, 0.0..=2.0, "Saturation");
                slider(ui, &mut grade.contrast, 0.0..=2.0, "Contrast");
                for (label, color, range) in [
                    ("Lift", &mut grade.lift, -0.25..=0.25),
                    ("Gamma", &mut grade.gamma, 0.25..=4.0),
                    ("Gain", &mut grade.gain, 0.0..=4.0),
                ] {
                    ui.push_id(label, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            for (index, channel) in color.iter_mut().enumerate() {
                                ui.add(
                                    egui::DragValue::new(channel)
                                        .speed(0.005)
                                        .range(range.clone())
                                        .prefix(["R ", "G ", "B "][index]),
                                );
                            }
                        });
                    });
                }
                if ui.button("Reset grading").clicked() {
                    *grade = Default::default();
                }
            });
            egui::CollapsingHeader::new("Bloom & anamorphic streaks").show(ui, |ui| {
                let bloom = &mut display.bloom;
                ui.checkbox(&mut bloom.enabled, "Bloom");
                ui.add_enabled_ui(bloom.enabled, |ui| {
                    ui.add(
                        egui::Slider::new(&mut bloom.intensity, 0.0..=10.0)
                            .logarithmic(true)
                            .text("Glow intensity"),
                    );
                    ui.add(
                        egui::DragValue::new(&mut bloom.threshold)
                            .speed(0.05)
                            .range(0.0..=60000.0)
                            .prefix("Threshold "),
                    );
                    slider(ui, &mut bloom.scatter, 0.0..=1.0, "Spread");
                    slider(ui, &mut bloom.anamorphic, 0.0..=1.0, "Horizontal streaks");
                    ui.weak("Threshold uses scene brightness before exposure.");
                });
            });
            egui::CollapsingHeader::new("Ambient occlusion").show(ui, |ui| {
                let ao = &mut display.ambient_occlusion;
                ui.checkbox(&mut ao.enabled, "Ambient occlusion");
                ui.add_enabled_ui(ao.enabled, |ui| {
                    slider(ui, &mut ao.intensity, 0.0..=3.0, "Strength");
                    slider(ui, &mut ao.radius, 0.01..=10.0, "World radius");
                    slider(ui, &mut ao.bias, 0.0..=0.5, "Surface bias");
                });
            });
            egui::CollapsingHeader::new("Heat shimmer").show(ui, |ui| {
                let heat = &mut display.heat_distortion;
                ui.checkbox(&mut heat.enabled, "Heat distortion");
                ui.add_enabled_ui(heat.enabled, |ui| {
                    slider(ui, &mut heat.strength, 0.0..=30.0, "Displacement");
                    ui.add(
                        egui::DragValue::new(&mut heat.threshold)
                            .speed(0.1)
                            .range(0.0..=60000.0)
                            .prefix("Hot threshold "),
                    );
                    slider(ui, &mut heat.speed, 0.0..=5.0, "Speed");
                    slider(ui, &mut heat.rise, 0.001..=0.3, "Plume height");
                    ui.weak("Bright surfaces drive rising shimmer. Play animates it.");
                });
            });
            egui::CollapsingHeader::new("Volumetric fog & light shafts").show(ui, |ui| {
                fog_controls(ui, &mut display.volumetric_fog);
            });
            egui::CollapsingHeader::new("Film grain & vignette").show(ui, |ui| {
                slider(ui, &mut display.grain.intensity, 0.0..=0.25, "Grain");
                slider(ui, &mut display.grain.size, 1.0..=4.0, "Grain size");
                slider(ui, &mut display.vignette.intensity, 0.0..=1.0, "Vignette");
                slider(ui, &mut display.vignette.roundness, 0.0..=1.0, "Roundness");
                slider(ui, &mut display.vignette.feather, 0.05..=1.0, "Feather");
            });
            if ui.button("Reset display").clicked() {
                *display = Default::default();
            }
        });
}
pub fn volumes(ui: &mut egui::Ui, volumes: &mut Vec<PostProcessVolume>) {
    egui::CollapsingHeader::new("POST-PROCESS VOLUMES").show(ui, |ui| {
        ui.weak("World-space boxes blend into the scene look. Higher priority applies last.");
        let mut remove = None;
        for (index, volume) in volumes.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                egui::CollapsingHeader::new(&volume.name)
                    .id_salt("volume")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut volume.enabled, "Enabled");
                            if ui.button("Remove").clicked() {
                                remove = Some(index);
                            }
                        });
                        ui.text_edit_singleline(&mut volume.name);
                        for (label, vector, range) in [
                            ("Center", &mut volume.center, -1_000_000.0..=1_000_000.0),
                            ("Half size", &mut volume.half_size, 0.01..=100_000.0),
                        ] {
                            ui.push_id(label, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(label);
                                    for (axis, value) in vector.iter_mut().enumerate() {
                                        ui.add(
                                            egui::DragValue::new(value)
                                                .speed(0.1)
                                                .range(range.clone())
                                                .prefix(["X ", "Y ", "Z "][axis]),
                                        );
                                    }
                                });
                            });
                        }
                        ui.add(
                            egui::DragValue::new(&mut volume.blend_distance)
                                .speed(0.1)
                                .range(0.001..=100_000.0)
                                .prefix("Blend distance "),
                        );
                        slider(ui, &mut volume.weight, 0.0..=1.0, "Weight");
                        ui.add(egui::DragValue::new(&mut volume.priority).prefix("Priority "));
                        controls(ui, &mut volume.display);
                    });
            });
        }
        if let Some(index) = remove {
            volumes.remove(index);
        }
        if ui
            .add_enabled(volumes.len() < 32, egui::Button::new("+ Add effect volume"))
            .clicked()
        {
            volumes.push(PostProcessVolume {
                name: format!("Effect volume {}", volumes.len() + 1),
                ..Default::default()
            });
        }
    });
}

fn fog_controls(ui: &mut egui::Ui, fog: &mut bozzard_scene::VolumetricFog) {
    ui.checkbox(&mut fog.enabled, "Volumetric fog");
    ui.add_enabled_ui(fog.enabled, |ui| {
        ui.add(
            egui::Slider::new(&mut fog.density, 0.0..=2.0)
                .logarithmic(true)
                .text("Density"),
        );
        ui.horizontal(|ui| {
            ui.label("Scattering color");
            crate::inspector::color_edit_button_rgb(ui, &mut fog.albedo);
        });
        slider(ui, &mut fog.anisotropy, -0.8..=0.8, "Forward scattering");
        slider(ui, &mut fog.light_intensity, 0.0..=4.0, "Light scattering");
        slider(ui, &mut fog.ambient, 0.0..=1.0, "Ambient scattering");
        ui.add(
            egui::DragValue::new(&mut fog.base_height)
                .speed(0.1)
                .range(-100_000.0..=100_000.0)
                .prefix("Base height "),
        );
        slider(ui, &mut fog.height_falloff, 0.0..=10.0, "Height falloff");
        ui.add(
            egui::Slider::new(&mut fog.max_distance, 1.0..=1000.0)
                .logarithmic(true)
                .text("View distance"),
        );
        fog.start_distance = fog.start_distance.min(fog.max_distance);
        slider(
            ui,
            &mut fog.start_distance,
            0.0..=fog.max_distance,
            "Start distance",
        );
        slider(ui, &mut fog.noise_amount, 0.0..=1.0, "Density variation");
        slider(ui, &mut fog.noise_scale, 0.01..=4.0, "Noise scale");
        ui.horizontal(|ui| {
            ui.label("Wind");
            for (axis, value) in fog.wind.iter_mut().enumerate() {
                ui.add(
                    egui::DragValue::new(value)
                        .speed(0.01)
                        .range(-100.0..=100.0)
                        .prefix(["X ", "Y ", "Z "][axis]),
                );
            }
        });
        ui.add(egui::Slider::new(&mut fog.steps, 16..=96).text("Ray steps"));
        ui.weak("Scene lights illuminate the haze. Enable their shadows to cast light shafts.");
        ui.weak("Play animates the wind.");
    });
}
