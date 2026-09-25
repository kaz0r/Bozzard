use super::*;
use bozzard_scene::middleware::ui::Input;
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct MenuInput {
    pointer: Option<[f32; 2]>,
    shift: bool,
    consumed: BTreeSet<KeyCode>,
}
impl MenuInput {
    pub(super) fn consumed(&self, key: KeyCode) -> bool {
        self.consumed.contains(&key)
    }
}
impl Player {
    pub(super) fn ui_input(&mut self, input: Input) -> Result<bool> {
        let size = self.view.as_ref().map_or([1280., 720.], |view| {
            let scale = view.window.scale_factor() as f32;
            [
                view.config.width as f32 / scale,
                view.config.height as f32 / scale,
            ]
        });
        let phase = self.demo.game_session().map(|s| s.phase);
        let consumed = self.demo.ui_input(self.options.layer, size, input)?;
        if phase != self.demo.game_session().map(|s| s.phase) {
            self.gameplay_controls.reset();
            self.last_frame = Instant::now();
            self.paused = false;
        }
        Ok(consumed)
    }
    pub(super) fn game_key(
        &mut self,
        physical: PhysicalKey,
        state: ElementState,
        repeat: bool,
        synthetic: bool,
    ) -> Result<bool> {
        let PhysicalKey::Code(code) = physical else {
            return Ok(false);
        };
        let pressed = state == ElementState::Pressed;
        if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
            self.menu_input.shift = pressed;
        }
        if !pressed {
            return Ok(self.menu_input.consumed.remove(&code)
                || (self.demo.game_session().is_some() && !self.demo.accepts_gameplay_input()));
        }
        if synthetic || repeat {
            return Ok(self.menu_input.consumed.contains(&code)
                || (self.demo.game_session().is_some() && !self.demo.accepts_gameplay_input()));
        }
        let name = if code == KeyCode::NumpadEnter {
            Some("Enter")
        } else {
            bozzard_scene::keys::canonical(&format!("{code:?}"))
        };
        let mut consumed = false;
        match code {
            KeyCode::PageDown => consumed = self.ui_input(Input::ScrollFocused(240.))?,
            KeyCode::PageUp => consumed = self.ui_input(Input::ScrollFocused(-240.))?,
            KeyCode::Tab => {
                consumed = self.ui_input(Input::FocusNext {
                    reverse: self.menu_input.shift,
                })?
            }
            KeyCode::Space | KeyCode::Enter | KeyCode::NumpadEnter => {
                consumed = self.ui_input(Input::Activate)?
            }
            KeyCode::ArrowLeft | KeyCode::ArrowDown => {
                consumed = self.ui_input(Input::Adjust(-1.))?
            }
            KeyCode::ArrowRight | KeyCode::ArrowUp => {
                consumed = self.ui_input(Input::Adjust(1.))?
            }
            _ => {}
        }
        if !consumed && let Some(name) = name {
            consumed = self.ui_input(Input::Key(name.into()))?;
        }
        consumed |= self.demo.game_session().is_some() && !self.demo.accepts_gameplay_input();
        if consumed {
            self.menu_input.consumed.insert(code);
        }
        Ok(consumed)
    }
    pub(super) fn game_pointer_event(&mut self, event: &WindowEvent) -> Result<bool> {
        match event {
            WindowEvent::Focused(false) => {
                self.menu_input = MenuInput::default();
                self.ui_input(Input::CancelPointer)?;
                if self
                    .demo
                    .game_session()
                    .is_some_and(|s| s.phase == bozzard_scene::GamePhase::Playing)
                {
                    self.ui_input(Input::Key("Escape".into()))?;
                }
                Ok(false)
            }
            WindowEvent::CursorLeft { .. } => {
                self.menu_input.pointer = None;
                self.ui_input(Input::CancelPointer)
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .view
                    .as_ref()
                    .map_or(1., |v| v.window.scale_factor() as f32);
                let point = [position.x as f32 / scale, position.y as f32 / scale];
                self.menu_input.pointer = Some(point);
                self.ui_input(Input::PointerMove(point))
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scale = self
                    .view
                    .as_ref()
                    .map_or(1., |v| v.window.scale_factor() as f32);
                let amount = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => -*y * 40.,
                    winit::event::MouseScrollDelta::PixelDelta(p) => -p.y as f32 / scale,
                };
                if let Some(point) = self.menu_input.pointer {
                    self.ui_input(Input::ScrollAt {
                        point,
                        delta: amount,
                    })
                } else {
                    Ok(false)
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: winit::event::MouseButton::Right,
                ..
            } => {
                if let Some(point) = self.menu_input.pointer {
                    self.ui_input(Input::SecondaryDown(point))
                } else {
                    Ok(false)
                }
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                if let Some(point) = self.menu_input.pointer {
                    self.ui_input(if *state == ElementState::Pressed {
                        Input::PointerDown(point)
                    } else {
                        Input::PointerUp(point)
                    })
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        }
    }
}
