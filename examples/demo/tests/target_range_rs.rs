//! End-to-end checks for the scripted Target Range scene: the same game as `target-range.json`,
//! with every rule in a Rhai script on a Script Manager component instead of a graph.
//!
//! The assertions are the Blueprint scene's, which is the point: a scene ported to scripts must
//! move, aim, jump, respawn, swap weapons, shoot and win in exactly the same way.
use bozzard_demo::SceneDemo;
use bozzard_scene::{
    BlueprintHidden, CursorCapture, GameAction, GamePhase, GameplayInput, Scene, TextRendering,
    Transform,
};
use std::path::PathBuf;

const TARGETS: [&str; 4] = ["target-1", "target-2", "target-3", "target-4"];
/// The controller turns `yaw - mouse_x * sensitivity` degrees; authoring is the scene's job.
const SENSITIVITY: f32 = 0.2;
const EYE_HEIGHT: f32 = 0.5;

fn load() -> (Scene, PathBuf) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenes/target-range-rs.json");
    let scene = bozzard_demo::load_document(Some(&path)).unwrap();
    assert_eq!(
        Scene::from_json(&scene.to_json().unwrap()).unwrap(),
        scene,
        "the scene must survive a serialization round trip"
    );
    (scene, path)
}

fn demo() -> SceneDemo {
    let (scene, path) = load();
    let mut demo = SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
    demo.game_action(GameAction::Start).unwrap();
    demo
}

fn position(demo: &SceneDemo, id: &str) -> [f32; 3] {
    demo.app
        .world
        .get::<Transform>(demo.instance().entity(id).unwrap())
        .unwrap()
        .translation
}

fn camera_yaw(demo: &SceneDemo) -> f32 {
    demo.app
        .world
        .get::<Transform>(demo.instance().entity("camera").unwrap())
        .unwrap()
        .rotation_degrees[1]
}

fn camera_pitch(demo: &SceneDemo) -> f32 {
    demo.app
        .world
        .get::<Transform>(demo.instance().entity("camera").unwrap())
        .unwrap()
        .rotation_degrees[0]
}

fn tick(demo: &mut SceneDemo, input: GameplayInput) {
    demo.set_gameplay_input(input);
    demo.app.step();
    demo.check_simulation().unwrap();
}

fn run(demo: &mut SceneDemo, ticks: usize, input: GameplayInput) {
    for _ in 0..ticks {
        tick(demo, input);
    }
}

/// Shoots flat along -Z, so yaw is the only thing the player has to aim.
fn yaw_to(from: [f32; 3], to: [f32; 3]) -> f32 {
    let (dx, dz) = (to[0] - from[0], to[2] - from[2]);
    f32::to_degrees((-dx).atan2(-dz))
}

/// Mouse-look aims the way a player would: relative deltas, never a teleported camera.
fn aim(demo: &mut SceneDemo, target: [f32; 3]) {
    for _ in 0..8 {
        let player = position(demo, "player");
        let error = (yaw_to(player, target) - camera_yaw(demo) + 540.0).rem_euclid(360.0) - 180.0;
        if error.abs() < 0.2 {
            return;
        }
        tick(
            demo,
            GameplayInput {
                orbit: [-error / SENSITIVITY, 0.0],
                ..Default::default()
            },
        );
    }
}

/// Spawned prefab instances are named `spawn-<serial>-<index>`, newest serial last.
fn latest_spawn(demo: &SceneDemo) -> String {
    (1..=64)
        .filter_map(|serial| {
            let id = format!("spawn-{serial}-0");
            demo.instance().entity(&id).map(|_| id)
        })
        .next_back()
        .expect("a shot must spawn a projectile")
}

fn scale_of(demo: &SceneDemo, id: &str) -> [f32; 3] {
    demo.app
        .world
        .get::<Transform>(demo.instance().entity(id).unwrap())
        .unwrap()
        .scale
}

fn hud_text(demo: &SceneDemo, id: &str) -> String {
    demo.app
        .world
        .get::<TextRendering>(demo.instance().entity(id).unwrap())
        .unwrap()
        .text
        .clone()
}

fn place(demo: &mut SceneDemo, id: &str, translation: [f32; 3]) {
    let entity = demo.instance().entity(id).unwrap();
    demo.app
        .world
        .get_mut::<Transform>(entity)
        .unwrap()
        .translation = translation;
}

