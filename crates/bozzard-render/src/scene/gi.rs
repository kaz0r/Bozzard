use super::*;
use std::sync::Arc;
const STRIDE: usize = 41;
#[derive(Clone, Debug)]
pub struct IrradianceVolume {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub resolution: [u32; 3],
    pub intensity: f32,
    pub normal_bias: f32,
    /// 9 SH vec4s and32 packed pairs of visibility moments per probe.
    pub probes: Arc<Vec<[f32; 4]>>,
}
impl IrradianceVolume {
    fn validate(&self) -> Result<()> {
        let extent = Vec3::from(self.max) - Vec3::from(self.min);
        ensure!(
            Vec3::from(self.min).is_finite()
                && Vec3::from(self.max).is_finite()
                && extent.is_finite()
                && extent.cmpge(Vec3::splat(0.01)).all()
                && extent.cmple(Vec3::splat(100_000.)).all(),
            "invalid GI bounds"
        );
        ensure!(
            self.resolution.iter().all(|n| (2..=16).contains(n)),
            "invalid GI resolution"
        );
        ensure!(
            self.probes.len()
                == self
                    .resolution
                    .iter()
                    .map(|n| *n as usize)
                    .product::<usize>()
                    * STRIDE,
            "invalid GI data length"
        );
        ensure!(
            self.intensity.is_finite()
                && (0.0..=10.).contains(&self.intensity)
                && self.normal_bias.is_finite()
                && (0.0..=1.).contains(&self.normal_bias),
            "invalid GI display settings"
        );
        Ok(())
    }
}
impl SceneRenderer {
    pub(super) fn prepare_gi(
        &mut self,
        gpu: &Gpu,
        volume: Option<&IrradianceVolume>,
    ) -> Result<()> {
        let Some(volume) = volume else {
            gpu.queue
                .write_buffer(&self.shadows.gi_uniform, 0, &[0; 64]);
            return Ok(());
        };
        volume.validate()?;
        if self
            .shadows
            .gi_snapshot
            .as_ref()
            .is_none_or(|data| !Arc::ptr_eq(data, &volume.probes))
        {
            for probe in volume.probes.chunks_exact(STRIDE) {
                ensure!(
                    (0.0..=1.).contains(&probe[0][3])
                        && probe[..9]
                            .iter()
                            .flatten()
                            .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
                        && probe[9..]
                            .iter()
                            .flatten()
                            .all(|v| v.is_finite() && (0.0..=1e12).contains(v)),
                    "invalid GI probe contents"
                );
            }
            self.shadows.gi_data =
                gpu.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("baked irradiance and visibility"),
                        contents: &float_bytes(volume.probes.iter().flatten().copied()),
                        usage: wgpu::BufferUsages::STORAGE,
                    });
            self.shadows.gi_snapshot = Some(volume.probes.clone());
            self.shadows.rebind(gpu);
        }
        let [x, y, z] = volume.resolution.map(|n| n as f32);
        gpu.queue.write_buffer(
            &self.shadows.gi_uniform,
            0,
            &float_bytes([
                volume.min[0],
                volume.min[1],
                volume.min[2],
                0.,
                volume.max[0],
                volume.max[1],
                volume.max[2],
                0.,
                x,
                y,
                z,
                1.,
                volume.intensity,
                volume.normal_bias,
                0.,
                0.,
            ]),
        );
        Ok(())
    }
}
