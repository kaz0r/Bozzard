use super::*;
use bozzard_scene::SurfaceMaterialOverride;

impl Editor {
    /// Shared transform target for numeric controls and viewport gizmos.
    pub fn selected_transform(&self) -> Result<Transform> {
        if self.selected_surface().is_some() {
            Ok(self.selected_material_override()?.transform)
        } else {
            Ok(self
                .selected_object()
                .context("select an object")?
                .transform)
        }
    }
    pub fn selected_transform_parent(&self) -> Result<Mat4> {
        let object = self.selected_object().context("select an object")?;
        let matrices = self.scene.global_transforms()?;
        if let Some(pivot) = self.selected_surface_pivot() {
            Ok(matrices[&object.id] * Mat4::from_translation(pivot))
        } else {
            Ok(object
                .parent
                .as_ref()
                .map_or(Mat4::IDENTITY, |p| matrices[p]))
        }
    }
    pub fn set_selected_transform(&mut self, transform: Transform) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play before editing transforms");
        if self.selected_surface().is_some() {
            let mut value = self.selected_material_override()?;
            value.transform = transform;
            self.set_selected_material_override(value)
        } else {
            let mut scene = self.scene.clone();
            scene
                .objects
                .iter_mut()
                .find(|o| Some(&o.id) == self.selected.as_ref())
                .context("select an object")?
                .transform = transform;
            self.apply("Transform", scene)
        }
    }
    pub fn selected_material_override(&self) -> Result<SurfaceMaterialOverride> {
        let selected = self
            .selected_surface()
            .context("select an imported surface first")?;
        let drawable = self.selected_object().unwrap().drawable.as_ref().unwrap();
        Ok(drawable
            .material_overrides
            .iter()
            .find(|value| {
                value.surface as usize == selected.index && value.source == selected.part.source_key
            })
            .cloned()
            .unwrap_or_else(|| {
                SurfaceMaterialOverride::inherited(
                    selected.index as u32,
                    selected.part.source_key.clone(),
                )
            }))
    }

    /// Called inside the normal gesture boundary for slider drags, or as one command.
    pub fn set_selected_material_override(&mut self, value: SurfaceMaterialOverride) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play before editing materials");
        value.validate()?;
        let selected = self
            .selected_surface()
            .context("select an imported surface first")?;
        ensure!(
            value.surface as usize == selected.index && value.source == selected.part.source_key,
            "surface changed while editing its material"
        );
        ensure!(
            selected.part.shading.is_some()
                || (value.metallic.is_none() && value.roughness.is_none()),
            "metallic/roughness require a PBR surface"
        );
        let id = self.selected.as_ref().unwrap();
        let mut scene = self.scene.clone();
        let values = &mut scene
            .objects
            .iter_mut()
            .find(|o| &o.id == id)
            .unwrap()
            .drawable
            .as_mut()
            .unwrap()
            .material_overrides;
        values.retain(|old| old.surface != value.surface);
        if !value.is_inherited() {
            values.push(value);
        }
        values.sort_by_key(|v| v.surface);
        self.apply("Edit surface", scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    fn editor() -> Editor {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/model-lab.json");
        let mut editor = Editor::open(&path).unwrap();
        editor.select_object(Some("courier-gltf".into()));
        editor.select_surface(0).unwrap();
        editor
    }
    fn values(editor: &Editor) -> &[SurfaceMaterialOverride] {
        &editor
            .scene
            .objects
            .iter()
            .find(|o| o.id == "courier-gltf")
            .unwrap()
            .drawable
            .as_ref()
            .unwrap()
            .material_overrides
    }
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            // Save As retains relative references to the repository's fixture assets.
            // Windows CI can put the system temp directory on a different drive.
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/material-tests");
            std::fs::create_dir_all(&root).unwrap();
            loop {
                let path = root.join(format!(
                    "bozzard-materials-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("{e}"),
                }
            }
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn material_drag_is_one_undo_step_and_keeps_shared_assets_immutable() {
        let mut editor = editor();
        let original = editor.scene.clone();
        let asset = editor.assets.handle("courier-gltf").unwrap();
        let data = editor.assets.get(asset).unwrap().shared_data().unwrap();
        let revision = editor.asset_revision();
        let mut value = editor.selected_material_override().unwrap();
        editor.begin_gesture("Edit surface material");
        for roughness in [0.6, 0.4, 0.2] {
            value.tint = [0.5, 1.0, 0.25];
            value.metallic = Some(0.7);
            value.roughness = Some(roughness);
            editor
                .set_selected_material_override(value.clone())
                .unwrap();
        }
        editor.finish_gesture();
        assert_eq!(values(&editor), &[value.clone()]);
        assert!(Arc::ptr_eq(
            &data,
            &editor.assets.get(asset).unwrap().shared_data().unwrap()
        ));
        assert_eq!(editor.asset_revision(), revision);
        assert!(editor.dirty());
        let rendered = editor.render(Layer::ThreeD, 1.0).unwrap();
        assert_eq!(
            rendered
                .items
                .iter()
                .filter(|d| !d.material.surface_overrides.is_empty())
                .count(),
            1
        );
        editor.undo().unwrap();
        assert_eq!(editor.scene, original);
        assert!(editor.undo_label().is_none());
        editor.redo().unwrap();
        assert_eq!(values(&editor), &[value.clone()]);
        let reset = SurfaceMaterialOverride::inherited(value.surface, value.source);
        editor.set_selected_material_override(reset).unwrap();
        assert!(values(&editor).is_empty());
        editor.undo().unwrap();
        assert_eq!(values(&editor)[0].roughness, Some(0.2));
        editor.begin_gesture("Edit surface material");
        let mut next = editor.selected_material_override().unwrap();
        next.roughness = Some(0.9);
        editor.set_selected_material_override(next).unwrap();
        editor.cancel_gesture().unwrap();
        assert_eq!(values(&editor)[0].roughness, Some(0.2));
    }

    #[test]
    fn overrides_save_reopen_duplicate_and_play_without_mutating_source() {
        let mut editor = editor();
        let mut value = editor.selected_material_override().unwrap();
        value.tint = [0.25, 0.5, 1.0];
        value.metallic = Some(0.6);
        value.roughness = Some(0.15);
        value.transform = Transform {
            translation: [2., 1., -3.],
            rotation_degrees: [15., 45., 5.],
            scale: [-1., 2., 0.5],
        };
        value.texture = Some(Texture::Asset("courier-paint".into()));
        value.uv_scale = [2., 3.];
        editor
            .set_selected_material_override(value.clone())
            .unwrap();
        editor.select_object(Some("courier-gltf".into()));
        editor.assign_asset_to_selected("courier-gltf").unwrap();
        assert_eq!(values(&editor), &[value.clone()]);
        editor.duplicate().unwrap();
        let copy = editor.selected.clone().unwrap();
        editor.select_surface(0).unwrap();
        assert_eq!(editor.selected_material_override().unwrap(), value);
        let mut changed = value.clone();
        changed.tint = [1.0, 0.0, 0.0];
        editor.set_selected_material_override(changed).unwrap();
        assert_eq!(values(&editor), &[value.clone()]);
        let temp = Temp::new();
        let path = temp.0.join("saved.json");
        editor.save(&path).unwrap();
        let mut reopened = Editor::open(&path).unwrap();
        assert_eq!(values(&reopened), &[value.clone()]);
        assert_eq!(reopened.scene, editor.scene);
        reopened.select_object(Some(copy));
        reopened.select_surface(0).unwrap();
        assert_eq!(
            reopened.selected_material_override().unwrap().tint,
            [1.0, 0.0, 0.0]
        );
        let authored = reopened.scene.clone();
        let render = reopened.render(Layer::ThreeD, 1.0).unwrap();
        reopened.start_play().unwrap();
        assert!(reopened.set_selected_material_override(value).is_err());
        let play = reopened.render(Layer::ThreeD, 1.0).unwrap();
        assert_eq!(
            render
                .items
                .iter()
                .map(|i| i.material.surface_overrides.len())
                .collect::<Vec<_>>(),
            play.items
                .iter()
                .map(|i| i.material.surface_overrides.len())
                .collect::<Vec<_>>()
        );
        reopened.stop_play();
        assert_eq!(reopened.scene, authored);
    }

    #[test]
    fn assigning_a_texture_targets_only_the_surface_and_tracks_asset_dependencies() {
        let mut editor = editor();
        let owner = editor.selected_object().unwrap().clone();
        editor.assign_asset_to_selected("soft-sprite").unwrap();
        assert_eq!(
            editor.selected_material_override().unwrap().texture,
            Some(Texture::Asset("soft-sprite".into()))
        );
        assert_eq!(
            editor
                .selected_object()
                .unwrap()
                .drawable
                .as_ref()
                .unwrap()
                .texture,
            owner.drawable.unwrap().texture
        );
        assert!(editor.scene.asset_users()["soft-sprite"].contains(&"courier-gltf".into()));
        let mut repeated = editor.scene.clone();
        let values = &mut repeated
            .objects
            .iter_mut()
            .find(|o| o.id == "courier-gltf")
            .unwrap()
            .drawable
            .as_mut()
            .unwrap()
            .material_overrides;
        let mut extra = values[0].clone();
        extra.surface = 1;
        values.push(extra);
        assert_eq!(
            repeated.asset_users()["soft-sprite"]
                .iter()
                .filter(|id| *id == "courier-gltf")
                .count(),
            1
        );
        assert!(editor.remove_asset("soft-sprite").is_err());
        assert!(editor.assign_asset_to_selected("courier-glb").is_err());
        editor.undo().unwrap();
        assert!(editor.selected_material_override().unwrap().is_inherited());
        editor.redo().unwrap();
        let mut value = editor.selected_material_override().unwrap();
        value.texture = Some(Texture::White);
        editor.set_selected_material_override(value).unwrap();
        assert!(!editor.selected_material_override().unwrap().is_inherited());
    }

    #[test]
    fn invalid_edits_are_atomic_and_changing_mesh_clears_its_overrides() {
        let mut editor = editor();
        let inherited = editor.selected_material_override().unwrap();
        for bad in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            let mut value = inherited.clone();
            value.metallic = Some(bad);
            assert!(editor.set_selected_material_override(value).is_err());
        }
        let mut stale = inherited.clone();
        stale.source = "0000000000000000".into();
        stale.roughness = Some(0.8);
        assert!(
            editor
                .set_selected_material_override(stale.clone())
                .is_err()
        );
        for transform in [
            Transform {
                translation: [f32::NAN, 0., 0.],
                ..Default::default()
            },
            Transform {
                scale: [0., 1., 1.],
                ..Default::default()
            },
        ] {
            assert!(editor.set_selected_transform(transform).is_err());
        }
        for texture in [
            Texture::Asset("missing".into()),
            Texture::Asset("courier-gltf".into()),
        ] {
            let mut value = inherited.clone();
            value.texture = Some(texture);
            assert!(editor.set_selected_material_override(value).is_err());
        }
        let mut bad_uv = inherited.clone();
        bad_uv.uv_scale = [0., f32::INFINITY];
        assert!(editor.set_selected_material_override(bad_uv).is_err());
        assert!(!editor.dirty());
        assert!(editor.undo_label().is_none());
        let mut scene = editor.scene.clone();
        scene
            .objects
            .iter_mut()
            .find(|o| o.id == "courier-gltf")
            .unwrap()
            .drawable
            .as_mut()
            .unwrap()
            .material_overrides
            .push(stale);
        editor.apply("Loaded obsolete override", scene).unwrap();
        assert!(editor.selected_material_override().unwrap().is_inherited());
        editor.select_object(Some("courier-gltf".into()));
        editor.assign_asset_to_selected("courier-glb").unwrap();
        assert!(values(&editor).is_empty());
        editor.undo().unwrap();
        assert_eq!(values(&editor).len(), 1);
    }
}
