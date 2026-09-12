//! Load spawn templates once at startup; fixed ticks never perform filesystem I/O.
use anyhow::{Context, Result, ensure};
use bozzard_scene::{AssetKind, Prefab, Scene};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub(super) fn load(
    document: &Scene,
    path: Option<&Path>,
) -> Result<(Scene, BTreeMap<String, Prefab>)> {
    document.validate()?;
    let mut scene = document.clone();
    let mut templates = BTreeMap::new();
    if !document
        .objects
        .iter()
        .flat_map(|o| &o.blueprints)
        .flat_map(|b| &b.graph.nodes)
        .any(|n| n.kind == bozzard_scene::blueprint::NodeKind::SpawnPrefab)
    {
        return Ok((scene, templates));
    }
    let root = path.and_then(Path::parent).unwrap_or(Path::new("."));
    let mut count = 0;
    let mut bytes = 0;
    let mut pending: BTreeMap<_, _> = document
        .objects
        .iter()
        .flat_map(|o| &o.blueprints)
        .filter(|b| b.enabled)
        .flat_map(|b| &b.graph.nodes)
        .filter(|n| {
            n.kind == bozzard_scene::blueprint::NodeKind::SpawnPrefab && !n.prefab.is_empty()
        })
        .map(|n| (n.prefab.clone(), document.assets[&n.prefab].clone()))
        .collect();
    while let Some((asset, source)) = pending.pop_first() {
        if templates.contains_key(&asset) {
            continue;
        }
        ensure!(templates.len() < 1024, "spawn templates exceed 1024 files");
        let mut json = String::new();
        std::fs::File::open(root.join(&source.path))
            .with_context(|| format!("loading spawn prefab '{asset}'"))?
            .take(32 * 1024 * 1024 + 1)
            .read_to_string(&mut json)?;
        bytes += json.len();
        ensure!(bytes <= 32 * 1024 * 1024, "spawn templates exceed 32 MiB");
        let mut prefab = Prefab::from_json(&json)?;
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
        for object in &mut prefab.objects {
            object.remap_assets(&mapping);
        }
        prefab.assets = bound;
        templates.insert(asset.clone(), prefab);
    }
    scene.validate()?;
    Ok((scene, templates))
}

fn normalize(path: &Path) -> PathBuf {
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
