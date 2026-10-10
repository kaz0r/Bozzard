//! How sources combine into an action's value: a single input, two buttons as an axis, four
//! buttons as a vector, or two axes as a stick.
use super::source::Source;
use crate::GameplayInput;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

/// Default dead zone for gamepad sticks and triggers.
pub const DEAD_ZONE: f32 = 0.15;
/// An action is held while its value's magnitude reaches this.
pub const PRESS_POINT: f32 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub enum Binding {
    /// One input. Dead zone applies to gamepad sticks and triggers.
    Input {
        input: Source,
        dead_zone: f32,
        invert: bool,
        scale: f32,
    },
    /// Two button-like inputs as one axis: positive minus negative.
    Axis { negative: Source, positive: Source },
    /// Four button-like inputs as a vector, clamped to unit length.
    Composite {
        up: Source,
        down: Source,
        left: Source,
        right: Source,
    },
    /// Two axes as a vector with a radial dead zone; gamepad sticks clamp to unit length.
    Stick {
        x: Source,
        y: Source,
        dead_zone: f32,
        invert_x: bool,
        invert_y: bool,
        scale: f32,
    },
}

/// A named part of a combined binding, addressed by rebinding as `action/part`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Negative,
    Positive,
    Up,
    Down,
    Left,
    Right,
    X,
    Y,
}
impl Part {
    const NAMES: [(&'static str, Self); 8] = [
        ("negative", Self::Negative),
        ("positive", Self::Positive),
        ("up", Self::Up),
        ("down", Self::Down),
        ("left", Self::Left),
        ("right", Self::Right),
        ("x", Self::X),
        ("y", Self::Y),
    ];
    pub fn parse(name: &str) -> Result<Self> {
        Self::NAMES
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
            .map(|(_, part)| *part)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown binding part '{name}'; use negative, positive, up, down, left, \
                     right, x or y"
                )
            })
    }
    pub fn name(self) -> &'static str {
        Self::NAMES.iter().find(|(_, p)| *p == self).unwrap().0
    }
    /// Stick parts take an axis; every other part takes a button-like input.
    pub fn takes_axis(self) -> bool {
        matches!(self, Self::X | Self::Y)
    }
}

