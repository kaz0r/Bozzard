//! Imported primitives become ordinary child objects; component editing needs no special ECS.
use super::*;

impl Editor {
    /// Convert a legacy whole-model drawable without losing its surface overrides.
    /// Linked instances must be unpacked first, just like any other structural edit.
    pub fn expand_model(&mut self, id: &str) -> Result<Vec<String>> {
        let mut scene = self.scene.clone();
        let children = self.expand_model_objects(&mut scene, id)?;
        self.finish_gesture();
        self.apply("Make model children independent", scene)?;
        Ok(children)
    }

    pub(super) fn expand_model_objects(&self, scene: &mut Scene, id: &str) -> Result<Vec<String>> {
        let original = scene
            .objects
            .iter()
            .find(|o| o.id == id)
            .context("model no longer exists")?
            .clone();
        let Some(drawable) = &original.drawable else {
            return Ok(Vec::new());
        };
        let Mesh::Asset(asset) = &drawable.mesh else {
            return Ok(Vec::new());
        };
        let Some(AssetData::Mesh(mesh)) = self
            .assets
            .handle(asset)
            .and_then(|h| self.assets.get(h))
            .and_then(|e| e.data())
        else {
            anyhow::bail!("load the model before making its children independent");
        };
        if mesh.parts.is_empty() {
            return Ok(Vec::new());
        }
        ensure!(
            !scene
                .prefabs
                .values()
                .any(|p| p.members.values().any(|member| member == id)),
            "Unpack this prefab before converting its model children, then save it as a prefab again"
        );
        let mut children = Vec::new();
        for (index, part) in mesh.parts.iter().enumerate() {
            let bounds = mesh
                .part_bounds(index)
                .context("cannot convert an empty model surface")?;
            let center = bounds[0] * 0.5 + bounds[1] * 0.5;
            let id = unique_id(scene, "surface");
            let mut geometry = original.effective_drawable().unwrap();
            geometry.mesh = Mesh::Surface {
                asset: asset.clone(),
                index: index as u32,
                source: part.source_key.clone(),
            };
            geometry
                .material_overrides
                .retain(|v| v.surface as usize == index && v.source == part.source_key);
            let mut transform = geometry
                .material_overrides
                .first()
                .map(|v| v.transform)
                .unwrap_or_default();
            transform.translation = (Vec3::from(transform.translation) + center).to_array();
            for value in &mut geometry.material_overrides {
                value.transform = Transform::default();
            }
            geometry.material_overrides.retain(|v| !v.is_inherited());
            let material = original
                .material
                .as_ref()
                .map(|_| bozzard_scene::Material::from_drawable(&geometry));
            scene.objects.push(Object {
                id: id.clone(),
                name: part.name.clone(),
                parent: Some(original.id.clone()),
                transform,
                drawable: Some(geometry),
                material,
                blueprints: Vec::new(),
                light: None,
                camera: None,
                spin: None,
                collider: None,
                mesh_collider: None,
                gravity: None,
                player_controller: None,
                trigger: None,
            });
            children.push(id);
        }
        let owner = scene
            .objects
            .iter_mut()
            .find(|o| o.id == original.id)
            .unwrap();
        owner.drawable = None;
        owner.material = None;
        Ok(children)
    }

