//! CPU font metrics shared by headless UI layout, renderer picking, and framing.
use anyhow::{Result, ensure};
use epaint::{
    Color32, FontFamily, FontId,
    text::{FontDefinitions, Fonts, LayoutJob, TextOptions},
};
use std::{cell::RefCell, collections::BTreeMap};
type Key = (u32, Option<u32>, bool, u8);
type Bounds = Option<[[f32; 2]; 2]>;
struct Cache {
    fonts: Fonts,
    metrics: BTreeMap<Key, BTreeMap<String, Bounds>>,
    entries: usize,
    bytes: usize,
}
fn options() -> TextOptions {
    TextOptions {
        max_texture_side: 4096,
        ..Default::default()
    }
}
thread_local! {static CACHE:RefCell<Cache>=RefCell::new(Cache{fonts:Fonts::new(options(),FontDefinitions::default()),metrics:BTreeMap::new(),entries:0,bytes:0});}
/// Alignment: 0 left, 1 center, 2 right. Positions use XY with positive Y downward.
pub fn bounds(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    monospace: bool,
    alignment: u8,
) -> Result<Bounds> {
    ensure!(
        text.len() <= 4096
            && font_size.is_finite()
            && (0.001..=1000.).contains(&font_size)
            && max_width.is_none_or(|v| v.is_finite() && (0.001..=1_000_000.).contains(&v))
            && alignment <= 2,
        "invalid text layout"
    );
    if text.is_empty() {
        return Ok(None);
    }
    let key = (
        font_size.to_bits(),
        max_width.map(f32::to_bits),
        monospace,
        alignment,
    );
    CACHE.with_borrow_mut(|cache| {
        if let Some(bounds) = cache
            .metrics
            .get(&key)
            .and_then(|metrics| metrics.get(text))
        {
            return Ok(*bounds);
        }
        if cache.entries >= 1024 || cache.bytes + text.len() > 1024 * 1024 {
            cache.metrics.clear();
            cache.bytes = 0;
            cache.entries = 0;
        }
        if cache.fonts.font_atlas_fill_ratio() > 0.8 {
            cache.fonts = Fonts::new(options(), FontDefinitions::default());
        }
        cache.fonts.begin_pass(options());
        let mut job = LayoutJob::simple(
            text.into(),
            FontId::new(
                64.,
                if monospace {
                    FontFamily::Monospace
                } else {
                    FontFamily::Proportional
                },
            ),
            Color32::WHITE,
            max_width.map_or(f32::INFINITY, |w| w * 64. / font_size),
        );
        job.halign = match alignment {
            1 => epaint::emath::Align::Center,
            2 => epaint::emath::Align::RIGHT,
            _ => epaint::emath::Align::LEFT,
        };
        let galley = cache.fonts.with_pixels_per_point(1.).layout_job(job);
        ensure!(
            cache.fonts.font_atlas_fill_ratio() < 1.,
            "text glyph atlas is full"
        );
        let rect = galley.rect.union(galley.mesh_bounds);
        let scale = font_size / 64.;
        let bounds = (galley.num_indices > 0).then_some([
            [rect.min.x * scale, rect.min.y * scale],
            [rect.max.x * scale, rect.max.y * scale],
        ]);
        cache.bytes += text.len();
        cache
            .metrics
            .entry(key)
            .or_default()
            .insert(text.into(), bounds);
        cache.entries += 1;
        Ok(bounds)
    })
}
