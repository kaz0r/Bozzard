//! Testable editor document transactions. The authored document never becomes the play world.
use anyhow::{Context, Result, ensure};
use bozzard_assets::{AssetData, AssetStore};
use bozzard_demo::{SceneDemo, save_document_from};
use bozzard_render::{DrawItem, Material, MeshKind, RenderScene, TextureKind};
use bozzard_scene::{
    AssetKind, AssetSource, Drawable, Layer, Mesh, Object, Scene, Texture, Transform,
};
use glam::{Mat4, Vec3};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

const HISTORY_LIMIT: usize = 100;
struct Change {
    label: String,
    scene: Scene,
}

pub struct Editor {
    scene: Scene,
    saved: Scene,
    pub path: PathBuf,
    pub selected: Option<String>,
    past: Vec<Change>,
    future: Vec<Change>,
    gesture: Option<Change>,
    pub play: Option<SceneDemo>,
    pub assets: AssetStore,
    revision: u64,
    asset_revision: u64,
}

impl Editor {
    pub fn open(path: &Path) -> Result<Self> {
        Self::new(Scene::from_json(&std::fs::read_to_string(path)?)?, path)
    }
    pub fn new(scene: Scene, path: &Path) -> Result<Self> {
        scene.validate()?;
        let assets = load_assets(&scene, path)?;
        Ok(Self {
            saved: scene.clone(),
            scene,
            path: path.to_path_buf(),
            selected: None,
            past: Vec::new(),
            future: Vec::new(),
            gesture: None,
            play: None,
            assets,
            revision: 1,
            asset_revision: 1,
        })
    }
    pub fn scene(&self) -> &Scene {
        &self.scene
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn asset_revision(&self) -> u64 {
        self.asset_revision
    }
    pub fn dirty(&self) -> bool {
        self.scene != self.saved
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|c| c.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|c| c.label.as_str())
    }
    pub fn selected_object(&self) -> Option<&Object> {
        self.scene
            .objects
            .iter()
            .find(|o| Some(&o.id) == self.selected.as_ref())
    }

