use super::*;
impl App {
    pub fn inspector(&mut self, ui: &mut egui::Ui) {
        if self.loading.is_some() {
            ui.disable();
        }
        theme::panel_title(ui, "Properties");
        self.prefab_inspector(ui);
        let Some(original) = self.editor.selected_object().cloned() else {
            ui.add_space(16.0);
            ui.weak("No entity selected");
            ui.small("Select in the viewport or Scene Hierarchy to inspect components.");
            return;
        };
        let mut object = original.clone();
        let mut scene = self.editor.scene().clone();
        let checkpoint_start = checkpoint_respawn(&scene, &original);
        ui.push_id(&original.id, |ui| {
        egui::ScrollArea::vertical().id_salt("entity-properties").show(ui, |ui| {
                    if self.surface_inspector(ui) { return; }
                    self.blueprint_inspector(ui, &mut object);
                    ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                        ui.add(egui::TextEdit::singleline(&mut object.name).desired_width(f32::INFINITY))
                            .on_hover_text(format!("Entity name · ID: {}", object.id));
                        ui.small("Parent");
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
                        egui::CollapsingHeader::new("TRANSFORM").default_open(true).show(ui, |ui| {
                        vector(ui, "Position", &mut object.transform.translation, 0.05);
                        vector(
                            ui,
                            "Rotation °",
                            &mut object.transform.rotation_degrees,
                            0.5,
                        );
                        vector(ui, "Scale", &mut object.transform.scale, 0.02);
                        if ui.small_button("Reset transform").clicked() {
                            object.transform = Transform::default();
                        }
                        });
                        ui.separator();
                        egui::CollapsingHeader::new("LIGHT").default_open(original.light.is_some()).show(ui, |ui| {
                            crate::lights::inspector(ui, &mut object.light);
                        });
                        ui.separator();
                        egui::CollapsingHeader::new("MESH RENDERER").default_open(original.drawable.is_some()).show(ui, |ui| {
                        let mut drawable = object.drawable.is_some();
                        if ui.checkbox(&mut drawable, "Renderable").changed() {
                            object.drawable = if drawable {
                                Some(bozzard_scene::Drawable {
                                    gi_static: true,
                                    material_overrides: Vec::new(),
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
                            ui.checkbox(&mut d.gi_static,"Contribute to GI bake (static)").on_hover_text("Moving components and their descendants are excluded automatically. Objects can still receive baked light.");
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut d.layer, Layer::TwoD, "2D");
                                ui.selectable_value(&mut d.layer, Layer::ThreeD, "3D");
                            });
                            let original_mesh = d.mesh.clone();
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
                            if d.mesh != original_mesh { d.material_overrides.clear(); }
                            ui.label("Texture / material effect");
                            texture_control(ui, &mut d.texture, &scene);
                            ui.horizontal(|ui| {
                                ui.label("Tint");
                                let mut color = d.color;
                                if color_edit_button_rgb(ui, &mut color).changed() {
                                    d.color = color;
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("UV repeat").on_hover_text("Also controls procedural checker density; tint colors checker and toon effects.");
                                for value in &mut d.uv_scale {
                                    ui.add(
                                        egui::DragValue::new(value)
                                            .speed(0.05)
                                            .range(0.001..=1000.0),
                                    );
                                }
                            });
                        }
                        });
                        ui.separator();
                        egui::CollapsingHeader::new("PHYSICS").default_open(original.collider.is_some()).show(ui, |ui| {
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
                        });
                        ui.separator();
                        egui::CollapsingHeader::new("PLAYER & TRIGGERS").default_open(original.player_controller.is_some() || original.trigger.is_some()).show(ui, |ui| {
                        let mut controller = object.player_controller.is_some();
                        if ui.checkbox(&mut controller, "Player Controller").changed() {
                            object.player_controller = controller.then(|| bozzard_scene::PlayerController {
                                camera: scene.views.get(&Layer::ThreeD).cloned().unwrap_or_default(),
                                ..Default::default()
                            });
                            if controller {
                                object.collider.get_or_insert_with(Default::default).enabled = true;
                                object.gravity.get_or_insert_with(Default::default).enabled = true;
                            }
                        }
                        if let Some(config) = &mut object.player_controller {
                            ui.weak("One root player per scene. Enabled collider + Gravity required. Selection does not control gameplay.");
                            egui::ComboBox::from_id_salt("follow-camera")
                                .selected_text(&config.camera).show_ui(ui, |ui| {
                                    for candidate in scene.objects.iter().filter(|o| eligible_follow_camera(o)) {
                                        if ui.selectable_value(&mut config.camera, candidate.id.clone(), &candidate.name).changed() {
                                            scene.views.insert(Layer::ThreeD, candidate.id.clone());
                                        }
                                    }
                                });
                            ui.weak("Choosing a root perspective camera also activates it for 3D (one Undo).");
                            positive_number(ui, "Move speed", &mut config.move_speed, 0.1);
                            positive_number(ui, "Controller jump speed", &mut config.jump_speed, 0.1);
                            positive_number(ui, "Follow distance", &mut config.camera_distance, 0.1);
                            positive_number(ui, "Follow height", &mut config.camera_height, 0.1);
                            positive_number(ui, "Camera clearance", &mut config.camera_radius, 0.05);
                            positive_number(ui, "Orbit sensitivity", &mut config.orbit_sensitivity, 0.01);
                            ui.horizontal(|ui| { ui.label("Fall / respawn Y"); ui.add(egui::DragValue::new(&mut config.fall_height).speed(0.1)); });
                            ui.weak("Controller jump speed overrides Gravity's legacy selected-box jump speed.");
                        }
                        let mut trigger = object.trigger.is_some();
                        if ui.checkbox(&mut trigger, "Trigger volume").changed() {
                            object.trigger = trigger.then(bozzard_scene::Trigger::default);
                            if trigger { object.collider = None; object.gravity = None; object.player_controller = None; }
                        }
                        if let Some(trigger) = &mut object.trigger {
                            ui.checkbox(&mut trigger.volume.enabled, "Trigger enabled");
                            vector(ui, "Trigger center", &mut trigger.volume.center, 0.05);
                            positive_vector(ui, "Trigger size", &mut trigger.volume.size, 0.05);
                            use bozzard_scene::TriggerAction;
                            egui::ComboBox::from_id_salt("trigger-action")
                                .selected_text(match trigger.action { TriggerAction::Sensor => "Sensor (Blueprints)", TriggerAction::Collectible => "Collectible", TriggerAction::Checkpoint { .. } => "Checkpoint", TriggerAction::Goal => "Goal" })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut trigger.action, TriggerAction::Sensor, "Sensor (Blueprints)");
                                    ui.selectable_value(&mut trigger.action, TriggerAction::Collectible, "Collectible");
                                    let checkpoint = checkpoint_action(&trigger.action, checkpoint_start);
                                    ui.selectable_value(&mut trigger.action, checkpoint, "Checkpoint");
                                    ui.selectable_value(&mut trigger.action, TriggerAction::Goal, "Goal (all collectibles)");
                                });
                            if let TriggerAction::Checkpoint { respawn } = &mut trigger.action {
                                vector(ui, "Respawn (world)", respawn, 0.1);
                                ui.weak("Place above a safe floor, clear of solids and above Fall Y.");
                            }
                            ui.weak("Non-solid box. Collectibles hide once per run; progress survives falls, resets on Stop / Play or player R.");
                        }
                        });
                        ui.separator();
                        egui::CollapsingHeader::new("BEHAVIOR").default_open(original.spin.is_some()).show(ui, |ui| {
                        let mut spin = object.spin.is_some();
                        if ui.checkbox(&mut spin, "Spin behavior").changed() {
                            object.spin = spin.then_some(Spin([0.0, 45.0, 0.0]));
                        }
                        if let Some(spin) = &mut object.spin {
                            vector(ui, "Degrees/sec", &mut spin.0, 0.5);
                            ui.weak("Runs in Play mode only");
                        }
                        });
                        ui.separator();
                        egui::CollapsingHeader::new("CAMERA").default_open(original.camera.is_some()).show(ui, |ui| {
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
                                    let eligible = layer != Layer::ThreeD
                                        || !scene.objects.iter().any(|o| o.player_controller.is_some())
                                        || eligible_follow_camera(&object);
                                    if ui.add_enabled(eligible, egui::Button::new(label)).clicked() {
                                        scene.views.insert(layer, object.id.clone());
                                    }
                                }
                            });
                        }
                        });
                    });
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
            ui.weak(format!(
                "Vertical speed: {:+.2} m/s",
                state.vertical_velocity
            ))
            .on_hover_text("Positive is upward; negative is downward.");
        }
        if ui.is_enabled()
            && self.editor.play.is_none()
            && (object != original || scene.views != self.editor.scene().views)
        {
            self.editor.begin_gesture("Edit component");
            if let Some(slot) = scene.objects.iter_mut().find(|o| o.id == object.id) {
                *slot = object;
            }
            synchronize_follow_camera(&mut scene);
            let r = self.editor.apply("Edit component", scene);
            self.result(r);
        }
    }
    pub fn lighting_inspector(&mut self, ui: &mut egui::Ui) {
        let mut scene = self.editor.scene().clone();
        let mut bake_gi = false;
        let mut fit_gi = false;
        let gi_current = self.editor.gi_current();
        ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
            egui::CollapsingHeader::new("GLOBAL ILLUMINATION")
                .open(if self.smoke_prefab_frame.is_some() {
                    Some(false)
                } else {
                    self.smoke_gi_frame.map(|_| true)
                })
                .show(ui, |ui| {
                    let actions = gi::controls(
                        ui,
                        &mut scene.gi,
                        &mut self.workspace.gi_visible,
                        gi_current,
                        self.smoke_gi_frame.is_some(),
                    );
                    fit_gi = actions.0;
                    bake_gi = actions.1;
                });
            ui.separator();
            let light = &mut scene.lighting;
            egui::CollapsingHeader::new("DIRECTIONAL LIGHT")
                .default_open(true)
                .show(ui, |ui| {
                    let direction = Vec3::from(light.sun_direction).normalize();
                    let mut azimuth = direction.z.atan2(direction.x).to_degrees();
                    let mut elevation = direction.y.clamp(-1., 1.).asin().to_degrees();
                    let changed = ui
                        .add(egui::Slider::new(&mut azimuth, -180.0..=180.0).text("Sun azimuth °"))
                        .changed();
                    let changed = ui
                        .add(
                            egui::Slider::new(&mut elevation, -90.0..=90.0).text("Sun elevation °"),
                        )
                        .changed()
                        || changed;
                    if changed {
                        let a = azimuth.to_radians();
                        let e = elevation.to_radians();
                        light.sun_direction = [e.cos() * a.cos(), e.sin(), e.cos() * a.sin()];
                    }
                    ui.label("Sun color (linear RGB)");
                    color_edit_button_rgb(ui, &mut light.sun_color);
                    ui.add(
                        egui::DragValue::new(&mut light.sun_intensity)
                            .speed(0.05)
                            .range(0.0..=100000.0)
                            .prefix("Sun intensity "),
                    );
                    ui.label("Ambient color (linear RGB)");
                    color_edit_button_rgb(ui, &mut light.ambient_color);
                    ui.add(
                        egui::DragValue::new(&mut light.ambient_intensity)
                            .speed(0.005)
                            .range(0.0..=100000.0)
                            .prefix("Ambient intensity "),
                    );
                    ui.checkbox(&mut light.shadows, "Sun shadows");
                    egui::ComboBox::from_id_salt("shadow-resolution")
                        .selected_text(format!("{} px", light.shadow_resolution))
                        .show_ui(ui, |ui| {
                            for resolution in [512, 1024, 2048, 4096] {
                                ui.selectable_value(
                                    &mut light.shadow_resolution,
                                    resolution,
                                    format!("{resolution} px"),
                                );
                            }
                        });
                    ui.add(
                        egui::DragValue::new(&mut light.shadow_bias)
                            .speed(0.001)
                            .range(0.0..=1.0)
                            .prefix("Depth bias "),
                    )
                    .on_hover_text(
                        "World units. Increase only enough to remove surface shadow speckling.",
                    );
                    ui.add(
                        egui::DragValue::new(&mut light.shadow_normal_bias)
                            .speed(0.001)
                            .range(0.0..=1.0)
                            .prefix("Normal bias "),
                    )
                    .on_hover_text("World units. Large values can detach shadows from objects.");
                    if ui.small_button("Reset lighting").clicked() {
                        *light = Default::default();
                    }
                });
            ui.separator();
            egui::CollapsingHeader::new("ENVIRONMENT")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut scene.environment.intensity)
                            .speed(0.01)
                            .range(0.0..=1000.0)
                            .prefix("Sky intensity "),
                    );
                    ui.checkbox(&mut scene.environment.background, "Show sky background");
                    for (name, color) in [
                        ("Zenith", &mut scene.environment.zenith),
                        ("Horizon", &mut scene.environment.horizon),
                        ("Ground", &mut scene.environment.ground),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            color_edit_button_rgb(ui, color);
                        });
                    }
                    if ui.button("Reset environment").clicked() {
                        scene.environment = Default::default();
                    }
                });
            ui.separator();
            crate::fog::controls(ui, &mut scene.fog);
            ui.separator();
            egui::CollapsingHeader::new("DISPLAY & BLOOM")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::Slider::new(&mut scene.display.exposure_ev, -16.0..=16.0)
                            .text("Exposure EV"),
                    );
                    ui.checkbox(&mut scene.display.tone_mapping, "Reinhard tone mapping");
                    let bloom = &mut scene.display.bloom;
                    ui.checkbox(&mut bloom.enabled, "Bloom");
                    ui.add_enabled_ui(bloom.enabled, |ui| {
                        ui.add(
                            egui::Slider::new(&mut bloom.intensity, 0.0..=10.0)
                                .logarithmic(true)
                                .text("Glow intensity"),
                        );
                        ui.horizontal(|ui| {
                            ui.label("Threshold");
                            ui.add(
                                egui::DragValue::new(&mut bloom.threshold)
                                    .speed(0.05)
                                    .range(0.0..=60000.),
                            );
                        });
                        ui.add(egui::Slider::new(&mut bloom.scatter, 0.0..=1.0).text("Spread"));
                        ui.weak("Threshold is scene brightness before exposure.");
                    });
                    if ui.button("Reset display").clicked() {
                        scene.display = Default::default();
                    }
                });
        });
        if ui.is_enabled()
            && self.editor.play.is_none()
            && (scene.fog != self.editor.scene().fog
                || scene.gi != self.editor.scene().gi
                || scene.lighting != self.editor.scene().lighting
                || scene.display != self.editor.scene().display
                || scene.environment != self.editor.scene().environment)
        {
            self.editor.begin_gesture("Edit scene lighting");
            let result = self.editor.apply("Edit scene lighting", scene);
            self.result(result);
        }
        if fit_gi {
            self.workspace.gi_visible = true;
            let result = self.editor.fit_gi_volume();
            self.result(result);
        }
        if bake_gi && self.loading.is_none() {
            let result = self.editor.bake_gi_job().map(|job| {
                self.loading = Some(loading::Loading::BakeGi(job));
            });
            self.result(result);
        }
    }
}
pub(super) fn texture_control(
    ui: &mut egui::Ui,
    texture: &mut Texture,
    scene: &bozzard_scene::Scene,
) {
    egui::ComboBox::from_id_salt("texture")
        .selected_text(match texture {
            Texture::White => "White / no map",
            Texture::Checker => "Checker",
            Texture::Normals => "World normals",
            Texture::ProceduralChecker => "Procedural checker",
            Texture::Toon => "Toon (3 bands)",
            Texture::Asset(id) => id,
        })
        .show_ui(ui, |ui| {
            for (value, label) in [
                (Texture::White, "White / no map"),
                (Texture::Checker, "Checker"),
                (Texture::Normals, "World normals"),
                (Texture::ProceduralChecker, "Procedural checker"),
                (Texture::Toon, "Toon (3 bands)"),
            ] {
                ui.selectable_value(texture, value, label);
            }
            for (id, asset) in &scene.assets {
                if asset.kind == AssetKind::Image {
                    ui.selectable_value(texture, Texture::Asset(id.clone()), id);
                }
            }
        });
}
pub(super) fn vector(ui: &mut egui::Ui, label: &str, value: &mut [f32; 3], speed: f64) {
    ui.push_id(label, |ui| {
        ui.label(label);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let width = ((ui.available_width() - 12.0) / 3.0 - 18.0).max(24.0);
            for (index, v) in value.iter_mut().enumerate() {
                ui.label(
                    egui::RichText::new([" X ", " Y ", " Z "][index])
                        .background_color(theme::AXES[index])
                        .color(Color32::WHITE),
                );
                ui.add_sized(
                    [width, 20.0],
                    egui::DragValue::new(v).speed(speed).max_decimals(3),
                )
                .on_hover_text(format!("{label} · {}", ["X", "Y", "Z"][index]));
            }
        });
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

fn eligible_follow_camera(object: &bozzard_scene::Object) -> bool {
    object.parent.is_none()
        && object.spin.is_none()
        && object.gravity.is_none()
        && object.collider.is_none()
        && object.trigger.is_none()
        && object.player_controller.is_none()
        && matches!(object.camera, Some(Camera::Perspective { .. }))
}

// Both camera widgets publish the active view and controller reference in one transaction.
fn synchronize_follow_camera(scene: &mut bozzard_scene::Scene) {
    if let Some(camera) = scene.views.get(&Layer::ThreeD) {
        for object in &mut scene.objects {
            if let Some(controller) = &mut object.player_controller {
                controller.camera.clone_from(camera);
            }
        }
    }
}

fn checkpoint_action(
    current: &bozzard_scene::TriggerAction,
    start: [f32; 3],
) -> bozzard_scene::TriggerAction {
    use bozzard_scene::TriggerAction;
    match current {
        TriggerAction::Checkpoint { .. } => current.clone(),
        _ => TriggerAction::Checkpoint { respawn: start },
    }
}

// Called on the validated authored document, never a partially edited inspector draft.
fn checkpoint_respawn(scene: &bozzard_scene::Scene, marker: &bozzard_scene::Object) -> [f32; 3] {
    if let Some(player) = scene.objects.iter().find(|o| o.player_controller.is_some()) {
        return player.transform.translation; // Validated controllers are roots with safe starts.
    }
    let mut matrix = marker.transform.matrix();
    let mut parent = marker.parent.as_deref();
    while let Some(id) = parent {
        let object = scene
            .objects
            .iter()
            .find(|o| o.id == id)
            .expect("validated parent");
        matrix = object.transform.matrix() * matrix;
        parent = object.parent.as_deref();
    }
    matrix.transform_point3(Vec3::ZERO).to_array()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bozzard_scene::{Scene, TriggerAction};

    fn editor() -> Editor {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/first-trail.json");
        Editor::open(&path).unwrap()
    }

    #[test]
    fn existing_checkpoint_respawn_is_preserved_when_building_choices() {
        let editor = editor();
        let marker = editor
            .scene()
            .objects
            .iter()
            .find(|o| o.id == "checkpoint")
            .unwrap();
        let current = &marker.trigger.as_ref().unwrap().action;
        let start = checkpoint_respawn(editor.scene(), marker);
        assert_ne!(*current, TriggerAction::Checkpoint { respawn: start });
        assert_eq!(checkpoint_action(current, start), *current);
        assert_eq!(
            checkpoint_action(&TriggerAction::Goal, start),
            TriggerAction::Checkpoint { respawn: start }
        );
    }

    #[test]
    fn floor_checkpoint_conversion_uses_safe_start_and_parent_fallback_is_world_space() {
        let mut editor = editor();
        let mut scene = editor.scene().clone();
        let marker = scene
            .objects
            .iter_mut()
            .find(|o| o.id == "checkpoint")
            .unwrap();
        marker.trigger.as_mut().unwrap().action = TriggerAction::Goal;
        editor.apply("Goal", scene).unwrap();
        let original = editor.scene().clone();
        let marker = original
            .objects
            .iter()
            .find(|o| o.id == "checkpoint")
            .unwrap();
        assert_eq!(marker.transform.translation[1], 0.1);
        let respawn = checkpoint_respawn(&original, marker);
        let mut scene = original.clone();
        scene
            .objects
            .iter_mut()
            .find(|o| o.id == "checkpoint")
            .unwrap()
            .trigger
            .as_mut()
            .unwrap()
            .action = checkpoint_action(&TriggerAction::Goal, respawn);
        editor.apply("Checkpoint", scene.clone()).unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &original);
        editor.redo().unwrap();
        assert_eq!(editor.scene(), &scene);

        let mut fallback: Scene = original;
        let player = fallback
            .objects
            .iter_mut()
            .find(|o| o.id == "player")
            .unwrap();
        player.player_controller = None;
        player.transform.translation = [3.0, 4.0, 5.0];
        player.transform.rotation_degrees = [0.0, 90.0, 0.0];
        player.transform.scale = [2.0; 3];
        let parent_matrix = player.transform.matrix();
        let marker = fallback
            .objects
            .iter_mut()
            .find(|o| o.id == "checkpoint")
            .unwrap();
        marker.parent = Some("player".into());
        let expected =
            parent_matrix.transform_point3(Vec3::from_array(marker.transform.translation));
        fallback.validate().unwrap();
        let marker = fallback
            .objects
            .iter()
            .find(|o| o.id == "checkpoint")
            .unwrap();
        assert!(
            Vec3::from_array(checkpoint_respawn(&fallback, marker)).abs_diff_eq(expected, 1e-5)
        );
    }

    #[test]
    fn follow_camera_switch_is_one_validated_undoable_transaction() {
        let mut editor = editor();
        editor.selected = Some("camera".into());
        editor.duplicate().unwrap();
        let camera = editor.selected.clone().unwrap();
        assert!(eligible_follow_camera(editor.selected_object().unwrap()));
        let original = editor.scene().clone();
        let mut next = original.clone();
        next.views.insert(Layer::ThreeD, camera.clone());
        synchronize_follow_camera(&mut next);
        editor.begin_gesture("Edit component");
        editor.apply("Edit component", next.clone()).unwrap();
        editor.finish_gesture();
        assert_eq!(
            editor
                .scene()
                .objects
                .iter()
                .find_map(|o| o.player_controller.as_ref())
                .unwrap()
                .camera,
            camera
        );
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &original);
        editor.redo().unwrap();
        assert_eq!(editor.scene(), &next);
    }
}

