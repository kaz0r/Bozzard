//! Behavioral coverage for the Flapwoods game scene: flap physics, death and
//! respawn, pipe drift, and the score beacon/marker driven by blueprints.
use bozzard_demo::SceneDemo;
use bozzard_scene::{GameplayInput, Light, Transform};

fn load() -> SceneDemo {
    let json = std::fs::read_to_string("scenes/flap-woods.json").unwrap();
    let doc = bozzard_scene::Scene::from_json(&json).unwrap();
    SceneDemo::new(&doc).unwrap()
}

fn step(d: &mut SceneDemo) {
    d.app.step();
    d.check_simulation().unwrap();
}

fn position(d: &SceneDemo, id: &str) -> [f32; 3] {
    d.app
        .world
        .get::<Transform>(d.instance().entity(id).unwrap())
        .unwrap()
        .translation
}

fn light(d: &SceneDemo, id: &str) -> f32 {
    d.app
        .world
        .get::<Light>(d.instance().entity(id).unwrap())
        .unwrap()
        .intensity
}

#[test]
fn bird_falls_flaps_and_respawns() {
    let mut d = load();
    step(&mut d);
    let y0 = position(&d, "bird")[1];

    // flap immediately while still near the spawn height
    d.set_gameplay_input(GameplayInput {
        movement: [0.; 2],
        jump: true,
        orbit: [0.; 2],
    });
    step(&mut d);
    d.set_gameplay_input(Default::default());
    let mut peak = f32::MIN;
    for _ in 0..40 {
        step(&mut d);
        peak = peak.max(position(&d, "bird")[1]);
    }
    assert!(peak > y0 + 0.5, "flap should climb: peak {peak} vs {y0}");

    // without further flaps the bird must fall, die at the floor and respawn
    let mut saw_low = false;
    let mut respawned = false;
    for _ in 0..900 {
        step(&mut d);
        let y = position(&d, "bird")[1];
        if y < -2.0 {
            saw_low = true;
        }
        if saw_low && y > 0.6 {
            respawned = true;
            break;
        }
    }
    assert!(
        saw_low && respawned,
        "bird must die and respawn: low={saw_low} respawn={respawned}"
    );
}

#[test]
fn bird_dies_on_pipe_contact() {
    let mut d = load();
    step(&mut d);
    let parent_x = position(&d, "pipe-1")[0];
    let e = d.instance().entity("pipe-1-bottom").unwrap();
    d.app.world.get_mut::<Transform>(e).unwrap().translation = [-5.0 - parent_x, 0.65, 0.0];
    let mut saw_low = false;
    let mut respawned = false;
    for _ in 0..900 {
        step(&mut d);
        let y = position(&d, "bird")[1];
        if y < -2.0 {
            saw_low = true;
        }
        if saw_low && y > 0.6 {
            respawned = true;
            break;
        }
    }
    assert!(
        saw_low && respawned,
        "pipe contact must kill and respawn: {saw_low} {respawned}"
    );
}

#[test]
fn pipes_drift_left() {
    let mut d = load();
    step(&mut d);
    let x0 = position(&d, "pipe-1")[0];
    for _ in 0..60 {
        step(&mut d);
    }
    let x1 = position(&d, "pipe-1")[0];
    assert!(x1 < x0 - 1.5, "pipes drift left: {x1} vs {x0}");
}

#[test]
fn scoring_counts_and_resets_on_bird_exit() {
    let mut d = load();
    step(&mut d);
    let parent_x = position(&d, "pipe-1")[0];
    let e = d.instance().entity("pipe-1-bottom").unwrap();
    d.app.world.get_mut::<Transform>(e).unwrap().translation = [-5.0 - parent_x, 5.0, 0.0];
    step(&mut d);
    let beacon = light(&d, "beacon");
    let cube = position(&d, "score-cube")[0];
    assert!(
        (beacon - 3.0).abs() < 1e-4,
        "one gate = intensity 3, got {beacon}"
    );
    assert!(
        (cube - (-3.6)).abs() < 1e-3,
        "marker should slide, at {cube}"
    );

    let e = d.instance().entity("bird-solid").unwrap();
    d.app.world.get_mut::<Transform>(e).unwrap().translation[1] = -30.0;
    step(&mut d);
    let beacon = light(&d, "beacon");
    assert!(
        beacon.abs() < 1e-4,
        "score resets on bird exit, got {beacon}"
    );
}

#[test]
fn idle_bird_loops_death_and_respawn() {
    let mut d = load();
    let mut deaths = 0;
    let mut was_low = false;
    for _ in 0..1500 {
        step(&mut d);
        let y = position(&d, "bird")[1];
        if y < -2.0 {
            was_low = true;
        } else if was_low && y > 0.6 {
            deaths += 1;
            was_low = false;
        }
    }
    assert!(
        deaths >= 2,
        "unattended bird should keep dying/respawning, saw {deaths}"
    );
}
