//! Data-only factory snapshot shared by persistence and session synchronization.
//! Rendering objects, scripts, asset paths and open interfaces never enter a save.
use anyhow::{Context, Result, ensure};
use bozzard_scene::{
    BlueprintRuntime,
    blueprint::{Blackboard, BlackboardValue as B, Value},
};
use serde::{Deserialize, Serialize};

const SCENE: &[&str] = &[
    "seed",
    "demo_mode",
    "chunk_x",
    "chunk_z",
    "cursor_x",
    "cursor_z",
    "selected",
    "direction",
    "clock",
    "ticks",
    "counts",
];
const OBJECT: &[&str] = &[
    "creative",
    "phase",
    "stock",
    "bar",
    "bar_slots",
    "session",
    "power_data",
    "power_other",
    "chunk_nodes",
    "cache_recipes",
    "cache_builds",
    "cache_facings",
    "cache_items",
    "cache_item_amounts",
    "cache_input_items",
    "cache_input_amounts",
    "cache_progress",
    "cache_assembler_iron",
    "cache_assembler_copper",
    "cache_split_state",
    "cache_storage_kinds_0",
    "cache_storage_kinds_1",
    "cache_storage_kinds_2",
    "cache_storage_kinds_3",
    "cache_storage_amounts_0",
    "cache_storage_amounts_1",
    "cache_storage_amounts_2",
    "cache_storage_amounts_3",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub scene: Blackboard,
    pub controller: Blackboard,
}

pub fn number(board: &Blackboard, name: &str) -> Result<f32> {
    match board.get(name) {
        Some(B::Scalar(Value::Number(n))) if n.is_finite() => Ok(*n),
        _ => anyhow::bail!("missing numeric field {name}"),
    }
}
pub fn values<'a>(board: &'a Blackboard, name: &str) -> Result<&'a [Value]> {
    match board.get(name) {
        Some(B::List { values, .. }) => Ok(values),
        _ => anyhow::bail!("missing list {name}"),
    }
}
pub fn numeric(value: &Value) -> Result<f32> {
    match value {
        Value::Number(n) if n.is_finite() => Ok(*n),
        _ => anyhow::bail!("expected finite number"),
    }
}
fn integer(n: f32, min: f32, max: f32) -> Result<()> {
    ensure!(
        n.is_finite() && n.fract() == 0. && (min..=max).contains(&n),
        "saved value out of range"
    );
    Ok(())
}
impl State {
    pub fn dev_world(&self) -> bool {
        matches!(
            self.controller["session"].values().get(63),
            Some(Value::Number(1.))
        )
    }

    pub fn allows_chunk(&self, planet: u8, x: i16, z: i16) -> bool {
        if self.dev_world() {
            planet == 0 && (0..=1).contains(&x) && (0..=1).contains(&z)
        } else {
            let radius = if planet == 1 { 6 } else { 8 };
            planet < 2 && (-radius..=radius).contains(&x) && (-radius..=radius).contains(&z)
        }
    }
    /// Capture at the end of a normal gameplay tick without finishing rotations,
    /// moving the local player, or asking Rhai to archive its live chunk. Network
    /// snapshots must include edits and production made since the last crossing.
    pub fn capture_live(runtime: &BlueprintRuntime) -> Result<Self> {
        let mut state = Self::capture(runtime)?;
        let scene = runtime.scene_blackboard();
        let controller = runtime.object_blackboard("controller").unwrap();
        let planet = numeric(&values(controller, "session")?[7])? as usize;
        let chunk = (number(scene, "chunk_z")? as i32 + 8) as usize * 17
            + (number(scene, "chunk_x")? as i32 + 8) as usize;
        let built = values(scene, "builds")?
            .iter()
            .any(|v| numeric(v).is_ok_and(|n| n > 0.));
        let storage = values(scene, "builds")?
            .iter()
            .any(|v| numeric(v).is_ok_and(|kind| kind == 4. || (12. ..=25.).contains(&kind)));
        for name in OBJECT
            .iter()
            .filter(|k| k.starts_with("cache_") || **k == "chunk_nodes")
        {
            let source = if *name == "chunk_nodes" {
                "nodes"
            } else {
                name.trim_start_matches("cache_")
            };
            let board = if source == "recipes" {
                controller
            } else {
                scene
            };
            let packed = if (*name != "chunk_nodes" && !built)
                || (source.starts_with("storage_") && !storage)
            {
                String::new()
            } else {
                pack_numbers(values(board, source)?)?
            };
            state.controller.get_mut(*name).unwrap().values_mut()[planet * 289 + chunk] =
                Value::Text(packed);
        }
        state.validate()?;
        Ok(state)
    }

