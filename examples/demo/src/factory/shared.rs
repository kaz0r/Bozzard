//! A canonical shared world plus independent player records. The world always
//! stores Earth first and contains no player's backpack, tool choice or position.
//! Projecting it into the existing Rhai schema is an explicit local-view step.
use super::state::{State, number, numeric, values};
use anyhow::{Context, Result, ensure};
use bozzard_scene::blueprint::{BlackboardValue as B, Value};
use serde::{Deserialize, Serialize};

pub const MAX_SAVED_PLAYERS: usize = 64;
pub const PLAYER_COLORS: [[f32; 3]; 4] = [
    [0.12, 0.44, 1.0], // host: blue
    [0.95, 0.16, 0.18],
    [1.0, 0.49, 0.08],
    [0.18, 0.83, 0.34],
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stack {
    pub kind: u8,
    pub amount: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub planet: u8,
    /// World tile coordinates, independent of another player's current chunk.
    pub x: i16,
    pub z: i16,
}
impl Default for Position {
    fn default() -> Self {
        Self {
            planet: 0,
            x: 0,
            z: 2,
        }
    }
}
impl Position {
    pub fn validate(self) -> Result<()> {
        ensure!(self.planet < 2, "unknown planet");
        let edge = if self.planet == 0 { 127 } else { 97 };
        ensure!(
            (-edge..=edge).contains(&self.x) && (-edge..=edge).contains(&self.z),
            "player outside planet"
        );
        Ok(())
    }
    pub fn chunk(self) -> (i16, i16) {
        (
            ((i32::from(self.x) + 7).div_euclid(15)) as i16,
            ((i32::from(self.z) + 7).div_euclid(15)) as i16,
        )
    }
    pub fn cell(self) -> (i16, i16) {
        let (cx, cz) = self.chunk();
        (
            (i32::from(self.x) - i32::from(cx) * 15) as i16,
            (i32::from(self.z) - i32::from(cz) * 15) as i16,
        )
    }
    pub fn archive_index(self) -> usize {
        let (cx, cz) = self.chunk();
        self.planet as usize * 289 + (cz + 8) as usize * 17 + (cx + 8) as usize
    }
    pub fn adjacent(self, other: Self) -> bool {
        self.planet == other.planet
            && (i32::from(self.x) - i32::from(other.x)).abs() <= 1
            && (i32::from(self.z) - i32::from(other.z)).abs() <= 1
            && self != other
    }
    pub fn near(self, other: Self, tiles: i32) -> bool {
        self.planet == other.planet
            && (i32::from(self.x) - i32::from(other.x)).abs() <= tiles
            && (i32::from(self.z) - i32::from(other.z)).abs() <= tiles
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Player {
    pub position: Position,
    pub backpack: [Stack; 25],
    pub selected: u8,
    pub direction: u8,
    pub bar: u8,
    pub bar_slots: [u8; 3],
}
impl Default for Player {
    fn default() -> Self {
        Self {
            position: Position::default(),
            backpack: [Stack::default(); 25],
            selected: 3,
            direction: 0,
            bar: 1,
            bar_slots: [1; 3],
        }
    }
}
impl Player {
    pub fn validate(&self) -> Result<()> {
        self.position.validate()?;
        ensure!(
            (1..=11).contains(&self.selected)
                && self.direction < 4
                && self.bar <= 3
                && self.bar_slots.iter().all(|s| (1..=8).contains(s)),
            "invalid player tools"
        );
        ensure!(
            self.backpack
                .iter()
                .all(|s| s.kind < 32 && s.amount <= 100 && (s.kind > 0 || s.amount == 0)),
            "invalid player inventory"
        );
        Ok(())
    }
    pub fn capture(state: &State) -> Result<Self> {
        state.validate()?;
        let session = values(&state.controller, "session")?;
        let mut backpack = [Stack::default(); 25];
        for (slot, stack) in backpack.iter_mut().zip(session[64..114].chunks_exact(2)) {
            *slot = Stack {
                kind: numeric(&stack[0])? as u8,
                amount: numeric(&stack[1])? as u16,
            };
        }
        let mut bar_slots = [0; 3];
        for (slot, value) in bar_slots
            .iter_mut()
            .zip(values(&state.controller, "bar_slots")?)
        {
            *slot = numeric(value)? as u8;
        }
        let player = Self {
            position: Position {
                planet: numeric(&session[7])? as u8,
                x: (number(&state.scene, "chunk_x")? * 15. + number(&state.scene, "cursor_x")?)
                    as i16,
                z: (number(&state.scene, "chunk_z")? * 15. + number(&state.scene, "cursor_z")?)
                    as i16,
            },
            backpack,
            selected: number(&state.scene, "selected")? as u8,
            direction: number(&state.scene, "direction")? as u8,
            bar: number(&state.controller, "bar")? as u8,
            bar_slots,
        };
        player.validate()?;
        Ok(player)
    }
    fn apply(&self, state: &mut State) {
        let (cx, cz) = self.position.chunk();
        let (x, z) = self.position.cell();
        for (key, n) in [
            ("chunk_x", cx),
            ("chunk_z", cz),
            ("cursor_x", x),
            ("cursor_z", z),
            ("selected", self.selected as i16),
            ("direction", self.direction as i16),
        ] {
            state
                .scene
                .insert(key.into(), B::Scalar(Value::Number(n as f32)));
        }
        state
            .controller
            .insert("bar".into(), B::Scalar(Value::Number(self.bar as f32)));
        for (value, slot) in state
            .controller
            .get_mut("bar_slots")
            .unwrap()
            .values_mut()
            .iter_mut()
            .zip(self.bar_slots)
        {
            *value = Value::Number(slot as f32);
        }
        let session = state.controller.get_mut("session").unwrap().values_mut();
        session[7] = Value::Number(self.position.planet as f32);
        let mut stock = [0.; 32];
        for (i, stack) in self.backpack.iter().enumerate() {
            session[64 + 2 * i] = Value::Number(stack.kind as f32);
            session[65 + 2 * i] = Value::Number(stack.amount as f32);
            stock[stack.kind as usize] += stack.amount as f32;
        }
        for (value, amount) in state
            .controller
            .get_mut("stock")
            .unwrap()
            .values_mut()
            .iter_mut()
            .zip(stock)
        {
            *value = Value::Number(amount);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct World {
    state: State,
}
impl World {
    pub fn from_local(mut state: State) -> Result<(Self, Player)> {
        let player = Player::capture(&state)?;
        if player.position.planet == 1 {
            swap_planets(&mut state);
        }
        Player::default().apply(&mut state);
        let world = Self { state };
        world.validate()?;
        Ok((world, player))
    }
    pub fn validate(&self) -> Result<()> {
        self.state.validate()?;
        ensure!(
            Player::capture(&self.state)? == Player::default(),
            "shared world contains a private player view"
        );
        for (i, value) in values(&self.state.controller, "session")?
            .iter()
            .enumerate()
        {
            if i != 0 && !(7..40).contains(&i) && !(64..114).contains(&i) && i != 120 {
                ensure!(
                    numeric(value)? == 0.,
                    "shared world contains local interface state"
                );
            }
        }
        Ok(())
    }
    pub fn project(&self, player: &Player) -> Result<State> {
        self.validate()?;
        player.validate()?;
        ensure!(
            self.discovered(player.position)?,
            "player location has not been discovered by host"
        );
        let mut state = self.state.clone();
        if player.position.planet == 1 {
            swap_planets(&mut state);
        }
        player.apply(&mut state);
        state.validate()?;
        Ok(state)
    }
    pub fn discovered(&self, position: Position) -> Result<bool> {
        position.validate()?;
        let page = values(&self.state.controller, "chunk_nodes")?
            .get(position.archive_index())
            .context("missing discovery page")?;
        Ok(matches!(page, Value::Text(t) if !t.is_empty()))
    }
    pub fn phase(&self) -> u8 {
        number(&self.state.controller, "phase").expect("validated world") as u8
    }
    pub fn state(&self) -> &State {
        &self.state
    }
    pub(crate) fn from_canonical(state: State) -> Result<Self> {
        let world = Self { state };
        world.validate()?;
        Ok(world)
    }
}
fn swap_planets(state: &mut State) {
    let a = state.controller["power_data"].clone();
    let b = state.controller["power_other"].clone();
    state.controller.insert("power_data".into(), b);
    state.controller.insert("power_other".into(), a);
    let counts = state.scene.get_mut("counts").unwrap().values_mut();
    let session = state.controller.get_mut("session").unwrap().values_mut();
    for i in 0..32 {
        std::mem::swap(&mut counts[i], &mut session[8 + i]);
    }
}
