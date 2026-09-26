//! Prefab lifecycle changes are validated before touching ECS membership.
use super::*;
use std::collections::BTreeSet;

pub(crate) fn validate_template(scene: &Scene, asset: &str, prefab: &Prefab) -> Result<()> {
    prefab.validate()?;
    ensure!(
        scene
            .assets
            .get(asset)
            .is_some_and(|a| a.kind == AssetKind::Prefab),
        "unknown prefab asset '{asset}'"
    );
    for object in &prefab.objects {
        for (id, kind) in object.asset_dependencies() {
            ensure!(
                scene.assets.get(id).is_some_and(|a| a.kind == kind),
                "prefab dependency '{id}' is not bound to the scene"
            );
        }
    }
    Ok(())
}

impl SceneInstance {
    /// Templates must already use this scene's asset IDs (including graph prefab bindings).
    pub fn register_prefab(&mut self, asset: String, prefab: Prefab) -> Result<()> {
        validate_template(&self.document, &asset, &prefab)?;
        self.templates.insert(asset, prefab);
        Ok(())
    }

    /// Spawned gameplay objects inherit their creator's additive-scene lifetime.
    /// Hosts can use `spawn_prefab` directly for objects that persist until replacement.
    pub fn spawn_prefab_for(
        &mut self,
        world: &mut World,
        owner: &str,
        asset: &str,
        position: [f32; 3],
    ) -> Result<String> {
        ensure!(self.entities.contains_key(owner), "prefab owner is missing");
        let scope = self
            .additive_scenes
            .iter()
            .find(|(_, group)| group.members.contains(owner))
            .map(|(handle, _)| handle.clone());
        let root = self.spawn_prefab(world, asset, position)?;
        if let Some(scope) = scope {
            self.additive_scenes
                .get_mut(&scope)
                .unwrap()
                .members
                .extend(self.document.prefabs[&root].members.values().cloned());
        }
        Ok(root)
    }

    pub fn spawn_prefab(
        &mut self,
        world: &mut World,
        asset: &str,
        position: [f32; 3],
    ) -> Result<String> {
        Ok(self
            .spawn_prefab_batch(world, &[(asset, position)])?
            .remove(0))
    }

