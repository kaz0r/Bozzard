use super::*;

#[derive(PartialEq)]
struct Influence {
    position: Vec3,
    range: f32,
    directional: bool,
    active: bool,
    cone: Option<(Vec3, f32)>,
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
                && a.cone == cone(b)
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
                cone: cone(light),
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
                if distance2 <= (f64::from(light.range) + margin).powi(2)
                    && light.cone.is_none_or(|(direction, cosine)| {
                        cone_intersects(world, light.position, direction, cosine, margin)
                    })
                {
                    mask | (1 << index)
                } else {
                    mask
                }
            })
    }
}

fn cone(light: &LocalLight) -> Option<(Vec3, f32)> {
    light.spot_angles.map(|angles| {
        // Match the direction and outer-angle inputs sent to the fragment shader.
        (
            Vec3::from(light.direction).normalize(),
            angles[1].to_radians().cos(),
        )
    })
}

/// A box's enclosing sphere cannot intersect an acute cone when its signed
/// distance to the infinite cone exceeds the radius. Range is checked separately.
/// Double precision and the existing world arithmetic margin retain tangencies.
fn cone_intersects(
    bounds: [Vec3; 2],
    origin: Vec3,
    direction: Vec3,
    cosine: f32,
    margin: f64,
) -> bool {
    let center = (bounds[0].as_dvec3() + bounds[1].as_dvec3()) * 0.5;
    let radius = ((bounds[1].as_dvec3() - bounds[0].as_dvec3()) * 0.5).length() + margin;
    let offset = center - origin.as_dvec3();
    let axis = direction.as_dvec3().normalize();
    let axial = offset.dot(axis);
    if axial + radius < 0. {
        return false;
    }
    let radial = (offset.length_squared() - axial * axial).max(0.).sqrt();
    let cosine = f64::from(cosine).clamp(0., 1.);
    let sine = (1. - cosine * cosine).max(0.).sqrt();
    radial * cosine - axial * sine <= radius
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spot(angle: f32) -> LocalLight {
        LocalLight {
            directional: false,
            position: [0.; 3],
            direction: [0., 0., -1.],
            color: [1.; 3],
            intensity: 1.,
            range: 20.,
            spot_angles: Some([angle, angle]),
            shadows: None,
        }
    }
    #[test]
    fn cone_masks_reject_sphere_false_positives_and_refresh_direction() {
        let mut selection = LightSelection::default();
        let mut light = spot(10.);
        selection.update(&[light]);
        let bound = [Vec3::splat(-0.1), Vec3::splat(0.1)];
        assert_eq!(
            selection.mask(Mat4::from_translation(Vec3::new(5., 0., -5.)), bound, true),
            0
        );
        assert_eq!(
            selection.mask(Mat4::from_translation(Vec3::new(0., 0., -5.)), bound, true),
            1
        );
        let revision = selection.revision();
        light.direction = [1., 0., -1.];
        selection.update(&[light]);
        assert!(selection.revision() > revision);
        assert_eq!(
            selection.mask(Mat4::from_translation(Vec3::new(5., 0., -5.)), bound, true),
            1
        );
        let revision = selection.revision();
        light.spot_angles = Some([80., 80.]);
        selection.update(&[light]);
        assert!(selection.revision() > revision);
        assert_eq!(
            selection.mask(Mat4::from_translation(Vec3::new(0., 0., -5.)), bound, true),
            1
        );
    }
    #[test]
    fn cone_sphere_test_keeps_every_sampled_illuminated_point() {
        for angle in [0.1_f32, 10., 45., 89.9] {
            for axis in [Vec3::NEG_Z, Vec3::new(1., 2., 3.).normalize()] {
                for x in -20..=20 {
                    for y in -20..=20 {
                        for z in -20..=20 {
                            let p = Vec3::new(x as f32, y as f32, z as f32) * 0.5;
                            let half = Vec3::new(0.13, 0.17, 0.19);
                            let bounds = [p - half, p + half];
                            let cosine = angle.to_radians().cos();
                            for point in shadows::corners(bounds).chain(std::iter::once(p)) {
                                if point.length_squared() > 0.
                                    && axis.dot(point.normalize()) >= cosine
                                {
                                    assert!(
                                        cone_intersects(bounds, Vec3::ZERO, axis, cosine, 1e-4),
                                        "{angle} {bounds:?} {point:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