    pub fn begin_gesture(&mut self, label: &str) {
        if self.gesture.is_none() && self.play.is_none() {
            self.gesture = Some(Change {
                label: label.into(),
                scene: self.scene.clone(),
            });
        }
    }
    pub fn finish_gesture(&mut self) {
        if let Some(change) = self.gesture.take()
            && change.scene != self.scene
        {
            self.record(change);
        }
    }
    fn record(&mut self, change: Change) {
        self.past.push(change);
        if self.past.len() > HISTORY_LIMIT {
            self.past.remove(0);
        }
        self.future.clear();
    }
    pub fn apply(&mut self, label: &str, scene: Scene) -> Result<()> {
        ensure!(
            self.play.is_none(),
            "Stop Play before editing the authored scene"
        );
        scene.validate()?;
        if scene == self.scene {
            return Ok(());
        }
        // Catalog replacements validate all imports before publishing a new document.
        let assets = if scene.assets != self.scene.assets {
            Some(load_assets(&scene, &self.path)?)
        } else {
            None
        };
        if self.gesture.is_none() {
            self.record(Change {
                label: label.into(),
                scene: self.scene.clone(),
            });
        }
        self.scene = scene;
        if let Some(assets) = assets {
            self.assets = assets;
            self.asset_revision += 1;
        }
        self.revision += 1;
        self.repair_selection();
        Ok(())
    }
    fn repair_selection(&mut self) {
        if self.selected_object().is_none() {
            self.selected = None;
        }
    }
    pub fn undo(&mut self) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play before undo");
        self.finish_gesture();
        if let Some(change) = self.past.last() {
            let assets = load_assets(&change.scene, &self.path)?;
            let change = self.past.pop().unwrap();
            self.future.push(Change {
                label: change.label,
                scene: std::mem::replace(&mut self.scene, change.scene),
            });
            self.assets = assets;
            self.asset_revision += 1;
            self.revision += 1;
            self.repair_selection();
        }
        Ok(())
    }
    pub fn redo(&mut self) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play before redo");
        self.finish_gesture();
        if let Some(change) = self.future.last() {
            let assets = load_assets(&change.scene, &self.path)?;
            let change = self.future.pop().unwrap();
            self.past.push(Change {
                label: change.label,
                scene: std::mem::replace(&mut self.scene, change.scene),
            });
            self.assets = assets;
            self.asset_revision += 1;
            self.revision += 1;
            self.repair_selection();
        }
        Ok(())
    }
    pub fn create(&mut self, mesh: Mesh, layer: Layer) -> Result<()> {
        let mut scene = self.scene.clone();
        let id = unique_id(&scene, "object");
        scene.objects.push(Object {
            id: id.clone(),
            name: match mesh {
                Mesh::Quad => "Sprite",
                Mesh::Cube => "Cube",
                Mesh::Asset(_) => "Mesh",
            }
            .into(),
            parent: None,
            transform: Transform::default(),
            camera: None,
            spin: None,
            drawable: Some(Drawable {
                layer,
                mesh,
                texture: Texture::White,
                color: [0.25, 0.8, 0.7],
                uv_scale: [1.0; 2],
            }),
        });
        self.apply("Create object", scene)?;
        self.selected = Some(id);
        Ok(())
    }
    pub fn duplicate(&mut self) -> Result<()> {
        let selected = self
            .selected
            .as_ref()
            .context("Select an object first")?
            .clone();
        let ids = subtree(&self.scene, &selected);
        let mut scene = self.scene.clone();
        let mut replacements = BTreeMap::new();
        for object in self.scene.objects.iter().filter(|o| ids.contains(&o.id)) {
            let mut copy = object.clone();
            copy.id = unique_id(&scene, "copy");
            copy.name.push_str(" copy");
            replacements.insert(object.id.clone(), copy.id.clone());
            scene.objects.push(copy);
        }
        for object in &mut scene.objects[self.scene.objects.len()..] {
            if let Some(parent) = &object.parent
                && let Some(new) = replacements.get(parent)
            {
                object.parent = Some(new.clone());
            }
            if object.id == replacements[&selected] {
                object.transform.translation[0] += 0.5;
            }
        }
        self.apply("Duplicate subtree", scene)?;
        self.selected = Some(replacements[&selected].clone());
        Ok(())
    }
    pub fn delete(&mut self) -> Result<()> {
        let id = self.selected.as_ref().context("Select an object first")?;
        let ids = subtree(&self.scene, id);
        ensure!(
            !self.scene.views.values().any(|id| ids.contains(id)),
            "An active camera is in this subtree; assign another active camera first"
        );
        let mut scene = self.scene.clone();
        scene.objects.retain(|o| !ids.contains(&o.id));
        self.apply("Delete subtree", scene)
    }
    pub fn start_play(&mut self) -> Result<()> {
        self.finish_gesture();
        if self.play.is_none() {
            self.play = Some(SceneDemo::new(&self.scene)?);
        }
        Ok(())
    }
    pub fn stop_play(&mut self) {
        self.play = None;
    }
    pub fn advance(&mut self, delta: Duration) {
        if let Some(play) = &mut self.play {
            play.app.advance(delta);
        }
    }
    pub fn save(&mut self, path: &Path) -> Result<()> {
        self.finish_gesture();
        // Always save the authored document, even during Play.
        save_document_from(&self.scene, path, Some(&self.path))?;
        let rebased = Scene::from_json(&std::fs::read_to_string(path)?)?;
        let assets = load_assets(&rebased, path)?;
        if path != self.path {
            // History contains paths relative to the old root; discard it on Save As.
            self.past.clear();
            self.future.clear();
        }
        self.scene = rebased.clone();
        self.saved = rebased;
        self.path = path.to_path_buf();
        self.assets = assets;
        self.asset_revision += 1;
        self.revision += 1;
        Ok(())
    }
    pub fn import(&mut self, source: &Path) -> Result<String> {
        ensure!(self.play.is_none(), "Stop Play before importing");
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let kind = match extension.as_str() {
            "png" | "jpg" | "jpeg" => AssetKind::Image,
            "obj" => AssetKind::Mesh,
            _ => anyhow::bail!("Choose PNG, JPEG, or OBJ"),
        };
        let base = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("asset")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let mut number = 1;
        let id = loop {
            let id = format!("{base}-{number}");
            if !self.scene.assets.contains_key(&id)
                && !root(&self.path)
                    .join(format!("assets/{id}.{extension}"))
                    .exists()
            {
                break id;
            }
            number += 1;
        };
        let relative = format!("assets/{id}.{extension}");
        let target = root(&self.path).join(&relative);
        // Validate from the original location before copying any bytes into the project.
        let sources = BTreeMap::from([(
            id.clone(),
            AssetSource {
                kind,
                path: source
                    .file_name()
                    .context("source has no filename")?
                    .to_str()
                    .context("non UTF-8 filename")?
                    .into(),
            },
        )]);
        let mut candidate = AssetStore::new(source.parent().unwrap_or(Path::new(".")), &sources)?;
        candidate.refresh();
        candidate.require_ready()?;
        std::fs::create_dir_all(target.parent().unwrap())?;
        // create_new prevents overwriting an existing user asset.
        use std::io::Write;
        let mut destination = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        let result = (|| -> Result<()> {
            destination.write_all(&std::fs::read(source)?)?;
            destination.sync_all()?;
            drop(destination);
            let mut scene = self.scene.clone();
            scene.assets.insert(
                id.clone(),
                AssetSource {
                    kind,
                    path: relative,
                },
            );
            self.apply("Import asset", scene)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(target);
        }
        result?;
        Ok(id)
    }
    pub fn render(&self, layer: Layer, aspect: f32) -> Result<RenderScene> {
        let edit;
        let demo = if let Some(play) = &self.play {
            play
        } else {
            edit = SceneDemo::new(&self.scene)?;
            &edit
        };
        extract(demo, layer, aspect)
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
        let demo = SceneDemo::new(&self.scene)?;
        let inv = projection.inverse();
        let origin = inv.project_point3(Vec3::new(ndc[0], ndc[1], 0.0));
        let direction = (inv.project_point3(Vec3::new(ndc[0], ndc[1], 1.0)) - origin).normalize();
        let matrices = demo.instance.global_transforms(&demo.app.world)?;
        let mut best: Option<(f32, String)> = None;
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
                            Some(t)
                        } else {
                            None
                        }
                    }
                }
                Mesh::Cube => ray_box(o, d),
                Mesh::Asset(id) => self
                    .assets
                    .handle(id)
                    .and_then(|h| self.assets.get(h))
                    .and_then(|e| e.data())
                    .and_then(|data| {
                        let AssetData::Mesh(mesh) = data else {
                            return None;
                        };
                        mesh.indices
                            .chunks_exact(3)
                            .filter_map(|tri| ray_triangle(o, d, tri.map_vertices(&mesh.vertices)))
                            .min_by(f32::total_cmp)
                    }),
            };
            if let Some(t) = hit
                && best.as_ref().is_none_or(|(distance, _)| t < *distance)
            {
                best = Some((t, object.id.clone()));
            }
        }
        Ok(best.map(|(_, id)| id))
    }
}

