use super::*;

/// Editor-only preferences. Snapping measures changes from the start of each drag.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapping {
    pub enabled: bool,
    pub movement: f32,
    pub rotation: f32,
    pub scale: f32,
}
impl Default for Snapping {
    fn default() -> Self {
        Self {
            enabled: false,
            movement: 0.5,
            rotation: 15.0,
            scale: 0.1,
        }
    }
}
impl Snapping {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.enabled, "Snap")
            .on_hover_text("Snap gizmo changes from the drag's starting transform. Hold Ctrl to temporarily invert this setting.");
        ui.menu_button("Snap settings", |ui| {
            ui.label("Move: local units per step");
            ui.add(egui::DragValue::new(&mut self.movement).speed(0.05).range(0.001..=1000.0));
            ui.label("Rotate: degrees per step");
            ui.add(egui::DragValue::new(&mut self.rotation).speed(1.0).range(0.1..=180.0));
            ui.label("Scale: multiplier step (0.1 = 10%)");
            ui.add(egui::DragValue::new(&mut self.scale).speed(0.01).range(0.01..=1.0));
            ui.weak("Relative to drag start; preserves existing offsets. Inspector values remain exact.");
            if ui.button("Reset increments").clicked() {
                *self = Self { enabled: self.enabled, ..Default::default() };
            }
        });
    }
    pub fn transform(
        &self,
        start: Transform,
        tool: Tool,
        axis: usize,
        amount: f32,
        ctrl: bool,
    ) -> Transform {
        let mut next = start;
        let active = self.enabled ^ ctrl;
        match tool {
            Tool::Move => next.translation[axis] += quantize(amount, self.movement, active),
            Tool::Rotate => next.rotation_degrees[axis] += quantize(amount, self.rotation, active),
            Tool::Scale => {
                // Quantize the multiplier change, not the signed scale. Preserve mirrored axes
                // and avoid introducing a singular transform when dragging toward zero.
                let factor = 1.0 + quantize(amount - 1.0, self.scale, active);
                for (index, scale) in next.scale.iter_mut().enumerate() {
                    if axis == 3 || axis == index {
                        *scale *= factor.clamp(0.01, 100.0);
                    }
                }
            }
        }
        next
    }
}
fn quantize(value: f32, step: f32, active: bool) -> f32 {
    if active && step.is_finite() && step > 0.0 {
        let result = (f64::from(value) / f64::from(step)).round() * f64::from(step);
        if result.is_finite() && result.abs() <= f64::from(f32::MAX) {
            return result as f32;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_snapping_preserves_offsets_and_rounds_both_directions() {
        let settings = Snapping {
            enabled: true,
            ..Default::default()
        };
        let start = Transform {
            translation: [0.13, 0.0, 0.0],
            rotation_degrees: [0.0, 7.0, 0.0],
            ..Default::default()
        };
        assert!(
            (settings
                .transform(start, Tool::Move, 0, 0.74, false)
                .translation[0]
                - 0.63)
                .abs()
                < 1e-6
        );
        assert!(
            (settings
                .transform(start, Tool::Move, 0, -0.74, false)
                .translation[0]
                + 0.37)
                .abs()
                < 1e-6
        );
        assert_eq!(
            settings
                .transform(start, Tool::Rotate, 1, 23.0, false)
                .rotation_degrees[1],
            37.0
        );
        assert_eq!(
            settings
                .transform(start, Tool::Rotate, 1, -361.0, false)
                .rotation_degrees[1],
            -353.0
        );
    }
    #[test]
    fn modifier_inverts_snap_and_scale_preserves_mirroring_without_zero() {
        let mut settings = Snapping::default();
        let start = Transform {
            scale: [-2.0, 3.0, 1.0],
            ..Default::default()
        };
        assert_eq!(
            settings
                .transform(start, Tool::Move, 0, 0.31, false)
                .translation[0],
            0.31
        );
        assert_eq!(
            settings
                .transform(start, Tool::Move, 0, 0.31, true)
                .translation[0],
            0.5
        );
        settings.enabled = true;
        assert_eq!(
            settings
                .transform(start, Tool::Move, 0, 0.31, true)
                .translation[0],
            0.31
        );
        let scaled = settings.transform(start, Tool::Scale, 0, 1.24, false);
        assert!((scaled.scale[0] + 2.4).abs() < 1e-6);
        assert_eq!(scaled.scale[1], 3.0);
        assert!(settings.transform(start, Tool::Scale, 0, 0.01, false).scale[0] < 0.0);
        assert_eq!(settings.transform(start, Tool::Scale, 0, 1.0, false), start);
    }
    #[test]
    fn uniform_scaling_snaps_one_factor_and_preserves_proportions() {
        let settings = Snapping {
            enabled: true,
            ..Default::default()
        };
        let start = Transform {
            scale: [-2.0, 3.0, 0.5],
            ..Default::default()
        };
        let next = settings.transform(start, Tool::Scale, 3, 1.24, false);
        for axis in 0..3 {
            assert!((next.scale[axis] / start.scale[axis] - 1.2).abs() < 1e-6);
        }
        assert_eq!(settings.transform(start, Tool::Scale, 3, 1.0, false), start);
        assert!(settings.transform(start, Tool::Scale, 3, 0.01, false).scale[0] < 0.0);
    }
    #[test]
    fn old_workspace_defaults_and_invalid_increments_are_safe() {
        let workspace: Workspace = serde_json::from_str("{}").unwrap();
        assert_eq!(workspace.snapping.rotation, 15.0);
        assert!(!workspace.snapping.enabled);
        for step in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(quantize(0.3, step, true), 0.3);
        }
        let settings = Snapping {
            enabled: true,
            movement: 2.0,
            ..Default::default()
        };
        let restored: Snapping =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(restored.enabled);
        assert_eq!(restored.movement, 2.0);
    }
}
