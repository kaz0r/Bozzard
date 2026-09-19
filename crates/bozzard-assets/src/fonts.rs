use crate::{AssetData, AssetStore};
use anyhow::{Context, Result};
use bozzard_scene::{Scene, TextFont, TextRendering};

impl AssetStore {
    pub fn text_font(&self, text: &TextRendering) -> Result<Option<bozzard_text::Font>> {
        let TextFont::Custom(id) = &text.font else {
            return Ok(None);
        };
        let resolve = |id: &str| -> Result<bozzard_text::Font> {
            let Some(AssetData::Font(font)) = self
                .handle(id)
                .and_then(|h| self.get(h))
                .and_then(|e| e.data())
            else {
                anyhow::bail!("font '{id}' is not loaded");
            };
            Ok(font.clone())
        };
        let font = resolve(id)?;
        let fallbacks = text
            .font_fallbacks
            .iter()
            .map(|id| resolve(id))
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(font.styled(
            &text.font_axes,
            &fallbacks,
            text.builtin_font_fallback,
        )?))
    }

    /// Data-dependent checks run after imports; the headless scene schema still
    /// checks IDs, bounded settings and finite values without loading font files.
    pub fn validate_text_fonts(&self, scene: &Scene) -> Result<()> {
        for object in &scene.objects {
            if let Some(text) = &object.text_rendering {
                self.text_font(text)
                    .with_context(|| format!("text font on '{}'", object.id))?;
            }
        }
        Ok(())
    }
}
