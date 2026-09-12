use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TemporalAntiAliasing {
    pub enabled: bool,
    /// Maximum contribution of reprojected history. Rejection lowers it automatically.
    pub history_weight: f32,
}
impl Default for TemporalAntiAliasing {
    fn default() -> Self {
        Self {
            enabled: false,
            history_weight: 0.9,
        }
    }
}
impl TemporalAntiAliasing {
    pub fn validate(&self) -> Result<()> {
        range(self.history_weight, 0., 0.97, "TAA history weight")
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionBlur {
    pub enabled: bool,
    /// Exposure as a fraction of a 60 Hz frame, expressed as a shutter angle.
    pub shutter_angle: f32,
    /// Maximum blur length in pixels at 1080p, scaled with viewport height.
    pub max_radius: f32,
    pub samples: u32,
}
impl Default for MotionBlur {
    fn default() -> Self {
        Self {
            enabled: false,
            shutter_angle: 180.,
            max_radius: 32.,
            samples: 12,
        }
    }
}
impl MotionBlur {
    pub fn validate(&self) -> Result<()> {
        range(self.shutter_angle, 0., 360., "shutter angle")?;
        range(self.max_radius, 0., 128., "motion blur radius")?;
        ensure!(
            (4..=32).contains(&self.samples),
            "motion blur samples must be 4..32"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenSpaceReflections {
    pub enabled: bool,
    pub strength: f32,
    pub max_distance: f32,
    /// World-space depth tolerance at a hit.
    pub thickness: f32,
    pub roughness_cutoff: f32,
    pub steps: u32,
}
impl Default for ScreenSpaceReflections {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 1.,
            max_distance: 30.,
            thickness: 0.2,
            roughness_cutoff: 0.65,
            steps: 64,
        }
    }
}
impl ScreenSpaceReflections {
    pub fn validate(&self) -> Result<()> {
        range(self.strength, 0., 1., "reflection strength")?;
        range(self.max_distance, 0.1, 200., "reflection distance")?;
        range(self.thickness, 0.005, 2., "reflection thickness")?;
        range(
            self.roughness_cutoff,
            0.05,
            1.,
            "reflection roughness cutoff",
        )?;
        ensure!(
            (16..=128).contains(&self.steps),
            "reflection steps must be 16..128"
        );
        Ok(())
    }
}
fn range(v: f32, min: f32, max: f32, name: &str) -> Result<()> {
    ensure!(
        v.is_finite() && (min..=max).contains(&v),
        "{name} must be finite and within {min}..{max}"
    );
    Ok(())
}
