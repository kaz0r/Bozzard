use super::*;
use glam::Mat4;
pub struct Drag {
    id: String,
    axis: usize,
    start: Transform,
    pointer: Pos2,
    screen_axis: Vec2,
    tool: Tool,
    ring: Option<RingDrag>,
}
struct RingDrag {
    segments: Vec<(Pos2, Pos2, f32)>,
    last_angle: f32,
    angle: f32,
}
fn ring_hit(segments: &[(Pos2, Pos2, f32)], pointer: Pos2) -> Option<(f32, f32)> {
    segments
        .iter()
        .map(|&(a, b, angle)| {
            let line = b - a;
            let t = ((pointer - a).dot(line) / line.length_sq().max(0.0001)).clamp(0.0, 1.0);
            (
                (pointer - (a + line * t)).length(),
                angle + t * std::f32::consts::TAU / 64.0,
            )
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}
struct GizmoAxis {
    axis: usize,
    screen: Vec2,
    base: Color32,
    end: Pos2,
    segments: Vec<(Pos2, Pos2, f32)>,
}
fn nearest_axis(axes: &[GizmoAxis], rect: Rect, pointer: Pos2) -> Option<usize> {
    if !rect.contains(pointer) {
        return None;
    }
    axes.iter()
        .filter_map(|axis| {
            let handle = pointer.distance(axis.end);
            let ring = ring_hit(&axis.segments, pointer)
                .map(|hit| hit.0)
                .unwrap_or(f32::INFINITY);
            let distance = if handle <= 11.0 && axis.screen.length() >= 2.0 {
                handle
            } else {
                f32::INFINITY
            }
            .min(if ring <= 7.0 { ring } else { f32::INFINITY });
            distance.is_finite().then_some((axis.axis, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        .map(|hit| hit.0)
}
// Preserve joins between arc segments. Painting each translucent segment separately
// creates dark seams and bright blobs where their caps overlap.
fn ring_paths(segments: &[(Pos2, Pos2, f32)]) -> Vec<Vec<Pos2>> {
    let mut paths: Vec<Vec<Pos2>> = Vec::new();
    for &(a, b, _) in segments {
        if let Some(path) = paths.last_mut()
            && path.last() == Some(&a)
        {
            path.push(b);
        } else {
            paths.push(vec![a, b]);
        }
    }
    paths
}
fn paint_ring(painter: &egui::Painter, paths: &[Vec<Pos2>], stroke: egui::Stroke) {
    for path in paths {
        let closed = path
            .first()
            .zip(path.last())
            .is_some_and(|(a, b)| a.distance(*b) < 0.01);
        let mut points = path.clone();
        if closed {
            points.pop();
            painter.add(egui::Shape::closed_line(points, stroke));
        } else {
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}
fn angle_delta(current: f32, previous: f32) -> f32 {
    (current - previous + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI
}
#[derive(Serialize, Deserialize)]
pub struct FlyCamera {
    position: [f32; 3],
    yaw: f32,
    pitch: f32,
}
impl FlyCamera {
    fn from_pose(pose: Mat4) -> Self {
        let forward = pose.transform_vector3(-Vec3::Z).normalize_or_zero();
        Self {
            position: pose.transform_point3(Vec3::ZERO).to_array(),
            yaw: (-forward.x).atan2(-forward.z),
            pitch: forward.y.clamp(-1.0, 1.0).asin().clamp(-1.553, 1.553),
        }
    }
    fn rotate(&mut self, delta: Vec2) {
        self.yaw = (self.yaw - delta.x * 0.003).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch - delta.y * 0.003).clamp(-1.553, 1.553);
    }
    fn rotation(&self) -> Mat4 {
        look_rotation([self.yaw, self.pitch])
    }
    fn pose(&self) -> Mat4 {
        Mat4::from_translation(Vec3::from_array(self.position)) * self.rotation()
    }
    fn move_by(&mut self, direction: Vec3) {
        self.position = (Vec3::from_array(self.position) + direction).to_array();
    }
    fn flight_direction(&self, axes: Vec3) -> Vec3 {
        // WASD follows the view; vertical controls always follow world up.
        (self
            .rotation()
            .transform_vector3(Vec3::new(axes.x, 0.0, axes.z))
            + Vec3::Y * axes.y)
            .normalize_or_zero()
    }
}
fn look_rotation(look: [f32; 2]) -> Mat4 {
    Mat4::from_rotation_y(look[0]) * Mat4::from_rotation_x(look[1])
}
impl App {
    pub fn viewport(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Scene viewport");
            ui.separator();
            ui.selectable_value(&mut self.workspace.tool, Tool::Move, "Move");
            ui.selectable_value(&mut self.workspace.tool, Tool::Rotate, "Rotate");
            ui.selectable_value(&mut self.workspace.tool, Tool::Scale, "Scale");
            ui.add_enabled_ui(self.editor.play.is_none(), |ui| {
                self.workspace.snapping.ui(ui);
            });
            ui.checkbox(&mut self.workspace.colliders_visible, "Colliders");
            if ui.button("Reset view").clicked() {
                self.workspace.pan = [0.0; 2];
                self.workspace.zoom = 1.0;
                self.workspace.camera = None;
            }
        });
        if self.editor.play.is_some() {
            ui.weak("Select a collider before Play · Hover viewport: WASD move · Space/Ctrl up/down without gravity · Shift faster · Space: jump when grounded");
        } else {
            ui.weak("Drag rings/handles · Esc: cancel drag · Hover to highlight an axis · Right drag: look · Hold right + WASD: fly · Space/Ctrl: up/down · Shift: faster · Middle drag: pan · Scroll: dolly (2D: zoom)");
        }
        let can_navigate = self.editor.play.is_none()
            && ui.input(|i| i.focused && !i.key_pressed(egui::Key::Escape));
        if !can_navigate {
            self.navigation_button = None;
        }
        let looking = can_navigate
            && !self.workspace.layer_2d
            && self.navigation_button == Some(egui::PointerButton::Secondary)
            && ui.input(|i| i.pointer.secondary_down());
        if self.mouse_captured && !looking {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
            self.mouse_captured = false;
        }
        self.sync_assets()?;
        let available = ui.available_size().max(Vec2::splat(1.0));
        let (rect, response) = ui.allocate_exact_size(available, Sense::click_and_drag());
        if self.editor.play.is_some()
            && !self.workspace.layer_2d
            && response.hovered()
            && ui.input(|i| i.focused)
            && !ui.ctx().egui_wants_keyboard_input()
            && self
                .editor
                .selected_object()
                .and_then(|o| o.collider)
                .is_some_and(|c| c.enabled)
        {
            // Ignore OS key repeats: holding Space must not auto-jump after landing.
            if ui.input(|i| {
                i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Space,
                            pressed: true,
                            repeat: false,
                            ..
                        }
                    )
                })
            }) {
                let result = self.editor.jump_selected_box().map(|_| ());
                self.result(result);
            }
            let delta = ui.input(|i| {
                let axis = |positive, negative| {
                    i.key_down(positive) as u8 as f32 - i.key_down(negative) as u8 as f32
                };
                Vec3::new(
                    axis(egui::Key::D, egui::Key::A),
                    if self
                        .editor
                        .selected_object()
                        .and_then(|o| o.gravity)
                        .is_some_and(|g| g.enabled)
                    {
                        0.0
                    } else {
                        i.key_down(egui::Key::Space) as u8 as f32 - i.modifiers.ctrl as u8 as f32
                    },
                    axis(egui::Key::S, egui::Key::W),
                )
                .normalize_or_zero()
                    * i.stable_dt.min(0.05)
                    * if i.modifiers.shift { 8.0 } else { 3.0 }
            });
            if delta != Vec3::ZERO {
                let result = self.editor.move_selected_box(delta).map(|movement| {
                    self.status = if movement.contacts.is_empty() {
                        "Moving selected box".into()
                    } else {
                        format!("Blocked/sliding against {}", movement.contacts.join(", "))
                    };
                });
                self.result(result);
            }
        }
        let ppp = ui.ctx().pixels_per_point();
        let limit = self.gpu.device.limits().max_texture_dimension_2d.min(4096);
        let size = [
            (rect.width() * ppp).round().clamp(1.0, limit as f32) as u32,
            (rect.height() * ppp).round().clamp(1.0, limit as f32) as u32,
        ];
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Editor scene viewport"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
            });
            let view = texture.create_view(&Default::default());
            // egui samples gamma colors; reinterpret the sRGB render target as UNORM for the UI.
            let sampled = texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            });
            let mut renderer = self.render_state.renderer.write();
            let id = if let Some(target) = &self.target {
                renderer.update_egui_texture_from_wgpu_texture(
                    &self.gpu.device,
                    &sampled,
                    wgpu::FilterMode::Linear,
                    target.id,
                );
                target.id
            } else {
                renderer.register_native_texture(
                    &self.gpu.device,
                    &sampled,
                    wgpu::FilterMode::Linear,
                )
            };
            self.target = Some(Target {
                texture,
                view,
                id,
                size,
            });
        }
        let aspect = size[0] as f32 / size[1] as f32;
        let mut scene = self.editor.render(self.layer(), aspect)?;
        if self.editor.play.is_none() {
            ui.input(|i| {
                for event in &i.events {
                    if let egui::Event::PointerButton {
                        pos,
                        button,
                        pressed: true,
                        ..
                    } = event
                        && can_navigate
                        && rect.contains(*pos)
                        && matches!(
                            button,
                            egui::PointerButton::Secondary | egui::PointerButton::Middle
                        )
                    {
                        self.navigation_button = Some(*button);
                    }
                }
            });
            let right = self.navigation_button == Some(egui::PointerButton::Secondary);
            let middle = self.navigation_button == Some(egui::PointerButton::Middle);
            let delta = ui.input(|i| i.pointer.delta());
            let scroll = if can_navigate && response.hovered() {
                ui.input(|i| i.smooth_scroll_delta.y)
            } else {
                0.0
            };
            if self.workspace.layer_2d {
                if right || middle {
                    self.workspace.pan[0] += 2.0 * delta.x / rect.width();
                    self.workspace.pan[1] -= 2.0 * delta.y / rect.height();
                }
                self.workspace.zoom =
                    (self.workspace.zoom * (scroll * 0.002).exp()).clamp(0.1, 20.0);
                scene.view_projection =
                    Mat4::from_translation(Vec3::new(
                        self.workspace.pan[0],
                        self.workspace.pan[1],
                        0.0,
                    )) * Mat4::from_scale(Vec3::new(self.workspace.zoom, self.workspace.zoom, 1.0))
                        * scene.view_projection;
            } else {
                let doc = self.editor.scene();
                let camera_id = &doc.views[&self.layer()];
                let authored_camera = doc
                    .objects
                    .iter()
                    .find(|o| &o.id == camera_id)
                    .and_then(|o| o.camera)
                    .context("missing viewport camera")?;
                let lens = authored_camera.projection(aspect)?;
                let base = scene.view_projection.inverse() * lens;
                let camera = self
                    .workspace
                    .camera
                    .get_or_insert_with(|| FlyCamera::from_pose(base));
                let flying = right
                    && ui.input(|i| i.pointer.secondary_down())
                    && !ui.ctx().egui_wants_keyboard_input();
                if flying {
                    if !self.mouse_captured {
                        let mode = if cfg!(windows) {
                            egui::CursorGrab::Confined
                        } else {
                            egui::CursorGrab::Locked
                        };
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::CursorGrab(mode));
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
                        self.mouse_captured = true;
                        // Ignore movement preceding the initial button press.
                    } else {
                        camera.rotate(
                            ui.input(|i| i.pointer.motion().unwrap_or_else(|| i.pointer.delta())),
                        );
                    }
                    let (axes, distance) = ui.input(|i| {
                        let axis = |positive, negative| {
                            i.key_down(positive) as u8 as f32 - i.key_down(negative) as u8 as f32
                        };
                        (
                            Vec3::new(
                                axis(egui::Key::D, egui::Key::A),
                                i.key_down(egui::Key::Space) as u8 as f32
                                    - i.modifiers.ctrl as u8 as f32,
                                axis(egui::Key::S, egui::Key::W),
                            ),
                            i.stable_dt.min(0.05) * if i.modifiers.shift { 12.0 } else { 3.0 },
                        )
                    });
                    camera.move_by(camera.flight_direction(axes) * distance);
                    ui.ctx().request_repaint();
                }
                camera.move_by(camera.rotation().transform_vector3(Vec3::new(
                    0.0,
                    0.0,
                    -scroll * 0.003,
                )));
                if middle {
                    camera.move_by(
                        camera
                            .rotation()
                            .transform_vector3(Vec3::new(-delta.x, delta.y, 0.0))
                            * 0.01,
                    );
                }
                scene.view_projection = lens * camera.pose().inverse();
            }
        }
        if self
            .navigation_button
            .is_some_and(|button| !ui.input(|i| i.pointer.button_down(button)))
        {
            self.navigation_button = None;
        }
        let projection = scene.view_projection;
        let target = self.target.as_ref().unwrap();
        self.renderer.draw(&self.gpu, &target.view, size, &scene)?;
        ui.painter().image(
            target.id,
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        if self.workspace.colliders_visible && !self.workspace.layer_2d {
            self.collider_overlay(ui, rect, projection)?;
        }
        let handled = if self.editor.play.is_none() {
            self.gizmo(ui, rect, projection)?
        } else {
            false
        };
        if response.clicked()
            && !handled
            && self.editor.play.is_none()
            && let Some(p) = response.interact_pointer_pos()
        {
            let ndc = [
                2.0 * (p.x - rect.left()) / rect.width() - 1.0,
                1.0 - 2.0 * (p.y - rect.top()) / rect.height(),
            ];
            self.editor.finish_gesture();
            self.editor.selected =
                self.editor
                    .pick_with_projection(self.layer(), projection, ndc)?;
        }
        Ok(())
    }
    fn gizmo(&mut self, ui: &mut egui::Ui, rect: Rect, projection: Mat4) -> Result<bool> {
        if self.drag.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.editor.cancel_gesture()?;
            self.drag = None;
            self.status = "Gizmo drag cancelled".into();
            self.error = false;
            return Ok(true);
        }
        let Some(object) = self.editor.selected_object().cloned() else {
            return Ok(false);
        };
        let demo = bozzard_demo::SceneDemo::new(self.editor.scene())?;
        let matrices = demo.instance.global_transforms(&demo.app.world)?;
        let origin = matrices[&object.id].transform_point3(Vec3::ZERO);
        let parent = object
            .parent
            .as_ref()
            .map(|p| matrices[p])
            .unwrap_or(Mat4::IDENTITY);
        let project = |point: Vec3| -> Option<Pos2> {
            let clip = projection * point.extend(1.0);
            if clip.w <= 0.0 {
                return None;
            }
            let ndc = clip.truncate() / clip.w;
            if !(0.0..=1.0).contains(&ndc.z) {
                return None;
            }
            Some(Pos2::new(
                rect.left() + (ndc.x + 1.0) * 0.5 * rect.width(),
                rect.top() + (1.0 - ndc.y) * 0.5 * rect.height(),
            ))
        };
        let Some(center) = project(origin) else {
            return Ok(false);
        };
        if !rect.contains(center) {
            return Ok(false);
        }
        let painter = ui.painter().with_clip_rect(rect);
        painter.circle_stroke(center, 7.0, egui::Stroke::new(1.5, Color32::WHITE));
        let mut handled = false;
        let mut axes = Vec::new();
        for axis in 0..3 {
            let world_axis = parent.transform_vector3([Vec3::X, Vec3::Y, Vec3::Z][axis]);
            let screen = project(origin + world_axis)
                .map(|unit| unit - center)
                .unwrap_or(Vec2::ZERO);
            if screen.length() < 2.0 && self.workspace.tool != Tool::Rotate {
                continue;
            }
            let base = [
                Color32::from_rgb(245, 95, 105),
                Color32::from_rgb(100, 230, 150),
                Color32::from_rgb(100, 160, 255),
            ][axis];
            let end = center + screen.normalized() * 70.0;
            let mut segments = Vec::new();
            if self.workspace.tool == Tool::Rotate {
                let a = [Vec3::Y, Vec3::Z, Vec3::X][axis];
                let b = [Vec3::Z, Vec3::X, Vec3::Y][axis];
                let mut last = None;
                for i in 0..=64 {
                    let angle = i as f32 / 64.0 * std::f32::consts::TAU;
                    let p = project(
                        origin + parent.transform_vector3(a * angle.cos() + b * angle.sin()) * 0.75,
                    );
                    if let (Some(previous), Some(p)) = (last, p) {
                        segments.push((previous, p, (i - 1) as f32 * std::f32::consts::TAU / 64.0));
                    }
                    last = p;
                }
            }
            axes.push(GizmoAxis {
                axis,
                screen,
                base,
                end,
                segments,
            });
        }
        let hover_axis = ui
            .input(|i| i.pointer.hover_pos())
            .and_then(|p| nearest_axis(&axes, rect, p));
        let press = ui.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    ..
                } => Some(*pos),
                _ => None,
            })
        });
        let press_axis = press.and_then(|p| nearest_axis(&axes, rect, p));
        // Capture before drawing so the clicked axis highlights in the same frame.
        if let (Some(pointer), Some(axis)) = (press, press_axis)
            && self.drag.is_none()
        {
            let geometry = axes.iter().find(|g| g.axis == axis).unwrap();
            self.editor.begin_gesture("Transform gizmo");
            self.drag = Some(Drag {
                id: object.id.clone(),
                axis,
                start: object.transform,
                pointer,
                screen_axis: geometry.screen,
                tool: self.workspace.tool,
                ring: ring_hit(&geometry.segments, pointer)
                    .filter(|(d, _)| *d <= 7.0)
                    .map(|(_, angle)| RingDrag {
                        segments: geometry.segments.clone(),
                        last_angle: angle,
                        angle: 0.0,
                    }),
            });
        }
        let dragging = self.drag.is_some();
        let active_axis = self
            .drag
            .as_ref()
            .filter(|d| d.id == object.id)
            .map(|d| d.axis);
        let emphasized = active_axis.or(hover_axis);
        // Draw the selected axis last, so crossing rings cannot cover its highlight.
        axes.sort_by_key(|g| Some(g.axis) == emphasized);
        for geometry in axes {
            let GizmoAxis {
                axis,
                screen,
                base,
                end,
                segments,
            } = geometry;
            let hovered = !dragging && hover_axis == Some(axis);
            let active = self
                .drag
                .as_ref()
                .is_some_and(|d| d.axis == axis && d.id == object.id);
            let draw = if active {
                Color32::from_rgb(255, 205, 75)
            } else if hovered {
                Color32::from_rgb(255, 230, 135)
            } else if dragging {
                base.gamma_multiply(0.55)
            } else {
                base
            };
            let line_width = if active {
                3.5
            } else if hovered {
                3.0
            } else {
                2.0
            };
            let paths = ring_paths(&segments);
            // A narrow, uniform dark edge gives contrast without changing the
            // apparent ring radius or obscuring nearby axes with a large glow.
            let outline = egui::Stroke::new(line_width + 2.0, Color32::from_black_alpha(160));
            paint_ring(&painter, &paths, outline);
            if screen.length() >= 2.0 {
                painter.line_segment([center, end], outline);
            }
            paint_ring(&painter, &paths, egui::Stroke::new(line_width, draw));
            if screen.length() >= 2.0 {
                painter.line_segment([center, end], egui::Stroke::new(line_width, draw));
                let handle_rect = Rect::from_center_size(end, Vec2::splat(10.0));
                painter.rect_filled(handle_rect, 2.0, draw);
                painter.rect_stroke(
                    handle_rect,
                    2.0,
                    egui::Stroke::new(1.0, Color32::from_black_alpha(180)),
                    egui::StrokeKind::Outside,
                );
                painter.text(
                    end + Vec2::new(8.0, -10.0),
                    egui::Align2::LEFT_CENTER,
                    ["X", "Y", "Z"][axis],
                    egui::FontId::proportional(12.0),
                    draw,
                );
            }
            handled |= hovered || self.drag.is_some();
            if let (Some(drag), Some(pointer)) =
                (&mut self.drag, ui.input(|i| i.pointer.latest_pos()))
                && drag.axis == axis
                && drag.id == object.id
            {
                let delta = pointer - drag.pointer;
                let mut next = self.editor.scene().clone();
                if let Some(object) = next.objects.iter_mut().find(|o| o.id == drag.id) {
                    let amount = delta.dot(drag.screen_axis.normalized());
                    let amount = match drag.tool {
                        Tool::Move => amount / drag.screen_axis.length(),
                        Tool::Rotate => {
                            if let Some(ring) = &mut drag.ring {
                                if let Some((_, angle)) = ring_hit(&ring.segments, pointer) {
                                    ring.angle += angle_delta(angle, ring.last_angle);
                                    ring.last_angle = angle;
                                }
                                ring.angle.to_degrees()
                            } else {
                                amount
                            }
                        }
                        Tool::Scale => (amount * 0.01).exp().clamp(0.01, 100.0),
                    };
                    object.transform = self.workspace.snapping.transform(
                        drag.start,
                        drag.tool,
                        drag.axis,
                        amount,
                        ui.input(|i| i.modifiers.ctrl),
                    );
                }
                let r = self.editor.apply("Transform gizmo", next);
                self.result(r);
            }
        }
        if self.drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
            self.drag = None;
            self.editor.finish_gesture();
        }
        Ok(handled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_hit_accepts_the_arc_and_unwraps_a_full_turn() {
        let points: Vec<_> = (0..64)
            .map(|i| {
                let a = i as f32 * std::f32::consts::TAU / 64.0;
                let b = (i + 1) as f32 * std::f32::consts::TAU / 64.0;
                (
                    Pos2::new(a.cos() * 100.0, a.sin() * 100.0),
                    Pos2::new(b.cos() * 100.0, b.sin() * 100.0),
                    a,
                )
            })
            .collect();
        assert!(ring_hit(&points, Pos2::new(0.0, 100.0)).unwrap().0 < 1.0);
        assert!(ring_hit(&points, Pos2::ZERO).unwrap().0 > 90.0);
        let mut total = 0.0;
        let mut previous = 0.0;
        for i in 1..=80 {
            let a = i as f32 * std::f32::consts::TAU / 64.0;
            let (_, angle) =
                ring_hit(&points, Pos2::new(a.cos() * 100.0, a.sin() * 100.0)).unwrap();
            total += angle_delta(angle, previous);
            previous = angle;
        }
        assert!((total.to_degrees() - 450.0).abs() < 0.01);
    }
    #[test]
    fn nearest_axis_selects_one_ring_at_intersections_and_respects_viewport() {
        let rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(200.0, 200.0));
        let axis = |id, a, b| GizmoAxis {
            axis: id,
            screen: Vec2::X,
            base: Color32::WHITE,
            end: Pos2::new(190.0, 190.0),
            segments: vec![(a, b, 0.0)],
        };
        let axes = vec![
            axis(0, Pos2::new(30.0, 50.0), Pos2::new(170.0, 50.0)),
            axis(1, Pos2::new(80.0, 10.0), Pos2::new(80.0, 150.0)),
        ];
        assert_eq!(nearest_axis(&axes, rect, Pos2::new(85.0, 51.0)), Some(0));
        assert_eq!(nearest_axis(&axes, rect, Pos2::new(81.0, 55.0)), Some(1));
        assert_eq!(nearest_axis(&axes, rect, Pos2::new(80.0, 50.0)), Some(0));
        assert_eq!(nearest_axis(&axes, rect, Pos2::new(140.0, 140.0)), None);
        assert_eq!(nearest_axis(&axes, rect, Pos2::new(80.0, -1.0)), None);
    }
    #[test]
    fn ring_paths_join_neighbors_but_do_not_bridge_clipped_gaps() {
        let a = Pos2::new(0.0, 0.0);
        let b = Pos2::new(1.0, 1.0);
        let c = Pos2::new(2.0, 2.0);
        let d = Pos2::new(4.0, 4.0);
        let e = Pos2::new(5.0, 5.0);
        let paths = ring_paths(&[(a, b, 0.0), (b, c, 1.0), (d, e, 2.0)]);
        assert_eq!(paths, vec![vec![a, b, c], vec![d, e]]);
    }
    #[test]
    fn noclip_yaw_stays_world_upright_and_vertical_motion_ignores_pitch() {
        let initial = Mat4::from_translation(Vec3::new(4.0, 3.0, 6.0))
            * look_rotation([34.0_f32.to_radians(), -22.0_f32.to_radians()]);
        let mut camera = FlyCamera::from_pose(initial);
        assert!(camera.pose().abs_diff_eq(initial, 0.0001));
        let position = camera.position;
        camera.rotate(Vec2::new(600.0, 80.0));
        assert_eq!(camera.position, position);
        assert!(camera.rotation().transform_vector3(Vec3::X).y.abs() < 0.0001);
        assert!(
            camera
                .flight_direction(Vec3::Y)
                .abs_diff_eq(Vec3::Y, 0.0001)
        );
        assert!(
            camera
                .flight_direction(-Vec3::Y)
                .abs_diff_eq(-Vec3::Y, 0.0001)
        );
        let forward = camera.rotation().transform_vector3(-Vec3::Z);
        assert!(
            camera
                .flight_direction(-Vec3::Z)
                .abs_diff_eq(forward, 0.0001)
        );
        assert!((camera.flight_direction(Vec3::new(1.0, 1.0, -1.0)).length() - 1.0).abs() < 0.0001);
        camera.rotate(Vec2::new(1_000_000.0, -1_000_000.0));
        assert!(camera.pitch < std::f32::consts::FRAC_PI_2);
        assert!(camera.pose().is_finite());
        assert!(camera.rotation().transform_vector3(Vec3::X).y.abs() < 0.0001);
    }
    #[test]
    fn fly_forward_follows_camera_heading() {
        let forward = look_rotation([std::f32::consts::FRAC_PI_2, 0.0]).transform_vector3(-Vec3::Z);
        assert!(forward.abs_diff_eq(-Vec3::X, 0.0001));
    }
}
