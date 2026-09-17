//! Once-per-world frame coordination. Viewports only borrow the generated texture views.
use anyhow::Result;
use bozzard_render::{Gpu, GpuFrameTiming, SceneRenderer, compute::Executor};
use bozzard_scene::SceneInstance;
use std::collections::BTreeMap;

pub struct ComputeBridge {
    pub executor: Executor,
    attempted: BTreeMap<String, (u64, u64)>,
    diagnostics: BTreeMap<String, String>,
}
impl ComputeBridge {
    pub fn new(gpu: &Gpu) -> Self {
        Self {
            executor: Executor::new(gpu),
            attempted: BTreeMap::new(),
            diagnostics: BTreeMap::new(),
        }
    }
    /// Install enabled device limits before the first script tick, including after restart/load.
    pub fn prepare(&self, instance: &mut SceneInstance) {
        if instance.compute_capabilities().device_generation
            != self.executor.capabilities().device_generation
        {
            instance.set_compute_capabilities(self.executor.capabilities().clone());
        }
    }
    pub fn diagnostics(&self) -> &BTreeMap<String, String> {
        &self.diagnostics
    }
    /// Compatible edits swap before the next tick. Failed source/GPU validation and incompatible
    /// host interfaces retain the last working revision until a corrected edit or Play restart.
    pub fn refresh(
        &mut self,
        gpu: &Gpu,
        instance: &mut SceneInstance,
        assets: &bozzard_assets::AssetStore,
    ) -> Result<()> {
        self.attempted
            .retain(|id, _| instance.compute_kernels().contains_key(id));
        self.diagnostics
            .retain(|id, _| instance.compute_kernels().contains_key(id));
        let mut next = None;
        for (id, previous) in instance.compute_kernels() {
            let Some(entry) = assets.handle(id).and_then(|h| assets.get(h)) else {
                continue;
            };
            if let bozzard_assets::LoadState::Failed(error) = entry.state() {
                if self.diagnostics.get(id) != Some(error) {
                    self.diagnostics.insert(id.clone(), error.clone());
                }
                continue;
            }
            let Some(bozzard_assets::AssetData::ComputeShader(candidate)) = entry.data() else {
                continue;
            };
            if candidate.id() == previous.id() {
                self.diagnostics.remove(id);
                continue;
            }
            let pair = (previous.id(), candidate.id());
            if self.attempted.get(id) == Some(&pair) {
                continue;
            }
            self.attempted.insert(id.clone(), pair);
            let compatible = previous.entries().all(|old| {
                candidate
                    .entry(&old.name)
                    .is_ok_and(|new| new.bindings == old.bindings)
            });
            if !compatible {
                self.diagnostics.insert(id.clone(), "Compute interface changed. Stop and restart Play after updating the script's bindings/parameters to recreate its resources. The last working shader remains active.".into());
                continue;
            }
            match self.executor.validate_kernel(gpu, candidate) {
                Ok(()) => {
                    next.get_or_insert_with(|| instance.compute_kernels().clone())
                        .insert(id.clone(), candidate.clone());
                    self.diagnostics.remove(id);
                }
                Err(error) => {
                    self.diagnostics.insert(id.clone(), format!("{error:#}"));
                }
            }
        }
        if let Some(next) = next {
            instance.register_compute_kernels(next)?;
        }
        Ok(())
    }
    /// Call before ticking, or while presentation is unavailable. This only posts CPU inbox data.
    pub fn poll(&mut self, gpu: &Gpu) -> Result<Vec<GpuFrameTiming>> {
        self.executor.poll(gpu)
    }
    /// Drain accepted work once, regardless of camera count or drawable dimensions.
    pub fn submit(&mut self, gpu: &Gpu, instance: &SceneInstance) -> Result<bool> {
        if let Some(mut compute) = instance.compute_if_initialized() {
            self.executor.submit(gpu, &mut compute.runtime)
        } else {
            self.executor.clear_world();
            Ok(false)
        }
    }
    pub fn sync_renderer(&self, renderer: &mut SceneRenderer) {
        renderer.set_generated_textures(
            self.executor.texture_revision(),
            self.executor.texture_views(),
        );
    }
    pub fn stop(&mut self) {
        self.executor.clear_world();
        self.attempted.clear();
        self.diagnostics.clear();
    }
}
