//! Scene-owned shader catalog; parsing and validation never create a graphics device.
use crate::{AssetKind, Scene, SceneInstance, compute::Kernel};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

/// Runtime-only state: neither handles nor generated material overrides enter scene/save JSON.
pub struct SceneCompute {
    pub runtime: crate::compute::Runtime,
    pub(crate) kernels: BTreeMap<String, Arc<Kernel>>,
    pub(crate) named_jobs: BTreeMap<(crate::compute::Owner, String), crate::compute::Ticket>,
    pub(crate) materials: BTreeMap<String, (crate::compute::Owner, crate::compute::Handle)>,
    reported: BTreeSet<crate::compute::Ticket>,
}
impl SceneCompute {
    pub(crate) fn new(
        capabilities: crate::compute::Capabilities,
        kernels: BTreeMap<String, Arc<Kernel>>,
    ) -> Self {
        Self {
            runtime: crate::compute::Runtime::new(capabilities),
            kernels,
            named_jobs: BTreeMap::new(),
            materials: BTreeMap::new(),
            reported: BTreeSet::new(),
        }
    }
    pub(crate) fn release_objects(&mut self, ids: &BTreeSet<String>) -> Result<()> {
        let owners: BTreeSet<_> = self
            .runtime
            .resources()
            .map(|(resource, _, _)| &resource.owner)
            .chain(self.runtime.jobs().map(|job| &job.owner))
            .filter(|owner| ids.contains(&owner.object))
            .cloned()
            .collect();
        for owner in owners {
            self.cancel_owner(&owner, true)?;
        }
        self.materials.retain(|target, _| !ids.contains(target));
        Ok(())
    }
    pub fn material_texture(&self, object: &str) -> Option<crate::compute::Handle> {
        let (owner, handle) = self.materials.get(object)?;
        self.runtime.resource(owner, *handle).ok().map(|_| *handle)
    }
    pub fn bind_material(
        &mut self,
        owner: &crate::compute::Owner,
        object: &str,
        handle: crate::compute::Handle,
    ) -> Result<()> {
        ensure!(
            matches!(
                self.runtime.resource(owner, handle)?.kind,
                crate::compute::ResourceKind::Texture { .. }
            ),
            "material output requires a compute texture"
        );
        ensure!(
            self.materials.contains_key(object) || self.materials.len() < 4096,
            "compute material override limit reached"
        );
        self.materials
            .insert(object.to_owned(), (owner.clone(), handle));
        Ok(())
    }
    pub(crate) fn cancel_owner(
        &mut self,
        owner: &crate::compute::Owner,
        release: bool,
    ) -> Result<()> {
        if release {
            self.runtime.release_owner(owner)?;
        } else {
            let tickets: Vec<_> = self
                .runtime
                .jobs()
                .filter(|job| job.owner == *owner)
                .map(|job| job.ticket)
                .collect();
            for ticket in tickets {
                self.runtime.cancel(owner, ticket)?;
                self.runtime.forget(owner, ticket)?;
            }
        }
        self.named_jobs.retain(|(o, _), _| o != owner);
        if release {
            self.materials.retain(|_, (o, _)| o != owner);
        }
        Ok(())
    }
    pub(crate) fn name_job(
        &mut self,
        owner: &crate::compute::Owner,
        name: &str,
        ticket: crate::compute::Ticket,
    ) -> Result<()> {
        ensure!(
            !name.is_empty() && name.len() <= 128,
            "compute job names must be 1–128 bytes"
        );
        let key = (owner.clone(), name.to_owned());
        ensure!(
            !self.named_jobs.contains_key(&key),
            "compute job '{name}' already exists; take or cancel it first"
        );
        ensure!(
            self.named_jobs.len() < crate::compute::MAX_JOBS,
            "named compute job limit reached"
        );
        self.named_jobs.insert(key, ticket);
        Ok(())
    }
    pub(crate) fn named_job(
        &self,
        owner: &crate::compute::Owner,
        name: &str,
    ) -> Result<crate::compute::Ticket> {
        self.named_jobs
            .get(&(owner.clone(), name.to_owned()))
            .copied()
            .with_context(|| format!("unknown compute job '{name}'"))
    }
}