    pub fn capture(runtime: &BlueprintRuntime) -> Result<Self> {
        fn select(board: &Blackboard, keys: &[&str]) -> Result<Blackboard> {
            keys.iter()
                .map(|key| {
                    Ok((
                        (*key).into(),
                        board
                            .get(*key)
                            .with_context(|| format!("missing {key}"))?
                            .clone(),
                    ))
                })
                .collect()
        }
        let mut state = Self {
            scene: select(runtime.scene_blackboard(), SCENE)?,
            controller: select(
                runtime
                    .object_blackboard("controller")
                    .context("missing factory controller")?,
                OBJECT,
            )?,
        };
        if let Some(B::List { values, .. }) = state.controller.get_mut("session") {
            for (i, value) in values.iter_mut().enumerate() {
                if i != 0
                    && i != 63
                    && !(7..40).contains(&i)
                    && !(64..114).contains(&i)
                    && !(128..160).contains(&i)
                    && i != 120
                {
                    *value = Value::Number(0.);
                }
            }
        }
        state.validate()?;
        Ok(state)
    }
    pub fn validate(&self) -> Result<()> {
        for (board, keys) in [(&self.scene, SCENE), (&self.controller, OBJECT)] {
            ensure!(
                board.len() == keys.len() && keys.iter().all(|k| board.contains_key(*k)),
                "save schema mismatch"
            );
            bozzard_scene::blueprint::validate_blackboard(board)?;
        }
        for (board, key) in [(&self.scene, "demo_mode"), (&self.controller, "creative")] {
            ensure!(
                matches!(board.get(key), Some(B::Scalar(Value::Bool(_)))),
                "invalid game mode"
            );
        }
        integer(number(&self.scene, "seed")?, 1., 2_147_483_648.)?;
        integer(number(&self.controller, "phase")?, 0., 7.)?;
        integer(number(&self.controller, "bar")?, 0., 5.)?;
        integer(number(&self.scene, "selected")?, 1., 29.)?;
        integer(number(&self.scene, "direction")?, 0., 3.)?;
        integer(number(&self.scene, "ticks")?, 0., f32::MAX)?;
        ensure!(
            (0. ..0.32).contains(&number(&self.scene, "clock")?),
            "invalid factory clock"
        );
        check_numbers(&self.scene, "counts", 64, 0., 100_000_000.)?;
        check_numbers(&self.controller, "stock", 64, 0., 2500.)?;
        check_numbers(&self.controller, "bar_slots", 3, 1., 8.)?;
        let session = values(&self.controller, "session")?;
        ensure!(session.len() == 160, "invalid session length");
        for value in session {
            numeric(value)?;
        }
        integer(numeric(&session[0])?, 0., 1.)?;
        integer(numeric(&session[7])?, 0., 1.)?;
        integer(numeric(&session[63])?, 0., 1.)?;
        let developer = self.dev_world();
        let radius = if numeric(&session[7])? == 1. { 6. } else { 8. };
        for key in ["chunk_x", "chunk_z"] {
            integer(number(&self.scene, key)?, -radius, radius)?;
        }
        for key in ["cursor_x", "cursor_z"] {
            integer(number(&self.scene, key)?, -7., 7.)?;
        }
        ensure!(
            self.allows_chunk(
                numeric(&session[7])? as u8,
                number(&self.scene, "chunk_x")? as i16,
                number(&self.scene, "chunk_z")? as i16
            ),
            "saved location outside world bounds"
        );
        ensure!(
            (0. ..=1_000_000_000.).contains(&numeric(&session[120])?),
            "invalid world time"
        );
        for value in session[8..40].iter().chain(&session[128..160]) {
            integer(numeric(value)?, 0., 100_000_000.)?;
        }
        let mut totals = [0.; 64];
        for stack in session[64..114].chunks_exact(2) {
            let kind = numeric(&stack[0])?;
            let amount = numeric(&stack[1])?;
            integer(kind, 0., 50.)?;
            integer(amount, 0., 100.)?;
            ensure!(
                kind > 0. || amount == 0.,
                "inventory stack without material"
            );
            totals[kind as usize] += amount;
        }
        ensure!(
            values(&self.controller, "stock")?
                .iter()
                .zip(totals)
                .all(|(v, n)| numeric(v).ok() == Some(n)),
            "inventory totals mismatch"
        );
        for key in OBJECT
            .iter()
            .filter(|k| k.starts_with("cache_") || **k == "chunk_nodes")
        {
            let pages = values(&self.controller, key)?;
            ensure!(pages.len() == 578, "invalid planet archive size");
            let count = if key.contains("storage_") { 900 } else { 225 };
            let max = if key.contains("amount") || key.contains("assembler_") {
                100
            } else if *key == "cache_builds" {
                29
            } else if *key == "cache_facings" {
                3
            } else if *key == "cache_progress" {
                8
            } else if *key == "cache_split_state" {
                2
            } else {
                53
            };
            for (index, page) in pages.iter().enumerate() {
                let Value::Text(text) = page else {
                    anyhow::bail!("invalid archive page");
                };
                validate_rle(text, count, max)?;
                if developer && *key == "chunk_nodes" && !text.is_empty() {
                    ensure!(
                        self.allows_chunk(
                            (index / 289) as u8,
                            (index % 17) as i16 - 8,
                            (index % 289 / 17) as i16 - 8
                        ),
                        "Dev World contains an outside region"
                    );
                }
            }
        }
        self.validate_expansion_buffers()?;
        let chunk = (number(&self.scene, "chunk_z")? as i32 + 8) as usize * 17
            + (number(&self.scene, "chunk_x")? as i32 + 8) as usize;
        let at = numeric(&session[7])? as usize * 289 + chunk;
        ensure!(
            matches!(&values(&self.controller,"chunk_nodes")?[at],Value::Text(t) if !t.is_empty()),
            "saved location is unexplored"
        );
        for key in ["power_data", "power_other"] {
            validate_power(values(&self.controller, key)?)?;
        }
        Ok(())
    }

