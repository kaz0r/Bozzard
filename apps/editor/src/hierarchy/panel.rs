use crate::{App, Color32, HierarchyDrag, Mesh, Sense, Vec2, egui};
use anyhow::Result;
use bozzard_scene::Layer;

#[derive(Default)]
struct HierarchyRowRequests {
    click_request: Option<(String, bool, bool)>,
    visibility_request: Option<(String, bool)>,
    reparent_request: Option<(String, Option<String>)>,
}

#[derive(Default)]
struct HierarchyRequests {
    matches: usize,
    visible_ids: Vec<String>,
    row: HierarchyRowRequests,
}

struct HierarchyRowContext<'a> {
    object: &'a bozzard_scene::Object,
    depth: usize,
    show_disclosure: bool,
    expandable: bool,
    hidden_objects: &'a std::collections::BTreeSet<String>,
    can_reparent: bool,
}

impl App {
    pub(crate) fn begin_hierarchy_rename(&mut self) {
        if self.editor.selected_surface().is_some() {
            self.status = "Select the whole model before renaming it".into();
            return;
        }
        if let Some((id, name)) = self
            .editor
            .selected_object()
            .map(|o| (o.id.clone(), o.name.clone()))
        {
            self.hierarchy_rename = Some((id.clone(), name, true));
            self.hierarchy_search.clear();
            self.editor.finish_gesture();
            self.hierarchy_state.reset_selection(Some(&id));
            self.hierarchy_state
                .reveal(self.editor.scene(), self.editor.selected.as_deref());
        }
    }

    pub(crate) fn duplicate_hierarchy_selection(&mut self) -> Result<()> {
        self.hierarchy_state.sync_object_selection(
            &self.editor.scene_snapshot(),
            self.editor.selected.as_deref(),
        );
        let ids = self.hierarchy_state.selected_objects();
        if ids.len() <= 1 {
            self.editor.duplicate()?;
            self.hierarchy_state
                .reset_selection(self.editor.selected.as_deref());
            return Ok(());
        }
        let active = self.editor.selected.clone();
        let replacements = self.editor.duplicate_objects(&ids)?;
        let next = active.and_then(|id| replacements.get(&id).cloned());
        self.editor.select_object(next.clone());
        self.hierarchy_state.set_selection(
            ids.into_iter()
                .filter_map(|id| replacements.get(&id).cloned()),
            next.as_deref(),
        );
        Ok(())
    }

    pub(crate) fn delete_hierarchy_selection(&mut self) -> Result<()> {
        self.hierarchy_state.sync_object_selection(
            &self.editor.scene_snapshot(),
            self.editor.selected.as_deref(),
        );
        let ids = self.hierarchy_state.selected_objects();
        if ids.len() <= 1 {
            self.editor.delete()?;
        } else {
            self.editor.delete_objects(&ids)?;
        }
        self.hierarchy_state
            .reset_selection(self.editor.selected.as_deref());
        Ok(())
    }

    pub(crate) fn hierarchy(&mut self, ui: &mut egui::Ui) {
        self.scene_documents(ui);
        if self.loading.is_some()
            || self.editor.play.is_some()
            || self.dialog.is_some()
            || self.confirm_discard
            || self.mouse_captured
            || self
                .hierarchy_rename
                .as_ref()
                .is_some_and(|(id, _, _)| self.editor.selected.as_ref() != Some(id))
        {
            self.hierarchy_rename = None;
        }
        if self.loading.is_some() {
            ui.disable();
        }
        self.hierarchy_toolbar(ui);
        let query = self.hierarchy_search.trim().to_lowercase();
        let scene = self.editor.scene_snapshot();
        let hidden_objects = self
            .open_scenes
            .hidden_objects_in(self.open_scenes.active(), &self.editor);
        self.hierarchy_state
            .sync_object_selection(&scene, self.editor.selected.as_deref());
        if self.editor.selected_surface().is_some() {
            self.hierarchy_state
                .reset_selection(self.editor.selected.as_deref());
        }
        self.hierarchy_state
            .sync_selection(&scene, self.editor.selected.as_deref());
        self.hierarchy_state.sync_surface_selection(
            self.editor
                .selected_surface()
                .and_then(|s| self.editor.selected.as_deref().map(|id| (id, s.index))),
        );

        let can_reparent = ui.is_enabled()
            && self.editor.play.is_none()
            && self.drag.is_none()
            && !self.mouse_captured
            && self.hierarchy_rename.is_none()
            && self.dialog.is_none()
            && !self.confirm_discard;
        let mut requests =
            self.render_hierarchy_rows(ui, &scene, &hidden_objects, &query, can_reparent);
        self.apply_hierarchy_interactions(ui, &mut requests, can_reparent);
        if requests.matches == 0 {
            ui.weak(if query.is_empty() {
                "Scene is empty. Use Create above to add an object."
            } else {
                "No matching objects. Clear search to see all."
            });
        } else if !query.is_empty() {
            ui.weak(format!(
                "{} of {} objects",
                requests.matches,
                scene.objects.len()
            ));
        }
        if self.hierarchy_state.selection_count() > 1 {
            ui.weak(format!(
                "{} objects selected",
                self.hierarchy_state.selection_count()
            ));
        }
    }