    /// Primary editor picking promotes legacy surface rows to component-bearing children.
    /// Legacy surface selection remains available for scripts editing old override documents.
    pub fn select_component_pick(&mut self, pick: Option<Pick>) -> Result<()> {
        if let Some(Pick {
            object,
            surface: Some(index),
        }) = &pick
        {
            let children = self.expand_model(object)?;
            self.select_object(Some(
                children
                    .get(*index)
                    .context("surface no longer exists")?
                    .clone(),
            ));
            return Ok(());
        }
        self.select_pick(pick)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_entities_preserve_geometry_and_support_independent_components() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/model-lab.json");
        let mut editor = Editor::open(&path).unwrap();
        let mut scene = editor.scene().clone();
        scene
            .objects
            .retain(|o| o.id == "courier-gltf" || o.camera.is_some());
        for object in &mut scene.objects {
            object.spin = None;
            object.blueprints.clear();
        }
        editor.apply("Isolate model", scene).unwrap();
        let before = editor.scene().clone();
        let bounds = editor
            .frame_bounds(Layer::ThreeD, Some("courier-gltf"))
            .unwrap()
            .unwrap();
        let projection = editor.render(Layer::ThreeD, 1.).unwrap().view_projection;
        let picks: Vec<_> = (0..21)
            .flat_map(|y| (0..21).map(move |x| [x as f32 / 10. - 1., y as f32 / 10. - 1.]))
            .map(|ndc| {
                (
                    ndc,
                    editor
                        .pick_surface_with_projection(Layer::ThreeD, projection, ndc)
                        .unwrap(),
                )
            })
            .collect();
        let children = editor.expand_model("courier-gltf").unwrap();
        assert_eq!(children.len(), 10);
        assert!(
            editor
                .scene()
                .objects
                .iter()
                .filter(|o| children.contains(&o.id))
                .all(|o| o.material.is_none()
                    && o.collider.is_none()
                    && o.gravity.is_none()
                    && o.blueprints.is_empty())
        );
        let expanded = editor.scene().clone();
        let after_bounds = editor
            .frame_bounds(Layer::ThreeD, Some("courier-gltf"))
            .unwrap()
            .unwrap();
        assert!(
            bounds[0].abs_diff_eq(after_bounds[0], 1e-5)
                && bounds[1].abs_diff_eq(after_bounds[1], 1e-5)
        );
        assert!(picks.iter().any(|(_, hit)| hit.is_some()));
        for (ndc, before) in picks {
            let after = editor
                .pick_surface_with_projection(Layer::ThreeD, projection, ndc)
                .unwrap();
            assert_eq!(
                after,
                before.map(|hit| Pick {
                    object: children[hit.surface.unwrap()].clone(),
                    surface: None
                })
            );
        }
        let render = editor.render(Layer::ThreeD, 1.).unwrap();
        assert_eq!(render.items.len(), 10);
        assert!(
            render
                .items
                .iter()
                .all(|item| matches!(item.mesh, MeshKind::ModelPart(..)))
        );
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &before);
        editor.redo().unwrap();
        assert_eq!(editor.scene(), &expanded);
        assert_eq!(
            Scene::from_json(&expanded.to_json().unwrap()).unwrap(),
            expanded
        );
        let mut scene = expanded.clone();
        let child = scene
            .objects
            .iter_mut()
            .find(|o| o.id == children[0])
            .unwrap();
        child.transform.translation[1] += 10.;
        child.collider = Some(bozzard_scene::BoxCollider::default());
        child.gravity = Some(bozzard_scene::Gravity::default());
        let mut material = bozzard_scene::Material::from_drawable(child.drawable.as_ref().unwrap());
        material.color = [1., 0., 0.];
        material.metallic = Some(0.8);
        child.material = Some(material);
        child.blueprints.push(bozzard_scene::BlueprintAttachment {
            enabled: true,
            graph: bozzard_scene::Blueprint::spinning(),
        });
        editor.apply("Child components", scene.clone()).unwrap();
        editor.start_play().unwrap();
        let play = editor.play.as_mut().unwrap();
        let a = play.instance().entity(&children[0]).unwrap();
        let b = play.instance().entity(&children[1]).unwrap();
        let start = *play.app.world.get::<Transform>(a).unwrap();
        let sibling = *play.app.world.get::<Transform>(b).unwrap();
        play.app.step();
        play.check_simulation().unwrap();
        assert!(play.app.world.get::<Transform>(a).unwrap().translation[1] < start.translation[1]);
        assert_ne!(
            play.app.world.get::<Transform>(a).unwrap().rotation_degrees,
            start.rotation_degrees
        );
        assert_eq!(*play.app.world.get::<Transform>(b).unwrap(), sibling);
        assert!(play.app.world.get::<bozzard_scene::Material>(b).is_none());
        editor.stop_play();
        assert_eq!(editor.scene(), &scene);
        let mut stale = expanded;
        let child = stale
            .objects
            .iter_mut()
            .find(|o| o.id == children[0])
            .unwrap();
        if let Mesh::Surface { source, .. } = &mut child.drawable.as_mut().unwrap().mesh {
            *source = "0000000000000000".into();
        }
        editor.apply("Stale surface", stale).unwrap();
        assert_eq!(editor.render(Layer::ThreeD, 1.).unwrap().items.len(), 9);
    }
}
