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
    last_pressure: usize,
    scene_generation: u64,
    reload_generation: u64,
}

impl Assets {
    pub fn store(&self) -> &AssetStore {
        &self.store
    }
    pub fn current(&self) -> bool {
        self.residency.required_current(&self.store)
    }
    pub fn load(scene: &Scene, source: Option<&Path>) -> Result<Self> {
        let root = source
            .and_then(Path::parent)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut store = AssetStore::new(root, &scene.assets)?;
        store.refresh();
        store.require_ready()?;
        store.validate_scene_resources(scene)?;
        Ok(Self {
            store,
            residency: bozzard_render_assets::Residency::default(),
            last_poll: Instant::now(),
            reload: None,
            last_pressure: 0,
            scene_generation: 0,
            reload_generation: 0,
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

    pub fn upload_required(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SceneRenderer,
        scene: &bozzard_render::RenderScene,
        budget: usize,
    ) -> Result<()> {
        self.residency = bozzard_render_assets::Residency::default();
        self.residency.set_budget(Some(budget));
        self.residency.require_scene(scene);
        self.residency.sync(gpu, renderer, &self.store)?;
        Ok(())
    }
    pub fn prepare_frame(
        &mut self,
        gpu: &Gpu,
        renderer: &mut SceneRenderer,
        scene: &bozzard_render::RenderScene,
    ) -> Result<bool> {
        self.residency.require_scene(scene);
        match self
            .residency
            .advance(gpu, renderer, &self.store, 4 * 1024 * 1024)
        {
            Ok(report) if report.evicted > 0 => eprintln!(
                "gpu_assets_evicted count={} resident_bytes={}",
                report.evicted,
                self.residency.stats().resident_bytes
            ),
            Ok(_) => {}
            Err(error) => {
                if !self.residency.has_required(&self.store) {
                    return Err(error);
                }
                eprintln!("gpu_upload_failed: {error:#}; keeping previous data");
            }
        }
        let stats = self.residency.stats();
        if stats.over_budget_bytes > 0 && self.last_pressure != stats.over_budget_bytes {
            eprintln!(
                "gpu_asset_budget_pressure resident_bytes={} staged_bytes={} budget_bytes={} required_overage={}",
                stats.resident_bytes,
                stats.staged_bytes,
                stats.budget_bytes.unwrap_or(0),
                stats.over_budget_bytes
            );
        }
        self.last_pressure = stats.over_budget_bytes;
        Ok(self.residency.has_required(&self.store))
    }

    pub fn adopt_scene_assets(&mut self, world: &bozzard_app::World) -> bool {
        if let Some(loaded) = world.resource::<bozzard_project::streaming::SceneAssets>()
            && loaded.generation != self.scene_generation
        {
            self.store = loaded.store.clone();
            self.scene_generation = loaded.generation;
            return true;
        }
        false
    }

    pub fn poll(&mut self) -> Result<()> {
        if self.reload.is_none() && self.last_poll.elapsed() >= Duration::from_millis(500) {
            self.reload = Some(self.store.refresh_job()?);
            self.reload_generation = self.scene_generation;
        }
        let Some(result) = self.reload.as_ref().and_then(|job| job.poll()) else {
            return Ok(());
        };
        self.reload = None;
        self.last_poll = Instant::now();
        if self.reload_generation != self.scene_generation {
            return Ok(());
        }
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
