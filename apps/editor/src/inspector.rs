use super::*;

/// Editable panel data, excluding the potentially large entity/asset document.
#[derive(Clone, PartialEq)]
pub(crate) struct SceneSettings {
    pub name: String,
    pub game_flow: Option<bozzard_scene::GameFlowSettings>,
    pub gi: bozzard_scene::GiSettings,
    pub lighting: bozzard_scene::Lighting,
    pub environment: bozzard_scene::EnvironmentSettings,
    pub fog: bozzard_scene::FogSettings,
    pub display: bozzard_scene::DisplaySettings,
    pub post_process_volumes: Vec<bozzard_scene::PostProcessVolume>,
}
impl From<&bozzard_scene::Scene> for SceneSettings {
    fn from(scene: &bozzard_scene::Scene) -> Self {
        Self {
            name: scene.name.clone(),
            game_flow: scene.game_flow.clone(),
            gi: scene.gi.clone(),
            lighting: scene.lighting,
            environment: scene.environment,
            fog: scene.fog,
            display: scene.display,
            post_process_volumes: scene.post_process_volumes.clone(),
        }
    }
}
impl SceneSettings {
    pub fn apply_to(self, document: &bozzard_scene::Scene) -> bozzard_scene::Scene {
        let mut scene = document.clone();
        scene.game_flow = self.game_flow;
        scene.gi = self.gi;
        scene.lighting = self.lighting;
        scene.environment = self.environment;
        scene.fog = self.fog;
        scene.display = self.display;
        scene.post_process_volumes = self.post_process_volumes;
        scene
    }
}

