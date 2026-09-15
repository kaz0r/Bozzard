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
}

pub fn simulation_running(world: &bozzard_ecs::World) -> bool {
    world
        .resource::<GameSession>()
        .is_none_or(|s| s.phase == GamePhase::Playing)
}
