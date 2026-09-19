//! Portable terrain, foliage and blockout fixture for native authoring checks.
use anyhow::{Context, Result, ensure};
use bozzard_assets::{
    blockout::{Blockout, BrushPrimitive},
    job::Job,
    terrain::{BrushMode, Terrain, TerrainBrush},
};
use bozzard_editor::{Editor, FoliageSettings, TerrainRequest};
use bozzard_scene::{Scene, Transform};
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
        ensure!(Instant::now() < deadline, "fixture preparation timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("usage: level_workshop OUTPUT_DIRECTORY")?,
    );
    ensure!(!root.exists(), "choose a new fixture directory");
    fs::create_dir_all(&root)?;
    let scene = Scene::from_json(
        r#"{
        "version":1,"name":"Level Workshop","views":{"3d":"camera"},"objects":[
        {"id":"camera","name":"Camera","transform":{"translation":[20,20,28],"rotation_degrees":[-30,36,0],"scale":[1,1,1]},"camera":{"projection":"perspective","vertical_fov_degrees":55,"near":0.1,"far":200}},
        {"id":"tree","name":"Tree prototype","spin":[0,15,0],"transform":{"translation":[-12,0,12],"rotation_degrees":[0,0,0],"scale":[1,1,1]}},
        {"id":"trunk","name":"Trunk","parent":"tree","transform":{"translation":[0,0.7,0],"rotation_degrees":[0,0,0],"scale":[0.3,1.4,0.3]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[0.3,0.16,0.06],"uv_scale":[1,1]}},
        {"id":"crown","name":"Crown","parent":"tree","transform":{"translation":[0,1.8,0],"rotation_degrees":[0,0,0],"scale":[1.2,1.4,1.2]},"drawable":{"layer":"3d","mesh":"cube","texture":"white","color":[0.08,0.35,0.12],"uv_scale":[1,1]}}
    ]}"#,
    )?;
    let path = root.join("scene.json");
    let mut editor = Editor::new(scene, &path)?;
    let mut terrain = Terrain::flat([65; 2], [32.; 2])?;
    terrain.brush(TerrainBrush {
        mode: BrushMode::Raise,
        center: [3., -4.],
        radius: 10.,
        strength: 4.,
        target_height: 0.,
    })?;
    let job = editor.terrain_job(TerrainRequest::Create {
        terrain,
        position: [0.; 3],
    })?;
    let ground = editor.accept_geometry(wait(&job)?)?;
    for (index, primitive) in [
        BrushPrimitive::Box,
        BrushPrimitive::Ramp,
        BrushPrimitive::Stairs { steps: 6 },
        BrushPrimitive::Cylinder { sides: 16 },
    ]
    .into_iter()
    .enumerate()
    {
        let job = editor.blockout_job(
            Blockout {
                version: 1,
                primitive,
            },
            vec![Transform {
                translation: [-7.5 + index as f32 * 5., 0., 9.],
                scale: [3., 2., 4.],
                ..Default::default()
            }],
        )?;
        editor.accept_geometry(wait(&job)?)?;
    }
    let job = editor.scatter_foliage_job(
        "tree",
        &ground,
        FoliageSettings {
            center: [0., -4.],
            radius: 10.,
            count: 35,
            seed: 42,
            spacing: 1.8,
            ..Default::default()
        },
    )?;
    let (_, placed, _) = editor.accept_foliage(wait(&job)?)?;
    ensure!(placed == 35, "fixture must place all trees");
    editor.save(&path)?;
    fs::write(
        root.join("game.bozzard.json"),
        r#"{"version":1,"name":"Level Workshop","start_scene":"scene.json","view":"3d","cook":"universal"}"#,
    )?;
    println!("{}", path.display());
    Ok(())
}
