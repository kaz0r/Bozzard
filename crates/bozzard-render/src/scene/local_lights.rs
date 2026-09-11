use super::*;

pub const MAX_LOCAL_LIGHTS: usize = 32;
pub(super) const UNIFORM_SIZE: u64 = 16 + MAX_LOCAL_LIGHTS as u64 * 64;

/// World-space object light. Directional lights ignore position and range.
#[derive(Clone, Copy, Debug)]
pub struct LocalLight {
    pub directional: bool,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub color: [f32; 3],
    /// Candela for point/spot, lux for directional lights.
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
        ensure!(
            !self.directional || self.spot_angles.is_none(),
            "directional lights cannot have a spot cone"
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
            if light.directional {
                2.
            } else if light.spot_angles.is_some() {
                1.
            } else {
                0.
            },
            0.,
            0.,
        ]);
    }
    Ok(float_bytes(values))
}

#[test]
fn directional_uniform_and_validation() {
    let mut light = LocalLight {
        directional: true,
        position: [100., 200., 300.],
        direction: [0., 0., -5.],
        color: [1., 0.5, 0.],
        intensity: 2.,
        range: 0.001,
        spot_angles: None,
    };
    let bytes = uniform(&[light]).unwrap();
    let values: Vec<_> = bytes
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
        .collect();
    assert_eq!(values[0], 1.);
    assert_eq!(&values[12..15], &[0., 0., -1.]);
    assert_eq!(values[17], 2.);
    light.spot_angles = Some([10., 20.]);
    assert!(uniform(&[light]).is_err());
    light.spot_angles = None;
    light.direction = [0.; 3];
    assert!(uniform(&[light]).is_err());
    light.direction = [0., 0., -1.];
    light.intensity = f32::NAN;
    assert!(uniform(&[light]).is_err());
}
