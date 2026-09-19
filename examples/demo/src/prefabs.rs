//! Load spawn templates once at startup; fixed ticks never perform filesystem I/O.
use anyhow::{Context, Result, ensure};
use bozzard_scene::{AssetKind, Prefab, Scene};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

pub(super) fn load(
    document: &Scene,
    path: Option<&Path>,
    progress: &bozzard_app::job::Progress,
) -> Result<(Scene, BTreeMap<String, Prefab>)> {
    progress.check()?;
    document.validate()?;
    let mut scene = document.clone();
    let mut templates = BTreeMap::new();
    let spawnable = document.spawn_asset_ids();
    if spawnable.is_empty() {
        return Ok((scene, templates));
    }
    let root = path.and_then(Path::parent).unwrap_or(Path::new("."));
    let mut count = 0;
    let mut loader = crate::prefab_sources::Loader::new(progress);
    let mut pending: BTreeMap<_, _> = spawnable
        .into_iter()
        .map(|asset| {
            let source = document.assets[&asset].clone();
            (asset, source)
        })
        .collect();
    while let Some((asset, source)) = pending.pop_first() {
        progress.stage(format!("Loading prefab {asset}"))?;
        if templates.contains_key(&asset) {
            continue;
        }
        ensure!(templates.len() < 1024, "spawn templates exceed 1024 files");
        let mut prefab = loader
            .load(&root.join(&source.path))
            .with_context(|| format!("loading spawn prefab '{asset}'"))?;
        count += prefab.objects.len();
        ensure!(count <= 100_000, "spawn templates exceed 100000 objects");
        let directory = Path::new(&source.path).parent().unwrap_or(Path::new("."));
        let mut mapping = BTreeMap::new();
        let mut bound = BTreeMap::new();
        for (id, dependency) in &prefab.assets {
            let mut dependency = dependency.clone();
            dependency.path = normalize(&directory.join(&dependency.path))
                .to_str()
                .context("prefab path is not UTF-8")?
                .replace('\\', "/");
            let existing = scene
                .assets
                .iter()
                .find(|(_, a)| {
                    a.kind == dependency.kind
                        && normalize(Path::new(&a.path)) == normalize(Path::new(&dependency.path))
                })
                .map(|(id, _)| id.clone());
            let new = existing.unwrap_or_else(|| {
                let base = format!("{asset}-{id}");
                let mut candidate = base.clone();
                let mut suffix = 1;
                while scene.assets.contains_key(&candidate) {
                    candidate = format!("{base}-{suffix}");
                    suffix += 1;
                }
                candidate
            });
            mapping.insert(id.clone(), new.clone());
            scene.assets.insert(new.clone(), dependency.clone());
            if dependency.kind == AssetKind::Prefab {
                pending.insert(new.clone(), dependency.clone());
            }
            bound.insert(new, dependency);
        }
        prefab.remap_assets(&mapping);
        prefab.assets = bound;
        templates.insert(asset.clone(), prefab);
    }
    scene.validate()?;
    Ok((scene, templates))
}

pub(super) fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if result.file_name().is_some_and(|n| n != "..") => {
                result.pop();
            }
            _ => result.push(part.as_os_str()),
        }
    }
    result
}
