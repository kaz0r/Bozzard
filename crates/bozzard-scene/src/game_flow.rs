//! Opt-in game lifecycle shared by the editor and standalone player.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameFlowSettings {
    pub title: String,
    pub instructions: String,
}
impl Default for GameFlowSettings {
    fn default() -> Self {
        Self {
            title: "My game".into(),
            instructions: "".into(),
        }
    }
}
impl GameFlowSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.title.trim().is_empty() && self.title.len() <= 120,
            "game title needs 1–120 UTF-8 bytes"
        );
        ensure!(
            self.instructions.len() <= 240,
            "game instructions exceed 240 UTF-8 bytes"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GamePhase {
    Ready,
    Playing,
    Paused,
    GameOver,
    Quit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameAction {
    Start,
    Pause,
    Resume,
    Restart,
    Quit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameKey {
    Enter,
    Escape,
    Restart,
    Quit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameSession {
    pub phase: GamePhase,
    pub message: String,
}
impl Default for GameSession {
    fn default() -> Self {
        Self {
            phase: GamePhase::Ready,
            message: String::new(),
        }
    }
}
impl GameSession {
    pub fn key_action(&self, key: GameKey) -> Option<GameAction> {
        use GameAction as A;
        use GameKey as K;
        use GamePhase as P;
        match (self.phase, key) {
            (P::Ready, K::Enter) => Some(A::Start),
            (P::Playing, K::Escape) => Some(A::Pause),
            (P::Paused, K::Escape | K::Enter) => Some(A::Resume),
            (P::GameOver, K::Enter) | (P::Playing | P::Paused | P::GameOver, K::Restart) => {
                Some(A::Restart)
            }
            (P::Ready | P::Paused | P::GameOver, K::Quit) => Some(A::Quit),
            _ => None,
        }
    }
    pub fn end_game(&mut self, message: &str) -> Result<()> {
        ensure!(
            message.len() <= 240,
            "game-over message exceeds 240 UTF-8 bytes"
        );
        if self.phase == GamePhase::Playing {
            self.message = message.into();
            self.phase = GamePhase::GameOver;
        }
        Ok(())
    }
    /// Button centers are normalized; offsets and hit areas use logical pixels.
    pub fn buttons(&self) -> Vec<GameButton> {
        use GameAction as A;
        use GamePhase as P;
        let entries: &[(&str, A)] = match self.phase {
            P::Ready => &[("Start game  [Enter]", A::Start), ("Quit  [Q]", A::Quit)],
            P::Paused => &[
                ("Resume  [Enter / Esc]", A::Resume),
                ("Restart  [R]", A::Restart),
                ("Quit  [Q]", A::Quit),
            ],
            P::GameOver => &[("Retry  [Enter / R]", A::Restart), ("Quit  [Q]", A::Quit)],
            P::Playing => {
                return vec![GameButton {
                    label: "Pause  [Esc]",
                    action: A::Pause,
                    anchor: [1., 0.],
                    offset: [-92., 34.],
                    width: 160.,
                }];
            }
            P::Quit => &[],
        };
        entries
            .iter()
            .enumerate()
            .map(|(i, (label, action))| GameButton {
                label,
                action: *action,
                anchor: [0.5, 0.5],
                offset: [0., 24. + i as f32 * 48.],
                width: 300.,
            })
            .collect()
    }
    pub fn hit(&self, size: [f32; 2], point: [f32; 2]) -> Option<GameAction> {
        self.buttons()
            .into_iter()
            .find(|button| button.contains(size, point))
            .map(|button| button.action)
    }
}

pub struct GameButton {
    pub label: &'static str,
    pub action: GameAction,
    pub anchor: [f32; 2],
    pub offset: [f32; 2],
    pub width: f32,
}
impl GameButton {
    pub fn contains(&self, size: [f32; 2], point: [f32; 2]) -> bool {
        let center = [
            size[0] * self.anchor[0] + self.offset[0],
            size[1] * self.anchor[1] + self.offset[1],
        ];
        (point[0] - center[0]).abs() <= self.width.min(size[0]) * 0.5
            && (point[1] - center[1]).abs() <= 20.
    }
}

pub fn simulation_running(world: &bozzard_ecs::World) -> bool {
    world
        .resource::<GameSession>()
        .is_none_or(|s| s.phase == GamePhase::Playing)
}
