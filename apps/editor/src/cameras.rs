//! Editor-only camera markers and a short lens-accurate viewing frustum.
use super::*;
use glam::Mat4;

fn corners(mut camera: Camera, pose: Mat4, aspect: f32) -> Result<[Vec3; 8]> {
    // A short preview, not the often kilometre-long far clipping plane.
    match &mut camera {
        Camera::Perspective { near, far, .. } | Camera::Orthographic { near, far, .. } => {
            *far = far.min(*near + 3.)
        }
    }
    let inverse = pose * camera.projection(aspect)?.inverse();
    Ok(std::array::from_fn(|i| {
        inverse.project_point3(Vec3::new(
            if i & 1 == 0 { -1. } else { 1. },
            if i & 2 == 0 { -1. } else { 1. },
            if i & 4 == 0 { 0. } else { 1. },
        ))
    }))
}

pub(super) fn draw(
    ui: &egui::Ui,
    rect: Rect,
    projection: Mat4,
    pointer: Option<Pos2>,
    object: &bozzard_scene::Object,
    pose: Mat4,
    selected: bool,
) -> Result<Option<f32>> {
    let Some(camera) = object.camera else {
        return Ok(None);
    };
    let painter = ui.painter().with_clip_rect(rect);
    let screen = |clip: glam::Vec4| {
        let p = clip.truncate() / clip.w;
        Pos2::new(
            rect.left() + (p.x + 1.) * rect.width() * 0.5,
            rect.top() + (1. - p.y) * rect.height() * 0.5,
        )
    };
    let color = if selected {
        Color32::from_rgb(255, 212, 90)
    } else {
        Color32::from_rgb(140, 205, 255)
    };
    let clip = projection * pose.transform_point3(Vec3::ZERO).extend(1.);
    let mut picked = None;
    if clip.w > 0. && (0.0..=clip.w).contains(&clip.z) {
        let center = screen(clip);
        if rect.contains(center) {
            let hovered = pointer.is_some_and(|p| p.distance(center) <= 12.);
            let stroke = egui::Stroke::new(if hovered { 2.5 } else { 1.5 }, color);
            painter.rect_filled(
                Rect::from_center_size(center, Vec2::new(15., 11.)),
                2.,
                Color32::from_black_alpha(190),
            );
            painter.rect_stroke(
                Rect::from_center_size(center, Vec2::new(15., 11.)),
                2.,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.add(egui::Shape::closed_line(
                vec![
                    center + egui::vec2(8., -2.),
                    center + egui::vec2(13., -5.),
                    center + egui::vec2(13., 5.),
                    center + egui::vec2(8., 2.),
                ],
                stroke,
            ));
            if hovered {
                picked = Some(clip.z / clip.w);
            }
            if hovered || selected {
                painter.text(
                    center + egui::vec2(17., -12.),
                    egui::Align2::LEFT_BOTTOM,
                    &object.name,
                    egui::FontId::proportional(12.),
                    color,
                );
            }
        }
    }
    if selected {
        let points = corners(camera, pose, rect.aspect_ratio())?;
        for i in 0..8 {
            for axis in 0..3 {
                let j = i ^ (1 << axis);
                if j > i
                    && let Some((a, b)) = colliders::clip_edge(
                        projection * points[i].extend(1.),
                        projection * points[j].extend(1.),
                    )
                {
                    painter.line_segment([screen(a), screen(b)], egui::Stroke::new(1., color));
                }
            }
        }
    }
    Ok(picked)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frustum_tracks_lens_aspect_and_parent_pose() {
        let pose = Mat4::from_translation(Vec3::new(2., 3., 4.)) * Mat4::from_rotation_y(0.7);
        for camera in [
            Camera::Perspective {
                vertical_fov_degrees: 60.,
                near: 0.1,
                far: 1000.,
            },
            Camera::Orthographic {
                vertical_size: 4.,
                near: 0.1,
                far: 1000.,
            },
        ] {
            let points = corners(camera, pose, 2.).unwrap();
            let lens = camera.projection(2.).unwrap() * pose.inverse();
            for (i, p) in points.into_iter().enumerate() {
                let clip = lens.project_point3(p);
                assert!((clip.x.abs() - 1.).abs() < 0.0001 && (clip.y.abs() - 1.).abs() < 0.0001);
                let local = pose.inverse().transform_point3(p);
                assert!((local.z + if i & 4 == 0 { 0.1 } else { 3.1 }).abs() < 0.0001);
            }
        }
    }
}
