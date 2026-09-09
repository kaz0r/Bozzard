use anyhow::Result;
use bozzard_assets::{AssetData, AssetStore, LoadState};
use bozzard_render::{Gpu, SceneRenderer};
use bozzard_scene::Scene;
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub struct Assets {
    store: AssetStore,
    last_poll: Instant,
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
            last_poll: Instant::now(),
        })
    }

    pub fn upload(&self, gpu: &Gpu, renderer: &mut SceneRenderer) -> Result<()> {
        for entry in self.store.entries() {
            if let Some(data) = entry.data() {
                upload(gpu, renderer, &entry.id, data)?;
            }
        }
        Ok(())
    }

    pub fn poll(&mut self, gpu: &Gpu, renderer: &mut SceneRenderer) -> Result<()> {
        if self.last_poll.elapsed() < Duration::from_millis(500) {
            return Ok(());
        }
        self.last_poll = Instant::now();
        for handle in self.store.refresh() {
            let entry = self
                .store
                .get(handle)
                .expect("handle returned by same store");
            match entry.state() {
                LoadState::Ready => {
                    upload(gpu, renderer, &entry.id, entry.data().expect("ready asset"))?;
                    println!(
                        "asset_reloaded id={} revision={}",
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

pub fn upload(gpu: &Gpu, renderer: &mut SceneRenderer, id: &str, data: &AssetData) -> Result<()> {
    match data {
        AssetData::Image(image) => {
            renderer.upload_image(gpu, id, image.width, image.height, &image.rgba)
        }
        AssetData::Mesh(mesh) => {
            let parts: Vec<_> = mesh
                .parts
                .iter()
                .map(|part| bozzard_render::ModelPart {
                    start: part.start,
                    count: part.count,
                    color: part.color,
                    alpha_cutoff: part.alpha_cutoff,
                    image: part.image.as_ref().map(|image| bozzard_render::ModelImage {
                        width: image.width,
                        height: image.height,
                        rgba: &image.rgba,
                    }),
                })
                .collect();
            renderer.upload_model(gpu, id, &mesh.vertices, &mesh.indices, &parts)
        }
    }
}
