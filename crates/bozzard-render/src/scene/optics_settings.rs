use anyhow::{Result, ensure};

/// Camera lens controls. Distances are world units from the camera near plane;
/// the lens assumes one world unit is a meter and a 24 mm sensor height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthOfField {
    pub enabled: bool,
    pub focus_distance: f32,
    pub focal_length_mm: f32,
    pub aperture: f32,
    /// Maximum circle-of-confusion radius in pixels at 1080 pixels high.
    pub max_blur_radius: f32,
}
impl Default for DepthOfField {
    fn default() -> Self {
        Self {
            enabled: false,
            focus_distance: 5.,
            focal_length_mm: 50.,
            aperture: 2.8,
            max_blur_radius: 18.,
        }
    }
}
fn range(value: f32, min: f32, max: f32, name: &str) -> Result<()> {
    ensure!(
        value.is_finite() && (min..=max).contains(&value),
        "{name} must be finite and within {min}..{max}"
    );
    Ok(())
}
impl DepthOfField {
    pub fn validate(&self) -> Result<()> {
        range(self.focus_distance, 0.5, 1000., "focus distance")?;
        range(self.focal_length_mm, 10., 200., "focal length")?;
        range(self.aperture, 0.7, 32., "aperture")?;
        range(self.max_blur_radius, 0., 32., "blur radius")
    }
}

/// GPU histogram metering. Manual exposure EV remains an additive compensation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoExposure {
    pub enabled: bool,
    pub strength: f32,
    pub min_ev: f32,
    pub max_ev: f32,
    pub target_gray: f32,
    /// Exponential adaptation rates per second, toward brighter/darker exposure.
    pub speed_up: f32,
    pub speed_down: f32,
    pub center_weight: f32,
}
impl Default for AutoExposure {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 1.,
            min_ev: -2.,
            max_ev: 2.,
            target_gray: 0.18,
            speed_up: 1.5,
            speed_down: 3.,
            center_weight: 0.65,
        }
    }
}
impl AutoExposure {
    pub fn validate(&self) -> Result<()> {
        range(self.strength, 0., 1., "auto exposure strength")?;
        range(self.min_ev, -16., 16., "minimum auto exposure")?;
        range(self.max_ev, -16., 16., "maximum auto exposure")?;
        ensure!(
            self.min_ev <= self.max_ev,
            "auto exposure minimum must not exceed maximum"
        );
        range(self.target_gray, 0.01, 0.5, "metering gray")?;
        range(self.speed_up, 0.01, 20., "brighten adaptation rate")?;
        range(self.speed_down, 0.01, 20., "darken adaptation rate")?;
        range(self.center_weight, 0., 1., "metering center weight")
    }
}
