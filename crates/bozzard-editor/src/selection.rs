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
        let Mesh::Asset(id) = &self.selected_object()?.drawable.as_ref()?.mesh else {
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
        let part = mesh.parts.get(index).context("surface no longer exists")?;
        let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
        let indices = mesh
            .indices
            .get(part.start as usize..part.start as usize + part.count as usize)
            .context("invalid surface indices")?;
        ensure!(!indices.is_empty(), "empty surface");
        for index in indices {
            let point = Vec3::from_slice(&mesh.vertices[*index as usize][..3]);
            bounds[0] = bounds[0].min(point);
            bounds[1] = bounds[1].max(point);
        }
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
        let transform = demo.instance.global_transforms(&demo.app.world)?[&object.id];
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
        let matrices = demo.instance.global_transforms(&demo.app.world)?;
        let mut best: Option<(f32, Pick)> = None;
        for object in &self.scene.objects {
            let Some(drawable) = &object.drawable else {
                continue;
            };
            if drawable.layer != layer {
                continue;
            }
            let inverse = matrices[&object.id].inverse();
            let o = inverse.transform_point3(origin);
            let d = inverse.transform_vector3(direction);
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
                Mesh::Asset(id) => self
                    .assets
                    .handle(id)
                    .and_then(|h| self.assets.get(h))
                    .and_then(|entry| {
                        let AssetData::Mesh(mesh) = entry.data()? else {
                            return None;
                        };
                        let hit = if reference {
                            entry.raycast_reference(o, d)
                        } else {
                            entry.raycast(o, d)
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
