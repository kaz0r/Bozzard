//! Reproducible, account-free Flap Woods transport run.
//! cargo run -p bozzard-network --example multiplayer_lab -- cafe-wifi 42 4
use anyhow::{Context, Result, ensure};
use bozzard_ecs::World;
use bozzard_network::{
    Message, Peer, decode, encode,
    flap::{Host, Replica},
    lab::{FaultTransport, Faults},
    rules::Rules,
};
use bozzard_scene::{Scene, load_sources};
use std::{path::Path, sync::Arc};

fn rules() -> Result<Arc<Rules>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/demo/scenes/flap-woods-multiplayer.json");
    let scene = Scene::from_json(&std::fs::read_to_string(&path)?)?;
    let mut instance = scene.spawn(&mut World::new())?;
    instance.register_scripts(load_sources(&scene, Some(&path))?)?;
    Rules::new(
        instance.script_module("flap-player")?,
        instance.script_module("flap-round")?,
    )
}

fn run(name: &str, seed: u64, peers: usize) -> Result<String> {
    ensure!((2..=4).contains(&peers), "peer count must be 2–4");
    let ids: Vec<Peer> = (0..peers).map(|i| 10 + i as Peer * 10).collect();
    let rules = rules()?;
    let mut host = Host::new(ids[0], rules.clone())?;
    for &id in ids.iter().skip(1) {
        host.join(id)?;
    }
    host.start(ids[0])?;
    for _ in 0..300 {
        host.step()?;
    }
    let mut replicas: Vec<_> = ids.iter().map(|_| Replica::new(rules.clone())).collect();
    for (&id, replica) in ids.iter().zip(&mut replicas) {
        replica.apply(ids[0], ids[0], id, host.snapshot(id)?)?;
    }
    let mut transport = FaultTransport::new(Faults::named(name, seed, ids[0])?);
    let mut event_hash = 0xcbf29ce484222325_u64;
    let mut max_history = 0;
    let mut max_age = 0;
    for tick in 0..600_u64 {
        for (&id, replica) in ids.iter().zip(&mut replicas) {
            let flap = (tick + id).is_multiple_of(13);
            replica.input(flap)?;
            if let Message::Input { ref frames, .. } = replica.message() {
                max_history = max_history.max(frames.len());
            }
            if id == ids[0] {
                host.receive(id, replica.message())?;
            } else {
                transport.send(tick, id, ids[0], encode(777, replica.message())?)?;
            }
        }
        host.step()?;
        if tick.is_multiple_of(3) {
            for &id in &ids {
                let snapshot = host.snapshot(id)?;
                if id == ids[0] {
                    replicas[0].apply(ids[0], ids[0], id, snapshot)?;
                } else {
                    transport.send(tick, ids[0], id, encode(777, Message::Snapshot(snapshot))?)?;
                }
            }
        }
        max_age = max_age.max(transport.oldest_packet_age(tick).unwrap_or(0));
        for packet in transport.receive(tick) {
            event_hash = event_hash.wrapping_mul(0x100000001b3) ^ packet.from;
            event_hash = event_hash.wrapping_mul(0x100000001b3) ^ packet.to;
            event_hash = event_hash.wrapping_mul(0x100000001b3) ^ packet.sent_tick;
            match decode(777, &packet.bytes)? {
                message @ Message::Input { .. } if packet.to == ids[0] => {
                    host.receive(packet.from, message)?;
                }
                Message::Snapshot(snapshot) if packet.from == ids[0] => {
                    let index = ids
                        .iter()
                        .position(|id| *id == packet.to)
                        .context("unknown destination")?;
                    replicas[index].apply(ids[0], ids[0], packet.to, snapshot)?;
                }
                _ => anyhow::bail!("unexpected lab packet"),
            }
        }
    }
    // The fault phase ends; deliver fresh inputs/snapshots until all histories reconcile.
    transport.clear();
    for _ in 0..150 {
        for (&id, replica) in ids.iter().zip(&replicas) {
            host.receive(id, replica.message())?;
        }
        host.step()?;
        for (&id, replica) in ids.iter().zip(&mut replicas) {
            replica.apply(ids[0], ids[0], id, host.snapshot(id)?)?;
        }
    }
    for (&id, replica) in ids.iter().zip(&replicas) {
        for &peer in &ids {
            ensure!(
                replica.birds[&peer] == host.bird(peer).context("missing bird")?,
                "peer {id} failed to converge on {peer}"
            );
        }
        ensure!(
            replica.predicted == host.bird(id),
            "peer {id} prediction did not converge"
        );
        ensure!(
            replica.pipes == Some(host.pipes),
            "peer {id} pipes did not converge"
        );
    }
    ensure!(
        max_history <= 120 && transport.peak_queue <= FaultTransport::MAX_PACKETS,
        "multiplayer buffers exceeded their limits"
    );
    Ok(format!(
        "scenario={name} seed={seed} peers={peers} event_hash={event_hash:016x} dropped={} duplicated={} peak_packets={} max_packet_age_ticks={max_age} max_input_history={max_history} convergence=ok",
        transport.dropped, transport.duplicated, transport.peak_queue,
    ))
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "cafe-wifi".into());
    let seed = args.next().unwrap_or_else(|| "42".into()).parse()?;
    let peers = args.next().unwrap_or_else(|| "4".into()).parse()?;
    ensure!(
        args.next().is_none(),
        "usage: multiplayer_lab [clean|cafe-wifi|satellite] [seed] [2..4 peers]"
    );
    println!("{}", run(&name, seed, peers)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_scenarios_reproduce_event_order_and_converge() {
        for name in ["clean", "cafe-wifi", "satellite"] {
            let first = run(name, 42, 4).unwrap();
            assert_eq!(first, run(name, 42, 4).unwrap());
        }
    }
}
