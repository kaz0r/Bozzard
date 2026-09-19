use anyhow::{Result, ensure};
use bozzard_assets::{AssetData, AssetStore};
use bozzard_render::{Gpu, PendingUpload, SceneRenderer, UploadProgress};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ResidencyReport {
    pub uploaded: usize,
    pub removed: usize,
    pub evicted: usize,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct ResidencyStats {
    pub resident_assets: usize,
    pub resident_bytes: usize,
    pub staged_bytes: usize,
    pub budget_bytes: Option<usize>,
    /// Required assets are pinned. Report pressure instead of cycling visible assets
    /// through eviction/reload forever when the working set exceeds the soft budget.
    pub over_budget_bytes: usize,
    pub evictions: u64,
}
struct Resident {
    data: Arc<AssetData>,
    allocation: Arc<usize>,
    last_used: u64,
}

fn same_material_image(a: &AssetData, b: &AssetData) -> bool {
    match (a, b) {
        (AssetData::Material(a), AssetData::Material(b)) => match (&a.image, &b.image) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        },
        _ => false,
    }
}
fn same_gpu(a: &AssetData, b: &AssetData) -> bool {
    std::ptr::eq(a, b) || same_material_image(a, b)
}

/// Tracks immutable asset snapshots associated with one renderer. A new renderer
/// needs a new Residency. Arc identities prevent allocator pointer reuse from
/// confusing a later asset with an old one. The default eagerly loads the catalog;
/// hosts enable streaming by supplying the current render scene's required assets.
#[derive(Default)]
pub struct Residency {
    current: BTreeMap<String, Resident>,
    failed: BTreeMap<String, Arc<AssetData>>,
    pending: Option<(String, Arc<AssetData>, PendingUpload)>,
    preparing: Option<(
        String,
        Arc<AssetData>,
        usize,
        bozzard_assets::job::Job<PendingUpload>,
    )>,
    required: Option<BTreeSet<String>>,
    budget_bytes: Option<usize>,
    clock: u64,
    evictions: u64,
    resident_bytes: usize,
}
impl Residency {
    pub fn set_budget(&mut self, bytes: Option<usize>) {
        self.budget_bytes = bytes;
    }
    pub fn set_required(&mut self, assets: BTreeSet<String>) {
        self.required = Some(assets);
    }
    pub fn require_catalog(&mut self) {
        self.required = None;
    }
    fn needs(&self, id: &str) -> bool {
        self.required.as_ref().is_none_or(|ids| ids.contains(id))
    }
    pub fn stats(&self) -> ResidencyStats {
        let resident_bytes = self.resident_bytes;
        let staged_bytes = self
            .pending
            .as_ref()
            .map_or(0, |(_, _, job)| job.memory_bytes())
            + self.preparing.as_ref().map_or(0, |(_, _, bytes, _)| *bytes);
        ResidencyStats {
            resident_assets: self.current.len(),
            resident_bytes,
            staged_bytes,
            budget_bytes: self.budget_bytes,
            over_budget_bytes: self.budget_bytes.map_or(0, |limit| {
                resident_bytes
                    .saturating_add(staged_bytes)
                    .saturating_sub(limit)
            }),
            evictions: self.evictions,
        }
    }
    /// Imported assets used by the extracted frame, including every material override.
    /// The same list covers offscreen shadow casters; CPU frustum culling must not
    /// evict their resources before the renderer's shadow passes can use them.
    pub fn require_scene(&mut self, scene: &bozzard_render::RenderScene) {
        self.set_required(required_assets(scene));
    }
    pub fn sync(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SceneRenderer,
        store: &AssetStore,
    ) -> Result<ResidencyReport> {
        let mut result = ResidencyReport::default();
        loop {
            let report = self.advance(gpu, renderer, store, 4 * 1024 * 1024)?;
            result.uploaded += report.uploaded;
            result.removed += report.removed;
            result.evicted += report.evicted;
            if self.pending.is_none()
                && self.preparing.is_none()
                && store.entries().all(|entry| {
                    !self.needs(&entry.id)
                        || entry.data().is_none_or(|data| {
                            !crate::needs_gpu(data)
                                || self
                                    .current
                                    .get(&entry.id)
                                    .is_some_and(|old| same_gpu(old.data.as_ref(), data))
                                || self
                                    .failed
                                    .get(&entry.id)
                                    .is_some_and(|old| same_gpu(old.as_ref(), data))
                        })
                })
            {
                break;
            }
            if self.preparing.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        ensure!(
            self.has_required(store),
            "GPU assets are unavailable; retry the failed upload"
        );
        Ok(result)
    }
    pub fn progress(&self) -> Option<(&str, UploadProgress)> {
        self.pending
            .as_ref()
            .map(|(id, _, job)| (id.as_str(), job.progress()))
    }
    pub fn preparing(&self) -> Option<(&str, bool)> {
        self.preparing
            .as_ref()
            .map(|(id, _, _, job)| (id.as_str(), job.cancelled()))
    }
    pub fn has_all(&self, store: &AssetStore) -> bool {
        store.entries().all(|entry| match entry.data() {
            Some(data) if !crate::needs_gpu(data) => true,
            Some(_) => self.current.contains_key(&entry.id),
            None => false,
        })
    }
    pub fn has_required(&self, store: &AssetStore) -> bool {
        self.required.as_ref().map_or_else(
            || self.has_all(store),
            |ids| {
                ids.iter().all(|id| {
                    store
                        .handle(id)
                        .and_then(|h| store.get(h))
                        .and_then(|e| e.data())
                        .is_some_and(|data| {
                            !crate::needs_gpu(data) || self.current.contains_key(id)
                        })
                })
            },
        )
    }
    pub fn required_current(&self, store: &AssetStore) -> bool {
        self.has_required(store)
            && store
                .entries()
                .filter(|e| self.needs(&e.id))
                .all(|e| self.is_current(store, &e.id))
    }
    /// Whether picking/inspection geometry matches the version currently on the GPU.
    /// `has_required` also accepts last-good resources during a staged replacement.
    pub fn is_current(&self, store: &AssetStore, id: &str) -> bool {
        store
            .handle(id)
            .and_then(|h| store.get(h))
            .and_then(|e| e.data())
            .is_some_and(|data| {
                !crate::needs_gpu(data)
                    || self
                        .current
                        .get(id)
                        .is_some_and(|current| same_gpu(current.data.as_ref(), data))
            })
    }
    pub fn cancel(&mut self) {
        if let Some((id, data, _)) = self.pending.take() {
            self.failed.insert(id, data);
        }
        if let Some((id, data, _, job)) = &self.preparing {
            job.cancel();
            self.failed.insert(id.clone(), data.clone());
        }
    }
    fn trim(&mut self, renderer: &mut SceneRenderer, reserve: usize, report: &mut ResidencyReport) {
        let Some(limit) = self.budget_bytes else {
            return;
        };
        let mut bytes = self.resident_bytes.saturating_add(reserve);
        if bytes <= limit {
            return;
        }
        let mut candidates: Vec<_> = self
            .current
            .iter()
            .filter(|(id, _)| !self.needs(id))
            .map(|(id, r)| (r.last_used, id.clone()))
            .collect();
        candidates.sort_unstable();
        for (_, id) in candidates {
            if bytes <= limit {
                break;
            }
            renderer.remove_asset(&id);
            let resident = self.current.remove(&id).unwrap();
            let size = if Arc::strong_count(&resident.allocation) == 1 {
                *resident.allocation
            } else {
                0
            };
            self.resident_bytes -= size;
            bytes = bytes.saturating_sub(size);
            report.evicted += 1;
            self.evictions = self.evictions.saturating_add(1);
        }
    }
    pub fn advance(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SceneRenderer,
        store: &AssetStore,
        budget: usize,
    ) -> Result<ResidencyReport> {
        self.clock = self.clock.saturating_add(1);
        // Compare borrowed payload identities against our retained Arcs. Avoid
        // cloning every catalog ID and incrementing every snapshot refcount per frame.
        let desired = |id: &str| {
            store
                .handle(id)
                .and_then(|h| store.get(h))
                .and_then(|entry| entry.data())
                .filter(|data| crate::needs_gpu(data))
        };
        let mut report = ResidencyReport::default();
        if let Some((id, data, _, job)) = &self.preparing
            && (!self.needs(id) || desired(id).is_none_or(|next| !same_gpu(next, data.as_ref())))
        {
            job.cancel();
        }
        if let Some(result) = self
            .preparing
            .as_ref()
            .and_then(|(_, _, _, job)| job.poll())
        {
            let (id, data, estimate, job) = self.preparing.take().unwrap();
            if !job.cancelled() {
                match result.and_then(|upload| {
                    ensure!(
                        upload.memory_bytes() == estimate,
                        "GPU upload footprint changed during preparation"
                    );
                    Ok(upload)
                }) {
                    Ok(upload) => self.pending = Some((id, data, upload)),
                    Err(error) => {
                        self.failed.insert(id.clone(), data);
                        return Err(error.context(format!(
                            "preparing GPU asset '{id}'; keeping previous data"
                        )));
                    }
                }
            }
        }
        if self.pending.as_ref().is_some_and(|(id, data, _)| {
            !self.needs(id) || desired(id).is_none_or(|next| !same_gpu(next, data.as_ref()))
        }) {
            self.pending = None;
        }
        let removed: Vec<_> = self
            .current
            .keys()
            .filter(|id| desired(id).is_none())
            .cloned()
            .collect();
        for id in removed {
            renderer.remove_asset(&id);
            let resident = self.current.remove(&id).unwrap();
            if Arc::strong_count(&resident.allocation) == 1 {
                self.resident_bytes -= *resident.allocation;
            }
            report.removed += 1;
        }
        self.failed
            .retain(|id, data| desired(id).is_some_and(|next| same_gpu(next, data.as_ref())));
        for (id, resident) in &mut self.current {
            if let Some(entry) = store.handle(id).and_then(|h| store.get(h))
                && let Some(data) = entry.data()
                && !std::ptr::eq(resident.data.as_ref(), data)
                && same_gpu(&resident.data, data)
            {
                resident.data = entry.shared_data().unwrap();
            }
            if self.required.as_ref().is_none_or(|ids| ids.contains(id)) {
                resident.last_used = self.clock;
            }
        }
        self.trim(renderer, self.stats().staged_bytes, &mut report);
        if self.pending.is_none() && self.preparing.is_none() {
            for entry in store.entries() {
                let id = &entry.id;
                if !self.needs(id) {
                    continue;
                }
                let Some(data) = entry.data().filter(|data| crate::needs_gpu(data)) else {
                    continue;
                };
                if self
                    .current
                    .get(id)
                    .is_some_and(|old| same_gpu(old.data.as_ref(), data))
                    || self
                        .failed
                        .get(id)
                        .is_some_and(|old| same_gpu(old.as_ref(), data))
                {
                    continue;
                }
                let data = entry.shared_data().expect("borrowed snapshot above");
                if let Some((source, allocation)) = self
                    .current
                    .iter()
                    .find(|(_, resident)| same_material_image(&resident.data, &data))
                    .map(|(source, resident)| (source.clone(), resident.allocation.clone()))
                {
                    renderer.alias_image(&source, id)?;
                    if let Some(old) = self.current.insert(
                        id.clone(),
                        Resident {
                            data,
                            allocation,
                            last_used: self.clock,
                        },
                    ) && Arc::strong_count(&old.allocation) == 1
                    {
                        self.resident_bytes -= *old.allocation;
                    }
                    continue;
                }
                let source =
                    crate::upload_source_with_features(data.clone(), gpu.device.features())?;
                let bytes = match bozzard_render::upload_memory_bytes(source.as_ref()) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        self.failed.insert(id.clone(), data.clone());
                        return Err(error.context(format!("measuring GPU asset '{id}'")));
                    }
                };
                self.trim(renderer, bytes, &mut report);
                let context = renderer.upload_context();
                let gpu = gpu.clone();
                match bozzard_assets::job::Job::start("Preparing GPU resources", move |progress| {
                    progress.check()?;
                    let upload = context.begin_upload(&gpu, source)?;
                    progress.check()?;
                    Ok(upload)
                }) {
                    Ok(job) => self.preparing = Some((id.clone(), data.clone(), bytes, job)),
                    Err(error) => {
                        self.failed.insert(id.clone(), data.clone());
                        return Err(error.context(format!(
                            "preparing GPU asset '{id}'; keeping previous data"
                        )));
                    }
                }
                break;
            }
        }
        if let Some((id, data, mut job)) = self.pending.take() {
            match job.advance(gpu, renderer, budget) {
                Ok(progress) if progress.complete => {
                    let bytes = job.memory_bytes();
                    job.finish(renderer, &id)?;
                    if let Some(old) = self.current.insert(
                        id.clone(),
                        Resident {
                            data,
                            allocation: Arc::new(bytes),
                            last_used: self.clock,
                        },
                    ) && Arc::strong_count(&old.allocation) == 1
                    {
                        self.resident_bytes -= *old.allocation;
                    }
                    self.resident_bytes += bytes;
                    self.failed.remove(&id);
                    report.uploaded += 1;
                }
                Ok(_) => self.pending = Some((id, data, job)),
                Err(error) => {
                    self.failed.insert(id.clone(), data);
                    return Err(
                        error.context(format!("uploading GPU asset '{id}'; keeping previous data"))
                    );
                }
            }
        }
        Ok(report)
    }
    /// A failed revision is retried only after an explicit retry or source change.
    pub fn retry_failed(&mut self) {
        self.failed.clear();
    }
}

pub fn required_assets(scene: &bozzard_render::RenderScene) -> BTreeSet<String> {
    use bozzard_render::{MeshKind, TextureKind};
    let mut ids = BTreeSet::new();
    for item in &scene.items {
        if let MeshKind::Imported(id) | MeshKind::ModelPart(id, _) = &item.mesh {
            ids.insert(id.as_str());
        }
        for texture in std::iter::once(&item.material.texture).chain(
            item.material
                .surface_overrides
                .iter()
                .filter_map(|s| s.texture.as_ref()),
        ) {
            if let TextureKind::Imported(id) | TextureKind::ModelPart(id, _) = texture {
                ids.insert(id.as_str());
            }
        }
    }
    // Repeated instances share IDs: allocate each name once per frame.
    ids.into_iter().map(str::to_owned).collect()
}
