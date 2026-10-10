//! Physical inputs an action can read: keys and mouse buttons, mouse and wheel deltas, and
//! gamepad buttons, sticks and triggers. Each reads one number from a tick's raw input.
use crate::GameplayInput;
use anyhow::{Context, Result};

/// Gamepad buttons by position, so South is A on Xbox and Cross on PlayStation. Bit `i` of
/// [`GamepadInput::buttons`] is `PadButton::ALL[i]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PadButton {
    South,
    East,
    West,
    North,
    LeftBumper,
    RightBumper,
    /// Analog: reads its travel in 0..1.
    LeftTrigger,
    /// Analog: reads its travel in 0..1.
    RightTrigger,
    Select,
    Start,
    Mode,
    LeftStick,
    RightStick,
    Up,
    Down,
    Left,
    Right,
}
impl PadButton {
    pub const ALL: [Self; 17] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::LeftBumper,
        Self::RightBumper,
        Self::LeftTrigger,
        Self::RightTrigger,
        Self::Select,
        Self::Start,
        Self::Mode,
        Self::LeftStick,
        Self::RightStick,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::South => "PadSouth",
            Self::East => "PadEast",
            Self::West => "PadWest",
            Self::North => "PadNorth",
            Self::LeftBumper => "PadLeftBumper",
            Self::RightBumper => "PadRightBumper",
            Self::LeftTrigger => "PadLeftTrigger",
            Self::RightTrigger => "PadRightTrigger",
            Self::Select => "PadSelect",
            Self::Start => "PadStart",
            Self::Mode => "PadMode",
            Self::LeftStick => "PadLeftStick",
            Self::RightStick => "PadRightStick",
            Self::Up => "PadUp",
            Self::Down => "PadDown",
            Self::Left => "PadLeft",
            Self::Right => "PadRight",
        }
    }
    pub fn bit(self) -> u32 {
        1 << self as u32
    }
    fn trigger(self) -> Option<usize> {
        match self {
            Self::LeftTrigger => Some(0),
            Self::RightTrigger => Some(1),
            _ => None,
        }
    }
}

/// Stick axes, each in −1..1 with y up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PadAxis {
    LeftX,
    LeftY,
    RightX,
    RightY,
}
impl PadAxis {
    pub const ALL: [Self; 4] = [Self::LeftX, Self::LeftY, Self::RightX, Self::RightY];
    pub fn name(self) -> &'static str {
        match self {
            Self::LeftX => "PadLeftX",
            Self::LeftY => "PadLeftY",
            Self::RightX => "PadRightX",
            Self::RightY => "PadRightY",
        }
    }
}

/// One gamepad's state for a tick: a level, plus presses queued since the previous tick so a
/// tap between ticks still registers. Hosts fill it from the first connected pad.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GamepadInput {
    pub connected: bool,
    /// Held buttons, bit `i` = `PadButton::ALL[i]`.
    pub buttons: u32,
    /// Buttons pressed since the previous tick, even if already released.
    pub pressed: u32,
    /// Stick positions in −1..1, y up: left x, left y, right x, right y.
    pub sticks: [f32; 4],
    /// Trigger travel in 0..1: left, right.
    pub triggers: [f32; 2],
}
impl GamepadInput {
    /// A button's value: triggers report their travel, digital buttons 0 or 1.
    pub fn button(&self, button: PadButton) -> f32 {
        let held = f32::from(u8::from(self.buttons & button.bit() != 0));
        match button.trigger() {
            // Pads without analog triggers only report the button.
            Some(index) if self.triggers[index] > 0. => self.triggers[index],
            _ => held,
        }
    }
    pub fn axis(&self, axis: PadAxis) -> f32 {
        self.sticks[axis as usize]
    }
    /// Non-finite or out-of-range values from a driver become neutral or clamped.
    pub fn sanitized(mut self) -> Self {
        let valid = (1u32 << PadButton::ALL.len()) - 1;
        self.buttons &= valid;
        self.pressed &= valid;
        for value in &mut self.sticks {
            *value = if value.is_finite() {
                value.clamp(-1., 1.)
            } else {
                0.
            };
        }
        for value in &mut self.triggers {
            *value = if value.is_finite() {
                value.clamp(0., 1.)
            } else {
                0.
            };
        }
        if !self.connected {
            return Self::default();
        }
        self
    }
    /// At rest: no button down and every stick and trigger near its center.
    pub fn idle(&self) -> bool {
        self.buttons == 0
            && self.pressed == 0
            && self.sticks.iter().all(|v| v.abs() < REST)
            && self.triggers.iter().all(|v| *v < REST)
    }
}
/// Below this a stick or trigger counts as released for capture and rest checks.
const REST: f32 = 0.3;

/// A signed axis: mouse or wheel deltas, or a gamepad stick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Axis {
    /// Look delta in logical points per tick, right positive.
    MouseX,
    /// Look delta in logical points per tick, up positive.
    MouseY,
    /// Wheel lines per tick, right positive.
    WheelX,
    /// Wheel lines per tick, away from the player positive.
    WheelY,
    Pad(PadAxis),
}
impl Axis {
    const MOUSE: [(&'static str, Self); 4] = [
        ("MouseX", Self::MouseX),
        ("MouseY", Self::MouseY),
        ("WheelX", Self::WheelX),
        ("WheelY", Self::WheelY),
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Pad(axis) => axis.name(),
            _ => Self::MOUSE.iter().find(|(_, a)| *a == self).unwrap().0,
        }
    }
    fn parse(name: &str) -> Option<Self> {
        Self::MOUSE
            .iter()
            .map(|(n, a)| (*n, *a))
            .chain(PadAxis::ALL.map(|a| (a.name(), Self::Pad(a))))
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, a)| a)
    }
    fn read(self, input: &GameplayInput) -> f32 {
        match self {
            Self::MouseX => input.orbit[0],
            // Screen deltas grow downwards; actions keep y up like sticks.
            Self::MouseY => -input.orbit[1],
            Self::WheelX => input.wheel[0],
            Self::WheelY => input.wheel[1],
            Self::Pad(axis) => input.pad.axis(axis),
        }
    }
    fn pad(self) -> bool {
        matches!(self, Self::Pad(_))
    }
}