    /// Version-one worlds used 32 material totals and a 128-number session.
    /// Extend only those exact legacy shapes; validation still rejects damaged data.
    pub fn upgrade_legacy(&mut self) {
        for (board, key, old, new) in [
            (&mut self.scene, "counts", 32, 64),
            (&mut self.controller, "stock", 32, 64),
        ] {
            if let Some(B::List {
                values, capacity, ..
            }) = board.get_mut(key)
            {
                if values.len() == old && *capacity == old {
                    values.resize(new, Value::Number(0.));
                    *capacity = new;
                }
            }
        }
        if let Some(B::List {
            values, capacity, ..
        }) = self.controller.get_mut("session")
        {
            if values.len() == 128 && *capacity == 128 {
                values.resize(160, Value::Number(0.));
                *capacity = 160;
            }
        }
    }

    fn validate_expansion_buffers(&self) -> Result<()> {
        for (at, encoded) in values(&self.controller, "cache_builds")?.iter().enumerate() {
            let Value::Text(text) = encoded else {
                unreachable!()
            };
            if text.is_empty() {
                continue;
            }
            let builds = decode_rle(text, 225)?;
            if !builds.iter().any(|kind| (12. ..=29.).contains(kind)) {
                continue;
            }
            let column = |name: &str, count| -> Result<Vec<f32>> {
                let Value::Text(text) = &values(&self.controller, name)?[at] else {
                    unreachable!()
                };
                decode_rle(text, count)
            };
            let items = column("cache_items", 225)?;
            let amounts = column("cache_item_amounts", 225)?;
            let kinds = (0..4)
                .map(|page| column(&format!("cache_storage_kinds_{page}"), 900))
                .collect::<Result<Vec<_>>>()?;
            let stored = (0..4)
                .map(|page| column(&format!("cache_storage_amounts_{page}"), 900))
                .collect::<Result<Vec<_>>>()?;
            for (cell, kind) in builds.into_iter().enumerate() {
                if (12. ..=25.).contains(&kind) {
                    ensure!(
                        items[cell] == 0. && amounts[cell] == 0.,
                        "expansion output outside saved slots"
                    );
                    let mut load = 0.;
                    for slot in 0..16 {
                        let item = kinds[slot / 4][cell * 4 + slot % 4];
                        let amount = stored[slot / 4][cell * 4 + slot % 4];
                        ensure!(
                            item <= 50. && (item > 0. || amount == 0.),
                            "invalid production stack"
                        );
                        load += amount;
                    }
                    ensure!(load <= 100., "production buffer capacity exceeded");
                } else if kind == 26. || kind == 27. {
                    ensure!(
                        amounts[cell] <= 100.
                            && (amounts[cell] == 0.
                                || [6., 7., 33., 34., 38., 39.].contains(&items[cell])),
                        "invalid pipe contents"
                    );
                } else if kind == 28. || kind == 29. {
                    ensure!(
                        amounts[cell] <= 1. && ![6., 7., 33., 34., 38., 39.].contains(&items[cell]),
                        "invalid corner belt contents"
                    );
                }
            }
        }
        Ok(())
    }
}
fn decode_rle(text: &str, count: usize) -> Result<Vec<f32>> {
    if text.is_empty() {
        return Ok(vec![0.; count]);
    }
    let mut result = Vec::with_capacity(count);
    for part in text.split(',') {
        let (value, run) = part.split_once(':').unwrap_or((part, "1"));
        let value: u32 = value.parse()?;
        let run: usize = run.parse()?;
        ensure!(
            run > 0 && run <= count - result.len(),
            "invalid archive length"
        );
        result.resize(result.len() + run, value as f32);
    }
    ensure!(result.len() == count, "invalid archive length");
    Ok(result)
}
pub(crate) fn pack_numbers(values: &[Value]) -> Result<String> {
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < values.len() {
        let value = numeric(&values[at])?;
        integer(value, 0., u32::MAX as f32)?;
        let mut end = at + 1;
        while end < values.len() && numeric(&values[end])? == value {
            end += 1;
        }
        let value = (value as u32).to_string();
        if end - at >= 3 {
            tokens.push(format!("{value}:{}", end - at));
        } else {
            tokens.extend(std::iter::repeat_n(value, end - at));
        }
        at = end;
    }
    Ok(tokens.join(","))
}
fn check_numbers(board: &Blackboard, key: &str, count: usize, min: f32, max: f32) -> Result<()> {
    let list = values(board, key)?;
    ensure!(list.len() == count, "invalid {key} length");
    for value in list {
        integer(numeric(value)?, min, max)?;
    }
    Ok(())
}
fn validate_rle(text: &str, count: usize, max: u32) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let mut length = 0;
    for token in text.split(',') {
        let (value, run) = token.split_once(':').unwrap_or((token, "1"));
        let value: u32 = value.parse().context("invalid archive value")?;
        let run: usize = run.parse().context("invalid archive run")?;
        ensure!(
            value <= max && run > 0 && run <= count && length + run <= count,
            "invalid archive bounds"
        );
        length += run;
    }
    ensure!(length == count, "truncated archive");
    Ok(())
}
fn validate_power(pages: &[Value]) -> Result<()> {
    ensure!(pages.len() == 867, "invalid power graph size");
    let mut graph = std::collections::BTreeMap::new();
    for (page, value) in pages.iter().enumerate() {
        let Value::Text(text) = value else {
            anyhow::bail!("invalid power page");
        };
        if text.is_empty() {
            continue;
        }
        let cells: Vec<_> = text.split('|').collect();
        ensure!(cells.len() == 75, "invalid power page length");
        for (cell, text) in cells.into_iter().enumerate() {
            if text.is_empty() {
                continue;
            }
            let entry = text
                .split(',')
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ensure!(
                (2..=10).contains(&entry.len())
                    && ([1, 3, 5, 6, 9, 10, 11].contains(&entry[0])
                        || (12..=25).contains(&entry[0]))
                    && entry[1] <= 1,
                "invalid circuit entry"
            );
            graph.insert((page * 75 + cell) as u32, entry);
        }
    }
    for (id, entry) in &graph {
        for peer in &entry[2..] {
            ensure!(
                peer != id && graph.get(peer).is_some_and(|other| other[2..].contains(id)),
                "invalid power connection"
            );
        }
    }
    Ok(())
}
