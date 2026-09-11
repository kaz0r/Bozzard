use bozzard_scene::Scene;
use std::collections::BTreeSet;

/// Transient navigation state, never serialized into a scene or its history.
#[derive(Default)]
pub struct HierarchyState {
    collapsed: BTreeSet<String>,
    selection_path: Vec<String>,
    surface_selection: Option<(String, usize)>,
}
impl HierarchyState {
    pub fn is_collapsed(&self, id: &str) -> bool {
        self.collapsed.contains(id)
    }
    pub fn toggle(&mut self, id: &str) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
    }
    pub fn expand_all(&mut self) {
        self.collapsed.clear();
    }
    pub fn collapse_all(&mut self, scene: &Scene) {
        self.collapsed = scene
            .objects
            .iter()
            .flat_map(|o| {
                o.parent.clone().into_iter().chain(
                    o.drawable
                        .as_ref()
                        .filter(|d| matches!(d.mesh, bozzard_scene::Mesh::Asset(_)))
                        .map(|_| o.id.clone()),
                )
            })
            .collect();
    }
    pub fn sync_selection(&mut self, scene: &Scene, selected: Option<&str>) {
        self.collapsed
            .retain(|id| scene.objects.iter().any(|o| &o.id == id));
        let mut path = Vec::new();
        let mut next = selected;
        while let Some(id) = next {
            if path.iter().any(|p| p == id) {
                break;
            }
            path.push(id.to_owned());
            next = scene
                .objects
                .iter()
                .find(|o| o.id == id)
                .and_then(|o| o.parent.as_deref());
        }
        if path != self.selection_path {
            for id in path.iter().skip(1) {
                self.collapsed.remove(id);
            }
            self.selection_path = path;
        }
    }
    pub fn sync_surface_selection(&mut self, selected: Option<(&str, usize)>) {
        let selected = selected.map(|(id, index)| (id.to_owned(), index));
        if selected != self.surface_selection {
            if let Some((id, _)) = &selected {
                for id in self.selection_path.iter().chain(std::iter::once(id)) {
                    self.collapsed.remove(id);
                }
            }
            self.surface_selection = selected;
        }
    }
    pub fn reveal(&mut self, scene: &Scene, selected: Option<&str>) {
        self.selection_path.clear();
        self.sync_selection(scene, selected);
    }
    pub fn visit_children(&self, id: &str, searching: bool) -> bool {
        searching || !self.is_collapsed(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> Scene {
        Scene::from_json(r#"{"version":1,"name":"Tree","views":{},"objects":[
          {"id":"root","name":"Root","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
          {"id":"branch","name":"Branch","parent":"root","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
          {"id":"leaf","name":"Leaf","parent":"branch","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]}}
        ]}"#).unwrap()
    }
    #[test]
    fn collapse_search_and_expand_do_not_modify_scene() {
        let scene = scene();
        let before = scene.clone();
        let mut tree = HierarchyState::default();
        tree.collapse_all(&scene);
        assert!(!tree.visit_children("root", false));
        assert!(tree.visit_children("root", true));
        assert!(!tree.is_collapsed("leaf"));
        tree.toggle("root");
        assert!(tree.visit_children("root", false));
        tree.expand_all();
        assert!(!tree.is_collapsed("branch"));
        assert_eq!(scene, before);
    }
    #[test]
    fn new_selection_reveals_ancestors_but_manual_collapse_sticks() {
        let scene = scene();
        let mut tree = HierarchyState::default();
        tree.collapse_all(&scene);
        tree.sync_selection(&scene, Some("leaf"));
        assert!(!tree.is_collapsed("root"));
        assert!(!tree.is_collapsed("branch"));
        tree.collapse_all(&scene);
        tree.sync_selection(&scene, Some("leaf"));
        assert!(tree.is_collapsed("root"));
        tree.reveal(&scene, Some("leaf"));
        assert!(!tree.is_collapsed("root"));
        assert!(!tree.is_collapsed("branch"));
    }
    #[test]
    fn imported_leaf_collapses_and_surface_selection_reveals_owner_once() {
        let mut scene = scene();
        let drawable = Scene::from_json(r#"{"version":1,"name":"Model","views":{},"assets":{"mesh":{"kind":"mesh","path":"mesh.obj"}},"objects":[
          {"id":"model","name":"Model","transform":{"translation":[0,0,0],"rotation_degrees":[0,0,0],"scale":[1,1,1]},"drawable":{"layer":"3d","mesh":{"asset":"mesh"},"texture":"white","color":[1,1,1],"uv_scale":[1,1]}}
        ]}"#).unwrap().objects.remove(0).drawable;
        scene.objects[2].drawable = drawable;
        let before = scene.clone();
        let mut tree = HierarchyState::default();
        tree.collapse_all(&scene);
        assert!(!tree.visit_children("leaf", false));
        assert!(tree.visit_children("leaf", true));
        tree.sync_selection(&scene, Some("leaf"));
        tree.sync_surface_selection(Some(("leaf", 0)));
        for id in ["root", "branch", "leaf"] {
            assert!(tree.visit_children(id, false));
        }
        tree.toggle("leaf");
        tree.sync_surface_selection(Some(("leaf", 0)));
        assert!(tree.is_collapsed("leaf"), "manual collapse must stick");
        tree.collapse_all(&scene);
        tree.sync_selection(&scene, Some("leaf"));
        tree.sync_surface_selection(Some(("leaf", 1)));
        for id in ["root", "branch", "leaf"] {
            assert!(!tree.is_collapsed(id));
        }
        tree.toggle("leaf");
        tree.sync_surface_selection(None);
        tree.sync_surface_selection(Some(("leaf", 1)));
        assert!(!tree.is_collapsed("leaf"));
        assert_eq!(scene, before);
    }
    #[test]
    fn reparented_selection_is_revealed_and_deleted_ids_pruned() {
        let mut scene = scene();
        let mut tree = HierarchyState::default();
        tree.sync_selection(&scene, Some("leaf"));
        tree.collapse_all(&scene);
        scene.objects[2].parent = Some("root".into());
        scene.objects.remove(1);
        tree.sync_selection(&scene, Some("leaf"));
        assert!(!tree.is_collapsed("root"));
        assert!(!tree.is_collapsed("branch"));
    }
}