impl App {
    pub fn inspector(&mut self, ui: &mut egui::Ui) {
        if self.loading.is_some() {
            ui.disable();
        }
        if self.hierarchy_state.selection_count() > 1 && self.editor.selected_surface().is_none() {
            self.multi_inspector(ui);
            return;
        }
        let Some(original) = self.editor.selected_object().cloned() else {
            theme::empty_state(
                ui,
                icons::ADS_CLICK,
                "Nothing selected",
                "Select an object in the viewport or the hierarchy to inspect it.",
            );
            return;
        };
        let mut object = original.clone();
        let mut scene = self.editor.scene_snapshot();
        let mut views = scene.views.clone();
        let checkpoint_start = checkpoint_respawn(&scene, &original);
        let mut remove = None;
        let mut generate_lods = false;
        let mut animation_preview = None;
        let mut error_slot: Option<anyhow::Error> = None;
        ui.push_id(&original.id, |ui| {
        egui::ScrollArea::vertical().id_salt("entity-properties").show(ui, |ui| {
                    if self.surface_inspector(ui) { return; }
                    let has_children = scene.objects.iter().any(|o| o.parent.as_deref() == Some(&object.id));
                    ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                        entity_header(ui, &mut object, &scene, has_children);
                    });
                    self.prefab_inspector(ui);
                    ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                        let mut reset = false;
                        theme::Foldout::new(icons::OPEN_WITH, "Transform")
                            .default_open(true)
                            .show_with_actions(
                            ui,
                            |ui| {
                                reset = ui
                                    .add(egui::Button::new(egui::RichText::new(icons::RESTART_ALT).color(theme::TEXT_MUTED)).frame_when_inactive(false))
                                    .on_hover_text("Reset transform")
                                    .clicked();
                            },
                            |ui| {
                                vector(ui, "Position", &mut object.transform.translation, 0.05);
                                vector(ui, "Rotation °", &mut object.transform.rotation_degrees, 0.5);
                                vector(ui, "Scale", &mut object.transform.scale, 0.02);
                            },
                        );
                        if reset {
                            object.transform = Transform::default();
                        }
                    });
                    if !object.blueprints.is_empty() {
                        self.blueprint_inspector(ui, &mut object);
                    }
                    self.script_manager_inspector(ui, &mut object);
                    self.shader_graph_inspector(ui, &mut object);
                    ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                        generate_lods = self.lod_tools.ui(ui, &object, &self.editor.assets);
                        // Every registered component with generic field metadata draws itself.
                        for entry in bozzard_scene::components()
                            .filter(|entry| entry.ui == bozzard_scene::Ui::Generic || self.custom_inspectors.contains(entry.name))
                        {
                            if !(entry.present)(&object) {
                                continue;
                            }
                            component_section(ui, entry.label, &mut remove, |ui| {
                                match self.custom_inspectors.draw(entry.name, ui, &mut object, &scene, &self.editor.assets) {
                                    Ok(true) => return,
                                    Ok(false) => {},
                                    Err(error) => { error_slot = Some(error); return; }
                                }
                                let shared_material = entry.name == "material"
                                    && crate::material_ui::component(ui, &mut object, &self.editor.assets);
                                if !shared_material {
                                match crate::component_ui::fields(
                                    ui,
                                    &mut object,
                                    entry,
                                    &scene,
                                    &self.editor.assets,
                                    &mut views,
                                ) {
                                    Ok(_) => {}
                                    Err(error) => error_slot = Some(error),
                                }
                                }
                                if let Err(error) = crate::motion_ui::component(ui, &mut object, entry.name, &scene) {
                                    error_slot = Some(error);
                                }
                                if let Err(error) = crate::navigation_ui::component(ui, &mut object, entry.name, &scene) { error_slot = Some(error); }
                                if let Err(error) = crate::particle_ui::component(ui, &mut object, entry.name) { error_slot = Some(error); }
                                if let Err(error) = crate::widget_ui::component(ui, &mut object, entry.name, &scene) { error_slot = Some(error); }
                                if let Err(error) = crate::sprite_ui::component(ui, &mut object, entry.name, &self.editor.assets) { error_slot = Some(error); }
                                crate::font_ui::component(ui, &mut object, entry.name, &self.editor.assets);
                                let active_state = self.editor.play.as_ref()
                                    .and_then(|play| play.app.world.resource::<bozzard_scene::middleware::animation::Runtime>())
                                    .and_then(|runtime| runtime.players.get(&object.id))
                                    .map(|player| player.state);
                                let graph_key = format!("{}:{}", self.editor.path.display(), object.id);
                                if entry.name == "animator" {
                                    match crate::animation_ui::component(ui, &mut object, &self.editor.assets, &scene,
                                        &graph_key, self.workspace.animation_graph.entry(graph_key.clone()).or_default(), active_state) {
                                        Ok(request) => animation_preview = request,
                                        Err(error) => error_slot = Some(error),
                                    }
                                }
                            });
                        }
                        // Selecting Checkpoint in the generic Action field must not leave a
                        // placeholder point behind: the scene-derived safe start fills in, as the
                        // hand-written combo used to do. `component_ui`'s trigger hook offers it
                        // again for a checkpoint that already exists.
                        fill_checkpoint_respawn(&original, &mut object, checkpoint_start);
                        // Components another build wrote. They are preserved verbatim, so the
                        // honest thing is to show them rather than pretend the object is empty.
                        for name in object.extras.keys().cloned().collect::<Vec<_>>() {
                            if bozzard_scene::component_type(&name).is_some() { continue; }
                            component_section(ui, &name, &mut remove, |ui| {
                                ui.weak("Saved by another build. This build keeps it unchanged and cannot edit it.");
                            });
                        }
                        if let Some(error) = error_slot.take() { self.result(Err(error)); }
                        if views != scene.views { std::sync::Arc::make_mut(&mut scene).views = views.clone(); }
                        if let Some(error) = add_component_menu(ui, &mut object, &scene, self.layer(), &self.editor.assets) { self.result(Err(error)); }
                    });
                });
        });
        if let Some(component) = remove {
            self.editor.finish_gesture();
            let document = std::sync::Arc::make_mut(&mut scene);
            document.views = views;
            remove_component(&mut object, document, &component);
            views = document.views.clone();
        }
        if let Some(play) = &self.editor.play
            && let Some(entity) = play.instance().entity(&original.id)
            && let Some(state) = play.app.world.get::<bozzard_scene::GravityState>(entity)
            && play
                .app
                .world
                .get::<bozzard_scene::Gravity>(entity)
                .is_some_and(|g| g.enabled)
            && (play
                .app
                .world
                .get::<bozzard_scene::BoxCollider>(entity)
                .is_some_and(|c| c.enabled)
                || play
                    .app
                    .world
                    .get::<bozzard_scene::MeshCollider>(entity)
                    .is_some_and(|c| c.enabled)
                || play
                    .app
                    .world
                    .get::<bozzard_scene::ShapeCollider>(entity)
                    .is_some_and(|c| c.enabled))
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
            && (object != original || views != self.editor.scene().views)
        {
            self.editor.begin_gesture("Edit component");
            let mut scene = std::sync::Arc::unwrap_or_clone(scene);
            scene.views = views;
            if let Some(slot) = scene.objects.iter_mut().find(|o| o.id == object.id) {
                *slot = object;
            }
            synchronize_follow_camera(&mut scene);
            let r = self.editor.apply("Edit component", scene);
            self.result(r);
        }
        if let Some(request) = animation_preview
            && self.editor.play.is_none()
        {
            match request {
                crate::animation_ui::PreviewRequest::Sample { state, phase } => {
                    let result = self
                        .editor
                        .scrub_animation_preview(&original.id, &state, phase);
                    self.result(result);
                }
                crate::animation_ui::PreviewRequest::Clear => self.editor.clear_timeline_preview(),
            }
            self.viewport_stamp = None;
        }
        if generate_lods && self.loading.is_none() && self.editor.play.is_none() {
            self.editor.finish_gesture();
            let result = self
                .editor
                .generate_lods_job(&original.id, self.lod_tools.requests());
            match result {
                Ok(job) => self.loading = Some(loading::Loading::Lods(job)),
                Err(error) => self.result(Err(error)),
            }
        }
    }
    pub fn lighting_inspector(&mut self, ui: &mut egui::Ui) {
        let mut scene = SceneSettings::from(self.editor.scene());
        let original = scene.clone();
        let mut bake_gi = false;
        let mut fit_gi = false;
        let gi_current = self.editor.gi_current();
        ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
            crate::theme::Foldout::new(crate::icons::FLAG, "Game flow").show(ui, |ui| {
                let mut enabled = scene.game_flow.is_some();
                if ui.checkbox(&mut enabled, "Start, pause and retry menus").changed() {
                    scene.game_flow = enabled.then(|| bozzard_scene::GameFlowSettings { title: scene.name.clone(), ..Default::default() });
                }
                if let Some(flow) = &mut scene.game_flow {
                    ui.label("Title"); ui.text_edit_singleline(&mut flow.title);
                    ui.label("Instructions"); ui.text_edit_multiline(&mut flow.instructions);
                    ui.small("Enter starts · Escape pauses · R retries · Q quits. Use End Game in a Blueprint to show the retry menu.");
                }
            });
            crate::theme::Foldout::new(crate::icons::FLARE, "Global illumination")
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
            crate::theme::Foldout::new(crate::icons::WB_SUNNY, "Directional light")
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
                    ui.add(
                        egui::DragValue::new(&mut light.shadow_distance)
                            .speed(0.5)
                            .range(0.0..=100000.0)
                            .prefix("Shadow distance "),
                    )
                    .on_hover_text(
                        "World units around the camera that sun shadows cover. 0 fits every caster; set it when distant objects blur nearby shadows.",
                    );
                    if ui.small_button("Reset lighting").clicked() {
                        *light = Default::default();
                    }
                });
            ui.separator();
            crate::theme::Foldout::new(crate::icons::CLOUD, "Environment")
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
            crate::post_processing::controls(ui, &mut scene.display);
            crate::post_processing::volumes(ui, &mut scene.post_process_volumes);
        });
        if ui.is_enabled() && self.editor.play.is_none() && scene != original {
            self.editor.begin_gesture("Edit scene lighting");
            let result = self
                .editor
                .apply("Edit scene lighting", scene.apply_to(self.editor.scene()));
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
fn add_component_menu(
    ui: &mut egui::Ui,
    object: &mut bozzard_scene::Object,
    scene: &bozzard_scene::Scene,
    layer: Layer,
    assets: &bozzard_assets::AssetStore,
) -> Option<anyhow::Error> {
    let mut error = None;
    ui.separator();
    ui.menu_button("Add Component", |ui| {
        let id = ui.id().with("component-search");
        let mut search = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
        ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search components…"));
        let query = search.trim().to_lowercase();
        ui.data_mut(|d| d.insert_temp(id, search));
        let mut matches = 0;
        for (label, available) in component_choices(object) {
            if !available || !label.to_lowercase().contains(&query) {
                continue;
            }
            matches += 1;
            if ui.button(label).clicked() {
                error = add_component(object, label, scene, layer, assets).err();
                ui.close();
            }
        }
        if matches == 0 {
            ui.weak("No matching components available");
        }
    });
    error
}

fn component_section(
    ui: &mut egui::Ui,
    name: &str,
    remove: &mut Option<String>,
    contents: impl FnOnce(&mut egui::Ui),
) {
    let registry = bozzard_scene::component_type_by_label(name).map(|entry| entry.name);
    let icon = theme::component_icon(registry.unwrap_or_default());
    theme::Foldout::new(icon, name).default_open(true).show_with_actions(ui, |ui| {
        let hint = match registry {
            Some("collider") => {
                "Remove collider and dependent Rigidbody / Player Controller (Undo restores them)"
            }
            Some("gravity") => {
                "Remove Rigidbody and dependent Player Controller (Undo restores them)"
            }
            Some("shape_collider") => {
                "Remove collider and dependent Rigidbody (Undo restores them)"
            }
            Some("camera") => "Remove camera (reassign a player follow-camera first)",
            _ => "Remove component (Undo restores it)",
        };
        let button = egui::Button::new(egui::RichText::new(icons::CLOSE).color(theme::TEXT_MUTED))
            .frame_when_inactive(false);
        if ui.add(button).on_hover_text(hint).clicked() {
            *remove = Some(name.to_string());
        }
    }, contents);
}

/// The object's identity at the top of the inspector: kind icon, name, parent and ID.
fn entity_header(
    ui: &mut egui::Ui,
    object: &mut bozzard_scene::Object,
    scene: &bozzard_scene::Scene,
    has_children: bool,
) {
    let (icon, tint) = theme::object_icon(object, has_children);
    egui::Frame::new()
        .fill(theme::RAISED)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(theme::RADIUS)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(icon).size(20.0).color(tint));
                ui.add(
                    egui::TextEdit::singleline(&mut object.name)
                        .font(theme::bold(ui.ctx(), 13.5))
                        .desired_width(f32::INFINITY),
                )
                .on_hover_text("Object name");
            });
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Parent").color(theme::TEXT_MUTED));
                egui::ComboBox::from_id_salt("parent")
                    .selected_text(
                        object
                            .parent
                            .as_ref()
                            .and_then(|id| scene.objects.iter().find(|o| &o.id == id))
                            .map_or("Root", |parent| parent.name.as_str()),
                    )
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
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&object.id)
                                .monospace()
                                .size(10.5)
                                .color(theme::TEXT_MUTED),
                        )
                        .truncate(),
                    )
                    .on_hover_text(format!("Object ID · {}", object.id));
                });
            });
        });
    ui.add_space(6.0);
}

