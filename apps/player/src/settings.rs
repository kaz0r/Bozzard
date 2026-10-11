//! Player settings: read before the window opens, then applied to the window, surface and
//! renderer whenever the game applies new values. Volumes reach the mix through the scene's
//! audio frame, so the player has nothing to do for them.
use super::*;
use crate::view::configure_surface_checked;
use bozzard_render::{DisplaySettings, Lighting};
use bozzard_scene::player_settings::{PlayerSettings, Quality, SettingsStore, WindowMode};
use winit::{
    monitor::MonitorHandle,
    window::{Fullscreen, WindowAttributes},
};

/// Read the user's settings into the runtime's store. Verification runs keep defaults unless
/// `--settings FILE` names a file. A rejected file is reported and the game starts anyway.
pub(crate) fn load(demo: &mut SceneRuntime, options: &Options) {
    let world = &mut demo.app.world;
    if let Some(path) = &options.settings {
        world.insert_resource(SettingsStore::new(Some(path.clone())));
    } else if options.frames.is_some() || options.verify_first_trail || options.verify_flap_woods {
        return;
    }
    let Some(Err(error)) = world
        .resource_mut::<SettingsStore>()
        .map(SettingsStore::load)
    else {
        return;
    };
    let message = format!("{error:#}");
    eprintln!("Settings: {message}");
    bozzard_diagnostics::log(
        world,
        bozzard_diagnostics::Level::Warning,
        "Settings",
        &message,
        Default::default(),
    );
}

/// The applied settings and their revision, or defaults for runtimes without a store.
pub(crate) fn applied(demo: &SceneRuntime) -> (PlayerSettings, u64) {
    demo.app
        .world
        .resource::<SettingsStore>()
        .map_or((PlayerSettings::default(), 0), |store| {
            (*store.applied(), store.revision())
        })
}

/// VSync on is FIFO, which every surface supports. Off prefers Immediate, then Mailbox,
/// and falls back to FIFO.
pub(crate) fn present_mode(vsync: bool, supported: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    use wgpu::PresentMode as Mode;
    if vsync {
        return Mode::Fifo;
    }
    [Mode::Immediate, Mode::Mailbox]
        .into_iter()
        .find(|mode| supported.contains(mode))
        .unwrap_or(Mode::Fifo)
}

pub(crate) fn surface_present_mode(
    surface: &wgpu::Surface<'_>,
    gpu: &Gpu,
    vsync: bool,
) -> wgpu::PresentMode {
    let supported = surface.get_capabilities(&gpu.adapter).present_modes;
    let mode = present_mode(vsync, &supported);
    if !vsync && mode == wgpu::PresentMode::Fifo {
        let message = "VSync off is unsupported by this display surface; keeping VSync";
        eprintln!("Settings: {message}");
        bozzard_diagnostics::crash::record("Warning", "Settings", message);
    }
    mode
}

/// Width × height, refresh rate (mHz) and bit depth of one video mode.
pub(crate) type VideoMode = (u32, u32, u32, u16);

/// The largest mode, then the fastest, then the deepest.
pub(crate) fn best_video_mode(modes: &[VideoMode]) -> Option<usize> {
    (0..modes.len()).max_by_key(|&i| {
        let (width, height, refresh, depth) = modes[i];
        (u64::from(width) * u64::from(height), refresh, depth)
    })
}

/// Borderless covers `monitor`. Exclusive fullscreen uses its best video mode, or borderless
/// when the platform lists none.
pub(crate) fn fullscreen(mode: WindowMode, monitor: Option<MonitorHandle>) -> Option<Fullscreen> {
    match mode {
        WindowMode::Windowed => None,
        WindowMode::Borderless => Some(Fullscreen::Borderless(monitor)),
        WindowMode::Fullscreen => {
            let modes: Vec<_> = monitor.iter().flat_map(|m| m.video_modes()).collect();
            let keys: Vec<VideoMode> = modes
                .iter()
                .map(|m| {
                    let size = m.size();
                    (
                        size.width,
                        size.height,
                        m.refresh_rate_millihertz(),
                        m.bit_depth(),
                    )
                })
                .collect();
            Some(match best_video_mode(&keys) {
                Some(index) => Fullscreen::Exclusive(modes[index].clone()),
                None => Fullscreen::Borderless(monitor),
            })
        }
    }
}

/// Size and fullscreen state for a new window.
pub(crate) fn window_attributes(
    attributes: WindowAttributes,
    settings: &PlayerSettings,
    monitor: Option<MonitorHandle>,
) -> WindowAttributes {
    let [width, height] = settings.window_size;
    attributes
        .with_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
        .with_fullscreen(fullscreen(settings.window_mode, monitor))
}

/// What a newly applied settings revision must reconfigure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Changes {
    pub fullscreen: bool,
    /// Logical size to request; a window returning from fullscreen is resized too.
    pub resize: Option<[u32; 2]>,
    pub present_mode: bool,
    /// Applied per frame; reported for completeness.
    pub quality: bool,
}
pub(crate) fn changes(previous: &PlayerSettings, next: &PlayerSettings) -> Changes {
    Changes {
        fullscreen: previous.window_mode != next.window_mode,
        resize: (next.window_mode == WindowMode::Windowed
            && (previous.window_mode != WindowMode::Windowed
                || previous.window_size != next.window_size))
            .then_some(next.window_size),
        present_mode: previous.vsync != next.vsync,
        quality: previous.quality != next.quality,
    }
}

