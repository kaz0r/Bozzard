//! CPU-only scripted Flap Woods multiplayer benchmark.
//!
//! Run with `cargo run --release -p bozzard-network --example flap_multiplayer_perf`.
//! No renderer, GPU, Steam client, or second account is required.

use bozzard_ecs::World;
use bozzard_network::{
    Peer,
    flap::{Host, Phase, Replica},
    rules::Rules,
};
use bozzard_scene::{GameplayInput, NetworkFrame, Scene, load_sources};
use serde_json::json;
use std::{hint::black_box, path::Path, sync::Arc, time::Instant};

const PEERS: [Peer; 4] = [10, 20, 30, 40];
const COUNTDOWN_TICKS: usize = 300;
const MEASURED_TICKS: usize = 30;
const SAMPLES: usize = 200;
const WARMUP_TICKS: usize = 1;

fn load_rules() -> Arc<Rules> {
    let scene_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/flap-woods-multiplayer.json");
    let scene = Scene::from_json(&std::fs::read_to_string(&scene_path).unwrap()).unwrap();
    let mut instance = scene.spawn(&mut World::new()).unwrap();
    instance
        .register_scripts(load_sources(&scene, Some(&scene_path)).unwrap())
        .unwrap();
    Rules::new(
        instance.script_module("flap-player").unwrap(),
        instance.script_module("flap-round").unwrap(),
    )
    .unwrap()
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[(sorted.len() as f64 * p).ceil() as usize - 1]
}

fn report(label: &str, samples: &mut [f64]) {
    samples.sort_by(f64::total_cmp);
    let median = if samples.len().is_multiple_of(2) {
        (samples[samples.len() / 2 - 1] + samples[samples.len() / 2]) / 2.0
    } else {
        samples[samples.len() / 2]
    };
    println!(
        "{label}: n={} median={:.3} us/tick p95={:.3} us/tick p99={:.3} us/tick",
        samples.len(),
        median,
        percentile(samples, 0.95),
        percentile(samples, 0.99),
    );
}

