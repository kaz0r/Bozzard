//! Atlas clip editing and tile painting inside the normal inspector transaction.
use anyhow::Result;
use bozzard_scene::{
    Object,
    middleware::{
        curve::Repeat,
        registry,
        sprite::{Atlas, Clip, FrameEvent, Sprite, Tilemap},
    },
};
use eframe::egui::{self, Color32, Rect, Stroke, TextureHandle};
use std::sync::Arc;
fn texture(ui: &egui::Ui, assets: &bozzard_assets::AssetStore, id: &str) -> Option<TextureHandle> {
    let entry = assets.handle(id).and_then(|h| assets.get(h))?;
    let bozzard_assets::AssetData::Image(image) = entry.data()? else {
        return None;
    };
    let key = egui::Id::new(("sprite_atlas_preview", id));
    let cached = ui.ctx().data(|d| d.get_temp::<(u64, TextureHandle)>(key));
    if let Some((revision, texture)) = cached
        && revision == entry.revision()
    {
        return Some(texture);
    }
    // Large source images use the same bounded thumbnail strategy as the content browser.
    let limit = 512u32;
    let ratio = (image.width.max(image.height) as f32 / limit as f32).max(1.);
    let width = (image.width as f32 / ratio).round().max(1.) as usize;
    let height = (image.height as f32 / ratio).round().max(1.) as usize;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let sx = (x as f32 * ratio) as u32;
            let sy = (y as f32 * ratio) as u32;
            let index =
                ((sy.min(image.height - 1) * image.width + sx.min(image.width - 1)) * 4) as usize;
            rgba.extend_from_slice(&image.rgba[index..index + 4]);
        }
    }
    let texture = ui.ctx().load_texture(
        format!("atlas {id}"),
        egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba),
        egui::TextureOptions::NEAREST,
    );
    ui.ctx()
        .data_mut(|d| d.insert_temp(key, (entry.revision(), texture.clone())));
    Some(texture)
}
fn uv(atlas: Atlas, frame: u32) -> Rect {
    let [x, y, w, h] = atlas.uv(frame, [false; 2]);
    Rect::from_min_max(egui::pos2(x, y), egui::pos2(x + w, y + h))
}
pub fn component(
    ui: &mut egui::Ui,
    object: &mut Object,
    name: &str,
    assets: &bozzard_assets::AssetStore,
) -> Result<()> {
    if name == "sprite" {
        let Some(mut sprite) = registry::get::<Sprite>(object)? else {
            return Ok(());
        };
        let before = sprite.clone();
        if let Some(image) = texture(ui, assets, &sprite.image) {
            ui.add(
                egui::Image::new(&image)
                    .uv(uv(sprite.atlas, sprite.frame))
                    .fit_to_exact_size(egui::vec2(100., 100.)),
            );
        }
        ui.label("Initial clip");
        egui::ComboBox::from_id_salt("sprite_initial")
            .selected_text(if sprite.initial.is_empty() {
                "Static frame"
            } else {
                &sprite.initial
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut sprite.initial, String::new(), "Static frame");
                for clip in sprite.clips.iter() {
                    ui.selectable_value(&mut sprite.initial, clip.name.clone(), &clip.name);
                }
            });
        let mut remove = None;
        for i in 0..sprite.clips.len() {
            let mut clip = sprite.clips[i].clone();
            let original = clip.clone();
            egui::CollapsingHeader::new(format!("Clip · {}", clip.name))
                .id_salt(("sprite_clip", i))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut clip.name).char_limit(128));
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Frames per second");
                        ui.add(
                            egui::DragValue::new(&mut clip.fps)
                                .range(0.1..=240.)
                                .speed(1.),
                        );
                    });
                    egui::ComboBox::from_id_salt("repeat")
                        .selected_text(format!("{:?}", clip.repeat))
                        .show_ui(ui, |ui| {
                            for value in [Repeat::Once, Repeat::Loop, Repeat::PingPong] {
                                ui.selectable_value(&mut clip.repeat, value, format!("{value:?}"));
                            }
                        });
                    ui.horizontal_wrapped(|ui| {
                        for frame in &mut clip.frames {
                            ui.add(
                                egui::DragValue::new(frame)
                                    .range(0..=sprite.atlas.frames() - 1)
                                    .speed(1.),
                            );
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(clip.frames.len() < 4096, egui::Button::new("Add frame"))
                            .clicked()
                        {
                            clip.frames.push(clip.frames.last().copied().unwrap_or(0));
                        }
                        if ui
                            .add_enabled(
                                clip.frames.len() > 1,
                                egui::Button::new("Remove last frame"),
                            )
                            .clicked()
                        {
                            clip.frames.pop();
                            clip.events
                                .retain(|e| (e.frame as usize) < clip.frames.len());
                        }
                        if ui.button("Use whole atlas").clicked() {
                            clip.frames = (0..sprite.atlas.frames()).collect();
                            clip.events
                                .retain(|e| (e.frame as usize) < clip.frames.len());
                        }
                    });
                    ui.label("Frame events → On Sprite Event");
                    let mut remove = None;
                    for (j, event) in clip.events.iter_mut().enumerate() {
                        ui.push_id(("event", j), |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::DragValue::new(&mut event.frame)
                                        .range(0..=clip.frames.len() as u32 - 1),
                                );
                                ui.add(egui::TextEdit::singleline(&mut event.name).char_limit(256));
                                if ui.small_button("×").clicked() {
                                    remove = Some(j);
                                }
                            });
                        });
                    }
                    if let Some(j) = remove {
                        clip.events.remove(j);
                    }
                    if ui
                        .add_enabled(
                            clip.events.len() < 1024,
                            egui::Button::new("Add frame event"),
                        )
                        .clicked()
                    {
                        clip.events.push(FrameEvent {
                            frame: 0,
                            name: "Event".into(),
                        });
                    }
                });
            if clip != original {
                if sprite.initial == original.name {
                    sprite.initial = clip.name.clone();
                }
                Arc::make_mut(&mut sprite.clips)[i] = clip;
            }
        }
        if let Some(i) = remove {
            let name = Arc::make_mut(&mut sprite.clips).remove(i).name;
            if sprite.initial == name {
                sprite.initial.clear();
            }
        }
        if ui
            .add_enabled(
                sprite.clips.len() < 64,
                egui::Button::new("Add animation clip"),
            )
            .clicked()
        {
            let mut n = sprite.clips.len() + 1;
            while sprite.clips.iter().any(|c| c.name == format!("Clip {n}")) {
                n += 1;
            }
            Arc::make_mut(&mut sprite.clips).push(Clip {
                name: format!("Clip {n}"),
                ..Default::default()
            });
        }
        if sprite != before {
            registry::set(object, &sprite)?;
        }
    } else if name == "tilemap" {
        let Some(mut map) = registry::get::<Tilemap>(object)? else {
            return Ok(());
        };
        let before = map.clone();
        let image = texture(ui, assets, &map.image);
        let brush_id = egui::Id::new(("tile_brush", &object.id));
        let mut brush = ui
            .ctx()
            .data(|d| d.get_temp::<u32>(brush_id))
            .unwrap_or(1)
            .min(map.atlas.frames());
        ui.horizontal(|ui| {
            ui.label("Paint tile (0 erases)");
            ui.add(egui::DragValue::new(&mut brush).range(0..=map.atlas.frames()));
        });
        ui.ctx().data_mut(|d| d.insert_temp(brush_id, brush));
        if brush > 0 {
            let mut solid = map.solid.contains(&brush);
            if ui.checkbox(&mut solid, "This tile is solid").changed() {
                if solid {
                    Arc::make_mut(&mut map.solid).insert(brush);
                } else {
                    Arc::make_mut(&mut map.solid).remove(&brush);
                }
            }
            if let Some(image) = &image {
                ui.add(
                    egui::Image::new(image)
                        .uv(uv(map.atlas, brush - 1))
                        .fit_to_exact_size(egui::Vec2::splat(48.)),
                );
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Fill with brush").clicked() {
                map.cells = Arc::new(vec![brush; map.cells.len()]);
            }
            if ui.button("Clear map").clicked() {
                map.cells = Arc::new(vec![0; map.cells.len()]);
            }
        });
        ui.small("Click or drag to paint. Solid tiles are outlined in orange.");
        egui::ScrollArea::both()
            .id_salt("tile_painter")
            .max_height(320.)
            .show(ui, |ui| {
                let cell = 24.;
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(map.dimensions[0] as f32, map.dimensions[1] as f32) * cell,
                    egui::Sense::click_and_drag(),
                );
                let visible = rect.intersect(ui.clip_rect());
                let min = ((visible.min - rect.min) / cell).floor();
                let max = ((visible.max - rect.min) / cell).ceil();
                let painter = ui.painter_at(visible);
                for y in (min.y.max(0.) as u32)..(max.y as u32).min(map.dimensions[1]) {
                    for x in (min.x.max(0.) as u32)..(max.x as u32).min(map.dimensions[0]) {
                        let tile = map.cells[(y * map.dimensions[0] + x) as usize];
                        let cell_rect = Rect::from_min_size(
                            rect.min + egui::vec2(x as f32, y as f32) * cell,
                            egui::Vec2::splat(cell),
                        );
                        painter.rect_filled(cell_rect, 0., Color32::from_gray(28));
                        if tile > 0 {
                            if let Some(image) = &image {
                                painter.image(
                                    image.id(),
                                    cell_rect,
                                    uv(map.atlas, tile - 1),
                                    Color32::WHITE,
                                );
                            } else {
                                painter.text(
                                    cell_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    tile.to_string(),
                                    egui::FontId::monospace(10.),
                                    Color32::LIGHT_GREEN,
                                );
                            }
                        }
                        painter.rect_stroke(
                            cell_rect,
                            0.,
                            Stroke::new(
                                1.,
                                if map.solid.contains(&tile) {
                                    Color32::from_rgb(240, 150, 40)
                                } else {
                                    Color32::from_gray(65)
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                if (response.clicked() || response.dragged())
                    && let Some(point) = response
                        .interact_pointer_pos()
                        .filter(|p| visible.contains(*p))
                {
                    let p = (point - rect.min) / cell;
                    let x = p.x as u32;
                    let y = p.y as u32;
                    if x < map.dimensions[0] && y < map.dimensions[1] {
                        Arc::make_mut(&mut map.cells)[(y * map.dimensions[0] + x) as usize] = brush;
                    }
                }
            });
        if map != before {
            registry::set(object, &map)?;
        }
    }
    Ok(())
}
