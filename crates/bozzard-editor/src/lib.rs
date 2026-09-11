//! Testable editor document transactions. The authored document never becomes the play world.
use anyhow::{Context, Result, ensure};
use bozzard_assets::{AssetData, AssetStore};
use bozzard_demo::{SceneDemo, prepare_document_from, save_document};
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

mod gi;
pub use gi::PreparedGi;
mod framing;
mod hierarchy;
mod prefabs;
pub use prefabs::{PrefabCommand, PreparedPrefab};
mod loading;
mod materials;
mod selection;
pub use loading::{LoadedScene, PreparedImport, PreparedSave};
pub use selection::{Pick, SelectedSurface};

const HISTORY_LIMIT: usize = 100;
struct Change {
    label: String,
    scene: Scene,
    assets: Option<AssetStore>,
}

pub struct Editor {
    scene: Scene,
    saved: Scene,
    pub path: PathBuf,
    pub selected: Option<String>,
    surface_selection: Option<selection::SurfaceSelection>,
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
        Ok(Self::from_loaded(scene, path.to_path_buf(), assets))
    }
    pub fn new_pending(scene: Scene, path: &Path) -> Result<Self> {
        scene.validate()?;
        let assets = AssetStore::new(root(path), &scene.assets)?;
        Ok(Self::from_loaded(scene, path.to_path_buf(), assets))
    }
    fn from_loaded(scene: Scene, path: PathBuf, assets: AssetStore) -> Self {
        Self {
            saved: scene.clone(),
            scene,
            path,
            selected: None,
            surface_selection: None,
            past: Vec::new(),
            future: Vec::new(),
            gesture: None,
            play: None,
            assets,
            revision: 1,
            asset_revision: 1,
        }
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
                assets: Some(self.assets.clone()),
            });
        }
    }
    pub fn finish_gesture(&mut self) {
        if let Some(mut change) = self.gesture.take()
            && change.scene != self.scene
        {
            if change.scene.assets == self.scene.assets {
                change.assets = None;
            }
            self.record(change);
        }
    }
    /// Restore the active gesture's starting document without changing Undo/Redo history.
    /// Failed asset restoration leaves the gesture active and the document unchanged.
    pub fn cancel_gesture(&mut self) -> Result<()> {
        if let Some(change) = &self.gesture {
            let original = change.scene.clone();
            self.apply("Cancel gesture", original)?;
            self.gesture = None;
        }
        Ok(())
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
            Some(self.cached_assets(&scene, &self.path)?)
        } else {
            None
        };
        if self.gesture.is_none() {
            self.record(Change {
                label: label.into(),
                scene: self.scene.clone(),
                assets: assets.as_ref().map(|_| self.assets.clone()),
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
        if self.selected_surface().is_none() {
            self.surface_selection = None;
        }
        if self.selected_object().is_none() {
            self.selected = None;
        }
    }
    pub fn undo(&mut self) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play before undo");
        self.finish_gesture();
        if let Some(change) = self.past.pop() {
            let assets = change
                .assets
                .filter(|_| change.scene.assets != self.scene.assets);
            self.future.push(Change {
                label: change.label,
                scene: std::mem::replace(&mut self.scene, change.scene),
                assets: assets.as_ref().map(|_| self.assets.clone()),
            });
            if let Some(assets) = assets {
                self.assets = assets;
                self.asset_revision += 1;
            }
            self.revision += 1;
            self.repair_selection();
        }
        Ok(())
    }
    pub fn redo(&mut self) -> Result<()> {
        ensure!(self.play.is_none(), "Stop Play before redo");
        self.finish_gesture();
        if let Some(change) = self.future.pop() {
            let assets = change
                .assets
                .filter(|_| change.scene.assets != self.scene.assets);
            self.past.push(Change {
                label: change.label,
                scene: std::mem::replace(&mut self.scene, change.scene),
                assets: assets.as_ref().map(|_| self.assets.clone()),
            });
            if let Some(assets) = assets {
                self.assets = assets;
                self.asset_revision += 1;
            }
            self.revision += 1;
            self.repair_selection();
        }
        Ok(())
    }
    pub fn create(&mut self, mesh: Mesh, layer: Layer) -> Result<()> {
        let mut scene = self.scene.clone();
        let id = unique_id(&scene, "object");
        scene.objects.push(Object {
            light: None,
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
            collider: None,
            gravity: None,
            player_controller: None,
            trigger: None,
            drawable: Some(Drawable {
                gi_static: true,
                material_overrides: Vec::new(),
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
    pub fn create_light(&mut self, kind: bozzard_scene::LightKind) -> Result<()> {
        let mut scene = self.scene.clone();
        let id = unique_id(&scene, "light");
        scene.objects.push(Object {
            id: id.clone(),
            name: match kind {
                bozzard_scene::LightKind::Point => "Point light",
                bozzard_scene::LightKind::Spot => "Spot light",
                bozzard_scene::LightKind::Directional => "Directional light",
            }
            .into(),
            light: Some(bozzard_scene::Light {
                kind,
                ..Default::default()
            }),
            transform: Transform {
                translation: [0., 2., 2.],
                ..Default::default()
            },
            parent: None,
            camera: None,
            drawable: None,
            spin: None,
            collider: None,
            gravity: None,
            player_controller: None,
            trigger: None,
        });
        self.finish_gesture();
        self.apply("Create light", scene)?;
        self.select_object(Some(id));
        Ok(())
    }
    pub fn duplicate(&mut self) -> Result<()> {
        ensure!(
            self.selected_surface().is_none(),
            "Select the whole model before duplicating it"
        );
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
            if object.id == selected || !self.scene.prefabs.contains_key(&selected) {
                copy.name.push_str(" copy");
            }
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
        // Duplicating a full instance keeps its source link and independent baseline.
        for (root, link) in &self.scene.prefabs {
            if let Some(new_root) = replacements.get(root) {
                let mut link = link.clone();
                for id in link.members.values_mut() {
                    *id = replacements[id].clone();
                }
                for base in &mut link.baseline {
                    base.id = replacements[&base.id].clone();
                    base.parent = base.parent.as_ref().map(|p| replacements[p].clone());
                }
                scene.prefabs.insert(new_root.clone(), link);
            }
        }
        self.apply("Duplicate subtree", scene)?;
        self.selected = Some(replacements[&selected].clone());
        Ok(())
    }
    pub fn delete(&mut self) -> Result<()> {
        ensure!(
            self.selected_surface().is_none(),
            "Select the whole model before deleting it"
        );
        let id = self.selected.as_ref().context("Select an object first")?;
        let ids = subtree(&self.scene, id);
        ensure!(
            !self.scene.views.values().any(|id| ids.contains(id)),
            "An active camera is in this subtree; assign another active camera first"
        );
        let mut scene = self.scene.clone();
        scene.objects.retain(|o| !ids.contains(&o.id));
        scene.prefabs.retain(|root, _| !ids.contains(root));
        self.apply("Delete subtree", scene)
    }
    pub fn start_play(&mut self) -> Result<()> {
        self.surface_selection = None;
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
        let rebased = prepare_document_from(&self.scene, path, Some(&self.path))?;
        let assets = load_assets(&rebased, path)?;
        // Complete all fallible preparation before the atomic destination replacement.
        save_document(&rebased, path)?;
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
    /// Instantiate a catalog entry in one undoable edit, preserving source material colors.
    pub fn add_asset_to_scene(&mut self, asset_id: &str) -> Result<Layer> {
        ensure!(self.play.is_none(), "Stop Play before adding assets");
        let source = self
            .scene
            .assets
            .get(asset_id)
            .context("asset is no longer in the catalog")?;
        let entry = self
            .assets
            .handle(asset_id)
            .and_then(|h| self.assets.get(h))
            .context("asset is not loaded")?;
        ensure!(
            matches!(entry.state(), bozzard_assets::LoadState::Ready),
            "repair or reload this asset before adding it"
        );
        let (layer, mesh, texture) = match source.kind {
            AssetKind::Prefab => {
                anyhow::bail!("Place prefabs using the background prefab operation")
            }
            AssetKind::Mesh => (Layer::ThreeD, Mesh::Asset(asset_id.into()), Texture::White),
            AssetKind::Image => (Layer::TwoD, Mesh::Quad, Texture::Asset(asset_id.into())),
        };
        let mut scene = self.scene.clone();
        let id = unique_id(&scene, "object");
        let mut transform = Transform::default();
        if let Some(AssetData::Image(image)) = entry.data() {
            transform.scale[0] = image.width as f32 / image.height as f32;
        }
        scene.objects.push(Object {
            light: None,
            id: id.clone(),
            name: asset_id.into(),
            parent: None,
            transform,
            camera: None,
            spin: None,
            collider: None,
            gravity: None,
            player_controller: None,
            trigger: None,
            drawable: Some(Drawable {
                gi_static: true,
                material_overrides: Vec::new(),
                layer,
                mesh,
                texture,
                color: [1.0; 3],
                uv_scale: [1.0; 2],
            }),
        });
        self.finish_gesture();
        self.apply("Add asset to scene", scene)?;
        self.selected = Some(id);
        Ok(layer)
    }
    pub fn assign_asset_to_selected(&mut self, asset_id: &str) -> Result<()> {
        ensure!(
            self.selected_surface().is_none(),
            "Select the whole model before assigning an asset"
        );
        let source = self
            .scene
            .assets
            .get(asset_id)
            .context("asset is no longer in the catalog")?;
        let entry = self
            .assets
            .handle(asset_id)
            .and_then(|h| self.assets.get(h))
            .context("asset is not loaded")?;
        ensure!(
            matches!(entry.state(), bozzard_assets::LoadState::Ready),
            "repair or reload this asset before assigning it"
        );
        let mut scene = self.scene.clone();
        let object = scene
            .objects
            .iter_mut()
            .find(|o| Some(&o.id) == self.selected.as_ref())
            .context("select a drawable object")?;
        let drawable = object
            .drawable
            .as_mut()
            .context("selected object has no drawable")?;
        match source.kind {
            AssetKind::Prefab => anyhow::bail!(
                "Place a prefab as a linked hierarchy instead of assigning it to a drawable"
            ),
            AssetKind::Mesh => {
                let mesh = Mesh::Asset(asset_id.into());
                if drawable.mesh != mesh {
                    drawable.mesh = mesh;
                    drawable.material_overrides.clear();
                }
            }
            AssetKind::Image => drawable.texture = Texture::Asset(asset_id.into()),
        }
        self.finish_gesture();
        self.apply("Assign asset", scene)
    }
    /// Remove only an unused catalog entry; the source file stays on disk for Undo/reuse.
    pub fn remove_asset(&mut self, asset_id: &str) -> Result<()> {
        ensure!(
            self.scene.assets.contains_key(asset_id),
            "asset is no longer in the catalog"
        );
        ensure!(
            self.scene
                .asset_users()
                .get(asset_id)
                .is_none_or(Vec::is_empty),
            "asset is used by scene objects"
        );
        let mut scene = self.scene.clone();
        scene.assets.remove(asset_id);
        self.finish_gesture();
        self.apply("Remove unused asset", scene)
    }
    pub fn import(&mut self, source: &Path) -> Result<String> {
        self.import_with(source, &bozzard_assets::job::Progress::default())
    }
    fn import_with(
        &mut self,
        source: &Path,
        progress: &bozzard_assets::job::Progress,
    ) -> Result<String> {
        ensure!(self.play.is_none(), "Stop Play before importing");
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let kind = match extension.as_str() {
            "png" | "jpg" | "jpeg" => AssetKind::Image,
            "obj" | "gltf" | "glb" => AssetKind::Mesh,
            "json"
                if source
                    .file_name()
                    .and_then(|p| p.to_str())
                    .is_some_and(|n| n.ends_with(".prefab.json")) =>
            {
                AssetKind::Prefab
            }
            _ => anyhow::bail!("Choose PNG, JPEG, OBJ, glTF, GLB, or .prefab.json"),
        };
        if kind == AssetKind::Prefab {
            return self.link_prefab(source, progress);
        }
        progress.stage("Reading model and packing textures")?;
        let package = if matches!(extension.as_str(), "gltf" | "glb") {
            Some(bozzard_assets::package_gltf(source, progress)?)
        } else {
            None
        };
        let packed = if kind == AssetKind::Mesh && package.is_none() {
            bozzard_assets::portable_model(source)?
        } else {
            None
        };
        let extension = if packed.is_some() || package.is_some() {
            "gltf".to_owned()
        } else {
            extension
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
                && !root(&self.path).join(format!("assets/{id}")).exists()
                && !root(&self.path)
                    .join(format!("assets/{id}.{extension}"))
                    .exists()
            {
                break id;
            }
            number += 1;
        };
        let relative = if package.is_some() {
            format!("assets/{id}/model.gltf")
        } else {
            format!("assets/{id}.{extension}")
        };
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
        candidate.refresh_with(progress)?;
        candidate.require_ready()?;
        progress.stage("Copying asset into project")?;
        if let Some(package) = package {
            let directory = target.parent().context("package directory")?;
            std::fs::create_dir_all(directory.parent().context("assets directory")?)?;
            std::fs::create_dir(directory)?;
            let result = (|| -> Result<()> {
                use std::io::Write;
                for (name, bytes) in package.files {
                    progress.stage(format!("Copying {name}"))?;
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(directory.join(name))?;
                    file.write_all(&bytes)?;
                    file.sync_all()?;
                }
                progress.stage("Validating project assets")?;
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
                let _ = std::fs::remove_dir_all(directory);
            }
            result?;
            return Ok(id);
        }
        std::fs::create_dir_all(target.parent().unwrap())?;
        // create_new prevents overwriting an existing user asset.
        use std::io::Write;
        let mut destination = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        let result = (|| -> Result<()> {
            let bytes = match packed {
                Some(bytes) => bytes,
                None => std::fs::read(source)?,
            };
            destination.write_all(&bytes)?;
            destination.sync_all()?;
            drop(destination);
            progress.stage("Validating project assets")?;
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
        extract(demo, &self.assets, layer, aspect)
    }
    /// Translate the selected Play-world collider without touching the authored scene.
    pub fn move_selected_box(&mut self, delta: Vec3) -> Result<bozzard_scene::MoveResult> {
        let id = self
            .selected
            .as_ref()
            .context("select a box collider to move")?;
        let play = self
            .play
            .as_mut()
            .context("start Play to move a collider")?;
        play.instance.move_box(&mut play.app.world, id, delta)
    }
    /// Jump only in the Play world; authoring remains unchanged.
    pub fn jump_selected_box(&mut self) -> Result<bool> {
        let id = self.selected.as_ref().context("select a box to jump")?;
        let play = self.play.as_mut().context("start Play to jump")?;
        let entity = play.instance.entity(id).context("unknown jumping object")?;
        let Some(gravity) = play.app.world.get::<bozzard_scene::Gravity>(entity) else {
            return Ok(false);
        };
        let speed = gravity.jump_speed;
        play.instance.jump_box(&mut play.app.world, id, speed)
    }
    /// Current authored or Play-world collider bounds and overlap pairs.
    pub fn collisions(&self) -> Result<bozzard_scene::CollisionSnapshot> {
        if let Some(play) = &self.play {
            play.instance.collisions(&play.app.world)
        } else {
            let demo = SceneDemo::new(&self.scene)?;
            demo.instance.collisions(&demo.app.world)
        }
    }
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
pub fn extract(
    demo: &SceneDemo,
    assets: &bozzard_assets::AssetStore,
    layer: Layer,
    aspect: f32,
) -> Result<RenderScene> {
    demo.check_simulation()?;
    let view = demo.instance.view(&demo.app.world, layer, aspect)?;
    let mut gi = None;
    if layer == Layer::ThreeD
        && demo.instance.document().gi.enabled
        && demo.instance.document().gi.baked.is_some()
    {
        let scene = demo.instance.capture(&demo.app.world)?;
        if bozzard_assets::gi::is_current(&scene, assets).unwrap_or(false) {
            let baked = scene.gi.baked.as_ref().unwrap();
            gi = Some(bozzard_render::IrradianceVolume {
                min: baked.volume.min,
                max: baked.volume.max,
                resolution: baked.volume.resolution,
                intensity: scene.gi.intensity,
                normal_bias: scene.gi.normal_bias,
                probes: baked.probes.clone(),
            });
        }
    }

    Ok(RenderScene {
        fog: bozzard_render::FogSettings {
            enabled: layer == Layer::ThreeD && view.fog.enabled,
            color: view.fog.color,
            distance_density: view.fog.distance_density,
            start_distance: view.fog.start_distance,
            height_density: view.fog.height_density,
            base_height: view.fog.base_height,
            height_falloff: view.fog.height_falloff,
        },
        gi,
        lights: view
            .lights
            .iter()
            .map(|world| bozzard_render::LocalLight {
                directional: world.light.kind == bozzard_scene::LightKind::Directional,
                shadows: world.light.requests_shadow_map().then_some(
                    bozzard_render::LocalShadowSettings {
                        bias: world.light.shadow_bias,
                        normal_bias: world.light.shadow_normal_bias,
                    },
                ),
                position: world.position,
                direction: world.direction,
                color: world.light.color,
                intensity: world.light.intensity,
                range: world.light.range,
                spot_angles: (world.light.kind == bozzard_scene::LightKind::Spot).then_some([
                    world.light.inner_angle_degrees,
                    world.light.outer_angle_degrees,
                ]),
            })
            .collect(),
        environment: bozzard_render::EnvironmentSettings {
            zenith: view.environment.zenith,
            horizon: view.environment.horizon,
            ground: view.environment.ground,
            intensity: if layer == Layer::ThreeD {
                view.environment.intensity
            } else {
                0.
            },
            background: layer == Layer::ThreeD && view.environment.background,
        },
        display: bozzard_render::DisplaySettings {
            bloom: bozzard_render::BloomSettings {
                enabled: layer == Layer::ThreeD && view.display.bloom.enabled,
                intensity: view.display.bloom.intensity,
                threshold: view.display.bloom.threshold,
                scatter: view.display.bloom.scatter,
            },
            exposure_ev: if layer == Layer::ThreeD {
                view.display.exposure_ev
            } else {
                0.
            },
            tone_mapping: layer == Layer::ThreeD && view.display.tone_mapping,
        },
        lighting: bozzard_render::Lighting {
            shadows: view.lighting.shadows,
            shadow_resolution: view.lighting.shadow_resolution,
            shadow_bias: view.lighting.shadow_bias,
            shadow_normal_bias: view.lighting.shadow_normal_bias,
            sun_direction: view.lighting.sun_direction,
            sun_color: view.lighting.sun_color,
            sun_intensity: view.lighting.sun_intensity,
            ambient_color: view.lighting.ambient_color,
            ambient_intensity: view.lighting.ambient_intensity,
        },
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
                    surface_overrides: d
                        .material_overrides
                        .into_iter()
                        .map(|value| bozzard_render::SurfaceMaterialOverride {
                            surface: value.surface,
                            source: value.source,
                            tint: value.tint,
                            metallic: value.metallic,
                            roughness: value.roughness,
                        })
                        .collect(),
                    tint: d.color,
                    uv_scale: d.uv_scale,
                    lit: layer == Layer::ThreeD,
                    texture: match d.texture {
                        Texture::White => TextureKind::White,
                        Texture::Checker => TextureKind::Checker,
                        Texture::Normals => TextureKind::Normals,
                        Texture::ProceduralChecker => TextureKind::ProceduralChecker,
                        Texture::Toon => TextureKind::Toon,
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
    fn bloom_history_save_reset_and_2d_isolation() {
        let dir = Temp::new();
        let mut e = editor();
        let initial = e.scene().clone();
        e.begin_gesture("Bloom slider");
        for intensity in [0.2, 0.4] {
            let mut scene = e.scene().clone();
            scene.display.bloom = bozzard_scene::BloomSettings {
                enabled: true,
                intensity,
                threshold: 1.5,
                scatter: 0.8,
            };
            e.apply("Bloom slider", scene).unwrap();
        }
        e.finish_gesture();
        assert!(e.render(Layer::ThreeD, 1.).unwrap().display.bloom.enabled);
        assert!(!e.render(Layer::TwoD, 1.).unwrap().display.bloom.enabled);
        e.undo().unwrap();
        assert_eq!(e.scene(), &initial);
        e.redo().unwrap();
        let authored = e.scene().clone();
        e.start_play().unwrap();
        assert!(e.render(Layer::ThreeD, 1.).unwrap().display.bloom.enabled);
        e.save(&dir.0.join("bloom.json")).unwrap();
        assert_eq!(
            Editor::open(&dir.0.join("bloom.json")).unwrap().scene(),
            &authored
        );
        e.stop_play();
        assert_eq!(e.scene(), &authored);
        let mut reset = e.scene().clone();
        reset.display = Default::default();
        e.apply("Reset display", reset).unwrap();
        assert!(!e.render(Layer::ThreeD, 1.).unwrap().display.bloom.enabled);
        e.undo().unwrap();
        assert_eq!(e.scene(), &authored);
    }
    #[test]
    fn directional_light_creation_and_extraction() {
        let mut e = editor();
        e.create_light(bozzard_scene::LightKind::Directional)
            .unwrap();
        assert_eq!(e.selected_object().unwrap().name, "Directional light");
        let view = e.render(Layer::ThreeD, 1.).unwrap();
        assert!(view.lights[0].directional);
        assert_eq!(view.lights[0].spot_angles, None);
        assert_eq!(view.lights[0].direction, [0., 0., -1.]);
        e.undo().unwrap();
        assert!(e.render(Layer::ThreeD, 1.).unwrap().lights.is_empty());
        e.redo().unwrap();
        assert!(e.render(Layer::ThreeD, 1.).unwrap().lights[0].directional);
    }

    #[test]
    fn local_light_history_duplicate_save_and_play_isolation() {
        use bozzard_scene::LightKind;
        for kind in [LightKind::Point, LightKind::Spot] {
            local_light_history_for_kind(kind);
        }
    }
    fn local_light_history_for_kind(kind: bozzard_scene::LightKind) {
        use bozzard_scene::Light;
        let dir = Temp::new();
        let mut e = editor();
        let initial = e.scene().clone();
        e.create_light(kind).unwrap();
        assert_eq!(
            bozzard_scene::MAX_SHADOWED_POINT_LIGHTS,
            bozzard_render::MAX_SHADOWED_POINT_LIGHTS
        );
        assert_eq!(
            bozzard_scene::MAX_SHADOWED_SPOT_LIGHTS,
            bozzard_render::MAX_SHADOWED_SPOT_LIGHTS
        );
        let id = e.selected.clone().unwrap();
        assert_eq!(e.render(Layer::ThreeD, 1.).unwrap().lights.len(), 1);
        assert!(e.render(Layer::TwoD, 1.).unwrap().lights.is_empty());
        let position = Vec3::from(e.selected_object().unwrap().transform.translation);
        assert_eq!(
            e.frame_bounds(Layer::ThreeD, Some(&id)).unwrap(),
            Some([position; 2])
        );
        e.undo().unwrap();
        assert_eq!(e.scene(), &initial);
        e.redo().unwrap();
        e.select_object(Some(id.clone()));
        e.begin_gesture("Light slider");
        for intensity in [120., 150.] {
            let mut scene = e.scene().clone();
            let light = scene
                .objects
                .iter_mut()
                .find(|o| o.id == id)
                .unwrap()
                .light
                .as_mut()
                .unwrap();
            light.intensity = intensity;
            light.shadows = true;
            light.shadow_bias = 0.02;
            light.shadow_normal_bias = 0.04;
            e.apply("Light slider", scene).unwrap();
        }
        e.finish_gesture();
        e.undo().unwrap();
        assert_eq!(e.selected_object().unwrap().light.unwrap().intensity, 100.);
        assert!(!e.selected_object().unwrap().light.unwrap().shadows);
        e.redo().unwrap();
        assert_eq!(e.selected_object().unwrap().light.unwrap().intensity, 150.);
        let shadow = e.render(Layer::ThreeD, 1.).unwrap().lights[0]
            .shadows
            .unwrap();
        assert_eq!((shadow.bias, shadow.normal_bias), (0.02, 0.04));
        e.duplicate().unwrap();
        let copy = e.selected.clone().unwrap();
        assert_ne!(copy, id);
        let mut scene = e.scene().clone();
        scene
            .objects
            .iter_mut()
            .find(|o| o.id == copy)
            .unwrap()
            .light
            .as_mut()
            .unwrap()
            .color = [0., 0., 1.];
        e.apply("Blue copy", scene).unwrap();
        assert_eq!(
            e.scene()
                .objects
                .iter()
                .find(|o| o.id == id)
                .unwrap()
                .light
                .unwrap()
                .color,
            [1.; 3]
        );
        let authored = e.scene().clone();
        e.start_play().unwrap();
        let play = e.play.as_mut().unwrap();
        let entity = play.instance.entity(&id).unwrap();
        play.app.world.get_mut::<Light>(entity).unwrap().intensity = 0.;
        play.app.world.get_mut::<Light>(entity).unwrap().shadows = false;
        assert!(
            e.render(Layer::ThreeD, 1.)
                .unwrap()
                .lights
                .iter()
                .any(|l| l.shadows.is_none())
        );
        assert!(
            e.render(Layer::ThreeD, 1.)
                .unwrap()
                .lights
                .iter()
                .any(|l| l.intensity == 0.)
        );
        let save = dir.0.join("lights.json");
        e.save(&save).unwrap();
        assert_eq!(Editor::open(&save).unwrap().scene(), &authored);
        e.stop_play();
        assert_eq!(e.scene(), &authored);
        let mut invalid = e.scene().clone();
        invalid
            .objects
            .iter_mut()
            .find(|o| o.id == id)
            .unwrap()
            .light
            .as_mut()
            .unwrap()
            .range = 0.;
        assert!(e.apply("Invalid light", invalid).is_err());
        assert_eq!(e.scene(), &authored);
        e.delete().unwrap();
        assert_eq!(e.render(Layer::ThreeD, 1.).unwrap().lights.len(), 1);
        e.undo().unwrap();
        assert_eq!(e.render(Layer::ThreeD, 1.).unwrap().lights.len(), 2);
    }
    #[test]
    fn play_jump_uses_configured_speed_without_changing_authoring() {
        let mut e = editor();
        e.create(Mesh::Cube, Layer::ThreeD).unwrap();
        let id = e.selected.clone().unwrap();
        let mut scene = e.scene.clone();
        let object = scene.objects.iter_mut().find(|o| o.id == id).unwrap();
        object.collider = Some(bozzard_scene::BoxCollider::default());
        object.gravity = Some(bozzard_scene::Gravity {
            jump_speed: 8.0,
            ..Default::default()
        });
        e.apply("Jump settings", scene).unwrap();
        let authored = e.scene.clone();
        e.start_play().unwrap();
        let play = e.play.as_mut().unwrap();
        let entity = play.instance.entity(&id).unwrap();
        play.app
            .world
            .insert(
                entity,
                bozzard_scene::GravityState {
                    grounded: true,
                    vertical_velocity: 0.0,
                },
            )
            .unwrap();
        assert!(e.jump_selected_box().unwrap());
        assert_eq!(
            e.play
                .as_ref()
                .unwrap()
                .app
                .world
                .get::<bozzard_scene::GravityState>(entity)
                .unwrap()
                .vertical_velocity,
            8.0
        );
        e.stop_play();
        assert_eq!(e.scene, authored);
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
    fn cancelled_gesture_restores_document_and_preserves_redo() {
        let mut e = editor();
        let initial = e.scene.clone();
        let mut changed = initial.clone();
        changed.objects[0].transform.translation[0] += 2.0;
        e.apply("Earlier edit", changed.clone()).unwrap();
        e.undo().unwrap();
        let revision = e.revision;
        e.begin_gesture("Drag");
        for offset in [1.0, 3.0] {
            let mut dragged = initial.clone();
            dragged.objects[0].transform.translation[1] += offset;
            e.apply("Drag", dragged).unwrap();
        }
        e.cancel_gesture().unwrap();
        e.finish_gesture();
        assert_eq!(e.scene, initial);
        assert!(!e.dirty());
        assert!(e.past.is_empty());
        assert_eq!(e.future.len(), 1);
        assert!(e.revision > revision);
        e.cancel_gesture().unwrap(); // No gesture is a harmless no-op.
        e.redo().unwrap();
        assert_eq!(e.scene, changed);
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
    fn model_imports_are_portable_and_asset_actions_are_undoable() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/assets");
        for extension in ["gltf", "glb"] {
            let dir = Temp::new();
            let downloads = dir.0.join("downloads");
            std::fs::create_dir_all(&downloads).unwrap();
            for file in [
                format!("courier.{extension}"),
                "courier.bin".into(),
                "courier-paint.png".into(),
            ] {
                std::fs::copy(fixture.join(&file), downloads.join(&file)).unwrap();
            }
            let mut e = Editor::new(
                bozzard_demo::scene_document().unwrap(),
                &dir.0.join("project/scene.json"),
            )
            .unwrap();
            let asset = e
                .import(&downloads.join(format!("courier.{extension}")))
                .unwrap();
            assert!(e.scene.assets[&asset].path.ends_with(".gltf"));
            let packaged =
                std::fs::read_to_string(dir.0.join("project").join(&e.scene.assets[&asset].path))
                    .unwrap();
            let packaged: serde_json::Value = serde_json::from_str(&packaged).unwrap();
            for array in ["buffers", "images"] {
                for resource in packaged[array].as_array().unwrap() {
                    if let Some(uri) = resource.get("uri").and_then(|u| u.as_str()) {
                        assert!(
                            !uri.starts_with("data:"),
                            "project packages must not expand resources into base64"
                        );
                        assert_eq!(Path::new(uri).components().count(), 1);
                    }
                }
            }
            std::fs::remove_dir_all(downloads).unwrap();
            e.assets.refresh();
            e.assets.require_ready().unwrap();
            let before = e.scene.clone();
            assert_eq!(e.add_asset_to_scene(&asset).unwrap(), Layer::ThreeD);
            assert_eq!(
                e.selected_object()
                    .unwrap()
                    .drawable
                    .as_ref()
                    .unwrap()
                    .color,
                [1.0; 3]
            );
            assert!(e.remove_asset(&asset).is_err());
            e.undo().unwrap();
            assert_eq!(e.scene, before);
            e.remove_asset(&asset).unwrap();
            assert!(!e.scene.assets.contains_key(&asset));
            e.undo().unwrap();
            assert!(e.scene.assets.contains_key(&asset));
            e.save(&dir.0.join("project/scene.json")).unwrap();
            let reopened = Editor::open(&e.path).unwrap();
            let mesh = reopened
                .assets
                .handle(&asset)
                .and_then(|h| reopened.assets.get(h))
                .and_then(|entry| entry.data())
                .unwrap();
            let AssetData::Mesh(mesh) = mesh else {
                panic!("not mesh")
            };
            assert_eq!(mesh.parts.len(), 10);
        }
    }
    #[test]
    fn obj_material_import_keeps_textures_after_source_removal() {
        let dir = Temp::new();
        let source = dir.0.join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("color.png"), PNG).unwrap();
        std::fs::write(
            source.join("mesh.mtl"),
            "newmtl paint\nKd 0.8 0.5 0.2\nmap_Kd color.png\n",
        )
        .unwrap();
        std::fs::write(source.join("mesh.obj"), "mtllib mesh.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nusemtl paint\nf 1/1 2/2 3/3\n").unwrap();
        let mut e = Editor::new(
            bozzard_demo::scene_document().unwrap(),
            &dir.0.join("project/scene.json"),
        )
        .unwrap();
        let id = e.import(&source.join("mesh.obj")).unwrap();
        std::fs::remove_dir_all(source).unwrap();
        e.assets.refresh();
        e.assets.require_ready().unwrap();
        let AssetData::Mesh(mesh) = e
            .assets
            .get(e.assets.handle(&id).unwrap())
            .unwrap()
            .data()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(mesh.parts.len(), 1);
        assert!(mesh.parts[0].image.is_some());
        assert_eq!(mesh.parts[0].color, [0.8, 0.5, 0.2, 1.0]);
        assert_eq!(mesh.vertices[0][7], 1.0);
        e.create(Mesh::Cube, Layer::ThreeD).unwrap();
        e.assign_asset_to_selected(&id).unwrap();
        assert_eq!(
            e.selected_object().unwrap().drawable.as_ref().unwrap().mesh,
            Mesh::Asset(id)
        );
    }
    #[test]
    fn framing_uses_imported_mesh_vertices_in_world_space() {
        let dir = Temp::new();
        let source = dir.0.join("triangle.obj");
        std::fs::write(&source, "v 2 0 0\nv 4 0 0\nv 2 3 0\nf 1 2 3\n").unwrap();
        let mut e = Editor::new(
            bozzard_demo::scene_document().unwrap(),
            &dir.0.join("project/scene.json"),
        )
        .unwrap();
        let asset = e.import(&source).unwrap();
        e.create(Mesh::Asset(asset), Layer::ThreeD).unwrap();
        let id = e.selected.clone().unwrap();
        let mut scene = e.scene.clone();
        let object = scene.objects.iter_mut().find(|o| o.id == id).unwrap();
        object.transform.translation = [10.0, 0.0, 0.0];
        e.apply("Place mesh", scene).unwrap();
        assert_eq!(
            e.frame_bounds(Layer::ThreeD, Some(&id)).unwrap(),
            Some([Vec3::new(12.0, 0.0, 0.0), Vec3::new(14.0, 3.0, 0.0)])
        );
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
    fn broken_asset_does_not_block_transform_history_or_overwrite_failed_save() {
        let dir = Temp::new();
        let path = dir.0.join("scene.json");
        let source = dir.0.join("source.png");
        std::fs::write(&source, PNG).unwrap();
        let mut e = Editor::new(bozzard_demo::scene_document().unwrap(), &path).unwrap();
        let id = e.import(&source).unwrap();
        e.save(&path).unwrap();
        let saved_bytes = std::fs::read(&path).unwrap();
        let before = e.scene.clone();
        e.create(Mesh::Cube, Layer::ThreeD).unwrap();
        let edited = e.scene.clone();
        let revision = e.asset_revision();
        let handle = e.assets.handle(&id).unwrap();
        std::fs::write(dir.0.join(&e.scene.assets[&id].path), b"broken").unwrap();
        e.undo().unwrap();
        assert_eq!(e.scene, before);
        e.redo().unwrap();
        assert_eq!(e.scene, edited);
        assert_eq!(e.asset_revision(), revision);
        assert!(e.assets.get(handle).unwrap().data().is_some());
        assert!(e.save(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), saved_bytes);
        let other = dir.0.join("other.json");
        std::fs::write(&other, b"existing destination").unwrap();
        assert!(e.save(&other).is_err());
        assert_eq!(std::fs::read(other).unwrap(), b"existing destination");
        assert_eq!(e.path, path);
        assert_eq!(e.scene, edited);
        assert!(e.dirty());
        assert!(e.undo_label().is_some());
    }
    #[test]
    fn collider_edits_undo_and_queries_follow_the_play_world() {
        let mut e = editor();
        let mut scene = e.scene().clone();
        scene.objects.retain(|o| o.camera.is_some());
        e.apply("Empty scene", scene).unwrap();
        e.create(Mesh::Cube, Layer::ThreeD).unwrap();
        let a = e.selected.clone().unwrap();
        e.create(Mesh::Cube, Layer::ThreeD).unwrap();
        let b = e.selected.clone().unwrap();
        let mut scene = e.scene().clone();
        for object in scene.objects.iter_mut().filter(|o| o.id == a || o.id == b) {
            object.collider = Some(bozzard_scene::BoxCollider::default());
        }
        e.apply("Add colliders", scene).unwrap();
        assert_eq!(
            e.collisions().unwrap().overlaps,
            vec![(a.clone(), b.clone())]
        );
        e.undo().unwrap();
        assert!(e.collisions().unwrap().boxes.is_empty());
        e.redo().unwrap();
        let authored = e.scene().clone();
        e.start_play().unwrap();
        let play = e.play.as_mut().unwrap();
        let entity = play.instance.entity(&b).unwrap();
        play.app
            .world
            .get_mut::<Transform>(entity)
            .unwrap()
            .translation[0] = 10.0;
        assert!(e.collisions().unwrap().overlaps.is_empty());
        assert_eq!(e.scene(), &authored);
        e.stop_play();
        assert_eq!(e.collisions().unwrap().overlaps, vec![(a, b)]);
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
