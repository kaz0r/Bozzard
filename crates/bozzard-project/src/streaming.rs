//! File/content acquisition for the scene core's cancellable publication boundary.
use crate::content::{ContentStore, ResolvedContent, default_cache_directory, load_catalog};
use anyhow::{Context, Result, ensure};
use bozzard_app::{
    World,
    job::{Job, Progress},
};
use bozzard_assets::AssetStore;
use bozzard_demo::relative_reference as relative;
use bozzard_scene::{
    Scene,
    scene_loading::{PreparedScene, SceneLoadPlan, SceneLoader, SceneLoaderHandle, SceneSource},
};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The scene and this decoded catalog are published at the same tick boundary.
/// Hosts adopt a changed generation before extracting their next rendered/audio frame.
#[derive(Clone)]
pub struct SceneAssets {
    pub generation: u64,
    pub store: AssetStore,
    mounts: BTreeMap<String, ResolvedContent>,
}

struct Loader {
    root: PathBuf,
    cache: PathBuf,
}

pub fn install(world: &mut World, scene_path: &Path, assets: &AssetStore) -> Result<()> {
    install_with_cache(world, scene_path, assets, default_cache_directory()?)
}

pub fn install_with_cache(
    world: &mut World,
    scene_path: &Path,
    assets: &AssetStore,
    cache: PathBuf,
) -> Result<()> {
    let root = std::path::absolute(scene_path)?
        .parent()
        .context("scene path has no parent")?
        .to_owned();
    world.insert_resource(SceneAssets {
        generation: 0,
        store: assets.clone(),
        mounts: BTreeMap::new(),
    });
    world.insert_resource(SceneLoaderHandle(Arc::new(Loader { root, cache })));
    Ok(())
}

impl SceneLoader for Loader {
    fn start(&self, plan: SceneLoadPlan, world: &World) -> Result<Job<PreparedScene>> {
        let current = world
            .resource::<SceneAssets>()
            .context("runtime asset catalog missing")?
            .clone();
        let root = self.root.clone();
        let cache = self.cache.clone();
        Job::start("Acquiring scene", move |progress| {
            acquire(plan, current, &root, cache, &progress)
        })
    }
}

fn acquire(
    plan: SceneLoadPlan,
    mut current: SceneAssets,
    root: &Path,
    cache: PathBuf,
    progress: &Progress,
) -> Result<PreparedScene> {
    if plan.checkpoint_document().is_some() {
        return restore(plan, current, root, progress);
    }
    progress.report(0, 4, format!("Acquiring {}", plan.name()))?;
    let source = plan.source().context("runtime scene source missing")?;
    let path = match source {
        SceneSource::File { path } => root.join(path),
        SceneSource::Content { catalog, address } => {
            let catalog = if catalog.starts_with("https://") || catalog.starts_with("http://") {
                catalog.clone()
            } else {
                root.join(catalog)
                    .to_str()
                    .context("catalog path is not UTF-8")?
                    .to_owned()
            };
            let acquisition = progress.subtask(0., 0.25)?;
            let catalog = load_catalog(&catalog, &acquisition)?;
            let resolved = ContentStore::new(cache).resolve(&catalog, address, &acquisition)?;
            let path = resolved.scene_path()?;
            current.mounts.insert(plan.name().into(), resolved);
            path
        }
    };
    progress.stage(format!("Reading scene {}", path.display()))?;
    let mut json = String::new();
    std::fs::File::open(&path)
        .with_context(|| format!("opening scene {}", path.display()))?
        .take(64 * 1024 * 1024 + 1)
        .read_to_string(&mut json)?;
    ensure!(json.len() <= 64 * 1024 * 1024, "scene file exceeds 64 MiB");
    progress.check()?;
    let document = Scene::from_json(&json)?;
    let bozzard_demo::RuntimeSceneFiles {
        mut scene,
        mut templates,
        sources,
        kernels,
    } = bozzard_demo::prepare_runtime_files(&document, Some(&path), progress)?;
    rebase(
        &mut scene,
        path.parent().context("scene parent")?,
        root,
        plan.base(),
    )?;
    // Materials are addressed through typed bindings, so independently authored
    // scenes may safely reuse generated names such as material-1. Other asset
    // kinds retain their global-ID contract (scripts can address those by name).
    let material_ids = bind_material_ids(&mut scene, plan.base());
    for prefab in templates.values_mut() {
        for (id, asset) in &mut prefab.assets {
            *asset = scene.assets[material_ids.get(id).unwrap_or(id)].clone();
        }
        prefab.remap_assets(&material_ids);
        prefab.assets = std::mem::take(&mut prefab.assets)
            .into_iter()
            .map(|(id, source)| (material_ids.get(&id).cloned().unwrap_or(id), source))
            .collect();
    }
    progress.report(1, 4, "Decoding scene assets")?;
    let mut assets = plan.base().assets.clone();
    for (id, asset) in &scene.assets {
        ensure!(
            assets.get(id).is_none_or(|old| old == asset),
            "scene asset conflict '{id}'"
        );
        assets.insert(id.clone(), asset.clone());
    }
    current.store = current.store.for_catalog(root, &assets)?;
    current.store.refresh_with(progress)?;
    current.store.require_ready()?;
    current.store.bake_audio_metadata(&mut scene)?;
    let mut prepared = plan
        .with_acquired_scene(scene)?
        .prepare(&progress.subtask(0.5, 0.9)?)?;
    prepared.bind_catalog(templates, sources, kernels)?;
    current
        .store
        .validate_scene_resources(prepared.document())?;
    current.generation = current
        .generation
        .checked_add(1)
        .context("asset generation exhausted")?;
    progress.report(4, 4, "Scene and assets ready")?;
    prepared.publish_resource(current);
    Ok(prepared)
}

