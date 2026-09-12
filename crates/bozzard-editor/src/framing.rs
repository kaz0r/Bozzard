use super::*;

impl Editor {
    /// Authored world bounds for drawable objects in a layer. A selected group includes
    /// descendants; a selection without geometry falls back to its world-space origin.
    pub fn frame_bounds(&self, layer: Layer, selection: Option<&str>) -> Result<Option<[Vec3; 2]>> {
        let demo = SceneDemo::new(&self.scene)?;
        let matrices = demo.instance().global_transforms(&demo.app.world)?;
        let mut included = BTreeSet::new();
        if let Some(id) = selection {
            ensure!(
                matrices.contains_key(id),
                "selected object no longer exists"
            );
            included.insert(id.to_owned());
            loop {
                let count = included.len();
                for object in &self.scene.objects {
                    if object.parent.as_ref().is_some_and(|p| included.contains(p)) {
                        included.insert(object.id.clone());
                    }
                }
                if count == included.len() {
                    break;
                }
            }
        }
        let mut bounds: Option<[Vec3; 2]> = None;
        let mut include = |point: Vec3| -> Result<()> {
            ensure!(point.is_finite(), "non-finite framing bounds");
            if let Some([min, max]) = &mut bounds {
                *min = min.min(point);
                *max = max.max(point);
            } else {
                bounds = Some([point, point]);
            }
            Ok(())
        };
        for object in &self.scene.objects {
            if selection.is_some() && !included.contains(&object.id) {
                continue;
            }
            if let Some(text) = &object.text_rendering
                && text.enabled
                && text.layer == layer
                && let Some([min, max]) =
                    bozzard_render::text_bounds(&bozzard_render_assets::text_mesh(text))?
            {
                for x in [min.x, max.x] {
                    for y in [min.y, max.y] {
                        include(matrices[&object.id].transform_point3(Vec3::new(x, y, 0.)))?;
                    }
                }
            }
            let Some(drawable) = &object.drawable else {
                continue;
            };
            if drawable.layer != layer {
                continue;
            }
            let matrix = matrices[&object.id];
            match &drawable.mesh {
                Mesh::Surface { .. } => {
                    let Some((_, bounds)) = self.assets.mesh_surface(&drawable.mesh) else {
                        continue;
                    };
                    let center = bounds[0] * 0.5 + bounds[1] * 0.5;
                    for i in 0..8 {
                        include(matrix.transform_point3(
                            Vec3::new(
                                bounds[i & 1].x,
                                bounds[(i >> 1) & 1].y,
                                bounds[(i >> 2) & 1].z,
                            ) - center,
                        ))?;
                    }
                }
                Mesh::Asset(id) => {
                    let data = self
                        .assets
                        .handle(id)
                        .and_then(|h| self.assets.get(h))
                        .and_then(|entry| entry.data())
                        .context("mesh unavailable for framing")?;
                    let AssetData::Mesh(mesh) = data else {
                        anyhow::bail!("framing asset is not a mesh");
                    };
                    if mesh.parts.is_empty() {
                        for vertex in &mesh.vertices {
                            include(matrix.transform_point3(Vec3::from_slice(&vertex[..3])))?;
                        }
                    } else {
                        for (index, part) in mesh.parts.iter().enumerate() {
                            let mut model = matrix;
                            if let Some(value) = drawable.material_overrides.iter().find(|v| {
                                v.surface as usize == index && v.source == part.source_key
                            }) {
                                let bounds = mesh.part_bounds(index).context("empty surface")?;
                                model *= value.matrix(bounds[0] * 0.5 + bounds[1] * 0.5);
                            }
                            for i in &mesh.indices
                                [part.start as usize..(part.start + part.count) as usize]
                            {
                                include(model.transform_point3(Vec3::from_slice(
                                    &mesh.vertices[*i as usize][..3],
                                )))?;
                            }
                        }
                    }
                }
                mesh => {
                    let depth = if *mesh == Mesh::Quad { 0.0 } else { 0.5 };
                    for x in [-0.5, 0.5] {
                        for y in [-0.5, 0.5] {
                            for z in [-depth, depth] {
                                include(matrix.transform_point3(Vec3::new(x, y, z)))?;
                            }
                        }
                    }
                }
            }
        }
        if bounds.is_none()
            && let Some(id) = selection
        {
            let point = matrices[id].transform_point3(Vec3::ZERO);
            ensure!(point.is_finite(), "non-finite selection origin");
            bounds = Some([point, point]);
        }
        Ok(bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_bounds_include_transformed_descendants_and_filter_layers() {
        let scene = Scene::from_json(r#"{"version":1,"name":"Frame","views":{},"objects":[
          {"id":"group","name":"Group","transform":{"translation":[10,0,0],"rotation_degrees":[0,0,0],"scale":[2,1,1]}},
          {"id":"child","name":"Child","parent":"group","transform":{"translation":[1,0,0],"rotation_degrees":[0,0,0],"scale":[-1,2,1]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}},
          {"id":"outside","name":"Outside","transform":{"translation":[100,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[1,1,1],"uv_scale":[1,1]}}
        ]}"#).unwrap();
        let editor = Editor::new(scene, Path::new("work/frame-test.json")).unwrap();
        assert_eq!(
            editor.frame_bounds(Layer::ThreeD, Some("group")).unwrap(),
            Some([Vec3::new(11.0, -1.0, -0.5), Vec3::new(13.0, 1.0, 0.5)])
        );
        assert_eq!(
            editor.frame_bounds(Layer::ThreeD, None).unwrap().unwrap()[1].x,
            100.5
        );
        assert!(editor.frame_bounds(Layer::TwoD, None).unwrap().is_none());
        assert_eq!(
            editor.frame_bounds(Layer::TwoD, Some("group")).unwrap(),
            Some([Vec3::new(10.0, 0.0, 0.0); 2])
        );
        assert!(editor.frame_bounds(Layer::ThreeD, Some("missing")).is_err());
        assert!(!editor.dirty());
    }
}
