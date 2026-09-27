//! Headless factory tick benchmark, using the same scene and scripts as the player.
use bozzard_scene::blueprint::{BlackboardValue, Value};
use std::{path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../earth-factory/scenes/earth.json")
        });
    let mut scene = bozzard_scene::Scene::from_json(&std::fs::read_to_string(&path)?)?;
    let controller = scene
        .objects
        .iter_mut()
        .find(|o| o.id == "controller")
        .unwrap();
    for (key, value) in [("title_open", false), ("creative", true)] {
        controller
            .blackboard
            .insert(key.into(), BlackboardValue::Scalar(Value::Bool(value)));
    }
    scene
        .blackboard
        .insert("seed".into(), BlackboardValue::Scalar(Value::Number(4.)));
    let start = Instant::now();
    let mut demo = bozzard_demo::SceneDemo::new_with_prefabs(&scene, Some(&path))?;
    println!("load: {:.1} ms", start.elapsed().as_secs_f64() * 1000.);
    let mut samples = Vec::new();
    let mut stages = std::collections::BTreeMap::<_, Vec<f64>>::new();
    for tick in 0..240 {
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
    demo.check_simulation()?;
    samples.sort_by(f64::total_cmp);
    println!(
        "idle creative ticks: median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
        samples[samples.len() / 2],
        samples[samples.len() * 95 / 100],
        samples.last().unwrap()
    );
    for (name, mut values) in stages {
        values.sort_by(f64::total_cmp);
        println!(
            "  {name}: median {:.3} ms, p95 {:.3} ms",
            values[values.len() / 2],
            values[values.len() * 95 / 100]
        );
    }
    Ok(())
}
