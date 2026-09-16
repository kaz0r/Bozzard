//! Compare instrumentation off/on with the same simulation and warmed allocations.
use bozzard_diagnostics::{Console, Diagnostics, Level, Location, MAX_EVENTS};
use std::{hint::black_box, time::Instant};
fn main() -> anyhow::Result<()> {
    let scene = bozzard_scene::Scene::from_json(include_str!("../scenes/middleware-lab.json"))?;
    for recording in [false, true, false, true] {
        let mut demo = bozzard_demo::SceneDemo::new(&scene)?;
        demo.with_instance(|instance, _| instance.set_gpu_particles(true));
        let mut times = Vec::with_capacity(1000);
        for tick in 0..1200 {
            let started = Instant::now();
            let d = demo.app.world.resource_mut::<Diagnostics>().unwrap();
            d.profiler.recording = recording;
            d.profiler.begin_frame();
            demo.app.step();
            if tick >= 200 {
                times.push(started.elapsed().as_secs_f64() * 1e6);
            }
        }
        demo.check_simulation()?;
        times.sort_by(f64::total_cmp);
        println!(
            "capture {recording}: median {:.2} us/tick, p95 {:.2} us/tick; {} retained spans",
            times[500],
            times[950],
            demo.app
                .world
                .resource::<Diagnostics>()
                .unwrap()
                .profiler
                .spans
                .len()
        );
    }
    let mut console = Console::default();
    let started = Instant::now();
    for i in 0..100_000 {
        console.push(
            Level::Info,
            "Benchmark",
            &i.to_string(),
            Location::default(),
            None,
        );
    }
    assert_eq!(console.events.len(), MAX_EVENTS);
    println!(
        "100k unique messages: {:.1} ms; {} retained, {} discarded",
        started.elapsed().as_secs_f64() * 1000.,
        console.events.len(),
        console.discarded
    );
    black_box(console);
    Ok(())
}
