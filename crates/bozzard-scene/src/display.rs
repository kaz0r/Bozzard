use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisplaySettings {
    pub bloom: BloomSettings,
    pub tone_mapper: ToneMapper,
    pub color_grading: ColorGrading,
    pub ambient_occlusion: AmbientOcclusion,
    pub heat_distortion: HeatDistortion,
    pub grain: FilmGrain,
    pub vignette: Vignette,
    /// Stops applied to HDR radiance before display mapping. +1 doubles exposure.
    pub exposure_ev: f32,
    /// Enable the selected tone mapper. Legacy scenes default to Reinhard.
    pub tone_mapping: bool,
}
impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            bloom: BloomSettings::default(),
            tone_mapper: ToneMapper::default(),
            color_grading: ColorGrading::default(),
            ambient_occlusion: AmbientOcclusion::default(),
            heat_distortion: HeatDistortion::default(),
            grain: FilmGrain::default(),
            vignette: Vignette::default(),
            exposure_ev: 0.,
            tone_mapping: true,
        }
    }
}
impl DisplaySettings {
    pub fn validate(&self) -> Result<()> {
        self.bloom.validate()?;
        self.color_grading.validate()?;
        self.ambient_occlusion.validate()?;
        self.heat_distortion.validate()?;
        self.grain.validate()?;
        self.vignette.validate()?;
        ensure!(
            self.exposure_ev.is_finite() && (-16.0..=16.0).contains(&self.exposure_ev),
            "exposure must be finite and within -16..16 stops"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BloomSettings {
    pub enabled: bool,
    pub intensity: f32,
    /// Scene-linear threshold, before exposure.
    pub threshold: f32,
    pub scatter: f32,
    /// Horizontal stretch of the bloom reconstruction kernel; zero is isotropic.
    pub anamorphic: f32,
}
impl Default for BloomSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: 0.15,
            threshold: 1.,
            scatter: 0.7,
            anamorphic: 0.,
        }
    }
}
impl BloomSettings {
    pub fn validate(&self) -> Result<()> {
        range(self.anamorphic, 0., 1., "anamorphic bloom")?;
        ensure!(
            self.intensity.is_finite() && (0.0..=10.).contains(&self.intensity),
            "bloom intensity must be in 0..10"
        );
        ensure!(
            self.threshold.is_finite() && (0.0..=60_000.).contains(&self.threshold),
            "bloom threshold must be in 0..60000"
        );
        ensure!(
            self.scatter.is_finite() && (0.0..=1.).contains(&self.scatter),
            "bloom scatter must be in 0..1"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToneMapper {
    #[default]
    Reinhard,
    /// A luminance-preserving filmic curve with a soft toe and shoulder.
    Filmic,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayPreset {
    Neutral,
    Cinematic,
    Bonfire,
    Neon,
    Noir,
}
impl DisplayPreset {
    pub const ALL: [Self; 5] = [
        Self::Neutral,
        Self::Cinematic,
        Self::Bonfire,
        Self::Neon,
        Self::Noir,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Neutral => "Neutral",
            Self::Cinematic => "Cinematic",
            Self::Bonfire => "Bonfire",
            Self::Neon => "Neon",
            Self::Noir => "Noir",
        }
    }
}
impl DisplaySettings {
    pub fn preset(preset: DisplayPreset) -> Self {
        let mut value = Self::default();
        if preset == DisplayPreset::Neutral {
            return value;
        }
        value.tone_mapper = ToneMapper::Filmic;
        value.bloom.enabled = true;
        value.bloom.intensity = 0.2;
        value.bloom.anamorphic = 0.35;
        value.ambient_occlusion.enabled = true;
        value.ambient_occlusion.intensity = 1.2;
        value.vignette.intensity = 0.28;
        value.grain.intensity = 0.025;
        value.color_grading.contrast = 1.05;
        match preset {
            DisplayPreset::Bonfire => {
                value.exposure_ev = 0.35;
                value.bloom.intensity = 0.3;
                value.bloom.anamorphic = 0.65;
                value.color_grading.temperature = 0.12;
                value.color_grading.lift = [0., 0., 0.001];
                value.ambient_occlusion.intensity = 0.7;
                value.heat_distortion.threshold = 0.6;
                value.heat_distortion.enabled = true;
                value.heat_distortion.strength = 4.;
                value.vignette.intensity = 0.4;
            }
            DisplayPreset::Neon => {
                value.bloom.intensity = 0.4;
                value.bloom.anamorphic = 0.85;
                value.color_grading.saturation = 1.2;
                value.color_grading.temperature = -0.15;
                value.color_grading.tint = 0.1;
            }
            DisplayPreset::Noir => {
                value.color_grading.saturation = 0.;
                value.color_grading.contrast = 1.25;
                value.grain.intensity = 0.07;
                value.vignette.intensity = 0.55;
                value.bloom.intensity = 0.1;
            }
            _ => {}
        }
        value
    }

    /// Blend scalar controls continuously, including disabled effects from zero strength.
    /// Discrete tone-mapper selection changes at the midpoint.
    pub fn blend(self, other: Self, weight: f32) -> Self {
        let t = weight.clamp(0., 1.);
        if t == 0. {
            return self;
        }
        if t == 1. {
            return other;
        }
        let mix = |a: f32, b: f32| a + (b - a) * t;
        let rgb = |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|i| mix(a[i], b[i]));
        let strength = |enabled, value| if enabled { value } else { 0. };
        Self {
            exposure_ev: mix(self.exposure_ev, other.exposure_ev),
            tone_mapping: if t < 0.5 {
                self.tone_mapping
            } else {
                other.tone_mapping
            },
            tone_mapper: if t < 0.5 {
                self.tone_mapper
            } else {
                other.tone_mapper
            },
            bloom: BloomSettings {
                enabled: self.bloom.enabled || other.bloom.enabled,
                intensity: mix(
                    strength(self.bloom.enabled, self.bloom.intensity),
                    strength(other.bloom.enabled, other.bloom.intensity),
                ),
                threshold: mix(self.bloom.threshold, other.bloom.threshold),
                scatter: mix(self.bloom.scatter, other.bloom.scatter),
                anamorphic: mix(self.bloom.anamorphic, other.bloom.anamorphic),
            },
            color_grading: ColorGrading {
                temperature: mix(
                    self.color_grading.temperature,
                    other.color_grading.temperature,
                ),
                tint: mix(self.color_grading.tint, other.color_grading.tint),
                saturation: mix(
                    self.color_grading.saturation,
                    other.color_grading.saturation,
                ),
                contrast: mix(self.color_grading.contrast, other.color_grading.contrast),
                lift: rgb(self.color_grading.lift, other.color_grading.lift),
                gamma: rgb(self.color_grading.gamma, other.color_grading.gamma),
                gain: rgb(self.color_grading.gain, other.color_grading.gain),
            },
            ambient_occlusion: AmbientOcclusion {
                enabled: self.ambient_occlusion.enabled || other.ambient_occlusion.enabled,
                intensity: mix(
                    strength(
                        self.ambient_occlusion.enabled,
                        self.ambient_occlusion.intensity,
                    ),
                    strength(
                        other.ambient_occlusion.enabled,
                        other.ambient_occlusion.intensity,
                    ),
                ),
                radius: mix(
                    self.ambient_occlusion.radius,
                    other.ambient_occlusion.radius,
                ),
                bias: mix(self.ambient_occlusion.bias, other.ambient_occlusion.bias),
            },
            heat_distortion: HeatDistortion {
                enabled: self.heat_distortion.enabled || other.heat_distortion.enabled,
                strength: mix(
                    strength(self.heat_distortion.enabled, self.heat_distortion.strength),
                    strength(
                        other.heat_distortion.enabled,
                        other.heat_distortion.strength,
                    ),
                ),
                threshold: mix(
                    self.heat_distortion.threshold,
                    other.heat_distortion.threshold,
                ),
                speed: mix(self.heat_distortion.speed, other.heat_distortion.speed),
                rise: mix(self.heat_distortion.rise, other.heat_distortion.rise),
            },
            grain: FilmGrain {
                intensity: mix(self.grain.intensity, other.grain.intensity),
                size: mix(self.grain.size, other.grain.size),
            },
            vignette: Vignette {
                intensity: mix(self.vignette.intensity, other.vignette.intensity),
                roundness: mix(self.vignette.roundness, other.vignette.roundness),
                feather: mix(self.vignette.feather, other.vignette.feather),
            },
        }
    }
}

/// Axis-aligned world-space effect region. Higher priorities apply last; ties use document order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PostProcessVolume {
    pub name: String,
    pub enabled: bool,
    pub center: [f32; 3],
    pub half_size: [f32; 3],
    /// Fade outside the box, with full strength inside.
    pub blend_distance: f32,
    pub weight: f32,
    pub priority: i32,
    pub display: DisplaySettings,
}
impl Default for PostProcessVolume {
    fn default() -> Self {
        Self {
            name: "Post-process volume".into(),
            enabled: true,
            center: [0.; 3],
            half_size: [5.; 3],
            blend_distance: 3.,
            weight: 1.,
            priority: 0,
            display: DisplaySettings::preset(DisplayPreset::Cinematic),
        }
    }
}
impl PostProcessVolume {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 128,
            "volume name must have 1..128 bytes"
        );
        for value in self.center {
            range(value, -1_000_000., 1_000_000., "volume center")?;
        }
        for value in self.half_size {
            range(value, 0.01, 100_000., "volume half size")?;
        }
        range(
            self.blend_distance,
            0.001,
            100_000.,
            "volume blend distance",
        )?;
        range(self.weight, 0., 1., "volume weight")?;
        self.display.validate()
    }
    pub fn influence(&self, position: glam::Vec3) -> f32 {
        if !self.enabled {
            return 0.;
        }
        let outside = (position - glam::Vec3::from_array(self.center)).abs()
            - glam::Vec3::from_array(self.half_size);
        let t = (1. - outside.max(glam::Vec3::ZERO).length() / self.blend_distance).clamp(0., 1.);
        self.weight * t * t * (3. - 2. * t)
    }
}