/// Cap authored rendering cost with existing renderer switches. High keeps the scene's look;
/// presets only disable or lower, never enable.
pub(crate) fn apply_quality(
    quality: Quality,
    display: &mut DisplaySettings,
    lighting: &mut Lighting,
) {
    if quality == Quality::High {
        return;
    }
    let shadow_limit = if quality == Quality::Low { 512 } else { 1024 };
    lighting.shadow_resolution = lighting.shadow_resolution.min(shadow_limit);
    display.volumetric_fog.enabled = false;
    display.reflections.enabled = false;
    if quality == Quality::Low {
        display.ambient_occlusion.enabled = false;
        display.bloom.enabled = false;
        display.depth_of_field.enabled = false;
        display.motion_blur.enabled = false;
    }
}

pub(crate) fn apply_quality_to_frame(
    quality: Quality,
    frame: &mut bozzard_render_assets::RenderFrame,
) {
    if quality == Quality::High {
        return;
    }
    let (mut display, mut lighting) = (frame.display, frame.lighting);
    apply_quality(quality, &mut display, &mut lighting);
    frame.set_display(display);
    frame.set_lighting(lighting);
}

impl View {
    /// Reconfigure for newly applied settings. Quality is read every frame.
    pub(crate) fn apply_settings(&mut self, next: PlayerSettings) -> Result<()> {
        let changes = changes(&self.settings, &next);
        self.settings = next;
        if changes.fullscreen {
            self.window
                .set_fullscreen(fullscreen(next.window_mode, self.window.current_monitor()));
        }
        if let Some([width, height]) = changes.resize
            && let Some(size) = self
                .window
                .request_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
        {
            self.resize(size.width, size.height)?;
        }
        if changes.present_mode {
            self.config.present_mode = surface_present_mode(&self.surface, &self.gpu, next.vsync);
            configure_surface_checked(&self.surface, &self.gpu, &self.config)?;
        }
        Ok(())
    }
}

