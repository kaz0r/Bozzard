use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Single-scattering height fog lit by the scene's sun, point and spot lights.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VolumetricFog {
    pub enabled: bool,
    /// Extinction per world unit at/below the base height.
    pub density: f32,
    /// Scattering albedo; zero absorbs light, one scatters it.
    pub albedo: [f32; 3],
    /// Henyey-Greenstein phase asymmetry. Positive values favor forward scattering.
    pub anisotropy: f32,
    pub base_height: f32,
    pub height_falloff: f32,
    pub start_distance: f32,
    pub max_distance: f32,
    pub noise_amount: f32,
    pub noise_scale: f32,
    /// World units per second, evaluated on the simulation clock.
    pub wind: [f32; 3],
    pub light_intensity: f32,
    /// Fraction of ambient/environment illumination scattered into the view.
    pub ambient: f32,
    /// Bounded half-resolution ray-march work; no temporal history required.
    pub steps: u32,
}
impl Default for VolumetricFog {
    fn default() -> Self {
        Self {
            enabled: false,
            density: 0.035,
            albedo: [0.9, 0.94, 1.],
            anisotropy: 0.3,
            base_height: 0.,
            height_falloff: 0.25,
            start_distance: 0.25,
            max_distance: 40.,
            noise_amount: 0.65,
            noise_scale: 0.35,
            wind: [0.15, 0.025, 0.07],
            light_intensity: 1.,
            ambient: 0.25,
            steps: 48,
        }
    }
}
impl VolumetricFog {
    pub fn validate(&self) -> Result<()> {
        let range = |value: f32, min: f32, max: f32, name: &str| -> Result<()> {
            ensure!(
                value.is_finite() && (min..=max).contains(&value),
                "volumetric {name} must be finite and within {min}..{max}"
            );
            Ok(())
        };
        range(self.density, 0., 2., "density")?;
        for value in self.albedo {
            range(value, 0., 1., "albedo")?;
        }
        range(self.anisotropy, -0.8, 0.8, "anisotropy")?;
        range(self.base_height, -100_000., 100_000., "base height")?;
        range(self.height_falloff, 0., 10., "height falloff")?;
        range(self.start_distance, 0., 1000., "start distance")?;
        range(self.max_distance, 1., 1000., "max distance")?;
        ensure!(
            self.start_distance <= self.max_distance,
            "volumetric start distance must not exceed max distance"
        );
        range(self.noise_amount, 0., 1., "noise amount")?;
        range(self.noise_scale, 0.01, 4., "noise scale")?;
        for value in self.wind {
            range(value, -100., 100., "wind")?;
        }
        range(self.light_intensity, 0., 4., "light intensity")?;
        range(self.ambient, 0., 1., "ambient")?;
        ensure!(
            (16..=96).contains(&self.steps),
            "volumetric steps must be within 16..96"
        );
        Ok(())
    }
}

impl VolumetricFog {
    pub(crate) fn blend(self, other: Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        let rgb = |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|i| mix(a[i], b[i]));
        Self {
            enabled: self.enabled || other.enabled,
            density: mix(
                if self.enabled { self.density } else { 0. },
                if other.enabled { other.density } else { 0. },
            ),
            albedo: rgb(self.albedo, other.albedo),
            anisotropy: mix(self.anisotropy, other.anisotropy),
            base_height: mix(self.base_height, other.base_height),
            height_falloff: mix(self.height_falloff, other.height_falloff),
            start_distance: mix(self.start_distance, other.start_distance),
            max_distance: mix(self.max_distance, other.max_distance),
            noise_amount: mix(self.noise_amount, other.noise_amount),
            noise_scale: mix(self.noise_scale, other.noise_scale),
            wind: rgb(self.wind, other.wind),
            light_intensity: mix(self.light_intensity, other.light_intensity),
            ambient: mix(self.ambient, other.ambient),
            steps: if t < 0.5 { self.steps } else { other.steps },
        }
    }
}
