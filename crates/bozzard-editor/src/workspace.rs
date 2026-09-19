//! Independently editable scene documents in one editor session.
//! The active document stays in the host's Editor; switching moves whole documents,
//! preserving selections, imported data, unsaved changes and each Undo/Redo history.
use super::*;

pub type SceneId = u64;

pub struct OpenScenes {
    active: SceneId,
    next: SceneId,
    inactive: BTreeMap<SceneId, Editor>,
    revision: u64,
    hidden: BTreeSet<SceneId>,
    view: Option<SceneView>,
}

impl Default for OpenScenes {
    fn default() -> Self {
        Self {
            active: 0,
            next: 1,
            inactive: BTreeMap::new(),
            revision: 0,
            hidden: BTreeSet::new(),
            view: None,
        }
    }
}

impl OpenScenes {
    pub const LIMIT: usize = 16;

    pub fn active(&self) -> SceneId {
        self.active
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn len(&self) -> usize {
        self.inactive.len() + 1
    }
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn documents<'a>(
        &'a self,
        current: &'a Editor,
    ) -> impl Iterator<Item = (SceneId, &'a Editor)> {
        std::iter::once((self.active, current))
            .chain(self.inactive.iter().map(|(&id, editor)| (id, editor)))
    }

    pub fn document<'a>(&'a self, current: &'a Editor, id: SceneId) -> Option<&'a Editor> {
        if id == self.active {
            Some(current)
        } else {
            self.inactive.get(&id)
        }
    }

