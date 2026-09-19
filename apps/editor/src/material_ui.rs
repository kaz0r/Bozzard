//! Shared source drafts and typed per-object overrides.
use anyhow::{Result, ensure};
use bozzard_assets::AssetStore;
use bozzard_editor::Editor;
use bozzard_scene::{
    Object, Texture,
    material_asset::{
        MaterialAsset, MaterialInstance, MaterialShader, MaterialTexture, MaterialValues,
        PropertyOverrides,
    },
};
use eframe::egui;
use std::sync::Arc;

fn properties(ui: &mut egui::Ui, fields: &mut PropertyOverrides, inherited: &MaterialValues) {
    ui.label("Enable a property to override its inherited value");
    ui.horizontal(|ui| {
        let mut enabled = fields.color.is_some();
        if ui.checkbox(&mut enabled, "Color").changed() {
            fields.color = enabled.then_some(inherited.color);
        }
        if let Some(color) = &mut fields.color {
            crate::inspector::color_edit_button_rgb(ui, color);
        }
    });
    ui.horizontal(|ui| {
        let mut enabled = fields.uv_scale.is_some();
        if ui.checkbox(&mut enabled, "UV repeat").changed() {
            fields.uv_scale = enabled.then_some(inherited.uv_scale);
        }
        if let Some(scale) = &mut fields.uv_scale {
            for value in scale {
                ui.add(egui::DragValue::new(value).speed(0.02).range(0.001..=1000.));
            }
        }
    });
    for (name, field, value) in [
        (
            "Metallic",
            &mut fields.metallic,
            inherited.metallic.unwrap_or(0.),
        ),
        (
            "Roughness",
            &mut fields.roughness,
            inherited.roughness.unwrap_or(0.5),
        ),
    ] {
        ui.horizontal(|ui| {
            let mut enabled = field.is_some();
            if ui.checkbox(&mut enabled, name).changed() {
                *field = enabled.then_some(value);
            }
            if let Some(value) = field {
                ui.add(egui::Slider::new(value, 0.0..=1.0));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_draft_is_exact_and_completed_save_publishes_before_job_drop() -> Result<()> {
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let temp = Temp(std::env::temp_dir().join(format!(
            "bozzard-material-pane-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        )));
        std::fs::create_dir(&temp.0)?;
        let scene = bozzard_scene::Scene::from_json(
            r#"{"version":1,"name":"Materials","views":{},"objects":[]}"#,
        )?;
        let mut editor = Editor::new(scene, &temp.0.join("scene.json"))?;
        let id = editor.create_material(None)?;
        let saved = editor.material_source(&id)?;
        let definition = MaterialAsset {
            properties: PropertyOverrides {
                color: Some([0.15, 0.32, 0.65]),
                ..Default::default()
            },
            ..Default::default()
        };
        editor.save_material(&id, &saved, &definition)?;
        let mut pane = MaterialPane::default();
        pane.open(&editor, &id)?;
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let mut output = ctx.run_ui(Default::default(), |_| pane.ui(&ctx, &mut editor, false));
            output.textures_delta.clear();
            assert!(!pane.dirty(), "idle UI changed the material source");
            assert_eq!(pane.draft.as_ref().unwrap().value, definition);
        }
        let draft = pane.draft.as_mut().unwrap();
        draft.value.name = "Published by the UI".into();
        draft.saving = Some(editor.material_job(&id, &draft.saved, &draft.value)?);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while pane.draft.as_ref().unwrap().saving.is_some() {
            ensure!(
                std::time::Instant::now() < deadline,
                "material save timed out"
            );
            let mut output = ctx.run_ui(Default::default(), |_| pane.ui(&ctx, &mut editor, false));
            output.textures_delta.clear();
            std::thread::yield_now();
        }
        assert_eq!(pane.draft.as_ref().unwrap().error, None);
        assert!(!pane.dirty());
        assert_eq!(
            MaterialAsset::from_json(&editor.material_source(&id)?)?.name,
            "Published by the UI"
        );
        assert_eq!(
            editor.assets.material(&id)?.values.color,
            [0.15, 0.32, 0.65]
        );
        Ok(())
    }
}
fn keywords(
    ui: &mut egui::Ui,
    values: &mut std::collections::BTreeMap<String, bool>,
    defaults: &std::collections::BTreeMap<String, bool>,
) {
    if defaults.is_empty() && values.is_empty() {
        return;
    }
    ui.label("Shader keywords");
    for (name, default) in defaults {
        ui.horizontal(|ui| {
            let mut overridden = values.contains_key(name);
            if ui.checkbox(&mut overridden, name).changed() {
                if overridden {
                    values.insert(name.clone(), *default);
                } else {
                    values.remove(name);
                }
            }
            if let Some(value) = values.get_mut(name) {
                ui.checkbox(value, "Enabled");
            } else {
                ui.weak(if *default {
                    "Inherited: on"
                } else {
                    "Inherited: off"
                });
            }
        });
    }
    let stale: Vec<_> = values
        .keys()
        .filter(|name| !defaults.contains_key(*name))
        .cloned()
        .collect();
    for name in stale {
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::YELLOW, format!("Undeclared: {name}"));
            if ui.small_button("Remove").clicked() {
                values.remove(&name);
            }
        });
    }
}
/// True replaces the legacy material fields; instance edits use ordinary scene Undo.
pub fn component(ui: &mut egui::Ui, object: &mut Object, assets: &AssetStore) -> bool {
    let Some(material) = &mut object.material else {
        return false;
    };
    let mut selected = material.shared.as_ref().map(|b| b.asset.clone());
    let previous = selected.clone();
    egui::ComboBox::from_id_salt("shared-material")
        .selected_text(selected.as_deref().unwrap_or("Local material"))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut selected, None, "Local material");
            for entry in assets
                .entries()
                .filter(|e| matches!(e.data(), Some(bozzard_assets::AssetData::Material(_))))
            {
                ui.selectable_value(&mut selected, Some(entry.id.clone()), &entry.id);
            }
        });
    if selected != previous {
        material.shared = selected.map(|id| Arc::new(MaterialInstance::new(id)));
    }
    let Some(binding) = &mut material.shared else {
        return false;
    };
    let mut draft = (**binding).clone();
    if let Ok(source) = assets.material(&draft.asset) {
        properties(ui, &mut draft.properties, &source.values);
        egui::ComboBox::from_id_salt("material-instance-texture")
            .selected_text(format!(
                "Texture: {}",
                if draft.texture.is_some() {
                    "Override"
                } else {
                    "Inherited"
                }
            ))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut draft.texture, None, "Inherited");
                for (label, texture) in [
                    ("White", Texture::White),
                    ("Checker", Texture::Checker),
                    ("Normals", Texture::Normals),
                    ("Procedural checker", Texture::ProceduralChecker),
                    ("Toon", Texture::Toon),
                ] {
                    ui.selectable_value(&mut draft.texture, Some(texture), label);
                }
                for entry in assets
                    .entries()
                    .filter(|e| matches!(e.data(), Some(bozzard_assets::AssetData::Image(_))))
                {
                    ui.selectable_value(
                        &mut draft.texture,
                        Some(Texture::Asset(entry.id.clone())),
                        &entry.id,
                    );
                }
            });
        let graph = object.shader_graph.as_ref().or(source.shader.as_deref());
        if let Some(graph) = graph {
            let mut defaults = graph.keywords.clone();
            if object.shader_graph.is_none() {
                defaults.extend(source.keywords.clone());
            }
            keywords(ui, &mut draft.keywords, &defaults);
        }
        if ui.small_button("Reset instance overrides").clicked() {
            draft = MaterialInstance::new(draft.asset.clone());
        }
    }
    if draft != **binding {
        *binding = Arc::new(draft);
    }
    true
}

