//! Headless factory tick benchmark, using the same scene and scripts as the player.
use bozzard_scene::blueprint::{BlackboardValue, Value};
use std::{path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut output = None;
    while let Some(arg) = args.next() {
        if arg == "--output" {
            output =
                Some(PathBuf::from(args.next().ok_or_else(|| {
                    anyhow::anyhow!("--output needs a JSON path")
                })?));
        } else {
            anyhow::ensure!(
                path.is_none() && !arg.starts_with('-'),
                "expected an optional scene path and --output FILE"
            );
            path = Some(PathBuf::from(arg));
        }
    }
    let path = path.unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/earth-factory/scenes/earth.json")
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
    let mut demo = bozzard_runtime::SceneRuntime::new_with_prefabs(&scene, Some(&path))?;
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
    let raw_samples = samples.clone();
    samples.sort_by(f64::total_cmp);
    println!(
        "factory fixed ticks: median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
        samples[samples.len() / 2],
        samples[samples.len() * 95 / 100],
        samples.last().unwrap()
    );
    let mut stage_reports = serde_json::Map::new();
    for (name, mut values) in stages {
        let raw = values.clone();
        values.sort_by(f64::total_cmp);
        println!(
            "  {name}: median {:.3} ms, p95 {:.3} ms",
            values[values.len() / 2],
            values[values.len() * 95 / 100]
        );
        stage_reports.insert(
            name.into(),
            serde_json::json!({
                "samples_ms": raw, "median_ms": values[values.len()/2],
                "p95_ms": values[values.len()*95/100]
            }),
        );
    }
    if let Some(output) = output {
        std::fs::write(
            output,
            serde_json::to_vec_pretty(&serde_json::json!({
                "warmup_ticks": 60, "sample_ticks": raw_samples.len(),
                "tick_ms": raw_samples, "median_ms": samples[samples.len()/2],
                "p95_ms": samples[samples.len()*95/100], "max_ms": samples.last(),
                "stages": stage_reports
            }))?,
        )?;
    }
    Ok(())
}
