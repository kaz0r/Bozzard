//! The engine functions a script may call, registered onto one interpreter.
use super::*;

/// Registers every engine function a script may call onto one interpreter.
///
/// Reads return a value; writes queue a [`Command`]. Both are declared as one expression over
/// `state`, so a function and its blueprint node stay easy to compare.
pub(super) fn register(host: Arc<Mutex<Host>>) -> Engine {
    let mut engine = Engine::new();
    // Imports are embedded at load time; never read host files during gameplay/replay.
    engine.set_module_resolver(rhai::module_resolvers::DummyModuleResolver);
    engine
        .set_max_operations(MAX_SCRIPT_OPERATIONS)
        .set_max_call_levels(32)
        .set_max_expr_depths(64, 64)
        .set_max_string_size(MAX_SCRIPT_BYTES)
        .set_max_array_size(1 << 16)
        .set_max_map_size(1 << 16);
    compute_api::register(&mut engine, host.clone());
    animation_api::register(&mut engine, host.clone());
    numeric_archive::register(&mut engine);
    macro_rules! borrow {
        ($host:expr) => {
            $host.lock().unwrap_or_else(|error| error.into_inner())
        };
    }
    macro_rules! read {
        ($name:expr, ($($arg:ident : $type:ty),*), |$state:ident| $body:expr) => {{
            let host = host.clone();
            engine.register_fn(
                $name,
                move |$($arg: $type),*| -> Result<Dynamic, Box<EvalAltResult>> {
                    let $state = borrow!(host);
                    let $state: &Host = &$state;
                    $body
                },
            );
        }};
    }
    macro_rules! write {
        ($name:expr, ($($arg:ident : $type:ty),*), |$state:ident| $body:expr) => {{
            let host = host.clone();
            engine.register_fn(
                $name,
                move |$($arg: $type),*| -> Result<(), Box<EvalAltResult>> {
                    let mut $state = borrow!(host);
                    let command: Command = $body;
                    $state.record(command);
                    Ok(())
                },
            );
        }};
    }

    // Object and clock reads, one per blueprint query node.
    write!("set_camera_size", (target: ImmutableString, size: f32), |state| {
        ensure_script(size.is_finite() && size > 0.0, || "camera size must be positive and finite".into())?;
        Command::CameraSize { target: state.target_of(&target)?, size }
    });
    read!("network_active", (), |state| Ok(Dynamic::from(
        state.network.active
    )));
    read!("network_state", (), |state| rhai::serde::to_dynamic(
        &state.network.state
    ));
    read!("network_object", (target: ImmutableString), |state| {
        match state.network.objects.get(target.as_str()) {
            Some(value) => rhai::serde::to_dynamic(value),
            None => Ok(Dynamic::from(Map::new())),
        }
    });
    {
        let host = host.clone();
        engine.register_fn(
            "network_send",
            move |kind: ImmutableString, payload: Map| -> Result<bool, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                if !state.network.active || state.network_requests >= module::MAX_NETWORK_REQUESTS {
                    return Ok(false);
                }
                ensure_script(
                    !kind.is_empty()
                        && kind.len() <= 64
                        && kind
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
                    || "invalid network request kind".into(),
                )?;
                let payload: serde_json::Value =
                    rhai::serde::from_dynamic(&Dynamic::from_map(payload))?;
                let bytes = serde_json::to_vec(&payload).map_err(|e| fail(e.to_string()))?;
                ensure_script(bytes.len() <= module::MAX_NETWORK_REQUEST_BYTES, || {
                    "network request exceeds 4096 bytes".into()
                })?;
                let owner = state.owner.clone();
                state.record(Command::NetworkRequest(NetworkRequest {
                    owner,
                    kind: kind.to_string(),
                    payload,
                }));
                state.network_requests += 1;
                Ok(true)
            },
        );
    }
    read!("delta_time", (), |state| Ok(Dynamic::from(state.dt)));
    read!("elapsed_time", (), |state| Ok(Dynamic::from(state.elapsed)));
    read!("render_stats", (), |state| rhai::serde::to_dynamic(
        state.render
    ));
    read!("simulation_stats", (), |state| rhai::serde::to_dynamic(
        state.simulation
    ));
    read!("fresh_seed", (), |_state| Ok(Dynamic::from(
        fresh_seed_value()
    )));
    read!("scene_loading", (), |state| Ok(Dynamic::from(
        state.loading.phase.busy()
    )));
    read!("scene_load_progress", (), |state| Ok(Dynamic::from(
        state.loading.progress
    )));
    read!("loaded_scene_handle", (), |state| Ok(Dynamic::from(
        state.loading.handle.clone()
    )));
    read!("scene_load_error", (), |state| Ok(Dynamic::from(
        state.loading.error.clone()
    )));
    read!(
        "is_valid_object",
        (target: ImmutableString), |state|
        Ok(Dynamic::from(
            state.objects.contains_key(state.object_id(&target))
        ))
    );
    read!(
        "is_rigidbody",
        (target: ImmutableString), |state|
        Ok(Dynamic::from(state.view(&target)?.rigidbody))
    );
    read!(
        "is_grounded",
        (target: ImmutableString), |state|
        Ok(Dynamic::from(state.view(&target)?.grounded))
    );
    read!(
        "same_object",
        (a: ImmutableString, b: ImmutableString), |state|
        Ok(Dynamic::from(state.object_id(&a) == state.object_id(&b)))
    );
    read!(
        "get_position",
        (target: ImmutableString), |state|
        Ok(Dynamic::from_array(array_of(state.view(&target)?.position)))
    );
    read!(
        "get_rotation",
        (target: ImmutableString), |state|
        Ok(Dynamic::from_array(array_of(state.view(&target)?.rotation)))
    );
    read!(
        "get_scale",
        (target: ImmutableString), |state|
        Ok(Dynamic::from_array(array_of(state.view(&target)?.scale)))
    );
    read!("forward_vector", (target: ImmutableString), |state| {
        // Most scene objects (particularly UI widgets) never need a direction.
        // Calculate it on demand from the read view, including queued rotations.
        let view = state.view(&target)?;
        let direction = crate::physics::forward(&Transform {
            translation: view.position,
            rotation_degrees: view.rotation,
            scale: view.scale,
        });
        Ok(Dynamic::from_array(array_of(direction.to_array())))
    });
    read!("get_text", (target: ImmutableString), |state| {
        let text = state
            .view(&target)?
            .text
            .clone()
            .ok_or_else(|| fail(format!("'{target}' has no Text Rendering")))?;
        Ok(Dynamic::from(text))
    });
    read!(
        "overlap_count",
        (target: ImmutableString), |state|
        Ok(Dynamic::from(state.view(&target)?.overlaps as f32))
    );

    // Input, including the held-key edge that `On Input Pressed` provides in a graph.
    for (name, pressed) in [("input_held", false), ("input_pressed", true)] {
        read!(name, (key: ImmutableString), |state| {
            let key = InputKey::parse(&key).map_err(|error| fail(format!("{error:#}")))?;
            Ok(Dynamic::from(if pressed {
                key.pressed(state.input, state.held)
            } else {
                key.active(state.input)
            }))
        });
    }
    read!("move_x", (), |state| Ok(Dynamic::from(
        state.input.movement[0]
    )));
    read!("move_y", (), |state| Ok(Dynamic::from(
        state.input.movement[1]
    )));
    read!("mouse_x", (), |state| Ok(Dynamic::from(
        state.input.orbit[0]
    )));
    read!("mouse_y", (), |state| Ok(Dynamic::from(
        state.input.orbit[1]
    )));
    read!("ui_events", (), |state| Ok(Dynamic::from(
        state.ui_events.clone()
    )));
    read!("ui_pointer", (), |state| Ok(Dynamic::from_array(
        state.ui_pointer.into_iter().map(Dynamic::from).collect()
    )));

    read!("world_pointer", (), |state| Ok(Dynamic::from_array(
        (if state.ui_pointer_blocked {
            [-1.; 2]
        } else {
            state.ui_pointer
        })
        .into_iter()
        .map(Dynamic::from)
        .collect()
    )));
    read!("screen_ray", (x: f32, y: f32), |state| {
        let mut result = Map::new();
        let valid = x.is_finite() && y.is_finite() && (0.0..=1.0).contains(&x)
            && (0.0..=1.0).contains(&y) && state.view_projection.is_some();
        result.insert("valid".into(), valid.into());
        if valid {
            let inverse = state.view_projection.unwrap().inverse();
            let near = inverse.project_point3(Vec3::new(x * 2. - 1., 1. - y * 2., 0.));
            let far = inverse.project_point3(Vec3::new(x * 2. - 1., 1. - y * 2., 1.));
            result.insert("origin".into(), Dynamic::from_array(array_of(near.to_array())));
            result.insert("direction".into(), Dynamic::from_array(array_of((far-near).normalize_or_zero().to_array())));
        }
        Ok(Dynamic::from_map(result))
    });
    read!("world_to_screen", (position: Array), |state| {
        let point = Vec3::from(vector_of(position)?);
        let value = state.view_projection.map(|projection| {
            let p = projection * point.extend(1.);
            if p.w <= 0. { return [-1.; 3]; }
            [p.x / p.w * 0.5 + 0.5, 0.5 - p.y / p.w * 0.5, p.z / p.w]
        }).unwrap_or([-1.; 3]);
        Ok(Dynamic::from_array(array_of(value)))
    });

    // Blackboards, shared with graphs on the same object or scene.
    for (name, scope) in [
        ("get_object_variable", VariableScope::Object),
        ("get_scene_variable", VariableScope::Scene),
    ] {
        read!(name, (variable: ImmutableString), |state| {
            let entry = state
                .board(scope, &state.owner)?
                .get(variable.as_str())
                .ok_or_else(|| fail(format!("unknown variable '{variable}'")))?;
            match entry {
                B::Scalar(value) => Ok(dynamic_of(value)),
                B::List { .. } => Err(fail(format!(
                    "variable '{variable}' is a list; scripts keep their own arrays"
                ))),
            }
        });
    }
    for (name, scope) in [
        ("get_object_list", VariableScope::Object),
        ("get_scene_list", VariableScope::Scene),
    ] {
        read!(name, (variable: ImmutableString), |state| {
            let entry = state
                .board(scope, &state.owner)?
                .get(variable.as_str())
                .ok_or_else(|| fail(format!("unknown variable '{variable}'")))?;
            match entry {
                B::List { values, .. } => {
                    Ok(Dynamic::from_array(values.iter().map(dynamic_of).collect::<Array>()))
                }
                B::Scalar(_) => Err(fail(format!("variable '{variable}' is a scalar"))),
            }
        });
    }
    // Explicit object targets let a script keep bounded subsystem state on a
    // separate authored blackboard without expanding the per-board limits.
    read!("get_object_variable", (target: ImmutableString, variable: ImmutableString), |state| {
        let owner = state.target_of(&target)?;
        match state.board(VariableScope::Object, &owner)?.get(variable.as_str()) {
            Some(B::Scalar(value)) => Ok(dynamic_of(value)),
            Some(B::List { .. }) => Err(fail(format!("variable '{variable}' is a list"))),
            None => Err(fail(format!("unknown variable '{variable}'"))),
        }
    });
    read!("get_object_list", (target: ImmutableString, variable: ImmutableString), |state| {
        let owner = state.target_of(&target)?;
        match state.board(VariableScope::Object, &owner)?.get(variable.as_str()) {
            Some(B::List { values, .. }) => Ok(Dynamic::from_array(values.iter().map(dynamic_of).collect::<Array>())),
            Some(B::Scalar(_)) => Err(fail(format!("variable '{variable}' is a scalar"))),
            None => Err(fail(format!("unknown variable '{variable}'"))),
        }
    });
    // Compare archived lists in place. Copying both lists into Rhai arrays just
    // to detect a change otherwise allocates and compares hundreds of entries
    // every fixed tick, even while all of the archived data remains unchanged.
    read!("object_lists_equal", (left: ImmutableString, left_name: ImmutableString, right: ImmutableString, right_name: ImmutableString), |state| {
        let left = state.target_of(&left)?;
        let right = state.target_of(&right)?;
        let list = |owner: &str, name: &str| {
            match state.board(VariableScope::Object, owner)?.get(name) {
                Some(B::List { values, .. }) => Ok(values),
                Some(B::Scalar(_)) => Err(fail(format!("variable '{name}' is a scalar"))),
                None => Err(fail(format!("unknown variable '{name}'"))),
            }
        };
        Ok((list(&left, &left_name)? == list(&right, &right_name)?).into())
    });
    // Sparse paged state (such as streamed factories) usually needs one entry,
    // not a fresh Rhai array containing every page in the blackboard list.
    for (name, scope) in [
        ("get_object_list_item", VariableScope::Object),
        ("get_scene_list_item", VariableScope::Scene),
    ] {
        read!(name, (variable: ImmutableString, index: rhai::INT), |state| {
            let entry = state.board(scope, &state.owner)?.get(variable.as_str())
                .ok_or_else(|| fail(format!("unknown variable '{variable}'")))?;
            match entry {
                B::List { values, .. } => {
                    let value = usize::try_from(index).ok().and_then(|i| values.get(i))
                        .ok_or_else(|| fail(format!("list '{variable}' index {index} is out of bounds")))?;
                    Ok(dynamic_of(value))
                }
                B::Scalar(_) => Err(fail(format!("variable '{variable}' is a scalar"))),
            }
        });
    }
    read!("get_object_list_item", (target: ImmutableString, variable: ImmutableString, index: rhai::INT), |state| {
        let owner = state.target_of(&target)?;
        match state.board(VariableScope::Object, &owner)?.get(variable.as_str()) {
            Some(B::List { values, .. }) => {
                let value = usize::try_from(index).ok().and_then(|i| values.get(i))
                    .ok_or_else(|| fail(format!("list '{variable}' index {index} is out of bounds")))?;
                Ok(dynamic_of(value))
            }
            Some(B::Scalar(_)) => Err(fail(format!("variable '{variable}' is a scalar"))),
            None => Err(fail(format!("unknown variable '{variable}'"))),
        }
    });

    // Seeded randomness, matching the `Random` node.
    {
        let host = host.clone();
        engine.register_fn(
            "random",
            move |min: f32, max: f32| -> Result<f32, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                ensure_script(min <= max && (max - min).is_finite(), || {
                    "random needs finite min <= max".into()
                })?;
                state.random = state
                    .random
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let fraction = (state.random >> 40) as f32 / 16777216.;
                Ok(min + (max - min) * fraction)
            },
        );
    }

    // Spatial queries, sharing the collision snapshot the tick already built.
    {
        let host = host.clone();
        engine.register_fn(
            "raycast",
            move |origin: Array,
                  direction: Array,
                  distance: f32,
                  ignore: ImmutableString|
                  -> Result<Map, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                let origin = Vec3::from(vector_of(origin)?);
                let direction = Vec3::from(vector_of(direction)?);
                let ignore = state.object_id(&ignore).to_owned();
                let geometry = state.geometry.clone();
                let hit = geometry
                    .raycast_budget(
                        origin,
                        direction,
                        distance,
                        (!ignore.is_empty()).then_some(ignore.as_str()),
                        u32::MAX,
                        &mut state.budget,
                    )
                    .map_err(|error| fail(format!("{error:#}")))?;
                let (object, position, normal, distance) = match hit {
                    Some(hit) => (
                        hit.object,
                        hit.position.to_array(),
                        hit.normal.to_array(),
                        hit.distance,
                    ),
                    None => (String::new(), [0.; 3], [0.; 3], 0.),
                };
                let mut map = Map::new();
                map.insert("hit".into(), Dynamic::from(!object.is_empty()));
                map.insert("object".into(), Dynamic::from(object));
                map.insert("position".into(), Dynamic::from_array(array_of(position)));
                map.insert("normal".into(), Dynamic::from_array(array_of(normal)));
                map.insert("distance".into(), Dynamic::from(distance));
                Ok(map)
            },
        );
    }
    for (name, sphere) in [("sphere_overlap", true), ("box_overlap", false)] {
        let host = host.clone();
        engine.register_fn(
            name,
            move |center: Array,
                  size: Dynamic,
                  ignore: ImmutableString|
                  -> Result<Array, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                let center = Vec3::from(vector_of(center)?);
                let ignore = state.object_id(&ignore).to_owned();
                let geometry = state.geometry.clone();
                let ignore = (!ignore.is_empty()).then_some(ignore.as_str());
                let hits = if sphere {
                    let radius = number_of(size, "radius")?;
                    geometry.overlap_sphere_budget(
                        center,
                        radius,
                        ignore,
                        u32::MAX,
                        MAX_SCRIPT_OVERLAP,
                        &mut state.budget,
                    )
                } else {
                    let size = vector_of(
                        size.into_array()
                            .map_err(|_| fail("box size must be a vector"))?,
                    )?;
                    geometry.overlap_box_budget(
                        center,
                        Vec3::from(size),
                        ignore,
                        u32::MAX,
                        MAX_SCRIPT_OVERLAP,
                        &mut state.budget,
                    )
                }
                .map_err(|error| fail(format!("{error:#}")))?;
                Ok(hits.into_iter().map(Dynamic::from).collect())
            },
        );
    }
    {
        let host = host.clone();
        engine.register_fn(
            "line_of_sight",
            move |from: Array,
                  to: Array,
                  ignore: ImmutableString|
                  -> Result<bool, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                let from = Vec3::from(vector_of(from)?);
                let delta = Vec3::from(vector_of(to)?) - from;
                let ignore = state.object_id(&ignore).to_owned();
                if delta == Vec3::ZERO {
                    return Ok(true);
                }
                let geometry = state.geometry.clone();
                let blocked = geometry
                    .raycast_budget(
                        from,
                        delta,
                        delta.length(),
                        (!ignore.is_empty()).then_some(ignore.as_str()),
                        u32::MAX,
                        &mut state.budget,
                    )
                    .map_err(|error| fail(format!("{error:#}")))?;
                Ok(blocked.is_none())
            },
        );
    }

    // Writes: every one queues a command instead of touching the world mid-tick.
    write!(
        "set_velocity",
        (target: ImmutableString, velocity: Array), |state|
        Command::SetVelocity {
            target: state.target_of(&target)?,
            velocity: vector_of(velocity)?,
        }
    );
    write!(
        "move_with_collision",
        (target: ImmutableString, delta: Array), |state|
        Command::MoveWithCollision {
            target: state.target_of(&target)?,
            delta: vector_of(delta)?,
        }
    );
    write!("jump", (target: ImmutableString, speed: f32), |state| {
        ensure_script(speed > 0., || "jump speed must be positive".into())?;
        Command::Jump {
            target: state.target_of(&target)?,
            speed,
        }
    });
    for (name, kind) in [
        ("translate", blueprint::NodeKind::Translate),
        ("rotate", blueprint::NodeKind::Rotate),
        ("set_position", blueprint::NodeKind::SetPosition),
        ("set_rotation", blueprint::NodeKind::SetRotation),
        ("set_scale", blueprint::NodeKind::SetScale),
    ] {
        write!(name, (target: ImmutableString, value: Array), |state| {
            let value = vector_of(value)?;
            let target = state.target_of(&target)?;
            state.mirror_transform(&target, kind, value);
            Command::Transform {
                target,
                kind,
                value,
            }
        });
    }
    write!("reset_interpolation", (target: ImmutableString), |state| {
        Command::ResetInterpolation { target: state.target_of(&target)? }
    });
    write!("set_color", (target: ImmutableString, color: Array), |state| {
        let color = vector_of(color)?;
        ensure_script(color.iter().all(|c| (0.0..=1.0).contains(c)), || {
            "colour components must be in 0..1".into()
        })?;
        Command::Color {
            target: state.target_of(&target)?,
            color,
        }
    });
    write!("set_mesh", (target: ImmutableString, asset: ImmutableString), |state| {
        ensure_script(!asset.is_empty() && asset.len() <= 256, || "mesh asset ID must contain 1..256 bytes".into())?;
        Command::Mesh { target: state.target_of(&target)?, asset: asset.to_string() }
    });
    write!(
        "set_visible",
        (target: ImmutableString, visible: bool), |state|
        Command::Visible {
            target: state.target_of(&target)?,
            visible,
        }
    );
    // Flat triples [tile X, tile Z, visibility/tint] plus per-object exceptions.
    // Zero removes geometry and lighting; 0..1 dims; an empty view restores normal rendering.
    write!("set_tile_view", (cells: Array, exterior: f32, objects: Map), |state| {
        ensure_script(cells.len().is_multiple_of(3) && cells.len() <= 300_000
            && objects.len() <= 100_000 && exterior.is_finite() && (0.0..=1.0).contains(&exterior),
            || "invalid tile view".into())?;
        let mut view = tile_view::TileView { exterior: Some(exterior), ..Default::default() };
        for row in cells.chunks_exact(3) {
            let p = vector_of(row.to_vec())?;
            ensure_script(p[0].fract() == 0.0 && p[1].fract() == 0.0
                && p[0].abs() <= 65_536.0 && p[1].abs() <= 65_536.0
                && (0.0..=1.0).contains(&p[2]), || "invalid tile view cell".into())?;
            view.cells.insert((p[0] as i32, p[1] as i32), p[2]);
        }
        for (id, value) in objects {
            let factor = number_of(value, "tile view factor")?;
            ensure_script((0.0..=1.0).contains(&factor), || "invalid tile view object".into())?;
            view.objects.insert(state.target_of(&id)?, factor);
        }
        Command::TileView(view)
    });
    write!(
        "set_text",
        (target: ImmutableString, text: ImmutableString), |state|
        {
            ensure_script(text.len() <= 4096, || "text exceeds 4096 UTF-8 bytes".into())?;
            let target = state.target_of(&target)?;
            let text = text.to_string();
            // A later read in this tick sees the queued text.
            state.mirror_text(&target, text.clone());
            Command::Text { target, text }
        }
    );
    write!(
        "set_light_intensity",
        (target: ImmutableString, intensity: f32), |state|
        Command::LightIntensity {
            target: state.target_of(&target)?,
            intensity,
        }
    );
    write!("set_light_color", (target: ImmutableString, color: Array), |state| {
        Command::LightColor {
            target: state.target_of(&target)?,
            color: vector_of(color)?,
        }
    });
    for (name, ambient) in [("set_sun_light", false), ("set_ambient_light", true)] {
        write!(name, (color: Array, intensity: f32), |_state| {
            let color = vector_of(color)?;
            ensure_script(color.iter().all(|v| (0.0..=1.).contains(v))
                && intensity.is_finite() && (0.0..=100_000.).contains(&intensity),
                || "light needs RGB in 0..1 and intensity in 0..100000".into())?;
            Command::SceneLight { ambient, color, intensity }
        });
    }
    write!("set_environment", (zenith: Array, horizon: Array, ground: Array, intensity: f32), |_state| {
        let zenith = vector_of(zenith)?;
        let horizon = vector_of(horizon)?;
        let ground = vector_of(ground)?;
        ensure_script([zenith, horizon, ground].iter().flatten().all(|v| (0.0..=1.).contains(v))
            && intensity.is_finite() && (0.0..=1000.).contains(&intensity),
            || "environment needs RGB in 0..1 and intensity in 0..1000".into())?;
        Command::Environment { zenith, horizon, ground, intensity }
    });
    write!("set_fog", (color: Array, density: f32), |_state| {
        let color = vector_of(color)?;
        ensure_script(color.iter().all(|v| (0.0..=1.).contains(v))
            && density.is_finite() && (0.0..=1000.).contains(&density),
            || "fog needs RGB in 0..1 and density in 0..1000".into())?;
        Command::Fog { color, density }
    });
    write!("set_star_intensity", (intensity: f32), |_state| {
        ensure_script(intensity.is_finite() && (0.0..=1000.).contains(&intensity),
            || "star intensity must be in 0..1000".into())?;
        Command::Stars(intensity)
    });
    write!("set_ui_text", (target: ImmutableString, text: ImmutableString), |state| {
        ensure_script(text.len() <= 4096, || "UI text exceeds 4096 UTF-8 bytes".into())?;
        Command::Ui { target: state.target_of(&target)?, control: middleware::ui::Control::Text(text.to_string()) }
    });
    write!("set_ui_visible", (target: ImmutableString, visible: bool), |state| {
        Command::Ui { target: state.target_of(&target)?, control: middleware::ui::Control::Visible(visible) }
    });
    write!("set_ui_enabled", (target: ImmutableString, enabled: bool), |state| {
        Command::Ui { target: state.target_of(&target)?, control: middleware::ui::Control::Enabled(enabled) }
    });
    write!("set_ui_opacity", (target: ImmutableString, opacity: f32), |state| {
        ensure_script(opacity.is_finite() && (0.0..=1.).contains(&opacity), || "UI opacity must be in 0..1".into())?;
        Command::Ui { target: state.target_of(&target)?, control: middleware::ui::Control::Opacity(opacity) }
    });
    write!("set_ui_size", (target: ImmutableString, width: f32, height: f32), |state| {
        ensure_script([width, height].iter().all(|n| n.is_finite() && (0.0..=10000.).contains(n)), || "invalid UI size".into())?;
        Command::Ui { target: state.target_of(&target)?, control: middleware::ui::Control::Size([width, height]) }
    });
    write!("clear_ui_focus", (), |_state| Command::Ui {
        target: String::new(),
        control: middleware::ui::Control::ClearFocus
    });
    write!("set_ui_background", (target: ImmutableString, rgba: Array), |state| {
        ensure_script(rgba.len() == 4, || "UI background needs RGBA".into())?;
        let mut color = [0.; 4];
        for (slot, value) in color.iter_mut().zip(rgba) { *slot = number_of(value, "UI color")?; }
        ensure_script(color.iter().all(|n| (0.0..=1.).contains(n)), || "UI color must be in 0..1".into())?;
        Command::Ui { target: state.target_of(&target)?, control: middleware::ui::Control::Background(color) }
    });
    write!("set_ui_world_position", (target: ImmutableString, position: Array), |state| {
        Command::Ui { target: state.target_of(&target)?, control: middleware::ui::Control::WorldPosition(vector_of(position)?) }
    });
    for (name, screen) in [("set_ui_screen_position", true), ("set_ui_offset", false)] {
        write!(name, (target: ImmutableString, x: f32, y: f32), |state| {
            ensure_script([x, y].iter().all(|n| n.is_finite() && n.abs() <= 10000.), || "invalid UI position".into())?;
            Command::Ui { target: state.target_of(&target)?, control: if screen {
                middleware::ui::Control::ScreenPosition([x, y])
            } else { middleware::ui::Control::Offset([x, y]) } }
        });
    }
    for (name, kind) in [
        ("set_focus_distance", blueprint::NodeKind::SetFocusDistance),
        ("set_aperture", blueprint::NodeKind::SetAperture),
        ("set_fog_density", blueprint::NodeKind::SetFogDensity),
        (
            "set_fog_light_intensity",
            blueprint::NodeKind::SetFogLightIntensity,
        ),
        ("set_exposure", blueprint::NodeKind::SetExposure),
        (
            "set_bloom_intensity",
            blueprint::NodeKind::SetBloomIntensity,
        ),
        ("set_saturation", blueprint::NodeKind::SetSaturation),
        ("set_heat_strength", blueprint::NodeKind::SetHeatStrength),
        (
            "set_grain_intensity",
            blueprint::NodeKind::SetGrainIntensity,
        ),
        (
            "set_vignette_intensity",
            blueprint::NodeKind::SetVignetteIntensity,
        ),
    ] {
        let host = host.clone();
        engine.register_fn(name, move |value: f32| -> Result<(), Box<EvalAltResult>> {
            let mut state = borrow!(host);
            ensure_script(value.is_finite(), || "value must be finite".into())?;
            state.record(Command::Display { kind, value });
            Ok(())
        });
    }
    {
        let host = host.clone();
        engine.register_fn(
            "spawn_prefab",
            move |asset: ImmutableString,
                  position: Array|
                  -> Result<ImmutableString, Box<EvalAltResult>> {
                let mut state = borrow!(host);
                let position = vector_of(position)?;
                let serial = state.next_token_serial;
                state.next_token_serial = state
                    .next_token_serial
                    .checked_add(1)
                    .ok_or_else(|| fail("script spawn handle counter exhausted"))?;
                let token = format!("{SPAWN_PREFIX}{}/{serial}", state.owner);
                let owner = state.owner.clone();
                state.record(Command::Spawn {
                    owner,
                    token: token.clone(),
                    asset: asset.to_string(),
                    position,
                });
                // The handle is a real target for the rest of the tick and for later ticks.
                state.tokens.insert(token.clone(), token.clone());
                state.objects.insert(
                    token.clone(),
                    ObjectView {
                        position,
                        scale: [1.; 3],
                        ..Default::default()
                    },
                );
                Ok(token.into())
            },
        );
    }
    write!(
        "destroy_prefab",
        (target: ImmutableString), |state|
        Command::Destroy {
            target: state.target_of(&target)?,
        }
    );
    for (name, script) in [("set_graph_enabled", false), ("set_script_enabled", true)] {
        write!(
            name,
            (target: ImmutableString, index: i64, enabled: bool), |state|
            {
                ensure_script(index >= 0, || "attachment index must be nonnegative".into())?;
                let target = state.target_of(&target)?;
                if script {
                    Command::ScriptEnabled {
                        target,
                        index: index as usize,
                        enabled,
                    }
                } else {
                    Command::GraphEnabled {
                        target,
                        index: index as usize,
                        enabled,
                    }
                }
            }
        );
    }
    for (name, requested) in [("lock_cursor", true), ("unlock_cursor", false)] {
        let host = host.clone();
        engine.register_fn(name, move || {
            borrow!(host).record(Command::Cursor(requested));
        });
    }
    {
        let host = host.clone();
        engine.register_fn("quit_game", move || {
            borrow!(host).record(Command::QuitGame);
        });
    }
    // Player settings: reads see this tick's earlier edits, like blackboard variables.
    {
        use crate::player_settings::{Quality, Request, VolumeChannel, WindowMode};
        read!("player_settings", (), |state| rhai::serde::to_dynamic(
            state.settings
        ));
        read!("get_window_mode_setting", (), |state| Ok(Dynamic::from(
            state.settings.window_mode.name().to_string()
        )));
        read!("get_window_size_setting", (), |state| Ok(
            Dynamic::from_array(
                state
                    .settings
                    .window_size
                    .iter()
                    .map(|&size| Dynamic::from(rhai::INT::from(size)))
                    .collect()
            )
        ));
        read!("get_vsync_setting", (), |state| Ok(Dynamic::from(
            state.settings.vsync
        )));
        read!("get_quality_setting", (), |state| Ok(Dynamic::from(
            state.settings.quality.name().to_string()
        )));
        read!("get_volume_setting", (channel: ImmutableString), |state| {
            let channel = VolumeChannel::parse(&channel).map_err(settings_error)?;
            Ok(Dynamic::from(state.settings.volume(channel)))
        });
        write!("set_window_mode_setting", (mode: ImmutableString), |state| {
            settings_command(&mut state.settings, WindowMode::parse(&mode).map(Request::WindowMode))?
        });
        write!("set_window_size_setting", (width: rhai::INT, height: rhai::INT), |state| {
            let size = (|| {
                Ok([
                    crate::player_settings::window_dimension(width as f32)?,
                    crate::player_settings::window_dimension(height as f32)?,
                ])
            })();
            settings_command(&mut state.settings, size.map(Request::WindowSize))?
        });
        write!("set_vsync_setting", (enabled: bool), |state| {
            settings_command(&mut state.settings, Ok(Request::Vsync(enabled)))?
        });
        write!("set_quality_setting", (preset: ImmutableString), |state| {
            settings_command(&mut state.settings, Quality::parse(&preset).map(Request::Quality))?
        });
        write!("set_volume_setting", (channel: ImmutableString, volume: f32), |state| {
            let request = VolumeChannel::parse(&channel).map(|channel| Request::Volume(channel, volume));
            settings_command(&mut state.settings, request)?
        });
        for (name, request) in [
            ("apply_settings", Request::Apply),
            ("save_settings", Request::Save),
            ("reset_settings", Request::Reset),
        ] {
            write!(name, (), |state| {
                settings_command(&mut state.settings, Ok(request))?
            });
        }
    }
    {
        let host = host.clone();
        engine.register_fn("end_game", move |message: ImmutableString| {
            borrow!(host).record(Command::EndGame(message.to_string()));
        });
    }
    for (name, kind) in [
        ("load_scene", blueprint::NodeKind::LoadScene),
        ("add_scene", blueprint::NodeKind::AddScene),
        ("load_scene_async", blueprint::NodeKind::LoadSceneAsync),
        ("add_scene_async", blueprint::NodeKind::AddSceneAsync),
        ("unload_scene", blueprint::NodeKind::UnloadScene),
        ("save_game", blueprint::NodeKind::SaveGame),
        ("load_game", blueprint::NodeKind::LoadGame),
    ] {
        let host = host.clone();
        engine.register_fn(name, move |scene: ImmutableString| {
            borrow!(host).record(Command::SceneControl {
                kind,
                name: scene.to_string(),
            });
        });
    }
    {
        let host = host.clone();
        engine.register_fn("cancel_scene_load", move || {
            borrow!(host).record(Command::SceneControl {
                kind: blueprint::NodeKind::CancelSceneLoad,
                name: String::new(),
            });
        });
    }
    {
        let host = host.clone();
        engine.register_fn("restart_scene", move || {
            borrow!(host).record(Command::SceneControl {
                kind: blueprint::NodeKind::RestartScene,
                name: String::new(),
            });
        });
    }
    for (name, scope) in [
        ("set_object_variable", VariableScope::Object),
        ("set_scene_variable", VariableScope::Scene),
    ] {
        write!(
            name,
            (variable: ImmutableString, value: Dynamic), |state|
            {
                let owner = state.owner.clone();
                let declared = match state.board(scope, &owner)?.get(variable.as_str()) {
                    Some(B::Scalar(declared)) => declared.kind(),
                    Some(B::List { .. }) => {
                        return Err(fail(format!(
                            "variable '{variable}' is a list; scripts keep their own arrays"
                        )));
                    }
                    None => return Err(fail(format!("unknown variable '{variable}'"))),
                };
                let value = scalar_of(value, declared)?;
                state.mirror_variable(scope, &owner, variable.to_string(), B::Scalar(value.clone()));
                Command::Variable {
                    scope,
                    owner,
                    name: variable.to_string(),
                    value,
                }
            }
        );
    }
    write!("set_object_variable", (target: ImmutableString, variable: ImmutableString, value: Dynamic), |state| {
        let owner = state.target_of(&target)?;
        let declared = match state.board(VariableScope::Object, &owner)?.get(variable.as_str()) {
            Some(B::Scalar(value)) => value.kind(),
            Some(B::List { .. }) => return Err(fail(format!("variable '{variable}' is a list"))),
            None => return Err(fail(format!("unknown variable '{variable}'"))),
        };
        let value = scalar_of(value, declared)?;
        state.mirror_variable(VariableScope::Object, &owner, variable.to_string(), B::Scalar(value.clone()));
        Command::Variable { scope: VariableScope::Object, owner, name: variable.to_string(), value }
    });
    for (name, scope) in [
        ("set_object_list", VariableScope::Object),
        ("set_scene_list", VariableScope::Scene),
    ] {
        write!(name, (variable: ImmutableString, values: Array), |state| {
            let owner = state.owner.clone();
            let (element, capacity) = match state.board(scope, &owner)?.get(variable.as_str()) {
                Some(B::List {
                    element, capacity, ..
                }) => (*element, *capacity),
                Some(B::Scalar(_)) => {
                    return Err(fail(format!("variable '{variable}' is a scalar")));
                }
                None => return Err(fail(format!("unknown variable '{variable}'"))),
            };
            ensure_script(values.len() <= capacity, || {
                format!("list '{variable}' exceeds its capacity of {capacity}")
            })?;
            let values = values
                .into_iter()
                .map(|value| scalar_of(value, element))
                .collect::<Result<Vec<_>, _>>()?;
            let replacement = B::List {
                element,
                capacity,
                values: values.clone(),
            };
            state.mirror_variable(scope, &owner, variable.to_string(), replacement);
            Command::ListVariable {
                scope,
                owner,
                name: variable.to_string(),
                values,
            }
        });
    }
    write!("set_object_list", (target: ImmutableString, variable: ImmutableString, values: Array), |state| {
        let owner = state.target_of(&target)?;
        let (element, capacity) = match state.board(VariableScope::Object, &owner)?.get(variable.as_str()) {
            Some(B::List { element, capacity, .. }) => (*element, *capacity),
            Some(B::Scalar(_)) => return Err(fail(format!("variable '{variable}' is a scalar"))),
            None => return Err(fail(format!("unknown variable '{variable}'"))),
        };
        ensure_script(values.len() <= capacity, || format!("list '{variable}' exceeds its capacity of {capacity}"))?;
        let values = values.into_iter().map(|v| scalar_of(v, element)).collect::<Result<Vec<_>, _>>()?;
        state.mirror_variable(VariableScope::Object, &owner, variable.to_string(),
            B::List { element, capacity, values: values.clone() });
        Command::ListVariable { scope: VariableScope::Object, owner, name: variable.to_string(), values }
    });
    // `print` is a Rhai keyword, so it is captured through the engine's own output hook rather
    // than registered as a function.
    {
        let host = host.clone();
        engine.on_print(move |text| {
            let mut host = borrow!(host);
            let owner = host.owner.clone();
            host.record(Command::Print {
                level: bozzard_diagnostics::Level::Info,
                owner,
                text: text.to_owned(),
            });
        });
    }

    for (name, level) in [
        ("log_info", bozzard_diagnostics::Level::Info),
        ("log_warning", bozzard_diagnostics::Level::Warning),
        ("log_error", bozzard_diagnostics::Level::Error),
    ] {
        let host = host.clone();
        engine.register_fn(name, move |text: ImmutableString| {
            let mut host = borrow!(host);
            let owner = host.owner.clone();
            host.record(Command::Print {
                level,
                owner,
                text: text.to_string(),
            });
        });
    }

    // The scalar vocabulary of a blueprint graph that Rhai does not already provide.
    engine.register_fn("lerp", |a: f32, b: f32, t: f32| -> f32 {
        a * (1. - t) + b * t
    });
    engine.register_fn(
        "lerp_vector",
        |a: Array, b: Array, t: f32| -> Result<Array, Box<EvalAltResult>> {
            let (a, b) = (Vec3::from(vector_of(a)?), Vec3::from(vector_of(b)?));
            Ok(array_of((a * (1. - t) + b * t).to_array()))
        },
    );
    engine.register_fn(
        "clamp",
        |value: f32, min: f32, max: f32| -> Result<f32, Box<EvalAltResult>> {
            ensure_script(min <= max, || "clamp needs min <= max".into())?;
            Ok(value.clamp(min, max))
        },
    );
    engine.register_fn(
        "length",
        |value: Array| -> Result<f32, Box<EvalAltResult>> {
            Ok(Vec3::from(vector_of(value)?).length())
        },
    );
    engine.register_fn(
        "normalize",
        |value: Array| -> Result<Array, Box<EvalAltResult>> {
            Ok(array_of(
                Vec3::from(vector_of(value)?).normalize_or_zero().to_array(),
            ))
        },
    );
    engine.register_fn(
        "dot",
        |a: Array, b: Array| -> Result<f32, Box<EvalAltResult>> {
            Ok(Vec3::from(vector_of(a)?).dot(Vec3::from(vector_of(b)?)))
        },
    );
    engine.register_fn(
        "cross",
        |a: Array, b: Array| -> Result<Array, Box<EvalAltResult>> {
            Ok(array_of(
                Vec3::from(vector_of(a)?)
                    .cross(Vec3::from(vector_of(b)?))
                    .to_array(),
            ))
        },
    );
    engine.register_fn(
        "distance",
        |a: Array, b: Array| -> Result<f32, Box<EvalAltResult>> {
            Ok(Vec3::from(vector_of(a)?).distance(Vec3::from(vector_of(b)?)))
        },
    );
    engine.register_fn(
        "add_vector",
        |a: Array, b: Array| -> Result<Array, Box<EvalAltResult>> {
            Ok(array_of(
                (Vec3::from(vector_of(a)?) + Vec3::from(vector_of(b)?)).to_array(),
            ))
        },
    );
    engine.register_fn(
        "scale_vector",
        |value: Array, factor: f32| -> Result<Array, Box<EvalAltResult>> {
            Ok(array_of(
                (Vec3::from(vector_of(value)?) * factor).to_array(),
            ))
        },
    );
    macro_rules! axis {
        ($name:literal, $axis:literal) => {
            engine.register_fn($name, |value: Array| -> Result<f32, Box<EvalAltResult>> {
                Ok(vector_of(value)?[$axis])
            });
        };
    }
    axis!("vector_x", 0);
    axis!("vector_y", 1);
    axis!("vector_z", 2);
    engine.register_fn(
        "modulo",
        |a: f32, b: f32| -> Result<f32, Box<EvalAltResult>> {
            ensure_script(b != 0., || "modulo by zero".into())?;
            Ok(a.rem_euclid(b))
        },
    );
    engine.register_fn("pow", |a: f32, b: f32| -> f32 { a.powf(b) });
    engine.register_fn("atan2", |y: f32, x: f32| -> f32 { y.atan2(x) });
    // Rhai spells these `ceiling` and `**`; blueprint authors expect the node names as well.
    engine.register_fn("ceil", |value: f32| -> f32 { value.ceil() });
    engine.register_fn("to_radians", |value: f32| -> f32 { value.to_radians() });
    engine.register_fn("to_degrees", |value: f32| -> f32 { value.to_degrees() });
    engine
}

fn settings_error(error: anyhow::Error) -> Box<EvalAltResult> {
    fail(format!("{error:#}"))
}
/// Validate a settings request against the host's mirror, then queue it.
fn settings_command(
    settings: &mut crate::player_settings::PlayerSettings,
    request: anyhow::Result<crate::player_settings::Request>,
) -> Result<Command, Box<EvalAltResult>> {
    let request = request.map_err(settings_error)?;
    settings.change(&request).map_err(settings_error)?;
    Ok(Command::Settings(request))
}
