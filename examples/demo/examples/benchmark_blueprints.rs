//! Run with --release; deterministic workload, no GPU or filesystem inside the timed loop.
use bozzard_demo::SceneDemo;
use bozzard_scene::{Blueprint, BlueprintAttachment, GameplayInput, Object, Scene};
use std::time::Instant;
fn main() -> anyhow::Result<()> {
    let mut scene =
        Scene::from_json(r#"{"version":1,"name":"blueprint benchmark","views":{},"objects":[]}"#)?;
    let graph = Blueprint::spinning();
    for index in 0..256 {
        scene.objects.push(Object {
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
    let mut demo = SceneDemo::new(&scene)?;
    for _ in 0..10 {
        demo.with_instance(|i, w| i.step_blueprints(w, 1. / 60., GameplayInput::default()))?;
    }
    let started = Instant::now();
    for _ in 0..300 {
        demo.with_instance(|i, w| i.step_blueprints(w, 1. / 60., GameplayInput::default()))?;
    }
    let seconds = started.elapsed().as_secs_f64();
    println!(
        "256 owners, 2048 graphs, 614400 actions: {:.3} ms/tick, {:.0} actions/s",
        seconds * 1000. / 300.,
        614400. / seconds
    );
    Ok(())
}
