//! Compare disabled, breakpoints-only, and tracing costs on the same warmed workload.
use bozzard_demo::SceneDemo;
use bozzard_scene::{
    Blueprint, BlueprintAttachment, BlueprintDebugger, GameplayInput, Object, Scene,
};
use std::time::Instant;
fn main() -> anyhow::Result<()> {
    let mut stress =
        Scene::from_json(r#"{"version":1,"name":"blueprint benchmark","views":{},"objects":[]}"#)?;
    let graph = Blueprint::spinning();
    for index in 0..256 {
        stress.objects.push(Object {
            id: format!("owner-{index}"),
            name: format!("Owner {index}"),
            blueprints: vec![
                BlueprintAttachment {
                    enabled: true,
                    graph: graph.clone()
                };
                8
            ],
            ..Default::default()
        });
    }
    let middleware = Scene::from_json(include_str!("../scenes/middleware-lab.json"))?;
    for (name, scene, only_blueprints) in [
        ("2,048 spinning graphs", &stress, true),
        ("Middleware lab", &middleware, false),
    ] {
        for mode in [0, 1, 2, 0, 1, 2] {
            let mut demo = SceneDemo::new(scene)?;
            demo.with_instance(|i, _| i.set_gpu_particles(true));
            let mut debugger = BlueprintDebugger::new([]);
            debugger.enabled = mode > 0;
            debugger.tracing = mode == 2;
            demo.app.world.insert_resource(debugger);
            let mut samples = Vec::new();
            for tick in 0..350 {
                let started = Instant::now();
                if only_blueprints {
                    demo.with_instance(|i, w| {
                        i.step_blueprints(w, 1. / 60., GameplayInput::default())
                    })?;
                } else {
                    demo.app.step();
                }
                if tick >= 50 {
                    samples.push(started.elapsed().as_secs_f64() * 1e6);
                }
            }
            demo.check_simulation()?;
            samples.sort_by(f64::total_cmp);
            println!(
                "{name}, {}: median {:.2} us/tick, p95 {:.2}; trace {}",
                ["disabled", "breakpoints only", "tracing"][mode],
                samples[150],
                samples[285],
                demo.app
                    .world
                    .resource::<BlueprintDebugger>()
                    .unwrap()
                    .trace
                    .len()
            );
        }
    }
    Ok(())
}
