//! End-to-end checks for Sand Island (`examples/sand-island`): first-person movement on the
//! voxel island, the eight-times day clock, the ship sailing away, the settings menu, and
//! the game: gathering, crafting at camp, building the raft and sailing it to the lighthouse.
//! These tests play the scene headless and without a GPU.
use bozzard_runtime::SceneRuntime;
use bozzard_scene::{
    BlueprintRuntime, Camera, GameplayInput, Layer, Scene, Transform,
    blueprint::{BlackboardValue, Value},
    keys,
    middleware::ui::Input,
};
use std::{collections::BTreeMap, path::PathBuf};

const EYE: f32 = 1.62;
/// Degrees of yaw per unit of mouse motion at sensitivity 1.
const TURN: f32 = 0.1;
const PALM: [f32; 2] = [3.0, -2.0];

fn load() -> (Scene, PathBuf) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/sand-island/scenes/island.json");
    let scene = bozzard_runtime::load_document(Some(&path)).unwrap();
    assert_eq!(
        Scene::from_json(&scene.to_json().unwrap()).unwrap(),
        scene,
        "the scene must survive a serialization round trip"
    );
    (scene, path)
}

const SIZE: [f32; 2] = [1280., 720.];

/// The scene as it opens, on its title screen.
fn title() -> SceneRuntime {
    let (scene, path) = load();
    SceneRuntime::new_with_prefabs(&scene, Some(&path)).unwrap()
}

/// The scene after Start: the castaway on the island, in control.
fn island() -> SceneRuntime {
    let mut demo = title();
    tick(&mut demo, GameplayInput::default());
    key(&mut demo, "Enter");
    run(&mut demo, 2, GameplayInput::default());
    assert_eq!(text(&demo, None, "mode"), "island");
    demo
}

/// A key the UI owns (Escape, C, Enter): consumed by the widget whose shortcut it is.
fn key(demo: &mut SceneRuntime, name: &str) {
    assert!(
        demo.ui_input(Layer::ThreeD, SIZE, Input::Key(name.into()))
            .unwrap(),
        "no visible widget took {name}"
    );
}

/// The visible button Escape would press. The native player quits on an Escape that
/// no widget takes, so every state of the game must have one.
fn escape_owner(demo: &SceneRuntime) -> String {
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, Layer::ThreeD, SIZE)
        .unwrap();
    let owners: Vec<_> = frame
        .focusable()
        .into_iter()
        .filter(|e| e.widget.shortcuts.iter().any(|s| s == "Escape"))
        .map(|e| e.owner.clone())
        .collect();
    assert_eq!(owners.len(), 1, "Escape owners: {owners:?}");
    owners[0].clone()
}

/// Set scene variables directly, as a debugger would.
fn patch(demo: &mut SceneRuntime, values: &[(&str, Value)]) {
    let scene = values
        .iter()
        .map(|(name, value)| (name.to_string(), BlackboardValue::Scalar(value.clone())))
        .collect();
    demo.app
        .world
        .resource_mut::<BlueprintRuntime>()
        .unwrap()
        .patch_blackboards(&scene, &BTreeMap::new())
        .unwrap();
}

/// The island's height function, as `scripts/island.rhai` and the voxelizer evaluate it.
fn ground(x: f32, z: f32) -> f32 {
    let a = z.atan2(x);
    let coast =
        16.5 + 2.2 * (2. * a + 0.7).sin() + 1.4 * (3. * a + 2.1).sin() + 0.7 * (5. * a + 0.3).sin();
    let s = (x * x + z * z).sqrt() / coast;
    if s < 1. {
        let dunes =
            0.32 * (x * 0.45 + 0.3).sin() * (z * 0.38 - 0.8).cos() + 0.16 * ((x + z) * 0.9).sin();
        2.3 * (1. - s * s) + dunes * (1. - s)
    } else {
        let t = ((s - 1.35) / (1.9 - 1.35)).clamp(0., 1.);
        -1.6 * (1. - (-(s - 1.) * 4.).exp()) - 3.5 * t * t * (3. - 2. * t)
    }
}

fn board(demo: &SceneRuntime, owner: Option<&str>, name: &str) -> Value {
    let runtime = demo.app.world.resource::<BlueprintRuntime>().unwrap();
    let board = match owner {
        Some(id) => runtime.object_blackboard(id).unwrap(),
        None => runtime.scene_blackboard(),
    };
    match &board[name] {
        BlackboardValue::Scalar(value) => value.clone(),
        other => panic!("{name} is not a scalar: {other:?}"),
    }
}

