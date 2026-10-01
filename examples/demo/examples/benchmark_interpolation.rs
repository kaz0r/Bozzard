//! Reproducible native frame ordering on a synthetic 60/120/144 Hz clock.
//! Measures CPU extraction/ticks and presentation motion, without opening a window.
use anyhow::{Result, ensure};
use bozzard_demo::SceneDemo;
use bozzard_scene::{Layer, Scene};
use glam::Vec3;
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

fn percentile(values: &[f64], fraction: f64) -> f64 {
    values[((values.len() as f64 * fraction).ceil() as usize).saturating_sub(1)]
}

fn distribution(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    json!({
        "n": values.len(),
        "median_ms": percentile(&values, 0.5),
        "p95_ms": percentile(&values, 0.95),
        "p99_ms": percentile(&values, 0.99),
    })
}

fn main() -> Result<()> {
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "work/interpolation".into());
    fs::create_dir_all(&output)?;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenes/interpolation-lab.json");
    let source = Scene::from_json(&fs::read_to_string(&path)?)?;
    let pillar = source.objects.iter().find(|o| o.id == "pillar").unwrap();
    let mut motion = fs::File::create(output.join("motion.csv"))?;
    writeln!(
        motion,
        "hz,threaded,interpolated,frame,time_s,shown_x,live_x"
    )?;
    let mut results = Vec::new();
    for scenery in [0, 1024] {
        let mut scene = source.clone();
        scene.objects.reserve(scenery);
        for index in 0..scenery {
            let mut object = pillar.clone();
            object.id = format!("scenery-{index:04}");
            object.transform.translation = [
                (index % 32) as f32 * 2. - 32.,
                1.,
                (index / 32) as f32 * -2. - 4.,
            ];
            scene.objects.push(object);
        }
        let mut final_state = None;
        for hz in [60_u128, 120, 144] {
            for threaded in [false, true] {
                for interpolated in [false, true] {
                    let mut demo = SceneDemo::new_with_prefabs(&scene, Some(&path))?;
                    demo.set_threaded_simulation(threaded)?;
                    let step = demo.app.timestep().as_nanos();
                    let mover = demo.instance().entity("mover").unwrap();
                    let mut time = 0;
                    let mut previous_shown: Option<f64> = None;
                    let mut previous_time = 0;
                    let mut squared_motion_error = 0.;
                    let mut motion_samples = 0;
                    let mut extraction = Vec::new();
                    let mut simulation = Vec::new();
                    let mut waits = Vec::new();
                    for frame in 0..hz * 2 {
                        let started = Instant::now();
                        demo.set_render_interpolation(interpolated)?;
                        let view = demo.render_view(Layer::ThreeD, 16. / 9., None)?;
                        let extraction_ms = started.elapsed().as_secs_f64() * 1000.;
                        let shown = f64::from(
                            view.objects
                                .iter()
                                .find(|(_, d)| d.color == [0.1, 0.75, 1.])
                                .unwrap()
                                .0
                                .transform_point3(Vec3::ZERO)
                                .x,
                        );
                        let live = demo
                            .app
                            .world
                            .get::<bozzard_scene::Transform>(mover)
                            .unwrap()
                            .translation[0];
                        if scenery == 0 {
                            writeln!(
                                motion,
                                "{hz},{threaded},{interpolated},{frame},{:.9},{shown:.9},{live:.9}",
                                time as f64 / 1e9
                            )?;
                        }
                        if frame >= hz / 2 {
                            extraction.push(extraction_ms);
                            if let Some(previous) = previous_shown {
                                let ideal_step = (time - previous_time) as f64 / 1e9 * 3.;
                                squared_motion_error +=
                                    ((shown - previous - ideal_step) / ideal_step).powi(2);
                                motion_samples += 1;
                            }
                        }
                        previous_shown = Some(shown);
                        previous_time = time;
                        let next_time = (frame + 1) * step * 60 / hz;
                        demo.advance_with_frame(
                            Duration::from_nanos((next_time - time) as u64),
                            || (),
                        )?;
                        time = next_time;
                        let stats = demo
                            .app
                            .world
                            .resource::<bozzard_diagnostics::SimulationMetrics>()
                            .unwrap();
                        if frame >= hz / 2 {
                            if stats.steps > 0 {
                                simulation.push(stats.cpu_ms);
                            }
                            waits.push(stats.wait_ms);
                        }
                    }
                    ensure!(
                        demo.app.ticks() == 120,
                        "frame partition changed fixed ticks"
                    );
                    let state = (
                        demo.instance().capture(&demo.app.world)?,
                        demo.instance().save_game_json(&demo.app.world)?,
                    );
                    if let Some(reference) = &final_state {
                        ensure!(reference == &state, "presentation changed gameplay state");
                    } else {
                        final_state = Some(state);
                    }
                    let result = json!({
                        "scenery":scenery,"hz":hz as u32,"threaded":threaded,"interpolated":interpolated,
                        "ticks":demo.app.ticks(),
                        "extraction":distribution(extraction),"simulation":distribution(simulation),"join_wait":distribution(waits),
                        "motion_step_rms_percent":(squared_motion_error / motion_samples as f64).sqrt() * 100.,
                    });
                    println!("{result}");
                    results.push(result);
                }
            }
        }
    }
    fs::write(
        output.join("summary.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    Ok(())
}
