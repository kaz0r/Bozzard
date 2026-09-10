use anyhow::{Result, ensure};
use glam::Vec3;

/// Renderer-only world-space illumination. Colors are linear RGB.
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    pub shadows: bool,
    pub shadow_resolution: u32,
    /// World-space depth and normal offsets for shadow acne control.
    pub shadow_bias: f32,
    pub shadow_normal_bias: f32,
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
    pub(super) fn validate(&self) -> Result<()> {
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
        let d = Vec3::from(self.sun_direction);
        ensure!(
            d.is_finite() && d.length_squared().is_finite() && d.length_squared() > 1e-12,
            "invalid sun direction"
        );
        ensure!(
            [self.sun_color, self.ambient_color]
                .iter()
                .flatten()
                .all(|x| x.is_finite() && (0.0..=1.0).contains(x)),
            "invalid light color"
        );
        ensure!(
            [self.sun_intensity, self.ambient_intensity]
                .iter()
                .all(|x| x.is_finite() && (0.0..=100_000.0).contains(x)),
            "invalid light intensity"
        );
        Ok(())
    }
    pub(super) fn uniform(&self) -> [f32; 12] {
        let d = Vec3::from(self.sun_direction).normalize();
        [
            d.x,
            d.y,
            d.z,
            self.sun_intensity,
            self.sun_color[0],
            self.sun_color[1],
            self.sun_color[2],
            self.ambient_intensity,
            self.ambient_color[0],
            self.ambient_color[1],
            self.ambient_color[2],
            0.,
        ]
    }
}
