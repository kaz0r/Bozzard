//! Prefabs are authoring assets. Scenes retain expanded objects for standalone/headless use.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prefab {
    pub version: u32,
    pub name: String,
    pub root: String,
    pub objects: Vec<Object>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assets: BTreeMap<String, AssetSource>,
    /// Direct nested instances; their expanded members remain in `objects`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub nested: BTreeMap<String, PrefabInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<PrefabBase>,
}

/// A variant keeps its previous inherited state so source edits can be merged
/// without overwriting local component, hierarchy or nested-instance changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabBase {
    pub asset: String,
    pub baseline: Vec<Object>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub nested: BTreeMap<String, PrefabInstance>,
}

impl Object {
    /// Remap an object's own ID and all built-in hierarchy/gameplay references.
    pub fn remap_ids(&mut self, mapping: &BTreeMap<String, String>) {
        crate::scene_loading::remap_object(self, mapping);
    }
}

/// Baselines use scene-local IDs, allowing a three-way merge without consulting old files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefabInstance {
    pub asset: String,
    pub members: BTreeMap<String, String>,
    pub baseline: Vec<Object>,
}

impl Prefab {
    pub fn authoring_scene(&self) -> Scene {
        let mut scene = document(self.objects.clone(), self.assets.clone());
        scene.name = self.name.clone();
        scene.prefabs = self.nested.clone();
        scene
    }
    pub fn from_json(json: &str) -> Result<Self> {
        let prefab: Self = serde_json::from_str(json).context("parsing prefab JSON")?;
        prefab.validate()?;
        Ok(prefab)
    }
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)? + "\n")
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported prefab version");
        ensure!(
            self.objects.iter().all(|o| o.player_controller.is_none()),
            "Player Controller requires scene-level camera wiring and cannot be saved in a prefab yet"
        );
        let mut scene = document(self.objects.clone(), self.assets.clone());
        scene.prefabs = self.nested.clone();
        scene.validate()?;
        if let Some(base) = &self.base {
            ensure!(
                self.assets
                    .get(&base.asset)
                    .is_some_and(|a| a.kind == AssetKind::Prefab),
                "variant references a missing base prefab"
            );
            let mut inherited = document(base.baseline.clone(), self.assets.clone());
            inherited.prefabs = base.nested.clone();
            inherited.validate()?;
            ensure!(
                base.baseline.iter().filter(|o| o.parent.is_none()).count() == 1
                    && base
                        .baseline
                        .iter()
                        .any(|o| o.id == self.root && o.parent.is_none()),
                "variant must preserve its base root identity"
            );
        }
        ensure!(
            self.objects.iter().filter(|o| o.parent.is_none()).count() == 1
                && self
                    .objects
                    .iter()
                    .any(|o| o.id == self.root && o.parent.is_none()),
            "prefab must contain one complete hierarchy with the declared root"
        );
        Ok(())
    }

    pub fn structural_dependencies(&self) -> BTreeSet<&str> {
        self.nested
            .values()
            .map(|link| link.asset.as_str())
            .chain(self.base.iter().map(|base| base.asset.as_str()))
            .collect()
    }

    pub fn remap_assets(&mut self, mapping: &BTreeMap<String, String>) {
        for object in &mut self.objects {
            object.remap_assets(mapping);
        }
        for link in self
            .nested
            .values_mut()
            .chain(self.base.iter_mut().flat_map(|b| b.nested.values_mut()))
        {
            if let Some(id) = mapping.get(&link.asset) {
                link.asset = id.clone();
            }
            for object in &mut link.baseline {
                object.remap_assets(mapping);
            }
        }
        if let Some(base) = &mut self.base {
            if let Some(id) = mapping.get(&base.asset) {
                base.asset = id.clone();
            }
            for object in &mut base.baseline {
                object.remap_assets(mapping);
            }
        }
    }

    /// Merge a resolved dependency already bound to this prefab's asset catalog.
    /// The caller publishes this candidate only after every dependency succeeds.
    pub fn refresh_dependency(&mut self, asset: &str, source: &Prefab) -> Result<()> {
        if let Some(base) = &mut self.base
            && base.asset == asset
        {
            ensure!(
                source.root == self.root,
                "variant base root identity changed"
            );
            merge_objects(
                &mut self.objects,
                &base.baseline,
                &source.objects,
                &BTreeSet::new(),
                true,
            )?;
            let keys: BTreeSet<_> = base
                .nested
                .keys()
                .chain(source.nested.keys())
                .cloned()
                .collect();
            for key in keys {
                if self.nested.get(&key) == base.nested.get(&key) {
                    if let Some(link) = source.nested.get(&key) {
                        self.nested.insert(key, link.clone());
                    } else {
                        self.nested.remove(&key);
                    }
                }
            }
            base.baseline = source.objects.clone();
            base.nested = source.nested.clone();
        }
        let mut used: BTreeSet<_> = self.objects.iter().map(|o| o.id.clone()).collect();
        let mut placement_roots = BTreeSet::new();
        let mut previous = Vec::new();
        let mut incoming = Vec::new();
        for (root, link) in self
            .nested
            .iter_mut()
            .filter(|(_, link)| link.asset == asset)
        {
            ensure!(
                link.members.get(&source.root) == Some(root),
                "nested prefab root identity changed"
            );
            let mut members = BTreeMap::new();
            for object in &source.objects {
                let id = link.members.get(&object.id).cloned().unwrap_or_else(|| {
                    let stem = format!("{root}-{}", object.id);
                    let mut id = stem.clone();
                    let mut suffix = 1;
                    while !used.insert(id.clone()) {
                        id = format!("{stem}-{suffix}");
                        suffix += 1;
                    }
                    id
                });
                members.insert(object.id.clone(), id);
            }
            let baseline: Vec<_> = source
                .objects
                .iter()
                .cloned()
                .map(|mut object| {
                    crate::scene_loading::remap_object(&mut object, &members);
                    object
                })
                .collect();
            placement_roots.insert(root.clone());
            previous.append(&mut link.baseline);
            incoming.extend(baseline.iter().cloned());
            link.members = members;
            link.baseline = baseline;
        }
        if !placement_roots.is_empty() {
            merge_objects(
                &mut self.objects,
                &previous,
                &incoming,
                &placement_roots,
                false,
            )?;
        }
        Ok(())
    }
}

