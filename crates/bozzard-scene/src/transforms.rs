//! Reuse composed transforms for static scene objects across simulation and extraction.
use super::*;
use std::sync::Mutex;

pub(super) fn is_valid_matrix(matrix: Mat4) -> bool {
    matrix.is_finite() && matrix.inverse().is_finite()
}

#[derive(Clone, Copy)]
struct Entry {
    local: Transform,
    parent: Mat4,
    global: Mat4,
}

#[derive(Default)]
struct CachedTransforms {
    revision: Option<u64>,
    ids: Vec<String>,
    entities: Vec<Entity>,
    parents: Vec<Option<usize>>,
    matrices: Vec<Mat4>,
    live: Vec<Option<Entry>>,
    rendered: Vec<Option<Entry>>,
    snapped: Vec<bool>,
}

fn compact<T>(items: &mut Vec<T>, active: usize) {
    let retained = active.max(64);
    if items.capacity() > retained.saturating_mul(4) {
        items.shrink_to(retained);
    }
}
#[derive(Default)]
pub(super) struct Cache(Mutex<CachedTransforms>);
impl Clone for Cache {
    fn clone(&self) -> Self {
        // Editor previews and cloned instances own independent runtime state.
        Self::default()
    }
}
#[cfg(test)]
impl Cache {
    pub(super) fn retained_capacities(&self) -> [usize; 8] {
        let cached = self.0.lock().unwrap();
        [
            cached.entities.capacity(),
            cached.parents.capacity(),
            cached.matrices.capacity(),
            cached.live.capacity(),
            cached.rendered.capacity(),
            cached.snapped.capacity(),
            cached.ids.capacity(),
            cached.matrices.len(),
        ]
    }
}

impl SceneInstance {
    pub(super) fn validate_render_pose(
        &self,
        world: &World,
        id: &str,
        global: Mat4,
    ) -> Result<(Mat4, bool)> {
        if is_valid_matrix(global) {
            Ok((global, false))
        } else {
            // Finite endpoints need not have a representable f32 blend. Snap to
            // the valid current world pose and propagate that choice to children.
            Ok((self.global_transform(world, id)?, true))
        }
    }

    pub fn global_transforms(&self, world: &World) -> Result<BTreeMap<String, Mat4>> {
        self.with_render_transforms(world, None, |matrices| {
            Ok(self
                .document
                .objects
                .iter()
                .zip(matrices)
                .map(|(object, &matrix)| (object.id.clone(), matrix))
                .collect())
        })
    }

    /// Compose local presentation poses before applying parents, preserving shear and
    /// nonuniform scale. Live physics/query transforms use a separate cache.
    pub fn interpolated_transforms(
        &self,
        world: &World,
        alpha: f32,
    ) -> Result<BTreeMap<String, Mat4>> {
        self.with_render_transforms(world, Some(alpha), |matrices| {
            Ok(self
                .document
                .objects
                .iter()
                .zip(matrices)
                .map(|(object, &matrix)| (object.id.clone(), matrix))
                .collect())
        })
    }

