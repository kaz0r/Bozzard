//! Applying the commands scripts queued, and the destroys they request.
use super::*;

impl SceneInstance {
    /// Applies queued script commands in order, then the destroys they asked for.
    pub(super) fn apply_commands(
        &mut self,
        world: &mut World,
        runtime: &mut ScriptRuntime,
        engine: Arc<ScriptEngine>,
        commands: Vec<Command>,
        tokens: &mut BTreeMap<String, String>,
    ) -> Result<()> {
        let mut destroy = Vec::new();
        let mut commands = commands.into_iter().peekable();
        while let Some(command) = commands.next() {
            match command {
                Command::NetworkRequest(request) => {
                    if world.resource::<NetworkOutbox>().is_none() {
                        world.insert_resource(NetworkOutbox::default());
                    }
                    world.resource_mut::<NetworkOutbox>().unwrap().push(request);
                }
                Command::SetVelocity { target, velocity } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Velocity target does not exist")?;
                    self.set_velocity(world, &target, entity, Vec3::from(velocity))?;
                }
                Command::MoveWithCollision { target, delta } => {
                    let target = resolve(tokens, &target);
                    let movement = self.move_box(world, &target, Vec3::from(delta))?;
                    runtime.moved.insert(
                        target,
                        movement
                            .contact_normals
                            .iter()
                            .any(|normal| normal.y >= 0.5),
                    );
                }
                Command::Jump { target, speed } => {
                    let target = resolve(tokens, &target);
                    self.jump_box(world, &target, speed)?;
                }
                Command::Transform {
                    target,
                    kind,
                    value,
                } => self.apply_transform(world, &resolve(tokens, &target), kind, value)?,
                Command::ResetInterpolation { target } => {
                    self.reset_render_interpolation(world, &resolve(tokens, &target))?;
                }
                Command::Color { target, color } => {
                    self.apply_color(world, &resolve(tokens, &target), color)?
                }
                Command::Mesh { target, asset } => {
                    ensure!(
                        self.document
                            .assets
                            .get(&asset)
                            .is_some_and(|source| source.kind == AssetKind::Mesh),
                        "Set Mesh requires a registered mesh asset: {asset}"
                    );
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Mesh target does not exist")?;
                    let previous = world
                        .get::<Drawable>(entity)
                        .context("Set Mesh needs a Drawable")?;
                    let mesh = Mesh::Asset(asset);
                    if previous.mesh != mesh {
                        let mut next = previous.clone();
                        next.mesh = mesh;
                        next.material_overrides.clear();
                        world.insert(entity, next)?;
                    }
                }
                Command::Text { target, text } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Text target does not exist")?;
                    let previous = world
                        .get::<TextRendering>(entity)
                        .context("Set Text needs Text Rendering")?;
                    if previous.text != text {
                        world
                            .get_mut::<TextRendering>(entity)
                            .expect("validated Text Rendering")
                            .text = text;
                    }
                }
                Command::Ui { target, control } => {
                    self.control_ui(world, &resolve(tokens, &target), control)?;
                }
                Command::Animation { target, control } => {
                    self.control_animation(world, &resolve(tokens, &target), control)?;
                }
                Command::Visible { target, visible } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Visible target does not exist")?;
                    world.insert(entity, BlueprintHidden(!visible))?;
                }
                Command::TileView(mut view) => {
                    let mut objects = BTreeMap::new();
                    for (target, factor) in view.objects {
                        let target = resolve(tokens, &target);
                        if let Some(prefab) = self.document.prefabs.get(&target) {
                            for member in prefab.members.values() {
                                objects.insert(member.clone(), factor);
                            }
                        } else {
                            objects.insert(target, factor);
                        }
                    }
                    view.objects = objects;
                    self.tile_view = view;
                }
                Command::LightIntensity { target, intensity } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Light Intensity target does not exist")?;
                    let mut light = *world
                        .get::<Light>(entity)
                        .context("Set Light Intensity needs a Light")?;
                    light.intensity = intensity;
                    light.validate()?;
                    world.insert(entity, light)?;
                }
                Command::LightColor { target, color } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("Set Light Color target does not exist")?;
                    let mut light = *world
                        .get::<Light>(entity)
                        .context("Set Light Color needs a Light")?;
                    light.color = color;
                    light.validate()?;
                    world.insert(entity, light)?;
                }
                Command::SceneLight {
                    ambient,
                    color,
                    intensity,
                } => {
                    let lighting = self.lighting_override.get_or_insert(self.document.lighting);
                    if ambient {
                        lighting.ambient_color = color;
                        lighting.ambient_intensity = intensity;
                    } else {
                        lighting.sun_color = color;
                        lighting.sun_intensity = intensity;
                    }
                }
                Command::Environment {
                    zenith,
                    horizon,
                    ground,
                    intensity,
                } => {
                    let environment = self
                        .environment_override
                        .get_or_insert(self.document.environment);
                    environment.zenith = zenith;
                    environment.horizon = horizon;
                    environment.ground = ground;
                    environment.intensity = intensity;
                }
                Command::Stars(intensity) => {
                    self.environment_override
                        .get_or_insert(self.document.environment)
                        .star_intensity = intensity;
                }
                Command::Fog { color, density } => {
                    let fog = self.fog_override.get_or_insert(self.document.fog);
                    fog.color = color;
                    fog.distance_density = density;
                    fog.enabled = density > 0.;
                }
                Command::Display { kind, value } => self.set_display_parameter(kind, value)?,
                Command::Spawn {
                    owner,
                    token,
                    asset,
                    position,
                } => {
                    let mut requests = vec![(owner, asset, position)];
                    let mut pending_tokens = vec![token];
                    while matches!(commands.peek(), Some(Command::Spawn { .. })) {
                        let Some(Command::Spawn {
                            owner,
                            token,
                            asset,
                            position,
                        }) = commands.next()
                        else {
                            unreachable!()
                        };
                        requests.push((owner, asset, position));
                        pending_tokens.push(token);
                    }
                    let roots = self.spawn_prefab_batch_for(world, &requests)?;
                    tokens.extend(pending_tokens.into_iter().zip(roots));
                }
                Command::Destroy { target } => destroy.push(resolve(tokens, &target)),
                Command::GraphEnabled {
                    target,
                    index,
                    enabled,
                } => self.set_blueprint_enabled(&resolve(tokens, &target), index, enabled)?,
                Command::ScriptEnabled {
                    target,
                    index,
                    enabled,
                } => self.set_script_enabled(&resolve(tokens, &target), index, enabled)?,
                Command::Cursor(requested) => {
                    world.insert_resource(CursorCapture {
                        requested: Some(requested),
                    });
                }
                Command::EndGame(message) => {
                    world
                        .resource_mut::<crate::GameSession>()
                        .context("End Game needs Game Flow enabled in scene settings")?
                        .end_game(&message)?;
                }
                Command::QuitGame => {
                    world.insert_resource(crate::GameSession {
                        phase: crate::GamePhase::Quit,
                        ..Default::default()
                    });
                }
                Command::CameraSize { target, size } => {
                    let target = resolve(tokens, &target);
                    let entity = *self
                        .entities
                        .get(&target)
                        .context("camera target does not exist")?;
                    let camera = world
                        .get::<Camera>(entity)
                        .context("target has no camera")?;
                    let Camera::Orthographic {
                        vertical_size,
                        near,
                        far,
                    } = *camera
                    else {
                        anyhow::bail!("set_camera_size requires an orthographic camera");
                    };
                    if vertical_size != size {
                        world.insert(
                            entity,
                            Camera::Orthographic {
                                vertical_size: size,
                                near,
                                far,
                            },
                        )?;
                    }
                }
                Command::SceneControl { kind, name } => {
                    self.request_scene_control(world, kind, &name)?
                }
                Command::Variable {
                    scope,
                    owner,
                    name,
                    value,
                } => world
                    .resource_mut::<BlueprintRuntime>()
                    .context("script variables need the blueprint runtime")?
                    .set_board_scalar(scope, &owner, &name, value)?,
                Command::ListVariable {
                    scope,
                    owner,
                    name,
                    values,
                } => world
                    .resource_mut::<BlueprintRuntime>()
                    .context("script variables need the blueprint runtime")?
                    .set_board_list(scope, &owner, &name, values)?,
                Command::Print { level, owner, text } => {
                    bozzard_diagnostics::log(
                        world,
                        level,
                        "Script",
                        &text,
                        bozzard_diagnostics::Location {
                            object: Some(owner),
                            ..Default::default()
                        },
                    );
                    runtime.messages.push_back(text.clone());
                    while runtime.messages.len() > 64 {
                        runtime.messages.pop_front();
                    }
                    println!("{text}");
                }
            }
        }
        self.destroy_script_prefabs(world, runtime, engine, destroy, tokens)?;
        Ok(())
    }
    /// Scenery has no lifecycle callbacks, so adjacent removals can share a scene
    /// validation. Flush before any scripted/graph prefab to preserve hook order.
    fn destroy_script_prefabs(
        &mut self,
        world: &mut World,
        runtime: &mut ScriptRuntime,
        engine: Arc<ScriptEngine>,
        targets: Vec<String>,
        tokens: &mut BTreeMap<String, String>,
    ) -> Result<()> {
        let mut pending = BTreeSet::new();
        for target in targets {
            if !self.entities.contains_key(&target) || pending.contains(&target) {
                continue;
            }
            let members = &self
                .document
                .prefabs
                .values()
                .find(|prefab| prefab.members.values().any(|id| id == &target))
                .context("Destroy Prefab target is not a live prefab instance")?
                .members;
            let passive = members.values().all(|id| {
                let object = &self.document.objects[self.object_indices[&self.entities[id]]];
                object.blueprints.is_empty()
                    && object
                        .script_manager
                        .as_ref()
                        .is_none_or(|m| m.scripts.is_empty())
            });
            if passive {
                pending.extend(members.values().cloned());
            } else {
                self.destroy_passive_prefabs(world, runtime, &pending)?;
                pending.clear();
                self.destroy_script_prefab(world, runtime, engine.clone(), &target, tokens)?;
            }
        }
        self.destroy_passive_prefabs(world, runtime, &pending)
    }

    fn destroy_passive_prefabs(
        &mut self,
        world: &mut World,
        runtime: &mut ScriptRuntime,
        ids: &BTreeSet<String>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        ensure!(
            !self.document.views.values().any(|id| ids.contains(id)),
            "cannot destroy an active camera; switch views first"
        );
        self.remove_objects_raw(world, ids, false)?;
        runtime.remove_objects(ids);
        Ok(())
    }
    /// `on_destroy` for a destroyed prefab's scripts, then the destroy itself.
    fn destroy_script_prefab(
        &mut self,
        world: &mut World,
        runtime: &mut ScriptRuntime,
        engine: Arc<ScriptEngine>,
        target: &str,
        tokens: &mut BTreeMap<String, String>,
    ) -> Result<()> {
        if !self.entities.contains_key(target) {
            return Ok(());
        }
        let members: Vec<String> = self
            .document
            .prefabs
            .values()
            .find(|prefab| prefab.members.values().any(|id| id == target))
            .context("Destroy Prefab target is not a live prefab instance")?
            .members
            .values()
            .cloned()
            .collect();
        if let Some(boards) = world.resource::<BlueprintRuntime>() {
            engine.lock().copy_boards(boards, &self.document);
        }
        for owner in &members {
            self.run_destroy_hooks(&engine, runtime, owner)?;
        }
        // Lifecycle actions must publish while the members still exist. In
        // particular, the next prefab's callback must read this cleanup state.
        let commands = std::mem::take(&mut engine.lock().commands);
        self.apply_commands(world, runtime, engine.clone(), commands, tokens)?;
        let mut blueprint_runtime = world
            .remove_resource::<BlueprintRuntime>()
            .unwrap_or_default();
        let mut budget = 100_000;
        let result = (|| -> Result<()> {
            for owner in &members {
                self.destroy_blueprint_events(
                    world,
                    &mut blueprint_runtime,
                    owner,
                    GameplayInput::default(),
                    0.,
                    &mut budget,
                )?;
            }
            self.destroy_prefab_raw(world, target)?;
            let ids = members.into_iter().collect();
            blueprint_runtime.remove_objects(&ids);
            runtime.remove_objects(&ids);
            Ok(())
        })();
        world.insert_resource(blueprint_runtime);
        result
    }
    /// `on_destroy` for every script attachment of one object.
    fn run_destroy_hooks(
        &self,
        engine: &ScriptEngine,
        runtime: &mut ScriptRuntime,
        owner: &str,
    ) -> Result<()> {
        let Some(manager) = self
            .document
            .objects
            .iter()
            .find(|object| object.id == owner)
            .and_then(|object| object.script_manager.as_ref())
        else {
            return Ok(());
        };
        for (index, attachment) in manager.scripts.iter().enumerate() {
            {
                let mut host = engine.lock();
                host.owner = owner.to_owned();
                host.attachment = index;
                self.prepare_script_compute(&mut host);
            }
            let Some(compiled) = self.scripts.get(&attachment.script).cloned() else {
                continue;
            };
            let args = vec![Dynamic::from(owner.to_owned())];
            if !compiled.takes("on_destroy", args.len()) {
                continue;
            }
            let key = (owner.to_owned(), index);
            let mut run = runtime.runs.remove(&key).unwrap_or_default();
            let result = engine
                .engine
                .call_fn::<Dynamic>(&mut run.scope, &compiled.ast, "on_destroy", args)
                .map_err(|error| anyhow::anyhow!("script hook on_destroy on '{owner}': {error}"));
            runtime.runs.insert(key, run);
            self.adopt_script_compute(engine);
            result.map(|_| ())?;
        }
        if let Some(mut compute) = self.compute_if_initialized() {
            for index in 0..manager.scripts.len() {
                compute.cancel_owner(&crate::compute::Owner::new(owner, index), true)?;
            }
            compute.materials.remove(owner);
        }
        Ok(())
    }
    // Seed an allocation-free context for ordinary scenes. Only an actual compute API call
    // creates runtime state; this also handles indirect Rhai calls without scanning source text.
    pub(super) fn prepare_script_compute(&self, host: &mut Host) {
        host.compute_ready = true;
        host.compute = self.compute_state.get().cloned();
        if host.compute.is_none() {
            host.compute_capabilities = self.compute_capabilities.clone();
            if host.compute_kernels.len() != self.compute_kernels.len()
                || self.compute_kernels.iter().any(|(id, kernel)| {
                    host.compute_kernels
                        .get(id)
                        .is_none_or(|k| k.id() != kernel.id())
                })
            {
                host.compute_kernels.clone_from(&self.compute_kernels);
            }
        }
    }
    pub(super) fn adopt_script_compute(&self, engine: &ScriptEngine) {
        if self.compute_state.get().is_none()
            && let Some(state) = &engine.lock().compute
        {
            let _ = self.compute_state.set(state.clone());
        }
    }
    /// One transform write, shared by every script transform function.
    fn apply_transform(
        &self,
        world: &mut World,
        target: &str,
        kind: blueprint::NodeKind,
        value: [f32; 3],
    ) -> Result<()> {
        use blueprint::NodeKind as K;
        let entity = *self
            .entities
            .get(target)
            .context("transform target does not exist")?;
        let previous = *world
            .get::<Transform>(entity)
            .context("transform target was removed")?;
        let mut next = previous;
        match kind {
            K::Translate => {
                next.translation = (Vec3::from(next.translation) + Vec3::from(value)).to_array()
            }
            K::Rotate => {
                next.rotation_degrees = (Vec3::from(next.rotation_degrees) + Vec3::from(value))
                    .to_array()
                    .map(|r| r.rem_euclid(360.))
            }
            K::SetPosition => next.translation = value,
            K::SetRotation => next.rotation_degrees = value,
            K::SetScale => next.scale = value,
            _ => anyhow::bail!("not a transform action"),
        }
        next.validate()?;
        if next == previous {
            return Ok(());
        }
        world.insert(entity, next)?;
        if let Err(error) = self.validate_transform_change(world, target) {
            world.insert(entity, previous)?;
            return Err(error);
        }
        Ok(())
    }
    /// One colour write, matching the blueprint `Set Color` node.
    fn apply_color(&self, world: &mut World, target: &str, color: [f32; 3]) -> Result<()> {
        let entity = *self
            .entities
            .get(target)
            .context("Set Color target does not exist")?;
        let has_text = if let Some(mut text) = world.get_mut::<TextRendering>(entity) {
            text.color[..3].copy_from_slice(&color);
            true
        } else {
            false
        };
        if let Some(mut material) = world.get_mut::<Material>(entity) {
            material.set_color(color);
        } else if let Some(mut drawable) = world.get_mut::<Drawable>(entity) {
            // Legacy objects also colour the mesh using its source material.
            drawable.color = color;
        } else {
            ensure!(
                has_text,
                "Set Color needs a mesh, Material or Text Rendering"
            );
        }
        Ok(())
    }
    /// `on_destroy` for every script of a scene being torn down, before its entities go.
    pub(crate) fn scene_script_destroy_events(&mut self, world: &mut World) -> Result<()> {
        let owners: Vec<String> = self
            .document
            .objects
            .iter()
            .filter(|object| object.script_manager.is_some())
            .map(|object| object.id.clone())
            .collect();
        self.object_script_destroy_events(world, &owners)
    }
    pub(crate) fn object_script_destroy_events(
        &mut self,
        world: &mut World,
        owners: &[String],
    ) -> Result<()> {
        if !self.has_scripts() {
            return Ok(());
        }
        let engine = self.script_engine();
        let snapshot = Arc::new(self.collision_snapshot(world)?.0);
        let mut runtime = world.remove_resource::<ScriptRuntime>().unwrap_or_default();
        self.build_view(
            world,
            &mut engine.lock(),
            &runtime,
            &snapshot,
            0.,
            GameplayInput::default(),
        );
        if let Some(boards) = world.resource::<BlueprintRuntime>() {
            engine.lock().copy_boards(boards, &self.document);
        }
        let result = (|| -> Result<()> {
            for owner in owners {
                self.run_destroy_hooks(&engine, &mut runtime, owner)?;
            }
            Ok(())
        })();
        let commands = std::mem::take(&mut engine.lock().commands);
        let mut tokens = std::mem::take(&mut runtime.tokens);
        let applied = self.apply_commands(world, &mut runtime, engine, commands, &mut tokens);
        runtime.tokens = tokens;
        world.insert_resource(runtime);
        result.and(applied)
    }
}

/// Follow a spawn handle to the object it created.
fn resolve(tokens: &BTreeMap<String, String>, target: &str) -> String {
    if target.starts_with(SPAWN_PREFIX) {
        tokens
            .get(target)
            .cloned()
            .unwrap_or_else(|| target.to_owned())
    } else {
        target.to_owned()
    }
}
