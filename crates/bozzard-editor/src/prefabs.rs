//! Transactional prefab authoring. Workers prepare; only acceptance writes the source and scene.
use super::*;
use bozzard_assets::job::{Job, Progress};
use bozzard_scene::{Prefab, PrefabInstance};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Debug)]
pub enum PrefabCommand {
    Create,
    Variant,
    Instantiate {
        asset: String,
        position: Option<[f32; 3]>,
    },
    Apply,
    Refresh {
        asset: String,
    },
}

pub struct PreparedPrefab {
    scene: Scene,
    assets: AssetStore,
    path: PathBuf,
    revision: u64,
    asset_revision: u64,
    selected: Option<String>,
    write: Option<SourceWrite>,
    progress: Progress,
    read: Option<SourceRead>,
    pub asset: String,
    pub label: String,
    pub layer: Option<Layer>,
}
struct SourceRead {
    path: PathBuf,
    bytes: Vec<u8>,
    dependencies: BTreeMap<PathBuf, std::sync::Arc<Vec<u8>>>,
}
struct Preparation {
    asset: String,
    label: &'static str,
    write: Option<SourceWrite>,
    read: Option<SourceRead>,
}
pub(super) struct SourceWrite {
    reads: BTreeMap<PathBuf, std::sync::Arc<Vec<u8>>>,
    path: PathBuf,
    json: String,
    previous: Option<Vec<u8>>,
}

#[derive(Clone)]
pub(super) struct PrefabSource {
    pub root: String,
    base: Option<bozzard_scene::PrefabBase>,
    disk: std::sync::Arc<Vec<u8>>,
}
impl PrefabSource {
    pub(super) fn validate_assets(&self, scene: &Scene) -> Result<()> {
        if let Some(base) = &self.base {
            ensure!(
                scene
                    .assets
                    .get(&base.asset)
                    .is_some_and(|a| a.kind == AssetKind::Prefab),
                "The variant's base prefab is required"
            );
            for (id, kind) in base
                .baseline
                .iter()
                .flat_map(Object::asset_dependencies)
                .chain(
                    base.nested
                        .values()
                        .map(|link| (link.asset.as_str(), AssetKind::Prefab)),
                )
            {
                ensure!(
                    scene.assets.get(id).is_some_and(|a| a.kind == kind),
                    "Asset '{id}' is required by the variant's inherited baseline"
                );
            }
        }
        Ok(())
    }
    pub(super) fn prepare_save(
        &self,
        scene: &Scene,
        original: &Path,
        target: &Path,
        progress: &Progress,
    ) -> Result<(SourceWrite, Self)> {
        ensure!(
            is_prefab_path(target),
            "Prefab source filenames must end with .prefab.json"
        );
        ensure!(
            read_bytes(original)? == *self.disk,
            "Prefab source changed on disk; reopen it before saving"
        );
        let prefab = Prefab {
            version: 1,
            name: scene.name.clone(),
            root: self.root.clone(),
            objects: scene.objects.clone(),
            assets: scene.assets.clone(),
            nested: scene.prefabs.clone(),
            base: self.base.clone(),
        };
        let json = prefab.to_json()?;
        let reads = bozzard_demo::resolve_prefab(prefab, target, progress)?.sources;
        let previous = if target == original
            || target
                .canonicalize()
                .ok()
                .zip(original.canonicalize().ok())
                .is_some_and(|(a, b)| a == b)
        {
            Some((*self.disk).clone())
        } else if target.exists() {
            Some(read_bytes(target)?)
        } else {
            None
        };
        let updated = Self {
            disk: std::sync::Arc::new(json.as_bytes().to_vec()),
            ..self.clone()
        };
        Ok((
            SourceWrite {
                reads,
                path: target.to_path_buf(),
                json,
                previous,
            },
            updated,
        ))
    }
}

pub(super) fn is_prefab_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".prefab.json"))
}

