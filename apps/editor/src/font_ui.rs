use bozzard_assets::{AssetData, AssetStore};
use bozzard_scene::{Object, TextFont};
use eframe::egui;

pub fn component(ui: &mut egui::Ui, object: &mut Object, name: &str, assets: &AssetStore) {
    if name != "text_rendering" {
        return;
    }
    let Some(text) = &mut object.text_rendering else {
        return;
    };
    let TextFont::Custom(id) = &text.font else {
        return;
    };
    let Some(AssetData::Font(font)) = assets
        .handle(id)
        .and_then(|h| assets.get(h))
        .and_then(|e| e.data())
    else {
        return;
    };
    if !font.axes().is_empty() {
        ui.label("Variable font axes");
        for axis in font.axes() {
            let mut value = text
                .font_axes
                .get(&axis.tag)
                .copied()
                .unwrap_or(axis.default);
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Slider::new(&mut value, axis.min..=axis.max).text(&axis.name))
                    .on_hover_text(&axis.tag)
                    .changed()
                {
                    if value == axis.default {
                        text.font_axes.remove(&axis.tag);
                    } else {
                        text.font_axes.insert(axis.tag.clone(), value);
                    }
                }
                if ui.small_button("Reset").clicked() {
                    text.font_axes.remove(&axis.tag);
                }
            });
        }
    }
    let unknown: Vec<_> = text
        .font_axes
        .keys()
        .filter(|tag| !font.axes().iter().any(|a| a.tag == **tag))
        .cloned()
        .collect();
    for tag in unknown {
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::YELLOW, format!("Unavailable axis: {tag}"));
            if ui.small_button("Remove").clicked() {
                text.font_axes.remove(&tag);
            }
        });
    }
    ui.label("Fallback fonts (first match wins)");
    let mut remove = None;
    let mut swap = None;
    for (index, id) in text.font_fallbacks.iter().enumerate() {
        ui.push_id(("fallback", index), |ui| {
            ui.horizontal(|ui| {
                ui.label(id);
                if ui.add_enabled(index > 0, egui::Button::new("↑")).clicked() {
                    swap = Some(index);
                }
                if ui.small_button("×").clicked() {
                    remove = Some(index);
                }
            })
        });
    }
    if let Some(index) = remove {
        text.font_fallbacks.remove(index);
    } else if let Some(index) = swap {
        text.font_fallbacks.swap(index, index - 1);
    }
    let can_add = text.font_fallbacks.len() < 4
        && assets.entries().any(|entry| {
            matches!(entry.data(), Some(AssetData::Font(_)))
                && &entry.id != id
                && !text.font_fallbacks.contains(&entry.id)
        });
    ui.add_enabled_ui(can_add, |ui| {
        ui.menu_button("Add fallback font", |ui| {
            for entry in assets
                .entries()
                .filter(|e| matches!(e.data(), Some(AssetData::Font(_))))
            {
                if &entry.id != id
                    && !text.font_fallbacks.contains(&entry.id)
                    && ui.button(&entry.id).clicked()
                {
                    text.font_fallbacks.push(entry.id.clone());
                    ui.close();
                }
            }
        });
    })
    .response
    .on_hover_text("Add up to four different imported TTF/OTF fonts.");
    ui.checkbox(
        &mut text.builtin_font_fallback,
        "Use bundled fonts for remaining glyphs",
    );
}