/// Horizontal speed of the newest projectile, measured over ten ticks of flight.
fn projectile_speed(demo: &mut SceneDemo) -> f32 {
    let id = latest_spawn(demo);
    let at = |demo: &SceneDemo| {
        let t = position(demo, &id);
        [t[0], t[2]]
    };
    let start = at(demo);
    run(demo, 10, GameplayInput::default());
    let end = at(demo);
    let distance = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
    distance / (10.0 / 60.0)
}

fn destroyed(demo: &SceneDemo, id: &str) -> bool {
    let entity = demo.instance().entity(id).unwrap();
    demo.app
        .world
        .get::<BlueprintHidden>(entity)
        .is_some_and(|hidden| hidden.0)
        && position(demo, id)[1] < -50.0
}

#[test]
fn a_scripted_controller_moves_aims_jumps_and_respawns_the_player() {
    let (scene, _) = load();
    let player = scene.objects.iter().find(|o| o.id == "player").unwrap();
    assert!(
        player.player_controller.is_none() && player.gravity.is_none(),
        "the scene must drive the player from scripts, not the built-in controller"
    );
    assert!(
        player
            .script_manager
            .as_ref()
            .is_some_and(|m| m.scripts.len() >= 2),
        "the player must be driven by scripts"
    );
    assert!(
        player.blueprints.is_empty(),
        "the scripted scene carries no graphs of its own"
    );
    let mut demo = demo();
    // The script's own gravity settles the body on the platform.
    run(&mut demo, 60, GameplayInput::default());
    let rest = position(&demo, "player");
    assert!(
        (rest[1] - 0.6).abs() < 0.02 && rest[0].abs() < 0.01,
        "expected the player to rest on the platform, got {rest:?}"
    );
    // First person: the camera is the eye, and it follows the body.
    let eye = [rest[0], rest[1] + EYE_HEIGHT, rest[2]];
    let camera = position(&demo, "camera");
    assert!(
        camera.iter().zip(eye).all(|(a, b)| (a - b).abs() <= 0.01),
        "camera {camera:?} must sit at the eye {eye:?}"
    );
    assert!(
        demo.app
            .world
            .get::<BlueprintHidden>(demo.instance().entity("player").unwrap())
            .unwrap()
            .0,
        "a first-person camera must not look at its own body"
    );
    // W walks along the camera's forward axis (-Z at yaw 0).
    run(
        &mut demo,
        60,
        GameplayInput {
            movement: [0.0, 1.0],
            ..Default::default()
        },
    );
    let walked = position(&demo, "player");
    assert!(
        (walked[2] - (rest[2] - 4.5)).abs() < 0.1 && walked[0].abs() < 0.01,
        "one second of forward movement should travel 4.5 units, got {walked:?}"
    );
    assert!((position(&demo, "camera")[1] - (walked[1] + EYE_HEIGHT)).abs() <= 0.01);
    // Mouse deltas turn the view, and the strafe axis follows it.
    tick(
        &mut demo,
        GameplayInput {
            orbit: [40.0, 0.0],
            ..Default::default()
        },
    );
    assert!(
        (camera_yaw(&demo) - -8.0).abs() < 0.001,
        "40 points at 0.2 degrees/point is an 8 degree turn, got {}",
        camera_yaw(&demo)
    );
    // The graph clamps pitch, so no amount of looking up or down rolls the camera over.
    tick(
        &mut demo,
        GameplayInput {
            orbit: [0.0, -10_000.0],
            ..Default::default()
        },
    );
    assert!(
        (camera_pitch(&demo) - 15.0).abs() < 0.001,
        "{}",
        camera_pitch(&demo)
    );
    tick(
        &mut demo,
        GameplayInput {
            orbit: [0.0, 10_000.0],
            ..Default::default()
        },
    );
    assert!(
        (camera_pitch(&demo) - -75.0).abs() < 0.001,
        "{}",
        camera_pitch(&demo)
    );
    tick(
        &mut demo,
        GameplayInput {
            orbit: [0.0, -375.0],
            ..Default::default()
        },
    );
    assert!(
        camera_pitch(&demo).abs() < 0.001,
        "level again for the shot"
    );
    run(
        &mut demo,
        30,
        GameplayInput {
            movement: [1.0, 0.0],
            ..Default::default()
        },
    );
    assert!(
        position(&demo, "player")[0] > walked[0] + 1.0,
        "strafing must move along the turned view"
    );
    // Jump rises and lands; grounded is read from the move's own floor contact.
    let ground = position(&demo, "player");
    tick(
        &mut demo,
        GameplayInput {
            jump: true,
            ..Default::default()
        },
    );
    let mut peak = ground[1];
    for _ in 0..90 {
        tick(&mut demo, GameplayInput::default());
        peak = peak.max(position(&demo, "player")[1]);
    }
    assert!(
        (peak - ground[1] - 1.8).abs() < 0.1,
        "a 6 unit/second jump under 9.81 gravity peaks near 1.83 units, got {}",
        peak - ground[1]
    );
    assert!(
        (position(&demo, "player")[1] - ground[1]).abs() < 0.01,
        "the jump must land"
    );
    // Air control must not re-launch: a second press on the ground does jump again.
    tick(
        &mut demo,
        GameplayInput {
            jump: true,
            ..Default::default()
        },
    );
    run(&mut demo, 2, GameplayInput::default());
    assert!(position(&demo, "player")[1] > ground[1] + 0.05);
    run(&mut demo, 120, GameplayInput::default());
    // Walking off the platform falls past the authored limit and respawns at the start.
    let forward = GameplayInput {
        movement: [0.0, 1.0],
        ..Default::default()
    };
    let mut respawned = false;
    // The body is 0.4 wide, so it keeps standing until its whole footprint leaves the edge.
    for _ in 0..900 {
        tick(&mut demo, forward);
        let at = position(&demo, "player");
        if at[2] < -10.0 && at[1] < ground[1] - 0.5 {
            respawned = true;
            break;
        }
    }
    assert!(
        respawned,
        "past the edge nothing holds the body up: the script's own gravity takes over"
    );
    let mut back = position(&demo, "player");
    for _ in 0..900 {
        tick(&mut demo, forward);
        back = position(&demo, "player");
        if back[2] > 6.0 {
            break;
        }
    }
    assert!(
        (back[2] - 7.0).abs() < 1.0 && (0.55..=0.7).contains(&back[1]),
        "a fall must respawn the player above the start, got {back:?}"
    );
}