pub(super) fn load_source(path: PathBuf, progress: &Progress) -> Result<LoadedScene> {
    let resolved = bozzard_demo::load_prefab(&path, progress)?;
    let prefab = resolved.prefab;
    let mut scene = prefab.authoring_scene();
    let mut assets = AssetStore::new(root(&path), &scene.assets)?;
    assets.load_pending_with(progress)?;
    assets.require_ready()?;
    assets.validate_scene_resources(&scene)?;
    assets.bake_audio_metadata(&mut scene)?;
    let disk = resolved
        .sources
        .get(&path.canonicalize()?)
        .context("prefab source snapshot missing")?
        .clone();
    Ok(LoadedScene {
        scene,
        path,
        assets,
        prefab_source: Some(PrefabSource {
            root: prefab.root,
            base: prefab.base,
            disk,
        }),
    })
}

impl Editor {
    pub(super) fn link_prefab(&mut self, path: &Path, progress: &Progress) -> Result<String> {
        progress.stage("Linking prefab source")?;
        let bytes = read_bytes(path)?;
        let prefab = Prefab::from_json(std::str::from_utf8(&bytes)?)?;
        let relative = relative_asset(path, root(&self.path))?;
        if let Some((id, _)) = self.scene.assets.iter().find(|(_, a)| {
            a.kind == AssetKind::Prefab
                && relative_asset(&root(&self.path).join(&a.path), root(&self.path))
                    .ok()
                    .as_ref()
                    == Some(&relative)
        }) {
            return Ok(id.clone());
        }
        let mut used = self.scene.assets.keys().cloned().collect();
        let id = fresh_id(&mut used, &format!("{}-prefab", slug(&prefab.name)));
        let mut scene = self.scene.clone();
        scene.assets.insert(
            id.clone(),
            AssetSource {
                kind: AssetKind::Prefab,
                path: relative,
            },
        );
        std::fs::create_dir_all(root(&self.path))?;
        self.apply("Link prefab asset", scene)?;
        progress.check()?;
        Ok(id)
    }

    pub fn selected_prefab_root(&self) -> Option<&str> {
        let id = self.selected.as_ref()?;
        self.scene
            .prefabs
            .iter()
            .find(|(_, link)| link.members.values().any(|m| m == id))
            .map(|(root, _)| root.as_str())
    }
    pub fn unpack_prefab(&mut self) -> Result<()> {
        let root = self
            .selected_prefab_root()
            .context("Select a prefab instance")?
            .to_owned();
        let mut scene = self.scene.clone();
        scene.prefabs.remove(&root);
        self.finish_gesture();
        self.apply("Unpack prefab", scene)
    }
    pub fn prefab_job(&mut self, command: PrefabCommand) -> Result<Job<PreparedPrefab>> {
        ensure!(self.play.is_none(), "Stop Play before editing prefabs");
        ensure!(
            self.selected_surface().is_none()
                || matches!(
                    command,
                    PrefabCommand::Instantiate { .. } | PrefabCommand::Refresh { .. }
                ),
            "Select the whole object first"
        );
        self.finish_gesture();
        let placing = matches!(command, PrefabCommand::Instantiate { .. });
        let scene = self.scene.clone();
        let path = self.path.clone();
        let assets = self.assets.clone();
        let selected = self.selected.clone();
        let revision = self.revision;
        let asset_revision = self.asset_revision;
        Job::start("Preparing prefab", move |progress| {
            let mut next = Editor::from_loaded(scene, path.clone(), assets);
            next.selected = selected;
            let Preparation {
                asset,
                label,
                write,
                read,
            } = next.prepare_prefab(command, &progress)?;
            progress.check()?;
            let layer = if placing {
                let ids = subtree(
                    &next.scene,
                    next.selected
                        .as_deref()
                        .context("placed prefab selection missing")?,
                );
                next.scene
                    .objects
                    .iter()
                    .filter(|o| ids.contains(&o.id))
                    .find_map(|o| o.drawable.as_ref().map(|d| d.layer))
            } else {
                None
            };
            Ok(PreparedPrefab {
                scene: next.scene,
                assets: next.assets,
                path,
                revision,
                asset_revision,
                selected: next.selected,
                asset,
                label: label.into(),
                layer,
                write,
                read,
                progress,
            })
        })
    }
    pub fn accept_prefab(&mut self, prepared: PreparedPrefab) -> Result<String> {
        prepared.progress.check()?;
        ensure!(
            self.play.is_none()
                && self.path == prepared.path
                && self.revision == prepared.revision
                && self.asset_revision == prepared.asset_revision,
            "Scene or assets changed while preparing the prefab; try again"
        );
        prepared.scene.validate()?;
        prepared.assets.require_ready()?;
        if let Some(read) = &prepared.read {
            ensure!(
                read_bytes(&read.path)? == read.bytes,
                "Prefab source changed while preparing; try again"
            );
            for (path, bytes) in &read.dependencies {
                ensure!(
                    read_bytes(path)? == **bytes,
                    "Prefab dependency changed while preparing; try again: {}",
                    path.display()
                );
            }
        }
        if let Some(write) = &prepared.write {
            publish(write)?;
        }
        let catalog_changed = self.scene.assets != prepared.scene.assets;
        self.record(Change {
            restore_file: None,
            label: prepared.label,
            scene: self.scene.clone(),
            assets: catalog_changed.then(|| self.assets.clone()),
        });
        self.scene = prepared.scene;
        self.assets = prepared.assets;
        self.selected = prepared.selected;
        self.revision += 1;
        self.asset_revision += 1;
        self.repair_selection();
        Ok(prepared.asset)
    }

