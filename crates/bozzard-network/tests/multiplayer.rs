use bozzard_network::{flap::*, *};
use std::{collections::BTreeMap, time::Duration};
const COUNTDOWN_TICKS: u16 = 300;
fn rules() -> std::sync::Arc<rules::Rules> {
    static RULES: std::sync::OnceLock<std::sync::Arc<rules::Rules>> = std::sync::OnceLock::new();
    RULES
        .get_or_init(|| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/demo/scenes/flap-woods-multiplayer.json");
            let scene =
                bozzard_scene::Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut instance = scene.spawn(&mut bozzard_ecs::World::new()).unwrap();
            instance
                .register_scripts(bozzard_scene::load_sources(&scene, Some(&path)).unwrap())
                .unwrap();
            rules::Rules::new(
                instance.script_module("flap-player").unwrap(),
                instance.script_module("flap-round").unwrap(),
            )
            .unwrap()
        })
        .clone()
}
fn replica() -> Replica {
    Replica::new(rules())
}
fn host() -> Host {
    let mut h = Host::new(10, rules()).unwrap();
    h.join(20).unwrap();
    h.join(30).unwrap();
    h
}
fn start_round(h: &mut Host) {
    h.start(10).unwrap();
    for _ in 0..COUNTDOWN_TICKS {
        h.step().unwrap();
    }
}
fn input(round: u64, seq: u64, flap: bool, ack: u64) -> Message {
    Message::Input {
        round,
        frames: vec![InputFrame {
            sequence: seq,
            flap,
        }],
        ack,
    }
}

#[test]
fn countdown_is_host_owned_replicated_and_freezes_everyone_for_five_seconds() {
    let mut h = host();
    h.start(10).unwrap();
    assert!(h.start(20).is_err());
    assert!(h.start(10).is_err());
    assert!(h.join(40).is_err());
    let mut replicas = [replica(), replica()];
    let pipes = h.pipes;
    let first = h.snapshot(20).unwrap();
    for tick in 0..COUNTDOWN_TICKS {
        for (replica, id) in replicas.iter_mut().zip([10, 20]) {
            let bytes = encode(777, Message::Snapshot(h.snapshot(id).unwrap())).unwrap();
            let Message::Snapshot(snapshot) = decode(777, &bytes).unwrap() else {
                panic!()
            };
            replica.apply(10, 10, id, snapshot).unwrap();
            assert_eq!(replica.phase.countdown_seconds(), Some(5 - tick / 60));
            replica.input(true).unwrap();
            let Message::Input { frames, .. } = replica.message() else {
                panic!()
            };
            assert!(frames.is_empty());
            assert_eq!(replica.predicted, h.bird(id));
        }
        h.receive(20, input(h.round, 1, true, 0)).unwrap();
        h.step().unwrap();
        assert_eq!(h.bird(20), Some(rules().spawn(1).unwrap()));
        assert_eq!(h.pipes, pipes);
    }
    assert_eq!(h.phase, Phase::Playing);
    replicas[1]
        .apply(10, 10, 20, h.snapshot(20).unwrap())
        .unwrap();
    assert!(
        !replicas[1].apply(10, 10, 20, first).unwrap(),
        "late countdown cannot restart it"
    );
    h.step().unwrap();
    assert!(
        h.bird(20).unwrap().velocity < 0.,
        "early flap was discarded"
    );
    assert_ne!(h.pipes, pipes);
    for _ in 0..300 {
        h.step().unwrap();
    }
    h.start(10).unwrap();
    assert_eq!(
        h.phase.countdown_seconds(),
        Some(5),
        "retries also count down"
    );
}

#[test]
fn countdown_cancels_when_last_guest_leaves_and_recovers_lost_snapshots() {
    let mut h = host();
    h.start(10).unwrap();
    let mut r = replica();
    r.apply(10, 10, 20, h.snapshot(20).unwrap()).unwrap();
    h.receive(20, r.message()).unwrap();
    for _ in 0..121 {
        h.step().unwrap();
    }
    let snapshot = h.snapshot(20).unwrap();
    assert!(
        snapshot.birds.is_empty(),
        "countdown travels even in empty deltas"
    );
    r.apply(10, 10, 20, snapshot).unwrap();
    assert_eq!(r.phase.countdown_seconds(), Some(3));
    h.leave(30);
    assert!(h.phase.round_active());
    h.leave(20);
    assert_eq!(h.phase, Phase::Lobby);
    h.join(40).unwrap();
    h.start(10).unwrap();
    assert_eq!(h.phase.countdown_seconds(), Some(5));
    let mut invalid = h.snapshot(40).unwrap();
    invalid.phase = Phase::Countdown { ticks_remaining: 0 };
    assert!(replica().apply(10, 10, 40, invalid).is_err());
}

