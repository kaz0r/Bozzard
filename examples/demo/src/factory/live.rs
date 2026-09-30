//! Apply accepted host transactions to the running factory, preserving local UI,
//! camera, item interpolation and unchanged model handles. Rhai reconciles only
//! affected resident cells on its next update, before input or production.
use super::{
    shared,
    state::{self, State},
};
use anyhow::{Context, Result, ensure};
use bozzard_scene::{
    BlueprintRuntime,
    blueprint::{Blackboard, BlackboardValue as B, Value},
};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub id: usize,
    pub build: bool,
    pub item: bool,
}

pub fn apply(
    runtime: &mut BlueprintRuntime,
    world: &shared::World,
    player: &shared::Player,
) -> Result<Vec<Change>> {
    let before = State::capture_live(runtime)?;
    let local = shared::Player::capture(&before)?;
    // The host's movement and flight are still driven by local gameplay. Guest
    // actions must never replace that view with the guest's coordinates.
    ensure!(
        local.position == player.position,
        "host transaction moved the local view"
    );
    let after = world.project(player)?;
    ensure!(
        before.scene["seed"] == after.scene["seed"],
        "transaction replaced the world"
    );
    let planet = player.position.planet as usize;
    let chunk = player.position.archive_index() % 289;
    let board = runtime
        .object_blackboard("controller")
        .context("missing controller")?;
    let resident = state::values(board, "resident")?;
    let mut changes = Vec::new();
    for (id, resident) in resident[..289].iter().enumerate() {
        if state::numeric(resident)? == 0. {
            continue;
        }
        let at = planet * 289 + id;
        let mut dirty_build = [false; 225];
        let mut dirty_item = [false; 225];
        for name in [
            "cache_builds",
            "cache_facings",
            "cache_items",
            "cache_item_amounts",
        ] {
            if before.controller[name].values()[at] == after.controller[name].values()[at] {
                continue;
            }
            let a = page(&before, name, at, 225)?;
            let b = page(&after, name, at, 225)?;
            let dirty = if name == "cache_builds" || name == "cache_facings" {
                &mut dirty_build
            } else {
                &mut dirty_item
            };
            for cell in 0..225 {
                dirty[cell] |= a[cell] != b[cell];
            }
        }
        for cell in 0..225 {
            if dirty_build[cell] || dirty_item[cell] {
                changes.push(Change {
                    id: id * 225 + cell,
                    build: dirty_build[cell],
                    item: dirty_item[cell] || dirty_build[cell],
                });
            }
        }
    }
    let mut scene = Blackboard::new();
    let mut controller = Blackboard::new();
    // The saved schema is a closed whitelist. Only changed authoritative fields
    // are written; transient session/UI fields are merged separately below.
    for (key, value) in &after.scene {
        if before.scene[key] != *value {
            scene.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in &after.controller {
        if key != "session" && before.controller[key] != *value {
            controller.insert(key.clone(), value.clone());
        }
    }
    let mut session = board["session"].clone();
    for i in (8..40).chain(64..114).chain(128..160).chain([120]) {
        session.values_mut()[i] = after.controller["session"].values()[i].clone();
    }
    if session.values()[64..114] != board["session"].values()[64..114] {
        session.values_mut()[49] =
            Value::Number(state::numeric(&board["session"].values()[49])? + 1.);
    }
    if session != board["session"] {
        controller.insert("session".into(), session);
    }
    let power_changed = before.controller["power_data"] != after.controller["power_data"];
    if power_changed {
        controller.insert("power_dirty".into(), B::Scalar(Value::Bool(true)));
    }
    let mut visited = board["visited"].clone();
    let mut discovered = false;
    for id in 0..289 {
        if matches!(&after.controller["chunk_nodes"].values()[planet*289+id], Value::Text(t) if !t.is_empty())
            && state::numeric(&visited.values()[id])? == 0.
        {
            visited.values_mut()[id] = Value::Number(1.);
            discovered = true;
        }
    }
    if discovered {
        controller.insert("visited".into(), visited);
        controller.insert(
            "residency_view".into(),
            B::Scalar(Value::Text(String::new())),
        );
    }
    // Archives are authoritative even for the currently occupied region. Hydrate
    // its live arrays now so the next capture cannot overwrite the guest's edit.
    let mut storage_changed = false;
    for name in after.controller.keys().filter(|k| {
        (k.as_str() != "cache_structures" && k.starts_with("cache_")) || k.as_str() == "chunk_nodes"
    }) {
        let at = planet * 289 + chunk;
        if before.controller[name].values()[at] == after.controller[name].values()[at] {
            continue;
        }
        let live = if name == "chunk_nodes" {
            "nodes"
        } else {
            name.trim_start_matches("cache_")
        };
        let count = if live.starts_with("storage_") {
            900
        } else {
            225
        };
        storage_changed |= live.starts_with("storage_");
        let source = if live == "recipes" {
            board
        } else {
            runtime.scene_blackboard()
        };
        let mut list = source
            .get(live)
            .context("missing live factory array")?
            .clone();
        let values = page(&after, name, at, count)?;
        for (value, number) in list.values_mut().iter_mut().zip(&values) {
            *value = Value::Number(*number);
        }
        if live == "recipes" {
            controller.insert(live.into(), list);
        } else {
            scene.insert(live.into(), list);
        }
        if live == "builds" {
            let mut index = runtime.scene_blackboard()["machine_cells"].clone();
            let B::List { values: cells, .. } = &mut index else {
                anyhow::bail!("invalid machine index");
            };
            *cells = values
                .iter()
                .enumerate()
                .filter(|(_, v)| **v > 0.)
                .map(|(i, _)| Value::Number(i as f32))
                .collect();
            scene.insert("machine_cells".into(), index);
        }
    }
    if storage_changed {
        scene.insert(
            "storage_revision".into(),
            B::Scalar(Value::Number(
                state::number(runtime.scene_blackboard(), "storage_revision")? + 1.,
            )),
        );
        // A remote edit can change which stack a drag/menu points at. Cancel it
        // before the next input event rather than applying it to a different item.
        for key in ["storage_drag", "storage_menu"] {
            scene.insert(key.into(), B::Scalar(Value::Number(-1.)));
        }
    }
    runtime.patch_blackboards(&scene, &[("controller".into(), controller)].into())?;
    Ok(changes)
}

fn page(state: &State, name: &str, at: usize, count: usize) -> Result<Vec<f32>> {
    let Value::Text(text) = &state.controller[name].values()[at] else {
        anyhow::bail!("invalid archive");
    };
    if text.is_empty() {
        return Ok(vec![0.; count]);
    }
    let mut result = Vec::with_capacity(count);
    for part in text.split(',') {
        let (number, run) = part.split_once(':').unwrap_or((part, "1"));
        let number: f32 = number.parse()?;
        let run: usize = run.parse()?;
        ensure!(
            run > 0 && run <= count - result.len(),
            "invalid archive length"
        );
        result.resize(result.len() + run, number);
    }
    ensure!(result.len() == count, "invalid archive length");
    Ok(result)
}

pub fn resident_items(runtime: &BlueprintRuntime, regions: &[usize]) -> Result<Vec<Change>> {
    if regions.is_empty() {
        return Ok(Vec::new());
    }
    let state = State::capture_live(runtime)?;
    let planet = state::numeric(&state.controller["session"].values()[7])? as usize;
    let mut changes = Vec::new();
    for id in regions {
        for (cell, amount) in page(&state, "cache_item_amounts", planet * 289 + id, 225)?
            .iter()
            .enumerate()
        {
            if *amount > 0. {
                changes.push(Change {
                    id: id * 225 + cell,
                    build: false,
                    item: true,
                });
            }
        }
    }
    Ok(changes)
}
