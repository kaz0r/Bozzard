//! Command-line options.
use super::*;

impl Default for Options {
    fn default() -> Self {
        Self {
            join_lobby: None,
            content_catalog: None,
            content_address: None,
            content_cache: None,
            content_handle: None,
            gamepack: None,
            project: None,
            game_name: None,
            export_project: None,
            export_dir: None,
            verify_first_trail: false,
            verify_flap_woods: false,
            backend: Backend::native(),
            software: false,
            hardware: false,
            smoke: false,
            benchmark_frames: None,
            frames: None,
            inject_device_recreation: false,
            output: "work/gpu-smoke".into(),
            scene: None,
            write_scene: None,
            save_path: "work/saved-scene.json".into(),
            layer: Layer::ThreeD,
            gpu_memory_mib: 512,
            occlusion_enabled: true,
            threaded_simulation: true,
            render_interpolation: true,
            settings: None,
        }
    }
}

pub(crate) fn options() -> Result<Option<Options>> {
    let mut result = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--join-lobby" | "+connect_lobby" => {
                result.join_lobby = Some(
                    args.next()
                        .context("--join-lobby needs a Steam lobby ID")?
                        .parse()?,
                );
            }
            "--content-catalog" => {
                result.content_catalog = Some(
                    args.next()
                        .context("--content-catalog needs a file or HTTPS URL")?,
                )
            }
            "--content" => {
                result.content_address = Some(args.next().context("--content needs an address")?)
            }
            "--content-cache" => {
                result.content_cache = Some(
                    args.next()
                        .context("--content-cache needs a directory")?
                        .into(),
                )
            }
            "--project" => {
                result.project = Some(
                    args.next()
                        .context("--project needs a manifest or .bpack file")?
                        .into(),
                )
            }
            "--export-project" => {
                result.export_project = Some(
                    args.next()
                        .context("--export-project needs a manifest")?
                        .into(),
                )
            }
            "--export-dir" => {
                result.export_dir = Some(
                    args.next()
                        .context("--export-dir needs a new folder")?
                        .into(),
                )
            }
            "--verify-first-trail" => result.verify_first_trail = true,
            "--verify-flap-woods" => result.verify_flap_woods = true,
            "--backend" => {
                result.backend = args.next().context("--backend needs a value")?.parse()?
            }
            "--software" => result.software = true,
            "--gpu-memory-mib" => {
                result.gpu_memory_mib = args
                    .next()
                    .context("--gpu-memory-mib needs a size")?
                    .parse()?;
                ensure!(
                    (1..=32768).contains(&result.gpu_memory_mib),
                    "GPU memory budget must be 1..32768 MiB"
                );
            }
            "--hardware" => result.hardware = true,
            "--no-occlusion" => result.occlusion_enabled = false,
            "--single-threaded" => result.threaded_simulation = false,
            "--no-interpolation" => result.render_interpolation = false,
            "--settings" => {
                result.settings = Some(
                    args.next()
                        .context("--settings needs a player settings file")?
                        .into(),
                )
            }
            "--smoke" => result.smoke = true,
            "--benchmark-frames" => {
                let frames = args
                    .next()
                    .context("--benchmark-frames needs a count")?
                    .parse()?;
                ensure!(
                    (1..=1000).contains(&frames),
                    "benchmark frames must be within 1..1000"
                );
                result.benchmark_frames = Some(frames);
            }
            "--scene" => result.scene = Some(args.next().context("--scene needs a file")?.into()),
            "--write-scene" => {
                result.write_scene = Some(args.next().context("--write-scene needs a file")?.into())
            }
            "--save-path" => {
                result.save_path = args.next().context("--save-path needs a file")?.into()
            }
            "--view" => {
                result.layer = match args.next().as_deref() {
                    Some("2d") => Layer::TwoD,
                    Some("3d") => Layer::ThreeD,
                    _ => bail!("--view expects 2d or 3d"),
                }
            }
            "--frames" => {
                let count = args.next().context("--frames needs a value")?.parse()?;
                ensure!(count > 0, "--frames must be positive");
                result.frames = Some(count);
            }
            "--inject-device-recreation" => result.inject_device_recreation = true,
            "--output" => result.output = args.next().context("--output needs a directory")?.into(),
            "--help" => {
                println!("--single-threaded disables simulation/render overlap for comparison.");
                println!(
                    "--no-interpolation renders exact fixed-tick poses for comparison or lower latency."
                );
                println!(
                    "--content-catalog FILE_OR_URL --content ADDRESS starts an addressable scene; --content-cache DIR selects its cache.\n--project FILE starts a user game from a manifest or .bpack file. Exported games find gamepack.bpack beside the executable.\n--export-project FILE --export-dir NEW_FOLDER exports a native game using this player.\n--settings FILE reads and saves player settings there instead of the per-user location; --frames and --verify-* runs otherwise use defaults.\n--verify-flap-woods checks start, score, pause, game over, retry and quit without graphics.\n--verify-first-trail checks the reference route without graphics; add --frames 340 to present the route."
                );
                println!(
                    "--join-lobby ID (or +connect_lobby ID) accepts a Steam invitation; requires a --features steam build and the multiplayer scene.\nbozzard-player [--backend metal|vulkan|dx12] [--software|--hardware] [--frames N]\nbozzard-player --smoke [--backend ...] [--software|--hardware] [--output DIRECTORY]\n--benchmark-frames N compares reference/culling/cached draws during --smoke --scene.\n--inject-device-recreation rebuilds the GPU after one presented frame with --frames 2 or more.\n--no-occlusion disables hierarchical depth culling for reference comparisons.\n--gpu-memory-mib N sets the imported-asset GPU budget (default 512); unused resources are evicted.\n--scene FILE loads JSON; --write-scene FILE saves it and exits without a GPU.\n--view 2d|3d chooses the starting view; --save-path FILE sets the F5 destination.\nWithout gameplay logic: 1/2 switch views, Space pauses, arrows pan, F5 saves, R reloads, Escape closes.\nScript and Blueprint scenes own their keys; F5 saves and F6 reloads.\nPlayer Controller scenes: WASD move, Space jump, right-drag orbit. Progress/win in title; physical R restarts."
                );
                return Ok(None);
            }
            _ => bail!("unknown argument: {arg}"),
        }
    }
    ensure!(
        !(result.software && result.hardware),
        "--software and --hardware are mutually exclusive"
    );
    ensure!(
        !(result.smoke && result.frames.is_some()),
        "--frames is for windowed runs; --smoke runs the graphics verification suite"
    );
    ensure!(
        !result.inject_device_recreation || result.frames.is_some_and(|count| count >= 2),
        "--inject-device-recreation requires --frames 2 or more"
    );
    ensure!(
        result.write_scene.is_none() || (!result.smoke && result.frames.is_none()),
        "--write-scene is a standalone command; it cannot be combined with --smoke or --frames"
    );
    ensure!(
        result.benchmark_frames.is_none() || (result.smoke && result.scene.is_some()),
        "--benchmark-frames requires --smoke --scene FILE"
    );
    project::resolve(&mut result)?;
    Ok(Some(result))
}