/// egui converts RGB through HSV even when idle; publish only an intentional edit.
pub(super) fn color_edit_button_rgb(ui: &mut egui::Ui, color: &mut [f32; 3]) -> egui::Response {
    let mut candidate = *color;
    let response = ui.color_edit_button_rgb(&mut candidate);
    if response.changed() {
        *color = candidate;
    }
    response
}
#[cfg(test)]
mod color_tests {
    use super::*;
    #[test]
    fn idle_picker_keeps_exact_scene_color_and_bake_fingerprint_inputs() {
        let ctx = egui::Context::default();
        for original in [
            [0.15, 0.32, 0.65],
            [0.65, 0.7, 0.8],
            [0.12, 0.1, 0.08],
            [0.75, 0.04, 0.02],
        ] {
            let mut color = original;
            for _ in 0..3 {
                let mut output = ctx.run_ui(Default::default(), |ui| {
                    assert!(!color_edit_button_rgb(ui, &mut color).changed());
                });
                output.textures_delta.clear();
                assert_eq!(color, original);
            }
        }
    }
}

impl App {
    fn prefab_inspector(&mut self, ui: &mut egui::Ui) {
        use bozzard_editor::PrefabCommand;
        if self.editor.selected_object().is_none() || self.editor.selected_surface().is_some() {
            return;
        }
        let asset = self
            .editor
            .selected_prefab_root()
            .map(|root| self.editor.scene().prefabs[root].asset.clone());
        ui.add_enabled_ui(self.editor.play.is_none() && self.loading.is_none(), |ui| {
            if let Some(asset) = asset {
                ui.colored_label(Color32::from_rgb(178, 155, 244), format!("Prefab · {asset}"));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Apply to prefab").on_hover_text("Writes this instance's component edits to the source file and updates linked instances in this scene. Placement stays local. Scene Undo does not undo the source file write.").clicked() {
                        self.start_prefab(PrefabCommand::Apply);
                    }
                    if ui.button("Refresh instances").on_hover_text("Read the source again and update instances in this scene, preserving local component overrides. Undoable.").clicked() {
                        self.start_prefab(PrefabCommand::Refresh { asset });
                    }
                    if ui.button("Unpack").on_hover_text("Keep these objects and detach their prefab link. Unpack before adding, removing, or reparenting children. Undoable.").clicked() {
                        let result = self.editor.unpack_prefab(); self.result(result);
                    }
                });
            } else if ui.button("Save as prefab").on_hover_text("Save this hierarchy into assets/ and link it to a reusable prefab. Scene Undo keeps the source file for reuse.").clicked() {
                self.start_prefab(PrefabCommand::Create);
            }
        });
        ui.separator();
    }
}
