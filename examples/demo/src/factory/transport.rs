//! Presentation records from the authoritative Rhai transport pass. These are
//! transient, bounded by the finite world, and never execute guest production.
use super::{shared::Position, state};
use anyhow::{Context, Result, ensure};
use bozzard_scene::{BlueprintRuntime, blueprint::Value};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemMotion {
    pub planet: u8,
    pub from: u32,
    pub to: u32,
    pub kind: u8,
    pub source: u8,
    pub target: u8,
}
impl ItemMotion {
    pub fn position(&self, id: u32) -> Result<Position> {
        ensure!(id < 289 * 225, "invalid transport address");
        let region = (id / 225) as i16;
        let cell = (id % 225) as i16;
        let position = Position {
            planet: self.planet,
            x: (region % 17 - 8) * 15 + cell % 15 - 7,
            z: (region / 17 - 8) * 15 + cell / 15 - 7,
        };
        position.validate()?;
        Ok(position)
    }
    pub fn validate(&self) -> Result<()> {
        let a = self.position(self.from)?;
        let b = self.position(self.to)?;
        ensure!(
            (a.x - b.x).abs() + (a.z - b.z).abs() == 1
                && (1..=26).contains(&self.kind)
                && [1, 2, 3, 4, 5, 7, 8, 11].contains(&self.source)
                && [2, 3, 4, 5, 7, 8, 11].contains(&self.target),
            "invalid item transfer"
        );
        Ok(())
    }
}

pub fn capture(runtime: &BlueprintRuntime, tick: f32) -> Result<Vec<ItemMotion>> {
    let board = runtime
        .object_blackboard("factory-transports")
        .context("missing transport presentation")?;
    let beats = state::values(board, "beats")?;
    let mut result = Vec::new();
    for (planet, name) in ["earth", "moon"].into_iter().enumerate() {
        if state::numeric(&beats[planet])? != tick {
            continue;
        }
        for (page, encoded) in state::values(board, name)?.iter().enumerate() {
            let Value::Text(encoded) = encoded else {
                anyhow::bail!("invalid transport page");
            };
            for (count, entry) in encoded.split(';').filter(|s| !s.is_empty()).enumerate() {
                let fields = entry
                    .split(',')
                    .map(str::parse::<u32>)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ensure!(
                    fields.len() == 5 && fields[0] as usize / 75 == page && count < 75,
                    "invalid transport page entry"
                );
                let motion = ItemMotion {
                    planet: planet as u8,
                    from: fields[0],
                    to: fields[1],
                    kind: fields[2].try_into()?,
                    source: fields[3].try_into()?,
                    target: fields[4].try_into()?,
                };
                motion.validate()?;
                result.push(motion);
            }
        }
    }
    Ok(result)
}