pub fn load_compute_kernels(
    document: &Scene,
    path: Option<&Path>,
) -> Result<BTreeMap<String, Arc<Kernel>>> {
    load_compute_kernels_with_progress(document, path, &bozzard_app::job::Progress::default())
}
pub fn load_compute_kernels_with_progress(
    document: &Scene,
    path: Option<&Path>,
    progress: &bozzard_app::job::Progress,
) -> Result<BTreeMap<String, Arc<Kernel>>> {
    let root = path.and_then(Path::parent).unwrap_or(Path::new("."));
    let mut kernels = BTreeMap::new();
    let mut bytes = 0;
    for (id, source) in document
        .assets
        .iter()
        .filter(|(_, a)| a.kind == AssetKind::ComputeShader)
    {
        progress.stage(format!("Reading compute shader {id}"))?;
        ensure!(kernels.len() < 256, "scene exceeds 256 compute shaders");
        let mut text = String::new();
        std::fs::File::open(root.join(&source.path))
            .with_context(|| format!("loading compute shader '{id}' ({})", source.path))?
            .take(crate::compute::MAX_SOURCE_BYTES as u64 + 1)
            .read_to_string(&mut text)?;
        bytes += text.len();
        ensure!(
            bytes <= 32 * 1024 * 1024,
            "compute shader sources exceed 32 MiB"
        );
        let kernel = Kernel::parse(text)
            .with_context(|| format!("compute shader '{id}' ({})", source.path))?;
        kernels.insert(id.clone(), Arc::new(kernel));
    }
    Ok(kernels)
}

pub(crate) fn validate_kernels(
    scene: &Scene,
    kernels: &BTreeMap<String, Arc<Kernel>>,
) -> Result<()> {
    ensure!(kernels.len() <= 256, "scene exceeds 256 compute shaders");
    for id in kernels.keys() {
        ensure!(
            scene
                .assets
                .get(id)
                .is_some_and(|a| a.kind == AssetKind::ComputeShader),
            "asset '{id}' is not a compute shader"
        );
    }
    ensure!(
        kernels.values().map(|k| k.source().len()).sum::<usize>() <= 32 * 1024 * 1024,
        "compute shader sources exceed 32 MiB"
    );
    Ok(())
}

impl SceneInstance {
    pub fn register_compute_kernels(
        &mut self,
        kernels: BTreeMap<String, Arc<Kernel>>,
    ) -> Result<()> {
        validate_kernels(&self.document, &kernels)?;
        self.compute_kernels = kernels;
        if let Some(state) = self.compute_state.get() {
            state.lock().unwrap_or_else(|e| e.into_inner()).kernels = self.compute_kernels.clone();
        }
        Ok(())
    }
    pub fn compute_kernels(&self) -> &BTreeMap<String, Arc<Kernel>> {
        &self.compute_kernels
    }
    /// The same CPU API used by scripts, available to compiled-in Rust systems.
    pub fn compute(&self) -> MutexGuard<'_, SceneCompute> {
        self.compute_cell()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }
    pub(crate) fn compute_cell(&self) -> &Arc<Mutex<SceneCompute>> {
        self.compute_state.get_or_init(|| {
            Arc::new(Mutex::new(SceneCompute::new(
                self.compute_capabilities.clone(),
                self.compute_kernels.clone(),
            )))
        })
    }
    /// Installing device limits does not allocate compute resources or queues in unused scenes.
    pub fn set_compute_capabilities(&mut self, capabilities: crate::compute::Capabilities) {
        self.compute_capabilities = capabilities;
        if let Some(state) = self.compute_state.get() {
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            let old_world = state.runtime.world();
            state
                .runtime
                .set_capabilities(self.compute_capabilities.clone());
            if state.runtime.world() != old_world {
                state.named_jobs.clear();
                state.materials.clear();
                state.reported.clear();
            }
        }
    }
    pub fn compute_capabilities(&self) -> &crate::compute::Capabilities {
        &self.compute_capabilities
    }
    pub fn compute_if_initialized(&self) -> Option<MutexGuard<'_, SceneCompute>> {
        self.compute_state
            .get()
            .map(|state| state.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn begin_compute_tick(&self, world: &mut crate::World) {
        let Some(mut state) = self.compute_if_initialized() else {
            return;
        };
        state.runtime.begin_tick();
        let failures: Vec<_> = state
            .runtime
            .jobs()
            .filter_map(|job| {
                if let crate::compute::JobState::Failed(error) = &job.state
                    && !state.reported.contains(&job.ticket)
                {
                    Some((
                        job.ticket,
                        job.owner.clone(),
                        job.label.clone(),
                        error.clone(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        let active: BTreeSet<_> = state.runtime.jobs().map(|job| job.ticket).collect();
        state.reported.retain(|ticket| active.contains(ticket));
        for (ticket, owner, label, error) in failures {
            state.reported.insert(ticket);
            bozzard_diagnostics::log(
                world,
                bozzard_diagnostics::Level::Error,
                "Compute",
                &format!("{label} request {}: {error}", ticket.serial()),
                bozzard_diagnostics::Location {
                    object: Some(owner.object),
                    attachment: Some(owner.attachment),
                    asset: label.split_once("::").map(|(asset, _)| asset.to_owned()),
                    ..Default::default()
                },
            );
        }
    }
}