fn bind_material_ids(scene: &mut Scene, base: &Scene) -> BTreeMap<String, String> {
    let mut used: std::collections::BTreeSet<_> = scene
        .assets
        .keys()
        .chain(base.assets.keys())
        .cloned()
        .collect();
    let mut mapping = BTreeMap::new();
    let mut serial = 1;
    for (id, asset) in &scene.assets {
        if asset.kind != bozzard_scene::AssetKind::Material
            || base.assets.get(id).is_none_or(|old| old == asset)
        {
            continue;
        }
        let bound = base
            .assets
            .iter()
            .find(|(target, old)| {
                *old == asset
                    && scene
                        .assets
                        .get(*target)
                        .is_none_or(|incoming| incoming == asset)
            })
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| {
                loop {
                    let candidate = format!("streamed-material-{serial}");
                    serial += 1;
                    if used.insert(candidate.clone()) {
                        break candidate;
                    }
                }
            });
        mapping.insert(id.clone(), bound);
    }
    fn remap(scene: &mut Scene, mapping: &BTreeMap<String, String>) {
        for object in &mut scene.objects {
            object.remap_assets(mapping);
        }
        for prefab in scene.prefabs.values_mut() {
            for object in &mut prefab.baseline {
                object.remap_assets(mapping);
            }
        }
        scene.assets = std::mem::take(&mut scene.assets)
            .into_iter()
            .map(|(id, asset)| (mapping.get(&id).cloned().unwrap_or(id), asset))
            .collect();
        for child in scene.runtime_scenes.values_mut() {
            remap(Arc::make_mut(child), mapping);
        }
    }
    if !mapping.is_empty() {
        remap(scene, &mapping);
    }
    mapping
}

fn restore(
    plan: SceneLoadPlan,
    mut current: SceneAssets,
    root: &Path,
    progress: &Progress,
) -> Result<PreparedScene> {
    let document = plan.checkpoint_document().context("saved scene missing")?;
    progress.report(0, 4, "Preparing saved scene dependencies")?;
    let files =
        bozzard_demo::prepare_runtime_files(document, Some(&root.join("scene.json")), progress)?;
    ensure!(
        files.scene.assets == document.assets,
        "saved prefab dependencies changed; restore the matching project/content version"
    );
    current.store = current.store.for_catalog(root, &document.assets)?;
    progress.report(1, 4, "Decoding saved scene assets")?;
    current.store.refresh_with(progress)?;
    current.store.require_ready()?;
    current.store.validate_scene_resources(document)?;
    let mut prepared = plan.prepare(&progress.subtask(0.5, 0.9)?)?;
    prepared.bind_catalog(files.templates, files.sources, files.kernels)?;
    current.generation = current
        .generation
        .checked_add(1)
        .context("asset generation exhausted")?;
    progress.report(4, 4, "Saved scene and assets ready")?;
    prepared.publish_resource(current);
    Ok(prepared)
}

fn rebase(scene: &mut Scene, source: &Path, root: &Path, base: &Scene) -> Result<()> {
    for (id, asset) in &mut scene.assets {
        let target = source.join(&asset.path);
        asset.path = relative(&target, root)?;
        if let Some(old) = base.assets.get(id)
            && old.kind == asset.kind
            && root
                .join(&old.path)
                .canonicalize()
                .ok()
                .zip(target.canonicalize().ok())
                .is_some_and(|(a, b)| a == b)
        {
            *asset = old.clone();
        }
    }
    for source_ref in scene.runtime_scene_sources.values_mut() {
        match source_ref {
            SceneSource::File { path } => *path = relative(&source.join(&*path), root)?,
            SceneSource::Content { catalog, .. }
                if !catalog.starts_with("https://") && !catalog.starts_with("http://") =>
            {
                *catalog = relative(&source.join(&*catalog), root)?;
            }
            _ => {}
        }
    }
    for child in scene.runtime_scenes.values_mut() {
        for (id, asset) in &mut Arc::make_mut(child).assets {
            *asset = scene.assets[id].clone();
        }
    }
    scene.validate()
}
