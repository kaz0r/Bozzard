use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplaySettings {
    /// Stops applied to HDR radiance before display mapping. +1 doubles exposure.
    pub exposure_ev: f32,
    /// Per-channel Reinhard curve. Disable for an exposure-only preview.
    pub tone_mapping: bool,
}
impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            exposure_ev: 0.,
            tone_mapping: true,
        }
    }
}
impl DisplaySettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.exposure_ev.is_finite() && (-16.0..=16.0).contains(&self.exposure_ev),
            "exposure must be finite and within -16..16 stops"
        );
        Ok(())
    }
}