fn remove_component(
    object: &mut bozzard_scene::Object,
    scene: &mut bozzard_scene::Scene,
    name: &str,
) {
    // Unknown labels (Transform, for instance) are not components and change nothing.
    if let Some(entry) = bozzard_scene::component_type_by_label(name) {
        (entry.remove)(object, scene);
    } else if object.extras.contains_key(name) {
        object.extras.remove(name);
    }
}

fn component_choices(object: &bozzard_scene::Object) -> Vec<(&'static str, bool)> {
    bozzard_scene::components()
        .map(|entry| {
            (
                entry.label,
                !(entry.present)(object) && (entry.available)(object),
            )
        })
        .collect()
}

fn add_component(
    object: &mut bozzard_scene::Object,
    label: &str,
    scene: &bozzard_scene::Scene,
    layer: Layer,
    assets: &bozzard_assets::AssetStore,
) -> Result<()> {
    let entry = bozzard_scene::component_type_by_label(label)
        .with_context(|| format!("unknown component '{label}'"))?;
    ensure!(
        !(entry.present)(object) && (entry.available)(object),
        "Component is unavailable or incompatible"
    );
    let bounds = object
        .drawable
        .as_ref()
        .and_then(|drawable| assets.mesh_surface(&drawable.mesh))
        .map(|(_, bounds)| bounds.map(|bound| bound.to_array()));
    let cooked = if entry.name == "mesh_collider" {
        Some(
            assets.cook_mesh_collider(
                object
                    .drawable
                    .as_ref()
                    .context("Mesh Collider needs a Mesh Renderer")?,
            )?,
        )
    } else {
        None
    };
    (entry.add)(
        object,
        &bozzard_scene::AddContext {
            layer,
            scene,
            bounds,
            cooked,
        },
    )
}