    /// Validate one prospective document for consecutive spawn commands. Templates
    /// and the complete resulting scene still pass the ordinary validation path.
    pub(crate) fn spawn_prefab_batch(
        &mut self,
        world: &mut World,
        requests: &[(&str, [f32; 3])],
    ) -> Result<Vec<String>> {
        let mut scene = self.document.clone();
        let mut roots = Vec::with_capacity(requests.len());
        for &(asset, position) in requests {
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
                    crate::scene_loading::remap_object(&mut o, &members);
                    o
                })
                .collect();
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
            roots.push(root);
        }
        let order = scene.order()?;
        for object in &scene.objects[self.document.objects.len()..] {
            self.entities
                .insert(object.id.clone(), object.spawn_in(world)?);
        }
        self.document = scene;
        self.order = order;
        self.rebuild_hierarchy_index();
        self.refresh_collectibles(world);
        Ok(roots)
    }

    pub(crate) fn spawn_prefab_batch_for(
        &mut self,
        world: &mut World,
        requests: &[(String, String, [f32; 3])],
    ) -> Result<Vec<String>> {
        let mut scopes = Vec::with_capacity(requests.len());
        for (owner, _, _) in requests {
            ensure!(self.entities.contains_key(owner), "prefab owner is missing");
            scopes.push(
                self.additive_scenes
                    .iter()
                    .find(|(_, group)| group.members.contains(owner))
                    .map(|(handle, _)| handle.clone()),
            );
        }
        let assets: Vec<_> = requests
            .iter()
            .map(|(_, asset, position)| (asset.as_str(), *position))
            .collect();
        let roots = self.spawn_prefab_batch(world, &assets)?;
        for (root, scope) in roots.iter().zip(scopes) {
            if let Some(scope) = scope {
                self.additive_scenes
                    .get_mut(&scope)
                    .unwrap()
                    .members
                    .extend(self.document.prefabs[root].members.values().cloned());
            }
        }
        Ok(roots)
    }

    /// Any member identifies its entire linked prefab; active cameras/players are protected.
    pub(crate) fn destroy_prefab_raw(&mut self, world: &mut World, target: &str) -> Result<()> {
        let (_, link) = self
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
        self.remove_objects_raw(world, &ids, false)
    }

    pub(crate) fn prepare_object_removal(
        &self,
        ids: &BTreeSet<String>,
        remove_views: bool,
    ) -> Result<(Scene, Vec<usize>)> {
        let mut scene = self.document.clone();
        for link in scene.prefabs.values() {
            let removed = link.members.values().filter(|id| ids.contains(*id)).count();
            ensure!(
                removed == 0 || removed == link.members.len(),
                "cannot remove part of a prefab"
            );
        }
        scene
            .prefabs
            .retain(|_, link| !link.members.values().any(|id| ids.contains(id)));
        scene.objects.retain(|o| !ids.contains(&o.id));
        if remove_views {
            scene.views.retain(|_, id| !ids.contains(id));
        }
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
                    && ids.contains(id)
                {
                    *value = blueprint::Value::Object(blueprint::ObjectRef::None);
                }
            }
        }
        for value in scene
            .blackboard
            .values_mut()
            .flat_map(blueprint::BlackboardValue::values_mut)
            .chain(
                scene
                    .objects
                    .iter_mut()
                    .chain(scene.prefabs.values_mut().flat_map(|p| &mut p.baseline))
                    .flat_map(|o| {
                        o.blackboard.values_mut().chain(
                            o.blueprints
                                .iter_mut()
                                .flat_map(|b| b.graph.blackboard.values_mut()),
                        )
                    })
                    .flat_map(blueprint::BlackboardValue::values_mut),
            )
        {
            if let blueprint::Value::Object(blueprint::ObjectRef::Id(id)) = value
                && ids.contains(id)
            {
                *value = blueprint::Value::Object(blueprint::ObjectRef::None);
            }
        }
        let order = scene.order()?;
        Ok((scene, order))
    }

    pub(crate) fn remove_objects_raw(
        &mut self,
        world: &mut World,
        ids: &BTreeSet<String>,
        remove_views: bool,
    ) -> Result<()> {
        let (scene, order) = self.prepare_object_removal(ids, remove_views)?;
        ensure!(
            ids.iter()
                .all(|id| self.entity(id).is_some_and(|e| world.contains(e))),
            "prefab member was removed outside the scene"
        );
        if let Some(mut compute) = self.compute_if_initialized() {
            compute.release_objects(ids)?;
        }
        for id in ids {
            let entity = self.entities.remove(id).unwrap();
            if let Some(physics) = world.resource_mut::<crate::physics::Physics>() {
                physics.remove_entity(entity);
            }
            world.despawn(entity)?;
        }
        self.document = scene;
        self.order = order;
        self.rebuild_hierarchy_index();
        for group in self.additive_scenes.values_mut() {
            group.members.retain(|id| !ids.contains(id));
        }
        if let Some(runtime) = world.resource_mut::<BlueprintRuntime>() {
            runtime.remove_objects(ids);
        }
        if let Some(runtime) = world.resource_mut::<crate::ScriptRuntime>() {
            runtime.remove_objects(ids);
        }
        middleware::checkpoint::remove_objects(world, ids);
        self.particle_state.remove_objects(ids);
        if world
            .resource::<GameplayState>()
            .is_some_and(|s| ids.contains(&s.player))
        {
            world.remove_resource::<GameplayState>();
            world.insert_resource(PlayerMotion::default());
        }
        self.refresh_collectibles(world);
        Ok(())
    }

    pub(crate) fn refresh_collectibles(&self, world: &mut World) {
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
