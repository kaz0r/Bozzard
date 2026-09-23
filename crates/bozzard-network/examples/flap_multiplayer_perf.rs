//! CPU-only scripted Flap Woods multiplayer benchmark.
//!
//! Run with `cargo run --release -p bozzard-network --example flap_multiplayer_perf`.
//! No renderer, GPU, Steam client, or second account is required.

use bozzard_ecs::World;
use bozzard_network::{
    Message, Peer,
    flap::{Host, Phase, Replica},
    rules::Rules,
};
use bozzard_scene::{GameplayInput, NetworkFrame, Scene, load_sources};
use serde_json::json;
use std::{collections::VecDeque, hint::black_box, path::Path, sync::Arc, time::Instant};

const PEERS: [Peer; 4] = [10, 20, 30, 40];
const COUNTDOWN_TICKS: usize = 300;
const MEASURED_TICKS: usize = 30;
const SAMPLES: usize = 200;
const WARMUP_TICKS: usize = 1;
const SNAPSHOT_DELAY_TICKS: usize = 6;
const SNAPSHOT_INTERVAL_TICKS: usize = 3;

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

fn playing_scenario(rules: Arc<Rules>) -> (Host, Vec<Replica>) {
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
        assert!(
            replica
                .apply(PEERS[0], PEERS[0], peer, host.snapshot(peer).unwrap())
                .unwrap()
        );
    }
    (host, replicas)
}

fn pending_inputs(replica: &Replica) -> usize {
    match replica.message() {
        Message::Input { frames, .. } => frames.len(),
        _ => unreachable!("replicas only send input messages"),
    }
}

fn network_frame(replica: &Replica) -> NetworkFrame {
    let objects = replica
        .birds
        .keys()
        .filter_map(|peer| replica.render_bird(*peer, PEERS[0], 0.5))
        .map(|bird| (format!("bird-{}", bird.slot), json!(bird)))
        .chain(
            replica
                .render_pipes(0.5)
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(index, pipe)| (format!("pipe-{}", index + 1), json!(pipe))),
        )
        .collect();
    let players = replica
        .birds
        .values()
        .map(|bird| {
            json!({
                "slot": bird.slot,
                "local": bird.slot == 0,
                "score": bird.score,
                "alive": bird.alive,
            })
        })
        .collect::<Vec<_>>();
    NetworkFrame {
        active: true,
        objects,
        state: json!({"players": players}),
    }
}

fn main() {
    let rules = load_rules();
    let sample_count = SAMPLES * MEASURED_TICKS;
    let mut host_samples = Vec::with_capacity(sample_count);
    let mut replica_samples = Vec::with_capacity(sample_count);
    let mut presentation_samples = Vec::with_capacity(sample_count);
    let mut delayed_replica_samples = Vec::with_capacity(sample_count);
    let mut moving_scene_samples = Vec::with_capacity(sample_count);
    let mut moving_frames = Vec::with_capacity(sample_count);

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
        let (mut host, mut replicas) = playing_scenario(rules.clone());

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
                assert!(replica.apply(PEERS[0], PEERS[0], peer, snapshot).unwrap());
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

    // Model 20 Hz snapshots delayed by six simulation ticks (~100 ms at 60 Hz).
    // Inputs are predicted each tick, while snapshots are applied in order after
    // the delay. Host simulation, snapshot production and queueing stay untimed.
    for _ in 0..SAMPLES {
        let (mut host, mut replicas) = playing_scenario(rules.clone());
        let mut in_flight = VecDeque::new();
        let mut saw_pending_replay = false;

        for tick in 0..(WARMUP_TICKS + SNAPSHOT_DELAY_TICKS + MEASURED_TICKS) {
            let is_warmup = tick < WARMUP_TICKS + SNAPSHOT_DELAY_TICKS;
            let mut elapsed_us = 0.0;

            // This path deliberately includes client-side prediction and replay
            // work caused by delayed acknowledgements.
            let started = (!is_warmup).then(Instant::now);
            let messages = replicas
                .iter_mut()
                .zip(PEERS)
                .map(|(replica, peer)| {
                    replica
                        .input((tick + peer as usize).is_multiple_of(5))
                        .unwrap();
                    (peer, replica.message())
                })
                .collect::<Vec<_>>();
            if let Some(started) = started {
                elapsed_us += started.elapsed().as_nanos() as f64 / 1_000.0;
            }
            for (peer, message) in messages {
                host.receive(peer, message).unwrap();
            }
            host.step().unwrap();
            if tick.is_multiple_of(SNAPSHOT_INTERVAL_TICKS) {
                let snapshots = PEERS
                    .into_iter()
                    .map(|peer| host.snapshot(peer).unwrap())
                    .collect::<Vec<_>>();
                in_flight.push_back((tick, snapshots));
            }
            if in_flight
                .front()
                .is_some_and(|(produced_tick, _)| tick - *produced_tick >= SNAPSHOT_DELAY_TICKS)
            {
                let (_, snapshots) = in_flight.pop_front().unwrap();
                let started = (!is_warmup).then(Instant::now);
                for ((replica, peer), snapshot) in replicas.iter_mut().zip(PEERS).zip(snapshots) {
                    assert!(replica.apply(PEERS[0], PEERS[0], peer, snapshot).unwrap());
                }
                if let Some(started) = started {
                    elapsed_us += started.elapsed().as_nanos() as f64 / 1_000.0;
                }
                // Inspect replay coverage outside timing: message() copies the
                // pending input queue for transmission.
                saw_pending_replay |= replicas.iter().any(|replica| pending_inputs(replica) > 0);
            }

            if !is_warmup {
                delayed_replica_samples.push(elapsed_us);
                moving_frames.push(network_frame(&replicas[0]));
            }
        }
        assert!(
            saw_pending_replay,
            "delayed scenario had no inputs to replay"
        );
        black_box(&replicas);
    }
    assert!(
        moving_frames
            .windows(2)
            .any(|pair| pair[0].objects != pair[1].objects),
        "delayed scenario did not produce changing presentation objects"
    );

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

    // Keep the unchanged-frame path above as the idle baseline. For this path,
    // publish evolving frames before starting each timer so only scene scripts
    // and their presentation updates are measured.
    assert_eq!(moving_frames.len(), sample_count);
    for frame in moving_frames {
        scene_world.insert_resource(frame);
        let started = Instant::now();
        scene_instance
            .step_scripts(&mut scene_world, 1.0 / 60.0, GameplayInput::default())
            .unwrap();
        moving_scene_samples.push(started.elapsed().as_nanos() as f64 / 1_000.0);
    }

    println!(
        "Flap Woods CPU benchmark: {} peers, {} ticks/path, {} warmup tick(s) for host/presentation paths, {} priming tick(s) for delayed replica path; Rust {} profile",
        PEERS.len(),
        sample_count,
        WARMUP_TICKS,
        WARMUP_TICKS + SNAPSHOT_DELAY_TICKS,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    report("host receive + scripted simulation", &mut host_samples);
    report(
        "replica snapshot apply/replay (excludes input prediction)",
        &mut replica_samples,
    );
    report(
        "replica prediction + snapshot apply/replay (6-tick snapshot delay)",
        &mut delayed_replica_samples,
    );
    report("scripted presentation accessors", &mut presentation_samples);
    report(
        "Flap Woods unchanged-frame scene presentation",
        &mut scene_samples,
    );
    report(
        "Flap Woods moving-frame scene presentation",
        &mut moving_scene_samples,
    );
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