/// Transient graph overrides; capture/save never writes them to the scene document.
#[derive(Clone, Default)]
pub(crate) struct DisplayOverrides {
    pub exposure: Option<f32>,
    pub bloom: Option<f32>,
    pub saturation: Option<f32>,
    pub heat: Option<f32>,
    pub grain: Option<f32>,
    pub vignette: Option<f32>,
}
impl DisplayOverrides {
    pub fn apply(&self, mut display: DisplaySettings) -> DisplaySettings {
        if let Some(value) = self.exposure {
            display.exposure_ev = value;
        }
        if let Some(value) = self.bloom {
            display.bloom.enabled = true;
            display.bloom.intensity = value;
        }
        if let Some(value) = self.saturation {
            display.color_grading.saturation = value;
        }
        if let Some(value) = self.heat {
            display.heat_distortion.enabled = true;
            display.heat_distortion.strength = value;
        }
        if let Some(value) = self.grain {
            display.grain.intensity = value;
        }
        if let Some(value) = self.vignette {
            display.vignette.intensity = value;
        }
        display
    }
}

impl crate::Scene {
    pub fn display_at(&self, position: glam::Vec3) -> DisplaySettings {
        let mut volumes: Vec<_> = self.post_process_volumes.iter().collect();
        volumes.sort_by_key(|volume| volume.priority);
        volumes.into_iter().fold(self.display, |value, volume| {
            value.blend(volume.display, volume.influence(position))
        })
    }
}
impl crate::SceneInstance {
    pub fn advance_display(&mut self, dt: f32) -> Result<()> {
        range(dt, 0., 1., "display timestep")?;
        // A bounded phase avoids float precision loss during long sessions.
        self.display_time = (self.display_time + dt) % 4096.;
        Ok(())
    }
    pub fn display_at(&self, position: glam::Vec3, layer: crate::Layer) -> DisplaySettings {
        if layer == crate::Layer::TwoD {
            return DisplaySettings {
                tone_mapping: false,
                ..Default::default()
            };
        }
        self.display_overrides
            .apply(self.document.display_at(position))
    }
    pub(crate) fn set_display_parameter(
        &mut self,
        kind: crate::blueprint::NodeKind,
        value: f32,
    ) -> Result<()> {
        use crate::blueprint::NodeKind as K;
        let mut next = self.display_overrides.clone();
        match kind {
            K::SetExposure => next.exposure = Some(value),
            K::SetBloomIntensity => next.bloom = Some(value),
            K::SetSaturation => next.saturation = Some(value),
            K::SetHeatStrength => next.heat = Some(value),
            K::SetGrainIntensity => next.grain = Some(value),
            K::SetVignetteIntensity => next.vignette = Some(value),
            _ => anyhow::bail!("not a display action"),
        }
        next.apply(self.document.display).validate()?;
        self.display_overrides = next;
        Ok(())
    }
}
