//! Generate a portable scale fixture without checking in thousands of repeated objects.
use anyhow::{Context, Result, ensure};
use bozzard_scene::{GameplayInput, Object, Scene, Transform};
use serde_json::json;
use std::{fmt::Write, path::PathBuf};

fn object(
    id: &str,
    name: &str,
    position: [f32; 3],
    scale: [f32; 3],
    mesh: &str,
    color: [f32; 3],
) -> Result<Object> {
    let mesh = if mesh == "stock" {
        json!({"asset":"stock-mesh"})
    } else {
        json!(mesh)
    };
    Ok(serde_json::from_value(json!({
        "id":id, "name":name,
        "transform":{"translation":position,"rotation_degrees":[0,0,0],"scale":scale},
        "drawable":{"layer":"3d","mesh":mesh,"texture":"white","color":color,"uv_scale":[1,1]}
    }))?)
}
fn warehouse(script: &str, stock: &str) -> Result<Scene> {
    let mut scene: Scene = serde_json::from_value(json!({
        "version":1,"name":"Occlusion warehouse · Space opens the shutter",
        "views":{"3d":"camera"},
        "objects":[{
            "id":"camera","name":"Inspection camera",
            "transform":{"translation":[0,2,12],"rotation_degrees":[0,0,0],"scale":[1,1,1]},
            "camera":{"projection":"perspective","vertical_fov_degrees":55,"near":0.1,"far":150}
        }],
        "blackboard":{"shutter-open":{"scalar":{"bool":false}}},
        "assets":{
            "shutter-script":{"kind":"script","path":script},
            "stock-mesh":{"kind":"mesh","path":stock}
        }
    }))?;
    scene.lighting.shadows = false;
    scene.objects.push(object(
        "floor",
        "Warehouse floor",
        [0., -0.15, -2.],
        [12., 0.3, 18.],
        "cube",
        [0.28, 0.31, 0.35],
    )?);
    scene.objects.push(object(
        "rear",
        "Rear wall",
        [0., 2., -10.],
        [12., 4., 0.3],
        "cube",
        [0.38, 0.43, 0.48],
    )?);
    for x in [-6., 6.] {
        scene.objects.push(object(
            &format!("side-{x}"),
            "Side wall",
            [x, 2., -2.],
            [0.3, 4., 16.],
            "cube",
            [0.38, 0.43, 0.48],
        )?);
    }
    let mut shutter = object(
        "shutter",
        "Shutter · Space in Play",
        [0., 2., 4.],
        [12., 4., 0.3],
        "cube",
        [0.18, 0.38, 0.58],
    )?;
    shutter.script_manager = Some(serde_json::from_value(
        json!({"scripts":[{"enabled":true,"script":"shutter-script"}]}),
    )?);
    scene.objects.push(shutter);
    // 16 aisles × 16 columns × 4 shelves = 1,024 detailed stored objects.
    // One shared sphere mesh stresses vertex work while keeping assets portable.
    for aisle in 0..16 {
        for shelf in 0..4 {
            for column in 0..16 {
                let id = format!("stock-{aisle}-{shelf}-{column}");
                scene.objects.push(object(
                    &id,
                    &format!("Stock {aisle}/{shelf}/{column}"),
                    [
                        (column as f32 - 7.5) * 0.48,
                        0.4 + shelf as f32 * 0.86,
                        2. - aisle as f32 * 0.65,
                    ],
                    [0.38; 3],
                    "stock",
                    [
                        0.72,
                        0.34 + shelf as f32 * 0.08,
                        0.16 + aisle as f32 * 0.025,
                    ],
                )?);
            }
        }
    }
    scene.validate()?;
    Ok(scene)
}
fn stock_mesh() -> Result<String> {
    // The scene format imports this shared mesh; Sphere is only an internal
    // renderer primitive. Keep the fixture independent of the renderer crate.
    let mut obj = String::from("# Portable warehouse stock: 1536 triangles\n");
    for ring in 0..=24 {
        let v = ring as f32 / 24.;
        let (sin_phi, cos_phi) = (v * std::f32::consts::PI).sin_cos();
        for segment in 0..=32 {
            let u = segment as f32 / 32.;
            let (sin_theta, cos_theta) = (u * std::f32::consts::TAU).sin_cos();
            let [x, y, z] = [sin_phi * cos_theta, cos_phi, sin_phi * sin_theta];
            writeln!(
                obj,
                "v {} {} {}\nvn {x} {y} {z}\nvt {u} {}",
                x * 0.5,
                y * 0.5,
                z * 0.5,
                1. - v
            )?;
        }
    }
    for ring in 0..24 {
        for segment in 0..32 {
            let a = ring * 33 + segment + 1;
            let b = a + 33;
            for face in [[a, b, a + 1], [a + 1, b, b + 1]] {
                writeln!(
                    obj,
                    "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}",
                    face[0], face[1], face[2]
                )?;
            }
        }
    }
    Ok(obj)
}
fn main() -> Result<()> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "work/occlusion-lab.json".into()),
    );
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("output requires a UTF-8 filename")?;
    let script_name = format!("{stem}-shutter.rs");
    let script_path = path.with_file_name(&script_name);
    let stock_name = format!("{stem}-stock.obj");
    let stock_path = path.with_file_name(&stock_name);
    ensure!(
        !path.exists() && !script_path.exists() && !stock_path.exists(),
        "choose a new output filename; existing files are preserved"
    );
    let scene = warehouse(&script_name, &stock_name)?;
    let json = scene.to_json()?;
    let mesh = stock_mesh()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    std::fs::write(&stock_path, mesh)?;
    std::fs::write(
        &script_path,
        include_str!("../scenes/scripts/occlusion-lab.rs"),
    )?;
    std::fs::write(&path, json)?;
    let mut demo = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path))?;
    demo.app.step();
    demo.check_simulation()?;
    let shutter = demo
        .instance()
        .entity("shutter")
        .context("shutter missing")?;
    for x in [14., 0.] {
        demo.set_gameplay_input(GameplayInput {
            pressed_keys: 1u128
                << bozzard_scene::keys::BOUND_KEYS
                    .iter()
                    .position(|key| *key == "Space")
                    .unwrap(),
            ..Default::default()
        });
        demo.app.step();
        demo.check_simulation()?;
        ensure!(
            demo.app
                .world
                .get::<Transform>(shutter)
                .context("shutter transform missing")?
                .translation
                == [x, 2., 4.],
            "Space did not toggle the shutter"
        );
        demo.set_gameplay_input(GameplayInput::default());
        demo.app.step();
    }
    println!(
        "Generated {} objects: {}",
        scene.objects.len(),
        path.display()
    );
    println!(
        "Open in the editor, enable View → Renderer statistics / Debug, then Play. Space opens/closes the shutter. View → Occlusion culling compares the reference path. Shadows are disabled to isolate color-pass work."
    );
    Ok(())
}