fn number(demo: &SceneRuntime, owner: Option<&str>, name: &str) -> f32 {
    match board(demo, owner, name) {
        Value::Number(n) => n,
        other => panic!("{name} is not a number: {other:?}"),
    }
}

fn text(demo: &SceneRuntime, owner: Option<&str>, name: &str) -> String {
    match board(demo, owner, name) {
        Value::Text(text) => text,
        other => panic!("{name} is not text: {other:?}"),
    }
}

fn flag(demo: &SceneRuntime, owner: Option<&str>, name: &str) -> bool {
    match board(demo, owner, name) {
        Value::Bool(b) => b,
        other => panic!("{name} is not a bool: {other:?}"),
    }
}

fn transform(demo: &SceneRuntime, id: &str) -> Transform {
    *demo
        .app
        .world
        .get::<Transform>(demo.instance().entity(id).unwrap())
        .unwrap()
}

fn feet(demo: &SceneRuntime) -> [f32; 3] {
    ["px", "py", "pz"].map(|name| number(demo, Some("player"), name))
}

fn tick(demo: &mut SceneRuntime, input: GameplayInput) {
    demo.set_gameplay_input(input);
    demo.app.step();
    demo.check_simulation().unwrap();
}

fn run(demo: &mut SceneRuntime, ticks: usize, input: GameplayInput) {
    for _ in 0..ticks {
        tick(demo, input);
    }
}

fn walk(forward: f32, right: f32) -> GameplayInput {
    GameplayInput {
        movement: [right, forward],
        ..Default::default()
    }
}

/// Turn with relative mouse motion, as a player would, until facing `target`.
fn face(demo: &mut SceneRuntime, target: [f32; 2]) {
    let p = feet(demo);
    let want = (-(target[0] - p[0]))
        .atan2(-(target[1] - p[2]))
        .to_degrees();
    let yaw = number(demo, Some("player"), "yaw");
    let mut delta = (want - yaw).rem_euclid(360.);
    if delta > 180. {
        delta -= 360.;
    }
    tick(
        demo,
        GameplayInput {
            orbit: [-delta / TURN, 0.],
            ..Default::default()
        },
    );
}

fn press(key: &str) -> GameplayInput {
    GameplayInput {
        keys: keys::bit(key),
        pressed_keys: keys::bit(key),
        ..Default::default()
    }
}

#[test]
fn the_castaway_stands_on_the_sand_the_voxels_were_cut_from() {
    let mut demo = island();
    run(&mut demo, 30, GameplayInput::default());
    let p = feet(&demo);
    assert!(flag(&demo, Some("player"), "grounded"));
    assert!((p[1] - ground(p[0], p[2])).abs() < 1e-3, "{p:?}");
    let eye = transform(&demo, "camera").translation;
    assert!((eye[1] - p[1] - EYE).abs() < 1e-3, "{eye:?} {p:?}");
    // Standing still is still: no creep, no jitter.
    run(&mut demo, 120, GameplayInput::default());
    assert_eq!(feet(&demo), p);
}

