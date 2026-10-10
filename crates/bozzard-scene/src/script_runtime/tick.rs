//! One script tick: build each attachment's view, run its hooks, then apply its commands.
use super::*;

impl SceneInstance {
    /// Runs every script attachment once, then applies what they asked for.
    pub fn step_scripts(&mut self, world: &mut World, dt: f32, input: GameplayInput) -> Result<()> {
        if !world
            .resource::<NetworkFrame>()
            .is_some_and(|frame| frame.active)
            && let Some(outbox) = world.resource_mut::<NetworkOutbox>()
        {
            outbox.clear();
        }
        if !crate::game_flow::simulation_running(world) {
            return Ok(());
        }
        self.begin_compute_tick(world);
        if !self.has_scripts() {
            return Ok(());
        }
        ensure!(
            dt.is_finite()
                && dt > 0.
                && input
                    .movement
                    .iter()
                    .chain(&input.orbit)
                    .all(|v| v.is_finite()),
            "invalid script timestep/input"
        );
        // Variables live on the blueprint runtime, so scripts and graphs share one set of boards.
        if world.resource::<BlueprintRuntime>().is_none() {
            world.insert_resource(BlueprintRuntime::default());
        }
        {
            let runtime = world
                .resource_mut::<BlueprintRuntime>()
                .expect("blueprint runtime");
            runtime.initialize_boards(&self.document);
            for object in &self.document.objects {
                if object.script_manager.is_some() {
                    runtime.add_object_defaults(&object.id, &object.blackboard);
                }
            }
        }
        let mut runtime = world.remove_resource::<ScriptRuntime>().unwrap_or_default();
        let result = (|| -> Result<()> {
            runtime.elapsed += dt;
            ensure!(runtime.elapsed.is_finite(), "script clock overflow");
            self.run_scripts(world, &mut runtime, dt, input)
        })();
        runtime
            .tokens
            .retain(|_, id| self.entities.contains_key(id));
        world.insert_resource(runtime);
        result?;
        self.apply_scene_controls(world)
    }
    /// One tick of hook calls plus the command pass that follows them.
    fn run_scripts(
        &mut self,
        world: &mut World,
        runtime: &mut ScriptRuntime,
        dt: f32,
        input: GameplayInput,
    ) -> Result<()> {
        let engine = self.script_engine();
        // Presentation-only scenes can have scripts but no collision geometry.
        // Avoid rebuilding every object's global transform and an empty broad
        // phase on each redraw. Inspect the live world so spawned colliders
        // immediately take the ordinary path on the next tick.
        let has_colliders = world.resource::<crate::physics::Physics>().is_some()
            || self.has_collision_geometry(world);
        let snapshot = Arc::new(if has_colliders {
            self.collision_snapshot(world)?.0
        } else {
            CollisionSnapshot::default()
        });
        // Contacts are only needed by a script that listens for solid collisions.
        let contacts = if self
            .scripts
            .values()
            .any(|script| script.hooks.contains_key("on_collision_enter"))
        {
            let matrices = self.global_transforms(world)?;
            self.blueprint_contacts(world, &snapshot, &matrices)
        } else {
            BTreeMap::new()
        };
        let overlaps = self.script_overlaps(world, &snapshot)?;
        let owners: Vec<(String, Attachments)> = self
            .document
            .objects
            .iter()
            .filter_map(|object| {
                let manager = object.script_manager.as_ref()?;
                Some((
                    object.id.clone(),
                    manager
                        .scripts
                        .iter()
                        .map(|attachment| {
                            (
                                attachment.enabled,
                                self.scripts.get(&attachment.script).cloned(),
                            )
                        })
                        .collect(),
                ))
            })
            .collect();
        bozzard_diagnostics::measure(world, "Script read view", |world| {
            let mut host = engine.lock();
            self.build_view(world, &mut host, runtime, &snapshot, dt, input);
            host.ui_events.clear();
            host.ui_pointer = [-1.; 2];
            host.ui_pointer_blocked = true;
            if let Some(ui) = world.resource_mut::<middleware::ui::Runtime>() {
                host.ui_pointer_blocked = ui.pointer_blocked;
                if let Some(p) = ui.pointer {
                    host.ui_pointer = std::array::from_fn(|i| p[i] / ui.viewport[i].max(1.));
                }
                for event in std::mem::take(&mut ui.script_events) {
                    let mut map = Map::new();
                    map.insert("kind".into(), event.kind.into());
                    map.insert("target".into(), event.target.into());
                    map.insert("x".into(), event.position[0].into());
                    map.insert("y".into(), event.position[1].into());
                    map.insert("delta".into(), event.delta.into());
                    map.insert("blocked".into(), event.blocked.into());
                    host.ui_events.push(Dynamic::from_map(map));
                }
            }
        });
        runtime.stats.hooks = 0;
        runtime.stats.commands = 0;
        runtime.stats.attachments.clear();
        runtime.stats.truncated = false;
        {
            let mut host = engine.lock();
            let Host {
                scene_board,
                object_boards,
                ..
            } = &mut *host;
            world
                .resource_mut::<BlueprintRuntime>()
                .expect("initialized boards")
                .swap_script_boards(scene_board, object_boards);
            host.borrowed_boards = true;
        }
        let hooks = (|| -> Result<()> {
            for (owner, attachments) in owners {
                let overlap = overlaps.get(&owner).cloned().unwrap_or_default();
                let owner_contacts = contacts.get(&owner).cloned().unwrap_or_default();
                for (index, (enabled, compiled)) in attachments.into_iter().enumerate() {
                    let compiled = compiled.with_context(|| {
                    let asset = self
                        .document_attachments(&owner)
                        .get(index)
                        .cloned()
                        .unwrap_or_default();
                    format!(
                        "script '{asset}' on '{owner}': no compiled source is bound; the scene was \
                         opened without loading its script catalog"
                    )
                })?;
                    let key = (owner.clone(), index);
                    engine.lock().attachment = index;
                    let mut run = runtime.runs.remove(&key).unwrap_or_default();
                    let hooks_before = runtime.stats.hooks;
                    let commands_before = engine.lock().commands.len();
                    let result = bozzard_diagnostics::measure(world, "Script hooks", |_| {
                        self.run_attachment(
                            &engine,
                            runtime,
                            &mut run,
                            &owner,
                            &compiled,
                            enabled,
                            &overlap,
                            &owner_contacts,
                            dt,
                        )
                    });
                    if runtime.stats.attachments.len() < MAX_ATTACHMENT_STATS {
                        runtime.stats.attachments.insert(
                            key.clone(),
                            ScriptAttachmentStats {
                                hooks: runtime.stats.hooks - hooks_before,
                                commands: engine.lock().commands.len() - commands_before,
                            },
                        );
                    } else {
                        runtime.stats.truncated = true;
                    }
                    runtime.runs.insert(key, run);
                    self.adopt_script_compute(&engine);
                    if let Err(error) = &result {
                        bozzard_diagnostics::log(
                            world,
                            bozzard_diagnostics::Level::Error,
                            "Script",
                            &format!("{error:#}"),
                            bozzard_diagnostics::Location {
                                object: Some(owner.clone()),
                                attachment: Some(index),
                                node: None,
                                asset: self.document_attachments(&owner).get(index).cloned(),
                                ..Default::default()
                            },
                        );
                    }
                    result?;
                }
            }
            Ok(())
        })();
        engine.lock().return_boards(
            world
                .resource_mut::<BlueprintRuntime>()
                .expect("borrowed boards"),
        );
        hooks?;
        let commands = std::mem::take(&mut engine.lock().commands);
        runtime.stats.commands = commands.len();
        let mut tokens = std::mem::take(&mut runtime.tokens);
        let result = bozzard_diagnostics::measure(world, "Script commands", |world| {
            self.apply_commands(world, runtime, engine, commands, &mut tokens)
        });
        runtime.tokens = tokens;
        result
    }
    /// Seeds readable object state. The caller then lends or copies the blackboards.
    pub(super) fn build_view(
        &self,
        world: &World,
        host: &mut Host,
        runtime: &ScriptRuntime,
        snapshot: &Arc<CollisionSnapshot>,
        dt: f32,
        input: GameplayInput,
    ) {
        host.network = world
            .resource::<NetworkFrame>()
            .cloned()
            .unwrap_or_default();
        host.network_requests = world
            .resource::<NetworkOutbox>()
            .map_or(0, NetworkOutbox::len);
        host.dt = dt;
        host.render = world
            .resource::<bozzard_diagnostics::RenderDiagnostics>()
            .map(|diagnostics| diagnostics.metrics)
            .unwrap_or_default();
        host.simulation = world
            .resource::<bozzard_diagnostics::SimulationMetrics>()
            .copied()
            .unwrap_or_default();
        let viewport = world
            .resource::<middleware::ui::Runtime>()
            .map(|ui| ui.viewport);
        let aspect = viewport
            .filter(|size| size[0] > 0. && size[1] > 0.)
            .map(|size| size[0] / size[1])
            .unwrap_or_else(|| {
                let measured = host.render.counters.viewport_aspect;
                if measured.is_finite() && measured > 0. {
                    measured
                } else {
                    1.
                }
            });
        let camera_id = world
            .resource::<crate::middleware::timeline::Runtime>()
            .and_then(|r| r.cameras.get(&Layer::ThreeD))
            .or_else(|| self.document.views.get(&Layer::ThreeD));
        host.view_projection = camera_id.and_then(|id| {
            let camera = world.get::<Camera>(self.entity(id)?)?;
            Some(camera.projection(aspect).ok()? * self.global_transform(world, id).ok()?.inverse())
        });
        self.prepare_script_compute(host);
        host.elapsed = runtime.elapsed;
        host.loading = self.scene_load_status(world);
        host.settings = crate::player_settings::current(world);
        host.input = input;
        host.tokens = runtime.tokens.clone();
        host.geometry = snapshot.clone();
        host.budget = 1_000_000;
        host.commands.clear();
        host.objects.clear();
        host.object_boards.clear();
        host.scene_board.clear();
        let mut overlaps: BTreeMap<&str, usize> = BTreeMap::new();
        for (a, b) in &snapshot.overlaps {
            *overlaps.entry(a).or_default() += 1;
            *overlaps.entry(b).or_default() += 1;
        }
        let has_text = world.query::<TextRendering>().next().is_some();
        let has_gravity = world.query::<Gravity>().next().is_some();
        let has_grounded = world.query::<GravityState>().next().is_some();
        for (id, entity) in &self.entities {
            let Some(transform) = world.get::<Transform>(*entity) else {
                continue;
            };
            host.objects.insert(
                id.clone(),
                ObjectView {
                    position: transform.translation,
                    rotation: transform.rotation_degrees,
                    scale: transform.scale,
                    text: has_text
                        .then(|| world.get::<TextRendering>(*entity))
                        .flatten()
                        .map(|text| text.text.clone()),
                    rigidbody: has_gravity
                        && world.get::<Gravity>(*entity).is_some_and(|g| g.enabled)
                        && world.get::<PlayerController>(*entity).is_none(),
                    grounded: runtime.moved.get(id).copied().unwrap_or_else(|| {
                        has_grounded
                            && world
                                .get::<GravityState>(*entity)
                                .is_some_and(|state| state.grounded)
                    }),
                    overlaps: overlaps.get(id.as_str()).copied().unwrap_or(0),
                    animation: world
                        .resource::<middleware::animation::Runtime>()
                        .and_then(|r| r.players.get(id))
                        .zip(world.get::<middleware::animation::Animator>(*entity))
                        .map(|(player, animator)| {
                            let state = animator.states.get(player.state);
                            animation_api::View {
                                state: state.map_or_else(String::new, |s| s.name.clone()),
                                progress: state.map_or(0., |s| player.clock.position(1., s.repeat)),
                                playing: player.clock.playing,
                            }
                        }),
                },
            );
        }
    }
    /// Overlap sets for script owners, matching what a blueprint sees for the same object.
    ///
    /// Trigger volumes are not colliders, so each one is tested here; this repeats the inline
    /// overlap pass of the blueprint step, which is scheduled to move onto this helper.
    fn script_overlaps(
        &self,
        world: &World,
        snapshot: &CollisionSnapshot,
    ) -> Result<BTreeMap<String, BTreeSet<String>>> {
        let owners = || {
            self.document.objects.iter().filter(|object| {
                object
                    .script_manager
                    .as_ref()
                    .is_some_and(|manager| !manager.scripts.is_empty())
            })
        };
        let mut result: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for object in owners() {
            result.entry(object.id.clone()).or_default();
        }
        for (a, b) in &snapshot.overlaps {
            if let Some(overlap) = result.get_mut(a) {
                overlap.insert(b.clone());
            }
            if let Some(overlap) = result.get_mut(b) {
                overlap.insert(a.clone());
            }
        }
        // In particular, network presentation scenes have no trigger owners.
        // Their scripts still receive an empty overlap set without a second
        // full-scene hierarchy traversal.
        let trigger_owners: Vec<_> = owners()
            .filter_map(|object| {
                world
                    .get::<Trigger>(self.entities[&object.id])
                    .map(|trigger| trigger.volume)
                    .filter(|volume| volume.enabled)
                    .map(|volume| (object, volume))
            })
            .collect();
        if trigger_owners.is_empty() {
            return Ok(result);
        }
        let matrices = self.global_transforms(world)?;
        for (object, volume) in trigger_owners {
            let entity = self.entities[&object.id];
            let (center, edges, corners) = volume.geometry(matrices[&object.id])?;
            let volume = CollisionBox {
                id: object.id.clone(),
                entity,
                center,
                edges,
                corners,
                layers: volume.layers,
                mask: volume.mask,
            };
            let meets = |other_layers: u32, other_mask: u32| {
                layers_interact(volume.layers, volume.mask, other_layers, other_mask)
            };
            let overlap = result.get_mut(&object.id).expect("script owner");
            for body in &snapshot.boxes {
                if body.id != object.id && meets(body.layers, body.mask) && volume.intersects(body)
                {
                    overlap.insert(body.id.clone());
                }
            }
            for mesh in &snapshot.meshes {
                if mesh.id != object.id && meets(mesh.layers, mesh.mask) && mesh.intersects(&volume)
                {
                    overlap.insert(mesh.id.clone());
                }
            }
        }
        Ok(result)
    }
    /// Calls one attachment's hooks for this tick, in the order the module documents.
    #[allow(clippy::too_many_arguments)]
    fn run_attachment(
        &self,
        engine: &ScriptEngine,
        runtime: &mut ScriptRuntime,
        run: &mut ScriptRun,
        owner: &str,
        compiled: &CompiledScript,
        enabled: bool,
        overlap: &BTreeSet<String>,
        contacts: &[Contact],
        dt: f32,
    ) -> Result<()> {
        let me = |override_owner: Option<&str>| {
            vec![Dynamic::from(override_owner.unwrap_or(owner).to_owned())]
        };
        let mut events: Vec<(&str, Vec<Dynamic>)> = Vec::new();
        if enabled && !run.enabled {
            events.push(("on_enable", me(None)));
        }
        if enabled && !run.started {
            events.push(("on_start", me(None)));
        }
        if enabled {
            let mut update = me(None);
            update.push(Dynamic::from(dt));
            events.push(("on_update", update));
            for other in overlap.difference(&run.overlap) {
                events.push((
                    "on_object_enter",
                    vec![
                        Dynamic::from(owner.to_owned()),
                        Dynamic::from(other.clone()),
                    ],
                ));
            }
            for other in run.overlap.difference(overlap) {
                events.push((
                    "on_object_exit",
                    vec![
                        Dynamic::from(owner.to_owned()),
                        Dynamic::from(other.clone()),
                    ],
                ));
            }
            if !overlap.is_empty() && run.overlap.is_empty() {
                events.push(("on_overlap_enter", me(None)));
            }
            if overlap.is_empty() && !run.overlap.is_empty() {
                events.push(("on_overlap_exit", me(None)));
            }
            for contact in contacts
                .iter()
                .filter(|contact| !run.collisions.contains(&contact.other))
            {
                events.push((
                    "on_collision_enter",
                    vec![
                        Dynamic::from(owner.to_owned()),
                        Dynamic::from(contact.other.clone()),
                        Dynamic::from_array(array_of(contact.normal.to_array())),
                        Dynamic::from(contact.impulse),
                    ],
                ));
            }
        } else if run.enabled {
            events.push(("on_disable", me(None)));
        }
        {
            let mut host = engine.lock();
            host.owner = owner.to_owned();
            host.held = run.held;
            host.random = owner
                .bytes()
                .fold(1, |n, byte| n.wrapping_mul(1099511628211) ^ u64::from(byte));
        }
        if enabled && !run.scope_initialized {
            // Rhai's call_fn evaluates top-level statements then rewinds the scope on every
            // invocation. Evaluate once explicitly so globals survive ticks and a reload gives
            // each attachment fresh script-local state.
            let _ = engine
                .engine
                .eval_ast_with_scope::<Dynamic>(&mut run.scope, &compiled.ast)
                .map_err(|error| anyhow::anyhow!("script initialization on '{owner}': {error}"))?;
            run.scope_initialized = true;
        }
        for (hook, args) in events {
            if !compiled.takes(hook, args.len()) {
                continue;
            }
            runtime.stats.hooks += 1;
            // A hook's return value is ignored: scripts write through engine actions.
            let _ = engine
                .engine
                .call_fn_with_options::<Dynamic>(
                    CallFnOptions::new(),
                    &mut run.scope,
                    &compiled.hook_ast,
                    hook,
                    args,
                )
                .map_err(|error| anyhow::anyhow!("script hook {hook} on '{owner}': {error}"))?;
        }
        if enabled {
            run.started = true;
            run.held = engine.lock().input.binding_mask();
        } else {
            run.held = 0;
        }
        run.overlap = overlap.clone();
        run.collisions = contacts
            .iter()
            .map(|contact| contact.other.clone())
            .collect();
        run.enabled = enabled;
        let attachment = engine.lock().attachment;
        if !enabled && let Some(mut compute) = self.compute_if_initialized() {
            compute.cancel_owner(&crate::compute::Owner::new(owner, attachment), false)?;
        }
        Ok(())
    }
}
