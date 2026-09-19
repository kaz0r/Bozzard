//! Transient, per-view LOD selection. Nothing here is serialized into a scene or save.
use super::*;
use std::sync::{Mutex, MutexGuard};

type Selections = std::collections::HashMap<Entity, (Lod, usize)>;
type Views = BTreeMap<(Layer, bool), (Entity, Selections)>;

#[derive(Default)]
pub(super) struct History(Mutex<Views>);
impl Clone for History {
    fn clone(&self) -> Self {
        // A cloned world or editor preview starts an independent presentation history.
        Self::default()
    }
}
impl History {
    pub fn lock(&self) -> Result<MutexGuard<'_, Views>> {
        self.0
            .lock()
            .map_err(|_| anyhow::anyhow!("LOD history lock poisoned"))
    }
}

pub(super) fn select(
    selections: &mut Selections,
    entity: Entity,
    lod: &Lod,
    distance: f32,
) -> Option<Option<Mesh>> {
    // Exact thresholds remain stateless and need no per-object allocation.
    if lod.hysteresis == 0. {
        selections.remove(&entity);
        return lod.level(distance);
    }
    let exact = || lod.levels.partition_point(|level| distance >= level.switch);
    let (previous, index) = selections
        .entry(entity)
        .or_insert_with(|| (lod.clone(), exact()));
    if previous != lod {
        *previous = lod.clone();
        *index = exact();
    } else {
        while *index < lod.levels.len()
            && distance >= lod.levels[*index].switch * (1. + lod.hysteresis)
        {
            *index += 1;
        }
        while *index > 0 && distance < lod.levels[*index - 1].switch * (1. - lod.hysteresis) {
            *index -= 1;
        }
    }
    index.checked_sub(1).map(|i| lod.levels[i].mesh.clone())
}