    fn prepare_prefab(
        &mut self,
        command: PrefabCommand,
        progress: &Progress,
    ) -> Result<Preparation> {
        let mut scene = self.scene.clone();
        let mut write = None;
        let mut read = None;
        let (asset, label) = match command {
            PrefabCommand::Create => {
                let selected = self
                    .selected
                    .as_ref()
                    .context("Select an object hierarchy first")?;
                let ids = subtree(&scene, selected);
                ensure!(
                    !scene.prefabs.contains_key(selected),
                    "Use Create variant for a linked prefab root"
                );
                ensure!(
                    scene.prefabs.values().all(|link| {
                        let count = link.members.values().filter(|id| ids.contains(*id)).count();
                        count == 0 || count == link.members.len()
                    }),
                    "Select a complete hierarchy containing whole prefab instances"
                );
                let name = scene
                    .objects
                    .iter()
                    .find(|o| o.id == *selected)
                    .context("selected object missing")?
                    .name
                    .clone();
                let slug = slug(&name);
                let mut n = 1;
                let (asset, relative) = loop {
                    let asset = format!("{slug}-prefab-{n}");
                    let relative = format!("assets/{asset}.prefab.json");
                    if !scene.assets.contains_key(&asset)
                        && !root(&self.path).join(&relative).exists()
                    {
                        break (asset, relative);
                    }
                    n += 1;
                };
                let target = root(&self.path).join(&relative);
                let prefab = capture(&scene, selected, &self.path, &target)?;
                let json = prefab.to_json()?;
                let baseline = scene
                    .objects
                    .iter()
                    .filter(|o| ids.contains(&o.id))
                    .cloned()
                    .map(|mut o| {
                        if o.id == *selected {
                            o.parent = None;
                            o.transform = prefab
                                .objects
                                .iter()
                                .find(|p| p.id == prefab.root)
                                .unwrap()
                                .transform;
                        }
                        o
                    })
                    .collect();
                scene.assets.insert(
                    asset.clone(),
                    AssetSource {
                        kind: AssetKind::Prefab,
                        path: relative,
                    },
                );
                // The new outer instance owns its expanded hierarchy. Its source
                // retains the direct nested links, without overlapping scene owners.
                scene.prefabs.retain(|root, _| !ids.contains(root));
                scene.prefabs.insert(
                    selected.clone(),
                    PrefabInstance {
                        asset: asset.clone(),
                        members: ids.iter().map(|id| (id.clone(), id.clone())).collect(),
                        baseline,
                    },
                );
                write = Some(SourceWrite {
                    reads: Default::default(),
                    path: target,
                    json,
                    previous: None,
                });
                (asset, "Create prefab")
            }
            PrefabCommand::Variant => {
                let id = self
                    .selected_prefab_root()
                    .context("Select a prefab instance to create a variant")?
                    .to_owned();
                let link = scene.prefabs[&id].clone();
                let (mut base, snapshot) = read_asset(&scene, &self.path, &link.asset, progress)?;
                let mut refreshed = scene.clone();
                merge_instances(
                    &mut refreshed,
                    &base,
                    &link.asset,
                    &snapshot.path,
                    &self.path,
                )?;
                ensure!(
                    refreshed.prefabs[&id].baseline == link.baseline,
                    "The base prefab changed; refresh instances before creating a variant"
                );
                let name = format!("{} variant", base.name);
                let (asset, relative) = new_source(&scene, &self.path, &name);
                let target = root(&self.path).join(&relative);
                let mut variant = capture(&scene, &id, &self.path, &target)?;
                let inverse: BTreeMap<_, _> = link
                    .members
                    .iter()
                    .map(|(a, b)| (b.clone(), a.clone()))
                    .collect();
                for object in &mut variant.objects {
                    if object.id == id {
                        object.transform =
                            link.baseline.iter().find(|o| o.id == id).unwrap().transform;
                    }
                    object.remap_ids(&inverse);
                }
                variant.root = inverse[&id].clone();
                variant.name = name;
                let mapping = definition_assets(&mut variant, &base, &snapshot.path, &target)?;
                base.remap_assets(&mapping);
                let mut used = variant.assets.keys().cloned().collect();
                let base_asset = fresh_id(&mut used, "base");
                variant.assets.insert(
                    base_asset.clone(),
                    AssetSource {
                        kind: AssetKind::Prefab,
                        path: relative_asset(&snapshot.path, root(&target))?,
                    },
                );
                variant.nested = base.nested.clone();
                variant.base = Some(bozzard_scene::PrefabBase {
                    asset: base_asset,
                    baseline: base.objects,
                    nested: base.nested,
                });
                let json = variant.to_json()?;
                scene.assets.insert(
                    asset.clone(),
                    AssetSource {
                        kind: AssetKind::Prefab,
                        path: relative,
                    },
                );
                let mapping = bind_assets(&mut scene, &variant, &target, &self.path)?;
                let baseline = remap(&variant, &link.members, &mapping);
                scene.prefabs.insert(
                    id.clone(),
                    PrefabInstance {
                        asset: asset.clone(),
                        members: link.members,
                        baseline,
                    },
                );
                self.selected = Some(id);
                read = Some(snapshot);
                write = Some(SourceWrite {
                    reads: Default::default(),
                    path: target,
                    json,
                    previous: None,
                });
                (asset, "Create prefab variant")
            }
            PrefabCommand::Instantiate { asset, position } => {
                let (prefab, snapshot) = read_asset(&scene, &self.path, &asset, progress)?;
                let target = snapshot.path.clone();
                read = Some(snapshot);
                let mut used: BTreeSet<_> = scene.objects.iter().map(|o| o.id.clone()).collect();
                let members: BTreeMap<_, _> = prefab
                    .objects
                    .iter()
                    .map(|o| (o.id.clone(), fresh_id(&mut used, "prefab")))
                    .collect();
                let sources = bind_assets(&mut scene, &prefab, &target, &self.path)?;
                let baseline = remap(&prefab, &members, &sources);
                let id = members[&prefab.root].clone();
                scene.objects.extend(baseline.clone());
                if let Some(position) = position {
                    scene
                        .objects
                        .iter_mut()
                        .find(|o| o.id == id)
                        .unwrap()
                        .transform
                        .translation = position;
                }
                scene.prefabs.insert(
                    id.clone(),
                    PrefabInstance {
                        asset: asset.clone(),
                        members,
                        baseline,
                    },
                );
                self.selected = Some(id);
                (asset, "Place prefab")
            }
            PrefabCommand::Refresh { asset } => {
                let (prefab, snapshot) = read_asset(&scene, &self.path, &asset, progress)?;
                let target = snapshot.path.clone();
                read = Some(snapshot);
                merge_instances(&mut scene, &prefab, &asset, &target, &self.path)?;
                (asset, "Refresh prefab instances")
            }
            PrefabCommand::Apply => {
                let id = self
                    .selected_prefab_root()
                    .context("Select a prefab instance")?
                    .to_owned();
                let link = scene.prefabs[&id].clone();
                let (previous_prefab, snapshot) =
                    read_asset(&scene, &self.path, &link.asset, progress)?;
                let target = snapshot.path.clone();
                let previous = snapshot.bytes.clone();
                read = Some(snapshot);
                // Refuse to overwrite source changes the selected instance has not seen.
                let mut refreshed = scene.clone();
                merge_instances(
                    &mut refreshed,
                    &previous_prefab,
                    &link.asset,
                    &target,
                    &self.path,
                )?;
                ensure!(
                    refreshed.prefabs[&id].baseline == link.baseline,
                    "The prefab source changed. Refresh instances before applying your edits"
                );
                let mut prefab = capture(&scene, &id, &self.path, &target)?;
                let inverse: BTreeMap<_, _> = link
                    .members
                    .iter()
                    .map(|(a, b)| (b.clone(), a.clone()))
                    .collect();
                for object in &mut prefab.objects {
                    if object.id == id {
                        object.transform =
                            link.baseline.iter().find(|o| o.id == id).unwrap().transform;
                    }
                    object.remap_ids(&inverse);
                }
                prefab.root = inverse[&id].clone();
                let mut inherited = previous_prefab;
                let mapping = definition_assets(&mut prefab, &inherited, &target, &target)?;
                inherited.remap_assets(&mapping);
                prefab.nested = inherited.nested;
                prefab.base = inherited.base;
                let json = prefab.to_json()?;
                merge_instances(&mut scene, &prefab, &link.asset, &target, &self.path)?;
                write = Some(SourceWrite {
                    reads: Default::default(),
                    path: target,
                    json,
                    previous: Some(previous),
                });
                (link.asset, "Apply prefab to source")
            }
        };
        progress.stage("Preparing prefab dependencies")?;
        scene.validate()?;
        let mut assets = self.assets.for_catalog(root(&self.path), &scene.assets)?;
        if let Some(write) = &write {
            assets.stage_prefab(&asset, &write.json)?;
        }
        // Refresh the selected definition even if a watcher hasn't seen it yet.
        if let Some(read) = &read
            && write.is_none()
        {
            assets.stage_prefab(&asset, std::str::from_utf8(&read.bytes)?)?;
        }
        assets.load_pending_with(progress)?;
        progress.check()?;
        assets.require_ready()?;
        assets.validate_scene_resources(&scene)?;
        self.scene = scene;
        self.assets = assets;
        Ok(Preparation {
            asset,
            label,
            write,
            read,
        })
    }
}