#[test]
fn lobby_chat_bounds_unicode_history_and_rejects_foreign_or_invalid_payloads() {
    use bozzard_network::chat::*;
    let mut host = ChatLog::default();
    let mut guest = ChatLog::default();
    assert!(!host.receive("Guest", b"other-game: hello"));
    for index in 0..10 {
        let text = clean_text(
            &format!("{index} Héj!\n{}", "🌲".repeat(200)),
            MAX_CHAT_CHARS,
        );
        assert_eq!(text.chars().count(), MAX_CHAT_CHARS);
        let mut bytes = CHAT_PREFIX.to_vec();
        bytes.extend_from_slice(text.as_bytes());
        for log in [&mut host, &mut guest] {
            assert!(log.receive("Friend\n", &bytes));
        }
    }
    assert_eq!(host.text(), guest.text());
    assert_eq!(host.text().lines().count(), MAX_CHAT_LINES);
    assert!(host.text().starts_with("Friend: 6 Héj!"));
    for payload in [
        vec![0xff],
        vec![b'a'; MAX_CHAT_CHARS * 4 + 1],
        b" \n\t".to_vec(),
    ] {
        let mut bytes = CHAT_PREFIX.to_vec();
        bytes.extend(payload);
        assert!(!host.receive("Guest", &bytes));
    }
}

#[test]
fn only_host_starts_and_only_members_send_owned_inputs() {
    let mut h = host();
    assert!(h.start(20).is_err());
    assert_eq!(h.phase, Phase::Lobby);
    start_round(&mut h);
    assert!(h.start(10).is_err());
    assert!(h.join(40).is_err());
    assert!(h.receive(999, input(h.round, 1, true, 0)).is_err());
    let snapshot = h.snapshot(20).unwrap();
    assert!(h.receive(20, Message::Snapshot(snapshot.clone())).is_err());
    assert!(
        h.receive(20, input(h.round, 1, true, snapshot.tick + 1))
            .is_err()
    );
    h.receive(20, input(h.round, 1, true, 0)).unwrap();
    h.receive(20, input(h.round, 1, true, 0)).unwrap();
    h.step().unwrap();
    assert!(h.bird(20).unwrap().y > h.bird(10).unwrap().y);
    let velocity = h.bird(20).unwrap().velocity;
    h.receive(20, input(h.round, 1, true, 0)).unwrap();
    h.step().unwrap();
    assert!(
        h.bird(20).unwrap().velocity < velocity,
        "duplicate input must not flap twice"
    );
    let mut r = replica();
    assert!(r.apply(10, 20, 20, snapshot).is_err());
}

#[test]
fn acknowledged_deltas_recover_dropped_updates_and_remove_departed_members() {
    let mut h = host();
    let mut r = replica();
    let first = h.snapshot(20).unwrap();
    assert_eq!(first.birds.len(), 3);
    r.apply(10, 10, 20, first).unwrap();
    h.receive(20, r.message()).unwrap();
    h.step().unwrap();
    assert!(
        h.snapshot(20).unwrap().birds.is_empty(),
        "idle birds aren't resent after acknowledgement"
    );
    h.join(40).unwrap();
    let lost = h.snapshot(20).unwrap();
    assert_eq!(lost.birds.len(), 1);
    h.step().unwrap();
    let resend = h.snapshot(20).unwrap();
    assert_eq!(resend.birds.len(), 1, "unacknowledged changes survive loss");
    r.apply(10, 10, 20, resend).unwrap();
    h.receive(20, r.message()).unwrap();
    h.leave(40);
    h.step().unwrap();
    r.apply(10, 10, 20, h.snapshot(20).unwrap()).unwrap();
    assert!(!r.birds.contains_key(&40));
    assert!(
        h.snapshot(40).is_err(),
        "no replication outside the lobby interest set"
    );
    assert!(
        !r.apply(10, 10, 20, lost).unwrap(),
        "reordered stale packets cannot resurrect birds"
    );
}

