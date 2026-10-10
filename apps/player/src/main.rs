mod accessibility;
use anyhow::{Context, Result, bail, ensure};
use std::path::Path;
mod assets;
mod cli;
#[cfg(test)]
mod controls_tests;
mod flap_woods;
mod game_flow;
mod gameplay_input;
mod handler;
mod player;
mod presentation;
mod project;
mod smoke;
mod view;
use bozzard_render::{Backend, Gpu, SceneRenderer, instance, wgpu};
use bozzard_runtime::{SceneRuntime, load_document, save_document_from};
use bozzard_scene::{Layer, Scene, Transform};
use cli::*;
use player::*;
use presentation::extract;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{DeviceEvent, DeviceId, ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, KeyCode, NamedKey, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

struct Options {
    join_lobby: Option<u64>,
    content_catalog: Option<String>,
    content_address: Option<String>,
    content_cache: Option<PathBuf>,
    content_handle: Option<bozzard_project::content::ResolvedContent>,
    gamepack: Option<std::sync::Arc<bozzard_project::GamePack>>,
    project: Option<PathBuf>,
    game_name: Option<String>,
    export_project: Option<PathBuf>,
    export_dir: Option<PathBuf>,
    verify_first_trail: bool,
    verify_flap_woods: bool,
    backend: Backend,
    software: bool,
    hardware: bool,
    smoke: bool,
    benchmark_frames: Option<u32>,
    frames: Option<u32>,
    inject_device_recreation: bool,
    output: PathBuf,
    scene: Option<PathBuf>,
    write_scene: Option<PathBuf>,
    save_path: PathBuf,
    layer: Layer,
    gpu_memory_mib: usize,
    occlusion_enabled: bool,
    threaded_simulation: bool,
    render_interpolation: bool,
}

struct View {
    accessibility: accessibility::Accessibility,
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    gpu: Gpu,
    config: wgpu::SurfaceConfiguration,
    renderer: SceneRenderer,
    render_cache: bozzard_render_assets::RenderSceneCache,
    compute: bozzard_render_assets::ComputeBridge,
    drawable: bool,
    occluded: bool,
    /// Reuse the rendered layout's decision instead of laying out UI for every mouse event.
    ui_wants_pointer: bool,
    surface_status: &'static str,
    surface_recovery: SurfaceRecovery,
    gpu_frame_ms: VecDeque<f64>,
    software: bool,
    hardware: bool,
    renderer_settings: view::RendererSettings,
    device_recoveries: u8,
}

/// Surface loss only invalidates presentation. A device callback signals the separate GPU path.
#[derive(Default)]
struct SurfaceRecovery {
    consecutive_losses: u8,
}

/// Everything the pointer-capture decision depends on, so it can be checked without a window.
#[derive(Clone, Copy)]
struct CursorCaptureState {
    paused: bool,
    focused: bool,
    layer: Layer,
    /// The scene uses gameplay input at all (a player controller or any enabled graph).
    gameplay: bool,
    /// The simulation is actually running: Game Flow menus, pause and game over are not.
    running: bool,
    /// Visible, enabled UI controls need the pointer even while gameplay is running.
    ui_wants_pointer: bool,
    /// Lock/Unlock Cursor request; None captures gameplay only when no UI needs the pointer.
    requested: Option<bool>,
}

struct Player {
    options: Options,
    view: Option<View>,
    demo: SceneRuntime,
    assets: assets::Assets,
    audio: bozzard_audio::NativeAudio,
    paused: bool,
    menu_input: game_flow::MenuInput,
    gameplay_controls: gameplay_input::GameplayControls,
    look: gameplay_input::Look,
    last_frame: Instant,
    last_present: Instant,
    frames: u32,
    cpu_frame_ms: VecDeque<f64>,
    presentation_interval_ms: VecDeque<f64>,
    fault_injected: bool,
    error: Option<anyhow::Error>,
    command_error: Option<String>,
}

/// The runtime and assets for `document`, set up from the command line. Startup and the
/// R/F6 reload both use this, so a reload keeps the same session: engine log lines reach
/// the terminal, a game pack's assets stay without hot reload, the scene's multiplayer
/// starts (joining `--join-lobby` if given) and `--single-threaded` still applies.
fn start_session(document: &Scene, options: &Options) -> Result<(SceneRuntime, assets::Assets)> {
    let mut demo = SceneRuntime::new_with_prefabs(document, options.scene.as_deref())?;
    if let Some(diagnostics) = demo
        .app
        .world
        .resource_mut::<bozzard_diagnostics::Diagnostics>()
    {
        diagnostics.echo = Some(bozzard_diagnostics::terminal);
    }
    ensure!(
        demo.instance().has_view(options.layer),
        "scene has no requested view; use --view 2d or --view 3d"
    );
    let mut assets = assets::Assets::load(demo.instance().document(), options.scene.as_deref())?;
    if options.gamepack.is_some() {
        assets.disable_hot_reload();
    }
    bozzard_project::streaming::install(
        &mut demo.app.world,
        options.scene.as_deref().unwrap_or(Path::new("scene.json")),
        assets.store(),
    )?;
    demo.enable_multiplayer(options.join_lobby)?;
    demo.set_threaded_simulation(options.threaded_simulation)?;
    Ok((demo, assets))
}

fn main() -> Result<()> {
    let _steam_shutdown = bozzard_runtime::steam_runtime::ShutdownGuard;
    if std::env::args().nth(1).as_deref() == Some("--runtime-info") {
        println!("{}", bozzard_project::runtime::description());
        return Ok(());
    }
    let Some(options) = options()? else {
        return Ok(());
    };
    if let Some(manifest) = &options.export_project {
        let (project, source) = bozzard_project::Project::load(manifest)?;
        project.require_runtime_modules(&[])?;
        let scene = load_document(Some(&source))?;
        let prepared = bozzard_project::prepare_export(
            &project,
            &scene,
            &source,
            &std::env::current_exe()?,
            options.export_dir.as_ref().unwrap(),
            &Default::default(),
        )?;
        let report = prepared.report();
        let destination = prepared.commit()?;
        println!(
            "export_ok path={} cooked={} reused={} copied={} cooked_bytes={}",
            destination.display(),
            report.built,
            report.reused,
            report.copied,
            report.cooked_bytes
        );
        return Ok(());
    }
    if options.smoke {
        return smoke::run(&options);
    }
    let document = load_document(options.scene.as_deref())?;
    if let Some(path) = &options.write_scene {
        if let Some(pack) = &options.gamepack {
            let content = path.with_extension("game-data");
            pack.copy_to(&content)?;
            let (_, source) =
                bozzard_project::Project::load(&content.join(bozzard_project::MANIFEST))?;
            save_document_from(&document, path, Some(&source))?;
        } else {
            save_document_from(&document, path, options.scene.as_deref())?;
        }
        println!("scene_saved path={}", path.display());
        return Ok(());
    }
    #[cfg(feature = "steam")]
    bozzard_runtime::steam_runtime::initialize_player(&document)?;
    let (demo, assets) = start_session(&document, &options)?;
    let mut player = Player::new(options, demo, assets);
    if player.options.verify_flap_woods {
        return flap_woods::verify(&mut player);
    }
    if player.options.verify_first_trail && player.options.frames.is_none() {
        for tick in 0..340 {
            project::route_tick(&mut player, tick)?;
        }
    } else {
        EventLoop::new()?.run_app(&mut player)?;
    }
    if player.options.verify_first_trail {
        project::verify_route(&mut player)?;
    }
    if let Some(error) = player.error {
        return Err(error);
    }
    if let Some(limit) = player.options.frames {
        ensure!(
            player.frames >= limit,
            "window closed before the requested frames were presented"
        );
    }
    Ok(())
}