/// Texture picker for imported surface materials, which are edited per source rather than per
/// object and so have no registry field of their own.
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
        // Side by side when the panel is wide; label above the fields when it is narrow.
        let inline = ui.available_width() >= 300.0;
        let mut fields = |ui: &mut egui::Ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let width = ((ui.available_width() - 6.0) / 3.0).max(36.0);
            for (index, v) in value.iter_mut().enumerate() {
                axis_field(
                    ui,
                    index,
                    width,
                    egui::DragValue::new(v).speed(speed).max_decimals(3),
                )
                .on_hover_text(format!("{label} · {}", ["X", "Y", "Z"][index]));
            }
        };
        let name = egui::Label::new(egui::RichText::new(label).color(theme::TEXT_DIM)).truncate();
        if inline {
            ui.horizontal(|ui| {
                ui.add_sized([62.0, 22.0], name);
                fields(ui);
            });
        } else {
            ui.add(name);
            ui.horizontal(fields);
        }
    });
}

/// One axis of a vector: a sunken field with the axis color down its left edge.
pub(super) fn axis_field(
    ui: &mut egui::Ui,
    axis: usize,
    width: f32,
    drag: egui::DragValue<'_>,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 22.0), Sense::hover());
    ui.painter().rect_filled(rect, 4, theme::SUNKEN);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())),
        egui::CornerRadius {
            nw: 4,
            sw: 4,
            ne: 0,
            se: 0,
        },
        theme::AXES[axis],
    );
    let mut field =
        ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(4.0, 0.0))));
    let widgets = &mut field.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = egui::Stroke::NONE;
    widgets.hovered.weak_bg_fill = theme::glass(8);
    field.add_sized([rect.width() - 8.0, 22.0], drag)
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

