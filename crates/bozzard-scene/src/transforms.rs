//! Reuse composed transforms for static scene objects across simulation and extraction.
use super::*;
use std::sync::{Arc, Mutex, MutexGuard};

pub(super) fn is_valid_matrix(matrix: Mat4) -> bool {
    matrix.is_finite() && matrix.inverse().is_finite()
}

#[derive(Clone, Copy)]
struct Entry {
    local: Transform,
    parent: Mat4,
    global: Mat4,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransformExtractionStats {
    pub source_reads: usize,
    pub revision_reuses: usize,
    pub topology_reads: usize,
    pub topology_rebuilds: usize,
}
#[derive(Default)]
struct CachedTransforms {
    revision: Option<u64>,
    ids: Vec<String>,
    entities: Vec<Entity>,
    entity_indices: bozzard_ecs::EntityMap<usize>,
    parents: Vec<Option<usize>>,
    matrices: Vec<Mat4>,
    live: Vec<Option<Entry>>,
    rendered: Vec<Option<Entry>>,
    snapped: Vec<bool>,
    source_revision: Option<bozzard_ecs::ComponentRevision>,
    last_static: bool,
    children: Vec<Vec<usize>>,
    ranks: Vec<usize>,
    dirty: Vec<u64>,
    dirty_epoch: u64,
    worklist: Vec<usize>,
    disabled: bool,
    stats: TransformExtractionStats,
    /// Live matrices lent to simulation systems; cleared whenever `matrices` changes.
    shared: Option<Arc<[Mat4]>>,
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

/// Live world matrices in document order. Systems share one copy until a transform
/// or the hierarchy changes, instead of each building an ID-keyed map.
#[derive(Clone)]
pub(crate) struct Matrices<'a> {
    instance: &'a SceneInstance,
    dense: Arc<[Mat4]>,
}
impl<'a> Matrices<'a> {
    /// No matrices, for systems that look up nothing this tick.
    pub(crate) fn empty(instance: &'a SceneInstance) -> Self {
        Self {
            instance,
            dense: Arc::new([]),
        }
    }
    pub(crate) fn get(&self, id: &str) -> Option<&Mat4> {
        self.entity(*self.instance.entities.get(id)?)
    }
    pub(crate) fn entity(&self, entity: Entity) -> Option<&Mat4> {
        self.dense.get(*self.instance.object_indices.get(&entity)?)
    }
    /// The matrix at a document index.
    pub(crate) fn at(&self, index: usize) -> Mat4 {
        self.dense[index]
    }
}
impl<Q: AsRef<str> + ?Sized> std::ops::Index<&Q> for Matrices<'_> {
    type Output = Mat4;
    fn index(&self, id: &Q) -> &Mat4 {
        self.get(id.as_ref())
            .expect("scene object has a live transform")
    }
}

