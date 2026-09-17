//! CPU font metrics shared by headless UI layout, renderer picking, and framing.
use anyhow::{Context, Result, ensure};
use epaint::{
    Color32, FontFamily, FontId,
    text::{FontData, FontDefinitions, Fonts, LayoutJob, TextOptions},
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// Validated immutable font snapshot. Identity follows the snapshot, not an asset
/// name: two projects can use the same name without sharing fonts or stale metrics.
#[derive(Clone, Debug)]
pub struct Font {
    id: u64,
    data: Arc<FontData>,
}
impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Font {
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= 4 * 1024 * 1024,
            "font must be 1 byte..4 MiB"
        );
        let font = skrifa::FontRef::from_index(&bytes, 0).context("invalid TTF/OTF font")?;
        use skrifa::raw::TableProvider;
        font.head().context("font has no valid head table")?;
        font.maxp().context("font has no valid maxp table")?;
        font.cmap().context("font has no valid character map")?;
        ensure!(
            font.glyf().is_ok() || font.cff().is_ok() || font.cff2().is_ok(),
            "font needs outline glyphs"
        );
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Ok(Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            data: Arc::new(FontData::from_owned(bytes)),
        })
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn family(&self) -> FontFamily {
        FontFamily::Name(format!("asset-font-{}", self.id).into())
    }
    pub fn install(&self, definitions: &mut FontDefinitions) {
        let name = format!("asset-font-{}", self.id);
        definitions
            .font_data
            .insert(name.clone(), self.data.clone());
        definitions.families.insert(self.family(), vec![name]);
    }
}

type Key = (u32, Option<u32>, bool, u8);
type Bounds = Option<[[f32; 2]; 2]>;
struct Cache {
    fonts: Fonts,
    custom: Option<u64>,
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
thread_local! {static CACHE:RefCell<Cache>=RefCell::new(Cache{fonts:Fonts::new(options(),FontDefinitions::default()),custom:None,metrics:BTreeMap::new(),entries:0,bytes:0});}
/// Alignment: 0 left, 1 center, 2 right. Positions use XY with positive Y downward.
pub fn bounds(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    monospace: bool,
    alignment: u8,
) -> Result<Bounds> {
    bounds_with_font(text, font_size, max_width, monospace, alignment, None)
}
pub fn bounds_with_font(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    monospace: bool,
    alignment: u8,
    custom: Option<&Font>,
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
        // ponytail: one active custom font per thread; a bounded multi-font metrics
        // cache only if alternating font families becomes a measured layout cost.
        if cache.custom != custom.map(Font::id) || cache.fonts.font_atlas_fill_ratio() > 0.8 {
            let mut definitions = FontDefinitions::default();
            if let Some(font) = custom {
                font.install(&mut definitions);
            }
            cache.fonts = Fonts::new(options(), definitions);
            cache.custom = custom.map(Font::id);
            cache.metrics.clear();
            cache.entries = 0;
            cache.bytes = 0;
        }
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
        cache.fonts.begin_pass(options());
        let mut job = LayoutJob::simple(
            text.into(),
            FontId::new(
                64.,
                custom.map_or_else(
                    || {
                        if monospace {
                            FontFamily::Monospace
                        } else {
                            FontFamily::Proportional
                        }
                    },
                    Font::family,
                ),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_font_validation_metrics_and_snapshot_isolation() -> Result<()> {
        assert!(Font::parse(vec![]).is_err());
        assert!(Font::parse(vec![0; 1024]).is_err());
        assert!(Font::parse(vec![0; 4 * 1024 * 1024 + 1]).is_err());
        let defaults = FontDefinitions::default();
        let bytes = defaults.font_data["Hack"].font.to_vec();
        let font = Font::parse(bytes.clone())?;
        let other = Font::parse(bytes)?;
        assert_ne!(font, other);
        let custom = bounds_with_font("WWW iii", 1., None, false, 0, Some(&font))?;
        assert!(custom.is_some());
        assert_eq!(custom, bounds("WWW iii", 1., None, true, 0)?);
        assert_ne!(custom, bounds("WWW iii", 1., None, false, 0)?);
        assert_eq!(
            custom,
            bounds_with_font("WWW iii", 1., None, false, 0, Some(&other))?
        );
        Ok(())
    }
}