#[test]
fn walking_eases_in_and_out_follows_the_slope_and_sprints_faster() {
    let mut demo = island();
    run(&mut demo, 10, GameplayInput::default());
    face(&mut demo, PALM);
    let start = feet(&demo);
    // Speed builds up over a few ticks instead of jumping to full.
    tick(&mut demo, walk(1., 0.));
    let first = number(&demo, Some("player"), "vx").hypot(number(&demo, Some("player"), "vz"));
    assert!(first > 0. && first < 1.5, "{first}");
    run(&mut demo, 59, walk(1., 0.));
    let speed = number(&demo, Some("player"), "vx").hypot(number(&demo, Some("player"), "vz"));
    assert!((speed - 3.4).abs() < 0.2, "walking speed {speed}");
    let p = feet(&demo);
    let moved = (p[0] - start[0]).hypot(p[2] - start[2]);
    assert!((2.6..3.4).contains(&moved), "moved {moved} m in a second");
    // Straight at the palm, and still on the (rising) sand.
    let toward = [PALM[0] - start[0], PALM[1] - start[2]];
    let cos = ((p[0] - start[0]) * toward[0] + (p[2] - start[2]) * toward[1])
        / (moved * toward[0].hypot(toward[1]));
    assert!(cos > 0.999, "{cos}");
    assert!((p[1] - ground(p[0], p[2])).abs() < 1e-3);
    // Letting go glides to a stop rather than snapping.
    tick(&mut demo, GameplayInput::default());
    let coasting = number(&demo, Some("player"), "vx").hypot(number(&demo, Some("player"), "vz"));
    assert!(coasting > 1. && coasting < speed, "{coasting}");
    run(&mut demo, 60, GameplayInput::default());
    let stopped = feet(&demo);
    run(&mut demo, 30, GameplayInput::default());
    let rest = feet(&demo);
    assert!((stopped[0] - rest[0]).hypot(stopped[2] - rest[2]) < 1e-3);
    // Shift sprints.
    face(&mut demo, [-20., 0.]);
    run(
        &mut demo,
        60,
        GameplayInput {
            keys: keys::bit("Shift"),
            ..walk(1., 0.)
        },
    );
    let sprint = number(&demo, Some("player"), "vx").hypot(number(&demo, Some("player"), "vz"));
    assert!(sprint > 5.0, "sprint speed {sprint}");
}

#[test]
fn jumps_land_back_on_the_sand_and_the_palm_trunk_blocks() {
    let mut demo = island();
    run(&mut demo, 10, GameplayInput::default());
    let before = feet(&demo);
    tick(
        &mut demo,
        GameplayInput {
            jump: true,
            ..Default::default()
        },
    );
    let mut peak = before[1];
    for _ in 0..90 {
        tick(&mut demo, GameplayInput::default());
        peak = peak.max(feet(&demo)[1]);
    }
    assert!(
        (0.85..1.1).contains(&(peak - before[1])),
        "jump height {}",
        peak - before[1]
    );
    assert_eq!(feet(&demo)[1], before[1]);
    assert!(flag(&demo, Some("player"), "grounded"));
    // Walk into the trunk for five seconds: the castaway goes around, never through.
    face(&mut demo, PALM);
    for _ in 0..300 {
        tick(&mut demo, walk(1., 0.));
        let p = feet(&demo);
        assert!((p[0] - PALM[0]).hypot(p[2] - PALM[1]) >= 0.659, "{p:?}");
    }
}

