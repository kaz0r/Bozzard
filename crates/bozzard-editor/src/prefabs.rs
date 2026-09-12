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
}
struct Preparation {
    asset: String,
    label: &'static str,
    write: Option<SourceWrite>,
    read: Option<SourceRead>,
}
struct SourceWrite {
    path: PathBuf,
    json: String,
    previous: Option<Vec<u8>>,
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
                    !scene
                        .prefabs
                        .values()
                        .any(|link| link.members.values().any(|id| ids.contains(id))),
                    "Unpack existing instances before saving a new prefab"
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
                scene.prefabs.insert(
                    selected.clone(),
                    PrefabInstance {
                        asset: asset.clone(),
                        members: ids.iter().map(|id| (id.clone(), id.clone())).collect(),
                        baseline,
                    },
                );
                write = Some(SourceWrite {
                    path: target,
                    json,
                    previous: None,
                });
                (asset, "Create prefab")
            }
            PrefabCommand::Instantiate { asset, position } => {
                let (prefab, target, bytes) = read_asset(&scene, &self.path, &asset)?;
                read = Some(SourceRead {
                    path: target.clone(),
                    bytes,
                });
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
                let (prefab, target, bytes) = read_asset(&scene, &self.path, &asset)?;
                read = Some(SourceRead {
                    path: target.clone(),
                    bytes,
                });
                merge_instances(&mut scene, &prefab, &asset, &target, &self.path)?;
                (asset, "Refresh prefab instances")
            }
            PrefabCommand::Apply => {
                let id = self
                    .selected_prefab_root()
                    .context("Select a prefab instance")?
                    .to_owned();
                let link = scene.prefabs[&id].clone();
                let (previous_prefab, target, previous) =
                    read_asset(&scene, &self.path, &link.asset)?;
                read = Some(SourceRead {
                    path: target.clone(),
                    bytes: previous.clone(),
                });
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
                    object.remap_blueprint_objects(&inverse);
                    if object.id == id {
                        object.transform =
                            link.baseline.iter().find(|o| o.id == id).unwrap().transform;
                    }
                    object.id = inverse[&object.id].clone();
                    object.parent = object.parent.as_ref().map(|p| inverse[p].clone());
                }
                prefab.root = inverse[&id].clone();
                let json = prefab.to_json()?;
                merge_instances(&mut scene, &prefab, &link.asset, &target, &self.path)?;
                write = Some(SourceWrite {
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
fn read_asset(scene: &Scene, path: &Path, asset: &str) -> Result<(Prefab, PathBuf, Vec<u8>)> {
    let source = scene.assets.get(asset).context("prefab asset missing")?;
    ensure!(source.kind == AssetKind::Prefab, "asset is not a prefab");
    let target = root(path).join(&source.path);
    let bytes =
        read_bytes(&target).with_context(|| format!("reading prefab {}", target.display()))?;
    Ok((
        Prefab::from_json(std::str::from_utf8(&bytes)?)?,
        target,
        bytes,
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
    let dependencies: BTreeSet<_> = objects
        .iter()
        .flat_map(|o| {
            o.asset_dependencies()
                .into_iter()
                .map(|(id, _)| id.to_owned())
        })
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
            o.remap_blueprint_objects(members);
            o.remap_assets(assets);
            o.id = members[&o.id].clone();
            o.parent = o.parent.map(|p| members[&p].clone());
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
        let retained: BTreeSet<_> = members.values().collect();
        for old in &link.baseline {
            if !retained.contains(&old.id) {
                let current = scene
                    .objects
                    .iter()
                    .find(|o| o.id == old.id)
                    .context("prefab member missing")?;
                ensure!(
                    current == old,
                    "Source removed locally edited child '{}'; unpack the instance before refreshing",
                    current.name
                );
                scene.objects.retain(|o| o.id != old.id);
            }
        }
        for source in &baseline {
            if let Some(current) = scene.objects.iter_mut().find(|o| o.id == source.id) {
                let old = link
                    .baseline
                    .iter()
                    .find(|o| o.id == source.id)
                    .context("prefab baseline missing")?;
                macro_rules! merge { ($($field:ident),*) => { $(if current.$field == old.$field { current.$field = source.$field.clone(); })* }; }
                merge!(
                    name,
                    camera,
                    drawable,
                    material,
                    spin,
                    collider,
                    mesh_collider,
                    text_rendering,
                    gravity,
                    trigger,
                    light,
                    blueprints
                );
                if current.id != root {
                    merge!(transform, parent);
                }
            } else {
                scene.objects.push(source.clone());
            }
        }
        scene.prefabs.insert(
            root,
            PrefabInstance {
                asset: asset.into(),
                members,
                baseline,
            },
        );
    }
    scene.validate()
}

fn publish(write: &SourceWrite) -> Result<()> {
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
