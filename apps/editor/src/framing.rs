use super::*;
use glam::Mat4;

fn corners([min, max]: [Vec3; 2]) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        )
    })
}
pub fn fit_2d(bounds: [Vec3; 2], projection: Mat4) -> Result<([f32; 2], f32)> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for point in corners(bounds) {
        let clip = projection * point.extend(1.0);
        ensure!(
            clip.is_finite() && clip.w > 0.0,
            "object lies behind the 2D camera"
        );
        let point = clip.truncate() / clip.w;
        ensure!(
            (0.0..=1.0).contains(&point.z),
            "object lies outside the 2D camera clipping range"
        );
        min = min.min(point);
        max = max.max(point);
    }
    let extent = (max - min).max_element().max(0.1);
    let zoom = (1.7 / extent).clamp(0.0001, 10000.0);
    let center = (min + max) * 0.5;
    Ok(([-center.x * zoom, -center.y * zoom], zoom))
}
pub fn fit_3d(
    bounds: [Vec3; 2],
    rotation: Mat4,
    camera: Camera,
    aspect: f32,
) -> Result<(Vec3, f32)> {
    let center = bounds[0] * 0.5 + bounds[1] * 0.5;
    let inverse = rotation.inverse();
    let local = corners(bounds).map(|p| inverse.transform_vector3(p - center));
    let mut distance: f32 = 1.0;
    let (near, far, zoom) = match camera {
        Camera::Perspective {
            vertical_fov_degrees,
            near,
            far,
        } => {
            let tan_y = (vertical_fov_degrees.to_radians() * 0.5).tan();
            for p in local {
                distance = distance
                    .max(p.z + (p.x.abs() / (tan_y * aspect)).max(p.y.abs() / tan_y) * 1.15);
            }
            (near, far, 1.0)
        }
        Camera::Orthographic {
            vertical_size,
            near,
            far,
        } => {
            let required = local
                .iter()
                .map(|p| (p.x.abs() / aspect).max(p.y.abs()) * 2.3)
                .fold(0.1, f32::max);
            (near, far, (vertical_size / required).clamp(0.0001, 10000.0))
        }
    };
    for p in local {
        distance = distance.max(p.z + near * 1.2);
    }
    ensure!(
        local.iter().all(|p| distance - p.z < far),
        "scene is too deep for the camera's far clipping plane"
    );
    let position = center + rotation.transform_vector3(Vec3::Z * distance);
    ensure!(position.is_finite(), "framed camera position is not finite");
    Ok((position, zoom))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn perspective_fits_every_corner_in_wide_and_portrait_views() {
        let bounds = [Vec3::new(-8.0, -2.0, -4.0), Vec3::new(3.0, 6.0, 7.0)];
        let rotation = Mat4::from_rotation_y(0.7) * Mat4::from_rotation_x(-0.3);
        let camera = Camera::Perspective {
            vertical_fov_degrees: 55.0,
            near: 0.1,
            far: 1000.0,
        };
        for aspect in [0.4, 2.5] {
            let (position, _) = fit_3d(bounds, rotation, camera, aspect).unwrap();
            let vp = camera.projection(aspect).unwrap()
                * (Mat4::from_translation(position) * rotation).inverse();
            for point in corners(bounds) {
                let p = vp.project_point3(point);
                assert!(
                    p.x.abs() < 1.0 && p.y.abs() < 1.0 && (0.0..1.0).contains(&p.z),
                    "{p:?}"
                );
            }
        }
    }
    #[test]
    fn orthographic_fit_recenters_and_handles_point_selection() {
        let camera = Camera::Orthographic {
            vertical_size: 10.0,
            near: 0.1,
            far: 100.0,
        };
        let bounds = [Vec3::new(20.0, 10.0, -5.0), Vec3::new(40.0, 12.0, -5.0)];
        let projection = camera.projection(0.5).unwrap();
        let (pan, zoom) = fit_2d(bounds, projection).unwrap();
        let vp = Mat4::from_translation(Vec3::new(pan[0], pan[1], 0.0))
            * Mat4::from_scale(Vec3::new(zoom, zoom, 1.0))
            * projection;
        for point in corners(bounds) {
            let p = vp.project_point3(point);
            assert!(p.x.abs() <= 0.851 && p.y.abs() <= 0.851);
        }
        assert!(
            fit_2d([Vec3::new(0.0, 0.0, -1.0); 2], projection)
                .unwrap()
                .1
                .is_finite()
        );
        assert!(fit_2d([Vec3::Z; 2], projection).is_err());
        let (position, zoom) = fit_3d(bounds, Mat4::IDENTITY, camera, 0.5).unwrap();
        let vp = Mat4::from_scale(Vec3::new(zoom, zoom, 1.0))
            * projection
            * Mat4::from_translation(-position);
        for point in corners(bounds) {
            let p = vp.project_point3(point);
            assert!(p.x.abs() < 1.0 && p.y.abs() < 1.0);
        }
    }
}
