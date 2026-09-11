use super::*;
use glam::{Mat4, Vec4};

// Clip the wire segment before perspective division (WebGPU depth: 0 <= z <= w).
pub(super) fn clip_edge(a: Vec4, b: Vec4) -> Option<(Vec4, Vec4)> {
    let planes = |v: Vec4| [v.w + v.x, v.w - v.x, v.w + v.y, v.w - v.y, v.z, v.w - v.z];
    let mut start = 0.0_f32;
    let mut end = 1.0_f32;
    for (a, b) in planes(a).into_iter().zip(planes(b)) {
        if a < 0.0 && b < 0.0 {
            return None;
        }
        if a < 0.0 {
            start = start.max(a / (a - b));
        }
        if b < 0.0 {
            end = end.min(a / (a - b));
        }
    }
    if start > end {
        return None;
    }
    let from = a.lerp(b, start);
    let to = a.lerp(b, end);
    (from.w > 0.0 && to.w > 0.0).then_some((from, to))
}
impl App {
    pub fn collider_overlay(&self, ui: &egui::Ui, rect: Rect, projection: Mat4) -> Result<f32> {
        let snapshot = self.editor.collisions()?;
        let painter = ui.painter().with_clip_rect(rect);
        let colliding: std::collections::BTreeSet<_> = snapshot
            .overlaps
            .iter()
            .flat_map(|(a, b)| [a.as_str(), b.as_str()])
            .collect();
        let project = |v: Vec4| {
            let p = v.truncate() / v.w;
            Pos2::new(
                rect.left() + (p.x + 1.0) * rect.width() * 0.5,
                rect.top() + (1.0 - p.y) * rect.height() * 0.5,
            )
        };
        for bounds in &snapshot.boxes {
            let color = if colliding.contains(bounds.id.as_str()) {
                Color32::from_rgb(255, 155, 60)
            } else {
                Color32::from_rgb(75, 205, 220)
            };
            let width = if self.editor.selected.as_deref() == Some(&bounds.id) {
                2.0
            } else {
                1.25
            };
            for i in 0..8 {
                for axis in 0..3 {
                    let j = i ^ (1 << axis);
                    if j <= i {
                        continue;
                    }
                    if let Some((a, b)) = clip_edge(
                        projection * bounds.corners[i].extend(1.0),
                        projection * bounds.corners[j].extend(1.0),
                    ) {
                        painter.line_segment(
                            [project(a), project(b)],
                            egui::Stroke::new(width, color),
                        );
                    }
                }
            }
        }
        let mut label = format!(
            "Colliders: {}  ·  Overlaps: {}",
            snapshot.boxes.len(),
            snapshot.overlaps.len()
        );
        for (a, b) in snapshot.overlaps.iter().take(4) {
            label.push_str(&format!("\n{a} ↔ {b}"));
        }
        if snapshot.overlaps.len() > 4 {
            label.push_str("\n… more overlaps");
        }
        let galley =
            painter.layout_no_wrap(label, egui::FontId::proportional(13.0), Color32::WHITE);
        let position = rect.left_top() + Vec2::splat(8.0);
        painter.rect_filled(
            Rect::from_min_size(
                position - Vec2::splat(4.0),
                galley.size() + Vec2::splat(8.0),
            ),
            3.0,
            Color32::from_black_alpha(180),
        );
        let height = galley.size().y + 12.0;
        painter.galley(position, galley, Color32::WHITE);
        Ok(height)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wires_clip_at_near_plane_instead_of_flipping_behind_camera() {
        assert!(
            clip_edge(
                Vec4::new(0.0, 0.0, -2.0, 1.0),
                Vec4::new(1.0, 0.0, -1.0, 1.0)
            )
            .is_none()
        );
        let (a, b) = clip_edge(
            Vec4::new(0.0, 0.0, -1.0, 1.0),
            Vec4::new(0.5, 0.0, 0.5, 1.0),
        )
        .unwrap();
        assert!(a.z.abs() < 0.0001);
        assert_eq!(b, Vec4::new(0.5, 0.0, 0.5, 1.0));
    }
}