fn main() {
    let rules = load_rules();
    let sample_count = SAMPLES * MEASURED_TICKS;
    let mut host_samples = Vec::with_capacity(sample_count);
    let mut replica_samples = Vec::with_capacity(sample_count);
    let mut presentation_samples = Vec::with_capacity(sample_count);

    // Exercise the authored Flap Woods Script Manager hooks in a headless scene.
    // NetworkFrame mirrors the data the live multiplayer presentation reads.
    let scene_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/flap-woods-multiplayer.json");
    let scene = Scene::from_json(&std::fs::read_to_string(&scene_path).unwrap()).unwrap();
    let mut scene_world = World::new();
    let mut scene_instance = scene.spawn(&mut scene_world).unwrap();
    scene_instance
        .register_scripts(load_sources(&scene, Some(&scene_path)).unwrap())
        .unwrap();
    let birds: Vec<_> = (0..4).map(|slot| rules.spawn(slot).unwrap()).collect();
    let mut objects = std::collections::BTreeMap::new();
    for bird in &birds {
        objects.insert(format!("bird-{}", bird.slot), json!(bird));
    }
    let pipes = rules.pipes().unwrap();
    for (index, pipe) in pipes.iter().enumerate() {
        objects.insert(format!("pipe-{}", index + 1), json!(pipe));
    }
    scene_world.insert_resource(NetworkFrame {
        active: true,
        objects,
        state: json!({
            "players": birds.iter().map(|bird| json!({
                "slot": bird.slot,
                "local": bird.slot == 0,
                "score": bird.score,
                "alive": bird.alive,
            })).collect::<Vec<_>>()
        }),
    });
    let mut scene_samples = Vec::with_capacity(sample_count);

    let mut noop_world = World::new();
    let mut noop_instance = scene.spawn(&mut noop_world).unwrap();
    let mut noop_sources = load_sources(&scene, Some(&scene_path)).unwrap();
    for script in ["flap-player", "flap-round", "flap-pipes"] {
        noop_sources.insert(script.into(), "fn on_update(me, dt) {}".into());
    }
    noop_instance.register_scripts(noop_sources).unwrap();
    noop_world.insert_resource(noop_world_resource(&scene_world));
    let mut noop_scene_samples = Vec::with_capacity(sample_count);

    for _ in 0..SAMPLES {
        let mut host = Host::new(PEERS[0], rules.clone()).unwrap();
        for peer in PEERS.into_iter().skip(1) {
            host.join(peer).unwrap();
        }
        host.start(PEERS[0]).unwrap();
        for _ in 0..COUNTDOWN_TICKS {
            host.step().unwrap();
        }
        assert_eq!(host.phase, Phase::Playing);

        let mut replicas: Vec<_> = PEERS
            .into_iter()
            .map(|_| Replica::new(rules.clone()))
            .collect();
        for (replica, peer) in replicas.iter_mut().zip(PEERS) {
            replica
                .apply(PEERS[0], PEERS[0], peer, host.snapshot(peer).unwrap())
                .unwrap();
        }

        for tick in 0..(WARMUP_TICKS + MEASURED_TICKS) {
            let is_warmup = tick < WARMUP_TICKS;
            // Inputs are prepared before host timing to isolate authority simulation.
            let messages: Vec<_> = replicas
                .iter_mut()
                .zip(PEERS)
                .map(|(replica, peer)| {
                    replica
                        .input((tick + peer as usize).is_multiple_of(5))
                        .unwrap();
                    (peer, replica.message())
                })
                .collect();

            let started = (!is_warmup).then(Instant::now);
            for (peer, message) in messages {
                host.receive(peer, message).unwrap();
            }
            host.step().unwrap();
            if let Some(started) = started {
                host_samples.push(started.elapsed().as_nanos() as f64 / 1_000.0);
            }

            let snapshots: Vec<_> = PEERS
                .into_iter()
                .map(|peer| host.snapshot(peer).unwrap())
                .collect();
            let started = (!is_warmup).then(Instant::now);
            for ((replica, peer), snapshot) in replicas.iter_mut().zip(PEERS).zip(snapshots) {
                replica.apply(PEERS[0], PEERS[0], peer, snapshot).unwrap();
            }
            if let Some(started) = started {
                replica_samples.push(started.elapsed().as_nanos() as f64 / 1_000.0);
            }

            let started = (!is_warmup).then(Instant::now);
            for replica in &replicas {
                for peer in PEERS {
                    black_box(replica.render_bird(peer, PEERS[0], 0.5));
                }
                black_box(replica.render_pipes(0.5));
            }
            if let Some(started) = started {
                presentation_samples.push(started.elapsed().as_nanos() as f64 / 1_000.0);
            }
        }
        black_box(host.bird(PEERS[0]));
    }

    // Warm the interpreter, populate caches and run scene hooks before collecting timings.
    scene_instance
        .step_scripts(&mut scene_world, 1.0 / 60.0, GameplayInput::default())
        .unwrap();
    for _ in 0..sample_count {
        let started = Instant::now();
        scene_instance
            .step_scripts(&mut scene_world, 1.0 / 60.0, GameplayInput::default())
            .unwrap();
        scene_samples.push(started.elapsed().as_nanos() as f64 / 1_000.0);
    }
    noop_instance
        .step_scripts(&mut noop_world, 1.0 / 60.0, GameplayInput::default())
        .unwrap();
    for _ in 0..sample_count {
        let started = Instant::now();
        noop_instance
            .step_scripts(&mut noop_world, 1.0 / 60.0, GameplayInput::default())
            .unwrap();
        noop_scene_samples.push(started.elapsed().as_nanos() as f64 / 1_000.0);
    }

    println!(
        "Flap Woods CPU benchmark: {} peers, {} ticks/path, {} warmup tick per host scenario and presentation path; Rust {} profile",
        PEERS.len(),
        sample_count,
        WARMUP_TICKS,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    report("host receive + scripted simulation", &mut host_samples);
    report(
        "replica prediction + snapshot apply/replay",
        &mut replica_samples,
    );
    report("scripted presentation accessors", &mut presentation_samples);
    report("Flap Woods scripted scene presentation", &mut scene_samples);
    report(
        "Flap Woods scene with no-op presentation scripts",
        &mut noop_scene_samples,
    );
}

fn noop_world_resource(source: &World) -> NetworkFrame {
    source
        .resource::<NetworkFrame>()
        .expect("populated network frame")
        .clone()
}
