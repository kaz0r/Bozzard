//! Headless scene simulation with optional real-time pacing and graceful shutdown.
//! Steam listen-server hosting lives in the multiplayer player.
use anyhow::{Context, Result, bail};
use bozzard_demo::{SceneDemo, load_document, save_document_from};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let mut ticks: u32 = 120;
    let mut realtime = false;
    let mut scene: Option<PathBuf> = None;
    let mut save: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--realtime" => realtime = true,
            "--ticks" => ticks = args.next().context("--ticks needs a count")?.parse()?,
            "--scene" => scene = Some(args.next().context("--scene needs a file")?.into()),
            "--save-scene" => save = Some(args.next().context("--save-scene needs a file")?.into()),
            "--help" => {
                println!(
                    "bozzard-server [--realtime] [--ticks COUNT] [--scene FILE] [--save-scene FILE]\nRuns fixed scene simulation without windows, graphics or audio. --realtime paces at 60 Hz; --ticks 0 runs until Ctrl-C/SIGTERM. Steam lobbies use the player listen server."
                );
                return Ok(());
            }
            _ => bail!("unknown argument '{arg}'"),
        }
    }
    let document = load_document(scene.as_deref())?;
    let mut demo = SceneDemo::new_with_prefabs(&document, scene.as_deref())?;
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopped);
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    eprintln!("server_started realtime={realtime} tick_limit={ticks}");
    let mut pacer = bozzard_network::Pacer::default();
    let mut last = Instant::now();
    let mut diagnostics = last;
    while !stopped.load(Ordering::Relaxed) && (ticks == 0 || demo.app.ticks() < u64::from(ticks)) {
        let steps = if realtime {
            let now = Instant::now();
            let steps = pacer.advance(now.duration_since(last));
            last = now;
            steps
        } else {
            1
        };
        for _ in 0..steps {
            if stopped.load(Ordering::Relaxed)
                || (ticks != 0 && demo.app.ticks() >= u64::from(ticks))
            {
                break;
            }
            demo.app.step();
            demo.check_simulation()?;
        }
        if diagnostics.elapsed() >= Duration::from_secs(5) {
            eprintln!(
                "server_status ticks={} entities={} dropped_ms={}",
                demo.app.ticks(),
                demo.app.world.len(),
                pacer.dropped.as_millis()
            );
            diagnostics = Instant::now();
        }
        if realtime {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    eprintln!(
        "server_shutdown graceful={} ticks={} dropped_ms={}",
        stopped.load(Ordering::Relaxed),
        demo.app.ticks(),
        pacer.dropped.as_millis()
    );
    if let Some(path) = save {
        save_document_from(
            &demo.instance().capture(&demo.app.world)?,
            &path,
            scene.as_deref(),
        )?;
    }
    println!(
        "headless_ok ticks={} entities={} scene={:?}",
        demo.app.ticks(),
        demo.app.world.len(),
        document.name
    );
    Ok(())
}
