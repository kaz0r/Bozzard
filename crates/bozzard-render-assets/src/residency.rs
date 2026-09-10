use anyhow::Result;
use bozzard_assets::{AssetData, AssetStore};
use bozzard_render::{Gpu, SceneRenderer};
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
}
impl Residency {
    pub fn sync(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SceneRenderer,
        store: &AssetStore,
    ) -> Result<ResidencyReport> {
        let desired = store
            .entries()
            .filter_map(|entry| entry.shared_data().map(|data| (entry.id.clone(), data)))
            .collect();
        self.reconcile(desired, |id, data| match data {
            Some(data) => crate::upload(gpu, renderer, id, data),
            None => {
                renderer.remove_asset(id);
                Ok(())
            }
        })
    }

    /// Explicit retries are separate from polling, so a failed GPU upload is not
    /// retried every frame while the source identity remains unchanged.
    pub fn retry_failed(&mut self) {
        self.failed.clear();
    }

    fn reconcile(
        &mut self,
        desired: BTreeMap<String, Arc<AssetData>>,
        mut apply: impl FnMut(&str, Option<&AssetData>) -> Result<()>,
    ) -> Result<ResidencyReport> {
        let mut report = ResidencyReport::default();
        let removed: Vec<_> = self
            .current
            .keys()
            .filter(|id| !desired.contains_key(*id))
            .cloned()
            .collect();
        for id in removed {
            apply(&id, None)?;
            self.current.remove(&id);
            report.removed += 1;
        }
        self.failed
            .retain(|id, data| desired.get(id).is_some_and(|new| Arc::ptr_eq(new, data)));
        let mut failure = None;
        for (id, data) in desired {
            if self
                .current
                .get(&id)
                .is_some_and(|old| Arc::ptr_eq(old, &data))
                || self
                    .failed
                    .get(&id)
                    .is_some_and(|old| Arc::ptr_eq(old, &data))
            {
                continue;
            }
            match apply(&id, Some(&data)) {
                Ok(()) => {
                    self.current.insert(id.clone(), data);
                    self.failed.remove(&id);
                    report.uploaded += 1;
                }
                Err(error) => {
                    self.failed.insert(id.clone(), data);
                    if failure.is_none() {
                        failure =
                            Some(error.context(format!(
                                "uploading asset '{id}'; keeping previous GPU data"
                            )));
                    }
                }
            }
        }
        if let Some(error) = failure {
            Err(error)
        } else {
            Ok(report)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image(value: u8) -> Arc<AssetData> {
        Arc::new(AssetData::Image(bozzard_assets::ImageData {
            width: 1,
            height: 1,
            rgba: vec![value; 4],
        }))
    }
    #[test]
    fn catalog_snapshots_upload_only_changed_assets_and_retire_removed_entries() {
        let a = image(1);
        let b = image(2);
        let mut residency = Residency::default();
        let mut gpu = BTreeMap::new();
        let mut apply = |id: &str, data: Option<&AssetData>| {
            if let Some(AssetData::Image(image)) = data {
                gpu.insert(id.to_owned(), image.rgba[0]);
            } else {
                gpu.remove(id);
            }
            Ok(())
        };
        let catalog = BTreeMap::from([("a".into(), a.clone()), ("b".into(), b.clone())]);
        assert_eq!(
            residency
                .reconcile(catalog.clone(), &mut apply)
                .unwrap()
                .uploaded,
            2
        );
        assert_eq!(
            residency.reconcile(catalog.clone(), &mut apply).unwrap(),
            ResidencyReport::default()
        );
        let mut changed = catalog.clone();
        changed.insert("b".into(), image(3));
        assert_eq!(
            residency.reconcile(changed, &mut apply).unwrap().uploaded,
            1
        );
        // Undo restores an earlier immutable snapshot and requires one replacement.
        assert_eq!(
            residency.reconcile(catalog, &mut apply).unwrap().uploaded,
            1
        );
        assert_eq!(
            residency
                .reconcile(BTreeMap::from([("a".into(), a)]), &mut apply)
                .unwrap()
                .removed,
            1
        );
        assert_eq!(gpu, BTreeMap::from([("a".into(), 1)]));
    }
    #[test]
    fn failure_keeps_last_good_and_does_not_starve_other_assets_or_retry_each_frame() {
        let mut residency = Residency::default();
        let old = image(1);
        let bad = image(2);
        let other = image(3);
        let mut calls = Vec::new();
        let mut apply = |id: &str, data: Option<&AssetData>| {
            calls.push(id.to_owned());
            if matches!(data,Some(AssetData::Image(image)) if image.rgba[0]==2) {
                anyhow::bail!("upload failed");
            }
            Ok(())
        };
        residency
            .reconcile(BTreeMap::from([("a".into(), old.clone())]), &mut apply)
            .unwrap();
        let catalog = BTreeMap::from([("a".into(), bad), ("b".into(), other)]);
        assert!(residency.reconcile(catalog.clone(), &mut apply).is_err());
        assert!(Arc::ptr_eq(&residency.current["a"], &old));
        assert!(residency.current.contains_key("b"));
        assert_eq!(
            residency.reconcile(catalog.clone(), &mut apply).unwrap(),
            ResidencyReport::default()
        );
        residency.retry_failed();
        assert!(residency.reconcile(catalog, &mut apply).is_err());
        assert_eq!(calls, ["a", "a", "b", "a"]);
    }
}