#[test]
fn the_current_turns_a_swimmer_back_before_the_open_sea() {
    let mut demo = island();
    run(&mut demo, 10, GameplayInput::default());
    face(&mut demo, [-40., 40.]);
    let mut swam = false;
    for _ in 0..60 * 30 {
        tick(&mut demo, walk(1., 0.));
        let p = feet(&demo);
        assert!(p[0].hypot(p[2]) <= 33.001, "{p:?}");
        if ground(p[0], p[2]) < -1.25 {
            swam = true;
            // Floating: the eye stays above the waves.
            assert!(transform(&demo, "camera").translation[1] > 0.1);
        }
    }
    assert!(swam, "the walk never reached deep water");
    match board(&demo, None, "toast") {
        Value::Text(text) => assert!(text.contains("current"), "{text}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_real_second_is_eight_game_seconds_and_t_speeds_time_up() {
    let mut demo = island();
    tick(&mut demo, GameplayInput::default());
    let seconds = |demo: &SceneRuntime| {
        number(demo, None, "clock") as f64 + number(demo, None, "clock_fraction") as f64
    };
    let start = seconds(&demo);
    // Three real minutes of play: still eight game seconds to each one.
    run(&mut demo, 60 * 180, GameplayInput::default());
    let elapsed = seconds(&demo) - start;
    assert!((elapsed - 1440.).abs() < 0.05, "{elapsed} game seconds");
    assert!((number(&demo, None, "time_of_day") as f64 - seconds(&demo) / 3600.).abs() < 1e-4);
    let before = seconds(&demo);
    run(
        &mut demo,
        60,
        GameplayInput {
            keys: keys::bit("T"),
            ..Default::default()
        },
    );
    let fast = seconds(&demo) - before;
    assert!((fast - 720.).abs() < 0.1, "{fast}");
    // The ship keeps steaming away and its smoke stays by its funnel.
    let ship = transform(&demo, "b-ship").translation;
    run(&mut demo, 600, GameplayInput::default());
    let later = transform(&demo, "b-ship").translation;
    let far = |p: [f32; 3]| p[0].hypot(p[2]);
    assert!(far(later) - far(ship) > 15., "{ship:?} -> {later:?}");
    assert!(far(later) > 1000.);
    for id in ["smoke-00", "smoke-15", "smoke-31"] {
        let puff = transform(&demo, id).translation;
        assert!(
            (puff[0] - later[0]).hypot(puff[2] - later[2]) < 120. && puff[1] > 20.,
            "{id} {puff:?} ship {later:?}"
        );
    }
}

#[test]
fn escape_opens_settings_that_pause_time_and_change_the_controls() {
    let mut demo = island();
    let size = [1280., 720.];
    run(&mut demo, 5, GameplayInput::default());
    // Escape belongs to the Settings button while playing, so the player never quits on it.
    assert!(
        demo.ui_input(Layer::ThreeD, size, Input::Key("Escape".into()))
            .unwrap()
    );
    tick(&mut demo, GameplayInput::default());
    assert!(flag(&demo, None, "menu_open"));
    tick(&mut demo, GameplayInput::default());
    assert_eq!(escape_owner(&demo), "ui-resume");
    let hours = number(&demo, None, "time_of_day");
    let p = feet(&demo);
    run(&mut demo, 60, walk(1., 0.));
    assert_eq!(
        number(&demo, None, "time_of_day"),
        hours,
        "time stands still"
    );
    assert_eq!(feet(&demo), p, "no walking behind the menu");
    // Drag the sensitivity and field-of-view sliders.
    for (owner, value) in [("ui-opt-sensitivity", 2.0), ("ui-opt-fov", 95.)] {
        assert!(
            demo.ui_input(
                Layer::ThreeD,
                size,
                Input::SetValue {
                    owner: owner.into(),
                    value,
                },
            )
            .unwrap()
        );
    }
    run(&mut demo, 2, GameplayInput::default());
    assert_eq!(number(&demo, None, "sensitivity"), 2.);
    assert_eq!(number(&demo, None, "fov"), 95.);
    // Resume with Escape again; the mouse now turns twice as far.
    assert!(
        demo.ui_input(Layer::ThreeD, size, Input::Key("Escape".into()))
            .unwrap()
    );
    tick(&mut demo, GameplayInput::default());
    assert!(!flag(&demo, None, "menu_open"));
    let yaw = number(&demo, Some("player"), "yaw");
    tick(
        &mut demo,
        GameplayInput {
            orbit: [10., 0.],
            ..Default::default()
        },
    );
    let turned = yaw - number(&demo, Some("player"), "yaw");
    assert!((turned - 10. * TURN * 2.).abs() < 1e-3, "{turned}");
    run(&mut demo, 120, GameplayInput::default());
    match *demo
        .app
        .world
        .get::<Camera>(demo.instance().entity("camera").unwrap())
        .unwrap()
    {
        Camera::Perspective {
            vertical_fov_degrees,
            ..
        } => assert!(
            (vertical_fov_degrees - 95.).abs() < 0.1,
            "{vertical_fov_degrees}"
        ),
        other => panic!("{other:?}"),
    }
    // Toggles flip on activation, as a click or Space would.
    assert!(
        demo.ui_input(Layer::ThreeD, size, Input::Key("Escape".into()))
            .unwrap()
    );
    tick(&mut demo, GameplayInput::default());
    for owner in ["ui-opt-invert", "ui-opt-bob"] {
        assert!(
            demo.ui_input(Layer::ThreeD, size, Input::ActivateObject(owner.into()))
                .unwrap()
        );
    }
    tick(&mut demo, GameplayInput::default());
    assert!(flag(&demo, None, "invert_y"));
    assert!(!flag(&demo, None, "bobbing"));
    // Keys pressed behind the open menu do not jump.
    let p = feet(&demo);
    run(&mut demo, 30, press("Space"));
    assert_eq!(feet(&demo), p);
}

/// Turn the eye toward a world point with relative mouse motion.
fn look_at(demo: &mut SceneRuntime, point: [f32; 3]) {
    let eye = transform(demo, "camera").translation;
    let d = [point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]];
    let want_yaw = (-d[0]).atan2(-d[2]).to_degrees();
    let want_pitch = d[1].atan2(d[0].hypot(d[2])).to_degrees();
    let yaw = number(demo, Some("player"), "yaw");
    let pitch = number(demo, Some("player"), "pitch");
    let mut delta = (want_yaw - yaw).rem_euclid(360.);
    if delta > 180. {
        delta -= 360.;
    }
    tick(
        demo,
        GameplayInput {
            orbit: [-delta / TURN, -(want_pitch - pitch) / TURN],
            ..Default::default()
        },
    );
}

/// Walk up to a point on the island until within `stop` metres of it.
fn go_to(demo: &mut SceneRuntime, target: [f32; 2], stop: f32) {
    for _ in 0..60 * 30 {
        let p = feet(demo);
        if (target[0] - p[0]).hypot(target[1] - p[2]) < stop {
            run(demo, 40, GameplayInput::default());
            return;
        }
        face(demo, target);
        tick(demo, walk(1., 0.));
    }
    panic!("never reached {target:?} from {:?}", feet(demo));
}

fn press_e(demo: &mut SceneRuntime) {
    tick(
        demo,
        GameplayInput {
            interact: true,
            ..Default::default()
        },
    );
    tick(demo, GameplayInput::default());
}

fn count(demo: &SceneRuntime, item: &str) -> f32 {
    number(demo, None, item)
}

fn site() -> [f32; 5] {
    let library = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/sand-island/scenes/scripts/island.rhai"),
    )
    .unwrap();
    let line = library
        .lines()
        .find(|line| line.starts_with("const SITE = ["))
        .unwrap();
    let values: Vec<f32> = line["const SITE = [".len()..line.len() - 2]
        .split(", ")
        .map(|v| v.parse().unwrap())
        .collect();
    values.try_into().unwrap()
}