impl Player {
    /// Apply a settings revision the game published since the last check.
    pub(crate) fn sync_settings(&mut self) -> Result<()> {
        let (settings, revision) = applied(&self.demo);
        let Some(view) = &mut self.view else {
            return Ok(());
        };
        if view.settings_revision == revision {
            return Ok(());
        }
        view.settings_revision = revision;
        view.apply_settings(settings)
            .context("applying player settings")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bozzard_scene::player_settings::{Request, VolumeChannel};

    #[test]
    fn vsync_maps_to_supported_present_modes_with_fifo_fallback() {
        use wgpu::PresentMode as Mode;
        let all = [Mode::Fifo, Mode::Mailbox, Mode::Immediate];
        assert_eq!(present_mode(true, &all), Mode::Fifo);
        assert_eq!(present_mode(false, &all), Mode::Immediate);
        assert_eq!(
            present_mode(false, &[Mode::Fifo, Mode::Mailbox]),
            Mode::Mailbox
        );
        assert_eq!(present_mode(false, &[Mode::Fifo]), Mode::Fifo);
        assert_eq!(present_mode(false, &[]), Mode::Fifo);
    }

    #[test]
    fn exclusive_fullscreen_prefers_the_largest_then_fastest_mode() {
        assert_eq!(best_video_mode(&[]), None);
        let modes = [
            (1920, 1080, 60_000, 32),
            (2560, 1440, 60_000, 32),
            (2560, 1440, 144_000, 24),
            (2560, 1440, 144_000, 32),
            (1280, 720, 240_000, 32),
        ];
        assert_eq!(best_video_mode(&modes), Some(3));
    }

    #[test]
    fn changes_reconfigure_only_what_differs() {
        let defaults = PlayerSettings::default();
        assert_eq!(changes(&defaults, &defaults), Changes::default());
        let fullscreen = PlayerSettings {
            window_mode: WindowMode::Fullscreen,
            window_size: [1920, 1080],
            ..defaults
        };
        assert_eq!(
            changes(&defaults, &fullscreen),
            Changes {
                fullscreen: true,
                ..Default::default()
            },
            "fullscreen ignores the windowed size"
        );
        assert_eq!(
            changes(&fullscreen, &defaults),
            Changes {
                fullscreen: true,
                resize: Some([1024, 640]),
                ..Default::default()
            },
            "returning to a window restores its size"
        );
        let edited = PlayerSettings {
            window_size: [1600, 900],
            vsync: false,
            quality: Quality::Low,
            music_volume: 0.5,
            ..defaults
        };
        assert_eq!(
            changes(&defaults, &edited),
            Changes {
                fullscreen: false,
                resize: Some([1600, 900]),
                present_mode: true,
                quality: true,
            }
        );
    }

    #[test]
    fn quality_presets_lower_existing_renderer_switches_and_high_keeps_the_scene() {
        let mut authored = DisplaySettings::default();
        authored.bloom.enabled = true;
        authored.ambient_occlusion.enabled = true;
        authored.volumetric_fog.enabled = true;
        authored.reflections.enabled = true;
        authored.depth_of_field.enabled = true;
        authored.motion_blur.enabled = true;
        authored.temporal_aa.enabled = true;
        let lighting = Lighting {
            shadow_resolution: 4096,
            ..Default::default()
        };
        let run = |quality| {
            let (mut display, mut light) = (authored, lighting);
            apply_quality(quality, &mut display, &mut light);
            (display, light)
        };
        let (high, high_light) = run(Quality::High);
        assert_eq!(high, authored);
        assert_eq!(high_light.shadow_resolution, 4096);
        let (medium, medium_light) = run(Quality::Medium);
        assert_eq!(medium_light.shadow_resolution, 1024);
        assert!(!medium.volumetric_fog.enabled && !medium.reflections.enabled);
        assert!(medium.bloom.enabled && medium.ambient_occlusion.enabled);
        let (low, low_light) = run(Quality::Low);
        assert_eq!(low_light.shadow_resolution, 512);
        assert!(
            !low.bloom.enabled
                && !low.ambient_occlusion.enabled
                && !low.depth_of_field.enabled
                && !low.motion_blur.enabled
        );
        assert!(low.temporal_aa.enabled, "anti-aliasing is part of the look");
        let mut small = Lighting {
            shadow_resolution: 256,
            ..Default::default()
        };
        apply_quality(Quality::Medium, &mut DisplaySettings::default(), &mut small);
        assert_eq!(small.shadow_resolution, 256, "presets never raise cost");
    }

    /// Without a window: a script applies settings, then the player's per-revision sync sees
    /// one new revision whose changes reconfigure the window, surface and quality, while the
    /// audio frame already carries the new volumes.
    #[test]
    fn applied_settings_reach_the_host_and_the_mix_headlessly() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/demo/scenes/script-lab.json");
        let document = load_document(Some(&path)).unwrap();
        let mut demo = SceneRuntime::new_with_prefabs(&document, Some(&path)).unwrap();
        demo.app.world.insert_resource(SettingsStore::new(None));
        let (before, revision) = applied(&demo);
        assert_eq!((before, revision), (PlayerSettings::default(), 0));
        let frame = demo
            .instance()
            .audio_frame(&demo.app.world, Layer::ThreeD)
            .unwrap();
        let (master, buses) = (frame.master, frame.buses);
        for request in [
            Request::WindowMode(WindowMode::Borderless),
            Request::Vsync(false),
            Request::Quality(Quality::Medium),
            Request::Volume(VolumeChannel::Master, 0.5),
            Request::Volume(VolumeChannel::Music, 0.5),
            Request::Apply,
        ] {
            bozzard_scene::player_settings::request(&mut demo.app.world, request).unwrap();
        }
        let (after, next) = applied(&demo);
        assert_eq!(next, revision + 1);
        assert_eq!(
            changes(&before, &after),
            Changes {
                fullscreen: true,
                resize: None,
                present_mode: true,
                quality: true,
            }
        );
        let frame = demo
            .instance()
            .audio_frame(&demo.app.world, Layer::ThreeD)
            .unwrap();
        assert_eq!(frame.master, master * 0.5);
        assert_eq!(frame.buses[1], buses[1] * 0.5);
        assert_eq!(frame.buses[0], buses[0]);
    }

    #[test]
    fn startup_load_rejects_newer_files_but_keeps_starting_with_defaults() {
        let directory =
            std::env::temp_dir().join(format!("bozzard-player-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("settings.json");
        let document = bozzard_runtime::scene_document().unwrap();
        let mut demo = SceneRuntime::new(&document).unwrap();
        let options = Options {
            settings: Some(file.clone()),
            ..Default::default()
        };
        std::fs::write(&file, r#"{"version":1,"vsync":false,"quality":"low"}"#).unwrap();
        load(&mut demo, &options);
        let (settings, revision) = applied(&demo);
        assert!(!settings.vsync && settings.quality == Quality::Low && revision == 1);

        std::fs::write(&file, r#"{"version":99}"#).unwrap();
        let mut demo = SceneRuntime::new(&document).unwrap();
        load(&mut demo, &options);
        assert_eq!(applied(&demo), (PlayerSettings::default(), 0));
        let console = &demo
            .app
            .world
            .resource::<bozzard_diagnostics::Diagnostics>()
            .unwrap()
            .console;
        assert!(
            console
                .events
                .iter()
                .any(|event| event.source == "Settings" && event.message.contains("version 99")),
            "the rejection is logged"
        );

        let automated = Options {
            frames: Some(3),
            ..Default::default()
        };
        let mut demo = SceneRuntime::new_with_prefabs(&document, None).unwrap();
        let before = applied(&demo);
        load(&mut demo, &automated);
        assert_eq!(applied(&demo), before, "verification runs keep defaults");
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