impl Binding {
    pub fn input(input: Source) -> Self {
        Self::Input {
            input,
            dead_zone: DEAD_ZONE,
            invert: false,
            scale: 1.,
        }
    }
    pub fn stick(x: Source, y: Source) -> Self {
        Self::Stick {
            x,
            y,
            dead_zone: DEAD_ZONE,
            invert_x: false,
            invert_y: false,
            scale: 1.,
        }
    }
    /// Shape-only checks; whether it suits an action depends on the action's kind.
    pub fn validate(&self) -> Result<()> {
        let check = |dead_zone: f32, scale: f32| -> Result<()> {
            ensure!(
                dead_zone.is_finite() && (0.0..=0.95).contains(&dead_zone),
                "dead zone must be within 0–0.95"
            );
            ensure!(
                scale.is_finite() && scale != 0. && scale.abs() <= 1000.,
                "binding scale must be finite, nonzero and within ±1000"
            );
            Ok(())
        };
        match self {
            Self::Input {
                dead_zone, scale, ..
            } => check(*dead_zone, *scale)?,
            Self::Axis { .. } | Self::Composite { .. } => {
                for source in self.sources() {
                    ensure!(
                        source.button_like(),
                        "'{}' is a full axis; button pairs and composites take keys, \
                         buttons or half axes such as {}+",
                        source.name(),
                        source.name()
                    );
                }
            }
            Self::Stick {
                x,
                y,
                dead_zone,
                scale,
                ..
            } => {
                for source in [x, y] {
                    ensure!(
                        !source.button_like(),
                        "'{}' is not an axis; stick bindings take axes such as PadLeftX or MouseX",
                        source.name()
                    );
                }
                check(*dead_zone, *scale)?;
            }
        }
        Ok(())
    }
    pub fn sources(&self) -> Vec<Source> {
        match *self {
            Self::Input { input, .. } => vec![input],
            Self::Axis { negative, positive } => vec![negative, positive],
            Self::Composite {
                up,
                down,
                left,
                right,
            } => vec![up, down, left, right],
            Self::Stick { x, y, .. } => vec![x, y],
        }
    }
    /// The parts a rebind may address, in display order.
    pub fn parts(&self) -> &'static [Part] {
        match self {
            Self::Input { .. } => &[],
            Self::Axis { .. } => &[Part::Negative, Part::Positive],
            Self::Composite { .. } => &[Part::Up, Part::Down, Part::Left, Part::Right],
            Self::Stick { .. } => &[Part::X, Part::Y],
        }
    }
    pub fn part_mut(&mut self, part: Part) -> Option<&mut Source> {
        match (self, part) {
            (Self::Axis { negative, .. }, Part::Negative) => Some(negative),
            (Self::Axis { positive, .. }, Part::Positive) => Some(positive),
            (Self::Composite { up, .. }, Part::Up) => Some(up),
            (Self::Composite { down, .. }, Part::Down) => Some(down),
            (Self::Composite { left, .. }, Part::Left) => Some(left),
            (Self::Composite { right, .. }, Part::Right) => Some(right),
            (Self::Stick { x, .. }, Part::X) => Some(x),
            (Self::Stick { y, .. }, Part::Y) => Some(y),
            _ => None,
        }
    }
    /// A short label for menus: `Space`, `S/W`, `W/A/S/D`, `PadLeftX/PadLeftY`.
    pub fn name(&self) -> String {
        match self {
            Self::Input { input, invert, .. } => {
                format!("{}{}", if *invert { "-" } else { "" }, input.name())
            }
            Self::Composite {
                up,
                down,
                left,
                right,
            } => [up, left, down, right]
                .map(Source::name)
                .join("/"),
            _ => self
                .sources()
                .iter()
                .map(Source::name)
                .collect::<Vec<_>>()
                .join("/"),
        }
    }
    /// This tick's value; scalar bindings fill only x.
    pub fn read(&self, input: &GameplayInput) -> [f32; 2] {
        match *self {
            Self::Input {
                input: source,
                dead_zone,
                invert,
                scale,
            } => {
                let mut value = source.read(input);
                if source.analog() {
                    value = axial(value, dead_zone);
                }
                [if invert { -value } else { value } * scale, 0.]
            }
            Self::Axis { negative, positive } => [press(&positive, input) - press(&negative, input), 0.],
            Self::Composite {
                up,
                down,
                left,
                right,
            } => unit([
                press(&right, input) - press(&left, input),
                press(&up, input) - press(&down, input),
            ]),
            Self::Stick {
                x,
                y,
                dead_zone,
                invert_x,
                invert_y,
                scale,
            } => {
                let mut value = [x.read(input), y.read(input)];
                if x.analog() && y.analog() {
                    value = radial(value, dead_zone);
                } else if x.analog() || y.analog() {
                    value = [axial(value[0], dead_zone), axial(value[1], dead_zone)];
                }
                if x.bounded() && y.bounded() {
                    value = unit(value);
                }
                [
                    if invert_x { -value[0] } else { value[0] } * scale,
                    if invert_y { -value[1] } else { value[1] } * scale,
                ]
            }
        }
    }
    /// Any of its buttons pressed since the previous tick.
    pub fn tapped(&self, input: &GameplayInput) -> bool {
        self.sources().iter().any(|source| source.tapped(input))
    }
}

/// A button-like part in 0..1, with the default dead zone for analog halves.
fn press(source: &Source, input: &GameplayInput) -> f32 {
    let value = source.read(input);
    (if source.analog() {
        axial(value, DEAD_ZONE)
    } else {
        value
    })
    .clamp(0., 1.)
}
/// Rescale so output starts at zero at the dead zone's edge and reaches 1 at full travel.
pub fn axial(value: f32, dead_zone: f32) -> f32 {
    let magnitude = value.abs();
    if magnitude <= dead_zone {
        return 0.;
    }
    value.signum() * ((magnitude - dead_zone) / (1. - dead_zone)).min(1.)
}
/// The stick's direction is kept; only its length passes through the dead zone.
pub fn radial(value: [f32; 2], dead_zone: f32) -> [f32; 2] {
    let length = value[0].hypot(value[1]);
    if length <= dead_zone {
        return [0.; 2];
    }
    let scaled = ((length - dead_zone) / (1. - dead_zone)).min(1.);
    [value[0] / length * scaled, value[1] / length * scaled]
}
fn unit(value: [f32; 2]) -> [f32; 2] {
    let length = value[0].hypot(value[1]);
    if length > 1. {
        [value[0] / length, value[1] / length]
    } else {
        value
    }
}

