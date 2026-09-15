//! Measures simulation CPU work, separately from GPU rendering and audio-device time.
use anyhow::Result;
use bozzard_scene::{Layer, Scene};
use std::{path::Path, time::Instant};
fn measure(scene: &Scene, gpu_particles: bool) -> Result<(f64, usize)> {
    let mut demo = bozzard_demo::SceneDemo::new(scene)?;
    demo.with_instance(|instance, _| instance.set_gpu_particles(gpu_particles));
    for _ in 0..360 {
        demo.app.step();
    }
    demo.check_simulation()?;
    let mut samples = Vec::with_capacity(240);
    for _ in 0..240 {
        let start = Instant::now();
        demo.app.step();
        samples.push(start.elapsed().as_secs_f64() * 1e6);
    }
    demo.check_simulation()?;
    samples.sort_by(f64::total_cmp);
    let count = demo
        .instance()
        .view(&demo.app.world, Layer::ThreeD, 16. / 9.)?
        .particles
        .len();
    Ok((samples[samples.len() / 2], count))
}
fn main() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/middleware-lab.json");
    let scene = Scene::from_json(&std::fs::read_to_string(path)?)?;
    for (name, document) in [
        ("Authored lab", scene.clone()),
        ("16,384-particle stress", {
            let mut scene = scene;
            let template = scene
                .objects
                .iter()
                .find(|o| o.particle_emitter.is_some())
                .unwrap()
                .clone();
            scene.objects.retain(|o| o.particle_emitter.is_none());
            for i in 0..8 {
                let mut emitter = template.clone();
                emitter.id = format!("stress-{i}");
                let settings = emitter.particle_emitter.as_mut().unwrap();
                settings.rate = 500.;
                settings.max_particles = 2048;
                settings.lifetime = 30.;
                scene.objects.push(emitter);
            }
            scene
        }),
    ] {
        let cpu = measure(&document, false)?;
        let gpu = measure(&document, true)?;
        assert_eq!(cpu.1, gpu.1);
        if name == "16,384-particle stress" {
            assert_eq!(cpu.1, 16_384, "stress fixture must fill the global budget");
        }
        println!(
            "{name}: {} live particles; median CPU reference {:.1} us/tick; GPU-backend scheduling {:.1} us/tick; {:.2}x less simulation CPU time",
            cpu.1,
            cpu.0,
            gpu.0,
            cpu.0 / gpu.0
        );
    }
    println!(
        "Timing covers the shared simulation tick only, excludes GPU execution/readback, and is machine-dependent."
    );
    Ok(())
}
