use super::*;

#[derive(PartialEq)]
struct Influence {
    position: Vec3,
    range: f32,
    directional: bool,
    active: bool,
}

/// Light geometry is independent of positive intensity/color changes. A revision
/// lets bindings catch up even when an earlier draw failed halfway through.
pub(in crate::scene) struct LightSelection {
    enabled: bool,
    revision: u64,
    influences: Vec<Influence>,
}
impl Default for LightSelection {
    fn default() -> Self {
        Self {
            enabled: true,
            revision: 1,
            influences: vec![],
        }
    }
}
impl LightSelection {
    pub(in crate::scene) fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.revision += 1;
        }
    }
    pub(in crate::scene) fn update(&mut self, lights: &[LocalLight]) {
        let matches = |(a, b): (&Influence, &LocalLight)| {
            a.position == Vec3::from(b.position)
                && a.range == b.range
                && a.directional == b.directional
                && a.active == (b.intensity > 0. && b.color.iter().any(|v| *v > 0.))
        };
        if self.influences.len() == lights.len() && self.influences.iter().zip(lights).all(matches)
        {
            return;
        }
        self.influences = lights
            .iter()
            .map(|light| Influence {
                position: light.position.into(),
                range: light.range,
                directional: light.directional,
                active: light.intensity > 0. && light.color.iter().any(|v| *v > 0.),
            })
            .collect();
        self.revision += 1;
    }
    pub(in crate::scene) fn revision(&self) -> u64 {
        self.revision
    }
    fn all(&self) -> u32 {
        ((1u64 << self.influences.len()) - 1) as u32
    }
    pub(in crate::scene) fn mask(&self, model: Mat4, bounds: [Vec3; 2], lit: bool) -> u32 {
        if !self.enabled {
            return self.all();
        }
        if !lit || self.influences.is_empty() {
            return 0;
        }
        // Projective model transforms and uncertain bounds retain every light.
        if model.x_axis.w != 0.
            || model.y_axis.w != 0.
            || model.z_axis.w != 0.
            || model.w_axis.w != 1.
            || !model.is_finite()
        {
            return self.all();
        }
        let mut world = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
        for corner in 0..8 {
            let p = model
                * Vec3::new(
                    bounds[corner & 1].x,
                    bounds[(corner >> 1) & 1].y,
                    bounds[(corner >> 2) & 1].z,
                )
                .extend(1.);
            if !p.is_finite() {
                return self.all();
            }
            world[0] = world[0].min(p.truncate());
            world[1] = world[1].max(p.truncate());
        }
        let arithmetic = model
            .to_cols_array()
            .into_iter()
            .map(f32::abs)
            .fold(0., f32::max)
            * bounds[0].abs().max(bounds[1].abs()).max_element().max(1.)
            * 4.;
        let magnitude = arithmetic + world[0].abs().max(world[1].abs()).max_element() + 1.;
        self.influences
            .iter()
            .enumerate()
            .fold(0, |mask, (index, light)| {
                if !light.active {
                    return mask;
                }
                if light.directional {
                    return mask | (1 << index);
                }
                // Loose relative padding covers f32 vertex/interpolation arithmetic,
                // including large translations and lights tangent to a surface.
                let margin =
                    f64::from(magnitude + light.position.abs().max_element() + light.range) * 1e-5;
                let distance2: f64 = (0..3)
                    .map(|axis| {
                        let p = f64::from(light.position[axis]);
                        let nearest = p.clamp(f64::from(world[0][axis]), f64::from(world[1][axis]));
                        (p - nearest).powi(2)
                    })
                    .sum();
                if distance2 <= (f64::from(light.range) + margin).powi(2) {
                    mask | (1 << index)
                } else {
                    mask
                }
            })
    }
}
