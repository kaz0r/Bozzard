//! The window, surface and GPU device, their recovery, and drawing frames.
use super::*;

impl SurfaceRecovery {
    pub(crate) fn lost(&mut self) -> Result<()> {
        self.consecutive_losses = self.consecutive_losses.saturating_add(1);
        ensure!(
            self.consecutive_losses <= 3,
            "surface recovery failed after 3 reconfigurations; check window/display backend"
        );
        Ok(())
    }
    pub(crate) fn presented(&mut self) {
        self.consecutive_losses = 0;
    }
}

pub(crate) fn configure_surface_checked(
    surface: &wgpu::Surface<'_>,
    gpu: &Gpu,
    config: &wgpu::SurfaceConfiguration,
) -> Result<()> {
    let errors = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    surface.configure(&gpu.device, config);
    if let Some(error) = pollster::block_on(errors.pop()) {
        bail!("graphics surface configuration failed: {error:#?}");
    }
    Ok(())
}

pub(crate) type RetiredComputeJob = (bozzard_scene::compute::Owner, u64, String, bool);

pub(crate) fn retire_pending_compute_jobs(
    demo: &mut SceneRuntime,
) -> Result<Vec<RetiredComputeJob>> {
    demo.with_instance(|instance, _| {
        let Some(mut compute) = instance.compute_if_initialized() else {
            return Ok(Vec::new());
        };
        let jobs: Vec<_> = compute
            .runtime
            .jobs()
            .filter(|job| !job.state.terminal())
            .map(|job| {
                (
                    job.owner.clone(),
                    job.ticket,
                    job.label.clone(),
                    job.readback,
                )
            })
            .collect();
        let mut retired = Vec::with_capacity(jobs.len());
        for (owner, ticket, label, readback) in jobs {
            compute.runtime.cancel(&owner, ticket).with_context(|| {
                format!(
                    "retiring compute request {} after GPU device loss",
                    ticket.serial()
                )
            })?;
            retired.push((owner, ticket.serial(), label, readback));
        }
        Ok(retired)
    })
}

impl View {
    pub(crate) fn new(
        event_loop: &ActiveEventLoop,
        options: &Options,
        restored_size: Option<PhysicalSize<u32>>,
    ) -> Result<Self> {
        let attributes = Window::default_attributes()
            .with_visible(false)
            .with_title("Bozzard Scene Lab — 1: 2D | 2: 3D | Space: pause | F5: save | R: reload");
        let attributes = if let Some(size) = restored_size {
            attributes.with_inner_size(PhysicalSize::new(size.width.max(1), size.height.max(1)))
        } else {
            attributes.with_inner_size(LogicalSize::new(1024.0, 640.0))
        };
        let window = Arc::new(event_loop.create_window(attributes)?);
        let accessibility = accessibility::Accessibility::new(event_loop, &window);
        window.set_visible(true);
        let instance = instance(options.backend);
        let surface = instance.create_surface(window.clone())?;
        let gpu = pollster::block_on(Gpu::request(&instance, Some(&surface), options.software))?;
        if options.hardware {
            gpu.require_hardware()?;
        }
        gpu.monitor_out_of_memory();
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .context("surface is unsupported by selected adapter")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        let mut renderer = SceneRenderer::new(&gpu, config.format);
        renderer.set_occlusion_enabled(options.occlusion_enabled);
        renderer.set_profiling_enabled(options.frames.is_some());
        let compute = bozzard_render_assets::ComputeBridge::new(&gpu);
        configure_surface_checked(&surface, &gpu, &config)?;
        if options.frames.is_some() {
            window.focus_window();
        }
        Ok(Self {
            accessibility,
            window,
            instance,
            surface,
            gpu,
            config,
            renderer,
            render_cache: Default::default(),
            compute,
            drawable: size.width > 0 && size.height > 0,
            occluded: false,
            // Keep the pointer free until the first visible frame establishes the UI policy.
            ui_wants_pointer: true,
            surface_status: "awaiting first redraw",
            surface_recovery: SurfaceRecovery::default(),
            gpu_frame_ms: VecDeque::new(),
            software: options.software,
            hardware: options.hardware,
            occlusion_enabled: options.occlusion_enabled,
            device_recoveries: 0,
            profile_frames: options.frames.is_some(),
        })
    }

