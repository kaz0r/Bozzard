//! CPU-only bake using the same cancellable worker and publication path as the editor.
use anyhow::{Context, Result, ensure};
use bozzard_editor::Editor;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let input = PathBuf::from(
        args.next()
            .context("usage: bake_gi INPUT.json OUTPUT.json [--fit]")?,
    );
    let output = PathBuf::from(args.next().context("missing output scene path")?);
    let fit = args.next();
    ensure!(
        fit.as_deref().is_none_or(|s| s == "--fit") && args.next().is_none(),
        "unknown argument"
    );
    let mut editor = Editor::open(&input)?;
    if fit.is_some() {
        editor.fit_gi_volume()?;
    }
    let started = Instant::now();
    let job = editor.bake_gi_job()?;
    let mut label = String::new();
    loop {
        if let Some(result) = job.poll() {
            editor.accept_gi(result?)?;
            break;
        }
        let next = job.label();
        if next != label {
            println!("{next}");
            label = next;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let bake = editor.scene().gi.baked.as_ref().unwrap();
    println!(
        "gi_bake_ok probes={} bytes={} elapsed_ms={:.1}",
        bake.volume.probe_count(),
        bake.probes.len() * 16,
        started.elapsed().as_secs_f64() * 1000.
    );
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    editor.save(&output)?;
    let reopened = Editor::open(&output)?;
    ensure!(reopened.gi_current(), "saved bake source changed on reopen");
    println!("gi_save_reload_ok output={}", output.display());
    Ok(())
}
