use anyhow::{Context, Result, ensure};
use epaint::{
    FontFamily,
    text::{FontData, FontDefinitions, VariationCoords},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Debug)]
pub struct VariationAxis {
    pub tag: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// Exact layout identity. Coordinates and fallback order are part of the key;
/// there is no per-frame ID allocation or hash collision between font styles.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FontKey {
    source: u64,
    axes: Arc<[(String, u32)]>,
    fallbacks: Arc<[u64]>,
    builtins: bool,
}

/// Validated immutable snapshot. Styled instances share source bytes; reparsing a
/// changed asset creates a new identity and invalidates both metrics and GPU text.
#[derive(Clone, Debug)]
pub struct Font {
    key: FontKey,
    data: Arc<FontData>,
    axes: Arc<[VariationAxis]>,
    fallbacks: Arc<[(u64, Arc<FontData>)]>,
    coords: VariationCoords,
}
impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
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
        let data = Arc::new(FontData::from_owned(bytes));
        let axes: Vec<_> = data
            .variation_axes()
            .into_iter()
            .map(|axis| VariationAxis {
                tag: axis.tag.to_string(),
                name: axis.name.unwrap_or_else(|| axis.tag.to_string()),
                min: axis.range.min,
                max: axis.range.max,
                default: axis.default,
            })
            .collect();
        ensure!(
            axes.len() <= 16
                && axes.iter().all(|a| a.tag.len() == 4
                    && skrifa::Tag::new_checked(a.tag.as_bytes()).is_ok()
                    && a.min.is_finite()
                    && a.max.is_finite()
                    && a.default.is_finite()
                    && a.min <= a.default
                    && a.default <= a.max),
            "invalid font variation axes"
        );
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Ok(Self {
            key: FontKey {
                source: NEXT.fetch_add(1, Ordering::Relaxed),
                axes: Arc::from([]),
                fallbacks: Arc::from([]),
                builtins: false,
            },
            data,
            axes: axes.into(),
            fallbacks: Arc::from([]),
            coords: Default::default(),
        })
    }
    pub fn id(&self) -> u64 {
        self.key.source
    }
    pub fn key(&self) -> FontKey {
        self.key.clone()
    }
    pub fn axes(&self) -> &[VariationAxis] {
        &self.axes
    }

    /// Fallbacks are raw font assets, in first-matching-glyph order. Axis values
    /// configure the primary font; fallback faces use their own default instance.
    pub fn styled(
        &self,
        axes: &BTreeMap<String, f32>,
        fallbacks: &[Font],
        builtins: bool,
    ) -> Result<Self> {
        if axes.is_empty()
            && fallbacks.is_empty()
            && !builtins
            && self.key.axes.is_empty()
            && self.fallbacks.is_empty()
            && !self.key.builtins
        {
            return Ok(self.clone());
        }
        ensure!(
            axes.len() <= 16 && fallbacks.len() <= 4,
            "font style exceeds 16 axes or 4 fallbacks"
        );
        let mut coordinates = Vec::new();
        for (tag, value) in axes {
            let axis = self
                .axes
                .iter()
                .find(|a| a.tag == *tag)
                .with_context(|| format!("font has no '{tag}' axis"))?;
            ensure!(
                value.is_finite() && (axis.min..=axis.max).contains(value),
                "font axis '{tag}' must be {}..{}",
                axis.min,
                axis.max
            );
            if *value != axis.default {
                coordinates.push((tag.clone(), value.to_bits()));
            }
        }
        for (index, font) in fallbacks.iter().enumerate() {
            ensure!(
                font.id() != self.id() && fallbacks[..index].iter().all(|f| f.id() != font.id()),
                "duplicate font in fallback chain"
            );
            ensure!(
                font.key.axes.is_empty() && font.fallbacks.is_empty() && !font.key.builtins,
                "fallbacks must be raw font assets"
            );
        }
        let key = FontKey {
            source: self.id(),
            axes: coordinates.into(),
            fallbacks: fallbacks.iter().map(Font::id).collect(),
            builtins,
        };
        if key == self.key {
            return Ok(self.clone());
        }
        Ok(Self {
            // TextFormat coordinates would also affect fallback faces. Install a
            // primary-only FontData tweak instead, once per atlas reset.
            coords: VariationCoords::new(
                key.axes
                    .iter()
                    .map(|(tag, v)| (tag.as_str(), f32::from_bits(*v))),
            ),
            key,
            data: self.data.clone(),
            axes: self.axes.clone(),
            fallbacks: fallbacks.iter().map(|f| (f.id(), f.data.clone())).collect(),
        })
    }
    pub fn family(&self) -> FontFamily {
        FontFamily::Name(format!("asset-font-{:?}", self.key).into())
    }
    pub fn install(&self, definitions: &mut FontDefinitions) {
        let family = self.family();
        let name = family.to_string();
        let data = if self.key.axes.is_empty() {
            self.data.clone()
        } else {
            let mut data = (*self.data).clone();
            data.tweak.coords = self.coords.clone();
            Arc::new(data)
        };
        definitions.font_data.insert(name.clone(), data);
        let mut chain = vec![name];
        for (id, data) in self.fallbacks.iter() {
            let name = format!("fallback-font-{id}");
            definitions.font_data.insert(name.clone(), data.clone());
            chain.push(name);
        }
        if self.key.builtins {
            chain.extend(
                definitions.families[&FontFamily::Proportional]
                    .iter()
                    .cloned(),
            );
        }
        definitions.families.insert(family, chain);
    }
}
