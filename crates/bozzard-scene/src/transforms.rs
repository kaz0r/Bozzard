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
pub(super) struct Cache(Mutex<Vec<Option<Entry>>>);
impl Clone for Cache {
    fn clone(&self) -> Self {
        // Editor previews and cloned instances own independent runtime state.
        Self::default()
    }
}

impl SceneInstance {
    pub fn global_transforms(&self, world: &World) -> Result<BTreeMap<String, Mat4>> {
        let mut cache = self
            .transform_cache
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("transform cache lock poisoned"))?;
        cache.resize(self.document.objects.len(), None);
        let mut matrices = BTreeMap::new();
        // Parents precede children. Comparing values rather than ECS ticks also handles
        // multiple writes within a tick, reparenting, and runtime prefab index reuse.
        for &index in &self.order {
            let object = &self.document.objects[index];
            let local = world
                .get::<Transform>(self.entities[&object.id])
                .context("scene object/transform was removed")?;
            let parent = object
                .parent
                .as_ref()
                .map(|p| matrices[p])
                .unwrap_or(Mat4::IDENTITY);
            let global = match cache[index] {
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
            };
            matrices.insert(object.id.clone(), global);
        }
        Ok(matrices)
    }
}
