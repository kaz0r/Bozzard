use super::*;

pub(super) const RESOLUTION: u32 = 1024;
pub(super) const UNIFORM_SIZE: u64 = MAX_SHADOWED_SPOT_LIGHTS as u64 * 80;

/// The cone's local -Z uses WebGPU's 0..1 perspective depth, independent of the camera.
fn projection(light: &LocalLight) -> Result<Mat4> {
    light.validate()?;
    let outer = light
        .spot_angles
        .context("shadow projection needs a spotlight")?[1];
    let direction = Vec3::from(light.direction).normalize();
    let up = if direction.y.abs() > 0.99 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let near = (light.range * 0.001).min(0.05);
    let view = glam::camera::rh::view::look_to_mat4(Vec3::from(light.position), direction, up);
    let matrix = glam::camera::rh::proj::directx::perspective(
        2. * outer.to_radians(),
        1.,
        near,
        light.range,
    ) * view;
    ensure!(matrix.is_finite(), "invalid spotlight shadow projection");
    Ok(matrix)
}
impl SceneRenderer {
    pub(super) fn update_spot_shadows(&mut self, gpu: &Gpu, scene: &RenderScene) -> Result<()> {
        let maps = scene
            .lights
            .iter()
            .filter(|l| l.spot_angles.is_some() && l.casts_shadow())
            .map(|l| Ok((projection(l)?, l.shadows.unwrap())))
            .collect::<Result<Vec<_>>>()?;
        if self.shadows.spots.update(gpu, &maps)? {
            self.shadows.rebind(gpu);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cone_projection_tracks_position_direction_and_webgpu_depth() {
        for direction in [
            Vec3::NEG_Z,
            Vec3::Y,
            -Vec3::Y,
            Vec3::new(1., 2., -3.).normalize(),
        ] {
            for range in [0.001_f32, 10., 100_000.] {
                for angle in [0.1_f32, 30., 89.9] {
                    // A tiny cone/range at a large origin would exceed f32 world precision.
                    let position = Vec3::new(0.2, -0.1, 0.3) * range.min(10.);
                    let light = LocalLight {
                        position: position.to_array(),
                        direction: direction.to_array(),
                        color: [1.; 3],
                        intensity: 1.,
                        range,
                        spot_angles: Some([0., angle]),
                        shadows: Some(Default::default()),
                    };
                    let matrix = projection(&light).unwrap();
                    let near = (range * 0.001).min(0.05);
                    for (distance, depth) in [(near, 0.), (range, 1.)] {
                        let p = matrix.project_point3(position + direction * distance);
                        // Narrow cones amplify f32 translation cancellation near the light.
                        // Check depth here and check axis placement at the far plane.
                        assert!((p.z - depth).abs() < 0.002, "{p:?} {range} {angle}");
                        if depth == 1. {
                            assert!(p.x.abs() < 0.002 && p.y.abs() < 0.002);
                        }
                    }
                    let up = if direction.y.abs() > 0.99 {
                        Vec3::Z
                    } else {
                        Vec3::Y
                    };
                    let side = direction.cross(up).normalize();
                    let edge = position
                        + direction * range * 0.5
                        + side * (range * 0.5 * angle.to_radians().tan());
                    assert!((matrix.project_point3(edge).x.abs() - 1.).abs() < 0.002);
                    assert!((matrix * (position - direction * range).extend(1.)).w < 0.);
                }
            }
        }
    }
}
