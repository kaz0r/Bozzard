use super::*;

pub(super) const RESOLUTION: u32 = 512;
pub(super) const FACES: usize = 6;
pub(super) const UNIFORM_SIZE: u64 = MAX_SHADOWED_POINT_LIGHTS as u64 * FACES as u64 * 80;

// Order agrees with point_shadow_face in local_lights.wgsl. These are ordinary
// 2D array layers; casters and receivers share the same explicit projection matrices.
const AXES: [(Vec3, Vec3); FACES] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::Z),
    (Vec3::NEG_Y, Vec3::NEG_Z),
    (Vec3::Z, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y),
];

fn projections(light: &LocalLight) -> Result<[Mat4; FACES]> {
    light.validate()?;
    ensure!(
        light.spot_angles.is_none(),
        "point shadow projection needs a point light"
    );
    // Two texels of real overlap on each edge contain the 3x3 bilinear PCF
    // footprint (1.5 texels). Never clamp a filter tap against an unrelated face.
    let half_tan = RESOLUTION as f32 / (RESOLUTION - 4) as f32;
    let near = (light.range * 0.001).min(0.05);
    let projection =
        glam::camera::rh::proj::directx::perspective(2. * half_tan.atan(), 1., near, light.range);
    let matrices = AXES.map(|(direction, up)| {
        projection * glam::camera::rh::view::look_to_mat4(Vec3::from(light.position), direction, up)
    });
    ensure!(
        matrices.iter().all(|m| m.is_finite()),
        "invalid point shadow projection"
    );
    Ok(matrices)
}
impl SceneRenderer {
    pub(super) fn update_point_shadows(&mut self, gpu: &Gpu, scene: &RenderScene) -> Result<()> {
        let mut maps = Vec::new();
        for light in scene
            .lights
            .iter()
            .filter(|l| l.spot_angles.is_none() && l.casts_shadow())
        {
            maps.extend(projections(light)?.map(|m| (m, light.shadows.unwrap())));
        }
        if self.shadows.points.update(gpu, &maps)? {
            self.shadows.rebind(gpu);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn six_faces_cover_axes_edges_and_corners_with_pcf_border() {
        for range in [0.001_f32, 10., 100_000.] {
            let origin = Vec3::new(0.2, -0.1, 0.3) * range.min(10.);
            let light = LocalLight {
                directional: false,
                position: origin.to_array(),
                direction: [0., 0., -1.],
                color: [1.; 3],
                intensity: 1.,
                range,
                spot_angles: None,
                shadows: Some(Default::default()),
            };
            let maps = projections(&light).unwrap();
            for (matrix, (forward, up)) in maps.iter().zip(AXES) {
                let near = (range * 0.001).min(0.05);
                for (distance, expected) in [(near, 0.), (range, 1.)] {
                    let p = matrix.project_point3(origin + forward * distance);
                    assert!((p.z - expected).abs() < 0.002, "{p:?}");
                }
                let right = forward.cross(up);
                for x in [-1., 0., 1.] {
                    for y in [-1., 0., 1.] {
                        let ray = forward + right * x + up * y;
                        let p = matrix.project_point3(origin + ray * (range * 0.25));
                        let pixel_border = (1. - p.x.abs().max(p.y.abs())) * RESOLUTION as f32 / 2.;
                        assert!(pixel_border > 1.9, "PCF crosses face edge: {p:?}");
                        assert!((0.0..1.).contains(&p.z));
                    }
                }
                assert!((*matrix * (origin - forward * range).extend(1.)).w < 0.);
            }
            let mut rotated = light;
            rotated.direction = [1., 2., 3.];
            assert_eq!(
                maps,
                projections(&rotated).unwrap(),
                "point rotation must not affect its maps"
            );
        }
    }
}