    pub(crate) fn recreate_device(
        &mut self,
        event_loop: &ActiveEventLoop,
        options: &Options,
        demo: &mut SceneRuntime,
        assets: &mut assets::Assets,
    ) -> Result<()> {
        let reason = self.gpu.failure().unwrap_or("device failure").to_owned();
        ensure!(
            !self.gpu.out_of_memory(),
            "GPU out of memory is terminal: {reason}; backend={:?}, size={}x{}",
            self.gpu.adapter.get_info().backend,
            self.config.width,
            self.config.height
        );
        self.device_recoveries = self.device_recoveries.saturating_add(1);
        ensure!(
            self.device_recoveries <= 2,
            "GPU recovery failed after 2 recreations: {reason}; backend={:?}, size={}x{}",
            self.gpu.adapter.get_info().backend,
            self.config.width,
            self.config.height
        );
        let pending = self.compute.executor.statistics();
        let retired = retire_pending_compute_jobs(demo)?;
        for (owner, ticket, label, readback) in retired.iter().take(8) {
            bozzard_diagnostics::log(
                &mut demo.app.world,
                bozzard_diagnostics::Level::Error,
                "Compute",
                &format!(
                    "{label} request {ticket}: cancelled after GPU device loss{}",
                    if *readback {
                        " (readback discarded)"
                    } else {
                        ""
                    }
                ),
                bozzard_diagnostics::Location {
                    object: Some(owner.object.clone()),
                    attachment: Some(owner.attachment),
                    asset: label.split_once("::").map(|(asset, _)| asset.to_owned()),
                    ..Default::default()
                },
            );
        }
        bozzard_diagnostics::log(
            &mut demo.app.world,
            bozzard_diagnostics::Level::Error,
            "Graphics",
            &format!(
                "{reason}; recreating device; cancelled_compute_jobs={} reported_first={} pending_gpu={} readback_bytes={}",
                retired.len(),
                retired.len().min(8),
                self.compute.executor.has_pending(),
                pending.readback_bytes
            ),
            Default::default(),
        );
        self.compute.stop();
        if self.gpu.adapter.get_info().backend == wgpu::Backend::Dx12 {
            // DXGI cannot attach a second swapchain to the same HWND while the
            // original window and its presentation resources are still alive.
            let mut replacement = Self::new(event_loop, options, Some(self.window.inner_size()))
                .context("recreating DX12 presentation window")?;
            replacement.device_recoveries = self.device_recoveries;
            assets
                .upload(&replacement.gpu, &mut replacement.renderer)
                .context("restoring graphics assets")?;
            demo.with_instance(|instance, _| replacement.compute.prepare(instance));
            replacement.gpu_frame_ms = std::mem::take(&mut self.gpu_frame_ms);
            replacement.surface_status = "GPU device recreated";
            *self = replacement;
            return Ok(());
        }
        // Bind a fresh presentation surface to the replacement device.
        self.surface = self
            .instance
            .create_surface(self.window.clone())
            .context("recreating window surface after device loss")?;
        let gpu = pollster::block_on(Gpu::request(
            &self.instance,
            Some(&self.surface),
            self.software,
        ))
        .context("recreating lost GPU device")?;
        if self.hardware {
            gpu.require_hardware()?;
        }
        gpu.monitor_out_of_memory();
        let mut config = self
            .surface
            .get_default_config(
                &gpu.adapter,
                self.config.width.max(1),
                self.config.height.max(1),
            )
            .context("recreated GPU cannot present to this surface")?;
        config.present_mode = wgpu::PresentMode::Fifo;
        let mut renderer = SceneRenderer::new(&gpu, config.format);
        renderer.set_occlusion_enabled(self.occlusion_enabled);
        renderer.set_profiling_enabled(self.profile_frames);
        assets
            .upload(&gpu, &mut renderer)
            .context("restoring graphics assets")?;
        let compute = bozzard_render_assets::ComputeBridge::new(&gpu);
        demo.with_instance(|instance, _| compute.prepare(instance));
        configure_surface_checked(&self.surface, &gpu, &config)?;
        self.gpu = gpu;
        self.config = config;
        self.renderer = renderer;
        self.compute = compute;
        self.surface_recovery.presented();
        self.surface_status = "GPU device recreated";
        Ok(())
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.drawable = width > 0 && height > 0;
        if self.drawable {
            self.config.width = width;
            self.config.height = height;
            configure_surface_checked(&self.surface, &self.gpu, &self.config)?;
        }
        Ok(())
    }

