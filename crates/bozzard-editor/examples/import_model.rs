//! Exercise the same project-local model import used by the editor, without a window.
use anyhow::{Context, Result, ensure};
use bozzard_editor::Editor;
use std::{path::PathBuf, time::Instant};

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let source = std::path::absolute(PathBuf::from(
        arguments
            .next()
            .context("usage: import_model SOURCE NEW_SCENE")?,
    ))?;
    let path = std::path::absolute(PathBuf::from(
        arguments.next().context("missing new scene path")?,
    ))?;
    ensure!(!path.exists(), "destination scene already exists");
    let mut scene = bozzard_demo::scene_document()?;
    scene.name = "Imported model".into();
    scene.objects.retain(|object| object.camera.is_some());
    let mut editor = Editor::new(scene, &path)?;
    let start = Instant::now();
    let id = editor.import(&source)?;
    editor.add_asset_to_scene(&id)?;
    editor.save(&path)?;
    let reopened = Editor::open(&path)?;
    reopened.assets.require_ready()?;
    println!(
        "import_save_reopen_ms={:.2} asset={} project_source={} scene={}",
        start.elapsed().as_secs_f64() * 1000.0,
        id,
        reopened.scene().assets[&id].path,
        path.display()
    );
    Ok(())
}
