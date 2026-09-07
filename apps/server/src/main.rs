//! Finite headless scene simulation. Networking is a later milestone.
use anyhow::{Context, Result, bail};
use bozzard_demo::{SceneDemo, load_document, save_document_from};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut ticks: u32 = 120;
    let mut scene: Option<PathBuf> = None;
    let mut save: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ticks" => ticks = args.next().context("--ticks needs a count")?.parse()?,
            "--scene" => scene = Some(args.next().context("--scene needs a file")?.into()),
            "--save-scene" => save = Some(args.next().context("--save-scene needs a file")?.into()),
            "--help" => {
                println!(
                    "bozzard-server [--ticks COUNT] [--scene FILE] [--save-scene FILE]\nRuns fixed scene simulation without windows, graphics, audio, or networking."
                );
                return Ok(());
            }
            _ => bail!("unknown argument '{arg}'"),
        }
    }
    let document = load_document(scene.as_deref())?;
    let mut demo = SceneDemo::new(&document)?;
    for _ in 0..ticks {
        demo.app.step();
        demo.check_simulation()?;
    }
    if let Some(path) = save {
        save_document_from(
            &demo.instance.capture(&demo.app.world)?,
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
