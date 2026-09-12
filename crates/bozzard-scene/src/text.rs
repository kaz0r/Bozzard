use crate::Layer;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFont {
    #[default]
    Sans,
    Monospace,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
}

/// Flat, unlit text in the object's local XY plane. The anchor is the top of the
/// text, horizontally aligned at the origin; rows advance toward local -Y.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextRendering {
    pub enabled: bool,
    pub layer: Layer,
    pub text: String,
    pub font: TextFont,
    /// Font em size in local/world units, not screen pixels.
    pub font_size: f32,
    pub max_width: Option<f32>,
    pub alignment: TextAlignment,
    /// Linear RGB and straight alpha.
    pub color: [f32; 4],
}
impl Default for TextRendering {
    fn default() -> Self {
        Self {
            enabled: true,
            layer: Layer::ThreeD,
            text: "Text".into(),
            font: TextFont::Sans,
            font_size: 0.5,
            max_width: None,
            alignment: TextAlignment::Left,
            color: [1.; 4],
        }
    }
}
impl TextRendering {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.text.len() <= 4096, "text exceeds 4096 UTF-8 bytes");
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
