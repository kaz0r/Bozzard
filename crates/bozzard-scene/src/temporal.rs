use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
    pub fn blend(self, other: Self, t: f32) -> Self {
        Self {
            enabled: if t < 0.5 { self.enabled } else { other.enabled },
            history_weight: mix(self.history_weight, other.history_weight, t),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
    pub fn blend(self, other: Self, t: f32) -> Self {
        Self {
            enabled: self.enabled || other.enabled,
            shutter_angle: mix(
                if self.enabled { self.shutter_angle } else { 0. },
                if other.enabled {
                    other.shutter_angle
                } else {
                    0.
                },
                t,
            ),
            max_radius: mix(self.max_radius, other.max_radius, t),
            samples: if t < 0.5 { self.samples } else { other.samples },
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
    pub fn blend(self, other: Self, t: f32) -> Self {
        Self {
            enabled: self.enabled || other.enabled,
            strength: mix(
                if self.enabled { self.strength } else { 0. },
                if other.enabled { other.strength } else { 0. },
                t,
            ),
            max_distance: mix(self.max_distance, other.max_distance, t),
            thickness: mix(self.thickness, other.thickness, t),
            roughness_cutoff: mix(self.roughness_cutoff, other.roughness_cutoff, t),
            steps: if t < 0.5 { self.steps } else { other.steps },
        }
    }
}
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0., 1.)
}
fn range(v: f32, min: f32, max: f32, name: &str) -> Result<()> {
    ensure!(
        v.is_finite() && (min..=max).contains(&v),
        "{name} must be finite and within {min}..{max}"
    );
    Ok(())
}
