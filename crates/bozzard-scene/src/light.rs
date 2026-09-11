use anyhow::{Result, ensure};
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

pub const MAX_LOCAL_LIGHTS: usize = 32;
pub const MAX_SHADOWED_SPOT_LIGHTS: usize = 8;

fn no_shadows(value: &bool) -> bool {
    !value
}
fn default_bias(value: &f32) -> bool {
    *value == 0.005
}
fn default_normal_bias(value: &f32) -> bool {
    *value == 0.01
}

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
    /// Only spotlights cast local shadows. Disabled spots still reserve the authored budget.
    #[serde(skip_serializing_if = "no_shadows")]
    pub shadows: bool,
    /// World-space receiver offsets, independent of the object's scale.
    #[serde(skip_serializing_if = "default_bias")]
    pub shadow_bias: f32,
    #[serde(skip_serializing_if = "default_normal_bias")]
    pub shadow_normal_bias: f32,
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
            shadows: false,
            shadow_bias: 0.005,
            shadow_normal_bias: 0.01,
        }
    }
}
impl Light {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [self.shadow_bias, self.shadow_normal_bias]
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
            "spotlight shadow bias must be finite and in 0..1 world units"
        );
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
    pub fn requests_shadow_map(&self) -> bool {
        self.kind == LightKind::Spot && self.shadows
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
