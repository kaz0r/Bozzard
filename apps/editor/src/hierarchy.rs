use crate::egui;
use bozzard_scene::{Object, Scene};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn visibility_eye(ui: &mut egui::Ui, visible: bool) -> egui::Response {
    const HIT_SIZE: f32 = 24.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(HIT_SIZE, HIT_SIZE), egui::Sense::click());
    let color = if ui.is_enabled() && visible {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let painter = ui.painter();
    if response.hovered() && ui.is_enabled() {
        painter.rect_filled(rect, 3.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let center = rect.center();
    let point = |x: f32, y: f32| center + egui::vec2(x, y);
    let stroke = egui::Stroke::new(1.25, color);
    painter.add(egui::Shape::line(
        vec![
            point(-7.0, 0.0),
            point(-3.5, -3.5),
            point(0.0, -4.5),
            point(3.5, -3.5),
            point(7.0, 0.0),
        ],
        stroke,
    ));
    painter.add(egui::Shape::line(
        vec![
            point(-7.0, 0.0),
            point(-3.5, 3.5),
            point(0.0, 4.5),
            point(3.5, 3.5),
            point(7.0, 0.0),
        ],
        stroke,
    ));
    painter.circle_stroke(center, 2.2, stroke);
    if !visible {
        painter.line_segment([point(-6.0, 5.5), point(6.0, -5.5)], stroke);
    }
    response
}

/// Keep a leaf's empty disclosure column the same width as a parent's button.
pub(super) fn disclosure_slot(
    ui: &mut egui::Ui,
    expandable: bool,
    collapsed: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(18.0, 24.0),
        if expandable {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    if expandable {
        let visuals = ui.style().interact(&response);
        if response.hovered() {
            ui.painter().rect_filled(rect, 3.0, visuals.bg_fill);
        }
        let offsets = if collapsed {
            [[-2., -4.], [3., 0.], [-2., 4.]]
        } else {
            [[-4., -2.], [4., -2.], [0., 3.]]
        };
        ui.painter().add(egui::Shape::convex_polygon(
            offsets
                .into_iter()
                .map(|[x, y]| rect.center() + egui::vec2(x, y))
                .collect(),
            visuals.fg_stroke.color,
            egui::Stroke::NONE,
        ));
    }
    response
}

/// Preserve document order without rescanning every object for each expanded row.
pub fn children(scene: &Scene) -> BTreeMap<Option<&str>, Vec<&Object>> {
    let mut children = BTreeMap::<_, Vec<_>>::new();
    for object in &scene.objects {
        children
            .entry(object.parent.as_deref())
            .or_default()
            .push(object);
    }
    children
}

/// Transient navigation state, never serialized into a scene or its history.
#[derive(Default)]
pub struct HierarchyState {
    collapsed: BTreeSet<String>,
    selection_path: Vec<String>,
    surface_selection: Option<(String, usize)>,
    selected_objects: BTreeSet<String>,
    selection_anchor: Option<String>,
    last_primary: Option<String>,
}
impl HierarchyState {
    /// Keep hierarchy multi-selection in sync with picks from other editor panes.
    pub fn sync_object_selection(&mut self, scene: &Scene, primary: Option<&str>) {
        let existing: BTreeSet<_> = scene
            .objects
            .iter()
            .map(|object| object.id.as_str())
            .collect();
        self.selected_objects
            .retain(|id| existing.contains(id.as_str()));
        if self.last_primary.as_deref() != primary {
            self.reset_selection(primary);
        } else if self.selected_objects.is_empty() {
            self.selected_objects.extend(primary.map(str::to_owned));
        }
        if self
            .selection_anchor
            .as_deref()
            .is_some_and(|id| !existing.contains(id))
        {
            self.selection_anchor = primary.map(str::to_owned);
        }
    }
    pub fn reset_selection(&mut self, primary: Option<&str>) {
        self.selected_objects.clear();
        self.selected_objects.extend(primary.map(str::to_owned));
        self.selection_anchor = primary.map(str::to_owned);
        self.last_primary = primary.map(str::to_owned);
    }
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected_objects.contains(id)
    }
    pub fn selected_objects(&self) -> Vec<String> {
        self.selected_objects.iter().cloned().collect()
    }
    pub fn selection_count(&self) -> usize {
        self.selected_objects.len()
    }
    pub fn set_selection(&mut self, ids: impl IntoIterator<Item = String>, primary: Option<&str>) {
        self.selected_objects = ids.into_iter().collect();
        self.selection_anchor = primary.map(str::to_owned);
        self.last_primary = primary.map(str::to_owned);
    }
    /// Range order is the rendered hierarchy order, after collapse and search.
    pub fn click_object(
        &mut self,
        visible: &[String],
        target: &str,
        shift: bool,
        toggle: bool,
    ) -> Option<String> {
        if shift
            && let Some(anchor) = self.selection_anchor.as_deref()
            && let (Some(from), Some(to)) = (
                visible.iter().position(|id| id == anchor),
                visible.iter().position(|id| id == target),
            )
        {
            if !toggle {
                self.selected_objects.clear();
            }
            let (start, end) = if from <= to { (from, to) } else { (to, from) };
            self.selected_objects
                .extend(visible[start..=end].iter().cloned());
            self.last_primary = Some(target.to_owned());
            return self.last_primary.clone();
        }
        if toggle {
            if !self.selected_objects.remove(target) {
                self.selected_objects.insert(target.to_owned());
            }
            self.selection_anchor = Some(target.to_owned());
            self.last_primary = if self.selected_objects.contains(target) {
                Some(target.to_owned())
            } else {
                visible
                    .iter()
                    .find(|id| self.selected_objects.contains(*id))
                    .cloned()
                    .or_else(|| self.selected_objects.iter().next().cloned())
            };
            return self.last_primary.clone();
        }
        self.reset_selection(Some(target));
        Some(target.to_owned())
    }
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
        let objects: BTreeMap<_, _> = scene.objects.iter().map(|o| (o.id.as_str(), o)).collect();
        self.collapsed
            .retain(|id| objects.contains_key(id.as_str()));
        let mut path = Vec::new();
        let mut next = selected;
        while let Some(id) = next {
            if path.iter().any(|p| p == id) {
                break;
            }
            path.push(id.to_owned());
            next = objects.get(id).and_then(|o| o.parent.as_deref());
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
    fn walk(scene: &Scene, indexed: bool) -> Vec<&str> {
        let index = indexed.then(|| children(scene));
        let mut stack: Vec<_> = scene
            .objects
            .iter()
            .filter(|o| o.parent.is_none())
            .rev()
            .collect();
        let mut order = Vec::new();
        while let Some(object) = stack.pop() {
            order.push(object.id.as_str());
            if let Some(index) = &index {
                stack.extend(
                    index
                        .get(&Some(object.id.as_str()))
                        .into_iter()
                        .flatten()
                        .rev()
                        .copied(),
                );
            } else {
                stack.extend(
                    scene
                        .objects
                        .iter()
                        .filter(|o| o.parent.as_deref() == Some(&object.id))
                        .rev(),
                );
            }
        }
        order
    }
    #[test]
    fn indexed_traversal_keeps_document_order_after_reparenting_and_deletion() {
        let mut scene = scene();
        assert_eq!(walk(&scene, true), vec!["root", "branch", "leaf"]);
        scene.objects.swap(0, 2);
        assert_eq!(walk(&scene, true), walk(&scene, false));
        scene
            .objects
            .iter_mut()
            .find(|o| o.id == "leaf")
            .unwrap()
            .parent = None;
        assert_eq!(walk(&scene, true), walk(&scene, false));
        scene.objects.retain(|o| o.id != "branch");
        assert_eq!(walk(&scene, true), vec!["leaf", "root"]);
    }
    #[test]
    #[ignore = "release CPU benchmark; run explicitly with --ignored --nocapture"]
    fn hierarchy_traversal_benchmark() {
        let mut scene = scene();
        let template = scene.objects[0].clone();
        scene.objects = (0..2000)
            .map(|i| {
                let mut object = template.clone();
                object.id = format!("node-{i}");
                object.parent = (i > 0 && i % 7 != 0).then(|| format!("node-{}", (i - 1) / 2));
                object
            })
            .collect();
        assert_eq!(walk(&scene, true), walk(&scene, false));
        assert_eq!(walk(&scene, true).len(), 2000);
        let mut times = [Vec::new(), Vec::new()];
        for iteration in 0..110 {
            for mode in [iteration % 2, 1 - iteration % 2] {
                let start = std::time::Instant::now();
                std::hint::black_box(walk(&scene, mode == 1));
                if iteration >= 10 {
                    times[mode].push(start.elapsed().as_secs_f64() * 1000.);
                }
            }
        }
        for (mode, times) in times.iter_mut().enumerate() {
            times.sort_by(f64::total_cmp);
            println!(
                "hierarchy_benchmark indexed={} objects=2000 samples=100 median_ms={:.6} exact_order=true",
                mode == 1,
                (times[49] + times[50]) * 0.5
            );
        }
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
    fn shift_click_selects_inclusive_visible_range_in_both_directions() {
        let visible = ["root", "branch", "leaf", "next"].map(str::to_owned);
        let mut tree = HierarchyState::default();
        assert_eq!(
            tree.click_object(&visible, "branch", false, false),
            Some("branch".into())
        );
        assert_eq!(
            tree.click_object(&visible, "next", true, false),
            Some("next".into())
        );
        assert_eq!(tree.selected_objects(), vec!["branch", "leaf", "next"]);
        assert_eq!(
            tree.click_object(&visible, "root", true, false),
            Some("root".into())
        );
        assert_eq!(tree.selected_objects(), vec!["branch", "root"]);
        // Command/Ctrl-click toggles without changing the document or anchor range.
        assert_eq!(
            tree.click_object(&visible, "leaf", false, true),
            Some("leaf".into())
        );
        assert_eq!(tree.selected_objects(), vec!["branch", "leaf", "root"]);
        assert_eq!(
            tree.click_object(&visible, "leaf", false, true),
            Some("root".into())
        );
        assert_eq!(tree.selected_objects(), vec!["branch", "root"]);
    }
    #[test]
    fn hidden_anchor_falls_back_to_single_selection_and_external_pick_resets_range() {
        let scene = scene();
        let visible = ["root", "branch", "leaf"].map(str::to_owned);
        let mut tree = HierarchyState::default();
        tree.click_object(&visible, "branch", false, false);
        tree.click_object(&visible, "leaf", true, false);
        assert_eq!(tree.selection_count(), 2);
        tree.sync_object_selection(&scene, Some("root"));
        assert_eq!(tree.selected_objects(), vec!["root"]);
        assert_eq!(
            tree.click_object(&visible[1..], "leaf", true, false),
            Some("leaf".into())
        );
        assert_eq!(tree.selected_objects(), vec!["leaf"]);
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

mod panel;