#[test]
fn the_scene_locks_the_cursor_draws_a_crosshair_and_hands_it_back_on_a_win() {
    let mut demo = demo();
    // Graphs run on fixed ticks: the first one takes the pointer.
    tick(&mut demo, GameplayInput::default());
    assert_eq!(
        demo.app
            .world
            .resource::<CursorCapture>()
            .and_then(|capture| capture.requested),
        Some(true),
        "the player script must claim the pointer for mouse-look"
    );
    let crosshair = demo.app.world.get::<TextRendering>(
        demo.instance()
            .entity("crosshair")
            .expect("the scene needs a crosshair"),
    );
    assert_eq!(crosshair.unwrap().text, "+");
    assert_eq!(crosshair.unwrap().screen.unwrap().anchor, [0.5, 0.5]);
    for id in TARGETS {
        let target = position(&demo, id);
        aim(&mut demo, target);
        tick(
            &mut demo,
            GameplayInput {
                fire: true,
                ..Default::default()
            },
        );
        run(&mut demo, 45, GameplayInput::default());
        assert!(destroyed(&demo, id), "{id} survived the shot");
    }
    assert_eq!(demo.game_session().unwrap().phase, GamePhase::GameOver);
    assert!(demo.game_session().unwrap().message.contains("You win"));
    assert_eq!(
        demo.app
            .world
            .resource::<CursorCapture>()
            .and_then(|capture| capture.requested),
        Some(false),
        "the win script must release the pointer before the run ends"
    );
    // Game over freezes the world: no more cubes change state.
    let frozen: Vec<_> = TARGETS.map(|id| position(&demo, id)).to_vec();
    run(&mut demo, 60, GameplayInput::default());
    for (id, before) in TARGETS.iter().zip(frozen) {
        assert_eq!(position(&demo, id), before);
    }
}