#[test]
fn three_clients_converge_with_scripted_loss_latency_duplicates_and_reordering() {
    let mut h = host();
    start_round(&mut h);
    let mut clients: BTreeMap<u64, Replica> = [10, 20, 30].map(|id| (id, replica())).into();
    for (&id, replica) in &mut clients {
        replica.apply(10, 10, id, h.snapshot(id).unwrap()).unwrap();
    }
    let mut queue: Vec<(usize, u64, bool, Vec<u8>)> = Vec::new();
    let mut drops = 0;
    for tick in 0..900 {
        for (&id, replica) in &mut clients {
            // Pilot all clients through multiple pipe cycles, including a latency burst.
            let bird = replica.predicted.unwrap();
            let target = replica
                .pipes
                .unwrap()
                .iter()
                .filter(|p| p.x > bird.x() - 1.)
                .min_by(|a, b| a.x.total_cmp(&b.x))
                .map_or(0., |p| p.gap);
            replica.input(bird.y < target - 0.3).unwrap();
            if !(tick + id as usize).is_multiple_of(7) {
                let bytes = encode(777, replica.message()).unwrap();
                queue.push((tick + (tick * 3 + id as usize) % 6, id, true, bytes.clone()));
                if tick % 19 == 0 {
                    queue.push((tick + 8, id, true, bytes));
                }
            } else {
                drops += 1;
            }
        }
        h.step().unwrap();
        if tick % 3 == 0 {
            for &id in clients.keys() {
                let snapshot = h.snapshot(id).unwrap();
                if !(tick + id as usize).is_multiple_of(11) && !(200..215).contains(&tick) {
                    queue.push((
                        tick + (tick + id as usize) % 9,
                        id,
                        false,
                        encode(777, Message::Snapshot(snapshot)).unwrap(),
                    ));
                } else {
                    drops += 1;
                }
            }
        }
        let mut due = Vec::new();
        queue.retain(|packet| {
            if packet.0 <= tick {
                due.push(packet.clone());
                false
            } else {
                true
            }
        });
        due.reverse();
        for (_, id, to_host, bytes) in due {
            let message = decode(777, &bytes).unwrap();
            if to_host {
                h.receive(id, message).unwrap();
            } else if let Message::Snapshot(snapshot) = message {
                clients
                    .get_mut(&id)
                    .unwrap()
                    .apply(10, 10, id, snapshot)
                    .unwrap();
            }
        }
    }
    assert!(drops > 100);
    // Stop sending new inputs, deliver the outstanding prediction history and acknowledge it.
    for _ in 0..150 {
        for (&id, replica) in &clients {
            h.receive(id, replica.message()).unwrap();
        }
        h.step().unwrap();
        for (&id, replica) in &mut clients {
            replica.apply(10, 10, id, h.snapshot(id).unwrap()).unwrap();
        }
    }
    for (&id, replica) in &clients {
        for peer in [10, 20, 30] {
            assert_eq!(replica.birds[&peer], h.bird(peer).unwrap());
        }
        assert_eq!(
            replica.predicted.unwrap(),
            h.bird(id).unwrap(),
            "prediction reconciles exactly once outstanding inputs are acknowledged"
        );
        assert_eq!(replica.pipes.unwrap(), h.pipes);
    }
}

#[test]
fn reset_rejects_old_round_and_bounds_prediction_and_input_queues() {
    let mut h = host();
    start_round(&mut h);
    let mut r = replica();
    r.apply(10, 10, 20, h.snapshot(20).unwrap()).unwrap();
    for _ in 0..500 {
        r.input(true).unwrap();
    }
    if let Message::Input { frames, .. } = r.message() {
        assert_eq!(frames.len(), 120);
    }
    let stale = r.message();
    for _ in 0..300 {
        h.step().unwrap();
    }
    assert_eq!(h.phase, Phase::Finished);
    assert!(h.start(20).is_err());
    start_round(&mut h);
    assert!(h.receive(20, stale).is_err());
    r.apply(10, 10, 20, h.snapshot(20).unwrap()).unwrap();
    assert_eq!(r.predicted.unwrap().y, 0.65);
    if let Message::Input { frames, .. } = r.message() {
        assert!(frames.is_empty());
    }
    assert!(
        h.receive(
            20,
            Message::Input {
                round: h.round,
                frames: vec![
                    InputFrame {
                        sequence: 1,
                        flap: true
                    };
                    121
                ],
                ack: 0
            }
        )
        .is_err()
    );
}

#[test]
fn packets_and_pacing_are_bounded() {
    let bytes = encode(777, Message::Goodbye).unwrap();
    assert!(decode(778, &bytes).is_err());
    assert!(decode(777, &vec![b' '; MAX_PACKET + 1]).is_err());
    assert!(decode(777, br#"{"protocol":999,"lobby":777,"message":"Goodbye"}"#).is_err());
    let mut p = Pacer::default();
    assert_eq!(p.advance(STEP * 100 + STEP / 2), 8);
    assert_eq!(p.dropped, STEP * 92);
    assert_eq!(p.advance(STEP - STEP / 2), 1);
    assert_eq!(p.advance(Duration::ZERO), 0);
}

#[test]
fn collision_eliminates_only_one_bird_and_scoring_is_host_owned() {
    let mut h = host();
    start_round(&mut h);
    let mut seq = 0;
    for _ in 0..1800 {
        seq += 1;
        for id in [10, 20] {
            let bird = h.bird(id).unwrap();
            let target = h
                .pipes
                .iter()
                .filter(|p| p.x > bird.x() - 1.)
                .min_by(|a, b| a.x.total_cmp(&b.x))
                .map_or(0., |p| p.gap);
            h.receive(id, input(h.round, seq, bird.y < target - 0.3, 0))
                .unwrap();
        }
        h.step().unwrap();
    }
    assert!(!h.bird(30).unwrap().alive);
    assert!(h.bird(20).unwrap().alive);
    assert!(h.bird(20).unwrap().score >= 8);
    assert_eq!(h.phase, Phase::Playing);
}
