//! Deterministic terrain painting workshop, prepared through real editor transactions.
use anyhow::{Context, Result, ensure};
use bozzard_assets::{
    job::Job,
    terrain::{BrushMode, Terrain, TerrainBrush, TerrainPaint, TerrainPaintBrush},
};
use bozzard_editor::{Editor, TerrainRequest};
use bozzard_scene::Scene;
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

fn wait<T: Send + 'static>(job: &Job<T>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        if Instant::now() >= deadline {
            job.cancel();
            anyhow::bail!("terrain painting workshop preparation timed out");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: terrain_painting OUTPUT_DIRECTORY")?,
    );
    ensure!(args.next().is_none(), "unexpected additional argument");
    // Reserve the directory atomically; a concurrent generator must not overwrite it.
    if let Some(parent) = root
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&root).context("choose a new workshop directory")?;
    let mut scene = Scene::from_json(
        r#"{
            "version":1,"name":"Terrain Painting Workshop","views":{"3d":"camera"},
            "objects":[{
                "id":"camera","name":"Landscape camera",
                "transform":{"translation":[23,23,29],"rotation_degrees":[-31,38,0],"scale":[1,1,1]},
                "camera":{"projection":"perspective","vertical_fov_degrees":55,"near":0.1,"far":200}
            }]
        }"#,
    )?;
    scene.lighting.sun_direction = [-0.45, 0.85, 0.3];
    scene.lighting.sun_color = [1., 0.94, 0.83];
    scene.lighting.sun_intensity = 2.4;
    scene.lighting.ambient_intensity = 0.18;
    scene.lighting.shadow_resolution = 1024;
    let path = root.join("scene.json");
    let mut editor = Editor::new(scene, &path)?;
    let mut terrain = Terrain::flat([65; 2], [32.; 2])?;
    for (center, radius, strength) in [
        ([-8., -5.], 11., 3.3),
        ([8., -8.], 9., 5.),
        ([9., 3.], 7., 2.4),
        ([-8., 9.], 8., 1.8),
    ] {
        terrain.brush(TerrainBrush {
            mode: BrushMode::Raise,
            center,
            radius,
            strength,
            target_height: 0.,
        })?;
    }
    terrain.paint = Some(TerrainPaint::new(terrain.heights.len()));
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain,
        position: [0.; 3],
    })?;
    let ground = editor.accept_terrain(wait(&job)?)?;
    editor.save(&root.join("before.json"))?;

    let source = editor.terrain_source(&ground)?;
    let mut painted = source.terrain.clone();
    // Overlapping soft stamps create a continuous trail without a separate path mesh.
    for step in 0..=96 {
        let z = 14.8 - step as f32 * 29.6 / 96.;
        let x = -4. + 3.2 * (z * 0.23).sin() + z * 0.1;
        painted.paint(TerrainPaintBrush {
            layer: 1,
            center: [x, z],
            radius: 1.65,
            strength: 0.6,
        })?;
    }
    for (layer, center, radius, strength) in [
        (1, [-1., 4.], 3.5, 0.85),
        (2, [8., -8.], 6.1, 1.),
        (2, [10., 2.], 3.5, 0.85),
        (2, [-9., -6.], 2.8, 0.65),
    ] {
        painted.paint(TerrainPaintBrush {
            layer,
            center,
            radius,
            strength,
        })?;
    }
    ensure!(
        painted.heights == source.terrain.heights,
        "painting must not change terrain geometry"
    );
    let job = editor.terrain_job(TerrainRequest::Sculpt {
        source: Box::new(source),
        terrain: painted,
    })?;
    editor.accept_terrain(wait(&job)?)?;
    editor.save(&path)?;
    fs::write(
        root.join("game.bozzard.json"),
        r#"{"version":1,"name":"Terrain Painting Workshop","start_scene":"scene.json","view":"3d","cook":"universal"}"#,
    )?;
    println!("Before: {}", root.join("before.json").display());
    println!("Painted: {}", path.display());
    Ok(())
}