    fn apply_hierarchy_interactions(
        &mut self,
        ui: &mut egui::Ui,
        requests: &mut HierarchyRequests,
        can_reparent: bool,
    ) {
        if let Some((id, visible)) = requests.row.visibility_request.take() {
            self.open_scenes
                .set_object_visible(self.open_scenes.active(), &id, visible);
            self.viewport_stamp = None;
            ui.ctx().request_repaint();
        }
        if let Some((id, shift, toggle)) = requests.row.click_request.take() {
            let active =
                self.hierarchy_state
                    .click_object(&requests.visible_ids, &id, shift, toggle);
            self.editor.select_object(active);
        }
        // Only the blank area after the rows unparents objects; row gaps remain inert.
        if can_reparent {
            let size = Vec2::new(
                ui.available_width(),
                (ui.available_height() - 26.0).max(0.0),
            );
            let (_, blank) = ui.allocate_exact_size(size, Sense::hover());
            if blank.dnd_hover_payload::<HierarchyDrag>().is_some() {
                ui.painter().rect_stroke(
                    blank.rect,
                    2.0,
                    egui::Stroke::new(1.5, Color32::LIGHT_BLUE),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    blank.rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Drop to unparent",
                    egui::FontId::proportional(12.0),
                    Color32::LIGHT_BLUE,
                );
            }
            if let Some(id) = blank.dnd_release_payload::<HierarchyDrag>() {
                requests.row.reparent_request = Some((id.0.clone(), None));
            }
        }
        if let Some((id, parent)) = requests.row.reparent_request.take() {
            let result = self.editor.reparent(&id, parent.as_deref());
            if result.is_ok() {
                self.editor.select_object(Some(id));
                self.hierarchy_state
                    .reset_selection(self.editor.selected.as_deref());
                self.hierarchy_state
                    .reveal(self.editor.scene(), self.editor.selected.as_deref());
                self.status = "Parent updated · World transform preserved".into();
                self.error = false;
            }
            self.result(result);
        }
    }