impl SceneInstance {
    /// Select exact full transform/source scans for the retention reference oracle.
    pub fn set_sparse_render_extraction_enabled(&self, enabled: bool) -> Result<()> {
        let mut cached = self
            .transform_cache
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("transform cache lock poisoned"))?;
        cached.disabled = !enabled;
        cached.source_revision = None;
        drop(cached);
        self.render_cache.lock()?.revision_tracking_disabled = !enabled;
        Ok(())
    }
    pub fn render_transform_stats(&self) -> Result<TransformExtractionStats> {
        Ok(self
            .transform_cache
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("transform cache lock poisoned"))?
            .stats)
    }
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

    /// Validated live world matrices, shared until a transform or the hierarchy changes.
    pub(crate) fn live_matrices(&self, world: &World) -> Result<Matrices<'_>> {
        let mut cached = self.refresh_transforms(world, None)?;
        let dense = match &cached.shared {
            Some(dense) => dense.clone(),
            None => {
                let dense: Arc<[Mat4]> = cached.matrices.as_slice().into();
                cached.shared = Some(dense.clone());
                dense
            }
        };
        Ok(Matrices {
            instance: self,
            dense,
        })
    }

    /// Validate every live transform after a write, recomposing only changed subtrees.
    pub(crate) fn validate_live_transforms(&self, world: &World) -> Result<()> {
        self.refresh_transforms(world, None).map(drop)
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
        extract(&self.refresh_transforms(world, alpha)?.matrices)
    }

    fn refresh_transforms(
        &self,
        world: &World,
        alpha: Option<f32>,
    ) -> Result<MutexGuard<'_, CachedTransforms>> {
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
        cached.stats = Default::default();
        // Live topology is private and every prefab/load mutation rebuilds its
        // hierarchy revision. The full-scan oracle also checks exact topology;
        // unit tests retain this fallback for deliberately unversioned edits.
        let check_topology = cached.disabled || cfg!(test);
        cached.stats.topology_reads = if check_topology {
            self.document.objects.len()
        } else {
            0
        };
        let topology_changed = check_topology
            && (cached.ids.len() != self.document.objects.len()
                || self
                    .document
                    .objects
                    .iter()
                    .enumerate()
                    .any(|(index, object)| {
                        cached.ids[index] != object.id
                            || cached.parents[index].map(|parent| cached.ids[parent].as_str())
                                != object.parent.as_deref()
                    }));
        if cached.revision != Some(self.hierarchy_revision) || topology_changed {
            cached.stats.topology_rebuilds = 1;
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
            cached.entity_indices = cached
                .entities
                .iter()
                .enumerate()
                .map(|(index, &entity)| (entity, index))
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
            cached.source_revision = None;
            cached.last_static = false;
            cached.children = vec![Vec::new(); cached.parents.len()];
            for index in 0..cached.parents.len() {
                if let Some(parent) = cached.parents[index] {
                    cached.children[parent].push(index);
                }
            }
            cached.ranks.resize(self.order.len(), 0);
            for (rank, &index) in self.order.iter().enumerate() {
                cached.ranks[index] = rank;
            }
            let active = self.document.objects.len();
            cached.matrices.resize(active, Mat4::IDENTITY);
            cached.snapped.resize(active, false);
            compact(&mut cached.live, active);
            compact(&mut cached.rendered, active);
            compact(&mut cached.snapped, active);
            compact(&mut cached.matrices, active);
            cached.revision = Some(self.hierarchy_revision);
        }
        let source_revision = world.component_revision::<Transform>();
        if !cached.disabled
            && history.is_none()
            && cached.last_static
            && cached.source_revision == Some(source_revision)
        {
            cached.stats.revision_reuses = cached.matrices.len();
            return Ok(cached);
        }
        cached.shared = None;
        cached.worklist.clear();
        let changes = cached
            .source_revision
            .filter(|_| !cached.disabled && history.is_none() && cached.last_static)
            .and_then(|previous| world.component_changes_since::<Transform>(previous));
        if let Some(changes) = changes {
            let active = cached.entities.len();
            cached.dirty.resize(active, 0);
            cached.dirty_epoch = if let Some(next) = cached.dirty_epoch.checked_add(1) {
                next
            } else {
                cached.dirty.fill(0);
                1
            };
            let epoch = cached.dirty_epoch;
            for entity in changes {
                if let Some(&index) = cached.entity_indices.get(&entity)
                    && cached.dirty[index] != epoch
                {
                    cached.dirty[index] = epoch;
                    cached.worklist.push(index);
                }
            }
            let mut cursor = 0;
            while cursor < cached.worklist.len() {
                let index = cached.worklist[cursor];
                for child_index in 0..cached.children[index].len() {
                    let child = cached.children[index][child_index];
                    if cached.dirty[child] != epoch {
                        cached.dirty[child] = epoch;
                        cached.worklist.push(child);
                    }
                }
                cursor += 1;
            }
            let CachedTransforms {
                worklist, ranks, ..
            } = &mut *cached;
            worklist.sort_unstable_by_key(|index| ranks[*index]);
        } else {
            cached.worklist.extend_from_slice(&self.order);
        }
        cached.stats.source_reads = cached.worklist.len();
        cached.stats.revision_reuses = cached.matrices.len().saturating_sub(cached.worklist.len());
        cached.last_static = false;
        let CachedTransforms {
            entities,
            parents,
            matrices,
            live,
            rendered,
            snapped,
            worklist,
            ..
        } = &mut *cached;
        let cache = if history.is_some() { rendered } else { live };
        cache.resize(self.document.objects.len(), None);
        if history.is_some() {
            snapped.resize(self.document.objects.len(), false);
        }
        // Parents precede children. Comparing values rather than ECS ticks also handles
        // multiple writes within a tick, reparenting, and runtime prefab index reuse.
        for &index in worklist.iter() {
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
        cached.source_revision = Some(source_revision);
        cached.last_static = history.is_none();
        Ok(cached)
    }
}
