use super::*;
use bozzard_assets::{MeshData, MeshPart};
use std::sync::{Arc, Weak};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub object: String,
    pub surface: Option<usize>,
}

pub(crate) struct SurfaceSelection {
    object: String,
    asset: String,
    index: usize,
    data: Weak<AssetData>,
    bounds: [Vec3; 2],
}

pub struct SelectedSurface<'a> {
    pub index: usize,
    pub mesh: &'a MeshData,
    pub part: &'a MeshPart,
}

impl Editor {
    /// Called after publishing a catalog or returning from document operations.
    pub fn repair_surface_selection(&mut self) {
        if self.selected_surface().is_none() {
            self.surface_selection = None;
        }
    }
    /// Selecting a hierarchy object returns to whole-object editing.
    pub fn select_object(&mut self, id: Option<String>) {
        self.surface_selection = None;
        self.selected = id;
    }
    pub fn selected_mesh(&self) -> Option<&MeshData> {
        self.object_mesh(self.selected.as_deref()?)
    }
    /// Loaded imported geometry for hierarchy rows, without changing selection.
    pub fn object_mesh(&self, object: &str) -> Option<&MeshData> {
        let object = self.scene.objects.iter().find(|o| o.id == object)?;
        let Mesh::Asset(id) = &object.drawable.as_ref()?.mesh else {
            return None;
        };
        match self.assets.get(self.assets.handle(id)?)?.data()? {
            AssetData::Mesh(mesh) => Some(mesh),
            _ => None,
        }
    }
    /// Transient inspection selection. It never mutates the scene or history.
    pub fn select_surface(&mut self, index: usize) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play before inspecting a surface");
        let object = self.selected_object().context("select a model first")?;
        let Mesh::Asset(asset) = &object
            .drawable
            .as_ref()
            .context("object has no drawable")?
            .mesh
        else {
            anyhow::bail!("selected object is not an imported model");
        };
        let data = self
            .assets
            .get(self.assets.handle(asset).context("missing mesh")?)
            .and_then(|e| e.shared_data())
            .context("mesh not loaded")?;
        let AssetData::Mesh(mesh) = data.as_ref() else {
            anyhow::bail!("asset is not a mesh");
        };
        let bounds = mesh
            .part_bounds(index)
            .context("surface no longer exists or is empty")?;
        self.surface_selection = Some(SurfaceSelection {
            object: object.id.clone(),
            asset: asset.clone(),
            index,
            bounds,
            data: Arc::downgrade(&data),
        });
        Ok(())
    }
    pub fn select_pick(&mut self, pick: Option<Pick>) -> Result<()> {
        let surface = pick.as_ref().and_then(|p| p.surface);
        self.select_object(pick.map(|p| p.object));
        if let Some(index) = surface {
            self.select_surface(index)?;
        }
        Ok(())
    }
    /// A reload must not silently reinterpret a primitive index as another surface.
    /// Weak identity avoids retaining large decoded model data after replacement.
    pub fn selected_surface(&self) -> Option<SelectedSurface<'_>> {
        if self.play.is_some() {
            return None;
        }
        let selection = self.surface_selection.as_ref()?;
        let object = self.selected_object()?;
        let Mesh::Asset(asset) = &object.drawable.as_ref()?.mesh else {
            return None;
        };
        if object.id != selection.object || *asset != selection.asset {
            return None;
        }
        let entry = self.assets.get(self.assets.handle(&selection.asset)?)?;
        let data = entry.shared_data()?;
        if !Weak::ptr_eq(&selection.data, &Arc::downgrade(&data)) {
            return None;
        }
        let AssetData::Mesh(mesh) = entry.data()? else {
            return None;
        };
        Some(SelectedSurface {
            index: selection.index,
            mesh,
            part: mesh.parts.get(selection.index)?,
        })
    }
    pub fn selected_surface_pivot(&self) -> Option<Vec3> {
        self.selected_surface()?;
        let bounds = self.surface_selection.as_ref()?.bounds;
        Some(bounds[0] * 0.5 + bounds[1] * 0.5)
    }
    pub fn selected_surface_corners(&self, layer: Layer) -> Result<Option<[Vec3; 8]>> {
        if self.selected_surface().is_none() {
            return Ok(None);
        }
        let object = self.selected_object().unwrap();
        if object.drawable.as_ref().unwrap().layer != layer {
            return Ok(None);
        }
        let selection = self.surface_selection.as_ref().unwrap();
        let demo = SceneDemo::new(&self.scene)?;
        let transform = demo.instance().global_transforms(&demo.app.world)?[&object.id]
            * self
                .selected_material_override()?
                .matrix(self.selected_surface_pivot().unwrap());
        Ok(Some(std::array::from_fn(|i| {
            transform.transform_point3(Vec3::new(
                selection.bounds[i & 1].x,
                selection.bounds[(i >> 1) & 1].y,
                selection.bounds[(i >> 2) & 1].z,
            ))
        })))
    }
    pub fn frame_selection_bounds(&self, layer: Layer) -> Result<Option<[Vec3; 2]>> {
        if let Some(corners) = self.selected_surface_corners(layer)? {
            return Ok(Some(corners.into_iter().fold(
                [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)],
                |[min, max], p| [min.min(p), max.max(p)],
            )));
        }
        self.frame_bounds(
            layer,
            Some(
                self.selected
                    .as_deref()
                    .context("select an object to frame")?,
            ),
        )
    }
    /// Ray selection against actual triangle geometry, including imported meshes.
    pub fn pick(&self, layer: Layer, aspect: f32, ndc: [f32; 2]) -> Result<Option<String>> {
        let projection = self.render(layer, aspect)?.view_projection;
        self.pick_with_projection(layer, projection, ndc)
    }
    pub fn pick_with_projection(
        &self,
        layer: Layer,
        projection: Mat4,
        ndc: [f32; 2],
    ) -> Result<Option<String>> {
        Ok(self
            .pick_surface_with_projection(layer, projection, ndc)?
            .map(|hit| hit.object))
    }
    /// The nearest triangle's imported surface, or the whole procedural object.
    pub fn pick_surface_with_projection(
        &self,
        layer: Layer,
        projection: Mat4,
        ndc: [f32; 2],
    ) -> Result<Option<Pick>> {
        self.pick_surface_impl(layer, projection, ndc, false)
    }
    /// Diagnostic reference retaining the full triangle scan for correctness/performance checks.
    pub fn pick_surface_reference_with_projection(
        &self,
        layer: Layer,
        projection: Mat4,
        ndc: [f32; 2],
    ) -> Result<Option<Pick>> {
        self.pick_surface_impl(layer, projection, ndc, true)
    }
    fn pick_surface_impl(
        &self,
        layer: Layer,
        projection: Mat4,
        ndc: [f32; 2],
        reference: bool,
    ) -> Result<Option<Pick>> {
        let demo = SceneDemo::new(&self.scene)?;
        let inv = projection.inverse();
        let origin = inv.project_point3(Vec3::new(ndc[0], ndc[1], 0.0));
        let direction = (inv.project_point3(Vec3::new(ndc[0], ndc[1], 1.0)) - origin).normalize();
        let matrices = demo.instance().global_transforms(&demo.app.world)?;
        let mut best: Option<(f32, Pick)> = None;
        for object in &self.scene.objects {
            let inverse = matrices[&object.id].inverse();
            let o = inverse.transform_point3(origin);
            let d = inverse.transform_vector3(direction);
            if let Some(text) = &object.text_rendering
                && text.enabled
                && text.color[3] > 0.
                && text.layer == layer
                && d.z.abs() >= 1e-8
                && let Some([min, max]) =
                    bozzard_render::text_bounds(&bozzard_render_assets::text_mesh(text))?
            {
                let t = -o.z / d.z;
                let p = o + d * t;
                if t > 0.
                    && p.x >= min.x
                    && p.x <= max.x
                    && p.y >= min.y
                    && p.y <= max.y
                    && best.as_ref().is_none_or(|(distance, _)| t < *distance)
                {
                    best = Some((
                        t,
                        Pick {
                            object: object.id.clone(),
                            surface: None,
                        },
                    ));
                }
            }
            let Some(drawable) = &object.drawable else {
                continue;
            };
            if drawable.layer != layer {
                continue;
            }
            let hit = match &drawable.mesh {
                Mesh::Quad => {
                    if d.z.abs() < 1e-8 {
                        None
                    } else {
                        let t = -o.z / d.z;
                        let p = o + d * t;
                        if t > 0.0 && p.x.abs() <= 0.5 && p.y.abs() <= 0.5 {
                            Some((t, None))
                        } else {
                            None
                        }
                    }
                }
                Mesh::Cube => ray_box(o, d).map(|t| (t, None)),
                Mesh::Surface { asset, .. } => {
                    let Some((part, bounds)) = self.assets.mesh_surface(&drawable.mesh) else {
                        continue;
                    };
                    let entry = self.assets.get(self.assets.handle(asset).unwrap()).unwrap();
                    let center = bounds[0] * 0.5 + bounds[1] * 0.5;
                    entry
                        .raycast_filtered(
                            o + center,
                            d,
                            |triangle| {
                                (part.start..part.start + part.count).contains(&(triangle * 3))
                            },
                            reference,
                        )
                        .map(|hit| (hit.distance, None))
                }
                Mesh::Asset(id) => self
                    .assets
                    .handle(id)
                    .and_then(|h| self.assets.get(h))
                    .and_then(|entry| {
                        let AssetData::Mesh(mesh) = entry.data()? else {
                            return None;
                        };
                        // ponytail: reuse the immutable BVH for each moved part; per-part BVHs
                        // are only needed if many edited surfaces make picking measurably slow.
                        let edited: Vec<_> = drawable
                            .material_overrides
                            .iter()
                            .filter_map(|v| {
                                let part = mesh.parts.get(v.surface as usize)?;
                                (part.source_key == v.source && v.transform != Transform::default())
                                    .then_some((v, part))
                            })
                            .collect();
                        let hit = if edited.is_empty() {
                            if reference {
                                entry.raycast_reference(o, d)
                            } else {
                                entry.raycast(o, d)
                            }
                        } else {
                            let mut hit = entry.raycast_filtered(
                                o,
                                d,
                                |triangle| {
                                    !edited.iter().any(|(_, part)| {
                                        (part.start..part.start + part.count)
                                            .contains(&(triangle * 3))
                                    })
                                },
                                reference,
                            );
                            for (value, part) in edited {
                                let bounds = mesh.part_bounds(value.surface as usize)?;
                                let inverse =
                                    value.matrix(bounds[0] * 0.5 + bounds[1] * 0.5).inverse();
                                let candidate = entry.raycast_filtered(
                                    inverse.transform_point3(o),
                                    inverse.transform_vector3(d),
                                    |triangle| {
                                        (part.start..part.start + part.count)
                                            .contains(&(triangle * 3))
                                    },
                                    reference,
                                );
                                if let Some(candidate) = candidate
                                    && hit.is_none_or(|old| {
                                        candidate.distance < old.distance
                                            || (candidate.distance == old.distance
                                                && candidate.triangle < old.triangle)
                                    })
                                {
                                    hit = Some(candidate);
                                }
                            }
                            hit
                        }?;
                        let index = hit.triangle as usize * 3;
                        Some((
                            hit.distance,
                            mesh.parts.iter().position(|part| {
                                index >= part.start as usize
                                    && index < part.start as usize + part.count as usize
                            }),
                        ))
                    }),
            };
            if let Some((t, surface)) = hit
                && best.as_ref().is_none_or(|(distance, _)| t < *distance)
            {
                best = Some((
                    t,
                    Pick {
                        object: object.id.clone(),
                        surface,
                    },
                ));
            }
        }
        Ok(best.map(|(_, id)| id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = loop {
                let path = std::env::temp_dir().join(format!(
                    "bozzard-surfaces-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => break path,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("{e}"),
                }
            };
            std::fs::write(path.join("parts.obj"), "mtllib parts.mtl\nv -3 -1 0\nv -1 -1 0\nv -2 1 0\nv 1 -1 0\nv 3 -1 0\nv 2 1 0\no Left\nusemtl LeftPaint\nf 1 2 3\no Right\nusemtl RightPaint\nf 4 5 6\n").unwrap();
            std::fs::write(
                path.join("parts.mtl"),
                "newmtl LeftPaint\nKd 1 0 0\nnewmtl RightPaint\nKd 0 1 0\n",
            )
            .unwrap();
            Self(path)
        }
        fn editor(&self) -> Editor {
            let scene = Scene::from_json(r#"{"version":1,"name":"Surfaces","views":{},"assets":{"model":{"kind":"mesh","path":"parts.obj"}},"objects":[{"id":"model","name":"Model","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"drawable":{"layer":"3d","mesh":{"asset":"model"},"texture":"white","color":[1,1,1],"uv_scale":[1,1]}}]}"#).unwrap();
            Editor::new(scene, &self.0.join("scene.json")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn projection() -> Mat4 {
        glam::camera::rh::proj::directx::orthographic(-12.0, 12.0, -8.0, 8.0, 0.1, 100.0)
            * glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Y)
    }

    #[test]
    fn edited_surface_pivots_bounds_and_bvh_picking_follow_parented_transforms() {
        let fixture = Fixture::new();
        let mut editor = fixture.editor();
        let mut scene = editor.scene.clone();
        let mut parent = scene.objects[0].clone();
        parent.id = "parent".into();
        parent.drawable = None;
        parent.transform.rotation_degrees = [12., 27., -18.];
        parent.transform.scale = [-2., 1.5, 0.7];
        scene.objects[0].parent = Some(parent.id.clone());
        scene.objects.push(parent);
        editor.apply("Parent model", scene).unwrap();
        editor.select_object(Some("model".into()));
        editor.select_surface(1).unwrap();
        let sibling = editor.selected_surface_corners(Layer::ThreeD).unwrap();
        editor.select_surface(0).unwrap();
        let original = editor.scene.clone();
        let data = editor
            .assets
            .get(editor.assets.handle("model").unwrap())
            .unwrap()
            .shared_data()
            .unwrap();
        let pivot = editor.selected_surface_pivot().unwrap();
        let owner = editor.scene.global_transforms().unwrap()["model"];
        assert_eq!(
            editor.selected_transform_parent().unwrap(),
            owner * Mat4::from_translation(pivot)
        );
        let mut transform = Transform {
            translation: [6., 2., 4.],
            rotation_degrees: [20., 25., 35.],
            scale: [-0.7, 1.4, 1.],
        };
        editor.begin_gesture("Move surface");
        for x in [4., 5., 6.] {
            transform.translation[0] = x;
            editor.set_selected_transform(transform).unwrap();
        }
        editor.finish_gesture();
        assert_eq!(
            editor.selected_object().unwrap().transform,
            original.objects[0].transform
        );
        let value = editor.selected_material_override().unwrap();
        let model = owner * value.matrix(pivot);
        let center = model.transform_point3(pivot);
        let old_center = owner.transform_point3(pivot);
        let old_normal = owner
            .inverse()
            .transpose()
            .transform_vector3(Vec3::Z)
            .normalize();
        let old_view = glam::camera::rh::proj::directx::orthographic(-8., 8., -8., 8., 0.1, 100.)
            * glam::camera::rh::view::look_at_mat4(
                old_center + old_normal * 10.,
                old_center,
                Vec3::Y,
            );
        assert!(
            editor
                .pick_surface_with_projection(Layer::ThreeD, old_view, [0., 0.])
                .unwrap()
                .is_none(),
            "old surface location still intercepts clicks"
        );
        let corners = editor
            .selected_surface_corners(Layer::ThreeD)
            .unwrap()
            .unwrap();
        assert!(((corners[0] + corners[7]) * 0.5 - center).length() < 1e-5);
        let framing = editor
            .frame_bounds(Layer::ThreeD, Some("model"))
            .unwrap()
            .unwrap();
        assert!((0..3).all(|a| center[a] >= framing[0][a] && center[a] <= framing[1][a]));
        let normal = model
            .inverse()
            .transpose()
            .transform_vector3(Vec3::Z)
            .normalize();
        let projection = glam::camera::rh::proj::directx::orthographic(-8., 8., -8., 8., 0.1, 100.)
            * glam::camera::rh::view::look_at_mat4(center + normal * 10., center, Vec3::Y);
        assert_eq!(
            editor
                .pick_surface_with_projection(Layer::ThreeD, projection, [0., 0.])
                .unwrap(),
            Some(Pick {
                object: "model".into(),
                surface: Some(0)
            })
        );
        for x in -8..=8 {
            for y in -8..=8 {
                let ndc = [x as f32 / 10., y as f32 / 10.];
                assert_eq!(
                    editor
                        .pick_surface_with_projection(Layer::ThreeD, projection, ndc)
                        .unwrap(),
                    editor
                        .pick_surface_reference_with_projection(Layer::ThreeD, projection, ndc)
                        .unwrap()
                );
            }
        }
        editor.select_surface(1).unwrap();
        assert_eq!(
            editor.selected_surface_corners(Layer::ThreeD).unwrap(),
            sibling
        );
        editor.select_surface(0).unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.scene, original);
        editor.redo().unwrap();
        assert_eq!(editor.selected_transform().unwrap(), transform);
        assert!(Arc::ptr_eq(
            &data,
            &editor
                .assets
                .get(editor.assets.handle("model").unwrap())
                .unwrap()
                .shared_data()
                .unwrap()
        ));
    }

    #[test]
    fn imported_hierarchy_parts_are_independent_entities_with_undo() {
        let fixture = Fixture::new();
        let mut editor = fixture.editor();
        // Exercise the portable import and instantiation path, not just a catalog OBJ.
        let asset = editor.import(&fixture.0.join("parts.obj")).unwrap();
        editor.add_asset_to_scene(&asset).unwrap();
        let owner = editor.selected.clone().unwrap();
        assert!(editor.selected_object().unwrap().drawable.is_none());
        let children: Vec<_> = editor
            .scene()
            .objects
            .iter()
            .filter(|o| o.parent.as_ref() == Some(&owner))
            .cloned()
            .collect();
        assert_eq!(children.len(), 2);
        for (index, child) in children.iter().enumerate() {
            assert!(
                child.material.is_none() && child.gravity.is_none() && child.blueprints.is_empty()
            );
            assert!(
                matches!(&child.drawable.as_ref().unwrap().mesh, Mesh::Surface { index: part, .. } if *part as usize == index)
            );
            assert!(
                editor
                    .assets
                    .mesh_surface(&child.drawable.as_ref().unwrap().mesh)
                    .is_some()
            );
        }
        let before = editor.scene().clone();
        editor.select_object(Some(children[1].id.clone()));
        assert!(editor.selected_surface().is_none());
        editor.duplicate().unwrap();
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &before);
        editor.select_object(Some(children[0].id.clone()));
        editor.delete().unwrap();
        assert!(
            editor
                .scene()
                .objects
                .iter()
                .any(|o| o.id == children[1].id)
        );
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &before);
        editor.select_object(Some(owner));
        assert!(
            editor
                .frame_selection_bounds(Layer::ThreeD)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn whole_imported_model_transform_gesture_preserves_parent_and_history() {
        let fixture = Fixture::new();
        let mut editor = fixture.editor();
        let mut scene = editor.scene().clone();
        let mut parent = scene.objects[0].clone();
        parent.id = "parent".into();
        parent.drawable = None;
        parent.transform.translation = [3.0, 4.0, 5.0];
        parent.transform.rotation_degrees = [10.0, 30.0, 0.0];
        parent.transform.scale = [-2.0, 3.0, 1.0];
        scene.objects[0].parent = Some(parent.id.clone());
        scene.objects.push(parent);
        editor.apply("Parent", scene).unwrap();
        editor
            .select_pick(Some(Pick {
                object: "model".into(),
                surface: Some(1),
            }))
            .unwrap();
        editor.select_object(editor.selected.clone());
        assert!(editor.selected_surface().is_none());
        let before = editor.scene().clone();
        editor.begin_gesture("Transform gizmo");
        for amount in [15.0, 30.0] {
            let mut next = editor.scene().clone();
            next.objects[0].transform.rotation_degrees[1] = amount;
            next.objects[0].transform.scale = [-2.0, 2.0, 2.0];
            editor.apply("Transform gizmo", next).unwrap();
        }
        editor.finish_gesture();
        let after = editor.scene().clone();
        assert_eq!(after.objects[1], before.objects[1]);
        assert_eq!(after.objects[0].parent, before.objects[0].parent);
        assert_eq!(after.objects[0].drawable, before.objects[0].drawable);
        editor.undo().unwrap();
        assert_eq!(editor.scene(), &before);
        editor.redo().unwrap();
        assert_eq!(editor.scene(), &after);
        editor.begin_gesture("Transform gizmo");
        editor.apply("Transform gizmo", before).unwrap();
        editor.cancel_gesture().unwrap();
        assert_eq!(editor.scene(), &after);
        assert_eq!(editor.selected_mesh().unwrap().parts.len(), 2);
    }

    #[test]
    fn picks_surface_geometry_and_frames_only_its_transformed_bounds() {
        let fixture = Fixture::new();
        let mut editor = fixture.editor();
        let hit = editor
            .pick_surface_with_projection(Layer::ThreeD, projection(), [-2.0 / 12.0, 0.0])
            .unwrap()
            .unwrap();
        assert_eq!(
            hit,
            Pick {
                object: "model".into(),
                surface: Some(0)
            }
        );
        editor.select_pick(Some(hit)).unwrap();
        assert_eq!(
            editor
                .selected_surface()
                .unwrap()
                .part
                .material_name
                .as_deref(),
            Some("LeftPaint")
        );
        assert_eq!(
            editor.frame_selection_bounds(Layer::ThreeD).unwrap(),
            Some([Vec3::new(-3.0, -1.0, 0.0), Vec3::new(-1.0, 1.0, 0.0)])
        );
        assert!(!editor.dirty());
        assert!(editor.undo_label().is_none());
        assert!(editor.select_surface(99).is_err());
        assert_eq!(editor.selected_surface().unwrap().index, 0);
        assert!(
            editor
                .pick_surface_with_projection(Layer::ThreeD, projection(), [0.0, 0.0])
                .unwrap()
                .is_none(),
            "empty gap must not pick the model AABB"
        );
        assert!(
            editor
                .pick_surface_with_projection(Layer::TwoD, projection(), [-2.0 / 12.0, 0.0])
                .unwrap()
                .is_none()
        );

        let mut scene = editor.scene().clone();
        let mut parent = scene.objects[0].clone();
        parent.id = "parent".into();
        parent.drawable = None;
        parent.transform.translation = [3.0, 0.0, 0.0];
        parent.transform.scale = [-2.0, 1.0, 1.0];
        scene.objects[0].parent = Some(parent.id.clone());
        scene.objects.push(parent);
        editor.apply("Parent transform", scene).unwrap();
        let hit = editor
            .pick_surface_with_projection(Layer::ThreeD, projection(), [7.0 / 12.0, 0.0])
            .unwrap()
            .unwrap();
        assert_eq!(hit.surface, Some(0));
        assert_eq!(
            editor.frame_selection_bounds(Layer::ThreeD).unwrap(),
            Some([Vec3::new(5.0, -1.0, 0.0), Vec3::new(9.0, 1.0, 0.0)])
        );
        editor.undo().unwrap();
        assert_eq!(editor.selected_surface().unwrap().index, 0);
        assert_eq!(
            editor
                .frame_selection_bounds(Layer::ThreeD)
                .unwrap()
                .unwrap()[0]
                .x,
            -3.0
        );
        editor.redo().unwrap();
        editor.select_object(Some("model".into()));
        assert_eq!(
            editor
                .frame_selection_bounds(Layer::ThreeD)
                .unwrap()
                .unwrap()[0]
                .x,
            -3.0
        );
    }

    #[test]
    fn accelerated_picks_match_reference_through_parented_mirrored_instances() {
        let fixture = Fixture::new();
        let mut obj = String::from("mtllib parts.mtl\n");
        for i in 0..80 {
            let x = (i % 10) as f32 * 0.7 - 3.5;
            let y = (i / 10) as f32 * 0.7 - 2.8;
            obj.push_str(&format!(
                "o Face{i}\nusemtl {}\nv {x} {y} 0\nv {} {y} 0\nv {x} {} 0\nf {} {} {}\n",
                if i % 2 == 0 {
                    "LeftPaint"
                } else {
                    "RightPaint"
                },
                x + 0.6,
                y + 0.6,
                i * 3 + 1,
                i * 3 + 2,
                i * 3 + 3
            ));
        }
        std::fs::write(fixture.0.join("parts.obj"), obj).unwrap();
        let mut editor = fixture.editor();
        let mut scene = editor.scene().clone();
        let mut parent = scene.objects[0].clone();
        parent.id = "group".into();
        parent.drawable = None;
        parent.transform.translation = [2., -0.5, 1.];
        parent.transform.rotation_degrees = [15., 30., -10.];
        parent.transform.scale = [-2., 0.7, 1.3];
        scene.objects[0].parent = Some("group".into());
        scene.objects.push(parent);
        let mut copy = scene.objects[0].clone();
        copy.id = "copy".into();
        copy.parent = None;
        copy.transform.translation = [-1., 0., 3.];
        copy.transform.scale = [0.6, 1.5, -1.];
        scene.objects.push(copy);
        editor.apply("Fixture transforms", scene).unwrap();
        let before = editor.scene().clone();
        let projections = [
            projection(),
            glam::camera::rh::proj::directx::perspective(1.2, 1.5, 0.1, 100.)
                * glam::camera::rh::view::look_at_mat4(Vec3::new(4., 3., 12.), Vec3::ZERO, Vec3::Y),
        ];
        let mut hits = 0;
        for projection in projections {
            for y in -10..=10 {
                for x in -15..=15 {
                    let ndc = [x as f32 / 15., y as f32 / 10.];
                    let actual = editor
                        .pick_surface_with_projection(Layer::ThreeD, projection, ndc)
                        .unwrap();
                    assert_eq!(
                        actual,
                        editor
                            .pick_surface_reference_with_projection(Layer::ThreeD, projection, ndc)
                            .unwrap()
                    );
                    hits += usize::from(actual.is_some());
                }
            }
        }
        assert!(hits > 50);
        assert_eq!(editor.scene(), &before);
        assert_eq!(editor.undo_label(), Some("Fixture transforms"));
    }

    #[test]
    fn nearer_object_wins_and_inspection_cannot_delete_or_duplicate_owner() {
        let fixture = Fixture::new();
        let mut editor = fixture.editor();
        let mut scene = editor.scene().clone();
        let mut cube = scene.objects[0].clone();
        cube.id = "front".into();
        cube.transform.translation = [2.0, 0.0, 2.0];
        cube.drawable.as_mut().unwrap().mesh = Mesh::Cube;
        scene.objects.push(cube);
        editor.apply("Near object", scene).unwrap();
        assert_eq!(
            editor
                .pick_surface_with_projection(Layer::ThreeD, projection(), [2.0 / 12.0, 0.0])
                .unwrap(),
            Some(Pick {
                object: "front".into(),
                surface: None
            })
        );
        editor.select_object(Some("model".into()));
        editor.select_surface(1).unwrap();
        let scene = editor.scene().clone();
        assert!(editor.delete().is_err());
        assert!(editor.duplicate().is_err());
        assert!(editor.assign_asset_to_selected("model").is_err());
        assert_eq!(*editor.scene(), scene);
        editor.select_pick(None).unwrap();
        assert!(editor.selected.is_none());
        assert!(editor.selected_surface().is_none());
    }

    #[test]
    fn failed_reload_retains_selection_but_replacement_and_play_clear_it() {
        let fixture = Fixture::new();
        let mut editor = fixture.editor();
        editor.select_object(Some("model".into()));
        editor.select_surface(1).unwrap();
        editor.assets = editor.assets.clone();
        editor.repair_surface_selection();
        assert_eq!(editor.selected_surface().unwrap().index, 1);
        std::fs::write(fixture.0.join("parts.obj"), "not an OBJ").unwrap();
        editor.assets.refresh();
        editor.repair_surface_selection();
        assert_eq!(editor.selected_surface().unwrap().index, 1);
        std::fs::write(
            fixture.0.join("parts.obj"),
            "mtllib parts.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl RightPaint\nf 1 2 3\n",
        )
        .unwrap();
        editor.assets.refresh();
        assert!(editor.selected_surface().is_none());
        editor.repair_surface_selection();
        assert!(editor.surface_selection.is_none());
        editor.select_surface(0).unwrap();
        let mut scene = editor.scene().clone();
        scene.objects[0].drawable.as_mut().unwrap().mesh = Mesh::Cube;
        editor.apply("Change model", scene).unwrap();
        assert!(editor.selected_surface().is_none());
        editor.undo().unwrap();
        assert!(
            editor.selected_surface().is_none(),
            "Undo must not revive a discarded surface index"
        );
        editor.select_surface(0).unwrap();
        editor.start_play().unwrap();
        assert!(editor.selected_surface().is_none());
        editor.stop_play();
        assert!(editor.selected_surface().is_none());
    }
}
