use anyhow::Result;
use bozzard_assets::{AssetStore, LoadState};
use bozzard_render::{Gpu, SceneRenderer};
use bozzard_scene::Scene;
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub struct Assets {
    store: AssetStore,
    residency: bozzard_render_assets::Residency,
    last_poll: Instant,
    reload: Option<bozzard_assets::job::Job<(AssetStore, Vec<bozzard_assets::Handle>)>>,
}

impl Assets {
    pub fn load(scene: &Scene, source: Option<&Path>) -> Result<Self> {
        let root = source
            .and_then(Path::parent)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut store = AssetStore::new(root, &scene.assets)?;
        store.refresh();
        store.require_ready()?;
        Ok(Self {
            store,
            residency: bozzard_render_assets::Residency::default(),
            last_poll: Instant::now(),
            reload: None,
        })
    }

    pub fn upload(&mut self, gpu: &Gpu, renderer: &mut SceneRenderer) -> Result<()> {
        self.residency = bozzard_render_assets::Residency::default();
        self.residency.sync(gpu, renderer, &self.store)?;
        for entry in self.store.entries() {
            if let Some(stats) = renderer.model_upload_stats(&entry.id) {
                println!("gpu_asset_ready id={} stats={stats:?}", entry.id);
            }
        }
        Ok(())
    }

    pub fn poll(&mut self, gpu: &Gpu, renderer: &mut SceneRenderer) -> Result<()> {
        if let Err(error) = self
            .residency
            .advance(gpu, renderer, &self.store, 4 * 1024 * 1024)
        {
            eprintln!("gpu_upload_failed: {error:#}");
        }
        if self.reload.is_none() && self.last_poll.elapsed() >= Duration::from_millis(500) {
            self.reload = Some(self.store.refresh_job()?);
        }
        let Some(result) = self.reload.as_ref().and_then(|job| job.poll()) else {
            return Ok(());
        };
        self.reload = None;
        self.last_poll = Instant::now();
        let (store, changed) = result?;
        self.store = store;
        for handle in changed {
            let entry = self
                .store
                .get(handle)
                .expect("handle returned by same store");
            match entry.state() {
                LoadState::Ready => {
                    println!(
                        "asset_decoded id={} revision={} queued_for_gpu=true",
                        entry.id,
                        entry.revision()
                    );
                }
                LoadState::Failed(message) => {
                    eprintln!("asset_reload_failed: {message}; keeping last good asset")
                }
                LoadState::Pending => {}
            }
        }
        Ok(())
    }
}