    /// Keep dense, document-indexed matrices inside the cache while extracting a
    /// snapshot. The public ID map is only materialized for callers that need it.
    pub(super) fn with_render_transforms<T>(
        &self,
        world: &World,
        alpha: Option<f32>,
        extract: impl FnOnce(&[Mat4]) -> Result<T>,
    ) -> Result<T> {
        if let Some(alpha) = alpha {
            ensure!(
                alpha.is_finite() && (0. ..=1.).contains(&alpha),
                "interpolation fraction must be finite and within 0..1"
            );
        }
        let history = alpha.and_then(|fraction| {
            world
                .resource::<interpolation::History>()
                .filter(|history| history.matches(self) && history.has_motion() && fraction < 1.)
                .map(|history| (history, fraction))
        });
        let mut cached = self
            .transform_cache
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("transform cache lock poisoned"))?;
        // Internal document edits can precede an index rebuild. Compare the
        // topology itself as well as its revision, without allocating per frame.
        let topology_changed = cached.ids.len() != self.document.objects.len()
            || self
                .document
                .objects
                .iter()
                .enumerate()
                .any(|(index, object)| {
                    cached.ids[index] != object.id
                        || cached.parents[index].map(|parent| cached.ids[parent].as_str())
                            != object.parent.as_deref()
                });
        if cached.revision != Some(self.hierarchy_revision) || topology_changed {
            cached.ids = self
                .document
                .objects
                .iter()
                .map(|object| object.id.clone())
                .collect();
            cached.entities = self
                .document
                .objects
                .iter()
                .map(|object| self.entities[&object.id])
                .collect();
            let indices: std::collections::HashMap<_, _> = self
                .document
                .objects
                .iter()
                .enumerate()
                .map(|(index, object)| (object.id.as_str(), index))
                .collect();
            cached.parents = self
                .document
                .objects
                .iter()
                .map(|object| object.parent.as_ref().map(|id| indices[id.as_str()]))
                .collect();
            cached.live.clear();
            cached.rendered.clear();
            let active = self.document.objects.len();
            cached.matrices.resize(active, Mat4::IDENTITY);
            cached.snapped.resize(active, false);
            compact(&mut cached.live, active);
            compact(&mut cached.rendered, active);
            compact(&mut cached.snapped, active);
            compact(&mut cached.matrices, active);
            cached.revision = Some(self.hierarchy_revision);
        }
        let CachedTransforms {
            entities,
            parents,
            matrices,
            live,
            rendered,
            snapped,
            ..
        } = &mut *cached;
        let cache = if history.is_some() { rendered } else { live };
        cache.resize(self.document.objects.len(), None);
        if history.is_some() {
            snapped.resize(self.document.objects.len(), false);
        }
        // Parents precede children. Comparing values rather than ECS ticks also handles
        // multiple writes within a tick, reparenting, and runtime prefab index reuse.
        for &index in &self.order {
            let object = &self.document.objects[index];
            let entity = entities[index];
            let local = world
                .get::<Transform>(entity)
                .context("scene object/transform was removed")?;
            let mut parent = parents[index]
                .map(|parent| matrices[parent])
                .unwrap_or(Mat4::IDENTITY);
            let interpolated = if let Some((history, fraction)) = history {
                let parent_snapped = parents[index].is_some_and(|parent| snapped[parent]);
                let (snap, sample) = history.local_sample(index, entity, *local, parent_snapped);
                snapped[index] = snap;
                if snap
                    && !parent_snapped
                    && let Some(id) = &object.parent
                {
                    // A cut uses its exact world pose, including moving ancestors.
                    // Other subtrees retain their interpolated presentation.
                    parent = self.global_transform(world, id)?;
                }
                sample
                    .map(|sample| interpolation::matrix(sample.previous, sample.current, fraction))
            } else {
                None
            };
            let global = if let Some(local_matrix) = interpolated {
                local.validate()?;
                cache[index] = None;
                let (global, fallback) =
                    self.validate_render_pose(world, &object.id, parent * local_matrix)?;
                snapped[index] |= fallback;
                global
            } else {
                match cache[index] {
                    Some(entry)
                        if super::render_extraction::transform_equal(entry.local, *local)
                            && super::render_extraction::floats_equal(
                                &entry.parent.to_cols_array(),
                                &parent.to_cols_array(),
                            ) =>
                    {
                        entry.global
                    }
                    _ => {
                        local.validate()?;
                        let global = parent * local.matrix();
                        let (global, fallback) = if history.is_some() {
                            self.validate_render_pose(world, &object.id, global)?
                        } else {
                            ensure!(
                                is_valid_matrix(global),
                                "invalid runtime transform on '{}'",
                                object.id
                            );
                            (global, false)
                        };
                        if fallback {
                            snapped[index] = true;
                            // A fallback does not compose from the rendered parent.
                            // It cannot be reused by the local/parent value cache.
                            cache[index] = None;
                        } else {
                            cache[index] = Some(Entry {
                                local: *local,
                                parent,
                                global,
                            });
                        }
                        global
                    }
                }
            };
            matrices[index] = global;
        }
        extract(matrices)
    }
}
