use super::*;
impl App {
    pub fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .default_size(300.0)
            .min_size(240.0)
            .max_size(450.0)
            .resizable(true)
            .show(ui, |ui| {
                ui.heading("Inspector");
                let Some(original) = self.editor.selected_object().cloned() else {
                    ui.weak("Select an object in the scene or hierarchy.");
                    return;
                };
                let mut object = original.clone();
                let mut scene = self.editor.scene().clone();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut object.name);
                        ui.weak(format!("ID: {}", object.id));
                        egui::ComboBox::from_id_salt("parent")
                            .selected_text(object.parent.as_deref().unwrap_or("Root"))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut object.parent, None, "Root");
                                for candidate in &scene.objects {
                                    if candidate.id != object.id {
                                        ui.selectable_value(
                                            &mut object.parent,
                                            Some(candidate.id.clone()),
                                            &candidate.name,
                                        );
                                    }
                                }
                            });
                        ui.separator();
                        ui.strong("Transform");
                        vector(ui, "Position", &mut object.transform.translation, 0.05);
                        vector(
                            ui,
                            "Rotation °",
                            &mut object.transform.rotation_degrees,
                            0.5,
                        );
                        vector(ui, "Scale", &mut object.transform.scale, 0.02);
                        if ui.button("Reset transform").clicked() {
                            object.transform = Transform::default();
                        }
                        ui.separator();
                        let mut drawable = object.drawable.is_some();
                        if ui.checkbox(&mut drawable, "Renderable").changed() {
                            object.drawable = if drawable {
                                Some(bozzard_scene::Drawable {
                                    layer: self.layer(),
                                    mesh: Mesh::Cube,
                                    texture: Texture::White,
                                    color: [0.25, 0.8, 0.7],
                                    uv_scale: [1.0; 2],
                                })
                            } else {
                                None
                            };
                        }
                        if let Some(d) = &mut object.drawable {
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut d.layer, Layer::TwoD, "2D");
                                ui.selectable_value(&mut d.layer, Layer::ThreeD, "3D");
                            });
                            egui::ComboBox::from_id_salt("mesh")
                                .selected_text(match &d.mesh {
                                    Mesh::Cube => "Cube",
                                    Mesh::Quad => "Quad",
                                    Mesh::Asset(id) => id,
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut d.mesh, Mesh::Cube, "Cube");
                                    ui.selectable_value(&mut d.mesh, Mesh::Quad, "Quad");
                                    for (id, asset) in &scene.assets {
                                        if asset.kind == AssetKind::Mesh {
                                            ui.selectable_value(
                                                &mut d.mesh,
                                                Mesh::Asset(id.clone()),
                                                id,
                                            );
                                        }
                                    }
                                });
                            egui::ComboBox::from_id_salt("texture")
                                .selected_text(match &d.texture {
                                    Texture::White => "White",
                                    Texture::Checker => "Checker",
                                    Texture::Asset(id) => id,
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut d.texture, Texture::White, "White");
                                    ui.selectable_value(
                                        &mut d.texture,
                                        Texture::Checker,
                                        "Checker",
                                    );
                                    for (id, asset) in &scene.assets {
                                        if asset.kind == AssetKind::Image {
                                            ui.selectable_value(
                                                &mut d.texture,
                                                Texture::Asset(id.clone()),
                                                id,
                                            );
                                        }
                                    }
                                });
                            ui.horizontal(|ui| {
                                ui.label("Tint");
                                ui.color_edit_button_rgb(&mut d.color);
                            });
                            ui.horizontal(|ui| {
                                ui.label("UV repeat");
                                for value in &mut d.uv_scale {
                                    ui.add(
                                        egui::DragValue::new(value)
                                            .speed(0.05)
                                            .range(0.001..=1000.0),
                                    );
                                }
                            });
                        }
                        ui.separator();
                        let mut spin = object.spin.is_some();
                        if ui.checkbox(&mut spin, "Spin behavior").changed() {
                            object.spin = spin.then_some(Spin([0.0, 45.0, 0.0]));
                        }
                        if let Some(spin) = &mut object.spin {
                            vector(ui, "Degrees/sec", &mut spin.0, 0.5);
                            ui.weak("Runs in Play mode only");
                        }
                        ui.separator();
                        let mut camera = object.camera.is_some();
                        if ui.checkbox(&mut camera, "Camera").changed() {
                            object.camera = if camera {
                                Some(Camera::Perspective {
                                    vertical_fov_degrees: 60.0,
                                    near: 0.1,
                                    far: 1000.0,
                                })
                            } else {
                                None
                            };
                        }
                        if let Some(camera) = &mut object.camera {
                            let mut orthographic = matches!(camera, Camera::Orthographic { .. });
                            if ui.checkbox(&mut orthographic, "Orthographic").changed() {
                                *camera = if orthographic {
                                    Camera::Orthographic {
                                        vertical_size: 7.0,
                                        near: 0.1,
                                        far: 1000.0,
                                    }
                                } else {
                                    Camera::Perspective {
                                        vertical_fov_degrees: 60.0,
                                        near: 0.1,
                                        far: 1000.0,
                                    }
                                };
                            }
                            match camera {
                                Camera::Orthographic {
                                    vertical_size,
                                    near,
                                    far,
                                } => {
                                    number(ui, "Vertical size", vertical_size, 0.1);
                                    number(ui, "Near", near, 0.01);
                                    number(ui, "Far", far, 1.0);
                                }
                                Camera::Perspective {
                                    vertical_fov_degrees,
                                    near,
                                    far,
                                } => {
                                    number(ui, "Vertical FOV", vertical_fov_degrees, 0.2);
                                    number(ui, "Near", near, 0.01);
                                    number(ui, "Far", far, 1.0);
                                }
                            }
                            ui.horizontal(|ui| {
                                for (layer, label) in
                                    [(Layer::TwoD, "Use for 2D"), (Layer::ThreeD, "Use for 3D")]
                                {
                                    if ui.button(label).clicked() {
                                        scene.views.insert(layer, object.id.clone());
                                    }
                                }
                            });
                        }
                    });
                });
                if object != original || scene.views != self.editor.scene().views {
                    self.editor.begin_gesture("Edit component");
                    if let Some(slot) = scene.objects.iter_mut().find(|o| o.id == object.id) {
                        *slot = object;
                    }
                    let r = self.editor.apply("Edit component", scene);
                    self.result(r);
                }
            });
    }
}
fn vector(ui: &mut egui::Ui, label: &str, value: &mut [f32; 3], speed: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        for (index, v) in value.iter_mut().enumerate() {
            ui.add(
                egui::DragValue::new(v)
                    .speed(speed)
                    .prefix(["X ", "Y ", "Z "][index])
                    .max_decimals(3),
            );
        }
    });
}
fn number(ui: &mut egui::Ui, label: &str, value: &mut f32, speed: f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).speed(speed));
    });
}
