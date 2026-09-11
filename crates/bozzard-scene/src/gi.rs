use anyhow::{Result, ensure};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Nine diffuse SH coefficients followed by an 8x8 map of distance moments
/// (two mean/mean-square pairs per vec4). Coefficient zero's W stores validity.
pub const GI_PROBE_STRIDE: usize = 41;
pub const GI_VISIBILITY_SIZE: usize = 8;
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GiVolumeSettings {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub resolution: [u32; 3],
    pub samples: u32,
    pub bounces: u32,
}
impl Default for GiVolumeSettings {
    fn default() -> Self {
        Self {
            min: [-5., -1., -5.],
            max: [5., 5., 5.],
            resolution: [8, 4, 8],
            samples: 128,
            bounces: 2,
        }
    }
}
impl GiVolumeSettings {
    pub fn probe_count(&self) -> usize {
        self.resolution.iter().map(|x| *x as usize).product()
    }
    pub fn validate(&self) -> Result<()> {
        let min = Vec3::from(self.min);
        let max = Vec3::from(self.max);
        let extent = max - min;
        ensure!(
            min.is_finite()
                && max.is_finite()
                && extent.is_finite()
                && extent.cmpge(Vec3::splat(0.01)).all()
                && extent.cmple(Vec3::splat(100_000.)).all(),
            "GI volume needs finite positive dimensions in 0.01..100000"
        );
        ensure!(
            self.resolution.iter().all(|n| (2..=16).contains(n)),
            "GI grid resolution must be in 2..16 per axis"
        );
        ensure!(
            self.samples.is_power_of_two() && (64..=1024).contains(&self.samples),
            "GI rays per probe must be a power of two in 64..1024"
        );
        ensure!(
            (1..=4).contains(&self.bounces),
            "GI diffuse bounces must be in 1..4"
        );
        Ok(())
    }
    pub fn position(&self, index: usize) -> Vec3 {
        let [x, y, _] = self.resolution.map(|n| n as usize);
        let cell = Vec3::new(
            (index % x) as f32,
            ((index / x) % y) as f32,
            (index / (x * y)) as f32,
        );
        Vec3::from(self.min)
            + (Vec3::from(self.max) - Vec3::from(self.min)) * cell
                / Vec3::from(self.resolution.map(|n| (n - 1) as f32))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BakedGi {
    pub source: String,
    pub volume: GiVolumeSettings,
    /// Shared across scene snapshots/Undo/Play, serialized with the scene.
    pub probes: Arc<Vec<[f32; 4]>>,
    // Retaining the validated allocation prevents in-place edits through Arc::get_mut.
    // Arc::make_mut produces a new identity, which must be validated again.
    #[serde(skip)]
    validated: Arc<Mutex<Option<ProbeData>>>,
}
type ProbeData = Arc<Vec<[f32; 4]>>;
impl PartialEq for BakedGi {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.volume == other.volume
            && (Arc::ptr_eq(&self.probes, &other.probes) || self.probes == other.probes)
    }
}
impl BakedGi {
    pub fn new(source: String, volume: GiVolumeSettings, probes: ProbeData) -> Result<Self> {
        let result = Self {
            source,
            volume,
            probes,
            validated: Default::default(),
        };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<()> {
        self.volume.validate()?;
        ensure!(
            self.source.len() == 16
                && self
                    .source
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid GI source fingerprint"
        );
        ensure!(
            self.probes.len() == self.volume.probe_count() * GI_PROBE_STRIDE,
            "GI probe data length does not match grid"
        );
        let mut validated = self.validated.lock().unwrap_or_else(|e| e.into_inner());
        if validated
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(p, &self.probes))
        {
            return Ok(());
        }
        for probe in self.probes.chunks_exact(GI_PROBE_STRIDE) {
            ensure!(
                (0.0..=1.).contains(&probe[0][3]),
                "invalid GI probe validity"
            );
            ensure!(
                probe[..9]
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && v.abs() <= 1_000_000.),
                "invalid GI coefficients"
            );
            ensure!(
                probe[9..]
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && (0.0..=1e12).contains(v)),
                "invalid GI visibility moments"
            );
        }
        *validated = Some(self.probes.clone());
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GiSettings {
    pub enabled: bool,
    pub intensity: f32,
    pub normal_bias: f32,
    pub volume: GiVolumeSettings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baked: Option<Arc<BakedGi>>,
}
impl Default for GiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: 1.,
            normal_bias: 0.05,
            volume: Default::default(),
            baked: None,
        }
    }
}
impl GiSettings {
    pub fn validate(&self) -> Result<()> {
        self.volume.validate()?;
        ensure!(
            self.intensity.is_finite() && (0.0..=10.).contains(&self.intensity),
            "GI intensity must be in 0..10"
        );
        ensure!(
            self.normal_bias.is_finite() && (0.0..=1.).contains(&self.normal_bias),
            "GI normal bias must be in 0..1"
        );
        if let Some(baked) = &self.baked {
            baked.validate()?;
        }
        Ok(())
    }
}
