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
            self.assets.iter().all(|(id, a)| a.kind != AssetKind::Prefab
                || self
                    .objects
                    .iter()
                    .flat_map(|o| &o.blueprints)
                    .flat_map(|b| &b.graph.nodes)
                    .any(|n| n.kind == blueprint::NodeKind::SpawnPrefab && &n.prefab == id)),
            "nested prefab assets must be referenced by Spawn Prefab nodes"
        );
        ensure!(
            self.objects.iter().all(|o| o.player_controller.is_none()),
            "Player Controller requires scene-level camera wiring and cannot be saved in a prefab yet"
        );
        let scene = document(self.objects.clone(), self.assets.clone());
        scene.validate()?;
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
}

fn document(objects: Vec<Object>, assets: BTreeMap<String, AssetSource>) -> Scene {
    Scene {
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
                "nested or overlapping prefabs are not supported yet"
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
