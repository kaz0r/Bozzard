//! Rendering adapter for headless-authored UI. Geometry, hit testing and accessibility share a layout.
use anyhow::{Result, ensure};
use bozzard_render::{
    DrawItem, Material, MeshKind, ScreenText, SpriteMesh, SpriteQuad, TextMesh, TextureKind,
};
use bozzard_scene::middleware::ui::{Frame, WidgetKind};
use glam::Mat4;
/// Preserve corner texels while stretching a nine-slice image. Borders are source pixels.
pub fn nine_slice(
    size: [f32; 2],
    uv: [f32; 4],
    borders: [f32; 4],
    image: [u32; 2],
    scale: f32,
) -> Result<Vec<SpriteQuad>> {
    ensure!(
        size.iter().all(|v| v.is_finite() && *v > 0.)
            && image.iter().all(|v| *v > 0)
            && scale.is_finite()
            && scale > 0.
            && borders.iter().all(|v| v.is_finite() && *v >= 0.),
        "invalid nine-slice dimensions"
    );
    let mut source = borders;
    let mut destination = borders.map(|v| v * scale);
    for axis in 0..2 {
        let span = uv[axis + 2] * image[axis] as f32;
        ensure!(
            span > 0. && span.is_finite(),
            "invalid nine-slice image region"
        );
        let total = source[axis] + source[axis + 2];
        if total > span {
            let ratio = span / total;
            source[axis] *= ratio;
            source[axis + 2] *= ratio;
        }
        let total = destination[axis] + destination[axis + 2];
        if total > size[axis] {
            let ratio = size[axis] / total;
            destination[axis] *= ratio;
            destination[axis + 2] *= ratio;
        }
    }
    let xs = [0., destination[0], size[0] - destination[2], size[0]];
    let ys = [0., destination[1], size[1] - destination[3], size[1]];
    let us = [
        uv[0],
        uv[0] + source[0] / image[0] as f32,
        uv[0] + uv[2] - source[2] / image[0] as f32,
        uv[0] + uv[2],
    ];
    let vs = [
        uv[1],
        uv[1] + source[1] / image[1] as f32,
        uv[1] + uv[3] - source[3] / image[1] as f32,
        uv[1] + uv[3],
    ];
    let mut quads = Vec::new();
    for y in 0..3 {
        for x in 0..3 {
            if xs[x + 1] > xs[x] && ys[y + 1] > ys[y] && us[x + 1] > us[x] && vs[y + 1] > vs[y] {
                quads.push(SpriteQuad {
                    rect: [xs[x], -ys[y], xs[x + 1] - xs[x], ys[y + 1] - ys[y]],
                    uv: [us[x], vs[y], us[x + 1] - us[x], vs[y + 1] - vs[y]],
                });
            }
        }
    }
    Ok(quads)
}
fn material(color: [f32; 4], texture: TextureKind) -> Material {
    Material {
        metallic: None,
        roughness: None,
        surface_overrides: Default::default(),
        tint: [color[0], color[1], color[2]],
        uv_scale: [1.; 2],
        texture,
        lit: false,
        shader: None,
    }
}
fn image_item(
    position: [f32; 2],
    quads: Vec<SpriteQuad>,
    color: [f32; 4],
    texture: TextureKind,
    clip: [f32; 4],
) -> Result<DrawItem> {
    let mut mesh = SpriteMesh::new(quads)?;
    mesh.screen = Some(ScreenText {
        anchor: [0.; 2],
        offset: position,
    });
    mesh.clip = Some(clip);
    mesh.opacity = color[3];
    Ok(DrawItem {
        motion_id: 0,
        model: Mat4::IDENTITY,
        mesh: MeshKind::Sprite(mesh),
        material: material(color, texture),
    })
}
fn rectangle(
    position: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    clip: [f32; 4],
) -> Result<DrawItem> {
    image_item(
        position,
        vec![SpriteQuad {
            rect: [0., 0., size[0], size[1]],
            uv: [0., 0., 1., 1.],
        }],
        color,
        TextureKind::White,
        clip,
    )
}
pub fn widget_items(frame: &Frame, assets: &bozzard_assets::AssetStore) -> Result<Vec<DrawItem>> {
    let mut items = Vec::new();
    for element in &frame.elements {
        let clip = element.clip.array();
        if clip[2] <= 0. || clip[3] <= 0. {
            continue;
        }
        let w = &element.widget;
        let p = element.rect.min;
        let size = element.rect.size;
        let scale = element.scale;
        let mut color = w.background;
        if element.high_contrast && w.kind != WidgetKind::Image && color[3] > 0. {
            color = [0., 0., 0., 1.];
        }
        if w.kind.interactive() {
            let multiplier = if element.pressed {
                0.7
            } else if element.hovered {
                1.3
            } else {
                1.
            };
            for c in &mut color[..3] {
                *c = (*c * multiplier).min(1.);
            }
        }
        if color[3] > 0. {
            let image = assets
                .handle(&w.image)
                .and_then(|h| assets.get(h))
                .and_then(|e| e.data())
                .and_then(|data| {
                    if let bozzard_assets::AssetData::Image(image) = data {
                        Some(image)
                    } else {
                        None
                    }
                });
            let (texture, quads) = if let Some(image) = image {
                (
                    TextureKind::Imported(w.image.clone()),
                    nine_slice(size, w.uv, w.border, [image.width, image.height], scale)?,
                )
            } else {
                (
                    TextureKind::White,
                    vec![SpriteQuad {
                        rect: [0., 0., size[0], size[1]],
                        uv: [0., 0., 1., 1.],
                    }],
                )
            };
            items.push(image_item(p, quads, color, texture, clip)?);
        }
        let padding = w.padding.map(|v| v * scale);
        let mut text_pos = [p[0] + padding[0], p[1] + padding[1]];
        let mut width = (size[0] - padding[0] - padding[2]).max(0.001);
        if w.kind == WidgetKind::Toggle {
            let side = (element.font_size * 0.8).min((size[1] - padding[1] - padding[3]).max(1.));
            let on = element.value > (w.min + w.max) * 0.5;
            items.push(rectangle(
                [text_pos[0], p[1] + (size[1] - side) * 0.5],
                [side, side],
                if on {
                    [0.25, 0.95, 0.65, 1.]
                } else {
                    [0.3, 0.35, 0.42, 1.]
                },
                clip,
            )?);
            text_pos[0] += side + 8. * scale;
            width = (width - side - 8. * scale).max(0.001);
        }
        if w.kind == WidgetKind::Slider {
            let track_width = (size[0] - padding[0] - padding[2]).max(1.);
            let y = p[1] + size[1] - padding[3] - 4. * scale;
            let height = (4. * scale).min(size[1]);
            let ratio = ((element.value - w.min) / (w.max - w.min)).clamp(0., 1.);
            items.push(rectangle(
                [p[0] + padding[0], y],
                [track_width, height],
                [0.2, 0.25, 0.32, 1.],
                clip,
            )?);
            if ratio > 0. {
                items.push(rectangle(
                    [p[0] + padding[0], y],
                    [track_width * ratio, height],
                    [0.25, 0.95, 0.65, 1.],
                    clip,
                )?);
            }
            let thumb = (10. * scale).min(track_width);
            items.push(rectangle(
                [
                    p[0] + padding[0] + (track_width - thumb) * ratio,
                    y - 3. * scale,
                ],
                [thumb, 10. * scale],
                [0.95, 1., 0.98, 1.],
                clip,
            )?);
        }
        if !element.text.is_empty() {
            let color = if element.high_contrast {
                [1.; 4]
            } else {
                w.text_color
            };
            let mut mat = material(color, TextureKind::Text);
            if !element.enabled {
                for c in &mut mat.tint {
                    *c *= 0.55;
                }
            }
            items.push(DrawItem {
                motion_id: 0,
                model: Mat4::IDENTITY,
                mesh: MeshKind::Text(TextMesh {
                    clip: Some(clip),
                    screen: Some(ScreenText {
                        anchor: [0.; 2],
                        offset: text_pos,
                    }),
                    text: element.text.clone(),
                    font_size: element.font_size,
                    max_width: Some(width.min(10000.)),
                    opacity: color[3],
                    ..Default::default()
                }),
                material: mat,
            });
        }
        if element.focused || element.high_contrast && w.kind.interactive() {
            let line = (2. * scale).min(size[0] * 0.5).min(size[1] * 0.5);
            let color = if element.high_contrast {
                [1., 1., 0., 1.]
            } else {
                [0.3, 1., 0.7, 1.]
            };
            for (position, size) in [
                (p, [size[0], line]),
                ([p[0], p[1] + size[1] - line], [size[0], line]),
                (p, [line, size[1]]),
                ([p[0] + size[0] - line, p[1]], [line, size[1]]),
            ] {
                items.push(rectangle(position, size, color, clip)?);
            }
        }
    }
    // Overlay scroll thumbs after children so content cannot obscure the affordance.
    for element in &frame.elements {
        if element.scroll_max <= 0. || element.clip.size.iter().any(|v| *v <= 0.) {
            continue;
        }
        let size = element.rect.size;
        let width = (4. * element.scale).max(2.).min(size[0]);
        let height = (size[1] * size[1] / (size[1] + element.scroll_max))
            .max(16. * element.scale)
            .min(size[1]);
        let y = element.rect.min[1]
            + (size[1] - height) * (element.scroll / element.scroll_max).clamp(0., 1.);
        items.push(rectangle(
            [element.rect.min[0] + size[0] - width, y],
            [width, height],
            if element.high_contrast {
                [1.; 4]
            } else {
                [0.55, 0.72, 0.82, 0.85]
            },
            element.clip.array(),
        )?);
    }
    Ok(items)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nine_slice_preserves_corners_and_shrinks_borders_in_tiny_targets() {
        let quads = nine_slice([100., 60.], [0., 0., 1., 1.], [8.; 4], [32, 32], 1.).unwrap();
        assert_eq!(quads.len(), 9);
        assert_eq!(quads[0].rect, [0., 0., 8., 8.]);
        assert_eq!(quads[4].rect, [8., -8., 84., 44.]);
        assert_eq!(quads[4].uv, [0.25, 0.25, 0.5, 0.5]);
        let tiny = nine_slice([4., 4.], [0., 0., 1., 1.], [8.; 4], [32, 32], 1.).unwrap();
        assert_eq!(tiny.len(), 4);
        assert!(tiny.iter().all(|q| q.rect[2] > 0. && q.rect[3] > 0.));
    }
}
