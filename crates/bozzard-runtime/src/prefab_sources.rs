//! Bounded authoring dependency resolution, shared by editor, runtime and cooker.
use anyhow::{Context, Result, ensure};
use bozzard_app::job::Progress;
use bozzard_scene::{AssetKind, Prefab};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct ResolvedPrefab {
    pub prefab: Prefab,
    /// Immutable bytes actually used by preparation; hosts reject changed sources
    /// before accepting an editor operation that depended on these snapshots.
    pub sources: BTreeMap<PathBuf, Arc<Vec<u8>>>,
}

pub fn load_prefab(path: &Path, progress: &Progress) -> Result<ResolvedPrefab> {
    let mut loader = Loader::new(progress);
    let prefab = loader.load(path)?;
    Ok(ResolvedPrefab {
        prefab,
        sources: loader.sources,
    })
}

pub fn resolve_prefab(prefab: Prefab, path: &Path, progress: &Progress) -> Result<ResolvedPrefab> {
    let mut loader = Loader::new(progress);
    // A source being edited must not be able to depend on itself through another file.
    loader.stack.insert(identity(path)?);
    let prefab = loader.resolve(prefab, path)?;
    Ok(ResolvedPrefab {
        prefab,
        sources: loader.sources,
    })
}

pub(super) struct Loader<'a> {
    progress: &'a Progress,
    sources: BTreeMap<PathBuf, Arc<Vec<u8>>>,
    cache: BTreeMap<PathBuf, Prefab>,
    stack: BTreeSet<PathBuf>,
    bytes: usize,
    objects: usize,
}
impl<'a> Loader<'a> {
    pub(super) fn new(progress: &'a Progress) -> Self {
        Self {
            progress,
            sources: Default::default(),
            cache: Default::default(),
            stack: Default::default(),
            bytes: 0,
            objects: 0,
        }
    }

    pub(super) fn load(&mut self, path: &Path) -> Result<Prefab> {
        self.progress.check()?;
        let path = identity(path)?;
        ensure!(
            !self.stack.contains(&path),
            "prefab dependency cycle at {}",
            path.display()
        );
        if let Some(prefab) = self.cache.get(&path) {
            return Ok(prefab.clone());
        }
        ensure!(self.stack.len() < 32, "prefab nesting exceeds 32 levels");
        ensure!(
            self.sources.len() < 1024,
            "prefab dependencies exceed 1024 files"
        );
        self.progress
            .stage(format!("Reading prefab {}", path.display()))?;
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .with_context(|| format!("reading prefab {}", path.display()))?
            .take((32 * 1024 * 1024 - self.bytes + 1) as u64)
            .read_to_end(&mut bytes)?;
        self.bytes += bytes.len();
        ensure!(
            self.bytes <= 32 * 1024 * 1024,
            "prefab dependencies exceed 32 MiB"
        );
        let prefab = Prefab::from_json(std::str::from_utf8(&bytes)?)?;
        self.sources.insert(path.clone(), Arc::new(bytes));
        self.stack.insert(path.clone());
        let prefab = self.resolve(prefab, &path)?;
        self.stack.remove(&path);
        self.objects += prefab.objects.len();
        ensure!(
            self.objects <= 100_000,
            "resolved prefab dependencies exceed 100000 objects"
        );
        self.cache.insert(path, prefab.clone());
        Ok(prefab)
    }

    fn resolve(&mut self, mut prefab: Prefab, path: &Path) -> Result<Prefab> {
        let directory = path.parent().unwrap_or(Path::new("."));
        // Base inheritance may introduce new nested references. Resolve it first,
        // then take the updated set of direct nested instances.
        let mut pending = prefab
            .base
            .as_ref()
            .map(|base| base.asset.clone())
            .into_iter()
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        loop {
            for asset in pending.drain(..) {
                if !visited.insert(asset.clone()) {
                    continue;
                }
                self.progress.check()?;
                let source = prefab
                    .assets
                    .get(&asset)
                    .context("missing prefab dependency")?;
                ensure!(
                    source.kind == AssetKind::Prefab,
                    "structural dependency is not a prefab"
                );
                let child_path = directory.join(&source.path);
                let mut child = self.load(&child_path)?;
                let mut mapping = BTreeMap::new();
                let mut bound = BTreeMap::new();
                for (id, dependency) in &child.assets {
                    let mut dependency = dependency.clone();
                    dependency.path = crate::relative_reference(
                        &child_path
                            .parent()
                            .unwrap_or(Path::new("."))
                            .join(&dependency.path),
                        directory,
                    )?;
                    let existing = prefab
                        .assets
                        .iter()
                        .find(|(_, a)| **a == dependency)
                        .map(|(id, _)| id.clone());
                    let id_new = existing.unwrap_or_else(|| {
                        let stem = format!("{asset}-{id}");
                        let mut key = stem.clone();
                        let mut suffix = 1;
                        while prefab.assets.contains_key(&key) {
                            key = format!("{stem}-{suffix}");
                            suffix += 1;
                        }
                        key
                    });
                    mapping.insert(id.clone(), id_new.clone());
                    prefab.assets.insert(id_new.clone(), dependency.clone());
                    bound.insert(id_new, dependency);
                }
                child.remap_assets(&mapping);
                child.assets = bound;
                prefab.refresh_dependency(&asset, &child)?;
                ensure!(
                    prefab.objects.len() <= 100_000,
                    "expanded prefab exceeds 100000 objects"
                );
            }
            pending = prefab
                .structural_dependencies()
                .into_iter()
                .filter(|id| !visited.contains(*id))
                .map(str::to_owned)
                .collect();
            if pending.is_empty() {
                break;
            }
        }
        prefab.validate()?;
        Ok(prefab)
    }
}

fn identity(path: &Path) -> Result<PathBuf> {
    if let Ok(path) = path.canonicalize() {
        return Ok(path);
    }
    let absolute = std::path::absolute(path)?;
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    loop {
        if let Ok(mut canonical) = ancestor.canonicalize() {
            for part in suffix.iter().rev() {
                canonical.push(part);
            }
            return Ok(crate::prefabs::normalize(&canonical));
        }
        suffix.push(
            ancestor
                .file_name()
                .context("prefab path has no existing ancestor")?
                .to_owned(),
        );
        ancestor = ancestor.parent().context("prefab path has no parent")?;
    }
}
