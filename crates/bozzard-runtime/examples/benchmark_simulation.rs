//! Headless fixed ticks of a physics, Player Controller and script scene.
//! Script movers use `move_with_collision` and rotate parented rigs, so the tick
//! exercises collision, hierarchy validation and the script read view together.
//!
//! cargo run --release -p bozzard-runtime --example benchmark_simulation -- 512
//! The optional argument is the number of parented scenery objects, half with colliders.
use bozzard_runtime::SceneRuntime;
use bozzard_scene::{GameplayInput, Scene};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Instant};

const MOVERS: usize = 16;
const BODIES: usize = 16;
const RIGS: usize = 8;

const DRIVER: &str = r#"
fn on_update(me, dt) {
    let t = elapsed_time();
    for i in 0..16 {
        let phase = t * 2.0 + i;
        move_with_collision("mover-" + i, [phase.sin() * 0.05, 0.0, phase.cos() * 0.05]);
    }
    for i in 0..8 {
        rotate("rig-" + i, [0.0, 1.5, 0.0]);
        set_position("rig-" + i + "-arm-0", [1.0 + (t + i).sin() * 0.25, 0.5, 0.0]);
    }
}
"#;

fn object(id: String, translation: [f32; 3], extra: Value) -> Value {
    let mut object = json!({
        "id": id, "name": "Object",
        "transform": {"translation": translation, "rotation_degrees": [0, 0, 0], "scale": [1, 1, 1]}
    });
    object
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    object
}

fn main() -> anyhow::Result<()> {
    let scenery: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(512);
    anyhow::ensure!(scenery <= 4096, "scenery must be at most 4096");
    let mut objects = vec![
        object(
            "camera".into(),
            [0., 4., 8.],
            json!({"camera": {"projection": "perspective", "vertical_fov_degrees": 55, "near": 0.1, "far": 200}}),
        ),
        object(
            "player".into(),
            [0., 1., 0.],
            json!({"collider": {"size": [0.8, 1.2, 0.8]}, "gravity": {"enabled": true},
                "player_controller": {"camera": "camera", "capsule_radius": 0.4, "capsule_height": 1.2,
                    "move_speed": 3, "fall_height": -20}}),
        ),
        object(
            "floor".into(),
            [0., -0.5, 0.],
            json!({"collider": {"size": [200, 1, 200]}}),
        ),
        object(
            "controller".into(),
            [0., 0., 0.],
            json!({"script_manager": {"scripts": [{"enabled": true, "script": "driver"}]}}),
        ),
    ];
    for i in 0..BODIES {
        objects.push(object(
            format!("body-{i}"),
            [(i % 4) as f32 * 2. - 30., 1. + (i / 4) as f32 * 1.5, 20.],
            json!({"collider": {}, "gravity": {"enabled": true}}),
        ));
    }
    for i in 0..MOVERS {
        objects.push(object(
            format!("mover-{i}"),
            [(i % 4) as f32 * 3. + 20., 0.5, (i / 4) as f32 * 3. + 20.],
            json!({"collider": {"size": [0.8, 0.8, 0.8]}}),
        ));
    }
    for i in 0..RIGS {
        objects.push(object(
            format!("rig-{i}"),
            [i as f32 * 4. - 16., 0., -20.],
            json!({}),
        ));
        for arm in 0..4 {
            let mut child = object(
                format!("rig-{i}-arm-{arm}"),
                [1. + arm as f32, 0.5, 0.],
                json!({}),
            );
            child["parent"] = json!(format!("rig-{i}"));
            objects.push(child);
        }
    }
    // Static colliders and parented decorations, like an authored level.
    for i in 0..scenery {
        let group = i / 32;
        if i % 32 == 0 {
            objects.push(object(
                format!("group-{group:03}"),
                [
                    (group % 8) as f32 * 24. - 96.,
                    0.,
                    (group / 8) as f32 * -24. - 72.,
                ],
                json!({}),
            ));
        }
        let mut child = object(
            format!("scenery-{i:04}"),
            [(i % 8) as f32 * 3., 0.5, (i / 8 % 4) as f32 * 3.],
            if i % 2 == 0 {
                json!({"collider": {}})
            } else {
                json!({})
            },
        );
        child["parent"] = json!(format!("group-{group:03}"));
        objects.push(child);
    }
    let scene = Scene::from_json(
        &json!({
            "version": 1, "name": "Simulation benchmark", "views": {"3d": "camera"},
            "assets": {"driver": {"kind": "script", "path": "driver.rhai"}},
            "objects": objects
        })
        .to_string(),
    )?;
    let mut demo = SceneRuntime::new(&scene)?;
    demo.with_instance(|instance, _| instance.register_script("driver".into(), DRIVER.into()))?;
    println!("simulation_benchmark objects={}", scene.objects.len());
    let mut samples = Vec::new();
    let mut stages = BTreeMap::<_, Vec<f64>>::new();
    for tick in 0..360 {
        let phase = tick as f32 * 0.03;
        demo.set_gameplay_input(GameplayInput {
            movement: [phase.sin(), phase.cos()],
            ..Default::default()
        });
        let profiler = &mut demo
            .app
            .world
            .resource_mut::<bozzard_diagnostics::Diagnostics>()
            .unwrap()
            .profiler;
        profiler.recording = true;
        profiler.begin_frame();
        let start = Instant::now();
        demo.app.step();
        demo.check_simulation()?;
        if tick >= 60 {
            samples.push(start.elapsed().as_secs_f64() * 1000.);
            for span in &demo
                .app
                .world
                .resource::<bozzard_diagnostics::Diagnostics>()
                .unwrap()
                .profiler
                .spans
            {
                stages.entry(span.name).or_default().push(span.duration_ms);
            }
        }
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "simulation fixed ticks: median {:.3} ms, p95 {:.3} ms",
        samples[samples.len() / 2],
        samples[samples.len() * 95 / 100]
    );
    for (name, mut values) in stages {
        values.sort_by(f64::total_cmp);
        println!(
            "  {name}: median {:.3} ms, p95 {:.3} ms",
            values[values.len() / 2],
            values[values.len() * 95 / 100]
        );
    }
    // Identical final poses show an optimization did not change the simulation.
    let transforms = demo.instance().global_transforms(&demo.app.world)?;
    let digest = transforms
        .values()
        .fold(0xcbf29ce484222325_u64, |hash, matrix| {
            matrix.to_cols_array().iter().fold(hash, |hash, value| {
                (hash ^ u64::from(value.to_bits())).wrapping_mul(0x100000001b3)
            })
        });
    println!("simulation final pose digest {digest:016x}");
    Ok(())
}
