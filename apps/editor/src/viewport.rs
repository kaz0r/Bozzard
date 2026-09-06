use super::*;
use glam::Mat4;
pub struct Drag {
    id: String,
    axis: usize,
    start: Transform,
    pointer: Pos2,
    screen_axis: Vec2,
    tool: Tool,
}
impl App {
    pub fn viewport(&mut self, ui: &mut egui::Ui) -> Result<()> {
        ui.horizontal(|ui| {
            ui.strong("Scene viewport");
            ui.separator();
            ui.selectable_value(&mut self.workspace.tool, Tool::Move, "Move");
            ui.selectable_value(&mut self.workspace.tool, Tool::Rotate, "Rotate");
            ui.selectable_value(&mut self.workspace.tool, Tool::Scale, "Scale");
            if ui.button("Reset view").clicked() {
                self.workspace.pan = [0.0; 2];
                self.workspace.orbit = [0.0; 2];
                self.workspace.zoom = 1.0;
            }
        });
        ui.weak("Click to select · Drag colored handles · Right drag to pan · Shift + right drag to orbit · Scroll to zoom");
        self.sync_assets()?;
        let available = ui.available_size().max(Vec2::splat(1.0));
        let (rect, response) = ui.allocate_exact_size(available, Sense::click_and_drag());
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
        if response.dragged_by(egui::PointerButton::Secondary) {
            let delta = ui.input(|i| i.pointer.delta());
            if ui.input(|i| i.modifiers.shift) && !self.workspace.layer_2d {
                self.workspace.orbit[0] += delta.x * 0.005;
                self.workspace.orbit[1] += delta.y * 0.005;
            } else {
                self.workspace.pan[0] += 2.0 * delta.x / rect.width();
                self.workspace.pan[1] -= 2.0 * delta.y / rect.height();
            }
        }
        if response.hovered() {
            self.workspace.zoom = (self.workspace.zoom
                * (ui.input(|i| i.smooth_scroll_delta.y) * 0.002).exp())
            .clamp(0.1, 20.0);
        }
        let aspect = size[0] as f32 / size[1] as f32;
        let mut scene = self.editor.render(self.layer(), aspect)?;
        if self.editor.play.is_none() {
            scene.view_projection =
                Mat4::from_translation(Vec3::new(
                    self.workspace.pan[0],
                    self.workspace.pan[1],
                    0.0,
                )) * Mat4::from_scale(Vec3::new(self.workspace.zoom, self.workspace.zoom, 1.0))
                    * scene.view_projection
                    * Mat4::from_rotation_x(self.workspace.orbit[1])
                    * Mat4::from_rotation_y(self.workspace.orbit[0]);
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
        for axis in 0..3 {
            let world_axis = parent.transform_vector3([Vec3::X, Vec3::Y, Vec3::Z][axis]);
            let Some(unit) = project(origin + world_axis) else {
                continue;
            };
            let screen = unit - center;
            if screen.length() < 2.0 {
                continue;
            }
            let color = [
                Color32::from_rgb(245, 95, 105),
                Color32::from_rgb(100, 230, 150),
                Color32::from_rgb(100, 160, 255),
            ][axis];
            let end = center + screen.normalized() * 70.0;
            if self.workspace.tool == Tool::Rotate {
                let mut last = None;
                let a = [Vec3::Y, Vec3::Z, Vec3::X][axis];
                let b = [Vec3::Z, Vec3::X, Vec3::Y][axis];
                for i in 0..=64 {
                    let angle = i as f32 / 64.0 * std::f32::consts::TAU;
                    let p = project(
                        origin + parent.transform_vector3(a * angle.cos() + b * angle.sin()) * 0.75,
                    );
                    if let (Some(previous), Some(p)) = (last, p) {
                        painter.line_segment([previous, p], egui::Stroke::new(1.2, color));
                    }
                    last = p;
                }
            }
            painter.line_segment([center, end], egui::Stroke::new(2.0, color));
            painter.rect_filled(Rect::from_center_size(end, Vec2::splat(10.0)), 2.0, color);
            painter.text(
                end + Vec2::new(8.0, -10.0),
                egui::Align2::LEFT_CENTER,
                ["X", "Y", "Z"][axis],
                egui::FontId::proportional(12.0),
                color,
            );
            let hit_rect = Rect::from_center_size(end, Vec2::splat(22.0)).intersect(rect);
            // The viewport itself captures drag responses. Start handles from the
            // press event, then retain ownership until release (even outside the handle).
            let press = ui.input(|i| {
                i.events.iter().find_map(|event| match event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } if hit_rect.contains(*pos) => Some(*pos),
                    _ => None,
                })
            });
            handled |= ui.input(|i| i.pointer.hover_pos().is_some_and(|p| hit_rect.contains(p)))
                || self.drag.is_some();
            if let Some(pointer) = press {
                self.editor.begin_gesture("Transform gizmo");
                self.drag = Some(Drag {
                    id: object.id.clone(),
                    axis,
                    start: object.transform,
                    pointer,
                    screen_axis: screen,
                    tool: self.workspace.tool,
                });
            }
            if let (Some(drag), Some(pointer)) = (&self.drag, ui.input(|i| i.pointer.latest_pos()))
                && drag.axis == axis
            {
                let delta = pointer - drag.pointer;
                let mut next = self.editor.scene().clone();
                if let Some(object) = next.objects.iter_mut().find(|o| o.id == drag.id) {
                    object.transform = drag.start;
                    let amount = delta.dot(drag.screen_axis.normalized());
                    match drag.tool {
                        Tool::Move => {
                            object.transform.translation[drag.axis] +=
                                amount / drag.screen_axis.length()
                        }
                        Tool::Rotate => object.transform.rotation_degrees[drag.axis] += amount,
                        Tool::Scale => {
                            object.transform.scale[drag.axis] *=
                                (amount * 0.01).exp().clamp(0.01, 100.0)
                        }
                    }
                }
                let r = self.editor.apply("Transform gizmo", next);
                self.result(r);
            }
            if self.drag.as_ref().is_some_and(|drag| drag.axis == axis)
                && !ui.input(|i| i.pointer.primary_down())
            {
                self.drag = None;
                self.editor.finish_gesture();
            }
        }
        Ok(handled)
    }
}
