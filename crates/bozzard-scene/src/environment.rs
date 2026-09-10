use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Distant procedural sky. Colors are linear radiance; intensity scales all bands.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnvironmentSettings {
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
    pub ground: [f32; 3],
    pub intensity: f32,
    pub background: bool,
}
impl Default for EnvironmentSettings {
    fn default() -> Self {
        Self {
            zenith: [0.15, 0.32, 0.65],
            horizon: [0.65, 0.7, 0.8],
            ground: [0.12, 0.1, 0.08],
            intensity: 0.35,
            background: true,
        }
    }
}
impl EnvironmentSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [self.zenith, self.horizon, self.ground]
                .iter()
                .flatten()
                .all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
            "environment colors must be linear RGB in 0..1"
        );
        ensure!(
            self.intensity.is_finite() && (0.0..=1000.0).contains(&self.intensity),
            "environment intensity must be finite and in 0..1000"
        );
        Ok(())
    }
}
