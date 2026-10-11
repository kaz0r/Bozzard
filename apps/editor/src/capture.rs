//! `--capture FILE`: drive the editor for a number of frames without opening a window, then
//! paint the last frame offscreen and write it as a binary PPM. Useful for checking layout
//! and styling changes on machines where a window would be in the way.
use super::*;
use eframe::App as _;
use eframe::egui_wgpu::{self, RenderState, ScreenDescriptor, WgpuConfiguration};

pub(crate) struct Options {
    pub path: PathBuf,
    /// Output size in physical pixels.
    pub size: [u32; 2],
    pub frames: u32,
    pub pixels_per_point: f32,
    /// Panes to show besides the default layout, e.g. `debug`, `timeline`, `blueprints`.
    pub show: Vec<String>,
    /// Object to select before the first frame, so the inspector has content.
    pub select: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            size: [1600, 1000],
            frames: 90,
            pixels_per_point: 1.,
            show: Vec::new(),
            select: None,
        }
    }
}

pub(crate) fn parse_size(text: &str) -> Result<[u32; 2]> {
    let (width, height) = text
        .split_once('x')
        .context("--capture-size needs WIDTHxHEIGHT")?;
    Ok([width.parse()?, height.parse()?])
}

pub(crate) fn run(
    options: &Options,
    setup: egui_wgpu::WgpuSetupExisting,
    make: impl FnOnce(&eframe::CreationContext<'_>) -> Result<App>,
) -> Result<()> {
    let instance = setup.instance.clone();
    let config = WgpuConfiguration {
        wgpu_setup: setup.into(),
        ..Default::default()
    };
    let state = pollster::block_on(RenderState::create(
        &config,
        &instance,
        None,
        Default::default(),
    ))?;
    ensure!(
        state.target_format == wgpu::TextureFormat::Rgba8Unorm,
        "offscreen capture expects an RGBA8 target, got {:?}",
        state.target_format
    );
    let ctx = egui::Context::default();
    let mut creation = eframe::CreationContext::_new_kittest(ctx.clone());
    creation.wgpu_render_state = Some(state.clone());
    let mut app = make(&creation)?;
    app.apply_capture_options(options)?;
    let mut frame = eframe::Frame::_new_kittest();
    frame.wgpu_render_state = Some(state.clone());

    let [width, height] = options.size;
    let points = Vec2::new(width as f32, height as f32) / options.pixels_per_point;
    let start = Instant::now();
    let mut count = 0;
    let mut settled = 0;
    let mut output = loop {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, points)),
            time: Some(start.elapsed().as_secs_f64()),
            focused: true,
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(options.pixels_per_point);
        app.raw_input_hook(&ctx, &mut input);
        let mut output = ctx.run_ui(input, |ui| {
            app.logic(ui.ctx(), &mut frame);
            app.ui(ui, &mut frame);
        });
        count += 1;
        // GPU uploads finish a frame or two after loading reports done.
        settled = if app.capture_busy() { 0 } else { settled + 1 };
        if (count >= options.frames && settled >= 10) || start.elapsed() > Duration::from_secs(60) {
            break output;
        }
        // Texture uploads accumulate across frames; only the last frame is painted.
        let mut renderer = state.renderer.write();
        for (id, deltas) in output.textures_delta.set.drain() {
            for delta in deltas {
                renderer.update_texture(&state.device, &state.queue, id, &delta);
            }
        }
        for id in output.textures_delta.free.drain() {
            renderer.free_texture(&id);
        }
        drop(renderer);
        std::thread::sleep(Duration::from_millis(16));
    };
    let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    let screen = ScreenDescriptor {
        size_in_pixels: options.size,
        pixels_per_point: output.pixels_per_point,
    };
    let gpu = Gpu::from_device(
        state.adapter.clone(),
        state.device.clone(),
        state.queue.clone(),
    );
    let image = bozzard_render::capture_offscreen(&gpu, width, height, |view| {
        let mut renderer = state.renderer.write();
        for (id, deltas) in output.textures_delta.set.drain() {
            for delta in deltas {
                renderer.update_texture(&state.device, &state.queue, id, &delta);
            }
        }
        let mut encoder = state.device.create_command_encoder(&Default::default());
        let commands =
            renderer.update_buffers(&state.device, &state.queue, &mut encoder, &jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Editor capture"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            renderer.render(&mut pass, &jobs, &screen);
        }
        state
            .queue
            .submit(commands.into_iter().chain([encoder.finish()]));
        for id in output.textures_delta.free.drain() {
            renderer.free_texture(&id);
        }
        Ok(())
    })?;
    image.write_ppm(&options.path)?;
    println!(
        "editor_capture_ok path={} frames={count} size={width}x{height}",
        options.path.display()
    );
    Ok(())
}

impl App {
    fn apply_capture_options(&mut self, options: &Options) -> Result<()> {
        for pane in &options.show {
            let workspace = &mut self.workspace;
            match pane.as_str() {
                "assets" => workspace.assets_visible = true,
                "blueprints" => workspace.blueprints_visible = true,
                "debug" => workspace.debug_visible = true,
                "kennel" => workspace.kennel_visible = true,
                "settings" => workspace.settings_visible = true,
                "shaders" => workspace.shaders_visible = true,
                "stats" => workspace.stats_visible = true,
                "timeline" => workspace.timeline_visible = true,
                "2d" => workspace.layer_2d = true,
                other => anyhow::bail!("--capture-show does not know the pane {other}"),
            }
        }
        if let Some(id) = &options.select {
            ensure!(
                self.editor.scene().objects.iter().any(|o| &o.id == id),
                "--capture-select: no object {id}"
            );
            self.editor.select_object(Some(id.clone()));
        }
        Ok(())
    }

    /// Whether loading or GPU uploads would still change the next frame.
    fn capture_busy(&self) -> bool {
        self.loading.is_some()
            || self.refresh.is_some()
            || self.residency.progress().is_some()
            || self.residency.preparing().is_some()
            || (!self.workspace.shaders_visible
                && !self.workspace.blueprints_visible
                && !self.residency.has_required(&self.editor.assets))
    }
}