#[test]
fn the_title_screen_waits_for_start() {
    let mut demo = title();
    run(&mut demo, 30, walk(1., 0.));
    assert_eq!(text(&demo, None, "mode"), "title");
    assert!(!flag(&demo, None, "control"));
    let p = feet(&demo);
    let clock = number(&demo, None, "clock");
    run(&mut demo, 60, walk(1., 0.));
    assert_eq!(feet(&demo), p, "no walking on the title screen");
    assert_eq!(
        number(&demo, None, "clock"),
        clock,
        "time waits for the player"
    );
    // Escape on the title screen is the Quit button's, Enter is Start's.
    assert_eq!(escape_owner(&demo), "ui-title-quit");
    key(&mut demo, "Enter");
    run(&mut demo, 2, GameplayInput::default());
    assert_eq!(text(&demo, None, "mode"), "island");
    assert!(flag(&demo, None, "control"));
    assert!(text(&demo, None, "toast").contains("wake up"));
    assert_eq!(escape_owner(&demo), "ui-settings-button");
}

#[test]
fn driftwood_fronds_and_coconuts_are_gathered_with_e() {
    let mut demo = island();
    // The nearest piece of driftwood on the beach.
    let p = feet(&demo);
    let wood = (0..6)
        .map(|i| (i, transform(&demo, &format!("wood-{i}")).translation))
        .filter(|(_, at)| at[1] > -100.)
        .min_by(|a, b| {
            let d = |at: [f32; 3]| (at[0] - p[0]).hypot(at[2] - p[2]);
            d(a.1).total_cmp(&d(b.1))
        })
        .unwrap();
    go_to(&mut demo, [wood.1[0], wood.1[2]], 1.3);
    look_at(&mut demo, [wood.1[0], wood.1[1] + 0.25, wood.1[2]]);
    tick(&mut demo, GameplayInput::default());
    assert!(
        text(&demo, None, "prompt").contains("driftwood"),
        "{}",
        text(&demo, None, "prompt")
    );
    press_e(&mut demo);
    assert_eq!(count(&demo, "wood"), 1.);
    assert!(
        transform(&demo, &format!("wood-{}", wood.0)).translation[1] < -100.,
        "picked up"
    );
    // The palm gives two fronds a pull.
    go_to(&mut demo, PALM, 1.4);
    let eye = transform(&demo, "camera").translation;
    look_at(&mut demo, [PALM[0], eye[1], PALM[1]]);
    tick(&mut demo, GameplayInput::default());
    assert!(
        text(&demo, None, "prompt").contains("fronds"),
        "{}",
        text(&demo, None, "prompt")
    );
    for pulls in 1..=3 {
        press_e(&mut demo);
        assert_eq!(count(&demo, "fronds"), 2. * pulls as f32);
    }
    assert_eq!(number(&demo, Some("game"), "palm_stock"), 7.);
    // And a coconut from the sand under it.
    let nut = transform(&demo, "nut-0").translation;
    go_to(&mut demo, [nut[0], nut[2]], 1.2);
    look_at(&mut demo, [nut[0], nut[1] + 0.2, nut[2]]);
    press_e(&mut demo);
    assert!(
        count(&demo, "coconuts") >= 1.,
        "{}",
        count(&demo, "coconuts")
    );
}