fn slug(name: &str) -> String {
    let result: String = name
        .chars()
        .take(60)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if result.is_empty() {
        "object".into()
    } else {
        result
    }
}

fn new_source(scene: &Scene, path: &Path, name: &str) -> (String, String) {
    let stem = slug(name);
    for index in 1u64.. {
        let asset = format!("{stem}-prefab-{index}");
        let relative = format!("assets/{asset}.prefab.json");
        if !scene.assets.contains_key(&asset) && !root(path).join(&relative).exists() {
            return (asset, relative);
        }
    }
    unreachable!("prefab filename space exhausted")
}

/// Rebase inherited metadata's asset IDs into a captured definition's catalog.
fn definition_assets(
    target: &mut Prefab,
    source: &Prefab,
    source_path: &Path,
    target_path: &Path,
) -> Result<BTreeMap<String, String>> {
    let mut mapping = BTreeMap::new();
    let mut used: BTreeSet<_> = target.assets.keys().cloned().collect();
    for (id, asset) in &source.assets {
        let mut asset = asset.clone();
        asset.path = relative_asset(&root(source_path).join(&asset.path), root(target_path))?;
        let bound = target
            .assets
            .iter()
            .find(|(_, a)| **a == asset)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| {
                if used.insert(id.clone()) {
                    id.clone()
                } else {
                    fresh_id(&mut used, id)
                }
            });
        target.assets.insert(bound.clone(), asset);
        mapping.insert(id.clone(), bound);
    }
    Ok(mapping)
}
fn fresh_id(used: &mut BTreeSet<String>, prefix: &str) -> String {
    let mut n = used.len() + 1;
    loop {
        let id = format!("{prefix}-{n}");
        if used.insert(id.clone()) {
            return id;
        }
        n += 1;
    }
}
fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 32 * 1024 * 1024, "prefab exceeds 32 MiB");
    Ok(bytes)
}
fn read_asset(
    scene: &Scene,
    path: &Path,
    asset: &str,
    progress: &Progress,
) -> Result<(Prefab, SourceRead)> {
    let source = scene.assets.get(asset).context("prefab asset missing")?;
    ensure!(source.kind == AssetKind::Prefab, "asset is not a prefab");
    let target = root(path).join(&source.path);
    let resolved = bozzard_demo::load_prefab(&target, progress)?;
    let mut dependencies = resolved.sources;
    let bytes = dependencies
        .remove(&target.canonicalize()?)
        .context("prefab source snapshot missing")?;
    Ok((
        resolved.prefab,
        SourceRead {
            path: target,
            bytes: std::sync::Arc::unwrap_or_clone(bytes),
            dependencies,
        },
    ))
}

