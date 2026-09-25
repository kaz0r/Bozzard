//! Widget appearance/layout and translation-table authoring, within the inspector's undo transaction.
use anyhow::Result;
use bozzard_scene::{
    Object,
    middleware::{
        registry,
        ui::{Anchors, Localization, Widget, WidgetKind},
    },
};
use eframe::egui;
use std::sync::Arc;
pub fn component(
    ui: &mut egui::Ui,
    object: &mut Object,
    name: &str,
    scene: &bozzard_scene::Scene,
) -> Result<()> {
    if name == "ui_widget" {
        let Some(mut widget) = registry::get::<Widget>(object)? else {
            return Ok(());
        };
        if let Some(parent) = object
            .parent
            .as_deref()
            .and_then(|id| scene.objects.iter().find(|candidate| candidate.id == id))
            && let Some(container) = registry::get::<Widget>(parent)?
            && container.layout != bozzard_scene::middleware::ui::Layout::Absolute
        {
            ui.colored_label(egui::Color32::YELLOW, format!("Parent {:?} layout owns this widget's position and size. Edit the parent's gap, grow, padding or layout mode.", container.layout));
        }
        if widget.auto_text_height {
            ui.weak(
                "Auto text height can expand this widget vertically beyond its authored height.",
            );
        }
        let before = widget.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("Anchor preset");
            for (name, anchor, pivot) in [
                ("Top left", [0.; 2], [0.; 2]),
                ("Center", [0.5; 2], [0.5; 2]),
                ("Bottom right", [1.; 2], [1.; 2]),
            ] {
                if ui.small_button(name).clicked() {
                    widget.anchors.min = anchor;
                    widget.anchors.max = anchor;
                    widget.anchors.pivot = pivot;
                    widget.anchors.offset = [0.; 2];
                }
            }
            if ui.small_button("Stretch").clicked() {
                widget.anchors = Anchors {
                    min: [0.; 2],
                    max: [1.; 2],
                    pivot: [0.; 2],
                    size: [0.; 2],
                    ..Default::default()
                };
            }
        });
        if ui.button("Reset appearance for widget kind").clicked() {
            widget.background = match widget.kind {
                WidgetKind::Label => [0.; 4],
                WidgetKind::Image => [1.; 4],
                _ => Widget::default().background,
            };
            widget.padding = if matches!(widget.kind, WidgetKind::Image | WidgetKind::Label) {
                [0.; 4]
            } else {
                [12.; 4]
            };
        }
        for (label, values, range, speed) in [
            (
                "Padding · left / top / right / bottom",
                &mut widget.padding,
                0.0..=1000.,
                1.,
            ),
            (
                "Nine-slice borders · source pixels",
                &mut widget.border,
                0.0..=4096.,
                1.,
            ),
            (
                "Image region · U / V / width / height",
                &mut widget.uv,
                0.0..=1.,
                0.01,
            ),
        ] {
            ui.label(label);
            ui.horizontal(|ui| {
                for value in values {
                    ui.add(
                        egui::DragValue::new(value)
                            .range(range.clone())
                            .speed(speed),
                    );
                }
            });
        }
        // Keep the region valid while adjusting its origin, before document validation.
        for axis in 0..2 {
            widget.uv[axis] = widget.uv[axis].min(0.9999);
            widget.uv[axis + 2] = widget.uv[axis + 2].clamp(0.0001, 1. - widget.uv[axis]);
        }
        ui.label("Keyboard shortcuts");
        let mut remove = None;
        for (i, key) in widget.shortcuts.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("shortcut")
                        .selected_text(key.as_str())
                        .show_ui(ui, |ui| {
                            for name in bozzard_scene::keys::BOUND_KEYS {
                                ui.selectable_value(key, (*name).into(), *name);
                            }
                        });
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            widget.shortcuts.remove(i);
        }
        if ui
            .add_enabled(
                widget.shortcuts.len() < 8,
                egui::Button::new("Add shortcut"),
            )
            .clicked()
        {
            widget.shortcuts.push("Enter".into());
        }
        ui.small("Button, Toggle and Slider emit On UI Event on this object. Use Name / Value pins in its Blueprint. Tab changes focus; Enter/Space activates; arrows adjust sliders.");
        if widget != before {
            registry::set(object, &widget)?;
        }
    } else if name == "localization" {
        let Some(mut locale) = registry::get::<Localization>(object)? else {
            return Ok(());
        };
        let mut changes = Vec::new();
        let mut remove_language = None;
        for (language, table) in locale.translations.iter() {
            egui::CollapsingHeader::new(format!("{language} · {} strings", table.len()))
                .id_salt(("language", language))
                .show(ui, |ui| {
                    if ui.small_button("Remove language").clicked() {
                        remove_language = Some(language.clone());
                    }
                    // Only visible rows clone their editable text; large catalogs stay cheap when collapsed/scrolled.
                    let rows: Vec<_> = table.iter().collect();
                    egui::ScrollArea::vertical()
                        .id_salt(language)
                        .max_height(240.)
                        .show_rows(ui, 28., rows.len(), |ui, range| {
                            for i in range {
                                let (key, value) = rows[i];
                                let mut value = value.clone();
                                ui.push_id(key, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(key);
                                        if ui
                                            .add(
                                                egui::TextEdit::singleline(&mut value)
                                                    .char_limit(4096),
                                            )
                                            .changed()
                                        {
                                            changes.push((
                                                language.clone(),
                                                key.clone(),
                                                Some(value),
                                            ));
                                        }
                                        if ui.small_button("×").clicked() {
                                            changes.push((language.clone(), key.clone(), None));
                                        }
                                    });
                                });
                            }
                        });
                    let id = ui.make_persistent_id(("new_translation", language));
                    let mut key = ui
                        .ctx()
                        .data(|d| d.get_temp::<String>(id))
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut key)
                                .hint_text("menu.start")
                                .char_limit(128),
                        );
                        if ui
                            .add_enabled(
                                !key.trim().is_empty()
                                    && !table.contains_key(&key)
                                    && table.len() < 4096,
                                egui::Button::new("Add key"),
                            )
                            .clicked()
                        {
                            changes.push((language.clone(), key.clone(), Some(String::new())));
                            key.clear();
                        }
                    });
                    ui.ctx().data_mut(|d| d.insert_temp(id, key));
                });
        }
        let id = ui.make_persistent_id("new_language");
        let mut language = ui
            .ctx()
            .data(|d| d.get_temp::<String>(id))
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut language)
                    .hint_text("Language: en, sv, fr…")
                    .char_limit(32),
            );
            if ui
                .add_enabled(
                    !language.is_empty()
                        && language
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-')
                        && locale.translations.len() < 64
                        && !locale.translations.contains_key(&language),
                    egui::Button::new("Add language"),
                )
                .clicked()
            {
                Arc::make_mut(&mut locale.translations)
                    .insert(language.clone(), Default::default());
                language.clear();
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(id, language));
        for (language, key, value) in changes {
            let table = Arc::make_mut(&mut locale.translations)
                .get_mut(&language)
                .unwrap();
            if let Some(value) = value {
                table.insert(key, value);
            } else {
                table.remove(&key);
            }
        }
        if let Some(language) = remove_language {
            Arc::make_mut(&mut locale.translations).remove(&language);
        }
        if registry::get::<Localization>(object)?.as_ref() != Some(&locale) {
            registry::set(object, &locale)?;
        }
    }
    Ok(())
}
