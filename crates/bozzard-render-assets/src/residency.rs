use anyhow::Result;
use bozzard_assets::{AssetData, AssetStore};
use bozzard_render::{Gpu, PendingUpload, SceneRenderer, UploadProgress};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ResidencyReport {
    pub uploaded: usize,
    pub removed: usize,
}

/// Tracks immutable asset snapshots associated with one renderer. A new renderer
/// needs a new Residency. Keeping Arc identities prevents allocator pointer reuse
/// from confusing a later asset with an old one.
#[derive(Default)]
pub struct Residency {
    current: BTreeMap<String, Arc<AssetData>>,
    failed: BTreeMap<String, Arc<AssetData>>,
    pending: Option<(String, Arc<AssetData>, PendingUpload)>,
    preparing: Option<(
        String,
        Arc<AssetData>,
        bozzard_assets::job::Job<PendingUpload>,
    )>,
}
impl Residency {
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
            if self.pending.is_none()
                && self.preparing.is_none()
                && store.entries().all(|entry| {
                    entry.shared_data().is_none_or(|data| {
                        self.current
                            .get(&entry.id)
                            .is_some_and(|old| Arc::ptr_eq(old, &data))
                            || self
                                .failed
                                .get(&entry.id)
                                .is_some_and(|old| Arc::ptr_eq(old, &data))
                    })
                })
            {
                break;
            }
            if self.preparing.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        anyhow::ensure!(
            self.has_all(store),
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
            .map(|(id, _, job)| (id.as_str(), job.cancelled()))
    }
    pub fn has_all(&self, store: &AssetStore) -> bool {
        store
            .entries()
            .all(|entry| entry.data().is_some() && self.current.contains_key(&entry.id))
    }
    /// Whether picking/inspection geometry matches the version currently on the GPU.
    /// `has_all` also accepts last-good resources during a staged replacement.
    pub fn is_current(&self, store: &AssetStore, id: &str) -> bool {
        store
            .handle(id)
            .and_then(|h| store.get(h))
            .and_then(|e| e.shared_data())
            .is_some_and(|data| {
                self.current
                    .get(id)
                    .is_some_and(|current| Arc::ptr_eq(current, &data))
            })
    }
    pub fn cancel(&mut self) {
        if let Some((id, data, _)) = self.pending.take() {
            self.failed.insert(id, data);
        }
        if let Some((id, data, job)) = &self.preparing {
            job.cancel();
            self.failed.insert(id.clone(), data.clone());
        }
    }
    pub fn advance(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SceneRenderer,
        store: &AssetStore,
        budget: usize,
    ) -> Result<ResidencyReport> {
        let desired: BTreeMap<_, _> = store
            .entries()
            .filter_map(|entry| entry.shared_data().map(|data| (entry.id.clone(), data)))
            .collect();
        let mut report = ResidencyReport::default();
        if let Some((id, data, job)) = &self.preparing
            && desired.get(id).is_none_or(|next| !Arc::ptr_eq(next, data))
        {
            job.cancel();
        }
        if let Some(result) = self.preparing.as_ref().and_then(|(_, _, job)| job.poll()) {
            let (id, data, job) = self.preparing.take().unwrap();
            if !job.cancelled() {
                match result {
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
            desired.get(id).is_none_or(|next| !Arc::ptr_eq(next, data))
        }) {
            self.pending = None;
        }
        let removed: Vec<_> = self
            .current
            .keys()
            .filter(|id| !desired.contains_key(*id))
            .cloned()
            .collect();
        for id in removed {
            renderer.remove_asset(&id);
            self.current.remove(&id);
            report.removed += 1;
        }
        self.failed
            .retain(|id, data| desired.get(id).is_some_and(|next| Arc::ptr_eq(next, data)));
        if self.pending.is_none() && self.preparing.is_none() {
            for (id, data) in &desired {
                if self
                    .current
                    .get(id)
                    .is_some_and(|old| Arc::ptr_eq(old, data))
                    || self
                        .failed
                        .get(id)
                        .is_some_and(|old| Arc::ptr_eq(old, data))
                {
                    continue;
                }
                let context = renderer.upload_context();
                let gpu = gpu.clone();
                let source = crate::upload_source(data.clone());
                match bozzard_assets::job::Job::start("Preparing GPU resources", move |progress| {
                    progress.check()?;
                    let upload = context.begin_upload(&gpu, source)?;
                    progress.check()?;
                    Ok(upload)
                }) {
                    Ok(job) => {
                        self.preparing = Some((id.clone(), data.clone(), job));
                    }
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
                    job.finish(renderer, &id)?;
                    self.current.insert(id.clone(), data);
                    self.failed.remove(&id);
                    report.uploaded += 1;
                }
                Ok(_) => {
                    self.pending = Some((id, data, job));
                }
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

    /// Explicit retries are separate from polling, so a failed GPU upload is not
    /// retried every frame while the source identity remains unchanged.
    pub fn retry_failed(&mut self) {
        self.failed.clear();
    }
}