/// Selecting Checkpoint must never leave a placeholder respawn: when the generic Action field
/// turns a trigger into a checkpoint, the scene-derived safe start fills in.
fn fill_checkpoint_respawn(
    before: &bozzard_scene::Object,
    object: &mut bozzard_scene::Object,
    safe: [f32; 3],
) {
    use bozzard_scene::TriggerAction;
    let was_checkpoint = before
        .trigger
        .as_ref()
        .is_some_and(|trigger| matches!(trigger.action, TriggerAction::Checkpoint { .. }));
    let Some(trigger) = &mut object.trigger else {
        return;
    };
    if !was_checkpoint && matches!(trigger.action, TriggerAction::Checkpoint { .. }) {
        trigger.action = TriggerAction::Checkpoint { respawn: safe };
    }
}

pub(crate) fn checkpoint_respawn(
    scene: &bozzard_scene::Scene,
    marker: &bozzard_scene::Object,
) -> [f32; 3] {
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
    fn settings_draft_preserves_entities_assets_and_history() {
        let mut editor = editor();
        let original = editor.scene().clone();
        let mut settings = SceneSettings::from(&original);
        settings.display.exposure_ev = 0.5;
        settings.game_flow = Some(bozzard_scene::GameFlowSettings::default());
        let scene = settings.apply_to(editor.scene());
        assert_eq!(scene.objects, original.objects);
        assert_eq!(scene.assets, original.assets);
        assert_eq!(scene.views, original.views);
        assert_eq!(scene.prefabs, original.prefabs);
        editor.apply("Scene settings", scene).unwrap();
        assert_eq!(editor.scene().display.exposure_ev, 0.5);
        assert!(editor.scene().game_flow.is_some());
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &original);
    }

    #[test]
    fn component_menu_adds_only_to_its_owner_and_rigidbody_adds_required_collider() {
        let mut editor = editor();
        editor.create(Mesh::Cube, Layer::ThreeD).unwrap();
        let fresh = editor.selected_object().unwrap().clone();
        let mut object = fresh.clone();
        assert!(
            component_choices(&object)
                .iter()
                .any(|(name, available)| *name == "Material" && *available)
        );
        for name in ["Material", "Rigidbody", "Blueprint"] {
            add_component(
                &mut object,
                name,
                editor.scene(),
                Layer::ThreeD,
                &editor.assets,
            )
            .unwrap();
        }
        assert!(object.material.is_some() && object.gravity.is_some() && object.collider.is_some());
        assert_eq!(object.blueprints.len(), 1);
        assert!(
            component_choices(&object)
                .iter()
                .any(|(name, available)| *name == "Material" && !available)
        );
        assert!(
            fresh.material.is_none() && fresh.collider.is_none() && fresh.blueprints.is_empty()
        );
        assert_eq!(editor.selected_object().unwrap(), &fresh);
    }

    #[test]
    fn text_component_is_opt_in_and_independent_of_the_mesh() {
        let mut editor = editor();
        editor.create(Mesh::Cube, Layer::TwoD).unwrap();
        let mut object = editor.selected_object().unwrap().clone();
        let mut scene = editor.scene().clone();
        assert!(object.text_rendering.is_none());
        add_component(
            &mut object,
            "Text Rendering",
            &scene,
            Layer::TwoD,
            &editor.assets,
        )
        .unwrap();
        assert_eq!(object.text_rendering.as_ref().unwrap().layer, Layer::TwoD);
        assert!(
            add_component(
                &mut object,
                "Text Rendering",
                &scene,
                Layer::TwoD,
                &editor.assets
            )
            .is_err()
        );
        remove_component(&mut object, &mut scene, "MESH RENDERER");
        assert!(object.drawable.is_none() && object.text_rendering.is_some());
        let transform = object.transform;
        remove_component(&mut object, &mut scene, "TEXT RENDERING");
        assert!(object.text_rendering.is_none());
        assert_eq!(object.transform, transform);
    }

    #[test]
    fn mesh_collider_accepts_rigidbody_and_survives_renderer_removal() {
        let mut editor = editor();
        editor.create(Mesh::Cube, Layer::ThreeD).unwrap();
        let mut object = editor.selected_object().unwrap().clone();
        let mut scene = editor.scene().clone();
        add_component(
            &mut object,
            "Mesh Collider",
            &scene,
            Layer::ThreeD,
            &editor.assets,
        )
        .unwrap();
        add_component(
            &mut object,
            "Rigidbody",
            &scene,
            Layer::ThreeD,
            &editor.assets,
        )
        .unwrap();
        assert!(object.collider.is_none());
        assert!(object.gravity.is_some());
        let before = object.clone();
        for name in ["Box Collider", "Player Controller", "Trigger"] {
            assert!(
                add_component(&mut object, name, &scene, Layer::ThreeD, &editor.assets).is_err()
            );
            assert_eq!(object, before);
        }
        remove_component(&mut object, &mut scene, "MESH RENDERER");
        assert_eq!(object.mesh_collider, before.mesh_collider);
        assert!(object.drawable.is_none());
        assert_eq!(object.transform, before.transform);
    }

    #[test]
    fn an_unrecognized_component_is_kept_as_data_and_dropped_by_its_own_key() {
        let mut editor = editor();
        editor.create(Mesh::Cube, Layer::ThreeD).unwrap();
        let fresh = editor.selected_object().unwrap().clone();
        let mut scene = editor.scene().clone();
        let mut object = fresh.clone();
        object.set_extra("plasma_shield", serde_json::json!({ "charge": 12 }));
        // It survives a save and load, so a scene from a newer build is not quietly rewritten.
        let mut document = scene.clone();
        let index = document
            .objects
            .iter()
            .position(|candidate| candidate.id == object.id)
            .unwrap();
        document.objects[index] = object.clone();
        let reloaded = Scene::from_json(&document.to_json().unwrap()).unwrap();
        assert_eq!(
            reloaded.objects[index].extra("plasma_shield"),
            Some(&serde_json::json!({ "charge": 12 }))
        );
        // The section's own key removes it, and a known label still routes through the registry.
        remove_component(&mut object, &mut scene, "plasma_shield");
        assert_eq!(object.extra("plasma_shield"), None);
        assert_eq!(object, fresh);
        remove_component(&mut object, &mut scene, "MESH RENDERER");
        assert!(object.drawable.is_none());
        assert_eq!(object.extras, fresh.extras);
    }

    #[test]
    fn removal_keeps_transform_and_cascades_required_components_undoably() {
        let mut editor = editor();
        editor.create(Mesh::Cube, Layer::ThreeD).unwrap();
        let fresh = editor.selected_object().unwrap().clone();
        let mut scene = editor.scene().clone();
        for (add, remove) in [
            ("Material", "MATERIAL"),
            ("Mesh Collider", "MESH COLLIDER"),
            ("Rigidbody", "BOX COLLIDER"),
            ("Light", "LIGHT"),
            ("Spin", "SPIN"),
            ("Trigger", "TRIGGER"),
            ("Camera", "CAMERA"),
        ] {
            let mut object = fresh.clone();
            add_component(&mut object, add, &scene, Layer::ThreeD, &editor.assets).unwrap();
            remove_component(&mut object, &mut scene, remove);
            assert_eq!(object, fresh, "{remove}");
        }
        let mut object = fresh.clone();
        add_component(
            &mut object,
            "Player Controller",
            &scene,
            Layer::ThreeD,
            &editor.assets,
        )
        .unwrap();
        remove_component(&mut object, &mut scene, "BOX COLLIDER");
        assert_eq!(object, fresh);
        remove_component(&mut object, &mut scene, "TRANSFORM");
        assert_eq!(object, fresh);
        remove_component(&mut object, &mut scene, "MESH RENDERER");
        assert!(object.drawable.is_none());
        *scene.objects.iter_mut().find(|o| o.id == fresh.id).unwrap() = object;
        editor.apply("Remove renderer", scene).unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.selected_object().unwrap(), &fresh);
        editor.redo().unwrap();
        assert!(editor.selected_object().unwrap().drawable.is_none());
    }

    #[test]
    fn a_new_checkpoint_takes_the_safe_start_instead_of_a_placeholder() {
        let editor = editor();
        let marker = editor
            .scene()
            .objects
            .iter()
            .find(|o| o.id == "checkpoint")
            .unwrap()
            .clone();
        let safe = checkpoint_respawn(editor.scene(), &marker);
        let authored = marker.trigger.as_ref().unwrap().action.clone();
        assert_ne!(authored, TriggerAction::Checkpoint { respawn: safe });

        let mut before = marker.clone();
        before.trigger.as_mut().unwrap().action = TriggerAction::Goal;
        let mut object = before.clone();
        object.trigger.as_mut().unwrap().action = TriggerAction::Checkpoint { respawn: [0.0; 3] };
        fill_checkpoint_respawn(&before, &mut object, safe);
        assert_eq!(
            object.trigger.as_ref().unwrap().action,
            TriggerAction::Checkpoint { respawn: safe }
        );

        // An existing checkpoint keeps the point the author placed.
        let mut kept = marker.clone();
        fill_checkpoint_respawn(&marker, &mut kept, safe);
        assert_eq!(kept.trigger.as_ref().unwrap().action, authored);
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
            .action = TriggerAction::Checkpoint { respawn };
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
        assert!(bozzard_scene::eligible_follow_camera(
            editor.selected_object().unwrap()
        ));
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
        let overrides = if asset.is_some() {
            self.editor.prefab_overrides()
        } else {
            Ok(Vec::new())
        };
        if self.editor.is_prefab_source() && asset.is_none() {
            ui.colored_label(Color32::from_rgb(178, 155, 244), "Prefab source hierarchy");
            ui.weak("Save writes this source. Place it in a scene to Play.");
            if ui
                .add_enabled(
                    self.loading.is_none(),
                    egui::Button::new("Save prefab source"),
                )
                .clicked()
            {
                self.save_scene(self.editor.path.clone());
            }
            ui.separator();
            return;
        }
        ui.add_enabled_ui(self.editor.play.is_none() && self.loading.is_none(), |ui| {
            if let Some(asset) = asset {
                ui.colored_label(Color32::from_rgb(178, 155, 244), format!("Prefab · {asset}"));
                let affected = self.editor.scene().prefabs.values().filter(|link| link.asset == asset).count();
                ui.small(format!("Source writes update {affected} linked instance(s) in this scene. Scene Undo does not undo a source file write."));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Create variant").on_hover_text("Create a new prefab inheriting this source, keeping the selected instance's component overrides.").clicked() {
                        self.start_prefab(PrefabCommand::Variant);
                    }
                    if ui.button("Edit source hierarchy").on_hover_text("Open this prefab source as an independent document. Edit its hierarchy, then Save and refresh scene instances.").clicked()
                        && let Some(source) = self.editor.scene().assets.get(&asset) {
                        self.open_additive(bozzard_editor::root(&self.editor.path).join(&source.path));
                    }
                    if ui.button("Apply to prefab").on_hover_text("Writes this instance's component edits to the source file and updates linked instances in this scene. Placement stays local. Scene Undo does not undo the source file write.").clicked() {
                        self.start_prefab(PrefabCommand::Apply);
                    }
                    if ui.button("Refresh instances").on_hover_text("Read the source again and update instances in this scene, preserving local component overrides. Undoable.").clicked() {
                        self.start_prefab(PrefabCommand::Refresh { asset: asset.clone() });
                    }
                    if ui.button("Unpack").on_hover_text("Keep these objects and detach their prefab link. Unpack before adding, removing, or reparenting children. Undoable.").clicked() {
                        let result = self.editor.unpack_prefab(); self.result(result);
                    }
                });
                match &overrides {
                    Ok(changes) if changes.is_empty() => { ui.weak("No component overrides on this instance."); }
                    Ok(changes) => {
                        ui.collapsing(format!("Component overrides · {}", changes.len()), |ui| {
                            let id = ui.make_persistent_id(("prefab-chosen", &asset));
                            let mut chosen = ui.data_mut(|d| d.get_temp::<std::collections::BTreeSet<(String, String)>>(id)).unwrap_or_default();
                            let current: std::collections::BTreeSet<_> = changes.iter().map(|c| (c.object.clone(), c.component.clone())).collect();
                            chosen.retain(|key| current.contains(key));
                            let mut previous = None;
                            for change in changes {
                                if previous.as_deref() != Some(change.object.as_str()) {
                                    ui.strong(format!("{} · {}", change.object_name, change.object));
                                    previous = Some(change.object.clone());
                                }
                                let key = (change.object.clone(), change.component.clone());
                                let mut checked = chosen.contains(&key);
                                ui.horizontal_wrapped(|ui| {
                                    if ui.checkbox(&mut checked, &change.label).changed() {
                                        if checked { chosen.insert(key.clone()); } else { chosen.remove(&key); }
                                    }
                                    ui.weak(format!("Inherited: {}  →  Override: {}", prefab_value(&change.inherited), prefab_value(&change.current)));
                                });
                            }
                            ui.horizontal_wrapped(|ui| {
                                if ui.add_enabled(!chosen.is_empty(), egui::Button::new("Revert selected in scene")).on_hover_text("One undoable scene edit; leaves unrelated overrides and source files untouched.").clicked() {
                                    let selected: Vec<_> = chosen.iter().cloned().collect();
                                    let result = self.editor.revert_prefab_components(&selected);
                                    self.result(result);
                                    chosen.clear();
                                }
                                if ui.add_enabled(!chosen.is_empty(), egui::Button::new("Apply selected to source…")).on_hover_text(format!("Writes a prefab source file and refreshes {affected} linked instance(s). Scene Undo cannot undo this file write.")).clicked() {
                                    self.start_prefab(PrefabCommand::ApplySelected { components: chosen.iter().cloned().collect() });
                                    chosen.clear();
                                }
                            });
                            ui.data_mut(|d| d.insert_temp(id, chosen));
                        });
                    }
                    Err(error) => { ui.colored_label(Color32::LIGHT_RED, format!("Cannot inspect overrides: {error:#}")); }
                }
            } else if ui.button("Save as prefab").on_hover_text("Save this hierarchy into assets/ and link it to a reusable prefab. Scene Undo keeps the source file for reuse.").clicked() {
                self.start_prefab(PrefabCommand::Create);
            }
        });
        ui.separator();
    }
}

fn prefab_value(value: &Option<serde_json::Value>) -> String {
    let text = value
        .as_ref()
        .map_or_else(|| "(absent)".into(), |v| v.to_string());
    if text.chars().count() > 100 {
        format!("{}…", text.chars().take(100).collect::<String>())
    } else {
        text
    }
}