    fn hierarchy_toolbar(&mut self, ui: &mut egui::Ui) {
        crate::theme::panel_title(ui, "Scene Hierarchy");
        ui.horizontal(|ui| {
            ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                ui.menu_button("+ Create", |ui| {
                    if ui.button("+ Empty Object").clicked() {
                        let r = self.editor.create_empty();
                        if r.is_ok() {
                            self.hierarchy_search.clear();
                        }
                        self.result(r);
                        ui.close();
                    }
                    if ui.button("+ Cube").clicked() {
                        let r = self.editor.create(Mesh::Cube, Layer::ThreeD);
                        if r.is_ok() {
                            self.hierarchy_search.clear();
                            self.workspace.layer_2d = false;
                        }
                        self.result(r);
                        ui.close();
                    }
                    if ui.button("+ Sprite").clicked() {
                        let r = self.editor.create(Mesh::Quad, Layer::TwoD);
                        if r.is_ok() {
                            self.hierarchy_search.clear();
                            self.workspace.layer_2d = true;
                        }
                        self.result(r);
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Particles", |ui| {
                        for kind in bozzard_scene::ParticleKind::ALL {
                            if ui.button(kind.name()).clicked() {
                                let result = self.editor.create_particle_emitter(kind);
                                if result.is_ok() {
                                    self.workspace.layer_2d = false;
                                    self.hierarchy_search.clear();
                                }
                                self.result(result);
                                ui.close();
                            }
                        }
                    });
                    for (kind, label) in [
                        (bozzard_scene::LightKind::Point, "Point light"),
                        (bozzard_scene::LightKind::Spot, "Spot light"),
                        (bozzard_scene::LightKind::Directional, "Directional light"),
                    ] {
                        if ui.button(label).clicked() {
                            let result = self.editor.create_light(kind);
                            if result.is_ok() {
                                self.workspace.layer_2d = false;
                                self.hierarchy_search.clear();
                            }
                            self.result(result);
                            ui.close();
                        }
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("···", |ui| {
                    ui.add_enabled_ui(self.hierarchy_search.trim().is_empty(), |ui| {
                        if ui.button("Expand all").clicked() {
                            self.hierarchy_state.expand_all();
                            ui.close();
                        }
                        if ui.button("Collapse all").clicked() {
                            self.hierarchy_state.collapse_all(self.editor.scene());
                            ui.close();
                        }
                    });
                })
                .response
                .on_hover_text("Hierarchy options");
                ui.weak(format!("{} entities", self.editor.scene().objects.len()));
            });
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.hierarchy_search)
                    .hint_text("Search entities or materials…")
                    .desired_width(ui.available_width() - 28.0),
            );
            if ui.small_button("×").on_hover_text("Clear search").clicked() {
                self.hierarchy_search.clear();
            }
        });
    }

    fn render_hierarchy_object_row(
        &mut self,
        ui: &mut egui::Ui,
        context: HierarchyRowContext<'_>,
    ) -> HierarchyRowRequests {
        let HierarchyRowContext {
            object,
            depth,
            show_disclosure,
            expandable,
            hidden_objects,
            can_reparent,
        } = context;
        let mut requests = HierarchyRowRequests::default();
        crate::left_aligned_hierarchy_row(ui, |ui| {
            if show_disclosure {
                ui.add_space((depth.min(12) * 12) as f32);
                let collapsed = self.hierarchy_state.is_collapsed(&object.id);
                let disclosure = super::disclosure_slot(ui, expandable, collapsed);
                if expandable
                    && disclosure
                        .on_hover_text(if collapsed {
                            "Expand children"
                        } else {
                            "Collapse children"
                        })
                        .clicked()
                {
                    self.hierarchy_state.toggle(&object.id);
                }
            }

            let hidden_by_parent = object
                .parent
                .as_ref()
                .is_some_and(|parent| hidden_objects.contains(parent));
            let visible = !hidden_objects.contains(&object.id);
            let eye = ui
                .add_enabled_ui(
                    self.editor.play.is_none() && self.drag.is_none() && !hidden_by_parent,
                    |ui| super::visibility_eye(ui, visible),
                )
                .inner
                .on_hover_text(if self.editor.play.is_some() {
                    "Editor visibility is available after Play"
                } else if hidden_by_parent {
                    "Hidden by parent · Show the parent to restore this object"
                } else if visible {
                    "Hide this object and its children in the editor viewport"
                } else {
                    "Show this object and its children in the editor viewport"
                });
            if eye.clicked()
                && self.editor.play.is_none()
                && self.drag.is_none()
                && !hidden_by_parent
            {
                requests.visibility_request = Some((object.id.clone(), !visible));
            }

            if self.render_hierarchy_rename(ui, object) {
                return;
            }

            let kind = if object.camera.is_some() {
                "◉"
            } else if object.drawable.is_some() {
                "◇"
            } else {
                "·"
            };
            let response = crate::clipped_selectable_row(
                ui,
                self.hierarchy_state.is_selected(&object.id)
                    && self.editor.selected_surface().is_none(),
                format!("{kind} {}", object.name),
            )
            .interact(if can_reparent {
                Sense::click_and_drag()
            } else {
                Sense::click()
            })
            .on_hover_text(format!(
                "{}\n{} · Shift-click selects a range · Ctrl/Cmd-click toggles · Double-click to frame · Drag to reparent",
                object.name, object.id
            ));
            if can_reparent {
                response.dnd_set_drag_payload(HierarchyDrag(object.id.clone()));
                if response.dnd_hover_payload::<HierarchyDrag>().is_some() {
                    ui.painter().rect_stroke(
                        response.rect,
                        2.0,
                        egui::Stroke::new(1.5, Color32::LIGHT_BLUE),
                        egui::StrokeKind::Inside,
                    );
                }
                if let Some(id) = response.dnd_release_payload::<HierarchyDrag>() {
                    requests.reparent_request = Some((id.0.clone(), Some(object.id.clone())));
                }
            }
            if response.clicked() || response.double_clicked() {
                self.editor.finish_gesture();
                let modifiers = ui.input(|i| i.modifiers);
                requests.click_request =
                    Some((object.id.clone(), modifiers.shift, modifiers.command));
            }
            if response.secondary_clicked() && !self.hierarchy_state.is_selected(&object.id) {
                self.editor.finish_gesture();
                self.hierarchy_state.reset_selection(Some(&object.id));
                self.editor.select_object(Some(object.id.clone()));
            }
            if response.double_clicked()
                && self.editor.play.is_none()
                && self.drag.is_none()
                && !self.mouse_captured
            {
                self.hierarchy_frame_requested = true;
            }
            response.context_menu(|ui| {
                self.hierarchy_object_context_menu(ui, object, &mut requests.reparent_request);
            });
        });
        requests
    }

    fn render_hierarchy_rename(
        &mut self,
        ui: &mut egui::Ui,
        object: &bozzard_scene::Object,
    ) -> bool {
        let Some((id, name, focus)) = &mut self.hierarchy_rename else {
            return false;
        };
        if id != &object.id {
            return false;
        }
        let response = ui.add(
            egui::TextEdit::singleline(name)
                .id_salt(("hierarchy-rename", id.as_str()))
                .desired_width(ui.available_width()),
        );
        let first = std::mem::take(focus);
        if first {
            response.request_focus();
            response.scroll_to_me(Some(egui::Align::Center));
        }
        let cancel = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let apply = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if cancel || apply || (!first && response.lost_focus()) {
            let (id, name, _) = self.hierarchy_rename.take().unwrap();
            response.surrender_focus();
            if apply && !cancel {
                let mut scene = self.editor.scene().clone();
                if let Some(target) = scene.objects.iter_mut().find(|o| o.id == id) {
                    target.name = name;
                    let result = self.editor.apply("Rename object", scene);
                    self.result(result);
                }
            }
        }
        true
    }

    fn hierarchy_object_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        object: &bozzard_scene::Object,
        reparent_request: &mut Option<(String, Option<String>)>,
    ) {
        ui.set_min_width(290.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        let mac = cfg!(target_os = "macos");
        ui.add_enabled_ui(
            self.editor.play.is_none()
                && self.loading.is_none()
                && self.drag.is_none()
                && !self.mouse_captured,
            |ui| {
                if ui
                    .add(egui::Button::new("Rename").shortcut_text(if mac {
                        "Cmd+Return"
                    } else {
                        "F2"
                    }))
                    .clicked()
                {
                    self.editor.select_object(Some(object.id.clone()));
                    self.begin_hierarchy_rename();
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Duplicate").shortcut_text(if mac {
                        "Cmd+D"
                    } else {
                        "Ctrl+D"
                    }))
                    .clicked()
                {
                    self.editor.finish_gesture();
                    if !self.hierarchy_state.is_selected(&object.id) {
                        self.hierarchy_state.reset_selection(Some(&object.id));
                        self.editor.select_object(Some(object.id.clone()));
                    }
                    let result = self.duplicate_hierarchy_selection();
                    if result.is_ok() {
                        self.hierarchy_search.clear();
                    }
                    self.result(result);
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Frame Selection").shortcut_text(if mac {
                        "Cmd+Shift+F"
                    } else {
                        "Ctrl+Shift+F"
                    }))
                    .clicked()
                {
                    self.editor.select_object(Some(object.id.clone()));
                    self.hierarchy_state.reset_selection(Some(&object.id));
                    self.hierarchy_frame_requested = true;
                    ui.close();
                }
                if ui
                    .add_enabled(object.parent.is_some(), egui::Button::new("Unparent"))
                    .on_hover_text(
                        "Move to Scene root, preserving world transform · Undo to restore",
                    )
                    .clicked()
                {
                    *reparent_request = Some((object.id.clone(), None));
                    ui.close();
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Delete").shortcut_text(if mac {
                        "Cmd+Backspace"
                    } else {
                        "Delete"
                    }))
                    .clicked()
                {
                    self.editor.finish_gesture();
                    if !self.hierarchy_state.is_selected(&object.id) {
                        self.hierarchy_state.reset_selection(Some(&object.id));
                        self.editor.select_object(Some(object.id.clone()));
                    }
                    let result = self.delete_hierarchy_selection();
                    self.result(result);
                    ui.close();
                }
            },
        );
    }

    fn render_hierarchy_rows(
        &mut self,
        ui: &mut egui::Ui,
        scene: &bozzard_scene::Scene,
        hidden_objects: &std::collections::BTreeSet<String>,
        query: &str,
        can_reparent: bool,
    ) -> HierarchyRequests {
        let children = super::children(scene);
        let mut stack: Vec<_> = children
            .get(&None)
            .into_iter()
            .flatten()
            .rev()
            .map(|o| (*o, 0usize))
            .collect();
        let mut requests = HierarchyRequests::default();
        if can_reparent {
            let root = ui.small("▼  Scene Collection").on_hover_text(
                "Scene root · Drop here to unparent, preserving world transform · Undo to restore",
            );
            if root.dnd_hover_payload::<HierarchyDrag>().is_some() {
                ui.painter().rect_stroke(
                    root.rect,
                    2.0,
                    egui::Stroke::new(1.5, Color32::LIGHT_BLUE),
                    egui::StrokeKind::Inside,
                );
            }
            if let Some(id) = root.dnd_release_payload::<HierarchyDrag>() {
                requests.row.reparent_request = Some((id.0.clone(), None));
            }
        }
        egui::ScrollArea::vertical()
            .id_salt("hierarchy-tree")
            .max_height((ui.available_height() - 24.0).max(1.0))
            .show(ui, |ui| {
                while let Some((object, depth)) = stack.pop() {
                    let object_matches = query.is_empty()
                        || object.name.to_lowercase().contains(query)
                        || object.id.to_lowercase().contains(query);
                    let mesh = object.drawable.as_ref().and_then(|drawable| {
                        let Mesh::Asset(id) = &drawable.mesh else {
                            return None;
                        };
                        match self
                            .editor
                            .assets
                            .get(self.editor.assets.handle(id)?)?
                            .data()?
                        {
                            bozzard_assets::AssetData::Mesh(mesh) => Some(mesh),
                            _ => None,
                        }
                    });
                    let has_surfaces = mesh.is_some_and(|m| !m.parts.is_empty());
                    let surface_matches = mesh.is_some_and(|m| {
                        m.parts.iter().enumerate().any(|(index, part)| {
                            crate::surfaces::surface_matches(index, part, query)
                        })
                    });
                    let expandable =
                        has_surfaces || children.contains_key(&Some(object.id.as_str()));
                    if object_matches || surface_matches {
                        requests.matches += 1;
                        requests.visible_ids.push(object.id.clone());
                        let row = self.render_hierarchy_object_row(
                            ui,
                            HierarchyRowContext {
                                object,
                                depth,
                                show_disclosure: query.is_empty(),
                                expandable,
                                hidden_objects,
                                can_reparent,
                            },
                        );
                        if row.click_request.is_some() {
                            requests.row.click_request = row.click_request;
                        }
                        if row.visibility_request.is_some() {
                            requests.row.visibility_request = row.visibility_request;
                        }
                        if row.reparent_request.is_some() {
                            requests.row.reparent_request = row.reparent_request;
                        }
                    }
                    if !self
                        .hierarchy_state
                        .visit_children(&object.id, !query.is_empty())
                    {
                        continue;
                    }
                    self.hierarchy_surfaces(
                        ui,
                        &object.id,
                        depth + 1,
                        if object_matches { "" } else { query },
                    );
                    for child in children
                        .get(&Some(object.id.as_str()))
                        .into_iter()
                        .flatten()
                        .rev()
                    {
                        stack.push((child, depth + 1));
                    }
                }
            });
        requests
    }
}
