use super::*;
use bozzard_assets::{AssetStore, Handle, job::Job};

pub enum Loading {
    Prefab(Job<bozzard_editor::PreparedPrefab>),
    BakeGi(Job<bozzard_editor::PreparedGi>),
    Import(Job<bozzard_editor::PreparedImport>),
    Open(Job<bozzard_editor::LoadedScene>),
    Save(Job<bozzard_editor::PreparedSave>),
}
impl Loading {
    pub fn label(&self) -> String {
        match self {
            Self::Prefab(job) => job.label(),
            Self::BakeGi(job) => job.label(),
            Self::Import(job) => job.label(),
            Self::Open(job) => job.label(),
            Self::Save(job) => job.label(),
        }
    }
    pub fn cancel(&self) {
        match self {
            Self::Prefab(job) => job.cancel(),
            Self::BakeGi(job) => job.cancel(),
            Self::Import(job) => job.cancel(),
            Self::Open(job) => job.cancel(),
            Self::Save(job) => job.cancel(),
        }
    }
    pub fn cancelled(&self) -> bool {
        match self {
            Self::Prefab(job) => job.cancelled(),
            Self::BakeGi(job) => job.cancelled(),
            Self::Import(job) => job.cancelled(),
            Self::Open(job) => job.cancelled(),
            Self::Save(job) => job.cancelled(),
        }
    }
}
pub type Refresh = (u64, Job<(AssetStore, Vec<Handle>)>);

impl App {
    pub fn start_prefab(&mut self, command: bozzard_editor::PrefabCommand) {
        let result = (|| {
            ensure!(
                self.loading.is_none(),
                "Wait for the current operation first"
            );
            self.drag = None;
            self.loading = Some(Loading::Prefab(self.editor.prefab_job(command)?));
            Ok(())
        })();
        self.result(result);
    }

    pub fn start_import(&mut self, path: PathBuf) {
        let result = (|| {
            if let Some(Loading::Import(job)) = &self.loading {
                ensure!(!job.cancelled(), "Wait for cancellation to finish");
                ensure!(
                    self.import_queue.len() < 31,
                    "Import queue is full (32 files); try remaining files after loading finishes"
                );
                self.import_queue.push_back(path);
                return Ok(());
            }
            ensure!(
                self.loading.is_none(),
                "Wait for the current import or cancel it first"
            );
            self.editor.finish_gesture();
            self.drag = None;
            self.loading = Some(Loading::Import(self.editor.import_job(path)?));
            self.workspace.assets_visible = true;
            Ok(())
        })();
        self.result(result);
    }
    pub fn poll_loading(&mut self) {
        let cancelled = self.loading.as_ref().is_some_and(Loading::cancelled);
        let completion = match self.loading.as_ref() {
            Some(Loading::Prefab(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    let label = prepared.label.clone();
                    let layer = prepared.layer;
                    let asset = self.editor.accept_prefab(prepared)?;
                    if let Some(layer) = layer {
                        self.workspace.layer_2d = layer == Layer::TwoD;
                    }
                    self.status = format!("{label}: {asset}. Save the scene to keep its links.");
                    self.asset_browser.reveal(asset);
                    self.workspace.assets_visible = true;
                    self.hierarchy_search.clear();
                    Ok(())
                })
            }),
            Some(Loading::BakeGi(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    self.editor.accept_gi(prepared)?;
                    self.status = "Global illumination baked. Save the scene to keep it.".into();
                    Ok(())
                })
            }),
            Some(Loading::Save(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    self.editor.accept_save(prepared)?;
                    self.status = format!("Saved {}", self.editor.path.display());
                    Ok(())
                })
            }),
            Some(Loading::Import(job)) => job.poll().map(|result| {
                result.and_then(|prepared| {
                    let id = self.editor.accept_import(prepared)?;
                    self.status = format!("Imported {id}");
                    self.asset_browser.reveal(id);
                    Ok(())
                })
            }),
            Some(Loading::Open(job)) => job.poll().map(|result| {
                result.map(|loaded| {
                    self.editor = loaded.into_editor();
                    self.hierarchy_state = hierarchy::HierarchyState::default();
                    self.workspace.camera = None;
                    self.workspace.ortho_zoom = 1.0;
                    self.residency.retry_failed();
                    // Catalog revisions are local to an editor; an older scene's refresh must never land here.
                    self.refresh = None;
                    self.reload_paused = false;
                    self.status = format!("Opened {}", self.editor.path.display());
                })
            }),
            None => None,
        };
        if let Some(result) = completion {
            self.loading = None;
            let success = result.is_ok();
            self.result(result);
            if cancelled {
                self.status = "Loading cancelled".into();
                self.error = false;
            }
            if !success {
                self.import_queue.clear();
            }
            if self.continue_after_save {
                self.continue_after_save = false;
                if success {
                    self.perform_pending();
                } else {
                    self.confirm_discard = true;
                }
            }
            if self.close_after_loading {
                self.close_after_loading = false;
                self.request(Pending::Close);
            } else if self.loading.is_none()
                && let Some(path) = self.import_queue.pop_front()
            {
                self.start_import(path);
            }
        }
    }
}
