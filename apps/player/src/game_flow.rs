use super::*;
use bozzard_scene::{GameAction, GameKey, GamePhase};

#[derive(Default)]
pub(super) struct MenuInput {
    pointer: Option<[f32; 2]>,
    pressed: Option<GameAction>,
}
impl Player {
    pub(super) fn game_action(&mut self, action: GameAction) -> Result<()> {
        self.demo.game_action(action)?;
        self.gameplay_controls.reset();
        self.menu_input.pressed = None;
        self.last_frame = Instant::now();
        self.paused = false;
        Ok(())
    }
    pub(super) fn game_key(
        &mut self,
        physical: PhysicalKey,
        state: ElementState,
        repeat: bool,
        synthetic: bool,
    ) -> Result<bool> {
        let Some(session) = self.demo.game_session() else {
            return Ok(false);
        };
        let key = match physical {
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => Some(GameKey::Enter),
            PhysicalKey::Code(KeyCode::Escape) => Some(GameKey::Escape),
            PhysicalKey::Code(KeyCode::KeyR) => Some(GameKey::Restart),
            PhysicalKey::Code(KeyCode::KeyQ) => Some(GameKey::Quit),
            _ => None,
        };
        if let Some(key) = key {
            if state == ElementState::Pressed
                && !repeat
                && !synthetic
                && let Some(action) = session.key_action(key)
            {
                self.game_action(action)?;
            }
            return Ok(true);
        }
        // Everything else is gameplay's while it is playing: a scene assigns its own buttons,
        // so only the app's menu keys above stay out of the input adapter.
        Ok(!self.demo.accepts_gameplay_input() || session.phase != GamePhase::Playing)
    }
    pub(super) fn game_pointer_event(&mut self, event: &WindowEvent) -> Result<()> {
        if self.demo.game_session().is_none() {
            return Ok(());
        }
        match event {
            WindowEvent::Focused(false) => {
                self.menu_input = MenuInput::default();
                self.game_action(GameAction::Pause)?;
            }
            WindowEvent::CursorLeft { .. } => self.menu_input = MenuInput::default(),
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.view.as_ref().unwrap().window.scale_factor() as f32;
                self.menu_input.pointer =
                    Some([position.x as f32 / scale, position.y as f32 / scale]);
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                let view = self.view.as_ref().unwrap();
                let scale = view.window.scale_factor() as f32;
                let size = [
                    view.config.width as f32 / scale,
                    view.config.height as f32 / scale,
                ];
                let hit = self
                    .menu_input
                    .pointer
                    .and_then(|point| self.demo.game_session().unwrap().hit(size, point));
                if *state == ElementState::Pressed {
                    self.menu_input.pressed = hit;
                } else if let Some(action) =
                    self.menu_input.pressed.take().filter(|a| Some(*a) == hit)
                {
                    self.game_action(action)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