#[test]
fn walking_the_weapon_table_swaps_the_gun_and_the_shot_kicks_the_view() {
    let mut demo = demo();
    for id in [
        "weapon-table",
        "weapon-ar",
        "weapon-pistol",
        "weapon-shotgun",
        "hud-weapon",
    ] {
        let object = demo
            .instance()
            .document()
            .objects
            .iter()
            .find(|o| o.id == id);
        assert!(object.is_some(), "{id} is missing from the scene");
    }
    let table = demo
        .instance()
        .document()
        .objects
        .iter()
        .find(|o| o.id == "weapon-table")
        .unwrap();
    assert!(
        table.collider.is_some(),
        "the table has to be solid, or the player cannot walk up to it"
    );
    // Standing anywhere else, E is inert.
    tick(&mut demo, GameplayInput::default());
    assert_eq!(hud_text(&demo, "hud-weapon"), "WEAPON: AR");
    tick(
        &mut demo,
        GameplayInput {
            interact: true,
            ..Default::default()
        },
    );
    assert_eq!(
        hud_text(&demo, "hud-weapon"),
        "WEAPON: AR",
        "E must only pick up what the player is standing at"
    );
    // Walk there like a player would: strafe right to the table, then forward along it.
    let strafe = GameplayInput {
        movement: [1.0, 0.0],
        ..Default::default()
    };
    for _ in 0..300 {
        tick(&mut demo, strafe);
        if position(&demo, "player")[0] > 6.4 {
            break;
        }
    }
    let beside = position(&demo, "player");
    assert!(
        beside[0] > 6.4 && beside[0] < 6.9,
        "the table must stop the player at the bay, got {beside:?}"
    );
    // Walk down the table until the pistol bay, then press E there.
    let forward = GameplayInput {
        movement: [0.0, 1.0],
        ..Default::default()
    };
    let mut switched = false;
    for _ in 0..300 {
        tick(&mut demo, forward);
        if (2.3..3.7).contains(&position(&demo, "player")[2]) {
            tick(
                &mut demo,
                GameplayInput {
                    interact: true,
                    ..Default::default()
                },
            );
            switched = hud_text(&demo, "hud-weapon") == "WEAPON: PISTOL";
            break;
        }
    }
    assert!(switched, "standing at the pistol bay must equip the pistol");
    // Each weapon has its own bullet size and launch speed.
    let aimed = camera_pitch(&demo);
    tick(
        &mut demo,
        GameplayInput {
            fire: true,
            ..Default::default()
        },
    );
    let scale = scale_of(&demo, &latest_spawn(&demo))[0];
    assert!(
        (scale - 0.22).abs() < 0.001,
        "the pistol fires its own bullet size, got {scale}"
    );
    let speed = projectile_speed(&mut demo);
    assert!(
        (speed - 22.0).abs() < 2.0,
        "the pistol round leaves at 22 units/second, measured {speed}"
    );
    // Firing kicks the view up, and the kick decays back to where the player aimed.
    let kicked = camera_pitch(&demo);
    assert!(
        kicked > aimed + 0.2,
        "the shot must kick the camera up: aimed {aimed}, now {kicked}"
    );
    run(&mut demo, 40, GameplayInput::default());
    assert!(
        (camera_pitch(&demo) - aimed).abs() < 0.05,
        "recoil has to settle back on target, got {}",
        camera_pitch(&demo)
    );
    // The shotgun is a heavier, faster round.
    place(&mut demo, "player", [6.6, 0.6, 1.0]);
    tick(&mut demo, GameplayInput::default());
    tick(
        &mut demo,
        GameplayInput {
            interact: true,
            ..Default::default()
        },
    );
    assert_eq!(hud_text(&demo, "hud-weapon"), "WEAPON: SHOTGUN");
    tick(
        &mut demo,
        GameplayInput {
            fire: true,
            ..Default::default()
        },
    );
    let scale = scale_of(&demo, &latest_spawn(&demo))[0];
    assert!(
        (scale - 0.34).abs() < 0.001,
        "the shotgun slug is the big one, got {scale}"
    );
    let speed = projectile_speed(&mut demo);
    assert!(
        (speed - 60.0).abs() < 4.0,
        "the shotgun slug leaves at 60 units/second, measured {speed}"
    );
}

#[test]
fn a_missed_shot_leaves_the_arena_instead_of_stretching_the_shadow_map() {
    // The sun shadow map is fitted to every lit draw, so a projectile flying off over the
    // horizon used to grow the fitted box (and its texels) until the whole scene broke out
    // in acne bands. A miss must not outlive its welcome.
    let mut demo = demo();
    tick(
        &mut demo,
        GameplayInput {
            fire: true,
            ..Default::default()
        },
    );
    let shot = latest_spawn(&demo);
    let mut furthest = 0.0f32;
    let mut survived = 0;
    for _ in 0..600 {
        let Some(entity) = demo.instance().entity(&shot) else {
            break;
        };
        if demo.app.world.get::<BlueprintHidden>(entity).is_some() {
            break;
        }
        let at = position(&demo, &shot);
        furthest = furthest.max((at[0].powi(2) + at[2].powi(2)).sqrt());
        survived += 1;
        tick(&mut demo, GameplayInput::default());
    }
    assert!(survived < 120, "the miss flew for {survived} ticks");
    assert!(
        furthest < 16.0,
        "the miss reached {furthest} units from the arena centre"
    );
}