/// Three-way component merge shared by variants, nested sources and scene instances.
/// Validate the complete candidate before publishing; errors leave the caller's
/// original document untouched when this is used on its prepared clone.
pub fn merge_objects(
    current: &mut Vec<Object>,
    old: &[Object],
    source: &[Object],
    placement_roots: &BTreeSet<String>,
    allow_local_deletions: bool,
) -> Result<()> {
    let old: BTreeMap<_, _> = old.iter().map(|o| (o.id.as_str(), o)).collect();
    let source: BTreeMap<_, _> = source.iter().map(|o| (o.id.as_str(), o)).collect();
    let indices: BTreeMap<_, _> = current
        .iter()
        .enumerate()
        .map(|(i, o)| (o.id.clone(), i))
        .collect();
    for (&id, previous) in &old {
        if !source.contains_key(id)
            && let Some(&index) = indices.get(id)
        {
            ensure!(
                &current[index] == *previous,
                "Source removed locally edited child '{}'; resolve the override before refreshing",
                current[index].name
            );
        }
    }
    for (&id, incoming) in &source {
        if let Some(&index) = indices.get(id) {
            let previous = old
                .get(id)
                .context("source added an object whose ID collides with a local child")?;
            let object = &mut current[index];
            if object.name == previous.name {
                object.name = incoming.name.clone();
            }
            if !placement_roots.contains(id) {
                if object.transform == previous.transform {
                    object.transform = incoming.transform;
                }
                if object.parent == previous.parent {
                    object.parent = incoming.parent.clone();
                }
            }
            for entry in components() {
                (entry.merge)(object, previous, incoming);
            }
        } else if let Some(previous) = old.get(id) {
            ensure!(
                allow_local_deletions && *previous == *incoming,
                "Source changed a locally removed child '{id}'; resolve the hierarchy before refreshing"
            );
        } else {
            current.push((*incoming).clone());
        }
    }
    current.retain(|o| !old.contains_key(o.id.as_str()) || source.contains_key(o.id.as_str()));
    Ok(())
}

