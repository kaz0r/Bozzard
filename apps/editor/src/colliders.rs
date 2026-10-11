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
/// Decal volumes draw in amber, apart from the cyan and green collider guides.
const DECAL_GUIDE: Color32 = Color32::from_rgb(246, 190, 70);

impl App {
    /// The selected object's Decal volume as a wire box, with a line down its projection axis.
    /// Edit mode only: Play moves objects away from their authored transforms.
    pub fn decal_overlay(&self, ui: &egui::Ui, rect: Rect, projection: Mat4) {
        let scene = self.editor.scene();
        let Some(object) = self.editor.selected_object() else {
            return;
        };
        let Ok(Some(decal)) = bozzard_scene::middleware::registry::get::<
            bozzard_scene::middleware::decal::Decal,
        >(object) else {
            return;
        };
        // Compose the authored parent chain; the hierarchy is validated acyclic.
        let mut world = object.transform.matrix();
        let mut parent = object.parent.as_deref();
        for _ in 0..scene.objects.len() {
            let Some(next) = parent.and_then(|id| scene.objects.iter().find(|o| o.id == id)) else {
                break;
            };
            world = next.transform.matrix() * world;
            parent = next.parent.as_deref();
        }
        let volume = world * Mat4::from_scale(glam::Vec3::from(decal.size));
        let painter = ui.painter().with_clip_rect(rect);
        let project = |v: Vec4| {
            let p = v.truncate() / v.w;
            Pos2::new(
                rect.left() + (p.x + 1.0) * rect.width() * 0.5,
                rect.top() + (1.0 - p.y) * rect.height() * 0.5,
            )
        };
        let draw = |a: glam::Vec3, b: glam::Vec3, stroke: egui::Stroke| {
            if let Some((a, b)) = clip_edge(
                projection * volume.transform_point3(a).extend(1.0),
                projection * volume.transform_point3(b).extend(1.0),
            ) {
                painter.line_segment([project(a), project(b)], stroke);
            }
        };
        let corner = |i: usize| {
            glam::Vec3::new(
                if i & 1 == 0 { -0.5 } else { 0.5 },
                if i & 2 == 0 { -0.5 } else { 0.5 },
                if i & 4 == 0 { -0.5 } else { 0.5 },
            )
        };
        let edge = egui::Stroke::new(1.25, DECAL_GUIDE);
        for i in 0..8 {
            for axis in 0..3 {
                let j = i ^ (1 << axis);
                if j > i {
                    draw(corner(i), corner(j), edge);
                }
            }
        }
        // Projection runs from the top face down local -Y.
        draw(
            glam::Vec3::new(0.0, 0.5, 0.0),
            glam::Vec3::new(0.0, -0.5, 0.0),
            egui::Stroke::new(2.0, DECAL_GUIDE),
        );
    /// The sphere, capsule and cylinder colliders behind a snapshot's meshes, by object ID.
    fn shape_colliders<'a>(
        &self,
        snapshot: &'a bozzard_scene::CollisionSnapshot,
    ) -> std::collections::HashMap<&'a str, bozzard_scene::ShapeCollider> {
        snapshot
            .meshes
            .iter()
            .filter_map(|mesh| {
                let shape = if let Some(play) = &self.editor.play {
                    let entity = play.instance().entity(&mesh.id)?;
                    play.app
                        .world
                        .get::<bozzard_scene::ShapeCollider>(entity)
                        .copied()
                } else {
                    self.editor
                        .scene()
                        .objects
                        .iter()
                        .find(|o| o.id == mesh.id)?
                        .shape_collider
                };
                Some((mesh.id.as_str(), shape?))
            })
            .collect()
    }
    pub fn collider_overlay(&self, ui: &egui::Ui, rect: Rect, projection: Mat4) -> Result<f32> {
        let mut snapshot = self.editor.collisions()?;
        if self.editor.play.is_none() {
            let hidden = self
                .open_scenes
                .hidden_objects_in(self.open_scenes.active(), &self.editor);
            snapshot.boxes.retain(|bounds| !hidden.contains(&bounds.id));
            snapshot.meshes.retain(|mesh| !hidden.contains(&mesh.id));
            snapshot
                .overlaps
                .retain(|(a, b)| !hidden.contains(a) && !hidden.contains(b));
        }
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
        let mut sampled = false;
        let mut shapes = 0;
        let shape_colliders = self.shape_colliders(&snapshot);
        for mesh in &snapshot.meshes {
            let selected = self.editor.selected.as_deref() == Some(&mesh.id);
            let color = if colliding.contains(mesh.id.as_str()) {
                Color32::from_rgb(255, 155, 60)
            } else {
                Color32::from_rgb(150, 220, 100)
            };
            let draw = |a: glam::Vec3, b: glam::Vec3| {
                if let Some((a, b)) =
                    clip_edge(projection * a.extend(1.), projection * b.extend(1.))
                {
                    painter.line_segment(
                        [project(a), project(b)],
                        egui::Stroke::new(if selected { 1.5 } else { 1. }, color),
                    );
                }
            };
            // A sphere, capsule or cylinder draws its own outline, not its faceted surface.
            if let Some(shape) = shape_colliders.get(mesh.id.as_str()) {
                shapes += 1;
                for [a, b] in shape.outline() {
                    draw(
                        mesh.matrix.transform_point3(a),
                        mesh.matrix.transform_point3(b),
                    );
                }
                continue;
            }
            let bounds = mesh.mesh.bounds();
            let corners: [glam::Vec3; 8] = std::array::from_fn(|i| {
                mesh.matrix.transform_point3(glam::Vec3::new(
                    bounds[i & 1].x,
                    bounds[(i >> 1) & 1].y,
                    bounds[(i >> 2) & 1].z,
                ))
            });
            for i in 0..8 {
                for axis in 0..3 {
                    let j = i ^ (1 << axis);
                    if j > i {
                        draw(corners[i], corners[j]);
                    }
                }
            }
            if selected {
                // ponytail: cap debug wire work, not collision accuracy; GPU wire rendering if needed.
                let step = mesh.mesh.triangles().len().div_ceil(2048).max(1);
                sampled |= step > 1;
                for triangle in mesh.mesh.triangles().iter().step_by(step) {
                    let p =
                        triangle.map(|p| mesh.matrix.transform_point3(glam::Vec3::from_array(p)));
                    for i in 0..3 {
                        draw(p[i], p[(i + 1) % 3]);
                    }
                }
            }
        }
        let mut label = format!(
            "Colliders: {} boxes + {} shapes + {} meshes  ·  Overlaps: {}",
            snapshot.boxes.len(),
            shapes,
            snapshot.meshes.len() - shapes,
            snapshot.overlaps.len()
        );
        if sampled {
            label.push_str("\nSelected mesh wires sampled (2048 triangles max)");
        }
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
