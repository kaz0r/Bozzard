//! Prefab lifecycle changes are validated before touching ECS membership.
use super::*;
use std::collections::BTreeSet;

impl SceneInstance {
    /// Templates must already use this scene's asset IDs (including graph prefab bindings).
    pub fn register_prefab(&mut self, asset: String, prefab: Prefab) -> Result<()> {
        prefab.validate()?;
        ensure!(
            self.document
                .assets
                .get(&asset)
                .is_some_and(|a| a.kind == AssetKind::Prefab),
            "unknown prefab asset '{asset}'"
        );
        for object in &prefab.objects {
            for (id, kind) in object.asset_dependencies() {
                ensure!(
                    self.document.assets.get(id).is_some_and(|a| a.kind == kind),
                    "prefab dependency '{id}' is not bound to the scene"
                );
            }
        }
        self.templates.insert(asset, prefab);
        Ok(())
    }

    pub fn spawn_prefab(
        &mut self,
        world: &mut World,
        asset: &str,
        position: [f32; 3],
    ) -> Result<String> {
        let template = self
            .templates
            .get(asset)
            .context("Prefab template unavailable; load or place this prefab before Play")?;
        ensure!(
            position.iter().all(|v| v.is_finite()),
            "prefab position must be finite"
        );
        // Never reuse persistent runtime IDs: a stale Blueprint reference must stay invalid.
        let members = loop {
            let serial = self
                .next_spawn
                .checked_add(1)
                .context("prefab ID space exhausted")?;
            self.next_spawn = serial;
            let members: BTreeMap<_, _> = template
                .objects
                .iter()
                .enumerate()
                .map(|(i, o)| (o.id.clone(), format!("spawn-{serial}-{i}")))
                .collect();
            if members.values().all(|id| !self.entities.contains_key(id)) {
                break members;
            }
        };
        let root = members[&template.root].clone();
        let baseline: Vec<_> = template
            .objects
            .iter()
            .cloned()
            .map(|mut o| {
                o.remap_blueprint_objects(&members);
                o.id = members[&o.id].clone();
                o.parent = o.parent.map(|id| members[&id].clone());
                o
            })
            .collect();
        // ponytail: validate a cloned document per spawn; batch changes if spawn-heavy scenes need it.
        let mut scene = self.document.clone();
        for mut object in baseline.clone() {
            if object.id == root {
                object.transform.translation = position;
            }
            scene.objects.push(object);
        }
        scene.prefabs.insert(
            root.clone(),
            PrefabInstance {
                asset: asset.into(),
                members,
                baseline,
            },
        );
        let order = scene.order()?;
        for object in &scene.objects[self.document.objects.len()..] {
            self.entities
                .insert(object.id.clone(), object.spawn_in(world)?);
        }
        self.document = scene;
        self.order = order;
        self.refresh_collectibles(world);
        Ok(root)
    }

    /// Any member identifies its entire linked prefab; active cameras/players are protected.
    pub fn destroy_prefab(&mut self, world: &mut World, target: &str) -> Result<()> {
        let (root, link) = self
            .document
            .prefabs
            .iter()
            .find(|(_, link)| link.members.values().any(|id| id == target))
            .context("Destroy Prefab target is not a live prefab instance")?;
        let ids: BTreeSet<_> = link.members.values().cloned().collect();
        ensure!(
            !self.document.views.values().any(|id| ids.contains(id)),
            "cannot destroy an active camera; switch views first"
        );
        let mut scene = self.document.clone();
        scene.prefabs.remove(root);
        scene.objects.retain(|o| !ids.contains(&o.id));
        let cleared = ids
            .iter()
            .map(|id| (id.clone(), blueprint::ObjectRef::None))
            .collect::<BTreeMap<_, _>>();
        for object in scene
            .objects
            .iter_mut()
            .chain(scene.prefabs.values_mut().flat_map(|p| &mut p.baseline))
        {
            for value in object
                .blueprints
                .iter_mut()
                .flat_map(|b| &mut b.graph.nodes)
                .flat_map(|n| &mut n.inputs)
            {
                if let blueprint::Value::Object(blueprint::ObjectRef::Id(id)) = value
                    && let Some(reference) = cleared.get(id)
                {
                    *value = blueprint::Value::Object(reference.clone());
                }
            }
        }
        let order = scene.order()?;
        ensure!(
            ids.iter()
                .all(|id| self.entity(id).is_some_and(|e| world.contains(e))),
            "prefab member was removed outside the scene"
        );
        for id in &ids {
            let entity = self.entities.remove(id).unwrap();
            if let Some(physics) = world.resource_mut::<crate::physics::Physics>() {
                physics.remove_entity(entity);
            }
            world.despawn(entity)?;
        }
        self.document = scene;
        self.order = order;
        self.refresh_collectibles(world);
        Ok(())
    }

    fn refresh_collectibles(&self, world: &mut World) {
        if let Some(state) = world.resource_mut::<GameplayState>() {
            state.collected.retain(|id| self.entities.contains_key(id));
            if state
                .checkpoint
                .as_ref()
                .is_some_and(|id| !self.entities.contains_key(id))
            {
                state.checkpoint = None;
            }
            state.total = self
                .document
                .objects
                .iter()
                .filter(|o| {
                    o.trigger.as_ref().is_some_and(|t| {
                        t.volume.enabled && matches!(t.action, TriggerAction::Collectible)
                    })
                })
                .count();
        }
    }
}