#[derive(Default)]
pub struct MaterialPane {
    draft: Option<Draft>,
}
struct Draft {
    scene: std::path::PathBuf,
    id: String,
    saved: String,
    original: MaterialAsset,
    value: MaterialAsset,
    history: Vec<MaterialAsset>,
    future: Vec<MaterialAsset>,
    editing: bool,
    shader: crate::shaders::ShaderPane,
    error: Option<String>,
    saving: Option<bozzard_assets::job::Job<bozzard_editor::PreparedMaterial>>,
}
impl MaterialPane {
    pub fn dirty(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(|draft| draft.value != draft.original || draft.saving.is_some())
    }

    pub fn open(&mut self, editor: &Editor, id: &str) -> Result<()> {
        ensure!(
            self.draft
                .as_ref()
                .is_none_or(|d| d.value == d.original && d.saving.is_none()),
            "Save or revert the open material draft first"
        );
        let saved = editor.material_source(id)?;
        let original = MaterialAsset::from_json(&saved)?;
        self.draft = Some(Draft {
            scene: editor.path.clone(),
            id: id.into(),
            saved,
            value: original.clone(),
            original,
            history: Vec::new(),
            future: Vec::new(),
            editing: false,
            shader: Default::default(),
            error: None,
            saving: None,
        });
        Ok(())
    }
    pub fn ui(&mut self, ctx: &egui::Context, editor: &mut Editor, busy: bool) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        if let Some(result) = draft.saving.as_ref().and_then(|job| job.poll()) {
            // Dropping a job cancels its progress token, including the token used
            // by guarded publication. Keep it alive until acceptance finishes.
            let saving = draft.saving.take();
            let result = result.and_then(|prepared| editor.accept_material(prepared));
            drop(saving);
            match result {
                Ok(json) => {
                    draft.original =
                        MaterialAsset::from_json(&json).expect("validated material source");
                    draft.saved = json;
                    draft.error = None;
                }
                Err(error) => draft.error = Some(format!("{error:#}")),
            }
        }
        let mut close = false;
        let mut record = true;
        let before = draft.value.clone();
        egui::Window::new(format!("Material source · {}", draft.id))
            .id(egui::Id::new("material-source-editor"))
            .default_size([760., 640.])
            .resizable(true)
            .show(ctx, |ui| {
                let available = editor.path == draft.scene
                    && editor.play.is_none()
                    && !busy
                    && draft.saving.is_none();
                // Isolate changing status widgets so they cannot change the form's
                // automatic IDs and steal keyboard focus after the first keystroke.
                ui.vertical(|ui| {
                    if let Some(job) = &draft.saving {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(job.label());
                            if ui.button("Cancel save").clicked() {
                                job.cancel();
                            }
                        });
                        ctx.request_repaint_after(std::time::Duration::from_millis(30));
                    } else if !available {
                        ui.label("Return to the original scene and stop Play to save this draft.");
                    }
                    if draft.value != draft.original {
                        ui.colored_label(egui::Color32::YELLOW, "Unsaved material draft");
                    } else {
                        ui.weak("Material source saved");
                    }
                    if let Some(error) = &draft.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                });
                if ui
                    .add_enabled(
                        draft.saving.is_none(),
                        egui::Button::new("Close and discard draft"),
                    )
                    .clicked()
                {
                    close = true;
                }
                ui.add_enabled_ui(available, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Save source").clicked() {
                            match editor.material_job(&draft.id, &draft.saved, &draft.value) {
                                Ok(job) => {
                                    draft.saving = Some(job);
                                    draft.error = None;
                                }
                                Err(error) => draft.error = Some(format!("{error:#}")),
                            }
                        }
                        if ui
                            .add_enabled(!draft.history.is_empty(), egui::Button::new("Undo draft"))
                            .clicked()
                        {
                            draft.future.push(draft.value.clone());
                            draft.value = draft.history.pop().unwrap();
                            draft.editing = false;
                            record = false;
                        }
                        if ui
                            .add_enabled(!draft.future.is_empty(), egui::Button::new("Redo draft"))
                            .clicked()
                        {
                            draft.history.push(draft.value.clone());
                            draft.value = draft.future.pop().unwrap();
                            draft.editing = false;
                            record = false;
                        }
                        if ui.button("Reload source").clicked() {
                            record = false;
                            let result = (|| -> Result<_> {
                                let json = editor.material_source(&draft.id)?;
                                let value = MaterialAsset::from_json(&json)?;
                                Ok((json, value))
                            })();
                            match result {
                                Ok((json, value)) => {
                                    draft.saved = json;
                                    draft.original = value.clone();
                                    draft.value = value;
                                    draft.history.clear();
                                    draft.future.clear();
                                    draft.editing = false;
                                    draft.error = None;
                                }
                                Err(error) => draft.error = Some(format!("{error:#}")),
                            }
                        }
                        if ui.button("Revert draft").clicked() {
                            record = false;
                            draft.value = draft.original.clone();
                            draft.history.clear();
                            draft.future.clear();
                            draft.editing = false;
                        }
                    });
                    ui.weak("Save updates every instance. Draft edits stay here until saved.");
                    egui::ScrollArea::vertical()
                        .max_height(300.)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Name");
                                ui.add(
                                    egui::TextEdit::singleline(&mut draft.value.name)
                                        .id_salt("source-material-name")
                                        .char_limit(128),
                                );
                            });
                            let mut parent = draft.value.parent.clone().unwrap_or_default();
                            ui.horizontal(|ui| {
                                ui.label("Parent file");
                                ui.add(
                                    egui::TextEdit::singleline(&mut parent)
                                        .id_salt("source-material-parent")
                                        .hint_text("Optional relative .material.json path")
                                        .char_limit(1024),
                                );
                            });
                            draft.value.parent = (!parent.trim().is_empty()).then_some(parent);
                            let inherited = editor
                                .assets
                                .material(&draft.id)
                                .map(|m| m.values.clone())
                                .unwrap_or_default();
                            properties(ui, &mut draft.value.properties, &inherited);
                            egui::ComboBox::from_id_salt("source-material-texture")
                                .selected_text(format!("Texture: {:?}", draft.value.texture))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut draft.value.texture,
                                        None,
                                        "Inherit parent",
                                    );
                                    for (label, texture) in [
                                        ("Original mesh maps", MaterialTexture::Source),
                                        ("White", MaterialTexture::White),
                                        ("Checker", MaterialTexture::Checker),
                                        ("Normals", MaterialTexture::Normals),
                                        ("Procedural checker", MaterialTexture::ProceduralChecker),
                                        ("Toon", MaterialTexture::Toon),
                                        ("Image file", MaterialTexture::Image(String::new())),
                                    ] {
                                        ui.selectable_value(
                                            &mut draft.value.texture,
                                            Some(texture),
                                            label,
                                        );
                                    }
                                });
                            if let Some(MaterialTexture::Image(path)) = &mut draft.value.texture {
                                ui.add(
                                    egui::TextEdit::singleline(path)
                                        .id_salt("source-material-image")
                                        .hint_text("Relative image path")
                                        .char_limit(1024),
                                );
                            }
                            ui.horizontal(|ui| {
                                ui.label("Shader");
                                if ui
                                    .selectable_label(
                                        matches!(draft.value.shader, MaterialShader::Inherit),
                                        "Inherit",
                                    )
                                    .clicked()
                                {
                                    draft.value.shader = MaterialShader::Inherit;
                                }
                                if ui
                                    .selectable_label(
                                        matches!(draft.value.shader, MaterialShader::Stock),
                                        "Stock",
                                    )
                                    .clicked()
                                {
                                    draft.value.shader = MaterialShader::Stock;
                                    draft.value.keywords.clear();
                                }
                                if ui.button("New graph").clicked() {
                                    draft.value.shader = MaterialShader::Graph(Default::default());
                                    draft.value.keywords.clear();
                                }
                                if ui
                                    .add_enabled(
                                        editor
                                            .selected_object()
                                            .is_some_and(|o| o.shader_graph.is_some()),
                                        egui::Button::new("Copy selected graph"),
                                    )
                                    .clicked()
                                {
                                    draft.value.shader = MaterialShader::Graph(
                                        editor
                                            .selected_object()
                                            .unwrap()
                                            .shader_graph
                                            .clone()
                                            .unwrap(),
                                    );
                                    draft.value.keywords.clear();
                                }
                            });
                            let graph = match &draft.value.shader {
                                MaterialShader::Graph(graph) => Some(graph),
                                MaterialShader::Inherit => editor
                                    .assets
                                    .material(&draft.id)
                                    .ok()
                                    .and_then(|m| m.shader.as_deref()),
                                MaterialShader::Stock => None,
                            };
                            if let Some(graph) = graph {
                                keywords(ui, &mut draft.value.keywords, &graph.keywords);
                            }
                        });
                    if let MaterialShader::Graph(graph) = &mut draft.value.shader {
                        draft.shader.toolbar(ui, graph, true);
                        ui.set_min_height(280.);
                        if let Some(error) = draft.shader.canvas(ui, graph, true) {
                            draft.error = Some(format!("{error:#}"));
                        }
                    }
                });
            });
        if record && draft.value != before && !draft.editing {
            draft.editing = true;
            draft.future.clear();
            draft.history.push(before);
            if draft.history.len() > 64 {
                draft.history.remove(0);
            }
        }
        if !ctx.input(|input| input.pointer.any_down()) && !ctx.egui_wants_keyboard_input() {
            draft.editing = false;
        }
        if close {
            self.draft = None;
        }
    }
}
