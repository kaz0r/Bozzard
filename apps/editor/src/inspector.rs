use super::*;
impl App {
    pub fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("inspector")
            .default_size(300.0)
            .min_size(240.0)
            .max_size(450.0)
            .resizable(true)
            .show(ui, |ui| {
                if self.loading.is_some() { ui.disable(); }
                ui.heading("Inspector");
                let Some(original) = self.editor.selected_object().cloned() else {
                    ui.weak("Click an object in the viewport or select its name in the Hierarchy.");
                    ui.weak("Use the Hierarchy search to find an object, or add a Cube or Sprite there.");
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
                                let mut color = d.color;
                                if ui.color_edit_button_rgb(&mut color).changed() {
                                    d.color = color;
                                }
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
                        let mut box_collider = object.collider.is_some();
                        if ui
                            .checkbox(&mut box_collider, "Box collider (3D)")
                            .changed()
                        {
                            if box_collider {
                                object.collider = Some(bozzard_scene::BoxCollider::default());
                            } else {
                                object.collider = None;
                                if let Some(gravity) = &mut object.gravity {
                                    gravity.enabled = false;
                                }
                            }
                        }
                        if let Some(collider) = &mut object.collider {
                            ui.checkbox(&mut collider.enabled, "Enabled");
                            vector(ui, "Center", &mut collider.center, 0.05);
                            positive_vector(ui, "Size", &mut collider.size, 0.05);
                            ui.weak("Blocks swept box movement. Add Gravity below to make it fall.");
                        }
                        let mut gravity = object.gravity.is_some();
                        if ui.checkbox(&mut gravity, "Gravity").changed() {
                            if gravity {
                                if object.collider.is_none() {
                                    object.collider = Some(bozzard_scene::BoxCollider::default());
                                }
                                object.gravity = Some(bozzard_scene::Gravity::default());
                            } else {
                                object.gravity = None;
                            }
                        }
                        if let Some(gravity) = &mut object.gravity {
                            ui.checkbox(&mut gravity.enabled, "Enabled");
                            positive_number(
                                ui,
                                "Acceleration (m/s²)",
                                &mut gravity.acceleration,
                                0.1,
                            );
                            positive_number(ui, "Max speed (m/s)", &mut gravity.max_speed, 0.5);
                            positive_number(ui, "Jump speed (m/s)", &mut gravity.jump_speed, 0.1);
                            // Continuous-motion estimates; fixed-step integration differs slightly.
                            let acceleration = f64::from(gravity.acceleration);
                            let launch = f64::from(gravity.jump_speed);
                            let fall_limit = f64::from(gravity.max_speed);
                            let height = launch * launch / (2.0 * acceleration);
                            let ascent_time = launch / acceleration;
                            let descent_time = if launch <= fall_limit {
                                ascent_time
                            } else {
                                fall_limit / acceleration
                                    + (height - fall_limit * fall_limit / (2.0 * acceleration))
                                        / fall_limit
                            };
                            ui.weak(format!(
                                "Estimated jump: {height:.2} m high · {:.2} s airtime",
                                ascent_time + descent_time
                            ))
                            .on_hover_text("Assumes enabled gravity, no obstacles, and landing at the starting height. Includes the fall speed limit; fixed-step motion may differ slightly.");
                            if ui.small_button("Reset gravity defaults")
                                .on_hover_text("Reset acceleration, fall speed and jump speed. Keeps Enabled unchanged. Supports Undo.")
                                .clicked()
                            {
                                *gravity = bozzard_scene::Gravity {
                                    enabled: gravity.enabled,
                                    ..Default::default()
                                };
                            }

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
                if let Some(play) = &self.editor.play
                    && let Some(entity) = play.instance.entity(&original.id)
                    && let Some(state) = play.app.world.get::<bozzard_scene::GravityState>(entity)
                    && play
                        .app
                        .world
                        .get::<bozzard_scene::Gravity>(entity)
                        .is_some_and(|g| g.enabled)
                    && play
                        .app
                        .world
                        .get::<bozzard_scene::BoxCollider>(entity)
                        .is_some_and(|c| c.enabled)
                {
                    ui.weak(if state.grounded {
                        "Grounded"
                    } else if state.vertical_velocity > 0.0 {
                        "Rising"
                    } else {
                        "Falling"
                    });
                    ui.weak(format!("Vertical speed: {:+.2} m/s", state.vertical_velocity))
                        .on_hover_text("Positive is upward; negative is downward.");
                }
                if ui.is_enabled() && self.editor.play.is_none()
                    && (object != original || scene.views != self.editor.scene().views) {
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

fn positive_number(ui: &mut egui::Ui, label: &str, value: &mut f32, speed: f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::DragValue::new(value)
                .speed(speed)
                .range(0.0001..=f32::MAX),
        );
    });
}

fn positive_vector(ui: &mut egui::Ui, label: &str, value: &mut [f32; 3], speed: f64) {
    ui.label(label);
    ui.horizontal(|ui| {
        for (index, v) in value.iter_mut().enumerate() {
            ui.add(
                egui::DragValue::new(v)
                    .speed(speed)
                    .prefix(["X ", "Y ", "Z "][index])
                    .range(0.0001..=f32::MAX)
                    .max_decimals(3),
            );
        }
    });
}