trait TriangleVertices {
    fn map_vertices(&self, vertices: &[[f32; 8]]) -> [Vec3; 3];
}
impl TriangleVertices for [u32] {
    fn map_vertices(&self, vertices: &[[f32; 8]]) -> [Vec3; 3] {
        [0, 1, 2].map(|i| Vec3::from_slice(&vertices[self[i] as usize][..3]))
    }
}
fn ray_triangle(o: Vec3, d: Vec3, [a, b, c]: [Vec3; 3]) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let t = o - a;
    let u = t.dot(p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(e1);
    let v = d.dot(q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = e2.dot(q) / det;
    (distance > 0.0).then_some(distance)
}
fn ray_box(o: Vec3, d: Vec3) -> Option<f32> {
    let mut near = 0.0_f32;
    let mut far = f32::INFINITY;
    for i in 0..3 {
        if d[i].abs() < 1e-8 {
            if o[i].abs() > 0.5 {
                return None;
            }
        } else {
            let a = (-0.5 - o[i]) / d[i];
            let b = (0.5 - o[i]) / d[i];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
    }
    (far >= near && far > 0.0).then_some(if near > 0.0 { near } else { far })
}
pub fn root(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
fn load_assets(scene: &Scene, path: &Path) -> Result<AssetStore> {
    let mut store = AssetStore::new(root(path), &scene.assets)?;
    store.refresh();
    store.require_ready()?;
    Ok(store)
}
fn unique_id(scene: &Scene, prefix: &str) -> String {
    let mut i = 1;
    loop {
        let id = format!("{prefix}-{i}");
        if !scene.objects.iter().any(|o| o.id == id) {
            return id;
        }
        i += 1;
    }
}
fn subtree(scene: &Scene, id: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::from([id.to_string()]);
    loop {
        let previous = ids.len();
        for o in &scene.objects {
            if o.parent.as_ref().is_some_and(|p| ids.contains(p)) {
                ids.insert(o.id.clone());
            }
        }
        if ids.len() == previous {
            return ids;
        }
    }
}
pub fn extract(demo: &SceneDemo, layer: Layer, aspect: f32) -> Result<RenderScene> {
    let view = demo.instance.view(&demo.app.world, layer, aspect)?;
    Ok(RenderScene {
        view_projection: view.view_projection,
        items: view
            .objects
            .into_iter()
            .map(|(model, d)| DrawItem {
                model,
                mesh: match d.mesh {
                    Mesh::Quad => MeshKind::Quad,
                    Mesh::Cube => MeshKind::Cube,
                    Mesh::Asset(id) => MeshKind::Imported(id),
                },
                material: Material {
                    tint: d.color,
                    uv_scale: d.uv_scale,
                    lit: layer == Layer::ThreeD,
                    texture: match d.texture {
                        Texture::White => TextureKind::White,
                        Texture::Checker => TextureKind::Checker,
                        Texture::Asset(id) => TextureKind::Imported(id),
                    },
                },
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    const PNG: &[u8] = include_bytes!("../../../examples/demo/scenes/assets/palette.png");
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "bozzard-editor-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn editor() -> Editor {
        Editor::new(
            bozzard_demo::scene_document().unwrap(),
            Path::new("work/editor-test/scene.json"),
        )
        .unwrap()
    }
    #[test]
    fn commands_duplicate_and_delete_subtrees_with_undo_redo() {
        let mut e = editor();
        let initial = e.scene.clone();
        e.selected = Some("orbit-pivot".into());
        e.duplicate().unwrap();
        assert_eq!(e.scene.objects.len(), initial.objects.len() + 2);
        let copy = e.selected.clone().unwrap();
        assert!(
            e.scene
                .objects
                .iter()
                .any(|o| o.parent.as_ref() == Some(&copy))
        );
        e.delete().unwrap();
        assert_eq!(e.scene, initial);
        e.undo().unwrap();
        assert_eq!(e.scene.objects.len(), initial.objects.len() + 2);
        e.undo().unwrap();
        assert_eq!(e.scene, initial);
        e.redo().unwrap();
        assert_eq!(e.scene.objects.len(), initial.objects.len() + 2);
        e.create(Mesh::Quad, Layer::TwoD).unwrap();
        assert!(e.redo_label().is_none());
    }
    #[test]
    fn gesture_is_one_undo_and_invalid_edits_are_transactional() {
        let mut e = editor();
        let initial = e.scene.clone();
        e.begin_gesture("Move");
        for x in [1.0, 2.0, 3.0] {
            let mut scene = e.scene.clone();
            scene.objects[0].transform.translation[0] = x;
            e.apply("Move", scene).unwrap();
        }
        e.finish_gesture();
        assert_eq!(e.past.len(), 1);
        assert!(e.dirty());
        let mut invalid = e.scene.clone();
        invalid.objects[0].transform.scale = [0.0; 3];
        assert!(e.apply("Invalid", invalid).is_err());
        assert_eq!(e.past.len(), 1);
        e.undo().unwrap();
        assert_eq!(e.scene, initial);
        assert!(!e.dirty());
    }
    #[test]
    fn play_is_a_separate_world_and_cannot_change_authored_state() {
        let mut e = editor();
        let initial = e.scene.clone();
        e.start_play().unwrap();
        let a = e
            .play
            .as_ref()
            .unwrap()
            .instance
            .entity("satellite")
            .unwrap();
        for _ in 0..120 {
            e.play.as_mut().unwrap().app.step();
        }
        assert_ne!(
            e.play
                .as_ref()
                .unwrap()
                .instance
                .capture(&e.play.as_ref().unwrap().app.world)
                .unwrap(),
            initial
        );
        assert!(e.create(Mesh::Cube, Layer::ThreeD).is_err());
        e.stop_play();
        assert_eq!(e.scene, initial);
        assert!(!e.dirty());
        e.start_play().unwrap();
        assert_ne!(
            a,
            e.play
                .as_ref()
                .unwrap()
                .instance
                .entity("satellite")
                .unwrap()
        );
    }
    #[test]
    fn save_round_trips_and_save_as_discards_history() {
        let dir = Temp::new();
        let path = dir.0.join("scene.json");
        let mut e = Editor::new(bozzard_demo::scene_document().unwrap(), &path).unwrap();
        e.create(Mesh::Cube, Layer::ThreeD).unwrap();
        assert!(e.dirty());
        // Saving during Play persists the authored document, not the simulated world.
        e.start_play().unwrap();
        for _ in 0..60 {
            e.play.as_mut().unwrap().app.step();
        }
        e.save(&path).unwrap();
        e.stop_play();
        assert!(!e.dirty());
        assert_eq!(Editor::open(&path).unwrap().scene(), e.scene());
        // Save As to another directory keeps the content but discards old-root history.
        let other = dir.0.join("elsewhere").join("level.json");
        e.save(&other).unwrap();
        assert_eq!(e.path, other);
        assert!(e.undo_label().is_none());
        assert!(e.redo_label().is_none());
        assert!(!e.dirty());
        assert_eq!(Editor::open(&other).unwrap().scene(), e.scene());
    }
    #[test]
    fn import_copies_into_the_project_and_rejects_bad_files() {
        let dir = Temp::new();
        let downloads = dir.0.join("downloads");
        let project = dir.0.join("project");
        std::fs::create_dir(&downloads).unwrap();
        std::fs::create_dir(&project).unwrap();
        let source = downloads.join("palette.png");
        std::fs::write(&source, PNG).unwrap();
        let mut e = Editor::new(
            bozzard_demo::scene_document().unwrap(),
            &project.join("scene.json"),
        )
        .unwrap();
        let id = e.import(&source).unwrap();
        assert_eq!(id, "palette-1");
        assert_eq!(
            std::fs::read(project.join("assets/palette-1.png")).unwrap(),
            PNG
        );
        assert_eq!(e.scene().assets[&id].path, "assets/palette-1.png");
        let handle = e.assets.handle(&id).unwrap();
        assert!(matches!(
            e.assets.get(handle).unwrap().data(),
            Some(AssetData::Image(_))
        ));
        // Undo removes the catalog entry; the copied bytes stay in the project.
        e.undo().unwrap();
        assert!(!e.scene().assets.contains_key(&id));
        e.redo().unwrap();
        // A second import gets a fresh id and never overwrites the first copy.
        assert_eq!(e.import(&source).unwrap(), "palette-2");
        // Corrupt and unsupported files are rejected before any bytes are copied.
        let broken = downloads.join("broken.png");
        std::fs::write(&broken, b"not a png").unwrap();
        assert!(e.import(&broken).is_err());
        assert!(!project.join("assets/broken-1.png").exists());
        let text = downloads.join("notes.txt");
        std::fs::write(&text, b"hello").unwrap();
        assert!(e.import(&text).is_err());
    }
    #[test]
    fn history_is_bounded_and_oldest_changes_fall_off() {
        let mut e = editor();
        for i in 0..(HISTORY_LIMIT + 5) {
            let mut scene = e.scene().clone();
            scene.objects[0].transform.translation[0] = 1000.0 + i as f32;
            e.apply("Move", scene).unwrap();
        }
        assert_eq!(e.past.len(), HISTORY_LIMIT);
        for _ in 0..HISTORY_LIMIT {
            e.undo().unwrap();
        }
        assert!(e.undo_label().is_none());
        // The five oldest changes fell off, so the initial state is unreachable.
        assert!(e.dirty());
    }
    #[test]
    fn active_camera_subtree_cannot_be_deleted() {
        let mut e = editor();
        e.selected = Some("camera-3d".into());
        let message = e.delete().unwrap_err().to_string();
        assert!(message.contains("camera"), "{message}");
        e.selected = Some("satellite".into());
        e.delete().unwrap();
        assert!(e.selected.is_none());
    }
    #[test]
    fn pick_finds_front_object_and_rejects_empty_space() {
        let mut e = editor();
        e.create(Mesh::Quad, Layer::TwoD).unwrap();
        let id = e.selected.clone().unwrap();
        assert_eq!(e.pick(Layer::TwoD, 1.0, [0.0, 0.0]).unwrap(), Some(id));
        assert_eq!(e.pick(Layer::TwoD, 1.0, [0.98, 0.98]).unwrap(), None);
    }
}
