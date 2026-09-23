//! Independently editable scene documents in one editor session.
//! The active document stays in the host's Editor; switching moves whole documents,
//! preserving selections, imported data, unsaved changes and each Undo/Redo history.
use super::*;
use std::{
    cell::RefCell,
    sync::{Arc, Weak},
};

pub type SceneId = u64;

pub struct OpenScenes {
    active: SceneId,
    next: SceneId,
    inactive: BTreeMap<SceneId, Editor>,
    revision: u64,
    hidden: BTreeSet<SceneId>,
    hidden_objects: BTreeMap<SceneId, BTreeSet<String>>,
    hidden_cache: RefCell<BTreeMap<SceneId, HiddenObjects>>,
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
            hidden_objects: BTreeMap::new(),
            hidden_cache: Default::default(),
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

    pub fn set_object_visible(&mut self, scene: SceneId, object: &str, visible: bool) {
        let hidden = self.hidden_objects.entry(scene).or_default();
        let changed = if visible {
            hidden.remove(object)
        } else {
            hidden.insert(object.to_owned())
        };
        if changed {
            self.revision += 1;
            self.hidden_cache.get_mut().remove(&scene);
        }
        if hidden.is_empty() {
            self.hidden_objects.remove(&scene);
        }
    }

    /// A hidden parent suppresses every descendant in the authoring viewport.
    /// Reuse the same closure across hierarchy, picking and overlay passes.
    pub fn hidden_objects_in(&self, scene: SceneId, editor: &Editor) -> Arc<BTreeSet<String>> {
        let document = editor.scene_snapshot();
        let source = Arc::downgrade(&document);
        let mut cache = self.hidden_cache.borrow_mut();
        if let Some(cached) = cache.get(&scene)
            && cached.source.ptr_eq(&source)
        {
            return Arc::clone(&cached.objects);
        }
        let mut hidden = BTreeSet::new();
        if let Some(explicit) = self.hidden_objects.get(&scene) {
            let mut children = BTreeMap::<&str, Vec<&str>>::new();
            for object in &document.objects {
                if let Some(parent) = object.parent.as_deref() {
                    children.entry(parent).or_default().push(&object.id);
                }
            }
            let mut stack: Vec<_> = explicit.iter().map(String::as_str).collect();
            while let Some(id) = stack.pop() {
                if hidden.insert(id.to_owned()) {
                    stack.extend(children.get(id).into_iter().flatten().copied());
                }
            }
        }
        let hidden = Arc::new(hidden);
        cache.insert(
            scene,
            HiddenObjects {
                source,
                objects: Arc::clone(&hidden),
            },
        );
        hidden
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
        self.hidden_objects.remove(&self.active);
        self.hidden_cache.get_mut().remove(&self.active);
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
        self.hidden_objects.remove(&id);
        self.hidden_cache.get_mut().remove(&id);
        self.revision += 1;
        Ok(())
    }

