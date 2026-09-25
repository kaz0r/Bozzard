//! Atomic edits of a document selection. The UI supplies IDs; this module owns validation.
use super::*;
use bozzard_scene::{AddContext, FieldValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BulkTransformAxis {
    Position,
    Rotation,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BulkTransformMode {
    Absolute,
    Relative,
}

impl Editor {
    fn bulk_scene(&self, ids: &[String]) -> Result<Scene> {
        ensure!(
            self.play.is_none(),
            "Stop Play before editing the authored scene"
        );
        ensure!(!ids.is_empty(), "Select at least one object");
        let unique: BTreeSet<_> = ids.iter().collect();
        ensure!(
            unique.len() == ids.len(),
            "Selection contains duplicate objects"
        );
        for id in ids {
            ensure!(
                self.scene.objects.iter().any(|o| &o.id == id),
                "Object '{id}' no longer exists in this document"
            );
        }
        Ok(self.scene.clone())
    }

    pub fn bulk_set_field(
        &mut self,
        ids: &[String],
        component: &str,
        key: &str,
        value: FieldValue,
    ) -> Result<()> {
        let entry = bozzard_scene::component_type(component).context("Unknown component")?;
        ensure!(entry.field(key).is_some(), "Unknown component field");
        let mut scene = self.bulk_scene(ids)?;
        for id in ids {
            let object = scene.objects.iter_mut().find(|o| &o.id == id).unwrap();
            ensure!(
                (entry.present)(object),
                "{} is missing from '{}'",
                entry.label,
                object.name
            );
            ensure!(
                entry
                    .visible_fields(object)
                    .iter()
                    .any(|field| field.key == key),
                "{} is hidden on '{}'",
                key,
                object.name
            );
            (entry.set)(object, key, value.clone())?;
        }
        self.apply(
            &format!("Edit {} on {} objects", entry.label, ids.len()),
            scene,
        )
    }

    pub fn bulk_transform(
        &mut self,
        ids: &[String],
        axis: BulkTransformAxis,
        mode: BulkTransformMode,
        value: [f32; 3],
    ) -> Result<()> {
        ensure!(
            value.iter().all(|v| v.is_finite()),
            "Transform values must be finite"
        );
        let mut scene = self.bulk_scene(ids)?;
        for id in ids {
            let object = scene.objects.iter_mut().find(|o| &o.id == id).unwrap();
            let target = match axis {
                BulkTransformAxis::Position => &mut object.transform.translation,
                BulkTransformAxis::Rotation => &mut object.transform.rotation_degrees,
                BulkTransformAxis::Scale => &mut object.transform.scale,
            };
            for i in 0..3 {
                target[i] = match mode {
                    BulkTransformMode::Absolute => value[i],
                    BulkTransformMode::Relative => target[i] + value[i],
                };
            }
        }
        self.apply(&format!("Transform {} objects", ids.len()), scene)
    }

    pub fn bulk_add_component(
        &mut self,
        ids: &[String],
        component: &str,
        layer: Layer,
    ) -> Result<()> {
        let entry = bozzard_scene::component_type(component).context("Unknown component")?;
        let mut scene = self.bulk_scene(ids)?;
        let mut added = 0;
        for id in ids {
            let index = scene.objects.iter().position(|o| &o.id == id).unwrap();
            let original = &scene.objects[index];
            if (entry.present)(original) {
                continue;
            }
            ensure!(
                (entry.available)(original),
                "{} is unavailable on '{}'",
                entry.label,
                original.name
            );
            let bounds = original
                .drawable
                .as_ref()
                .and_then(|d| self.assets.mesh_surface(&d.mesh))
                .map(|(_, b)| b.map(|v| v.to_array()));
            let cooked = if component == "mesh_collider" {
                Some(
                    self.assets.cook_mesh_collider(
                        original
                            .drawable
                            .as_ref()
                            .context("Mesh Collider needs a Mesh Renderer")?,
                    )?,
                )
            } else {
                None
            };
            let mut object = original.clone();
            (entry.add)(
                &mut object,
                &AddContext {
                    layer,
                    scene: &scene,
                    bounds,
                    cooked,
                },
            )?;
            scene.objects[index] = object;
            added += 1;
        }
        ensure!(
            added > 0,
            "{} is already present on every selected object",
            entry.label
        );
        self.finish_gesture();
        self.apply(
            &format!("Add {} to {} objects", entry.label, ids.len()),
            scene,
        )
    }

    pub fn bulk_remove_component(&mut self, ids: &[String], component: &str) -> Result<()> {
        let entry = bozzard_scene::component_type(component).context("Unknown component")?;
        let mut scene = self.bulk_scene(ids)?;
        let mut removed = 0;
        for id in ids {
            let index = scene.objects.iter().position(|o| &o.id == id).unwrap();
            if !(entry.present)(&scene.objects[index]) {
                continue;
            }
            let mut object = scene.objects[index].clone();
            (entry.remove)(&mut object, &mut scene);
            scene.objects[index] = object;
            removed += 1;
        }
        ensure!(
            removed > 0,
            "{} is missing from every selected object",
            entry.label
        );
        self.finish_gesture();
        self.apply(
            &format!("Remove {} from {} objects", entry.label, ids.len()),
            scene,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> Editor {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/first-trail.json");
        Editor::open(&path).unwrap()
    }

    #[test]
    fn transforms_are_one_undo_step_and_reject_invalid_selection_atomically() {
        let mut editor = editor();
        let ids: Vec<_> = editor
            .scene()
            .objects
            .iter()
            .take(2)
            .map(|o| o.id.clone())
            .collect();
        let before = editor.scene().clone();
        editor
            .bulk_transform(
                &ids,
                BulkTransformAxis::Position,
                BulkTransformMode::Relative,
                [1., 2., 3.],
            )
            .unwrap();
        for id in &ids {
            let old = before.objects.iter().find(|o| &o.id == id).unwrap();
            let new = editor.scene().objects.iter().find(|o| &o.id == id).unwrap();
            assert_eq!(
                new.transform.translation,
                std::array::from_fn(|i| old.transform.translation[i] + [1., 2., 3.][i])
            );
        }
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &before);
        let mut invalid = ids.clone();
        invalid.push("missing".into());
        assert!(
            editor
                .bulk_transform(
                    &invalid,
                    BulkTransformAxis::Scale,
                    BulkTransformMode::Absolute,
                    [2.; 3]
                )
                .is_err()
        );
        assert_eq!(editor.scene(), &before);
    }

    #[test]
    fn ten_lights_share_one_validated_intensity_command() {
        let mut scene = Scene::from_json(
            r#"{"version":1,"name":"Lights","views":{},"objects":[
            {"id":"light","name":"Light","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
             "light":{"kind":"point","intensity":10}}]}"#,
        ).unwrap();
        let original = scene.objects[0].clone();
        scene.objects = (0..10)
            .map(|n| {
                let mut object = original.clone();
                object.id = format!("light-{n}");
                object.name = format!("Light {n}");
                object
            })
            .collect();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/bulk-lights-test.json");
        let mut editor = Editor::new(scene.clone(), &path).unwrap();
        let ids: Vec<_> = scene.objects.iter().map(|o| o.id.clone()).collect();
        editor
            .bulk_set_field(&ids, "light", "intensity", FieldValue::Number(25.))
            .unwrap();
        assert!(
            editor
                .scene()
                .objects
                .iter()
                .all(|o| o.light.unwrap().intensity == 25.)
        );
        let applied = editor.scene().clone();
        assert!(
            editor
                .bulk_set_field(
                    &ids,
                    "light",
                    "intensity",
                    FieldValue::Text("invalid".into())
                )
                .is_err()
        );
        assert_eq!(editor.scene(), &applied);
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &scene);
    }
}
