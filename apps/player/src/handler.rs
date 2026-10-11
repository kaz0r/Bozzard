//! The winit event loop: window, device and redraw events.
use super::*;

impl ApplicationHandler for Player {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.view.is_some() {
            return;
        }
        let (player_settings, revision) = settings::applied(&self.demo);
        match View::new(event_loop, &self.options, &player_settings, None) {
            Ok(mut view) => {
                view.settings_revision = revision;
                let uploaded = (|| {
                    let render = extract(
                        &self.demo,
                        self.assets.store(),
                        self.options.layer,
                        view.config.width as f32 / view.config.height as f32,
                    )?;
                    self.assets.upload_required(
                        &view.gpu,
                        &mut view.renderer,
                        &render,
                        self.options.gpu_memory_mib * 1024 * 1024,
                    )
                })();
                if let Err(error) = uploaded {
                    self.fail(event_loop, error);
                    return;
                }
                self.gameplay_controls
                    .set_scale_factor(view.window.scale_factor());
                self.view = Some(view);
                self.last_frame = Instant::now();
                self.last_present = Instant::now();
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.view = None;
        self.look = gameplay_input::Look::Off;
        self.gameplay_controls
            .set_mouse_look(gameplay_input::Look::Off);
        self.gameplay_controls.reset();
        self.demo.clear_gameplay_input();
    }

    /// Locked look reads raw device motion: the cursor does not move, so deltas never arrive
    /// through CursorMoved.
    fn device_event(&mut self, event_loop: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if event_loop.exiting() || self.view.is_none() {
            return;
        }
        let DeviceEvent::MouseMotion { delta } = event else {
            return;
        };
        if let Some(input) = self.gameplay_controls.motion([delta.0, delta.1]) {
            self.demo.set_gameplay_input(input);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let frame_started = matches!(event, WindowEvent::RedrawRequested).then(Instant::now);
        if event_loop.exiting() {
            return;
        }
        if self.view.as_ref().is_none_or(|v| id != v.window.id()) {
            return;
        }
        if let Some(view) = &mut self.view {
            view.accessibility
                .adapter
                .process_event(&view.window, &event);
        }
        if matches!(event, WindowEvent::RedrawRequested) {
            if let Err(error) = self
                .demo
                .set_render_interpolation(self.options.render_interpolation && !self.paused)
            {
                self.fail(event_loop, error);
                return;
            }
            if self.options.inject_device_recreation && !self.fault_injected && self.frames >= 1 {
                self.fault_injected = true;
                let before = (
                    self.demo.app.ticks(),
                    self.demo.app.world.len(),
                    self.demo.instance().document().name.clone(),
                );
                if let Err(error) = self.view.as_mut().unwrap().recreate_device(
                    event_loop,
                    &self.options,
                    &mut self.demo,
                    &mut self.assets,
                ) {
                    self.fail(
                        event_loop,
                        error.context("injected device recreation failed"),
                    );
                } else {
                    let after = (
                        self.demo.app.ticks(),
                        self.demo.app.world.len(),
                        self.demo.instance().document().name.clone(),
                    );
                    if before != after {
                        self.fail(event_loop, anyhow::anyhow!(
                            "device recreation changed CPU scene state: before={before:?} after={after:?}"));
                        return;
                    }
                    println!("device_recreation_ok scene_tick={}", self.demo.app.ticks());
                }
                return;
            }
            if self
                .view
                .as_ref()
                .is_some_and(|view| view.gpu.failure().is_some())
            {
                if self
                    .view
                    .as_ref()
                    .is_some_and(|view| view.gpu.out_of_memory())
                {
                    let view = self.view.as_ref().unwrap();
                    self.fail(
                        event_loop,
                        anyhow::anyhow!(
                            "GPU out of memory: {}; backend={:?}, size={}x{}; scene={}",
                            view.gpu.failure().unwrap_or("unknown"),
                            view.gpu.adapter.get_info().backend,
                            view.config.width,
                            view.config.height,
                            self.demo.instance().document().name
                        ),
                    );
                    return;
                }
                let result = self.view.as_mut().unwrap().recreate_device(
                    event_loop,
                    &self.options,
                    &mut self.demo,
                    &mut self.assets,
                );
                if let Err(error) = result {
                    if self.view.as_ref().unwrap().device_recoveries >= 2 {
                        self.fail(event_loop, error.context("GPU recovery exhausted"));
                    } else {
                        eprintln!("gpu_recovery_retry: {error:#}");
                    }
                }
                return;
            }
            if let Some(view) = &mut self.view {
                let refreshed = self.demo.with_instance(|instance, _| {
                    view.compute.prepare(instance);
                    view.compute
                        .refresh(&view.gpu, instance, self.assets.store())
                });
                if let Err(error) = refreshed {
                    self.fail(event_loop, error);
                    return;
                }
                if let Err(error) = view.compute.poll(&view.gpu) {
                    if view.gpu.failure().is_some() {
                        return;
                    }
                    self.fail(event_loop, error);
                    return;
                }
            }
            let view = self.view.as_ref().unwrap();
            let scale = view.window.scale_factor() as f32;
            let size = [
                view.config.width.max(1) as f32 / scale,
                view.config.height.max(1) as f32 / scale,
            ];
            let requests = view.accessibility.drain();
            for request in requests {
                let result = (|| -> Result<()> {
                    let frame = self.demo.instance().ui_frame(
                        &self.demo.app.world,
                        self.options.layer,
                        size,
                    )?;
                    if let Some(element) = frame
                        .elements
                        .iter()
                        .find(|e| e.id == request.target_node.0)
                        && let Some(input) =
                            bozzard_render_assets::accessibility::action(element, &request)
                    {
                        self.ui_input(input)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    self.fail(event_loop, error);
                    return;
                }
            }
        }
        let ui_consumed = match self.game_pointer_event(&event) {
            Ok(consumed) => consumed,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        if ui_consumed {
            self.gameplay_controls.reset();
            self.demo.clear_gameplay_input();
        }
        self.gameplay_controls.set_scene_keyboard(
            self.demo.gameplay().is_none() && self.demo.instance().has_gameplay_logic(),
        );
        if !ui_consumed && self.demo.accepts_gameplay_input() {
            if let Some(input) = self.gameplay_controls.event(&event)
                && (self.options.layer == Layer::ThreeD
                    || self.demo.instance().has_gameplay_logic())
            {
                self.demo.set_gameplay_input(input);
            } else {
                self.demo.clear_gameplay_input();
            }
        }
        if let WindowEvent::KeyboardInput {
            event,
            is_synthetic: false,
            ..
        } = &event
            && event.state == ElementState::Pressed
            && let Some(text) = &event.text
        {
            self.demo.multiplayer_text(text);
        }
        if let WindowEvent::KeyboardInput {
            event,
            is_synthetic,
            ..
        } = &event
            && let Err(error) = self.dispatch_keyboard(
                event.physical_key,
                &event.logical_key,
                event.state,
                event.repeat,
                *is_synthetic,
            )
        {
            eprintln!("scene command failed: {error:#}");
        }
        if self
            .demo
            .game_session()
            .is_some_and(|s| s.phase == bozzard_scene::GamePhase::Quit)
        {
            event_loop.exit();
            return;
        }
        let now = Instant::now();
        let simulation_elapsed = self.simulation_elapsed(now);
        if matches!(event, WindowEvent::RedrawRequested) {
            self.demo
                .with_instance(|instance, _| instance.set_gpu_particles(true));
            if self.options.verify_first_trail
                && let Err(error) = project::route_tick(self, self.demo.app.ticks())
            {
                self.fail(event_loop, error);
                return;
            }
            if let Err(error) = self.demo.check_simulation() {
                self.fail(event_loop, error);
                return;
            }
            if self.assets.adopt_scene_assets(&self.demo.app.world)
                && let Some(view) = &mut self.view
            {
                let result = self.demo.with_instance(|instance, _| {
                    view.compute.prepare(instance);
                    view.compute
                        .refresh(&view.gpu, instance, self.assets.store())
                });
                if let Err(error) = result {
                    self.fail(event_loop, error);
                    return;
                }
            }
            if let Some(view) = &mut self.view {
                if let Err(error) = view.compute.submit(&view.gpu, self.demo.instance()) {
                    if view.gpu.failure().is_some() {
                        return;
                    }
                    self.fail(event_loop, error);
                    return;
                }
                view.compute.sync_renderer(&mut view.renderer);
            }
            let audio_result = self
                .demo
                .instance()
                .audio_frame(&self.demo.app.world, self.options.layer)
                .and_then(|mut frame| {
                    if self.paused {
                        for source in &mut frame.sources {
                            if source.transport
                                == bozzard_scene::middleware::audio::Transport::Playing
                            {
                                source.transport =
                                    bozzard_scene::middleware::audio::Transport::Paused;
                            }
                        }
                    }
                    let root = self
                        .options
                        .scene
                        .as_deref()
                        .and_then(Path::parent)
                        .unwrap_or_else(|| Path::new("."));
                    self.audio
                        .sync(&frame, self.demo.instance().document(), root)
                });
            if let Err(error) = audio_result {
                eprintln!("Audio: {error:#}");
                self.command_error = Some(format!("Audio: {error:#}"));
            }
            self.last_frame = now;
        }
        let title = self.window_title();
        let Some(view) = self.view.as_mut() else {
            return;
        };
        view.window.set_title(&title);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Occluded(occluded) => view.occluded = occluded,
            WindowEvent::KeyboardInput { event, .. }
                if self.demo.game_session().is_none()
                    && !self.demo.steam_overlay_active()
                    && !self.demo.multiplayer_active()
                    && !self.menu_input.consumed(KeyCode::Escape)
                    && event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Escape) =>
            {
                event_loop.exit()
            }
            WindowEvent::Resized(size) => {
                if let Err(error) = view.resize(size.width, size.height)
                    && view.gpu.failure().is_none()
                {
                    self.fail(event_loop, error.context("resizing graphics surface"));
                }
            }
            WindowEvent::RedrawRequested => {
                match view.draw(
                    &mut self.demo,
                    &mut self.assets,
                    self.options.layer,
                    simulation_elapsed,
                ) {
                    Ok(true) => {
                        let presented = Instant::now();
                        if self.frames > 0 && self.options.frames.is_some() {
                            if self.presentation_interval_ms.len() == 2048 {
                                self.presentation_interval_ms.pop_front();
                            }
                            self.presentation_interval_ms.push_back(
                                presented.duration_since(self.last_present).as_secs_f64() * 1000.,
                            );
                        }
                        bozzard_render_assets::publish_frame(
                            &mut self.demo.app.world,
                            view.renderer.frame_stats(),
                        );
                        if let Some(started) = frame_started {
                            if self.cpu_frame_ms.len() == 2048 {
                                self.cpu_frame_ms.pop_front();
                            }
                            self.cpu_frame_ms
                                .push_back(started.elapsed().as_secs_f64() * 1000.);
                        }
                        self.frames = self.frames.saturating_add(1);
                        self.last_present = presented;
                        if self
                            .options
                            .frames
                            .is_some_and(|limit| self.frames >= limit)
                        {
                            if let Err(error) = view.gpu.wait() {
                                self.fail(event_loop, error);
                                return;
                            }
                            for timing in view
                                .renderer
                                .poll_gpu_profiles(&view.gpu)
                                .unwrap_or_default()
                            {
                                if !timing.failed {
                                    let total = timing
                                        .passes
                                        .iter()
                                        .filter_map(|pass| pass.milliseconds)
                                        .sum();
                                    if view.gpu_frame_ms.len() == 2048 {
                                        view.gpu_frame_ms.pop_front();
                                    }
                                    view.gpu_frame_ms.push_back(total);
                                }
                            }
                            print_frame_percentiles("player_cpu_frame", &self.cpu_frame_ms);
                            print_frame_percentiles(
                                "player_presentation_interval",
                                &self.presentation_interval_ms,
                            );
                            print_frame_percentiles("player_gpu_passes", &view.gpu_frame_ms);
                            if let Some(sim) = self
                                .demo
                                .app
                                .world
                                .resource::<bozzard_diagnostics::SimulationMetrics>()
                            {
                                println!(
                                    "player_simulation threaded={} steps={} cpu_ms={:.3} wait_ms={:.3}",
                                    sim.threaded, sim.steps, sim.cpu_ms, sim.wait_ms
                                );
                            }
                            if let Some(net) = self.demo.multiplayer_telemetry() {
                                println!(
                                    "player_network snapshot_age_ms={:.1} oldest_input_age_ticks={} replay_depth={} command_queue={} publication_age_ms={:.1} worker_ms={:.3}",
                                    net.snapshot_age_ms,
                                    net.oldest_input_age_ticks,
                                    net.replay_depth,
                                    net.command_queue,
                                    net.publication_age_ms,
                                    net.worker_ms
                                );
                            }
                            println!("window_ok frames={}", self.frames);
                            event_loop.exit();
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        if view.gpu.failure().is_none() {
                            self.fail(event_loop, error);
                        }
                    }
                }
            }
            _ => {}
        }
        // Apply the current rendered UI's policy before another pointer/device event arrives.
        self.sync_mouse_look();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let overlay_was_active = self.demo.steam_overlay_active();
        if let Err(error) = self.demo.pump_multiplayer() {
            self.fail(event_loop, error);
            return;
        }
        if overlay_was_active || self.demo.steam_overlay_active() {
            self.gameplay_controls.reset();
            self.menu_input = Default::default();
            self.sync_mouse_look();
        }
        if self.demo.multiplayer_quit() {
            event_loop.exit();
            return;
        }
        if event_loop.exiting() {
            return;
        }
        if let Err(error) = self.sync_settings() {
            self.fail(event_loop, error);
            return;
        }
        // Some window systems stop redraw events while minimized. Accepted compute requests and
        // async maps still progress; result delivery waits for the next actual simulation tick.
        if let Some(view) = &mut self.view {
            let result = view
                .compute
                .poll(&view.gpu)
                .and_then(|_| view.compute.submit(&view.gpu, self.demo.instance()));
            if let Err(error) = result {
                if view.gpu.failure().is_some() {
                    view.window.request_redraw();
                    return;
                }
                self.fail(event_loop, error);
                return;
            }
            view.compute.sync_renderer(&mut view.renderer);
        }
        if self.options.frames.is_some() && self.last_present.elapsed() > Duration::from_secs(30) {
            self.fail(
                event_loop,
                anyhow::anyhow!(
                    "no successful presentation within 30 seconds: {}",
                    self.view
                        .as_ref()
                        .map(|v| v.surface_status)
                        .unwrap_or("no active window")
                ),
            );
            return;
        }
        if let Some(view) = &self.view {
            view.window.request_redraw();
        }
        // FIFO presentation already paces a drawable window. Waiting another 16 ms
        // here adds idle time after CPU work and can halve the presentation rate.
        // Keep retrying slowly when minimized, occluded, or recovering the surface.
        let presenting = self.view.as_ref().is_some_and(|view| {
            view.drawable
                && !view.occluded
                && view.surface_status == "presented"
                && view.gpu.failure().is_none()
        });
        event_loop.set_control_flow(if presenting {
            winit::event_loop::ControlFlow::Poll
        } else {
            winit::event_loop::ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16))
        });
    }
}
