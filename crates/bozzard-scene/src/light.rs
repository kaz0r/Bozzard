use anyhow::{Result, ensure};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

pub const MAX_LOCAL_LIGHTS: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    #[default]
    Point,
    Spot,
}

/// Punctual light attached to an object's transform. Spotlights face local -Z.
/// Range is in world units, independent of object scale; angles are half angles.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Light {
    pub enabled: bool,
    pub kind: LightKind,
    pub color: [f32; 3],
    /// Luminous intensity in candela, with inverse-square distance falloff.
    pub intensity: f32,
    pub range: f32,
    pub inner_angle_degrees: f32,
    pub outer_angle_degrees: f32,
}
impl Default for Light {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: LightKind::Point,
            color: [1.; 3],
            intensity: 100.,
            range: 10.,
            inner_angle_degrees: 20.,
            outer_angle_degrees: 30.,
        }
    }
}
impl Light {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
            "light color must be linear RGB in 0..1"
        );
        ensure!(
            self.intensity.is_finite() && (0.0..=100_000.).contains(&self.intensity),
            "light intensity must be in 0..100000 cd"
        );
        ensure!(
            self.range.is_finite() && (0.001..=100_000.).contains(&self.range),
            "light range must be in 0.001..100000 world units"
        );
        ensure!(
            self.inner_angle_degrees.is_finite()
                && self.outer_angle_degrees.is_finite()
                && self.inner_angle_degrees >= 0.
                && self.inner_angle_degrees <= self.outer_angle_degrees
                && (0.1..=89.9).contains(&self.outer_angle_degrees),
            "spot half angles need 0 <= inner <= outer, with outer in 0.1..89.9 degrees"
        );
        Ok(())
    }
    pub fn at(&self, transform: Mat4) -> Result<WorldLight> {
        self.validate()?;
        let position = transform.transform_point3(Vec3::ZERO);
        let direction = transform.transform_vector3(Vec3::NEG_Z).try_normalize();
        ensure!(
            position.is_finite() && direction.is_some(),
            "invalid light world transform"
        );
        Ok(WorldLight {
            light: *self,
            position: position.to_array(),
            direction: direction.unwrap().to_array(),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WorldLight {
    pub light: Light,
    pub position: [f32; 3],
    pub direction: [f32; 3],
}
