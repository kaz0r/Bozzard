use super::*;

pub const MAX_LOCAL_LIGHTS: usize = 32;
pub const MAX_SHADOWED_SPOT_LIGHTS: usize = 8;
pub(super) const UNIFORM_SIZE: u64 = 16 + MAX_LOCAL_LIGHTS as u64 * 64;

/// Receiver offsets in world units. Shadow map resolution is currently 1024px.
#[derive(Clone, Copy, Debug)]
pub struct SpotShadowSettings {
    pub bias: f32,
    pub normal_bias: f32,
}
impl Default for SpotShadowSettings {
    fn default() -> Self {
        Self {
            bias: 0.005,
            normal_bias: 0.01,
        }
    }
}

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
    pub shadows: Option<SpotShadowSettings>,
}
impl LocalLight {
    pub fn validate(&self) -> Result<()> {
        if let Some(shadow) = self.shadows {
            ensure!(
                self.spot_angles.is_some(),
                "only spotlights support local shadows"
            );
            ensure!(
                [shadow.bias, shadow.normal_bias]
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.).contains(v)),
                "invalid spotlight shadow bias"
            );
        }
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
    pub(super) fn casts_shadow(&self) -> bool {
        self.spot_angles.is_some()
            && self.shadows.is_some()
            && self.intensity > 0.
            && self.color.iter().any(|v| *v > 0.)
    }
}

pub(super) fn uniform(lights: &[LocalLight]) -> Result<Vec<u8>> {
    ensure!(
        lights.len() <= MAX_LOCAL_LIGHTS,
        "renderer supports at most {MAX_LOCAL_LIGHTS} local lights"
    );
    let mut values = vec![0.; UNIFORM_SIZE as usize / 4];
    ensure!(
        lights.iter().filter(|l| l.shadows.is_some()).count() <= MAX_SHADOWED_SPOT_LIGHTS,
        "renderer supports at most {MAX_SHADOWED_SPOT_LIGHTS} shadowed spotlights"
    );
    values[0] = lights.len() as f32;
    let mut shadow_slot = 0;
    for (light, row) in lights.iter().zip(values[4..].chunks_exact_mut(16)) {
        light.validate()?;
        let direction = Vec3::from(light.direction).normalize();
        let slot = if light.casts_shadow() {
            shadow_slot += 1;
            shadow_slot as f32
        } else {
            0.
        };
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
            slot,
            0.,
        ]);
    }
    Ok(float_bytes(values))
}
