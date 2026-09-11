use super::*;
use bozzard_scene::SurfaceMaterialOverride;

impl Editor {
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
        self.apply("Edit surface material", scene)
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
            loop {
                let path = std::env::temp_dir().join(format!(
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