/// The file form: which fields are present selects the binding's shape.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredBinding {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    negative: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    positive: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    up: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    down: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    left: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    right: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    y: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dead_zone: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    invert: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    invert_x: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    invert_y: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<f32>,
}
impl TryFrom<StoredBinding> for Binding {
    type Error = anyhow::Error;
    fn try_from(s: StoredBinding) -> Result<Self> {
        let shapes = [
            s.input.is_some(),
            s.negative.is_some() || s.positive.is_some(),
            s.up.is_some() || s.down.is_some() || s.left.is_some() || s.right.is_some(),
            s.x.is_some() || s.y.is_some(),
        ];
        ensure!(
            shapes.iter().filter(|shape| **shape).count() == 1,
            "a binding needs exactly one of `input`, `negative`+`positive`, \
             `up`+`down`+`left`+`right` or `x`+`y`"
        );
        let binding = if let Some(input) = s.input {
            ensure!(
                s.invert_x.is_none() && s.invert_y.is_none(),
                "`invert_x`/`invert_y` belong to x/y stick bindings; use `invert`"
            );
            Self::Input {
                input,
                dead_zone: s.dead_zone.unwrap_or(DEAD_ZONE),
                invert: s.invert.unwrap_or(false),
                scale: s.scale.unwrap_or(1.),
            }
        } else if shapes[3] {
            let (Some(x), Some(y)) = (s.x, s.y) else {
                bail!("a stick binding needs both `x` and `y`");
            };
            ensure!(
                s.invert.is_none(),
                "stick bindings use `invert_x`/`invert_y`, not `invert`"
            );
            Self::Stick {
                x,
                y,
                dead_zone: s.dead_zone.unwrap_or(DEAD_ZONE),
                invert_x: s.invert_x.unwrap_or(false),
                invert_y: s.invert_y.unwrap_or(false),
                scale: s.scale.unwrap_or(1.),
            }
        } else {
            ensure!(
                s.dead_zone.is_none()
                    && s.invert.is_none()
                    && s.invert_x.is_none()
                    && s.invert_y.is_none()
                    && s.scale.is_none(),
                "button pairs and composites take no dead zone, inversion or scale; swap \
                 their inputs instead"
            );
            if shapes[1] {
                let (Some(negative), Some(positive)) = (s.negative, s.positive) else {
                    bail!("an axis binding needs both `negative` and `positive`");
                };
                Self::Axis { negative, positive }
            } else {
                let (Some(up), Some(down), Some(left), Some(right)) = (s.up, s.down, s.left, s.right)
                else {
                    bail!("a composite binding needs `up`, `down`, `left` and `right`");
                };
                Self::Composite {
                    up,
                    down,
                    left,
                    right,
                }
            }
        };
        binding.validate()?;
        Ok(binding)
    }
}
impl From<&Binding> for StoredBinding {
    fn from(binding: &Binding) -> Self {
        let dead_zone = |value: f32| (value != DEAD_ZONE).then_some(value);
        let flag = |value: bool| value.then_some(true);
        let scale = |value: f32| (value != 1.).then_some(value);
        match *binding {
            Binding::Input {
                input,
                dead_zone: zone,
                invert,
                scale: factor,
            } => Self {
                input: Some(input),
                // Keys ignore the dead zone, so never write one for them.
                dead_zone: if input.analog() { dead_zone(zone) } else { None },
                invert: flag(invert),
                scale: scale(factor),
                ..Self::default()
            },
            Binding::Axis { negative, positive } => Self {
                negative: Some(negative),
                positive: Some(positive),
                ..Self::default()
            },
            Binding::Composite {
                up,
                down,
                left,
                right,
            } => Self {
                up: Some(up),
                down: Some(down),
                left: Some(left),
                right: Some(right),
                ..Self::default()
            },
            Binding::Stick {
                x,
                y,
                dead_zone: zone,
                invert_x,
                invert_y,
                scale: factor,
            } => Self {
                x: Some(x),
                y: Some(y),
                dead_zone: dead_zone(zone),
                invert_x: flag(invert_x),
                invert_y: flag(invert_y),
                scale: scale(factor),
                ..Self::default()
            },
        }
    }
}
impl Serialize for Binding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StoredBinding::from(self).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Binding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        StoredBinding::deserialize(deserializer)
            .and_then(|stored| Self::try_from(stored).map_err(serde::de::Error::custom))
    }
}