/// One physical input. Names are what scene files, settings and scripts write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A key or mouse button, by index into [`crate::keys::BOUND_KEYS`].
    Key(u8),
    Pad(PadButton),
    Axis(Axis),
    /// One direction of an axis, read as 0..: `PadLeftX+`, `WheelY-`.
    Half(Axis, bool),
}
impl Source {
    /// Accepts any key name `On Input Pressed` accepts except the seven aliases, gamepad
    /// buttons (`PadSouth`), axes (`PadLeftX`, `MouseX`, `WheelY`) and half axes (`PadLeftX+`).
    pub fn parse(name: &str) -> Result<Self> {
        let trimmed = name.trim();
        let unknown = || {
            format!(
                "unknown input '{name}'; use a key or mouse button (W, Space, MouseLeft), a \
                 gamepad button (PadSouth, PadRightTrigger), an axis (PadLeftX, MouseX, \
                 WheelY) or a half axis (PadLeftX+, WheelY-)"
            )
        };
        if let Some((rest, positive)) = trimmed
            .strip_suffix('+')
            .map(|rest| (rest, true))
            .or_else(|| trimmed.strip_suffix('-').map(|rest| (rest, false)))
        {
            return Axis::parse(rest)
                .map(|axis| Self::Half(axis, positive))
                .with_context(unknown);
        }
        if let Some(axis) = Axis::parse(trimmed) {
            return Ok(Self::Axis(axis));
        }
        if let Some(button) = PadButton::ALL
            .into_iter()
            .find(|b| b.name().eq_ignore_ascii_case(trimmed))
        {
            return Ok(Self::Pad(button));
        }
        crate::keys::canonical(trimmed)
            .filter(|key| crate::keys::alias_index(key).is_none())
            .and_then(|key| crate::keys::BOUND_KEYS.iter().position(|k| *k == key))
            .map(|index| Self::Key(index as u8))
            .with_context(unknown)
    }
    pub fn name(&self) -> String {
        match self {
            Self::Key(index) => crate::keys::name(usize::from(*index)).to_owned(),
            Self::Pad(button) => button.name().to_owned(),
            Self::Axis(axis) => axis.name().to_owned(),
            Self::Half(axis, positive) => {
                format!("{}{}", axis.name(), if *positive { '+' } else { '-' })
            }
        }
    }
    /// Every source name, for editor pickers: keys, gamepad buttons, then axes.
    pub fn authorable() -> Vec<String> {
        let axes: Vec<_> = Axis::MOUSE
            .map(|(_, a)| a)
            .into_iter()
            .chain(PadAxis::ALL.map(Axis::Pad))
            .collect();
        (0..crate::keys::BOUND_KEYS.len())
            .map(|i| Self::Key(i as u8))
            .chain(PadButton::ALL.map(Self::Pad))
            .chain(axes.iter().map(|a| Self::Axis(*a)))
            .chain(
                axes.iter()
                    .flat_map(|a| [Self::Half(*a, true), Self::Half(*a, false)]),
            )
            .map(|s| s.name())
            .collect()
    }
    /// The raw value: 0 or 1 for buttons, trigger travel, signed axis position or delta.
    pub fn read(&self, input: &GameplayInput) -> f32 {
        match *self {
            Self::Key(index) => f32::from(u8::from(input.keys & (1 << index) != 0)),
            Self::Pad(button) => input.pad.button(button),
            Self::Axis(axis) => axis.read(input),
            Self::Half(axis, positive) => {
                let value = axis.read(input);
                (if positive { value } else { -value }).max(0.)
            }
        }
    }
    /// Pressed since the previous tick, even if released again before it.
    pub fn tapped(&self, input: &GameplayInput) -> bool {
        match *self {
            Self::Key(index) => input.pressed_keys & (1 << index) != 0,
            Self::Pad(button) => input.pad.pressed & button.bit() != 0,
            _ => false,
        }
    }
    /// Gamepad sticks and triggers rest near, not at, zero: dead zones apply to them.
    pub fn analog(&self) -> bool {
        match self {
            Self::Pad(button) => button.trigger().is_some(),
            Self::Axis(axis) | Self::Half(axis, _) => axis.pad(),
            Self::Key(_) => false,
        }
    }
    /// Reads 0..1 like a button: keys, gamepad buttons and half axes.
    pub fn button_like(&self) -> bool {
        !matches!(self, Self::Axis(_))
    }
    /// Mouse and wheel deltas have no upper bound.
    pub fn bounded(&self) -> bool {
        match self {
            Self::Axis(axis) | Self::Half(axis, _) => axis.pad(),
            _ => true,
        }
    }
}
impl serde::Serialize for Source {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.name())
    }
}
impl<'de> serde::Deserialize<'de> for Source {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::parse(&name).map_err(serde::de::Error::custom)
    }
}