fn document(objects: Vec<Object>, assets: BTreeMap<String, AssetSource>) -> Scene {
    Scene {
        blackboard: objects
            .iter()
            .flat_map(|o| &o.blueprints)
            .flat_map(|b| &b.graph.nodes)
            .filter(|n| n.uses_variable() && n.scope == blueprint::VariableScope::Scene)
            .map(|n| {
                (
                    n.variable.clone(),
                    if n.uses_list() {
                        blueprint::BlackboardValue::List {
                            element: if matches!(
                                n.kind,
                                blueprint::NodeKind::SphereOverlap
                                    | blueprint::NodeKind::BoxOverlap
                            ) {
                                blueprint::PinType::Object
                            } else {
                                n.value_type
                            },
                            capacity: 256,
                            values: vec![],
                        }
                    } else {
                        blueprint::BlackboardValue::Scalar(n.value_type.default_value())
                    },
                )
            })
            .collect(),
        runtime_scenes: Default::default(),
        runtime_scene_sources: Default::default(),
        game_flow: None,
        version: SCENE_VERSION,
        name: "Prefab".into(),
        objects,
        assets,
        views: BTreeMap::new(),
        prefabs: BTreeMap::new(),
        fog: Default::default(),
        gi: Default::default(),
        environment: Default::default(),
        display: Default::default(),
        post_process_volumes: Vec::new(),
        lighting: Default::default(),
    }
}

pub(super) fn validate(scene: &Scene) -> Result<()> {
    if scene.prefabs.is_empty() {
        return Ok(());
    }
    let objects: BTreeMap<_, _> = scene.objects.iter().map(|o| (o.id.as_str(), o)).collect();
    let mut owners = BTreeMap::new();
    for (root, link) in &scene.prefabs {
        ensure!(
            scene
                .assets
                .get(&link.asset)
                .is_some_and(|a| a.kind == AssetKind::Prefab),
            "prefab '{root}' references a missing prefab asset"
        );
        let members: BTreeSet<_> = link.members.values().map(String::as_str).collect();
        ensure!(
            members.len() == link.members.len()
                && members.contains(root.as_str())
                && link.members.keys().all(|id| !id.is_empty()),
            "invalid prefab member mapping"
        );
        ensure!(
            link.baseline.len() == members.len(),
            "invalid prefab baseline"
        );
        let mut seen = BTreeSet::new();
        for base in &link.baseline {
            ensure!(
                members.contains(base.id.as_str()) && seen.insert(base.id.as_str()),
                "invalid prefab baseline member"
            );
            let object = objects
                .get(base.id.as_str())
                .context("Unpack the prefab before removing its children")?;
            ensure!(
                owners.insert(base.id.as_str(), root.as_str()).is_none(),
                "scene prefab ownership overlaps; edit nesting in the prefab source"
            );
            if base.id == *root {
                ensure!(base.parent.is_none(), "prefab baseline root has a parent");
            } else {
                ensure!(
                    base.parent.as_deref().is_some_and(|p| members.contains(p)),
                    "invalid prefab baseline hierarchy"
                );
                ensure!(
                    object.parent == base.parent,
                    "Unpack the prefab before reparenting its children"
                );
            }
            ensure!(
                base.player_controller.is_none() && object.player_controller.is_none(),
                "Unpack the prefab before adding a Player Controller"
            );
        }
        // Validate only this baseline's dependencies, avoiding a full catalog copy per instance.
        let dependencies: BTreeSet<_> = link
            .baseline
            .iter()
            .flat_map(|o| o.asset_dependencies().into_iter().map(|(id, _)| id))
            .collect();
        let assets = dependencies
            .into_iter()
            .map(|id| {
                Ok((
                    id.to_owned(),
                    scene
                        .assets
                        .get(id)
                        .context("missing prefab baseline asset")?
                        .clone(),
                ))
            })
            .collect::<Result<_>>()?;
        document(link.baseline.clone(), assets).validate()?;
    }
    for object in &scene.objects {
        if let Some(owner) = object.parent.as_deref().and_then(|p| owners.get(p)) {
            ensure!(
                owners.get(object.id.as_str()) == Some(owner),
                "Unpack the prefab before adding or nesting children"
            );
        }
    }
    Ok(())
}
