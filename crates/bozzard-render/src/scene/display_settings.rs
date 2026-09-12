use super::BloomSettings;
use super::VolumetricFog;
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplaySettings {
    /// Simulation time; deterministic captures set this explicitly.
    pub time_seconds: f32,
    pub bloom: BloomSettings,
    pub tone_mapper: ToneMapper,
    pub color_grading: ColorGrading,
    pub ambient_occlusion: AmbientOcclusion,
    pub heat_distortion: HeatDistortion,
    pub grain: FilmGrain,
    pub vignette: Vignette,
    pub volumetric_fog: VolumetricFog,
    /// Stops applied to HDR radiance before display mapping. +1 doubles exposure.
    pub exposure_ev: f32,
    /// Enable the selected tone mapper. Legacy scenes default to Reinhard.
    pub tone_mapping: bool,
}
impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            time_seconds: 0.,
            bloom: BloomSettings::default(),
            tone_mapper: ToneMapper::default(),
            color_grading: ColorGrading::default(),
            ambient_occlusion: AmbientOcclusion::default(),
            heat_distortion: HeatDistortion::default(),
            grain: FilmGrain::default(),
            vignette: Vignette::default(),
            volumetric_fog: VolumetricFog::default(),
            exposure_ev: 0.,
            tone_mapping: true,
        }
    }
}
impl DisplaySettings {
    pub fn validate(&self) -> Result<()> {
        range(self.time_seconds, 0., f32::MAX, "display time")?;
        self.bloom.validate()?;
        self.color_grading.validate()?;
        self.ambient_occlusion.validate()?;
        self.heat_distortion.validate()?;
        self.grain.validate()?;
        self.vignette.validate()?;
        self.volumetric_fog.validate()?;
        ensure!(
            self.exposure_ev.is_finite() && (-16.0..=16.0).contains(&self.exposure_ev),
            "exposure must be finite and within -16..16 stops"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToneMapper {
    #[default]
    Reinhard,
    /// A luminance-preserving filmic curve with a soft toe and shoulder.
    Filmic,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorGrading {
    pub temperature: f32,
    pub tint: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub lift: [f32; 3],
    pub gamma: [f32; 3],
    pub gain: [f32; 3],
}
impl Default for ColorGrading {
    fn default() -> Self {
        Self {
            temperature: 0.,
            tint: 0.,
            saturation: 1.,
            contrast: 1.,
            lift: [0.; 3],
            gamma: [1.; 3],
            gain: [1.; 3],
        }
    }
}
impl ColorGrading {
    pub fn validate(&self) -> Result<()> {
        range(self.temperature, -1., 1., "temperature")?;
        range(self.tint, -1., 1., "tint")?;
        range(self.saturation, 0., 2., "saturation")?;
        range(self.contrast, 0., 2., "contrast")?;
        for value in self.lift {
            range(value, -0.25, 0.25, "color lift")?;
        }
        for value in self.gamma {
            range(value, 0.25, 4., "color gamma")?;
        }
        for value in self.gain {
            range(value, 0., 4., "color gain")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmbientOcclusion {
    pub enabled: bool,
    pub intensity: f32,
    /// World-space sampling radius.
    pub radius: f32,
    /// World-space bias to avoid self occlusion on flat surfaces.
    pub bias: f32,
}
impl Default for AmbientOcclusion {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: 1.,
            radius: 0.7,
            bias: 0.025,
        }
    }
}
impl AmbientOcclusion {
    pub fn validate(&self) -> Result<()> {
        range(self.intensity, 0., 3., "AO intensity")?;
        range(self.radius, 0.01, 10., "AO radius")?;
        range(self.bias, 0., 0.5, "AO bias")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatDistortion {
    pub enabled: bool,
    /// Maximum displacement in pixels at a 1080-pixel viewport height.
    pub strength: f32,
    /// Scene-linear brightness identifying heat sources.
    pub threshold: f32,
    pub speed: f32,
    /// Vertical reach as a fraction of the viewport height.
    pub rise: f32,
}
impl Default for HeatDistortion {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 3.,
            threshold: 2.,
            speed: 1.,
            rise: 0.08,
        }
    }
}
impl HeatDistortion {
    pub fn validate(&self) -> Result<()> {
        range(self.strength, 0., 30., "heat strength")?;
        range(self.threshold, 0., 60_000., "heat threshold")?;
        range(self.speed, 0., 5., "heat speed")?;
        range(self.rise, 0.001, 0.3, "heat rise")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilmGrain {
    pub intensity: f32,
    pub size: f32,
}
impl Default for FilmGrain {
    fn default() -> Self {
        Self {
            intensity: 0.,
            size: 1.,
        }
    }
}
impl FilmGrain {
    pub fn validate(&self) -> Result<()> {
        range(self.intensity, 0., 0.25, "grain intensity")?;
        range(self.size, 1., 4., "grain size")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vignette {
    pub intensity: f32,
    pub roundness: f32,
    pub feather: f32,
}
impl Default for Vignette {
    fn default() -> Self {
        Self {
            intensity: 0.,
            roundness: 0.7,
            feather: 0.6,
        }
    }
}
impl Vignette {
    pub fn validate(&self) -> Result<()> {
        range(self.intensity, 0., 1., "vignette intensity")?;
        range(self.roundness, 0., 1., "vignette roundness")?;
        range(self.feather, 0.05, 1., "vignette feather")
    }
}
fn range(value: f32, min: f32, max: f32, name: &str) -> Result<()> {
    ensure!(
        value.is_finite() && (min..=max).contains(&value),
        "{name} must be finite and within {min}..{max}"
    );
    Ok(())
}