#[test]
fn camp_crafts_rope_a_paddle_and_a_sail_and_feeds_the_castaway() {
    let mut demo = island();
    patch(
        &mut demo,
        &[
            ("fronds", Value::Number(9.)),
            ("wood", Value::Number(2.)),
            ("coconuts", Value::Number(1.)),
            ("energy", Value::Number(50.)),
        ],
    );
    tick(&mut demo, GameplayInput::default());
    key(&mut demo, "C");
    run(&mut demo, 2, GameplayInput::default());
    assert!(flag(&demo, None, "camp_open"));
    assert!(
        !flag(&demo, None, "control"),
        "the castaway stands still at camp"
    );
    assert_eq!(escape_owner(&demo), "ui-camp-close");
    let activate = |demo: &mut SceneRuntime, id: &str| {
        let done = demo
            .ui_input(Layer::ThreeD, SIZE, Input::ActivateObject(id.into()))
            .unwrap();
        run(demo, 2, GameplayInput::default());
        done
    };
    // A paddle needs rope first: its button is greyed out.
    assert!(!activate(&mut demo, "ui-craft-paddle"));
    for _ in 0..3 {
        assert!(activate(&mut demo, "ui-craft-rope"));
    }
    assert_eq!((count(&demo, "rope"), count(&demo, "fronds")), (3., 3.));
    assert!(activate(&mut demo, "ui-craft-paddle"));
    assert_eq!(
        (
            count(&demo, "paddle"),
            count(&demo, "wood"),
            count(&demo, "rope")
        ),
        (1., 0., 2.)
    );
    assert!(
        !activate(&mut demo, "ui-craft-sail"),
        "four fronds needed, three left"
    );
    patch(&mut demo, &[("fronds", Value::Number(4.))]);
    tick(&mut demo, GameplayInput::default());
    assert!(activate(&mut demo, "ui-craft-sail"));
    assert_eq!((count(&demo, "sail"), count(&demo, "rope")), (1., 0.));
    assert!(activate(&mut demo, "ui-eat"));
    assert_eq!(count(&demo, "coconuts"), 0.);
    assert!((count(&demo, "energy") - 80.).abs() < 0.1);
    key(&mut demo, "C");
    run(&mut demo, 2, GameplayInput::default());
    assert!(!flag(&demo, None, "camp_open"));
    assert!(flag(&demo, None, "control"));
}