// Lexical paths also support preparing a new prefab before its directory exists.
pub(crate) fn relative_asset(path: &Path, destination: &Path) -> Result<String> {
    fn normalized(path: &Path) -> Result<PathBuf> {
        let absolute = std::path::absolute(path)?;
        // Canonicalize the closest existing ancestor, preserving symlink/Windows root semantics.
        let mut ancestor = absolute.as_path();
        let mut suffix = Vec::new();
        loop {
            if let Ok(mut result) = ancestor.canonicalize() {
                for component in suffix.iter().rev() {
                    result.push(component);
                }
                return Ok(result);
            }
            suffix.push(
                ancestor
                    .file_name()
                    .context("cannot resolve prefab asset root")?,
            );
            ancestor = ancestor
                .parent()
                .context("cannot resolve prefab asset root")?;
        }
    }
    let target = normalized(path)?;
    let dest = normalized(destination)?;
    let a: Vec<_> = dest.components().collect();
    let b: Vec<_> = target.components().collect();
    let common = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    ensure!(
        common > 0 && a.first() == b.first(),
        "prefab assets must be on the same filesystem root"
    );
    let mut result = PathBuf::new();
    for _ in common..a.len() {
        result.push("..");
    }
    for c in &b[common..] {
        result.push(c);
    }
    Ok(result
        .to_str()
        .context("non UTF-8 asset path")?
        .replace('\\', "/"))
}
fn capture(scene: &Scene, id: &str, scene_path: &Path, target: &Path) -> Result<Prefab> {
    let ids = subtree(scene, id);
    let mut objects: Vec<_> = scene
        .objects
        .iter()
        .filter(|o| ids.contains(&o.id))
        .cloned()
        .collect();
    let selected = objects
        .iter_mut()
        .find(|o| o.id == id)
        .context("Select an object first")?;
    selected.parent = None;
    selected.transform.translation = [0.0; 3];
    let name = selected.name.clone();
    let nested: BTreeMap<_, _> = scene
        .prefabs
        .iter()
        .filter(|(root, _)| ids.contains(*root) && root.as_str() != id)
        .map(|(root, link)| (root.clone(), link.clone()))
        .collect();
    let dependencies: BTreeSet<_> = objects
        .iter()
        .flat_map(|o| {
            o.asset_dependencies()
                .into_iter()
                .map(|(id, _)| id.to_owned())
        })
        .chain(nested.values().map(|link| link.asset.clone()))
        .chain(
            nested
                .values()
                .flat_map(|link| &link.baseline)
                .flat_map(|o| {
                    o.asset_dependencies()
                        .into_iter()
                        .map(|(id, _)| id.to_owned())
                }),
        )
        .collect();
    let mut assets = BTreeMap::new();
    for id in dependencies {
        let mut source = scene
            .assets
            .get(&id)
            .context("missing prefab dependency")?
            .clone();
        source.path = relative_asset(&root(scene_path).join(&source.path), root(target))?;
        assets.insert(id, source);
    }
    let prefab = Prefab {
        nested,
        base: None,
        version: 1,
        name,
        root: id.into(),
        objects,
        assets,
    };
    prefab.validate()?;
    Ok(prefab)
}
fn bind_assets(
    scene: &mut Scene,
    prefab: &Prefab,
    target: &Path,
    scene_path: &Path,
) -> Result<BTreeMap<String, String>> {
    let mut remap = BTreeMap::new();
    let mut used: BTreeSet<_> = scene.assets.keys().cloned().collect();
    for (id, source) in &prefab.assets {
        let source = AssetSource {
            kind: source.kind,
            path: relative_asset(&root(target).join(&source.path), root(scene_path))?,
        };
        let existing = scene
            .assets
            .iter()
            .find(|(_, s)| {
                s.kind == source.kind
                    && relative_asset(&root(scene_path).join(&s.path), root(scene_path))
                        .ok()
                        .as_ref()
                        == Some(&source.path)
            })
            .map(|(id, _)| id.clone());
        let bound = existing.unwrap_or_else(|| {
            let bound = if used.insert(id.clone()) {
                id.clone()
            } else {
                fresh_id(&mut used, id)
            };
            scene.assets.insert(bound.clone(), source);
            bound
        });
        remap.insert(id.clone(), bound);
    }
    Ok(remap)
}
fn remap(
    prefab: &Prefab,
    members: &BTreeMap<String, String>,
    assets: &BTreeMap<String, String>,
) -> Vec<Object> {
    prefab
        .objects
        .iter()
        .cloned()
        .map(|mut o| {
            o.remap_ids(members);
            o.remap_assets(assets);
            o
        })
        .collect()
}
fn merge_instances(
    scene: &mut Scene,
    prefab: &Prefab,
    asset: &str,
    target: &Path,
    scene_path: &Path,
) -> Result<()> {
    let roots: Vec<_> = scene
        .prefabs
        .iter()
        .filter(|(_, p)| p.asset == asset)
        .map(|(id, _)| id.clone())
        .collect();
    ensure!(
        !roots.is_empty(),
        "No linked instances of this prefab in the scene"
    );
    let assets = bind_assets(scene, prefab, target, scene_path)?;
    let mut used = scene.objects.iter().map(|o| o.id.clone()).collect();
    let placement_roots = roots.iter().cloned().collect();
    let mut previous = Vec::new();
    let mut incoming = Vec::new();
    for root in roots {
        let link = scene.prefabs[&root].clone();
        ensure!(
            link.members.get(&prefab.root) == Some(&root),
            "Prefab root identity changed; unpack or replace the instance"
        );
        let mut members = BTreeMap::new();
        for object in &prefab.objects {
            let id = link
                .members
                .get(&object.id)
                .cloned()
                .unwrap_or_else(|| fresh_id(&mut used, "prefab"));
            members.insert(object.id.clone(), id);
        }
        let baseline = remap(prefab, &members, &assets);
        previous.extend(link.baseline);
        incoming.extend(baseline.iter().cloned());
        scene.prefabs.insert(
            root,
            PrefabInstance {
                asset: asset.into(),
                members,
                baseline,
            },
        );
    }
    // Build the object index and remove deleted members once for the whole
    // operation, rather than rescanning a large scene for every linked instance.
    bozzard_scene::merge_prefab_objects(
        &mut scene.objects,
        &previous,
        &incoming,
        &placement_roots,
        false,
    )?;
    scene.validate()
}

pub(super) fn publish(write: &SourceWrite) -> Result<()> {
    for (path, bytes) in &write.reads {
        ensure!(
            read_bytes(path)? == **bytes,
            "Prefab dependency changed while preparing; retry: {}",
            path.display()
        );
    }
    ensure!(
        write.json.len() <= 32 * 1024 * 1024,
        "prefab exceeds 32 MiB"
    );
    if let Some(previous) = &write.previous {
        ensure!(
            read_bytes(&write.path)? == *previous,
            "Prefab source changed while preparing; refresh and try again"
        );
    }
    let parent = root(&write.path);
    std::fs::create_dir_all(parent)?;
    if write.previous.is_none() {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&write.path)?;
        let result = file
            .write_all(write.json.as_bytes())
            .and_then(|()| file.sync_all());
        if result.is_err() {
            drop(file);
            let _ = std::fs::remove_file(&write.path);
        }
        result?;
    } else {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temp = parent.join(format!(
            ".bozzard-prefab-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = (|| -> Result<()> {
            file.write_all(write.json.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temp, &write.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result?;
    }
    Ok(())
}
