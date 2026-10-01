//! Reuse composed transforms for static scene objects across simulation and extraction.
use super::*;
use std::sync::Mutex;

#[derive(Clone, Copy)]
struct Entry {
    local: Transform,
    parent: Mat4,
    global: Mat4,
}

#[derive(Default)]
struct CachedTransforms {
    live: Vec<Option<Entry>>,
    rendered: Vec<Option<Entry>>,
    snapped: Vec<bool>,
}
#[derive(Default)]
pub(super) struct Cache(Mutex<CachedTransforms>);
impl Clone for Cache {
    fn clone(&self) -> Self {
        // Editor previews and cloned instances own independent runtime state.
        Self::default()
    }
}

impl SceneInstance {
    pub fn global_transforms(&self, world: &World) -> Result<BTreeMap<String, Mat4>> {
        self.compose_transforms(world, None)
    }

    /// Compose local presentation poses before applying parents, preserving shear and
    /// nonuniform scale. Live physics/query transforms use a separate cache.
    pub fn interpolated_transforms(
        &self,
        world: &World,
        alpha: f32,
    ) -> Result<BTreeMap<String, Mat4>> {
        ensure!(
            alpha.is_finite() && (0. ..=1.).contains(&alpha),
            "interpolation fraction must be finite and within 0..1"
        );
        let history = world
            .resource::<interpolation::History>()
            .filter(|h| h.matches(self));
        if alpha == 1. || history.is_none_or(|h| !h.has_motion()) {
            return self.global_transforms(world);
        }
        self.compose_transforms(world, Some(alpha))
    }

    fn compose_transforms(
        &self,
        world: &World,
        alpha: Option<f32>,
    ) -> Result<BTreeMap<String, Mat4>> {
        let mut cached = self
            .transform_cache
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("transform cache lock poisoned"))?;
        let CachedTransforms {
            live,
            rendered,
            snapped,
        } = &mut *cached;
        let cache = if alpha.is_some() { rendered } else { live };
        cache.resize(self.document.objects.len(), None);
        let history = alpha.and_then(|fraction| {
            world
                .resource::<interpolation::History>()
                .map(|history| (history, fraction))
        });
        if history.is_some() {
            snapped.resize(self.document.objects.len(), false);
        }
        let mut matrices = BTreeMap::new();
        // Parents precede children. Comparing values rather than ECS ticks also handles
        // multiple writes within a tick, reparenting, and runtime prefab index reuse.
        for &index in &self.order {
            let object = &self.document.objects[index];
            let entity = self.entities[&object.id];
            let local = world
                .get::<Transform>(entity)
                .context("scene object/transform was removed")?;
            let mut parent = object
                .parent
                .as_ref()
                .map(|p| matrices[p])
                .unwrap_or(Mat4::IDENTITY);
            let interpolated = if let Some((history, fraction)) = history {
                let parent_snapped = object
                    .parent
                    .as_ref()
                    .is_some_and(|id| snapped[self.object_indices[&self.entities[id]]]);
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
                let global = parent * local_matrix;
                ensure!(
                    global.is_finite() && global.inverse().is_finite(),
                    "invalid interpolated transform on '{}'",
                    object.id
                );
                global
            } else {
                match cache[index] {
                    Some(entry) if entry.local == *local && entry.parent == parent => entry.global,
                    _ => {
                        local.validate()?;
                        let global = parent * local.matrix();
                        ensure!(
                            global.is_finite() && global.inverse().is_finite(),
                            "invalid runtime transform on '{}'",
                            object.id
                        );
                        cache[index] = Some(Entry {
                            local: *local,
                            parent,
                            global,
                        });
                        global
                    }
                }
            };
            matrices.insert(object.id.clone(), global);
        }
        Ok(matrices)
    }
}