    pub fn document_mut<'a>(
        &'a mut self,
        current: &'a mut Editor,
        id: SceneId,
    ) -> Option<&'a mut Editor> {
        if id == self.active {
            Some(current)
        } else {
            self.inactive.get_mut(&id)
        }
    }

    pub fn any_dirty(&self, current: &Editor) -> bool {
        self.documents(current).any(|(_, editor)| editor.dirty())
    }

    pub fn visible(&self, id: SceneId) -> bool {
        !self.hidden.contains(&id)
    }

    pub fn set_visible(&mut self, id: SceneId, visible: bool) {
        let changed = if visible {
            self.hidden.remove(&id)
        } else {
            self.hidden.insert(id)
        };
        if changed {
            self.revision += 1;
        }
    }

    pub fn find_path(&self, current: &Editor, path: &Path) -> Option<SceneId> {
        let path = document_path(path);
        self.documents(current)
            .find_map(|(id, editor)| (document_path(&editor.path) == path).then_some(id))
    }

    pub fn validate_save_path(&self, current: &Editor, path: &Path) -> Result<()> {
        ensure!(
            self.find_path(current, path)
                .is_none_or(|id| id == self.active),
            "That file is open in another scene; switch to it before saving"
        );
        Ok(())
    }

    /// Replacing the active document requires the host to resolve its dirty state first.
    pub fn replace(&mut self, current: &mut Editor, incoming: Editor) -> Result<()> {
        ensure!(current.play.is_none(), "Stop Play before replacing a scene");
        ensure!(
            self.find_path(current, &incoming.path)
                .is_none_or(|id| id == self.active),
            "scene is already open in another document"
        );
        *current = incoming;
        self.hidden.remove(&self.active);
        self.revision += 1;
        Ok(())
    }

    /// Accept an already prepared document. Disk/asset work belongs to the open worker.
    pub fn add(&mut self, current: &mut Editor, mut incoming: Editor) -> Result<SceneId> {
        ensure!(
            current.play.is_none() && incoming.play.is_none(),
            "Stop Play before opening another scene"
        );
        ensure!(self.len() < Self::LIMIT, "open scene limit: 16");
        ensure!(
            self.find_path(current, &incoming.path).is_none(),
            "scene is already open"
        );
        let id = self.next;
        let next = id.checked_add(1).context("editor scene IDs exhausted")?;
        current.finish_gesture();
        incoming.finish_gesture();
        let previous = std::mem::replace(current, incoming);
        self.inactive.insert(self.active, previous);
        self.active = id;
        self.next = next;
        self.revision += 1;
        Ok(id)
    }

    pub fn activate(&mut self, current: &mut Editor, id: SceneId) -> Result<()> {
        ensure!(current.play.is_none(), "Stop Play before switching scenes");
        if id == self.active {
            return Ok(());
        }
        ensure!(self.inactive.contains_key(&id), "scene is no longer open");
        current.finish_gesture();
        let incoming = self.inactive.remove(&id).unwrap();
        let previous = std::mem::replace(current, incoming);
        self.inactive.insert(self.active, previous);
        self.active = id;
        self.revision += 1;
        Ok(())
    }

    /// Close a clean document. Hosts must explicitly resolve unsaved changes first.
    pub fn close(&mut self, current: &mut Editor, id: SceneId) -> Result<()> {
        ensure!(current.play.is_none(), "Stop Play before closing a scene");
        ensure!(self.len() > 1, "the editor needs at least one open scene");
        let target = self
            .document(current, id)
            .context("scene is no longer open")?;
        ensure!(!target.dirty(), "Save the scene before closing it");
        self.discard_and_close(current, id)
    }

    /// Explicit discard, used only after the host's unsaved-changes decision.
    pub fn discard_and_close(&mut self, current: &mut Editor, id: SceneId) -> Result<()> {
        ensure!(current.play.is_none(), "Stop Play before closing a scene");
        ensure!(self.len() > 1, "the editor needs at least one open scene");
        ensure!(
            self.document(current, id).is_some(),
            "scene is no longer open"
        );
        if id == self.active {
            let replacement = *self.inactive.keys().next().unwrap();
            self.activate(current, replacement)?;
        }
        self.inactive.remove(&id);
        self.hidden.remove(&id);
        self.revision += 1;
        Ok(())
    }

    /// Rebuild only after document/asset changes. Selecting objects does not clone
    /// geometry or rebuild the preview world. Play uses the active scene alone.
    pub fn sync_view(&mut self, current: &Editor) -> Result<()> {
        if (!current.is_prefab_source() && self.inactive.values().all(Editor::is_prefab_source))
            || current.play.is_some()
        {
            self.view = None;
            return Ok(());
        }
        let current_view = self.view.as_ref().is_some_and(|v| {
            v.workspace == self.revision
                && v.stamp.len() == self.len()
                && v.stamp
                    .iter()
                    .zip(self.documents(current))
                    .all(|(stamp, (id, editor))| {
                        stamp.id == id
                            && stamp.revision == editor.revision()
                            && stamp.catalog == editor.asset_revision()
                            && stamp
                                .assets
                                .iter()
                                .copied()
                                .eq(editor.assets.entries().map(|a| a.revision()))
                    })
        });
        if !current_view {
            let stamp = self
                .documents(current)
                .map(|(id, e)| DocumentStamp {
                    id,
                    revision: e.revision(),
                    catalog: e.asset_revision(),
                    assets: e.assets.entries().map(|a| a.revision()).collect(),
                })
                .collect();
            let mut scene = current.scene.clone();
            scene.objects.clear();
            scene.assets.clear();
            scene.prefabs.clear();
            scene.views.clear();
            scene.blackboard.clear();
            scene.runtime_scenes.clear();
            scene.runtime_scene_sources.clear();
            // GI baked for one document cannot describe their combined geometry.
            scene.gi = Default::default();
            let mut owners = BTreeMap::new();
            let mut entries = Vec::new();
            for (id, editor) in self.documents(current).filter(|(id, e)| {
                if current.is_prefab_source() {
                    *id == self.active
                } else {
                    !e.is_prefab_source()
                }
            }) {
                let visible = self.visible(id);
                let names: BTreeMap<_, _> = editor
                    .scene
                    .objects
                    .iter()
                    .map(|o| (o.id.clone(), qualified(id, &o.id)))
                    .collect();
                let assets: BTreeMap<_, _> = editor
                    .scene
                    .assets
                    .keys()
                    .map(|a| (a.clone(), qualified(id, a)))
                    .collect();
                for entry in editor.assets.entries().filter(|_| visible) {
                    entries.push((qualified(id, &entry.id), entry));
                }
                for (name, source) in editor.scene.assets.iter().filter(|_| visible) {
                    scene.assets.insert(assets[name].clone(), source.clone());
                }
                for object in &editor.scene.objects {
                    let mut object = object.clone();
                    owners.insert(names[&object.id].clone(), (id, object.id.clone()));
                    object.remap_blueprint_objects(&names);
                    object.remap_assets(&assets);
                    object.id = names[&object.id].clone();
                    object.parent = object.parent.map(|p| names[&p].clone());
                    // This document exists only for authoring extraction and picking.
                    // Gameplay remains in each source and runs via Play active scene.
                    object.blueprints.clear();
                    object.blackboard.clear();
                    object.script_manager = None;
                    object.player_controller = None;
                    object.joint = None;
                    if !visible {
                        // Keep transform ancestry and cameras for navigation, even
                        // when all geometry in the camera's document is hidden.
                        object.drawable = None;
                        object.material = None;
                        object.shader_graph = None;
                        object.text_rendering = None;
                        object.particle_emitter = None;
                        object.light = None;
                        object.lod = None;
                        object.collider = None;
                        object.mesh_collider = None;
                        object.gravity = None;
                        object.trigger = None;
                        object.spin = None;
                        object.extras.clear();
                    }
                    scene.objects.push(object);
                }
                for (layer, camera) in &editor.scene.views {
                    scene
                        .views
                        .entry(*layer)
                        .or_insert_with(|| names[camera].clone());
                }
            }
            if current.is_prefab_source() {
                // Inspection cameras never enter the source file or its history.
                let cameras = bozzard_demo::scene_document()?;
                for (layer, id) in cameras.views {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        scene.views.entry(layer)
                    {
                        let mut camera =
                            cameras.objects.iter().find(|o| o.id == id).unwrap().clone();
                        camera.id = format!("prefab-inspection-{}", camera.id);
                        entry.insert(camera.id.clone());
                        scene.objects.push(camera);
                    }
                }
            }
            scene.validate()?;
            let assets = AssetStore::shared_catalog(entries)?;
            let generation = self.view.as_ref().map_or(1, |v| v.editor.revision + 1);
            let mut editor = Editor::from_loaded(scene, current.path.clone(), assets);
            editor.revision = generation;
            self.view = Some(SceneView {
                stamp,
                workspace: self.revision,
                editor,
                owners,
            });
        }
        if let Some(view) = &mut self.view {
            view.editor.selected = current
                .selected
                .as_ref()
                .map(|id| qualified(self.active, id));
        }
        Ok(())
    }

    pub fn view<'a>(&'a self, current: &'a Editor) -> &'a Editor {
        if current.play.is_some() {
            return current;
        }
        self.view.as_ref().map_or(current, |v| &v.editor)
    }

    pub fn owner(&self, pick: Pick) -> Option<(SceneId, Pick)> {
        if let Some(view) = &self.view {
            let (id, object) = view.owners.get(&pick.object)?;
            Some((
                *id,
                Pick {
                    object: object.clone(),
                    surface: pick.surface,
                },
            ))
        } else {
            Some((self.active, pick))
        }
    }

    pub fn view_asset(&self, current: &Editor, id: &str) -> String {
        if self.view.is_some() && current.play.is_none() {
            qualified(self.active, id)
        } else {
            id.to_owned()
        }
    }
}

fn qualified(id: SceneId, name: &str) -> String {
    format!("document-{id}-{name}")
}

fn document_path(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let path = absolute.as_path();
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        let parent = path.parent().unwrap_or(Path::new("."));
        std::fs::canonicalize(parent)
            .unwrap_or_else(|_| parent.to_path_buf())
            .join(path.file_name().unwrap_or_default())
    })
}

#[derive(PartialEq)]
struct DocumentStamp {
    id: SceneId,
    revision: u64,
    catalog: u64,
    assets: Vec<u64>,
}
struct SceneView {
    stamp: Vec<DocumentStamp>,
    workspace: u64,
    editor: Editor,
    owners: BTreeMap<String, (SceneId, String)>,
}
