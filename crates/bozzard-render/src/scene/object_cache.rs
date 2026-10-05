use super::*;
use std::collections::{HashMap, HashSet};

impl SceneRenderer {
    pub(super) fn remap_object_bindings(&mut self, draws: &[PreparedDraw]) {
        if !self.state_caching {
            self.object_identities = draws.iter().map(preparation::surface_identity).collect();
            return;
        }
        if self.object_identities.len() == draws.len()
            && self
                .object_identities
                .iter()
                .zip(draws)
                .all(|(old, draw)| *old == preparation::surface_identity(draw))
        {
            return;
        }
        let next: Vec<_> = draws.iter().map(preparation::surface_identity).collect();
        let unique = |ids: &[Option<preparation::SurfaceIdentity>]| {
            let mut seen = HashSet::with_capacity(ids.len());
            ids.iter().all(|id| id.is_some_and(|id| seen.insert(id)))
        };
        if unique(&next)
            && unique(&self.object_identities)
            && self.object_identities.len() == self.objects.len()
        {
            let previous = std::mem::take(&mut self.objects);
            let mut by_id: HashMap<_, _> = self
                .object_identities
                .iter()
                .copied()
                .zip(previous)
                .map(|(id, binding)| (id.unwrap(), binding))
                .collect();
            self.objects = next
                .iter()
                .zip(draws)
                .map(|(id, draw)| {
                    by_id
                        .remove(&id.unwrap())
                        .unwrap_or_else(|| ObjectBinding::new(draw.object.material.texture.clone()))
                })
                .collect();
        }
        self.object_identities = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_cpu_records_need_no_gpu_allocation() {
        let mut record = ObjectBinding::new(TextureKind::White);
        record.uniform_revision = 42;
        record.transform = Some((Mat4::IDENTITY, Mat4::IDENTITY, 1.));
        assert!(record.resources.is_none());
        assert_eq!(record.uniform_revision, 42);
        assert_eq!(record.transform.unwrap().0, Mat4::IDENTITY);
    }
}
