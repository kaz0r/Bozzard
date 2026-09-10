use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Scene-wide sun and diffuse ambient illumination. Colors are linear RGB.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Lighting {
    pub shadows: bool,
    pub shadow_resolution: u32,
    /// World-space depth and normal offsets for shadow acne control.
    pub shadow_bias: f32,
    pub shadow_normal_bias: f32,
    /// World-space direction from a surface toward the sun; normalized at rendering.
    pub sun_direction: [f32; 3],
    pub sun_color: [f32; 3],
    pub sun_intensity: f32,
    pub ambient_color: [f32; 3],
    pub ambient_intensity: f32,
}
impl Default for Lighting {
    fn default() -> Self {
        Self {
            shadows: true,
            shadow_resolution: 2048,
            shadow_bias: 0.005,
            shadow_normal_bias: 0.01,
            sun_direction: [0.4, 0.8, 0.6],
            sun_color: [1.; 3],
            sun_intensity: 3.,
            ambient_color: [1.; 3],
            ambient_intensity: 0.03,
        }
    }
}
impl Lighting {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.shadow_resolution.is_power_of_two()
                && (256..=4096).contains(&self.shadow_resolution),
            "shadow resolution must be a power of two in 256..4096"
        );
        ensure!(
            [self.shadow_bias, self.shadow_normal_bias]
                .iter()
                .all(|x| x.is_finite() && (0.0..=1.0).contains(x)),
            "shadow bias must be finite and in 0..1 world units"
        );
        let direction = glam::Vec3::from(self.sun_direction);
        ensure!(
            direction.is_finite()
                && direction.length_squared().is_finite()
                && direction.length_squared() > 1e-12,
            "sun direction must be finite and nonzero"
        );
        for color in [self.sun_color, self.ambient_color] {
            ensure!(
                color
                    .iter()
                    .all(|x| x.is_finite() && (0.0..=1.0).contains(x)),
                "light colors must be linear RGB in 0..1"
            );
        }
        for intensity in [self.sun_intensity, self.ambient_intensity] {
            ensure!(
                intensity.is_finite() && (0.0..=100_000.0).contains(&intensity),
                "light intensity must be finite and in 0..100000"
            );
        }
        Ok(())
    }
}
