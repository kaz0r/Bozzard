use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplaySettings {
    pub bloom: BloomSettings,
    /// Stops applied to HDR radiance before display mapping. +1 doubles exposure.
    pub exposure_ev: f32,
    /// Per-channel Reinhard curve. Disable for an exposure-only preview.
    pub tone_mapping: bool,
}
impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            bloom: BloomSettings::default(),
            exposure_ev: 0.,
            tone_mapping: true,
        }
    }
}
impl DisplaySettings {
    pub fn validate(&self) -> Result<()> {
        self.bloom.validate()?;
        ensure!(
            self.exposure_ev.is_finite() && (-16.0..=16.0).contains(&self.exposure_ev),
            "exposure must be finite and within -16..16 stops"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BloomSettings {
    pub enabled: bool,
    pub intensity: f32,
    /// Scene-linear threshold, before exposure.
    pub threshold: f32,
    pub scatter: f32,
}
impl Default for BloomSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: 0.15,
            threshold: 1.,
            scatter: 0.7,
        }
    }
}
impl BloomSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.intensity.is_finite() && (0.0..=10.).contains(&self.intensity),
            "bloom intensity must be in 0..10"
        );
        ensure!(
            self.threshold.is_finite() && (0.0..=60_000.).contains(&self.threshold),
            "bloom threshold must be in 0..60000"
        );
        ensure!(
            self.scatter.is_finite() && (0.0..=1.).contains(&self.scatter),
            "bloom scatter must be in 0..1"
        );
        Ok(())
    }
}