    /// Rebuild only after document/asset changes. Selecting objects does not clone
    /// geometry or rebuild the preview world. Play uses the active scene alone.
    pub fn sync_view(&mut self, current: &Editor) -> Result<()> {
        let single_document =
            !current.is_prefab_source() && self.inactive.values().all(Editor::is_prefab_source);
        let active_has_hidden_objects = self
            .hidden_objects
            .get(&self.active)
            .is_some_and(|objects| !objects.is_empty());
        if (single_document && self.visible(self.active) && !active_has_hidden_objects)
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
            if single_document {
                // Keep source asset IDs for a filtered single scene. Qualifying
                // them would make Residency upload every asset again on an eye click.
                let hidden = self.hidden_objects_in(self.active, current);
                let mut scene = current.scene.clone();
                let mut owners = BTreeMap::new();
                for object in &mut scene.objects {
                    owners.insert(object.id.clone(), (self.active, object.id.clone()));
                    object.prepare_authoring_preview(
                        self.visible(self.active) && !hidden.contains(&object.id),
                    )?;
                }
                scene.validate()?;
                let generation = self.view.as_ref().map_or(1, |v| v.editor.revision + 1);
                let mut editor =
                    Editor::from_loaded(scene, current.path.clone(), current.assets.clone());
                editor.revision = generation;
                editor.selected = current.selected.clone();
                self.view = Some(SceneView {
                    stamp,
                    workspace: self.revision,
                    editor,
                    owners,
                    qualified: false,
                });
                return Ok(());
            }
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
                let hidden_objects = self.hidden_objects_in(id, editor);
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
                    let object_hidden = hidden_objects.contains(&object.id);
                    owners.insert(names[&object.id].clone(), (id, object.id.clone()));
                    object.remap_blueprint_objects(&names);
                    object.remap_assets(&assets);
                    object.id = names[&object.id].clone();
                    object.parent = object.parent.map(|p| names[&p].clone());
                    // This document exists only for authoring extraction and picking.
                    // Gameplay remains in each source and runs via Play active scene.
                    object.prepare_authoring_preview(visible && !object_hidden)?;
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
                qualified: true,
            });
        }
        if let Some(view) = &mut self.view {
            view.editor.selected = current.selected.as_ref().map(|id| {
                if view.qualified {
                    qualified(self.active, id)
                } else {
                    id.clone()
                }
            });
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
        if self.view.as_ref().is_some_and(|view| view.qualified) && current.play.is_none() {
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
    qualified: bool,
}
struct HiddenObjects {
    source: Weak<Scene>,
    objects: Arc<BTreeSet<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eye_scene() -> Scene {
        let mut scene = bozzard_demo::scene_document().unwrap();
        let mut drawable = scene
            .objects
            .iter()
            .find_map(|o| o.drawable.clone())
            .unwrap();
        drawable.layer = Layer::ThreeD;
        drawable.mesh = Mesh::Cube;
        scene.objects.retain(|o| o.camera.is_some());
        for (id, z) in [("front", 2.0), ("rear", 4.0)] {
            scene.objects.push(Object {
                id: id.into(),
                name: id.into(),
                drawable: Some(drawable.clone()),
                transform: Transform {
                    translation: [0., 0., z],
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        scene
    }

    #[test]
    fn hidden_descendants_are_reused_until_visibility_or_ancestry_changes() -> Result<()> {
        let mut editor = Editor::new(eye_scene(), Path::new("work/eye.json"))?;
        let mut open = OpenScenes::default();
        open.set_object_visible(open.active(), "front", false);
        let first = open.hidden_objects_in(open.active(), &editor);
        assert!(first.contains("front"));
        assert!(!first.contains("rear"));
        assert!(Arc::ptr_eq(
            &first,
            &open.hidden_objects_in(open.active(), &editor)
        ));

        let mut scene = editor.scene().clone();
        scene
            .objects
            .iter_mut()
            .find(|o| o.id == "rear")
            .unwrap()
            .parent = Some("front".into());
        editor.apply("Reparent", scene)?;
        let reparented = open.hidden_objects_in(open.active(), &editor);
        assert!(reparented.contains("rear"));
        assert!(
            !first.contains("rear"),
            "published visibility snapshots remain immutable"
        );
        editor.undo()?;
        assert!(
            !open
                .hidden_objects_in(open.active(), &editor)
                .contains("rear")
        );

        open.set_object_visible(open.active(), "front", true);
        let shown = open.hidden_objects_in(open.active(), &editor);
        assert!(shown.is_empty());
        assert!(Arc::ptr_eq(
            &shown,
            &open.hidden_objects_in(open.active(), &editor)
        ));
        Ok(())
    }

    #[test]
    fn visibility_cache_invalidation_is_local_to_the_edited_document() -> Result<()> {
        let mut editor = Editor::new(eye_scene(), Path::new("work/eye-first.json"))?;
        let mut open = OpenScenes::default();
        let first_id = open.active();
        open.set_object_visible(first_id, "front", false);
        let first = open.hidden_objects_in(first_id, &editor);
        let second_id = open.add(
            &mut editor,
            Editor::new(eye_scene(), Path::new("work/eye-second.json"))?,
        )?;
        let second = open.hidden_objects_in(second_id, &editor);
        open.set_object_visible(second_id, "rear", false);
        assert!(second.is_empty(), "published snapshots stay immutable");
        assert!(open.hidden_objects_in(second_id, &editor).contains("rear"));
        assert!(Arc::ptr_eq(
            &first,
            &open.hidden_objects_in(first_id, open.document(&editor, first_id).unwrap())
        ));
        // Whole-scene visibility and active-document changes do not alter ancestry.
        open.set_visible(first_id, false);
        open.activate(&mut editor, first_id)?;
        assert!(Arc::ptr_eq(
            &first,
            &open.hidden_objects_in(first_id, &editor)
        ));
        open.set_object_visible(first_id, "front", true);
        assert!(open.hidden_objects_in(first_id, &editor).is_empty());
        Ok(())
    }

    #[test]
    fn eye_refreshes_live_and_paused_previews_and_picking_without_changing_play() -> Result<()> {
        for edits in [0, 2] {
            let mut editor = Editor::new(eye_scene(), Path::new("work/eye.json"))?;
            for index in 0..edits {
                let mut scene = editor.scene().clone();
                scene.name = format!("Edited {index}");
                editor.apply("Rename scene", scene)?;
            }
            let original = editor.scene().clone();
            let revision = editor.revision();
            let history = editor.undo_label().map(str::to_owned);
            let mut open = OpenScenes::default();
            let mut effects = EffectsPreview::new(&editor)?;
            assert_eq!(effects.render(&editor, Layer::ThreeD, 1.)?.items.len(), 2);
            for toggle in 0..8 {
                let visible = toggle % 2 == 1;
                open.set_object_visible(open.active(), "front", visible);
                open.sync_view(&editor)?;
                let view = open.view(&editor);
                // Exercise a click after advance as well as the next frame's advance.
                if toggle % 3 == 1 {
                    effects.advance(view, Duration::from_millis(16), true)?;
                } else if toggle % 3 == 2 {
                    effects.advance(view, Duration::ZERO, false)?;
                }
                assert_eq!(
                    effects.render(view, Layer::ThreeD, 1.)?.items.len(),
                    if visible { 2 } else { 1 }
                );
                let pick = view.pick_with_projection(Layer::ThreeD, Mat4::IDENTITY, [0.; 2])?;
                assert_eq!(
                    pick.as_deref(),
                    Some(if visible { "front" } else { "rear" })
                );
            }
            open.set_object_visible(open.active(), "front", false);
            editor.start_play()?;
            open.sync_view(&editor)?;
            assert_eq!(open.view(&editor).render(Layer::ThreeD, 1.)?.items.len(), 2);
            editor.stop_play();
            open.sync_view(&editor)?;
            assert_eq!(
                effects
                    .render(open.view(&editor), Layer::ThreeD, 1.)?
                    .items
                    .len(),
                1
            );
            assert_eq!(editor.scene(), &original);
            assert_eq!(editor.revision(), revision);
            assert_eq!(editor.undo_label(), history.as_deref());
        }
        Ok(())
    }

    #[test]
    fn eye_can_hide_compound_shapes_and_joint_endpoints_in_single_and_additive_views() -> Result<()>
    {
        let mut scene = eye_scene();
        let front = scene.objects.iter_mut().find(|o| o.id == "front").unwrap();
        front.gravity = Some(Default::default());
        scene.objects.push(Object {
            id: "shape".into(),
            name: "Shape".into(),
            parent: Some("front".into()),
            collider: Some(Default::default()),
            ..Default::default()
        });
        let rear = scene.objects.iter_mut().find(|o| o.id == "rear").unwrap();
        rear.collider = Some(Default::default());
        rear.joint = Some(bozzard_scene::Joint {
            other: "front".into(),
            ..Default::default()
        });
        let mut editor = Editor::new(scene.clone(), Path::new("work/eye.json"))?;
        let mut open = OpenScenes::default();
        for additive in [false, true] {
            if additive {
                let incoming = Editor::new(scene.clone(), Path::new("work/eye-other.json"))?;
                open.add(&mut editor, incoming)?;
            }
            for id in ["front", "rear", "shape"] {
                open.set_object_visible(open.active(), id, false);
                open.sync_view(&editor)?;
                open.view(&editor).render(Layer::ThreeD, 1.)?;
                open.set_object_visible(open.active(), id, true);
            }
        }
        assert_eq!(editor.scene(), &scene);
        assert!(!editor.dirty());
        Ok(())
    }

    #[test]
    fn eye_can_hide_a_player_controller_without_invalidating_the_preview() -> Result<()> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/first-trail.json");
        let editor = Editor::open(&path)?;
        let player = editor
            .scene()
            .objects
            .iter()
            .find(|o| o.player_controller.is_some())
            .unwrap();
        let mut open = OpenScenes::default();
        open.set_object_visible(open.active(), &player.id, false);
        open.sync_view(&editor)?;
        open.view(&editor).render(Layer::ThreeD, 1.)?;
        assert!(!editor.dirty());
        Ok(())
    }

    #[test]
    fn eye_can_hide_targets_used_by_motion_and_navigation() -> Result<()> {
        use bozzard_scene::middleware::{
            navigation::{NavAgent, NavSurface},
            registry,
            timeline::Timeline,
            tween::{Property, Track, Tween},
        };
        for timeline in [false, true] {
            let mut scene = eye_scene();
            registry::set(
                scene.objects.iter_mut().find(|o| o.id == "front").unwrap(),
                &NavSurface::default(),
            )?;
            registry::set(
                scene.objects.iter_mut().find(|o| o.id == "rear").unwrap(),
                &NavAgent {
                    surface: "front".into(),
                    ..Default::default()
                },
            )?;
            let mut track = Track::new(Property::Color);
            track.target = bozzard_scene::blueprint::ObjectRef::Id("front".into());
            let motion = Tween {
                tracks: std::sync::Arc::new(vec![track]),
                ..Default::default()
            };
            let mut controller = Object {
                id: "motion".into(),
                name: "Motion".into(),
                ..Default::default()
            };
            if timeline {
                registry::set(
                    &mut controller,
                    &Timeline {
                        motion,
                        ..Default::default()
                    },
                )?;
            } else {
                registry::set(&mut controller, &motion)?;
            }
            scene.objects.push(controller);
            let editor = Editor::new(scene.clone(), Path::new("work/eye.json"))?;
            let mut open = OpenScenes::default();
            open.set_object_visible(open.active(), "front", false);
            open.sync_view(&editor)?;
            assert_eq!(open.view(&editor).render(Layer::ThreeD, 1.)?.items.len(), 1);
            assert_eq!(editor.scene(), &scene);
        }
        Ok(())
    }

    #[test]
    fn hiding_all_canvases_does_not_regenerate_game_menus() -> Result<()> {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/ui-2d-lab.json");
        let editor = Editor::open(&path)?;
        let mut open = OpenScenes::default();
        for object in &editor.scene().objects {
            if object.extras.contains_key("ui_canvas") {
                open.set_object_visible(open.active(), &object.id, false);
            }
        }
        open.sync_view(&editor)?;
        let view = open.view(&editor);
        assert!(
            view.ui_frame(Layer::TwoD, [1280., 720.])?
                .elements
                .is_empty()
        );
        assert_eq!(
            view.edit_demo()?.instance().document().objects.len(),
            editor.scene().objects.len()
        );
        Ok(())
    }

    #[test]
    fn object_eye_hides_preview_descendants_without_changing_play_scene() {
        let mut scene = bozzard_demo::scene_document().unwrap();
        let drawable = scene
            .objects
            .iter()
            .find_map(|object| object.drawable.clone())
            .unwrap();
        scene.objects.push(Object {
            id: "eye-parent".into(),
            name: "Eye parent".into(),
            ..Default::default()
        });
        scene.objects.push(Object {
            id: "eye-child".into(),
            name: "Eye child".into(),
            parent: Some("eye-parent".into()),
            drawable: Some(drawable),
            ..Default::default()
        });
        let mut editor = Editor::new(scene.clone(), Path::new("work/eye/scene.json")).unwrap();
        let mut open = OpenScenes::default();
        let active = open.active();
        open.sync_view(&editor).unwrap();
        let preview_child = |open: &OpenScenes, editor: &Editor| {
            open.view(editor)
                .scene()
                .objects
                .iter()
                .find(|object| {
                    object.id == "eye-child" || object.id == qualified(active, "eye-child")
                })
                .unwrap()
                .drawable
                .is_some()
        };
        assert!(preview_child(&open, &editor));
        open.set_object_visible(active, "eye-parent", false);
        assert!(
            open.hidden_objects_in(active, &editor)
                .contains("eye-child")
        );
        open.sync_view(&editor).unwrap();
        assert!(!preview_child(&open, &editor));
        assert_eq!(editor.scene(), &scene);
        assert!(!editor.dirty());

        open.set_object_visible(active, "eye-child", false);
        open.set_object_visible(active, "eye-parent", true);
        open.sync_view(&editor).unwrap();
        assert!(!preview_child(&open, &editor));
        open.set_object_visible(active, "eye-child", true);
        open.sync_view(&editor).unwrap();
        assert!(preview_child(&open, &editor));

        open.set_object_visible(active, "eye-parent", false);
        editor.start_play().unwrap();
        open.sync_view(&editor).unwrap();
        assert!(
            open.view(&editor)
                .scene()
                .objects
                .iter()
                .any(|object| { object.id == "eye-child" && object.drawable.is_some() })
        );
    }

    #[test]
    fn hiding_game_hud_keeps_asset_ids_and_removes_widgets() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo/scenes/ui-2d-lab.json");
        let mut editor = Editor::open(&path).unwrap();
        editor.selected = Some("ui-game-hud".into());
        let visible = editor.ui_frame(Layer::TwoD, [1280.0, 720.0]).unwrap();
        assert!(
            visible
                .elements
                .iter()
                .any(|item| item.owner.starts_with("ui-game-hud-"))
        );
        let mut open = OpenScenes::default();
        open.set_object_visible(open.active(), "ui-game-hud", false);
        open.sync_view(&editor).unwrap();
        let preview = open.view(&editor);
        assert_eq!(open.view_asset(&editor, "atlas"), "atlas");
        assert!(preview.assets.handle("atlas").is_some());
        assert!(preview.assets.handle("panel").is_some());
        for source in editor.assets.entries() {
            let shared = preview
                .assets
                .get(preview.assets.handle(&source.id).unwrap())
                .unwrap();
            assert!(std::ptr::eq(source.data().unwrap(), shared.data().unwrap()));
        }
        assert!(
            preview
                .scene()
                .objects
                .iter()
                .all(|object| !object.id.starts_with("document-"))
        );
        let mut rendered = preview.render(Layer::TwoD, 16.0 / 9.0).unwrap();
        let widgets = preview.ui_frame(Layer::TwoD, [1280.0, 720.0]).unwrap();
        assert!(
            widgets
                .elements
                .iter()
                .all(|item| !item.owner.starts_with("ui-game-hud-"))
        );
        rendered
            .items
            .extend(bozzard_render_assets::widget_items(&widgets, &preview.assets).unwrap());
        let missing: Vec<_> = bozzard_render_assets::required_assets(&rendered)
            .into_iter()
            .filter(|id| {
                preview
                    .assets
                    .handle(id)
                    .and_then(|handle| preview.assets.get(handle))
                    .and_then(|entry| entry.data())
                    .is_none()
            })
            .collect();
        assert!(
            missing.is_empty(),
            "preview requires absent assets: {missing:?}"
        );
        open.sync_view(&editor).unwrap();
        assert_eq!(open.view(&editor).selected.as_deref(), Some("ui-game-hud"));
    }
}
