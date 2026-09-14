//! Window-local input adapter. Gameplay uses physical WASD, never selection or OS repeats.
use bozzard_scene::{GameplayInput, keys};
use winit::{
    event::{ElementState, MouseButton, WindowEvent},
    keyboard::KeyCode,
};

/// How the camera follows the mouse. The native player locks the pointer and falls back to
/// plain pointer motion when a platform refuses the grab; right-drag always also works.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
    #[default]
    Off,
    /// Pointer motion turns the view with no button held.
    Cursor,
    /// Locked, hidden pointer: raw device motion turns the view.
    Locked,
}

pub struct GameplayControls {
    // Physical keys currently held, in the engine's binding bits, so a scene can bind any of
    // them; W/A/S/D are read back out for movement.
    keys: u128,
    focused: bool,
    modified: bool,
    orbiting: bool,
    look: Look,
    cursor: Option<[f64; 2]>,
    scale_factor: f64,
}
impl Default for GameplayControls {
    fn default() -> Self {
        Self {
            keys: 0,
            focused: false,
            modified: false,
            orbiting: false,
            look: Look::Off,
            cursor: None,
            scale_factor: 1.0,
        }
    }
}
impl GameplayControls {
    /// Only the application knows whether it owns the pointer; deltas are relative,
    /// so a mode change restarts the cursor sample.
    pub fn set_mouse_look(&mut self, look: Look) {
        if self.look != look {
            self.look = look;
            self.cursor = None;
        }
    }
    pub fn focused(&self) -> bool {
        self.focused
    }
    /// Raw pointer motion (locked look only): device deltas, not logical cursor points.
    pub fn motion(&mut self, delta: [f64; 2]) -> Option<GameplayInput> {
        (self.look == Look::Locked && self.focused).then(|| GameplayInput {
            orbit: delta.map(|value| value as f32),
            ..self.current()
        })
    }
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
            && (self.orbiting || self.look == Look::Cursor)
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
        self.keys = 0;
        self.orbiting = false;
        self.cursor = None;
    }
    /// Records a physical key's held state. Names the engine does not bind are ignored.
    fn hold(&mut self, requested: &str, pressed: bool) {
        let bit = keys::bit(requested);
        if pressed {
            self.keys |= bit;
        } else {
            self.keys &= !bit;
        }
    }
    pub fn key(&mut self, code: KeyCode, pressed: bool, repeat: bool) -> GameplayInput {
        if !self.focused || self.modified {
            return GameplayInput::default();
        }
        if !repeat {
            // Key names are physical positions, so a non-QWERTY layout still binds the
            // button the scene asked for.
            self.hold(&format!("{code:?}"), pressed);
        }
        GameplayInput {
            jump: code == KeyCode::Space && pressed && !repeat,
            interact: code == KeyCode::KeyE && pressed && !repeat,
            ..self.current()
        }
    }
    pub fn current(&self) -> GameplayInput {
        let down = |name: &str| u8::from(self.keys & keys::bit(name) != 0) as f32;
        GameplayInput {
            movement: [down("D") - down("A"), down("W") - down("S")],
            keys: self.keys,
            ..Default::default()
        }
    }
    /// None explicitly cancels pending edges on focus/cursor loss.
    /// RMB drag is deliberately unconfined: no hidden/locked pointer in gameplay.
    pub fn event(&mut self, event: &WindowEvent) -> Option<GameplayInput> {
        // Every button a scene may bind is tracked, including the mouse.
        if let WindowEvent::MouseInput {
            state,
            button: button @ (MouseButton::Left | MouseButton::Right | MouseButton::Middle),
            ..
        } = event
            && self.focused
            && !self.modified
        {
            let name = match button {
                MouseButton::Middle => "MouseMiddle",
                MouseButton::Right => "MouseRight",
                _ => "MouseLeft",
            };
            self.hold(name, *state == ElementState::Pressed);
        }
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
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if self.focused && !self.modified => {
                return Some(GameplayInput {
                    fire: true,
                    ..self.current()
                });
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
    fn locked_look_uses_raw_motion_and_confined_look_needs_no_button() {
        use winit::{dpi::PhysicalPosition, event::DeviceId};
        let moved = |position: [f64; 2]| WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(position[0], position[1]),
        };
        let mut controls = GameplayControls::default();
        controls.event(&WindowEvent::Focused(true));
        // Off: pointer motion alone never turns the view.
        assert!(controls.motion([30.0, 10.0]).is_none());
        controls.event(&moved([100.0, 100.0]));
        assert_eq!(
            controls.event(&moved([140.0, 90.0])).unwrap().orbit,
            [0.0; 2]
        );
        // Cursor look: plain motion turns the view, still no button.
        controls.set_mouse_look(Look::Cursor);
        controls.event(&moved([200.0, 200.0]));
        let sample = controls.event(&moved([230.0, 190.0])).unwrap();
        assert_eq!(sample.orbit, [30.0, -10.0]);
        assert!(!sample.fire);
        // Locked: the cursor does not move, so raw device motion drives the view.
        controls.set_mouse_look(Look::Locked);
        controls.event(&moved([400.0, 400.0]));
        assert_eq!(
            controls.event(&moved([500.0, 480.0])).unwrap().orbit,
            [0.0; 2],
            "a locked pointer cannot also be read as cursor points"
        );
        let sample = controls.motion([12.0, -7.0]).unwrap();
        assert_eq!(sample.orbit, [12.0, -7.0]);
        assert_eq!(controls.motion([1.0, 1.0]).unwrap().movement, [0.0; 2]);
        assert!(!sample.jump && !sample.fire);
        // Focus loss and mode changes stop the view turning.
        controls.event(&WindowEvent::Focused(false));
        assert!(controls.motion([50.0, 0.0]).is_none());
        controls.set_mouse_look(Look::Off);
        assert!(controls.motion([50.0, 0.0]).is_none());
    }

    #[test]
    fn left_click_is_a_fire_edge_and_right_drag_only_orbits() {
        use winit::{dpi::PhysicalPosition, event::DeviceId};
        let mouse = |button, state| WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button,
        };
        let moved = |position: [f64; 2]| WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(position[0], position[1]),
        };
        let mut controls = GameplayControls::default();
        // Unfocused clicks and right-clicks never fire.
        assert!(
            !controls
                .event(&mouse(MouseButton::Left, ElementState::Pressed))
                .unwrap()
                .fire
        );
        controls.event(&WindowEvent::Focused(true));
        assert!(
            controls
                .event(&mouse(MouseButton::Left, ElementState::Pressed))
                .unwrap()
                .fire
        );
        assert!(
            !controls
                .event(&mouse(MouseButton::Left, ElementState::Released))
                .unwrap()
                .fire,
            "fire is a press edge, not a held state"
        );
        assert!(
            !controls
                .event(&mouse(MouseButton::Right, ElementState::Pressed))
                .unwrap()
                .fire
        );
        controls.event(&moved([10.0, 10.0]));
        let drag = controls.event(&moved([40.0, 4.0])).unwrap();
        assert_eq!(
            drag.orbit,
            [30.0, -6.0],
            "right-drag still orbits while aiming"
        );
        assert!(!drag.fire);
        controls.event(&WindowEvent::Focused(false));
        assert!(
            !controls
                .event(&mouse(MouseButton::Left, ElementState::Pressed))
                .unwrap()
                .fire
        );
    }

    #[test]
    fn any_bound_button_is_tracked_so_a_scene_can_assign_it() {
        let mut controls = GameplayControls::default();
        controls.event(&WindowEvent::Focused(true));
        let down = |controls: &GameplayControls, name: &str| {
            controls.current().keys & bozzard_scene::keys::bit(name) != 0
        };
        assert!(controls.key(KeyCode::KeyF, true, false).keys == bozzard_scene::keys::bit("F"));
        assert!(down(&controls, "F") && !down(&controls, "G"));
        // A repeat leaves the held button alone; a release clears it again.
        assert!(down(&controls, "F"));
        assert!(controls.key(KeyCode::KeyF, true, true).keys & bozzard_scene::keys::bit("F") != 0);
        controls.key(KeyCode::KeyF, false, false);
        assert!(!down(&controls, "F"));
        // Digits, arrows and mouse buttons resolve to their own bits.
        controls.key(KeyCode::Digit3, true, false);
        controls.key(KeyCode::ArrowLeft, true, false);
        let held = controls.current().keys;
        assert!(held & bozzard_scene::keys::bit("3") != 0);
        assert!(held & bozzard_scene::keys::bit("ArrowLeft") != 0);
        controls.event(&WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Middle,
        });
        assert!(down(&controls, "MouseMiddle"));
        // Movement still comes from the physical W/A/S/D buttons.
        assert_eq!(
            controls.key(KeyCode::KeyW, true, false).movement,
            [0.0, 1.0]
        );
        assert_eq!(
            controls.key(KeyCode::KeyD, true, false).movement,
            [1.0, 1.0]
        );
        // Focus loss clears every binding, not just movement.
        assert!(controls.event(&WindowEvent::Focused(false)).is_none());
        assert_eq!(controls.current().keys, 0);
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
