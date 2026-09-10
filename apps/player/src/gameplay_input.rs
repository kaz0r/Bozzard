//! Window-local input adapter. Gameplay uses physical WASD, never selection or OS repeats.
use bozzard_scene::GameplayInput;
use winit::{
    event::{ElementState, MouseButton, WindowEvent},
    keyboard::KeyCode,
};

pub struct GameplayControls {
    held: [bool; 4],
    focused: bool,
    modified: bool,
    orbiting: bool,
    cursor: Option<[f64; 2]>,
    scale_factor: f64,
}
impl Default for GameplayControls {
    fn default() -> Self {
        Self {
            held: [false; 4],
            focused: false,
            modified: false,
            orbiting: false,
            cursor: None,
            scale_factor: 1.0,
        }
    }
}
impl GameplayControls {
    pub fn set_scale_factor(&mut self, scale_factor: f64) {
        if self.scale_factor != scale_factor {
            self.scale_factor = scale_factor;
            self.cursor = None;
        }
    }

    // Match egui pointer deltas: authored orbit sensitivity is degrees per logical point.
    fn cursor_moved(&mut self, position: [f64; 2]) -> GameplayInput {
        let next = position.map(|value| value / self.scale_factor);
        let previous = self.cursor.replace(next);
        if self.focused
            && self.orbiting
            && let Some(previous) = previous
        {
            GameplayInput {
                orbit: [
                    (next[0] - previous[0]) as f32,
                    (next[1] - previous[1]) as f32,
                ],
                ..self.current()
            }
        } else {
            self.current()
        }
    }

    pub fn reset(&mut self) {
        self.held = [false; 4];
        self.orbiting = false;
        self.cursor = None;
    }
    pub fn key(&mut self, code: KeyCode, pressed: bool, repeat: bool) -> GameplayInput {
        if !self.focused || self.modified {
            return GameplayInput::default();
        }
        if let Some(index) = [KeyCode::KeyA, KeyCode::KeyD, KeyCode::KeyW, KeyCode::KeyS]
            .iter()
            .position(|key| *key == code)
            && !repeat
        {
            self.held[index] = pressed;
        }
        GameplayInput {
            jump: code == KeyCode::Space && pressed && !repeat,
            ..self.current()
        }
    }
    pub fn current(&self) -> GameplayInput {
        GameplayInput {
            movement: [
                self.held[1] as u8 as f32 - self.held[0] as u8 as f32,
                self.held[2] as u8 as f32 - self.held[3] as u8 as f32,
            ],
            ..Default::default()
        }
    }
    /// None explicitly cancels pending edges on focus/cursor loss.
    /// RMB drag is deliberately unconfined: no hidden/locked pointer in gameplay.
    pub fn event(&mut self, event: &WindowEvent) -> Option<GameplayInput> {
        match event {
            WindowEvent::Focused(focused) => {
                self.focused = *focused;
                self.modified = false;
                self.reset();
                return None;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.modified = state.control_key() || state.alt_key() || state.super_key();
                if self.modified {
                    self.reset();
                    return None;
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.reset();
                return None;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => {
                self.orbiting = self.focused && !self.modified && *state == ElementState::Pressed;
            }
            WindowEvent::CursorMoved { position, .. } => {
                return Some(self.cursor_moved([position.x, position.y]));
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.set_scale_factor(*scale_factor);
                return None; // Discard queued deltas in the old coordinate system.
            }
            _ => {}
        }
        Some(self.current())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn orbit_uses_logical_points_and_scale_changes_reset_cursor() {
        let drag = |scale| {
            let mut controls = GameplayControls::default();
            controls.event(&WindowEvent::Focused(true));
            controls.set_scale_factor(scale);
            controls.orbiting = true;
            assert_eq!(
                controls.cursor_moved([100.0 * scale, 50.0 * scale]).orbit,
                [0.0; 2]
            );
            controls.cursor_moved([125.0 * scale, 40.0 * scale]).orbit
        };
        assert_eq!(drag(1.0), [25.0, -10.0]);
        assert_eq!(drag(1.0), drag(2.0));
        let mut controls = GameplayControls::default();
        controls.event(&WindowEvent::Focused(true));
        controls.orbiting = true;
        controls.cursor_moved([100.0, 50.0]);
        controls.set_scale_factor(2.0);
        assert_eq!(controls.cursor_moved([200.0, 100.0]).orbit, [0.0; 2]);
        assert_eq!(controls.cursor_moved([250.0, 80.0]).orbit, [25.0, -10.0]);
    }

    #[test]
    fn physical_movement_jump_edges_and_focus_loss() {
        let mut controls = GameplayControls::default();
        assert_eq!(controls.key(KeyCode::KeyW, true, false).movement, [0.0; 2]);
        controls.event(&WindowEvent::Focused(true));
        assert_eq!(
            controls.key(KeyCode::KeyW, true, false).movement,
            [0.0, 1.0]
        );
        assert!(controls.key(KeyCode::Space, true, false).jump);
        assert!(!controls.key(KeyCode::Space, true, true).jump);
        assert!(!controls.key(KeyCode::Space, false, false).jump);
        assert!(controls.event(&WindowEvent::Focused(false)).is_none());
        assert_eq!(controls.current().movement, [0.0; 2]);
        controls.event(&WindowEvent::Focused(true));
        assert_eq!(controls.current().movement, [0.0; 2]);
        assert_eq!(controls.key(KeyCode::KeyW, true, true).movement, [0.0; 2]);
        assert_eq!(
            controls.key(KeyCode::KeyW, true, false).movement,
            [0.0, 1.0]
        );
        controls.modified = true;
        controls.reset();
        assert_eq!(controls.key(KeyCode::KeyW, true, false).movement, [0.0; 2]);
    }
}
