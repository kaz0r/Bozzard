//! Editor extensions remain outside the headless scene/component registry.
use anyhow::{Context as _, Result, ensure};
use bozzard_assets::AssetStore;
use bozzard_scene::{Object, Scene};
use eframe::egui;
use serde_json::Value;
use std::collections::BTreeMap;

/// Read-only context available to a component inspector.
pub struct Context<'a> {
    pub object: &'a Object,
    pub scene: &'a Scene,
    pub assets: &'a AssetStore,
}
/// Edit only the component's serialized value. Return an error to discard that edit.
/// The editor loads it through the component schema, validates the scene and records Undo.
pub type Inspector = fn(&mut egui::Ui, &mut Value, Context<'_>) -> Result<()>;

pub struct Registry {
    entries: BTreeMap<&'static str, Inspector>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            entries: BTreeMap::from([("spin", spin as Inspector)]),
        }
    }
}
impl Registry {
    /// Empty registry; components with field metadata retain their generic inspector.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
    /// Register after `bozzard_scene::register_component` and before starting the editor.
    pub fn register(&mut self, component: &'static str, inspector: Inspector) -> Result<()> {
        ensure!(
            bozzard_scene::component_type(component).is_some(),
            "unknown component {component}"
        );
        ensure!(
            !self.entries.contains_key(component),
            "inspector already registered for {component}"
        );
        ensure!(self.entries.len() < 256, "custom inspector limit reached");
        self.entries.insert(component, inspector);
        Ok(())
    }
    pub(crate) fn contains(&self, component: &str) -> bool {
        self.entries.contains_key(component)
    }
    pub(crate) fn draw(
        &self,
        component: &str,
        ui: &mut egui::Ui,
        object: &mut Object,
        scene: &Scene,
        assets: &AssetStore,
    ) -> Result<bool> {
        let Some(draw) = self.entries.get(component) else {
            return Ok(false);
        };
        let entry = bozzard_scene::component_type(component)
            .context("custom inspector component was removed")?;
        let Some(original) = (entry.save)(object)? else {
            return Ok(false);
        };
        let mut value = original.clone();
        ui.push_id(("custom-inspector", component), |ui| {
            draw(
                ui,
                &mut value,
                Context {
                    object,
                    scene,
                    assets,
                },
            )
        })
        .inner?;
        if ui.is_enabled() && value != original {
            let mut candidate = object.clone();
            (entry.load)(&mut candidate, value)?;
            *object = candidate;
        }
        Ok(true)
    }
}

/// Built-in example of a custom inspector over an existing typed component.
fn spin(ui: &mut egui::Ui, value: &mut Value, _: Context<'_>) -> Result<()> {
    let mut rate: bozzard_scene::Spin = serde_json::from_value(value.clone())?;
    let mut changed = false;
    ui.small("ROTATION RATE · degrees per second");
    ui.horizontal(|ui| {
        for (axis, speed) in ["X ", "Y ", "Z "].into_iter().zip(&mut rate.0) {
            changed |= ui
                .add(
                    egui::DragValue::new(speed)
                        .speed(1.)
                        .range(-36_000.0..=36_000.0)
                        .prefix(axis),
                )
                .changed();
        }
    });
    if ui.button("One turn per second around Y").clicked() {
        rate.0 = [0., 360., 0.];
        changed = true;
    }
    if ui.small_button("Stop rotation").clicked() {
        rate.0 = [0.; 3];
        changed = true;
    }
    if changed {
        *value = serde_json::to_value(rate)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_edits_use_schema_history_and_round_trip_and_errors_leave_objects_unchanged()
    -> Result<()> {
        let scene = Scene::from_json(
            r#"{"version":1,"name":"Inspectors","views":{},"objects":[{"id":"a","name":"A","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"spin":[0,20,0]}]}"#,
        )?;
        let path = std::env::temp_dir().join("bozzard-custom-inspector-scene.json");
        let mut editor = bozzard_editor::Editor::new(scene.clone(), &path)?;
        let mut registry = Registry::new();
        registry.register("spin", |_, value, context| {
            assert_eq!(context.object.id, "a");
            *value = serde_json::json!([0., 360., 0.]);
            Ok(())
        })?;
        assert!(registry.register("spin", spin).is_err());
        assert!(registry.register("missing", spin).is_err());
        let ctx = egui::Context::default();
        let mut object = scene.objects[0].clone();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            registry
                .draw("spin", ui, &mut object, &scene, &editor.assets)
                .unwrap();
        });
        output.textures_delta.clear();
        assert_eq!(object.spin.unwrap().0, [0., 360., 0.]);
        let mut changed = scene.clone();
        changed.objects[0] = object.clone();
        editor.apply("Custom inspector", changed.clone())?;
        editor.undo()?;
        assert_eq!(editor.scene(), &scene);
        editor.redo()?;
        assert_eq!(editor.scene(), &changed);
        assert_eq!(Scene::from_json(&editor.scene().to_json()?)?, changed);
        for inspector in [
            (|_: &mut egui::Ui, value: &mut Value, _: Context<'_>| {
                *value = Value::Null;
                Ok(())
            }) as Inspector,
            |_, value, _| {
                *value = Value::Null;
                anyhow::bail!("rejected")
            },
        ] {
            let mut invalid = Registry::new();
            invalid.register("spin", inspector)?;
            let mut output = ctx.run_ui(Default::default(), |ui| {
                assert!(
                    invalid
                        .draw("spin", ui, &mut object, &scene, &editor.assets)
                        .is_err()
                );
            });
            output.textures_delta.clear();
            assert_eq!(object, changed.objects[0]);
        }
        Ok(())
    }
}
