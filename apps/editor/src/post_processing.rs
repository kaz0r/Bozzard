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
    ui.add(
        egui::Slider::new(value, range)
            .clamping(egui::SliderClamping::Edits)
            .text(label),
    );
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
            egui::CollapsingHeader::new("Temporal anti-aliasing").show(ui, |ui| {
                ui.checkbox(&mut display.temporal_aa.enabled, "Smooth edges (TAA)");
                slider(
                    ui,
                    &mut display.temporal_aa.history_weight,
                    0.0..=0.97,
                    "History weight",
                );
                ui.weak(
                    "Higher values stabilize thin detail. Camera cuts reset history automatically.",
                );
            });
            egui::CollapsingHeader::new("Motion blur").show(ui, |ui| {
                ui.checkbox(&mut display.motion_blur.enabled, "Motion blur");
                slider(
                    ui,
                    &mut display.motion_blur.shutter_angle,
                    0.0..=360.0,
                    "Shutter angle",
                );
                slider(
                    ui,
                    &mut display.motion_blur.max_radius,
                    0.0..=128.0,
                    "Maximum pixels at 1080p",
                );
                ui.add(egui::Slider::new(&mut display.motion_blur.samples, 4..=32).text("Samples"));
                ui.weak("Camera and moving meshes contribute. Pause produces a sharp still.");
            });
            egui::CollapsingHeader::new("Screen-space reflections").show(ui, |ui| {
                ui.checkbox(&mut display.reflections.enabled, "Reflections");
                slider(ui, &mut display.reflections.strength, 0.0..=1.0, "Strength");
                slider(
                    ui,
                    &mut display.reflections.max_distance,
                    0.1..=200.0,
                    "Trace distance",
                );
                slider(
                    ui,
                    &mut display.reflections.roughness_cutoff,
                    0.05..=1.0,
                    "Roughness cutoff",
                );
                slider(
                    ui,
                    &mut display.reflections.thickness,
                    0.005..=2.0,
                    "Surface thickness",
                );
                ui.add(
                    egui::Slider::new(&mut display.reflections.steps, 16..=128).text("Ray steps"),
                );
                ui.weak(
                    "Reflects visible surfaces. Rough and off-screen areas retain sky reflections.",
                );
            });
            egui::CollapsingHeader::new("Camera focus & bokeh").show(ui, |ui| {
                lens_controls(ui, &mut display.depth_of_field);
            });
            egui::CollapsingHeader::new("Auto exposure").show(ui, |ui| {
                exposure_controls(ui, &mut display.auto_exposure);
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

fn lens_controls(ui: &mut egui::Ui, lens: &mut bozzard_scene::DepthOfField) {
    ui.checkbox(&mut lens.enabled, "Depth of field");
    ui.add_enabled_ui(lens.enabled, |ui| {
        ui.add(
            egui::Slider::new(&mut lens.focus_distance, 0.5..=1000.0)
                .logarithmic(true)
                .text("Focus distance"),
        );
        slider(ui, &mut lens.focal_length_mm, 10.0..=200.0, "Lens mm");
        ui.add(
            egui::Slider::new(&mut lens.aperture, 0.7..=32.0)
                .logarithmic(true)
                .text("Aperture f-stop"),
        );
        slider(ui, &mut lens.max_blur_radius, 0.0..=32.0, "Maximum blur");
        ui.weak("Lower f-stops and longer lenses give softer backgrounds.");
        ui.weak(
            "Focus distance starts at the camera near plane. Lens mm changes blur, not framing.",
        );
    });
}
fn exposure_controls(ui: &mut egui::Ui, exposure: &mut bozzard_scene::AutoExposure) {
    ui.checkbox(&mut exposure.enabled, "Eye adaptation");
    ui.add_enabled_ui(exposure.enabled, |ui| {
        slider(ui, &mut exposure.strength, 0.0..=1.0, "Adaptation strength");
        slider(
            ui,
            &mut exposure.min_ev,
            -16.0..=exposure.max_ev,
            "Minimum EV",
        );
        slider(
            ui,
            &mut exposure.max_ev,
            exposure.min_ev..=16.0,
            "Maximum EV",
        );
        slider(
            ui,
            &mut exposure.target_gray,
            0.01..=0.5,
            "Target brightness",
        );
        slider(ui, &mut exposure.speed_up, 0.01..=20.0, "Brighten speed");
        slider(ui, &mut exposure.speed_down, 0.01..=20.0, "Darken speed");
        slider(
            ui,
            &mut exposure.center_weight,
            0.0..=1.0,
            "Center weighting",
        );
        ui.weak("Exposure EV adds compensation. Limits preserve the scene's mood.");
        ui.weak("Play adapts gradually; edit previews meter immediately.");
    });
}

impl App {
    pub fn effects_inspector(&mut self, ui: &mut egui::Ui) {
        theme::panel_title(ui, "Effects");
        if self.workspace.layer_2d {
            ui.weak("Effects apply to the 3D view.");
        }
        ui.horizontal(|ui| {
            ui.add_enabled_ui(self.editor.play.is_none(),|ui|{ui.checkbox(&mut self.preview_running,"Live preview");});
            ui.checkbox(&mut self.preview_bypass,"Before").on_hover_text("Compare the base scene with the authored effects. This only changes your viewport.");
        });
        ui.weak("Preview animates particles and atmosphere. Play runs gameplay too.");
        let mut scene = self.editor.scene().clone();
        let original = scene.clone();
        let mut particle = None;
        let mut wet = false;
        let mut volume = false;
        ui.add_enabled_ui(self.editor.play.is_none()&&!self.workspace.layer_2d,|ui| {
            ui.horizontal(|ui| {
                presets(ui,&mut scene.display);
                ui.menu_button("Quality",|ui| {
                    for (name,steps,samples) in [("Performance",32,8),("Balanced",48,12),("High",96,24)] {
                        if ui.button(name).clicked() {
                            scene.display.temporal_aa.enabled=true;
                            scene.display.reflections.steps=steps;
                            scene.display.motion_blur.samples=samples;
                            scene.display.volumetric_fog.steps=steps;
                            ui.close();
                        }
                    }
                }).response.on_hover_text("Set sampling quality and enable temporal anti-aliasing.");
            });
            ui.separator();
            ui.label(egui::RichText::new("Camera").strong());
            ui.checkbox(&mut scene.display.temporal_aa.enabled,"Smooth edges (TAA)");
            ui.checkbox(&mut scene.display.motion_blur.enabled,"Motion blur");
            if scene.display.motion_blur.enabled {slider(ui,&mut scene.display.motion_blur.shutter_angle,0.0..=360.0,"Shutter angle");}
            ui.checkbox(&mut scene.display.depth_of_field.enabled,"Focus & bokeh");
            if scene.display.depth_of_field.enabled {
                ui.add(egui::Slider::new(&mut scene.display.depth_of_field.focus_distance,0.5..=1000.0).logarithmic(true).text("Focus distance"));
                if ui.add_enabled(self.editor.selected.is_some(),egui::Button::new("Focus selected object")).clicked() {
                    let result=(||->Result<f32> {
                        let matrices=scene.global_transforms()?;
                        let selected=self.editor.selected.as_ref().context("Select an object")?;
                        let position=matrices[selected].transform_point3(Vec3::ZERO);
                        let camera=self.workspace.camera.as_ref().map(|c|c.pose()).unwrap_or(matrices[&scene.views[&Layer::ThreeD]]);
                        let forward=-camera.z_axis.truncate().normalize();
                        Ok((position-camera.w_axis.truncate()).dot(forward).clamp(0.5,1000.))
                    })();
                    match result {Ok(distance)=>scene.display.depth_of_field.focus_distance=distance,Err(error)=>self.result(Err(error))}
                }
                slider(ui,&mut scene.display.depth_of_field.aperture,0.7..=32.0,"Aperture f/");
            }
            ui.checkbox(&mut scene.display.auto_exposure.enabled,"Adapt to brightness");
            slider(ui,&mut scene.display.exposure_ev,-4.0..=4.0,"Exposure EV");
            ui.separator();
            ui.label(egui::RichText::new("Light & atmosphere").strong());
            ui.checkbox(&mut scene.display.bloom.enabled,"Bloom & light streaks");
            if scene.display.bloom.enabled {slider(ui,&mut scene.display.bloom.intensity,0.0..=1.0,"Glow");}
            ui.checkbox(&mut scene.display.volumetric_fog.enabled,"Volumetric fog & light shafts");
            if scene.display.volumetric_fog.enabled {ui.add(egui::Slider::new(&mut scene.display.volumetric_fog.density,0.0..=0.3).logarithmic(true).text("Haze density"));}
            ui.checkbox(&mut scene.display.ambient_occlusion.enabled,"Contact shading (AO)");
            ui.checkbox(&mut scene.display.heat_distortion.enabled,"Heat shimmer");
            ui.checkbox(&mut scene.display.reflections.enabled,"Screen-space reflections");
            let has_mesh=self.editor.selected.as_ref().is_some_and(|id|scene.objects.iter().any(|o|&o.id==id&&o.drawable.is_some()));
            wet=ui.add_enabled(has_mesh,egui::Button::new("Make selected surface wet")).on_hover_text("Apply a smooth dielectric material and enable reflections. Undo restores the material.").clicked();
            ui.separator();
            ui.label(egui::RichText::new("Color & film").strong());
            ui.checkbox(&mut scene.display.tone_mapping,"Tone mapping");
            slider(ui,&mut scene.display.color_grading.temperature,-1.0..=1.0,"Warmth");
            slider(ui,&mut scene.display.color_grading.saturation,0.0..=2.0,"Saturation");
            slider(ui,&mut scene.display.grain.intensity,0.0..=0.25,"Film grain");
            slider(ui,&mut scene.display.vignette.intensity,0.0..=1.0,"Vignette");
            ui.separator();
            ui.label(egui::RichText::new("Add particles").strong());
            ui.horizontal_wrapped(|ui| {for kind in bozzard_scene::ParticleKind::ALL {if ui.button(format!("+ {}",kind.name())).clicked(){particle=Some(kind);}}});
            ui.weak("Created at the selection. Tune wind, size and trails in the Inspector.");
            ui.separator();
            egui::CollapsingHeader::new("Fine tuning").show(ui,|ui|{controls(ui,&mut scene.display);});
            volume=ui.add_enabled(scene.post_process_volumes.len()<32,egui::Button::new("+ Effect volume at camera")).clicked();
            volumes(ui,&mut scene.post_process_volumes);
        });
        if scene != original {
            let result = self.editor.apply("Edit effects", scene);
            self.result(result);
        }
        if let Some(kind) = particle {
            let result = self.editor.create_particle_emitter(kind);
            self.result(result);
        }
        if wet {
            let result = self.editor.apply_wet_material();
            self.result(result);
        }
        if volume {
            let position = self
                .workspace
                .camera
                .as_ref()
                .map(|c| c.pose().transform_point3(Vec3::ZERO))
                .unwrap_or(Vec3::ZERO);
            let result = self.editor.create_effect_volume(position);
            self.result(result);
        }
    }
}
