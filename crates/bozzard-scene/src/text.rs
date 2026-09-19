use crate::Layer;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// A font family: the two built-ins or a font asset id (`AssetKind::Font`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFont {
    #[default]
    Sans,
    Monospace,
    /// A font asset; the scene's assets map must contain this id.
    Custom(String),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
}

/// Screen coordinates use a normalized anchor (0,0 top-left; 1,1 bottom-right)
/// plus a pixel offset. Text size/wrap are pixels; entity transforms are ignored.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenText {
    pub anchor: [f32; 2],
    pub offset: [f32; 2],
}
impl Default for ScreenText {
    fn default() -> Self {
        Self {
            anchor: [0.; 2],
            offset: [24.; 2],
        }
    }
}
impl ScreenText {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.anchor
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "HUD anchor must be in 0..1"
        );
        ensure!(
            self.offset
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 10000.),
            "invalid HUD offset"
        );
        Ok(())
    }
}

/// Flat, unlit text in the object's local XY plane. The anchor is the top of the
/// text, horizontally aligned at the origin; rows advance toward local -Y.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextRendering {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<ScreenText>,
    pub enabled: bool,
    pub layer: Layer,
    pub text: String,
    pub font: TextFont,
    /// OpenType axis tags and design-space values for the custom primary font.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub font_axes: std::collections::BTreeMap<String, f32>,
    /// Additional custom font assets, searched in order for missing glyphs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub font_fallbacks: Vec<String>,
    /// Search the bundled Sans/emoji chain after the custom faces.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub builtin_font_fallback: bool,
    /// Font em size in local units, or logical pixels when screen-anchored.
    pub font_size: f32,
    pub max_width: Option<f32>,
    pub alignment: TextAlignment,
    /// Linear RGB and straight alpha.
    pub color: [f32; 4],
}
impl Default for TextRendering {
    fn default() -> Self {
        Self {
            screen: None,
            enabled: true,
            layer: Layer::ThreeD,
            text: "Text".into(),
            font: TextFont::Sans,
            font_axes: Default::default(),
            font_fallbacks: Vec::new(),
            builtin_font_fallback: false,
            font_size: 0.5,
            max_width: None,
            alignment: TextAlignment::Left,
            color: [1.; 4],
        }
    }
}
impl TextRendering {
    pub fn validate(&self) -> Result<()> {
        if let Some(screen) = self.screen {
            screen.validate()?;
        }
        ensure!(self.text.len() <= 4096, "text exceeds 4096 UTF-8 bytes");
        if let TextFont::Custom(id) = &self.font {
            ensure!(
                !id.is_empty() && id.len() <= 128,
                "custom font needs an asset id"
            );
            let mut unique = std::collections::BTreeSet::from([id]);
            ensure!(
                self.font_fallbacks.len() <= 4
                    && self
                        .font_fallbacks
                        .iter()
                        .all(|id| !id.is_empty() && id.len() <= 128 && unique.insert(id)),
                "font fallbacks must be at most four distinct font assets"
            );
        } else {
            ensure!(
                self.font_axes.is_empty()
                    && self.font_fallbacks.is_empty()
                    && !self.builtin_font_fallback,
                "font axes and fallback settings need a custom primary font"
            );
        }
        ensure!(
            self.font_axes.len() <= 16
                && self.font_axes.iter().all(|(tag, value)| tag.len() == 4
                    && !tag.trim_end_matches(' ').is_empty()
                    && tag
                        .trim_end_matches(' ')
                        .bytes()
                        .all(|b| (33..=126).contains(&b))
                    && value.is_finite()),
            "invalid font variation coordinates"
        );
        ensure!(
            self.font_size.is_finite() && (0.001..=1000.).contains(&self.font_size),
            "invalid text font size"
        );
        ensure!(
            self.max_width
                .is_none_or(|w| w.is_finite() && (0.001..=10000.).contains(&w)),
            "invalid text wrap width"
        );
        ensure!(
            self.color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
            "invalid text color"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn font_style_validation_checks_tags_finite_coordinates_and_fallback_identity() {
        let mut text = TextRendering {
            font: TextFont::Custom("primary".into()),
            ..Default::default()
        };
        text.font_axes.insert("wght".into(), 900.);
        text.font_fallbacks.push("fallback".into());
        assert!(text.validate().is_ok());
        text.font_fallbacks.push("primary".into());
        assert!(text.validate().is_err());
        text.font_fallbacks.pop();
        text.font_fallbacks.push("fallback".into());
        assert!(text.validate().is_err());
        text.font_fallbacks.pop();
        text.font_axes.insert("bad".into(), 1.);
        assert!(text.validate().is_err());
        text.font_axes.remove("bad");
        text.font_axes.insert("wght".into(), f32::NAN);
        assert!(text.validate().is_err());
        text.font_axes.clear();
        text.font = TextFont::Sans;
        assert!(text.validate().is_err());
    }
    #[test]
    fn bounds_are_checked_in_bytes_and_disabled_components_still_validate() {
        let mut text = TextRendering {
            enabled: false,
            text: "😀".repeat(1024),
            ..Default::default()
        };
        assert!(text.validate().is_ok());
        text.text.push('x');
        assert!(text.validate().is_err());
        text.text.clear();
        for bad in [0., -1., f32::NAN, f32::INFINITY, 10001.] {
            text.max_width = Some(bad);
            assert!(text.validate().is_err());
        }
        text.max_width = None;
        for bad in [0., -1., f32::NAN, f32::INFINITY, 1001.] {
            text.font_size = bad;
            assert!(text.validate().is_err());
        }
        text.font_size = 0.5;
        for bad in [-1., 2., f32::NAN, f32::INFINITY] {
            text.color[3] = bad;
            assert!(text.validate().is_err());
        }
    }
}
