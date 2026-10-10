use super::view::retire_pending_compute_jobs;
use super::*;
#[test]
fn surface_loss_reconfigures_at_most_three_times_and_success_resets_the_budget() {
    let mut recovery = SurfaceRecovery::default();
    for _ in 0..3 {
        recovery.lost().unwrap();
    }
    assert!(
        recovery
            .lost()
            .unwrap_err()
            .to_string()
            .contains("3 reconfigurations")
    );
    recovery.presented();
    recovery.lost().unwrap();
}
#[test]
fn device_loss_retires_pending_readback_with_an_explicit_outcome() {
    use bozzard_scene::compute::{BindingKind, Capabilities, JobState, Kernel, Owner, Scope};
    let mut player = authored_player();
    let owner = Owner::new("pending", 0);
    let ticket = player.demo.with_instance(|instance, _| {
        instance.set_compute_capabilities(Capabilities {
            backend: Some("test".into()),
            device_generation: 1,
            max_buffer_bytes: 1024,
            ..Default::default()
        });
        let kernel = Kernel::parse(
            "@group(0) @binding(0) var<storage, read_write> data: array<u32>; \
             @compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) \
             { data[id.x] += 1u; }",
        )
        .unwrap();
        let BindingKind::Storage { layout, .. } =
            &kernel.entry("main").unwrap().binding("data").unwrap().kind
        else {
            panic!("expected a storage buffer")
        };
        let mut compute = instance.compute();
        let buffer = compute
            .runtime
            .create_buffer(
                &owner,
                Scope::Attachment,
                "values",
                Arc::new(layout.clone()),
                4,
            )
            .unwrap();
        compute.runtime.readback(&owner, buffer).unwrap()
    });
    let retired = retire_pending_compute_jobs(&mut player.demo).unwrap();
    assert_eq!(retired.len(), 1);
    assert_eq!(retired[0].1, ticket.serial());
    assert!(retired[0].3);
    player.demo.with_instance(|instance, _| {
        let mut compute = instance.compute();
        assert_eq!(
            compute.runtime.job(&owner, ticket).unwrap().state,
            JobState::Cancelled
        );
        assert!(
            compute
                .runtime
                .take_result(&owner, ticket, 4)
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
    });
}
fn authored_player() -> Player {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/first-trail.json");
    let document = load_document(Some(&path)).unwrap();
    let assets = assets::Assets::load(&document, Some(&path)).unwrap();
    let demo = SceneRuntime::new(&document).unwrap();
    let options = Options {
        scene: Some(path),
        ..Default::default()
    };
    Player::new(options, demo, assets)
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires a desktop and GPU; exercises cold uploads in a live swapchain"]
fn newly_placed_factory_models_keep_the_window_presenting() -> Result<()> {
    const MACHINES: [&str; 10] = [
        "smelter",
        "miner",
        "constructor",
        "assembler",
        "splitter",
        "merger",
        "storage",
        "generator",
        "belt",
        "pole",
    ];
    struct Probe {
        player: Player,
        placed: usize,
        last_placement_tick: u64,
    }
    impl ApplicationHandler for Probe {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.player.resumed(event_loop);
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            self.player.window_event(event_loop, id, event);
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.player.about_to_wait(event_loop);
            if event_loop.exiting()
                || self.placed == MACHINES.len()
                || self.player.frames < 10 + self.placed as u32 * 20
            {
                return;
            }
            let name = MACHINES[self.placed];
            let mesh = format!("machine-{name}-{name}-mk1");
            let result = (|| -> Result<()> {
                ensure!(
                    self.player
                        .view
                        .as_ref()
                        .unwrap()
                        .renderer
                        .model_upload_stats(&mesh)
                        .is_none(),
                    "{name} was preloaded; the test must exercise a cold upload"
                );
                let position = [
                    (self.placed % 3) as f32 * 1.5 - 1.5,
                    0.,
                    (self.placed / 3) as f32 * 1.7 - 0.85,
                ];
                self.player.demo.with_instance(|instance, world| {
                    instance.spawn_prefab(world, &format!("machine-{name}"), position)?;
                    if self.placed < 4 || name == "generator" {
                        instance.spawn_prefab(world, &format!("{name}-power-off"), position)?;
                    }
                    Ok::<_, anyhow::Error>(())
                })?;
                Ok(())
            })();
            if let Err(error) = result {
                self.player.fail(event_loop, error);
                return;
            }
            self.placed += 1;
            self.last_placement_tick = self.player.demo.app.ticks();
        }
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut catalog = load_document(Some(&path))?.assets;
    catalog.retain(|id, _| {
        MACHINES
            .iter()
            .any(|name| id == &format!("machine-{name}") || id == &format!("{name}-power-off"))
    });
    // Declare spawnable templates without creating any model at startup.
    let spawn_nodes: Vec<_> = catalog
        .keys()
        .enumerate()
        .map(|(i, asset)| {
            serde_json::json!({
                "id": i + 1, "position": [0,0], "kind": "spawn_prefab", "prefab": asset,
                "inputs": ["exec", {"vector": [0,0,0]}]
            })
        })
        .collect();
    let document = bozzard_scene::Scene::from_json(&serde_json::json!({
        "version": 1, "name": "Cold factory model placement", "assets": catalog,
        "views": {"3d": "camera"},
        "objects": [{
            "id": "camera", "name": "Camera",
            "transform": {"translation": [20,20,20], "rotation_degrees": [-35.264,45,0], "scale": [1,1,1]},
            "camera": {"projection": "orthographic", "vertical_size": 6, "near": 0.1, "far": 100}
        }, {
            "id": "templates", "name": "Untriggered spawn declarations",
            "transform": {"translation": [0,0,0], "rotation_degrees": [0,0,0], "scale": [1,1,1]},
            "blueprints": [{"enabled": true, "graph": {
                "version": 1, "name": "Templates", "nodes": spawn_nodes, "wires": []
            }}]
        }]
    }).to_string())?;
    let mut player = authored_player();
    player.demo = SceneRuntime::new_with_prefabs(&document, Some(&path))?;
    player.demo.set_threaded_simulation(true)?;
    player.assets = assets::Assets::load(player.demo.instance().document(), Some(&path))?;
    player.options.scene = Some(path);
    player.options.frames = Some(230);
    let mut probe = Probe {
        player,
        placed: 0,
        last_placement_tick: 0,
    };
    let mut builder = EventLoop::builder();
    winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
    winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
    builder.build()?.run_app(&mut probe)?;
    if let Some(error) = probe.player.error {
        return Err(error);
    }
    ensure!(
        probe.placed == MACHINES.len() && probe.player.frames == 230,
        "presentation stopped before all machines were placed"
    );
    for name in MACHINES {
        ensure!(
            probe
                .player
                .view
                .as_ref()
                .unwrap()
                .renderer
                .model_upload_stats(&format!("machine-{name}-{name}-mk1"))
                .is_some(),
            "{name} was never uploaded"
        );
    }
    ensure!(
        probe.player.demo.app.ticks() > probe.last_placement_tick,
        "simulation stopped during streaming"
    );
    Ok(())
}

#[test]
fn the_pointer_is_never_captured_outside_a_running_game() {
    let base = CursorCaptureState {
        paused: false,
        focused: true,
        layer: Layer::ThreeD,
        gameplay: true,
        running: true,
        ui_wants_pointer: false,
        requested: None,
    };
    assert!(
        cursor_capture_wanted(base),
        "a running game owns the pointer"
    );
    assert!(
        !cursor_capture_wanted(CursorCaptureState {
            running: false,
            // Scenes with graphs report gameplay input in the run menu too.
            requested: Some(true),
            ..base
        }),
        "the run menu, pause overlay and win screen are clicked with a visible pointer"
    );
    assert!(!cursor_capture_wanted(CursorCaptureState {
        requested: Some(false),
        ..base
    }));
    assert!(!cursor_capture_wanted(CursorCaptureState {
        focused: false,
        ..base
    }));
    assert!(!cursor_capture_wanted(CursorCaptureState {
        paused: true,
        ..base
    }));
    assert!(!cursor_capture_wanted(CursorCaptureState {
        layer: Layer::TwoD,
        ..base
    }));
    assert!(!cursor_capture_wanted(CursorCaptureState {
        gameplay: false,
        ..base
    }));
    assert!(!cursor_capture_wanted(CursorCaptureState {
        ui_wants_pointer: true,
        ..base
    }));
    assert!(
        cursor_capture_wanted(CursorCaptureState {
            ui_wants_pointer: true,
            requested: Some(true),
            ..base
        }),
        "an explicit Lock Cursor request controls mouse-look during gameplay"
    );
}

#[test]
#[cfg(all(feature = "steam", target_os = "linux"))]
#[ignore = "requires Steam overlay injection and a desktop; opens Friends during 600 frames, sends no invitations"]
fn steam_overlay_activates_in_native_window_and_keeps_presenting() -> Result<()> {
    use bozzard_scene::middleware::ui::Input;
    struct Probe {
        player: Player,
        requested: bool,
        active_frame: Option<u32>,
        overlay_frames: u32,
    }
    impl ApplicationHandler for Probe {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.player.resumed(event_loop);
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            let before = self.player.frames;
            let active = self.player.demo.steam_overlay_active();
            self.player.window_event(event_loop, id, event);
            if active {
                self.overlay_frames += self.player.frames - before;
            }
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.player.about_to_wait(event_loop);
            if !self.requested
                && self.player.frames >= 60
                && bozzard_runtime::steam_runtime::overlay_available()
            {
                self.requested = true;
                for id in ["coop-open-title", "coop-steam-overlay"] {
                    if let Err(error) = self.player.ui_input(Input::ActivateObject(id.into())) {
                        self.player.fail(event_loop, error);
                        return;
                    }
                }
            }
            if self.player.demo.steam_overlay_active() {
                self.active_frame.get_or_insert(self.player.frames);
            }
        }
    }
    let _shutdown = bozzard_runtime::steam_runtime::ShutdownGuard;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let document = load_document(Some(&path))?;
    bozzard_runtime::steam_runtime::initialize_player(&document)?;
    let mut player = authored_player();
    player.demo = SceneRuntime::new_with_prefabs(&document, Some(&path))?;
    player.assets = assets::Assets::load(player.demo.instance().document(), Some(&path))?;
    player.options.scene = Some(path);
    player.options.frames = Some(600);
    player.demo.enable_multiplayer(None)?;
    let mut probe = Probe {
        player,
        requested: false,
        active_frame: None,
        overlay_frames: 0,
    };
    let mut builder = EventLoop::builder();
    winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
    winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
    builder.build()?.run_app(&mut probe)?;
    if let Some(error) = probe.player.error {
        return Err(error);
    }
    ensure!(
        probe.requested,
        "Steam did not inject its overlay into the native window"
    );
    let active = probe
        .active_frame
        .context("Steam never reported GameOverlayActivated")?;
    ensure!(
        probe.overlay_frames > 5,
        "frames stopped while overlay was active"
    );
    println!(
        "steam_overlay_ok activated_at={active} presented={} overlay_frames={}",
        probe.player.frames, probe.overlay_frames
    );
    Ok(())
}

#[test]
fn steam_overlay_blocks_native_shortcuts_without_pausing() -> Result<()> {
    let mut player = authored_player();
    player.demo.set_steam_overlay_active(true);
    for code in [
        KeyCode::KeyW,
        KeyCode::Space,
        KeyCode::Escape,
        KeyCode::F5,
        KeyCode::F6,
    ] {
        player.dispatch_keyboard(
            PhysicalKey::Code(code),
            &Key::Named(NamedKey::Escape),
            ElementState::Pressed,
            false,
            false,
        )?;
    }
    assert!(!player.paused);
    assert!(player.simulation_elapsed(Instant::now()).is_some());
    assert!(!player.demo.accepts_gameplay_input());
    player.demo.set_steam_overlay_active(false);
    assert!(player.demo.accepts_gameplay_input());
    Ok(())
}

#[test]
#[cfg(feature = "steam")]
#[ignore = "requires local Steam; creates/leaves a solo lobby without sending invitations or chat"]
fn factory_lobby_preserves_native_frames_keyboard_and_chat_capture() -> Result<()> {
    use bozzard_scene::{BlueprintRuntime, blueprint::Value, middleware::ui::Input};
    let _shutdown = bozzard_runtime::steam_runtime::ShutdownGuard;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let document = load_document(Some(&path))?;
    let mut player = authored_player();
    player.demo = SceneRuntime::new_with_prefabs(&document, Some(&path))?;
    player.demo.app.step();
    player.demo.check_simulation()?;
    player.demo.enable_multiplayer(None)?;
    player.demo.pump_multiplayer()?;
    player.gameplay_controls.event(&WindowEvent::Focused(true));
    for id in ["coop-open-title", "coop-create"] {
        player.ui_input(Input::ActivateObject(id.into()))?;
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while !player.demo.multiplayer_active() {
        ensure!(Instant::now() < deadline, "Steam lobby did not start");
        std::thread::sleep(Duration::from_millis(20));
        player.demo.pump_multiplayer()?;
    }
    for id in ["coop-close", "title-create"] {
        player.ui_input(Input::ActivateObject(id.into()))?;
        player.demo.app.step();
        player.demo.check_simulation()?;
        player.demo.pump_multiplayer()?;
    }
    let value = |player: &Player, name: &str| match player
        .demo
        .app
        .world
        .resource::<BlueprintRuntime>()
        .unwrap()
        .scene_blackboard()[name]
        .values()[0]
    {
        Value::Number(v) => v,
        _ => panic!("missing {name}"),
    };
    let key = |player: &mut Player, code: KeyCode, logical: Key, state: ElementState| {
        player.dispatch_keyboard(PhysicalKey::Code(code), &logical, state, false, false)
    };
    let x = value(&player, "cursor_x");
    key(
        &mut player,
        KeyCode::KeyD,
        Key::Character("d".into()),
        ElementState::Pressed,
    )?;
    player.demo.app.step();
    player.demo.check_simulation()?;
    assert_eq!(value(&player, "cursor_x"), x + 1.);
    key(
        &mut player,
        KeyCode::KeyD,
        Key::Character("d".into()),
        ElementState::Released,
    )?;
    let z = value(&player, "cursor_z");
    key(
        &mut player,
        KeyCode::Enter,
        Key::Named(NamedKey::Enter),
        ElementState::Pressed,
    )?;
    assert!(player.demo.multiplayer_chatting());
    key(
        &mut player,
        KeyCode::KeyW,
        Key::Character("w".into()),
        ElementState::Pressed,
    )?;
    assert!(player.demo.multiplayer_text("Unsent test"));
    player.demo.app.step();
    player.demo.check_simulation()?;
    assert_eq!(value(&player, "cursor_z"), z);
    key(
        &mut player,
        KeyCode::Escape,
        Key::Named(NamedKey::Escape),
        ElementState::Pressed,
    )?;
    assert!(!player.demo.multiplayer_chatting());
    key(
        &mut player,
        KeyCode::KeyW,
        Key::Character("w".into()),
        ElementState::Released,
    )?;
    player.demo.set_threaded_simulation(true)?;
    let before = value(&player, "ticks");
    for _ in 0..60 {
        let elapsed = player
            .simulation_elapsed(player.last_frame + Duration::from_secs_f64(1. / 60.))
            .context("co-op disabled native simulation")?;
        player.demo.advance_with_frame(elapsed, || ())?;
        player.demo.pump_multiplayer()?;
    }
    assert!(value(&player, "ticks") > before);
    assert_eq!(
        value(&player, "cursor_z"),
        z,
        "chat left a held movement key"
    );
    Ok(())
}

#[test]
fn middleware_menu_keeps_a_free_pointer_and_accepts_slider_clicks() {
    use bozzard_scene::middleware::ui::Input;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/middleware-lab.json");
    let scene = load_document(Some(&path)).unwrap();
    let mut demo = SceneRuntime::new_with_prefabs(&scene, Some(&path)).unwrap();
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, Layer::ThreeD, [1280., 720.])
        .unwrap();
    assert!(frame.wants_pointer());
    assert!(
        !cursor_capture_wanted(CursorCaptureState {
            paused: false,
            focused: true,
            layer: Layer::ThreeD,
            gameplay: demo.accepts_gameplay_input(),
            running: true,
            ui_wants_pointer: frame.wants_pointer(),
            requested: None,
        }),
        "Blueprint menu actions must not hide or lock the pointer"
    );
    let slider = frame.element("volume").unwrap();
    let point = [
        slider.rect.min[0] + slider.rect.size[0] * 0.8,
        slider.rect.min[1] + slider.rect.size[1] * 0.5,
    ];
    assert!(
        demo.ui_input(Layer::ThreeD, frame.size, Input::PointerDown(point))
            .unwrap()
    );
    assert!(
        demo.ui_input(Layer::ThreeD, frame.size, Input::PointerUp(point))
            .unwrap()
    );
    let updated = demo
        .instance()
        .ui_frame(&demo.app.world, Layer::ThreeD, frame.size)
        .unwrap();
    assert!(updated.element("volume").unwrap().value > slider.value);
}

#[test]
fn game_menu_keys_do_not_repeat_or_leak_into_gameplay() {
    use bozzard_scene::{GamePhase as P, TextRendering};
    let mut player = authored_player();
    let scene = Scene::from_json(include_str!(
        "../../../examples/demo/scenes/game-flow-lab.json"
    ))
    .unwrap();
    player.demo = SceneRuntime::new(&scene).unwrap();
    player.gameplay_controls.event(&WindowEvent::Focused(true));
    let key = |player: &mut Player, code, repeat, synthetic| {
        player
            .dispatch_keyboard(
                PhysicalKey::Code(code),
                &Key::Character("ignored".into()),
                ElementState::Pressed,
                repeat,
                synthetic,
            )
            .unwrap();
    };
    key(&mut player, KeyCode::Enter, true, false);
    key(&mut player, KeyCode::Enter, false, true);
    assert_eq!(player.demo.game_session().unwrap().phase, P::Ready);
    key(&mut player, KeyCode::Space, false, false);
    key(&mut player, KeyCode::Enter, false, false);
    player.demo.app.step();
    let counter = player.demo.instance().entity("counter").unwrap();
    assert_eq!(
        player
            .demo
            .app
            .world
            .get::<TextRendering>(counter)
            .unwrap()
            .text,
        "Taps: 0"
    );
    key(&mut player, KeyCode::Escape, false, false);
    key(&mut player, KeyCode::Escape, true, false);
    assert_eq!(player.demo.game_session().unwrap().phase, P::Paused);
    key(&mut player, KeyCode::Space, false, false);
    key(&mut player, KeyCode::Enter, false, false);
    player.demo.app.step();
    assert_eq!(
        player
            .demo
            .app
            .world
            .get::<TextRendering>(counter)
            .unwrap()
            .text,
        "Taps: 0"
    );
    for _ in 0..3 {
        key(&mut player, KeyCode::Space, false, false);
        player.demo.app.step();
    }
    player.demo.check_simulation().unwrap();
    assert_eq!(player.demo.game_session().unwrap().phase, P::GameOver);
    key(&mut player, KeyCode::Enter, false, false);
    assert_eq!(player.demo.game_session().unwrap().phase, P::Playing);
    player
        .game_pointer_event(&WindowEvent::Focused(false))
        .unwrap();
    assert_eq!(player.demo.game_session().unwrap().phase, P::Paused);
    key(&mut player, KeyCode::KeyQ, false, false);
    assert_eq!(player.demo.game_session().unwrap().phase, P::Quit);
}

#[test]
fn earth_factory_escape_uses_the_menu_instead_of_the_window_exit_shortcut() {
    let mut player = authored_player();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let scene = load_document(Some(&path)).unwrap();
    player.demo = SceneRuntime::new_with_prefabs(&scene, Some(&path)).unwrap();
    player.demo.app.step();
    player.demo.check_simulation().unwrap();
    player
        .ui_input(bozzard_scene::middleware::ui::Input::ActivateObject(
            "title-create".into(),
        ))
        .unwrap();
    player.demo.app.step();
    player.demo.check_simulation().unwrap();
    player.gameplay_controls.event(&WindowEvent::Focused(true));
    let escape = |player: &mut Player, state, repeat| {
        player
            .dispatch_keyboard(
                PhysicalKey::Code(KeyCode::Escape),
                &Key::Named(NamedKey::Escape),
                state,
                repeat,
                false,
            )
            .unwrap()
    };
    escape(&mut player, ElementState::Pressed, false);
    assert!(player.menu_input.consumed(KeyCode::Escape));
    player.demo.app.step();
    player.demo.check_simulation().unwrap();
    let open = |player: &Player| {
        player
            .demo
            .instance()
            .ui_frame(&player.demo.app.world, Layer::ThreeD, [1280., 720.])
            .unwrap()
            .element("menu-panel")
            .is_some()
    };
    assert!(open(&player));
    assert!(bozzard_scene::game_flow::simulation_running(
        &player.demo.app.world
    ));
    escape(&mut player, ElementState::Pressed, true);
    player.demo.app.step();
    assert!(open(&player), "holding Escape must not toggle repeatedly");
    escape(&mut player, ElementState::Released, false);
    escape(&mut player, ElementState::Pressed, false);
    player.demo.app.step();
    assert!(!open(&player));
    player.demo.check_simulation().unwrap();
}
#[test]
fn a_scene_assigned_key_reaches_gameplay_instead_of_a_menu_command() {
    use bozzard_scene::GamePhase as P;
    let mut player = authored_player();
    let scene = Scene::from_json(include_str!(
        "../../../examples/demo/scenes/game-flow-lab.json"
    ))
    .unwrap();
    player.demo = SceneRuntime::new(&scene).unwrap();
    player.gameplay_controls.event(&WindowEvent::Focused(true));
    // Start the run, then press a key only a scene would use.
    player
        .dispatch_keyboard(
            PhysicalKey::Code(KeyCode::Enter),
            &Key::Named(NamedKey::Enter),
            ElementState::Pressed,
            false,
            false,
        )
        .unwrap();
    assert_eq!(player.demo.game_session().unwrap().phase, P::Playing);
    let before = player
        .demo
        .app
        .world
        .resource::<bozzard_scene::GameplayInput>()
        .copied();
    player
        .dispatch_keyboard(
            PhysicalKey::Code(KeyCode::KeyF),
            &Key::Character("f".into()),
            ElementState::Pressed,
            false,
            false,
        )
        .unwrap();
    let after = player
        .demo
        .app
        .world
        .resource::<bozzard_scene::GameplayInput>()
        .copied()
        .unwrap();
    assert!(
        after.keys & bozzard_scene::keys::bit("F") != 0,
        "an assigned key must reach the scene, not the app's command path"
    );
    assert!(before.unwrap_or_default().keys & bozzard_scene::keys::bit("F") == 0);
    // Released again, the key leaves gameplay alone.
    player
        .dispatch_keyboard(
            PhysicalKey::Code(KeyCode::KeyF),
            &Key::Character("f".into()),
            ElementState::Released,
            false,
            false,
        )
        .unwrap();
    assert_eq!(
        player
            .demo
            .app
            .world
            .resource::<bozzard_scene::GameplayInput>()
            .copied()
            .unwrap()
            .keys
            & bozzard_scene::keys::bit("F"),
        0
    );
}

#[test]
fn blueprint_input_without_player_controller_toggles_rendered_mesh() {
    let mut player = authored_player();
    let scene = bozzard_scene::Scene::from_json(include_str!(
        "../../../examples/demo/scenes/blueprint-lab.json"
    ))
    .unwrap();
    player.demo = SceneRuntime::new(&scene).unwrap();
    player.gameplay_controls.event(&WindowEvent::Focused(true));
    let visible = |p: &Player| {
        p.demo
            .instance()
            .view(&p.demo.app.world, Layer::ThreeD, 1.)
            .unwrap()
            .objects
            .len()
    };
    let count = visible(&player);
    for (pressed, expected) in [(true, count - 1), (false, count - 1), (true, count)] {
        player
            .dispatch_keyboard(
                PhysicalKey::Code(KeyCode::Space),
                &Key::Named(NamedKey::Space),
                if pressed {
                    ElementState::Pressed
                } else {
                    ElementState::Released
                },
                false,
                false,
            )
            .unwrap();
        player.demo.app.step();
        player.demo.check_simulation().unwrap();
        assert!(!player.paused);
        assert_eq!(visible(&player), expected);
    }
}

#[test]
fn combined_dispatch_reserves_gameplay_positions_and_restarts_physically() {
    let mut player = authored_player();
    player.gameplay_controls.event(&WindowEvent::Focused(true));
    player.demo.app.step();
    player.command_error = Some("retained status".into());
    for code in [
        KeyCode::KeyS,
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyD,
        KeyCode::Space,
        KeyCode::KeyP,
    ] {
        player
            .dispatch_keyboard(
                PhysicalKey::Code(code),
                &Key::Character("r".into()),
                ElementState::Pressed,
                false,
                false,
            )
            .unwrap();
        assert_eq!(player.demo.app.ticks(), 1, "logical R must not reload");
        assert_eq!(player.command_error.as_deref(), Some("retained status"));
        let input = player
            .demo
            .app
            .world
            .resource::<bozzard_scene::GameplayInput>()
            .unwrap();
        if code == KeyCode::KeyS {
            assert_eq!(input.movement, [0.0, -1.0], "Colemak backward still moves");
        }
        if code == KeyCode::Space {
            assert!(input.jump);
            assert!(!player.paused);
        }
        player
            .dispatch_keyboard(
                PhysicalKey::Code(code),
                &Key::Character("r".into()),
                ElementState::Released,
                false,
                false,
            )
            .unwrap();
    }
    // Reservation applies to all logical commands, not just restart.
    player.options.save_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for code in [
        KeyCode::KeyS,
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyD,
        KeyCode::Space,
    ] {
        for logical in [Key::Character("1".into()), Key::Named(NamedKey::F5)] {
            player
                .dispatch_keyboard(
                    PhysicalKey::Code(code),
                    &logical,
                    ElementState::Pressed,
                    false,
                    false,
                )
                .unwrap();
            assert_eq!(player.options.layer, Layer::ThreeD);
            assert_eq!(player.command_error.as_deref(), Some("retained status"));
        }
    }
    for (state, repeat, synthetic) in [
        (ElementState::Pressed, true, false),
        (ElementState::Released, false, false),
        (ElementState::Pressed, false, true),
    ] {
        player
            .dispatch_keyboard(
                PhysicalKey::Code(KeyCode::KeyR),
                &Key::Character("p".into()),
                state,
                repeat,
                synthetic,
            )
            .unwrap();
        assert_eq!(player.demo.app.ticks(), 1);
    }
    player
        .dispatch_keyboard(
            PhysicalKey::Code(KeyCode::KeyR),
            &Key::Character("p".into()),
            ElementState::Pressed,
            false,
            false,
        )
        .unwrap();
    assert_eq!(player.demo.app.ticks(), 0);
    assert!(player.command_error.is_none());
    assert_eq!(player.gameplay_controls.current().movement, [0.0; 2]);

    // The same physical S/logical R still reloads legacy non-controller scenes.
    player.options.scene = None;
    player.demo = SceneRuntime::new(&bozzard_runtime::scene_document().unwrap()).unwrap();
    player.demo.app.step();
    player
        .dispatch_keyboard(
            PhysicalKey::Code(KeyCode::KeyS),
            &Key::Character("r".into()),
            ElementState::Pressed,
            false,
            false,
        )
        .unwrap();
    assert_eq!(player.demo.app.ticks(), 0);
    player
        .dispatch_keyboard(
            PhysicalKey::Code(KeyCode::Space),
            &Key::Named(NamedKey::Space),
            ElementState::Pressed,
            false,
            false,
        )
        .unwrap();
    assert!(player.paused);
}

#[test]
fn scripted_factory_receives_enter_digits_and_r_instead_of_viewer_shortcuts() {
    use bozzard_scene::blueprint::{BlackboardValue, Value};

    let mut player = authored_player();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let mut document = load_document(Some(&path)).unwrap();
    document
        .blackboard
        .insert("seed".into(), BlackboardValue::Scalar(Value::Number(4.)));
    // This input-routing fixture builds a free belt from the original demo bar.
    // Normal progression starts on Production, where slot 2 is a locked miner.
    document.blackboard.insert(
        "demo_mode".into(),
        BlackboardValue::Scalar(Value::Bool(true)),
    );
    player.options.scene = Some(path.clone());
    player.demo = SceneRuntime::new_with_prefabs(&document, Some(&path)).unwrap();
    // The initial window may already be focused without emitting Focused(true).
    for _ in 0..8 {
        player.demo.app.step();
        player.demo.check_simulation().unwrap();
    }
    let press = |player: &mut Player, code, logical| {
        player
            .dispatch_keyboard(
                PhysicalKey::Code(code),
                &logical,
                ElementState::Pressed,
                false,
                false,
            )
            .unwrap();
        player.demo.app.advance(Duration::from_millis(20));
        player.demo.check_simulation().unwrap();
    };
    let number = |player: &Player, name: &str| {
        let board = player
            .demo
            .app
            .world
            .resource::<bozzard_scene::BlueprintRuntime>()
            .unwrap();
        let BlackboardValue::Scalar(Value::Number(value)) =
            board.scene_blackboard().get(name).unwrap()
        else {
            panic!("{name} is not a number");
        };
        *value
    };
    let start_x = number(&player, "cursor_x");
    press(&mut player, KeyCode::KeyD, Key::Character("d".into()));
    assert_eq!(number(&player, "cursor_x"), start_x + 1.0);
    player
        .dispatch_keyboard(
            PhysicalKey::Code(KeyCode::KeyD),
            &Key::Character("d".into()),
            ElementState::Released,
            false,
            false,
        )
        .unwrap();
    player.demo.app.step();
    press(&mut player, KeyCode::Enter, Key::Named(NamedKey::Enter));
    let board = player
        .demo
        .app
        .world
        .resource::<bozzard_scene::BlueprintRuntime>()
        .unwrap();
    assert!(matches!(
        board.scene_blackboard().get("started"),
        Some(BlackboardValue::Scalar(Value::Bool(true)))
    ));
    press(&mut player, KeyCode::Digit2, Key::Character("2".into()));
    assert_eq!(number(&player, "selected"), 2.0);
    assert_eq!(player.options.layer, Layer::ThreeD);

    // The outer corner is always empty: deposits scatter only within -6..=6.
    for (code, logical, count) in [
        (
            KeyCode::KeyD,
            Key::Character("d".into()),
            7 - number(&player, "cursor_x") as i32,
        ),
        (
            KeyCode::KeyS,
            Key::Character("s".into()),
            7 - number(&player, "cursor_z") as i32,
        ),
    ] {
        for _ in 0..count {
            press(&mut player, code, logical.clone());
            player
                .dispatch_keyboard(
                    PhysicalKey::Code(code),
                    &logical,
                    ElementState::Released,
                    false,
                    false,
                )
                .unwrap();
            player.demo.app.step();
        }
    }
    let ticks = player.demo.app.ticks();
    press(&mut player, KeyCode::KeyR, Key::Character("r".into()));
    assert_eq!(number(&player, "direction"), 1.0);
    assert!(
        player.demo.app.ticks() > ticks,
        "R must advance gameplay instead of reloading the scene"
    );
    press(&mut player, KeyCode::Space, Key::Named(NamedKey::Space));
    let board = player
        .demo
        .app
        .world
        .resource::<bozzard_scene::BlueprintRuntime>()
        .unwrap();
    let Some(BlackboardValue::List { values, .. }) = board.scene_blackboard().get("builds") else {
        panic!("factory builds list is missing");
    };
    assert_eq!(values.last(), Some(&Value::Number(2.0)));

    player
        .dispatch_keyboard(
            PhysicalKey::Code(KeyCode::KeyR),
            &Key::Character("r".into()),
            ElementState::Released,
            false,
            false,
        )
        .unwrap();
    player.demo.app.step();
    press(
        &mut player,
        KeyCode::ControlLeft,
        Key::Named(NamedKey::Control),
    );
    let input = player
        .gameplay_controls
        .event(&WindowEvent::ModifiersChanged(
            winit::keyboard::ModifiersState::CONTROL.into(),
        ))
        .unwrap();
    player.demo.set_gameplay_input(input);
    press(&mut player, KeyCode::KeyR, Key::Character("r".into()));
    assert!(number(&player, "camera_progress") > 0. && number(&player, "camera_progress") < 1.);
    for _ in 0..40 {
        player.demo.app.step();
        player.demo.check_simulation().unwrap();
    }
    assert_eq!(number(&player, "camera_heading"), 90.);
    assert_eq!(
        number(&player, "direction"),
        1.,
        "Ctrl+R must not rotate the build tool"
    );

    player
        .dispatch_keyboard(
            PhysicalKey::Code(KeyCode::F6),
            &Key::Named(NamedKey::F6),
            ElementState::Pressed,
            false,
            false,
        )
        .unwrap();
    assert_eq!(player.demo.app.ticks(), 0, "F6 reloads the authored scene");
    // Reload returns to the title. Enter activates the focused Create button.
    player.demo.app.step();
    player.demo.check_simulation().unwrap();
    assert!(
        player
            .ui_input(bozzard_scene::middleware::ui::Input::Focus(
                "title-create".into(),
            ))
            .unwrap()
    );
    press(
        &mut player,
        KeyCode::NumpadEnter,
        Key::Named(NamedKey::Enter),
    );
    let board = player
        .demo
        .app
        .world
        .resource::<bozzard_scene::BlueprintRuntime>()
        .unwrap();
    assert!(matches!(
        board.scene_blackboard().get("started"),
        Some(BlackboardValue::Scalar(Value::Bool(true)))
    ));
}

#[test]
fn command_errors_survive_gameplay_titles_until_a_successful_command() {
    let mut player = authored_player();
    // Failed commands survive simulation/redraw titles and ordinary gameplay keys.
    let source = player.options.scene.clone();
    player.options.scene = Some(PathBuf::from("__bozzard_missing_scene__/missing.json"));
    assert!(
        player
            .handle_key(&Key::Character("r".into()), false)
            .is_err()
    );
    let failure = player.command_error.clone().unwrap();
    for _ in 0..3 {
        player.demo.app.step();
        player
            .handle_key(&Key::Character("w".into()), false)
            .unwrap();
        player
            .handle_key(&Key::Character("r".into()), true)
            .unwrap();
        assert!(player.window_title().contains(&format!("ERROR: {failure}")));
    }
    player.options.scene = source;
    player
        .handle_key(&Key::Character("r".into()), false)
        .unwrap();
    assert!(player.command_error.is_none());
    assert!(!player.window_title().contains("ERROR:"));
    // Saving to a directory reliably fails without depending on host permissions.
    player.options.save_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(player.handle_key(&Key::Named(NamedKey::F5), false).is_err());
    player.demo.app.step();
    assert!(player.window_title().contains("ERROR:"));
    player
        .handle_key(&Key::Character("r".into()), false)
        .unwrap();
    assert!(!player.window_title().contains("ERROR:"));
}

#[test]
fn authored_player_space_does_not_pause_and_restart_clears_win_and_input() {
    let mut player = authored_player();
    let document = load_document(player.options.scene.as_deref()).unwrap();
    player
        .handle_key(&Key::Named(NamedKey::Space), false)
        .unwrap();
    assert!(!player.paused);
    let camera = player.demo.instance().camera_entity(Layer::ThreeD).unwrap();
    let pose = *player.demo.app.world.get::<Transform>(camera).unwrap();
    player
        .handle_key(&Key::Named(NamedKey::ArrowLeft), false)
        .unwrap();
    assert_eq!(
        *player.demo.app.world.get::<Transform>(camera).unwrap(),
        pose
    );
    for tick in 0..340 {
        player
            .demo
            .set_gameplay_input(bozzard_scene::GameplayInput {
                movement: [0.0, 1.0],
                jump: tick == 80,
                ..Default::default()
            });
        player.demo.app.step();
    }
    player.demo.check_simulation().unwrap();
    assert!(player.demo.gameplay().unwrap().won);
    player
        .handle_key(&Key::Character("r".into()), false)
        .unwrap();
    let state = player.demo.gameplay().unwrap();
    assert!(!state.won && state.collected.is_empty() && state.checkpoint.is_none());
    assert_eq!(
        player
            .demo
            .instance()
            .capture(&player.demo.app.world)
            .unwrap(),
        document
    );
    assert_eq!(
        player
            .demo
            .app
            .world
            .resource::<bozzard_scene::GameplayInput>()
            .unwrap()
            .movement,
        [0.0; 2]
    );
}
#[test]
fn view_pause_pan_and_transactional_reload_work_without_a_gpu() {
    let assets = assets::Assets::load(&bozzard_runtime::scene_document().unwrap(), None).unwrap();
    let demo = SceneRuntime::new(&bozzard_runtime::scene_document().unwrap()).unwrap();
    let mut player = Player::new(Options::default(), demo, assets);
    player
        .handle_key(&Key::Named(NamedKey::Space), false)
        .unwrap();
    assert!(player.paused);
    player
        .handle_key(&Key::Named(NamedKey::Space), true)
        .unwrap();
    assert!(player.paused, "key repeat must not toggle pause");
    player
        .handle_key(&Key::Character("1".into()), false)
        .unwrap();
    assert_eq!(player.options.layer, Layer::TwoD);
    player
        .handle_key(&Key::Named(NamedKey::ArrowRight), false)
        .unwrap();
    let camera = player.demo.instance().camera_entity(Layer::TwoD).unwrap();
    assert_eq!(
        player
            .demo
            .app
            .world
            .get::<Transform>(camera)
            .unwrap()
            .translation[0],
        0.25
    );
    player.options.scene = Some(PathBuf::from("__bozzard_missing_scene__/missing.json"));
    assert!(
        player
            .handle_key(&Key::Character("r".into()), false)
            .is_err()
    );
    assert_eq!(
        player.demo.instance().camera_entity(Layer::TwoD).unwrap(),
        camera
    );
    player.options.scene = None;
    player
        .handle_key(&Key::Character("r".into()), false)
        .unwrap();
    assert_ne!(
        player.demo.instance().camera_entity(Layer::TwoD).unwrap(),
        camera
    );
    assert_eq!(player.demo.app.ticks(), 0);
}

#[test]
fn reload_keeps_the_game_pack_and_multiplayer_session_settings() -> Result<()> {
    use bozzard_scene::middleware::ui::Input;
    // An exported game runs from a pack whose assets never hot reload, before or after R.
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter-2d");
    let pack = std::env::temp_dir().join(format!(
        "bozzard-player-reload-{}.bpack",
        std::process::id()
    ));
    bozzard_project::gamepack::write(&source, &pack, &Default::default())?;
    let mut options = Options {
        project: Some(pack.clone()),
        ..Default::default()
    };
    let resolved = project::resolve(&mut options);
    std::fs::remove_file(&pack)?;
    resolved?;
    let document = load_document(options.scene.as_deref())?;
    let (demo, assets) = start_session(&document, &options)?;
    let mut player = Player::new(options, demo, assets);
    assert!(!player.assets.hot_reload());
    player.handle_key(&Key::Character("r".into()), false)?;
    assert!(
        !player.assets.hot_reload(),
        "R re-enabled hot reload for packed assets"
    );

    // Earth Factory's co-op menu belongs to the multiplayer session startup creates.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/earth-factory/scenes/earth.json");
    let options = Options {
        scene: Some(path.clone()),
        ..Default::default()
    };
    let (demo, assets) = start_session(&load_document(Some(&path))?, &options)?;
    let mut player = Player::new(options, demo, assets);
    let coop_menu_opens = |player: &mut Player| -> Result<bool> {
        player.demo.app.step();
        player.demo.check_simulation()?;
        player.ui_input(Input::ActivateObject("coop-open-title".into()))?;
        // The open co-op menu consumes Escape.
        Ok(player.demo.multiplayer_key("Escape"))
    };
    assert!(coop_menu_opens(&mut player)?);
    player.handle_key(&Key::Named(NamedKey::F6), false)?;
    assert!(
        coop_menu_opens(&mut player)?,
        "F6 dropped the co-op multiplayer session"
    );
    Ok(())
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires a desktop and GPU timestamp queries; presents 40 frames"]
fn reload_keeps_gpu_profiling_in_frame_runs() -> Result<()> {
    struct Probe {
        player: Player,
        /// GPU timings recorded before R.
        reloaded: Option<usize>,
    }
    impl ApplicationHandler for Probe {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.player.resumed(event_loop);
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            self.player.window_event(event_loop, id, event);
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            self.player.about_to_wait(event_loop);
            if self.reloaded.is_none() && self.player.frames >= 10 && !event_loop.exiting() {
                self.reloaded = self
                    .player
                    .view
                    .as_ref()
                    .map(|view| view.gpu_frame_ms.len());
                if let Err(error) = self.player.handle_key(&Key::Character("r".into()), false) {
                    self.player.fail(event_loop, error);
                }
            }
        }
    }
    // `--frames` turns on GPU pass timing; the renderer R creates must keep it.
    let mut player = authored_player();
    player.options.frames = Some(40);
    let mut probe = Probe {
        player,
        reloaded: None,
    };
    let mut builder = EventLoop::builder();
    winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
    winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
    builder.build()?.run_app(&mut probe)?;
    if let Some(error) = probe.player.error {
        return Err(error);
    }
    let before = probe.reloaded.context("R was never pressed")?;
    ensure!(probe.player.frames == 40, "window closed early");
    let after = probe.player.view.as_ref().unwrap().gpu_frame_ms.len();
    ensure!(
        after > before,
        "GPU pass timing stopped at R: {before} samples before, {after} after"
    );
    Ok(())
}
