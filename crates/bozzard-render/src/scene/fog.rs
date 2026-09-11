use anyhow::{Result, ensure};

/// Analytic extinction from the near plane; independent of camera projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogSettings {
    pub enabled: bool,
    pub color: [f32; 3],
    pub distance_density: f32,
    pub start_distance: f32,
    pub height_density: f32,
    pub base_height: f32,
    pub height_falloff: f32,
}
impl Default for FogSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [0.5, 0.6, 0.7],
            distance_density: 0.02,
            start_distance: 0.,
            height_density: 0.,
            base_height: 0.,
            height_falloff: 1.,
        }
    }
}
impl FogSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
            "invalid fog color"
        );
        ensure!(
            [
                self.distance_density,
                self.height_density,
                self.height_falloff
            ]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1000.).contains(v)),
            "fog densities and falloff must be within 0..1000"
        );
        ensure!(
            self.start_distance.is_finite() && (0.0..=100000.).contains(&self.start_distance),
            "fog start distance must be within 0..100000"
        );
        ensure!(
            self.base_height.is_finite() && (-100000.0..=100000.).contains(&self.base_height),
            "fog base height must be within -100000..100000"
        );
        Ok(())
    }
    pub(super) fn uniform(self, raw: bool) -> [f32; 12] {
        [
            self.color[0],
            self.color[1],
            self.color[2],
            if self.enabled && !raw { 1. } else { 0. },
            self.distance_density,
            self.start_distance,
            self.height_density,
            self.base_height,
            self.height_falloff,
            0.,
            0.,
            0.,
        ]
    }
}