#[test]
fn the_raft_is_built_launched_and_sailed_to_the_lighthouse() {
    let mut demo = island();
    patch(
        &mut demo,
        &[
            ("wood", Value::Number(6.)),
            ("rope", Value::Number(4.)),
            ("sail", Value::Number(1.)),
            ("paddle", Value::Number(1.)),
            ("coconuts", Value::Number(2.)),
            ("energy", Value::Number(100.)),
        ],
    );
    let [x, y, z, ..] = site();
    go_to(&mut demo, [x, z], 2.6);
    for stage in 1..=5 {
        look_at(&mut demo, [x, y + 0.4, z]);
        tick(&mut demo, GameplayInput::default());
        assert!(
            text(&demo, None, "prompt").starts_with("E"),
            "{}",
            text(&demo, None, "prompt")
        );
        press_e(&mut demo);
        assert_eq!(number(&demo, None, "raft_stage"), stage as f32);
        assert_eq!(
            transform(&demo, &format!("raft-stage-{stage}")).translation,
            [0., 0., 0.]
        );
    }
    for item in ["wood", "rope", "sail", "paddle", "coconuts"] {
        assert_eq!(count(&demo, item), 0., "{item}");
    }
    // Launching takes a second press: there is no way back.
    press_e(&mut demo);
    assert_eq!(text(&demo, None, "mode"), "island");
    assert!(text(&demo, None, "prompt").contains("no coming back"));
    press_e(&mut demo);
    assert_eq!(text(&demo, None, "mode"), "launch");
    tick(&mut demo, GameplayInput::default());
    assert_eq!(escape_owner(&demo), "ui-settings-button");
    run(&mut demo, 60 * 4, GameplayInput::default());
    assert_eq!(text(&demo, None, "mode"), "raft");
    // Steer for the lighthouse.
    let light = [-505.0_f32, 485.0];
    for _ in 0..60 * 150 {
        if text(&demo, None, "mode") == "ended" {
            break;
        }
        let rx = number(&demo, Some("player"), "raft_x");
        let rz = number(&demo, Some("player"), "raft_z");
        let heading = number(&demo, Some("player"), "raft_yaw");
        let want = (-(light[0] - rx)).atan2(-(light[1] - rz)).to_degrees();
        let mut error = (want - heading).rem_euclid(360.);
        if error > 180. {
            error -= 360.;
        }
        tick(&mut demo, walk(1., (-error / 15.).clamp(-1., 1.)));
    }
    assert_eq!(
        text(&demo, None, "mode"),
        "ended",
        "never reached the lighthouse"
    );
    let rx = number(&demo, Some("player"), "raft_x");
    let rz = number(&demo, Some("player"), "raft_z");
    assert!((rx - light[0]).hypot(rz - light[1]) < 76.);
    let raft = transform(&demo, "raft").translation;
    assert!((raft[0] - rx).abs() < 1e-3 && raft[1] < 0.3, "{raft:?}");
    assert!(!flag(&demo, None, "control"));
    tick(&mut demo, GameplayInput::default());
    assert_eq!(escape_owner(&demo), "ui-end-quit");
    // Play again starts over.
    key(&mut demo, "Enter");
    run(&mut demo, 3, GameplayInput::default());
    assert_eq!(text(&demo, None, "mode"), "title");
}

#[test]
fn running_out_of_energy_means_waking_up_the_next_morning() {
    let mut demo = island();
    patch(&mut demo, &[("energy", Value::Number(0.05))]);
    run(&mut demo, 60 * 4, walk(1., 0.));
    assert_eq!(number(&demo, None, "day"), 2.);
    assert!((number(&demo, None, "time_of_day") - 7.).abs() < 0.05);
    assert!(count(&demo, "energy") > 40.);
    let p = feet(&demo);
    assert!(
        (p[0] - 1.6).hypot(p[2] + 0.9) < 0.5,
        "woke up in the shade: {p:?}"
    );
    assert!(text(&demo, None, "toast").contains("collapsed"));
}

#[test]
fn resting_at_camp_skips_to_dusk_and_restores_energy() {
    let mut demo = island();
    patch(&mut demo, &[("energy", Value::Number(30.))]);
    tick(&mut demo, GameplayInput::default());
    key(&mut demo, "C");
    run(&mut demo, 2, GameplayInput::default());
    assert!(
        demo.ui_input(Layer::ThreeD, SIZE, Input::ActivateObject("ui-rest".into()))
            .unwrap()
    );
    let ship = number(&demo, None, "world_time");
    run(&mut demo, 60, GameplayInput::default());
    assert_eq!(
        escape_owner(&demo),
        "ui-settings-button",
        "Escape during the fade"
    );
    run(&mut demo, 60 * 3, GameplayInput::default());
    assert_eq!(number(&demo, None, "day"), 1.);
    assert!((number(&demo, None, "time_of_day") - 18.5).abs() < 0.05);
    assert!(count(&demo, "energy") > 75.);
    // The ship sailed on while the castaway slept.
    assert!(number(&demo, None, "world_time") - ship > 60. * 60.);
    assert!(flag(&demo, None, "control"));
}

#[test]
fn looking_at_the_lighthouse_marks_it() {
    let mut demo = island();
    assert!(!flag(&demo, None, "lighthouse_seen"));
    look_at(&mut demo, [-505., 29.25, 485.]);
    run(&mut demo, 60, GameplayInput::default());
    assert!(flag(&demo, None, "lighthouse_seen"));
    assert!(text(&demo, None, "toast").contains("lighthouse"));
}