    pub(crate) fn draw(
        &mut self,
        demo: &mut SceneRuntime,
        assets: &mut assets::Assets,
        layer: Layer,
        elapsed: Option<Duration>,
    ) -> Result<bool> {
        // Even a skipped/failed presentation must consume this frame's simulation time.
        let mut elapsed = elapsed;
        let result = self.draw_prepared(demo, assets, layer, &mut elapsed);
        if let Some(elapsed) = elapsed {
            demo.advance_with_frame(elapsed, || ())?;
        }
        result
    }

    pub(crate) fn draw_prepared(
        &mut self,
        demo: &mut SceneRuntime,
        assets: &mut assets::Assets,
        layer: Layer,
        elapsed: &mut Option<Duration>,
    ) -> Result<bool> {
        if self.occluded {
            self.surface_status = "window occluded";
            return Ok(false);
        }
        if !self.drawable {
            self.surface_status = "window has zero size";
            return Ok(false);
        }
        if let Some(reason) = self.gpu.failure() {
            bail!("GPU device lost: {reason}");
        }
        self.renderer
            .set_hud_scale(self.window.scale_factor() as f32);
        assets.poll()?;
        let mut scene = presentation::extract_frame(
            demo,
            assets.store(),
            &self.render_cache,
            layer,
            self.config.width as f32 / self.config.height as f32,
        )?;
        let scale = self.window.scale_factor() as f32;
        let ui = demo.instance().ui_frame(
            &demo.app.world,
            layer,
            [
                self.config.width as f32 / scale,
                self.config.height as f32 / scale,
            ],
        )?;
        self.ui_wants_pointer = ui.wants_pointer();
        self.accessibility
            .update(&ui, &demo.instance().document().name, scale);
        scene.append_items(bozzard_render_assets::widget_items(&ui, assets.store())?);
        if !assets.prepare_frame(&self.gpu, &mut self.renderer, &scene)? {
            self.surface_status = "streaming graphics resources";
            return Ok(false);
        }
        if !assets.current() {
            scene.clear_gi();
        }
        // Streaming may skip a frame while a new mesh uploads. Do that before
        // acquiring a swapchain image: dropping an acquired, unpresented image
        // can exhaust the surface's images and stall subsequent acquisition.
        // Surface acquisition may wait for VSync. Let simulation run during that
        // wait as well as draw submission, rather than starting it afterwards.
        let mut submit = || -> Result<bool> {
            let (frame, reconfigure) = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
                wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
                wgpu::CurrentSurfaceTexture::Outdated => {
                    self.surface_status = "surface outdated";
                    configure_surface_checked(&self.surface, &self.gpu, &self.config)?;
                    return Ok(false);
                }
                wgpu::CurrentSurfaceTexture::Timeout => {
                    self.surface_status = "surface acquisition timed out";
                    return Ok(false);
                }
                wgpu::CurrentSurfaceTexture::Occluded => {
                    self.surface_status = "window occluded; an active desktop is required";
                    return Ok(false);
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    self.surface_recovery.lost()?;
                    self.surface_status = "surface lost; reconfiguring";
                    configure_surface_checked(&self.surface, &self.gpu, &self.config)?;
                    return Ok(false);
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    bail!("graphics surface validation failed")
                }
            };
            self.renderer.draw(
                &self.gpu,
                &frame.texture.create_view(&Default::default()),
                [self.config.width, self.config.height],
                &scene,
            )?;
            self.window.pre_present_notify();
            self.gpu.queue.present(frame);
            self.surface_recovery.presented();
            self.surface_status = "presented";
            if reconfigure {
                configure_surface_checked(&self.surface, &self.gpu, &self.config)?;
            }
            Ok(true)
        };
        let presented = if let Some(elapsed) = elapsed.take() {
            demo.advance_with_frame(elapsed, submit)??
        } else {
            submit()?
        };
        if !presented {
            return Ok(false);
        }
        for timing in self.renderer.poll_gpu_profiles(&self.gpu)? {
            if !timing.failed {
                let total = timing
                    .passes
                    .iter()
                    .filter_map(|pass| pass.milliseconds)
                    .sum();
                if self.gpu_frame_ms.len() == 2048 {
                    self.gpu_frame_ms.pop_front();
                }
                self.gpu_frame_ms.push_back(total);
            }
        }
        Ok(true)
    }
}
