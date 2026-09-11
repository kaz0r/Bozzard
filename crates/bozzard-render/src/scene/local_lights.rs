use super::*;

pub const MAX_LOCAL_LIGHTS: usize = 32;
pub(super) const UNIFORM_SIZE: u64 = 16 + MAX_LOCAL_LIGHTS as u64 * 64;

/// World-space punctual light; a missing cone denotes an omnidirectional point.
#[derive(Clone, Copy, Debug)]
pub struct LocalLight {
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    /// Inner and outer half angles, in degrees.
    pub spot_angles: Option<[f32; 2]>,
}
impl LocalLight {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.position.iter().all(|v| v.is_finite()),
            "invalid local light position"
        );
        let direction = Vec3::from(self.direction);
        ensure!(
            direction.is_finite() && direction.try_normalize().is_some(),
            "invalid local light direction"
        );
        ensure!(
            self.color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
            "invalid local light color"
        );
        ensure!(
            self.intensity.is_finite() && (0.0..=100_000.).contains(&self.intensity),
            "invalid local light intensity"
        );
        ensure!(
            self.range.is_finite() && (0.001..=100_000.).contains(&self.range),
            "invalid local light range"
        );
        if let Some([inner, outer]) = self.spot_angles {
            ensure!(
                inner.is_finite()
                    && outer.is_finite()
                    && inner >= 0.
                    && inner <= outer
                    && (0.1..=89.9).contains(&outer),
                "invalid spotlight half angles"
            );
        }
        Ok(())
    }
}

pub(super) fn uniform(lights: &[LocalLight]) -> Result<Vec<u8>> {
    ensure!(
        lights.len() <= MAX_LOCAL_LIGHTS,
        "renderer supports at most {MAX_LOCAL_LIGHTS} local lights"
    );
    let mut values = vec![0.; UNIFORM_SIZE as usize / 4];
    values[0] = lights.len() as f32;
    for (light, row) in lights.iter().zip(values[4..].chunks_exact_mut(16)) {
        light.validate()?;
        let direction = Vec3::from(light.direction).normalize();
        let [inner, outer] = light
            .spot_angles
            .unwrap_or([0., 0.])
            .map(|a| a.to_radians().cos());
        row.copy_from_slice(&[
            light.position[0],
            light.position[1],
            light.position[2],
            light.range,
            light.color[0],
            light.color[1],
            light.color[2],
            light.intensity,
            direction.x,
            direction.y,
            direction.z,
            outer,
            inner,
            if light.spot_angles.is_some() { 1. } else { 0. },
            0.,
            0.,
        ]);
    }
    Ok(float_bytes(values))
}
