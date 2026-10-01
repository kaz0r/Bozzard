//! Editor-Play simulation check for the standalone Earth factory prototype.
use bozzard_demo::SceneDemo;
use bozzard_scene::blueprint::{BlackboardValue, Value};
use bozzard_scene::{BlueprintRuntime, GameplayInput, Scene, keys};
use std::path::PathBuf;

fn demo_with_seed(seed: Option<f32>) -> SceneDemo {
    factory_with_mode(seed, true)
}

#[test]
#[ignore = "manual release-mode profile; timing varies by host"]
fn profile_chunk_transitions() {
    use std::{collections::BTreeMap, time::Instant};
    for demonstration in [false, true] {
        let mut demo = factory_with_mode(Some(4.), demonstration);
        tick(&mut demo, None);
        let mut samples: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        let mut slowest = Vec::new();
        for key in ["D", "S", "A", "W"] {
            for _ in 0..60 {
                for input in [Some(key), None] {
                    let before = (number(&demo, "chunk_x"), number(&demo, "chunk_z"));
                    let visited = controller_numbers(&demo, "visited");
                    let resident = controller_numbers(&demo, "resident");
                    let start = Instant::now();
                    tick(&mut demo, input);
                    let ms = start.elapsed().as_secs_f64() * 1000.;
                    let after = (number(&demo, "chunk_x"), number(&demo, "chunk_z"));
                    let kind = if before != after {
                        "crossing"
                    } else if visited != controller_numbers(&demo, "visited") {
                        "discovery"
                    } else if resident != controller_numbers(&demo, "resident") {
                        "streaming"
                    } else {
                        "ordinary"
                    };
                    samples.entry(kind).or_default().push(ms);
                    slowest.push((ms, kind, after));
                }
            }
        }
        println!("demonstration={demonstration}");
        for (kind, mut values) in samples {
            values.sort_by(f64::total_cmp);
            println!(
                "{kind}: n={} median={:.3}ms p95={:.3}ms max={:.3}ms",
                values.len(),
                values[values.len() / 2],
                values[values.len() * 95 / 100],
                values.last().unwrap()
            );
        }
        slowest.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("slowest: {:?}", &slowest[..8]);
    }
}

fn factory_with_mode(seed: Option<f32>, demonstration: bool) -> SceneDemo {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../earth-factory/scenes/earth.json");
    let mut scene = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    scene
        .objects
        .iter_mut()
        .find(|o| o.id == "controller")
        .unwrap()
        .blackboard
        .insert(
            "title_open".into(),
            BlackboardValue::Scalar(Value::Bool(false)),
        );
    scene.blackboard.insert(
        "demo_mode".into(),
        BlackboardValue::Scalar(Value::Bool(demonstration)),
    );
    if let Some(seed) = seed {
        scene
            .blackboard
            .insert("seed".into(), BlackboardValue::Scalar(Value::Number(seed)));
    }
    SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap()
}

#[test]
fn spaceship_debris_is_sparse_seeded_bounded_and_clear_of_resources_on_both_planets() {
    use std::collections::BTreeSet;
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    let wrecks = demo.instance().script_module("factory-debris").unwrap();
    let deposits = demo.instance().script_module("factory-deposits").unwrap();
    let mut histogram = [0usize; 7];
    for seed in (1i64..=64).chain([1926, 8362, 2_000_000_000]) {
        let mut earth = Vec::new();
        for planet in 0i64..2 {
            let plan: Vec<f32> = wrecks.call_args("layout", (seed, planet)).unwrap();
            let count = plan[0] as usize;
            assert!((2..=6).contains(&count));
            assert_eq!(plan.len(), 1 + count * 5);
            histogram[count] += 1;
            let replay: Vec<f32> = wrecks.call_args("layout", (seed, planet)).unwrap();
            assert_eq!(plan, replay, "layout depends on exploration or call order");
            let mut regions = BTreeSet::new();
            let mut variants = BTreeSet::new();
            let radius = if planet == 0 { 8 } else { 6 };
            for wreck in plan[1..].chunks_exact(5) {
                let id = wreck[0] as i64;
                assert!(regions.insert(id), "two wrecks occupy the same region");
                let (cx, cz) = (id % 17 - 8, id / 17 - 8);
                assert!(cx.abs() <= radius && cz.abs() <= radius);
                assert!(
                    cx.abs().max(cz.abs()) >= 2,
                    "wreck crowds the landing region"
                );
                let (x, z) = (wreck[1] as i64, wreck[2] as i64);
                assert!(x.abs() <= 4 && z.abs() <= 4);
                assert!((0..4).contains(&(wreck[3] as i64)));
                assert!([0., 90., 180., 270.].contains(&wreck[4]));
                variants.insert(wreck[3] as i64);
                let nodes: Vec<f32> = deposits
                    .call_args("generate_region_nodes", (seed, cx, cz, planet))
                    .unwrap();
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        assert_eq!(
                            nodes[((z + dz + 7) * 15 + x + dx + 7) as usize],
                            0.,
                            "wreck covers a deposit"
                        );
                        assert!(
                            wrecks
                                .call_args::<_, bool>(
                                    "blocked",
                                    (seed, planet, cx * 15 + x + dx, cz * 15 + z + dz)
                                )
                                .unwrap()
                        );
                    }
                }
                assert!(
                    !wrecks
                        .call_args::<_, bool>(
                            "blocked",
                            (seed, planet, cx * 15 + x + 2, cz * 15 + z)
                        )
                        .unwrap()
                );
            }
            assert_eq!(variants.len(), count.min(4));
            if planet == 0 {
                earth = plan;
            } else {
                assert_ne!(earth, plan);
            }
        }
    }
    assert!(
        histogram[2] > histogram[3] && histogram[3] > histogram[4],
        "extra debris is too common: {histogram:?}"
    );
    assert!(histogram[4] > 0, "optional slots never spawn");
    assert!(histogram[6] > 0, "maximum layout is never exercised");
    println!("debris count distribution across 134 seeded planets: {histogram:?}");
}

#[test]
fn spaceship_debris_caps_live_models_at_six_and_restores_after_planet_travel() {
    let mut demo = factory_with_mode(Some(1926.), false);
    tick(&mut demo, None);
    let original: Vec<f32> = demo
        .instance()
        .script_module("factory-debris")
        .unwrap()
        .call_args("layout", (1926i64, 0i64))
        .unwrap();
    assert_eq!(original[0], 6.);
    let load_all = r#"
        let plan=debris::current_layout();
        for slot in 0..plan[0].to_int() {
            let id=plan[1+slot*5].to_int();chunks::discover_chunk(id%17-8,id/17-8);
        }
        let live=0;for handle in get_object_list("factory-transports","debris_handles") {if handle!="" {live+=1;}}
        if live!=plan[0].to_int() {throw "incorrect planet-wide wreck count";}
    "#;
    factory_code(&mut demo, load_all, 1);
    factory_code(&mut demo, "set_object_variable(\"phase\",7.0);", 1);
    board_other_planet(&mut demo);
    assert_eq!(controller_numbers(&demo, "session")[7], 1.);
    factory_code(
        &mut demo,
        r#"
        for handle in get_object_list("factory-transports","debris_handles") {if handle!="" {throw "off-planet wreck leaked";}}
    "#,
        1,
    );
    factory_code(&mut demo, load_all, 1);
    board_other_planet(&mut demo);
    assert_eq!(controller_numbers(&demo, "session")[7], 0.);
    factory_code(&mut demo, load_all, 1);
    let replay: Vec<f32> = demo
        .instance()
        .script_module("factory-debris")
        .unwrap()
        .call_args("layout", (1926i64, 0i64))
        .unwrap();
    assert_eq!(original, replay);
    factory_code(
        &mut demo,
        r#"
        world::begin_world(4);
        for handle in get_object_list("factory-transports","debris_handles") {if handle!="" {throw "new world kept old wrecks";}}
        if debris::current_layout()!=debris::layout(4,0) {throw "new world kept old layout";}
    "#,
        1,
    );
}

#[test]
fn spaceship_debris_reserves_story_tiles_in_coop_without_spending_inventory() {
    use bozzard_demo::factory::{
        authority::Executor, replication::requests::Action, shared::Stack,
    };
    use std::time::Duration;
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    factory_code(
        &mut demo,
        r#"
        let plan=debris::current_layout();let id=plan[1].to_int();
        world::enter_chunk(id%17-8,id/17-8);
        set_scene_variable("cursor_x",plan[2]);set_scene_variable("cursor_z",plan[3]);
        set_object_variable("phase",7.0);
    "#,
        1,
    );
    let (mut world, mut player) = coop_world(&demo);
    player.backpack[0] = Stack {
        kind: 1,
        amount: 100,
    };
    let before = world.clone();
    let inventory = player.backpack;
    let mut executor = Executor::new(demo.instance()).unwrap();
    let result = executor
        .apply(
            &mut world,
            10,
            &mut player,
            &Action::Place {
                kind: 2,
                direction: 0,
            },
            Duration::ZERO,
        )
        .unwrap();
    assert!(!result.accepted);
    assert_eq!(result.message, "Keep the spaceship wreckage clear.");
    assert_eq!(world, before);
    assert_eq!(player.backpack, inventory);
}

#[test]
fn spaceship_debris_streams_without_duplicates_and_survives_view_restore() {
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    factory_code(
        &mut demo,
        r#"
        let plan=debris::current_layout();let id=plan[1].to_int();
        let cx=id%17-8;let cz=id/17-8;
        world::enter_chunk(cx,cz);
        let before=get_object_list("factory-transports","debris_handles");
        if before[0]=="" {throw "wreck missing from its region";}
        chunks::load_chunk_visuals(cx,cz);debris::load_region(id);
        if get_object_list("factory-transports","debris_handles")!=before {throw "duplicate wreck";}
        chunks::unload_chunk_visuals(id);
        if get_object_list("factory-transports","debris_handles")[0]!="" {throw "wreck leaked after unload";}
        chunks::load_chunk_visuals(cx,cz);world::restore_chunk(cx,cz);
        if get_object_list("factory-transports","debris_handles")[0]=="" {throw "wreck missing after reload";}
        set_object_variable("creative",true);
        set_scene_variable("cursor_x",plan[2]);set_scene_variable("cursor_z",plan[3]);
        set_scene_variable("selected",2.0);building::place_selected();
        if get_scene_list("builds")[grid::index(plan[2].to_int(),plan[3].to_int())]!=0.0 {throw "built over story wreck";}
        world::archive_chunk();
    "#,
        1,
    );
    let old = demo
        .instance()
        .document()
        .objects
        .iter()
        .filter(|o| {
            o.name == "Broken cockpit"
                || o.name == "Split cargo hull"
                || o.name == "Torn survey wing"
                || o.name == "Ruptured engine"
        })
        .count();
    assert!(old > 0);
    // Native persistence copies only game data. Clearing/restoring presentation
    // must reconstruct the same wreck locations without retaining stale handles.
    let directory = save_directory(&mut demo);
    factory_code(&mut demo, "persistence::prepare_save(1);", 1);
    factory_code(&mut demo, "persistence::update(0.0);", 1);
    finish_save_io(&mut demo);
    factory_code(
        &mut demo,
        "world::begin_world(17);data::session_set(117,1.0);data::session_set(116,2.0);",
        1,
    );
    factory_code(&mut demo, "persistence::update(0.0);", 1);
    finish_save_io(&mut demo);
    factory_code(
        &mut demo,
        r#"
        let plan=debris::current_layout();
        if get_object_list("factory-transports","debris_handles")[0]=="" {throw "saved wreck missing";}
        let expected=debris::layout(get_scene_variable("seed").to_int(),data::session_value(7).to_int());
        if plan!=expected {throw "saved wreck rerolled";}
    "#,
        1,
    );
    assert_eq!(number(&demo, "seed"), 4.);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn resource_planets_are_sparse_spaced_seeded_and_progression_safe() {
    use std::collections::{BTreeMap, BTreeSet};
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../earth-factory/scenes/scripts/earth_factory.rs");
    let source = std::fs::read_to_string(path)
        .unwrap()
        .replace("fn on_start(me)", "fn factory_start(me)")
        .replace("fn on_update(me, dt)", "fn factory_update(me, dt)");
    let mut first_planet = BTreeMap::new();
    for (seed, reverse) in [
        (1., false),
        (4., false),
        (2_000_000_000., false),
        (1., true),
    ] {
        let mut demo = factory_with_mode(Some(seed), false);
        // Evaluate the actual generator once per tick, without spawning a whole
        // planet's models or coupling this distribution check to render capacity.
        let script = format!(
            r#"{source}
            fn on_start(me) {{}}
            fn on_update(me, dt) {{
                let step = get_scene_variable("ticks").to_int();
                let id = if {reverse} {{ 288 - step }} else {{ step }};
                set_scene_list("nodes", deposits::generate_region_nodes(get_scene_variable("seed").to_int(), id % 17 - 8, id / 17 - 8));
                set_scene_variable("ticks", (step + 1).to_float());
            }}
        "#
        );
        demo.with_instance(|instance, _| instance.register_script("earth-factory".into(), script))
            .unwrap();
        let mut planet = BTreeMap::new();
        let mut positions = BTreeSet::new();
        let mut counts = [0usize; 64];
        for step in 0..289 {
            tick(&mut demo, None);
            let id = if reverse { 288 - step } else { step };
            let nodes = numbers(&demo, "nodes");
            let deposits = nodes.iter().filter(|&&kind| kind > 0.).count();
            if id == 144 {
                assert_eq!(deposits, 5);
                for kind in [1., 2., 3., 8., 9.] {
                    assert!(nodes.contains(&kind));
                }
            } else {
                assert!((2..=4).contains(&deposits));
            }
            let mut seen = BTreeSet::new();
            for (cell, &kind) in nodes.iter().enumerate().filter(|(_, kind)| **kind > 0.) {
                assert!(seen.insert(kind as usize), "duplicate resource in a region");
                counts[kind as usize] += 1;
                let x = (id % 17 - 8) * 15 + cell as i32 % 15 - 7;
                let z = (id / 17 - 8) * 15 + cell as i32 / 15 - 7;
                assert!(
                    !((-1..=2).contains(&x) && (-1..=2).contains(&z)),
                    "landing pad blocked"
                );
                for dx in -2..=2 {
                    for dz in -2..=2 {
                        if dx * dx + dz * dz < 9 {
                            assert!(
                                !positions.contains(&(x + dx, z + dz)),
                                "crowded deposits across region boundaries for seed {seed}"
                            );
                        }
                    }
                }
                positions.insert((x, z));
            }
            planet.insert(id, nodes);
        }
        assert!(
            (1..=10).chain([40, 45]).all(|kind| counts[kind] > 0),
            "finite planet missing a resource"
        );
        assert!(
            [127, 143, 145, 161]
                .iter()
                .any(|id| planet[id].contains(&4.)),
            "coal needs a reachable neighboring outpost"
        );
        let min_common = [1, 2, 3, 8, 9]
            .iter()
            .map(|&kind| counts[kind])
            .min()
            .unwrap();
        let max_uncommon = [4, 5, 7].iter().map(|&kind| counts[kind]).max().unwrap();
        let min_uncommon = [4, 5, 7].iter().map(|&kind| counts[kind]).min().unwrap();
        assert!(min_common > max_uncommon);
        assert!(
            counts[6] < min_uncommon && counts[10] < min_uncommon,
            "oil and silver must be rare: {counts:?}"
        );
        println!(
            "seed={seed} reverse={reverse} deposits={} kinds={counts:?}",
            positions.len()
        );
        if reverse {
            assert_eq!(
                planet, first_planet,
                "discovery order changed resource locations"
            );
        } else if seed == 1. {
            first_planet = planet;
        } else {
            assert_ne!(planet, first_planet, "different seed reused the layout");
        }
    }
}

#[test]
fn debug_hud_uses_render_measurements_and_tracks_loaded_chunks() {
    use bozzard_diagnostics::{RenderCounters, RenderDiagnostics};
    use std::time::{Duration, Instant};
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    let hud = |demo: &SceneDemo, id: &str| {
        demo.instance()
            .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
            .unwrap()
            .element(id)
            .unwrap()
            .text
            .clone()
    };
    assert_eq!(hud(&demo, "debug-fps"), "-- FPS");
    assert_eq!(hud(&demo, "debug-timing"), "Frame -- ms   CPU draw -- ms");
    assert_eq!(
        hud(&demo, "debug-entities"),
        "Visible entities --   Draws --"
    );
    assert_eq!(hud(&demo, "debug-chunks"), "Chunks 1 loaded / 1 explored");
    assert_eq!(
        hud(&demo, "debug-simulation"),
        "Sim --   CPU -- ms   Wait -- ms"
    );

    let mut diagnostics = RenderDiagnostics::default();
    let start = Instant::now();
    let counters = RenderCounters {
        visible_entities: 123,
        draw_calls: 17,
        triangles: 4567,
        cpu_draw_ms: 3.25,
        viewport_aspect: 1.6,
    };
    for i in 0..=10 {
        diagnostics.record(start + Duration::from_millis(i * 25), counters);
    }
    demo.app.world.insert_resource(diagnostics);
    demo.app
        .world
        .insert_resource(bozzard_diagnostics::SimulationMetrics {
            available: true,
            threaded: true,
            cpu_ms: 2.25,
            wait_ms: 0.4,
            steps: 1,
        });
    settle(&mut demo, 16);
    assert_eq!(hud(&demo, "debug-fps"), "40 FPS");
    assert_eq!(
        hud(&demo, "debug-simulation"),
        "Sim worker   CPU 2.3 ms   Wait 0.4 ms"
    );
    assert_eq!(
        hud(&demo, "debug-timing"),
        "Frame 25.0 ms   CPU draw 3.3 ms"
    );
    assert_eq!(
        hud(&demo, "debug-entities"),
        "Visible entities 123   Draws 17"
    );
    assert_eq!(
        hud(&demo, "debug-triangles"),
        "Triangles 4567   Simulating 0"
    );

    move_cursor(&mut demo, 7, 0);
    settle(&mut demo, 16);
    assert_eq!(hud(&demo, "debug-chunks"), "Chunks 2 loaded / 2 explored");
    press(&mut demo, "N");
    assert_eq!(hud(&demo, "debug-chunks"), "Chunks 1 loaded / 1 explored");
}

fn tick(demo: &mut SceneDemo, key: Option<&str>) {
    demo.set_gameplay_input(GameplayInput {
        keys: key.map_or(0, keys::bit),
        ..Default::default()
    });
    demo.app.step();
    demo.check_simulation().unwrap();
}

#[test]
fn steam_overlay_cancels_controls_and_drags_while_production_continues() {
    use bozzard_scene::middleware::ui::{Input, Runtime, ScriptEvent};
    let mut demo = demo_with_seed(Some(4.));
    tick(&mut demo, None);
    let cursor = [number(&demo, "cursor_x"), number(&demo, "cursor_z")];
    let builds = numbers(&demo, "builds");
    demo.set_gameplay_input(GameplayInput {
        keys: keys::bit("D") | keys::bit("F"),
        ..Default::default()
    });
    demo.app.world.insert_resource(Runtime {
        active: Some("menu-open".into()),
        pointer: Some([300., 200.]),
        script_events: vec![ScriptEvent {
            kind: "down",
            target: String::new(),
            position: [300., 200.],
            delta: 0.,
            blocked: false,
        }],
        ..Default::default()
    });
    demo.set_steam_overlay_active(true);
    assert!(!demo.accepts_gameplay_input());
    let ui = demo.app.world.resource::<Runtime>().unwrap();
    assert!(ui.active.is_none() && ui.pointer.is_none());
    assert_eq!(ui.script_events.len(), 1);
    assert_eq!(ui.script_events[0].kind, "cancel");
    for input in [
        Input::Key("Escape".into()),
        Input::ActivateObject("menu-open".into()),
        Input::ScrollFocused(120.),
    ] {
        assert!(
            demo.ui_input(bozzard_scene::Layer::ThreeD, [1080., 600.], input)
                .unwrap()
        );
    }
    assert!(demo.multiplayer_key("Enter"));
    assert!(demo.multiplayer_text("wasd"));
    tick_keys(&mut demo, &["D", "Space", "X", "N", "R", "J", "E", "F"]);
    settle(&mut demo, 620);
    assert!(!demo.app.is_paused());
    assert_eq!(
        [number(&demo, "cursor_x"), number(&demo, "cursor_z")],
        cursor
    );
    assert_eq!(numbers(&demo, "builds"), builds);
    assert!(
        numbers(&demo, "counts")[20] > 0.,
        "powered factory must keep delivering while Steam is open"
    );
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(frame.element("menu-panel").is_none());
    assert!(frame.element("journal-book").is_none());
    demo.set_steam_overlay_active(false);
    assert!(demo.accepts_gameplay_input());
    tick(&mut demo, None);
    assert_eq!(
        number(&demo, "cursor_x"),
        cursor[0],
        "closing must not replay held keys"
    );
    press(&mut demo, "D");
    assert_eq!(number(&demo, "cursor_x"), cursor[0] + 1.);
}

#[test]
fn escape_menu_blocks_controls_but_keeps_factory_running_and_exit_quits() {
    use bozzard_scene::middleware::ui::Input;
    let mut demo = demo_with_seed(Some(4.));
    tick(&mut demo, None);
    let seed = number(&demo, "seed");
    let builds = numbers(&demo, "builds");
    let cursor = [number(&demo, "cursor_x"), number(&demo, "cursor_z")];
    let hud = |demo: &SceneDemo| {
        demo.instance()
            .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
            .unwrap()
    };
    assert!(
        demo.ui_input(
            bozzard_scene::Layer::ThreeD,
            [1080., 600.],
            Input::Key("Escape".into())
        )
        .unwrap()
    );
    tick(&mut demo, None);
    let frame = hud(&demo);
    assert!(frame.element("menu-panel").is_some());
    assert!(frame.element("menu-save").unwrap().enabled);
    assert!(frame.element("menu-load").unwrap().enabled);
    assert!(!demo.app.is_paused());
    assert!(bozzard_scene::game_flow::simulation_running(
        &demo.app.world
    ));
    tick_keys(
        &mut demo,
        &["D", "Space", "X", "N", "R", "Ctrl", "2", "J", "E", "F"],
    );
    assert_eq!(number(&demo, "seed"), seed);
    assert_eq!(numbers(&demo, "builds"), builds);
    assert_eq!(
        [number(&demo, "cursor_x"), number(&demo, "cursor_z")],
        cursor
    );
    assert!(hud(&demo).element("journal-book").is_none());
    assert!(hud(&demo).element("storage-panel").is_none());
    settle(&mut demo, 620);
    assert!(
        numbers(&demo, "counts")[20] > 0.,
        "machine parts must reach storage while the menu is open"
    );
    assert!(hud(&demo).element("menu-panel").is_some());
    click_widget(&mut demo, "menu-continue");
    assert!(hud(&demo).element("menu-panel").is_none());
    press(&mut demo, "D");
    assert_eq!(number(&demo, "cursor_x"), cursor[0] + 1.);
    press(&mut demo, "J");
    settle(&mut demo, 16);
    press(&mut demo, "Escape");
    assert!(hud(&demo).element("journal-book").is_none());
    assert!(hud(&demo).element("menu-panel").is_some());
    ui_event(&mut demo, Input::Key("Escape".into()));
    tick(&mut demo, None);
    assert!(hud(&demo).element("menu-panel").is_none());
    click_widget(&mut demo, "menu-open");
    click_widget(&mut demo, "menu-exit");
    assert_eq!(
        demo.game_session().unwrap().phase,
        bozzard_scene::GamePhase::Quit
    );
}

#[test]
fn wheel_zoom_is_smooth_bounded_consumed_once_and_blocked_by_panels() {
    use bozzard_scene::{Camera, Layer, middleware::ui::Input};
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    let size = |demo: &SceneDemo| {
        let Camera::Orthographic { vertical_size, .. } = demo
            .app
            .world
            .get::<Camera>(demo.instance().entity("camera").unwrap())
            .unwrap()
        else {
            panic!("orthographic camera");
        };
        *vertical_size
    };
    let wheel = |demo: &mut SceneDemo, point, delta| {
        demo.ui_input(
            Layer::ThreeD,
            [1080., 600.],
            Input::ScrollAt { point, delta },
        )
        .unwrap()
    };
    assert_eq!(size(&demo), 19.);
    assert!(!wheel(&mut demo, [700., 300.], -40.));
    tick(&mut demo, None);
    let target = controller_number(&demo, "camera_zoom_target");
    assert!(size(&demo) < 19. && size(&demo) > target);
    settle(&mut demo, 60);
    assert_eq!(
        controller_number(&demo, "camera_zoom_target"),
        target,
        "one wheel event must not repeat across ticks"
    );
    assert_eq!(size(&demo), target);
    assert!(
        wheel(&mut demo, [100., 100.], -40.),
        "objective panel owns scrolling"
    );
    tick(&mut demo, None);
    assert_eq!(size(&demo), target);
    for _ in 0..3 {
        wheel(&mut demo, [700., 300.], -400.);
        tick(&mut demo, None);
    }
    settle(&mut demo, 60);
    assert_eq!(size(&demo), 9.);
    for _ in 0..3 {
        wheel(&mut demo, [700., 300.], 400.);
        tick(&mut demo, None);
    }
    settle(&mut demo, 60);
    assert_eq!(size(&demo), 32.);
    press(&mut demo, "Escape");
    assert!(wheel(&mut demo, [700., 300.], -400.));
    tick(&mut demo, None);
    assert_eq!(size(&demo), 32.);
    press(&mut demo, "Escape");
    press(&mut demo, "J");
    settle(&mut demo, 16);
    assert!(wheel(&mut demo, [700., 300.], -400.));
    tick(&mut demo, None);
    assert_eq!(size(&demo), 32.);
    press(&mut demo, "J");
    settle(&mut demo, 16);
    tick_keys(&mut demo, &["Ctrl", "R"]);
    settle(&mut demo, 40);
    for _ in 0..8 {
        press(&mut demo, "W");
    } // After the turn, W moves west.
    assert_eq!(number(&demo, "chunk_x"), -1.);
    assert_eq!(size(&demo), 32.);
    press(&mut demo, "N");
    assert_eq!(size(&demo), 19.);
}

fn tick_keys(demo: &mut SceneDemo, held: &[&str]) {
    demo.set_gameplay_input(GameplayInput {
        keys: held.iter().fold(0, |mask, key| mask | keys::bit(key)),
        ..Default::default()
    });
    demo.app.step();
    demo.check_simulation().unwrap();
}

fn numbers(demo: &SceneDemo, name: &str) -> Vec<f32> {
    let board = demo.app.world.resource::<BlueprintRuntime>().unwrap();
    let BlackboardValue::List { values, .. } = board.scene_blackboard().get(name).unwrap() else {
        panic!("{name} is not a list");
    };
    values
        .iter()
        .map(|value| match value {
            Value::Number(number) => *number,
            _ => panic!("{name} contains a non-number"),
        })
        .collect()
}

fn number(demo: &SceneDemo, name: &str) -> f32 {
    let board = demo.app.world.resource::<BlueprintRuntime>().unwrap();
    let BlackboardValue::Scalar(Value::Number(value)) = board.scene_blackboard().get(name).unwrap()
    else {
        panic!("{name} is not a number");
    };
    *value
}

fn press(demo: &mut SceneDemo, key: &str) {
    tick(demo, Some(key));
    tick(demo, None);
}

fn move_cursor(demo: &mut SceneDemo, x: i32, z: i32) {
    let mut cursor_x = number(demo, "cursor_x") as i32;
    let mut cursor_z = number(demo, "cursor_z") as i32;
    while cursor_x < x {
        press(demo, "D");
        cursor_x += 1;
    }
    while cursor_x > x {
        press(demo, "A");
        cursor_x -= 1;
    }
    while cursor_z < z {
        press(demo, "S");
        cursor_z += 1;
    }
    while cursor_z > z {
        press(demo, "W");
        cursor_z -= 1;
    }
}

#[test]
fn earth_generates_all_nodes_and_runs_a_factory_in_editor_play() {
    // Reproducible build/production fixture; N below still exercises a fresh world.
    let mut demo = demo_with_seed(Some(4.));
    tick(&mut demo, None);

    let nodes = numbers(&demo, "nodes");
    let builds = numbers(&demo, "builds");
    assert_eq!(nodes.len(), 225);
    for kind in 1..=7 {
        assert!(nodes.contains(&(kind as f32)), "missing Earth node {kind}");
    }
    for (node, machine) in [(1.0, 1.0), (2.0, 1.0), (4.0, 6.0)] {
        assert!(
            nodes
                .iter()
                .zip(&builds)
                .any(|(found, built)| *found == node && *built == machine),
            "the demonstration must put {machine} on resource {node}"
        );
    }
    assert!(builds.contains(&3.0), "demonstration smelter missing");
    assert!(builds.contains(&5.0), "demonstration assembler missing");
    assert!(builds.contains(&4.0), "demonstration storage missing");

    let copper_cell = nodes
        .iter()
        .enumerate()
        .find(|(cell, node)| **node == 2.0 && builds[*cell] == 0.0)
        .unwrap()
        .0;
    let copper_x = copper_cell as i32 % 15 - 7;
    let copper_z = copper_cell as i32 / 15 - 7;
    move_cursor(&mut demo, copper_x, copper_z);
    press(&mut demo, "1");
    press(&mut demo, "Space");
    assert_eq!(numbers(&demo, "builds")[copper_cell], 1.0);

    for _ in 0..600 {
        tick(&mut demo, None);
    }
    let counts = numbers(&demo, "counts");
    assert!(
        counts[20] > 0.0,
        "the demo lines should deposit machine parts"
    );

    tick(&mut demo, Some("N"));
    tick(&mut demo, None);
    let rerolled = numbers(&demo, "nodes");
    assert_ne!(
        nodes, rerolled,
        "new world should scatter nodes differently"
    );
    for kind in 1..=7 {
        assert!(rerolled.contains(&(kind as f32)), "reroll lost node {kind}");
    }
}

#[test]
fn demonstration_produces_parts_in_every_orientation() {
    for seed in 1..=4 {
        let mut demo = demo_with_seed(Some(seed as f32));
        for _ in 0..300 {
            tick(&mut demo, None);
        }
        let counts = numbers(&demo, "counts");
        assert!(
            counts[20] > 0.0,
            "seed {seed} did not deliver machine parts"
        );
    }
}

#[test]
fn conveyor_items_keep_their_identity_and_move_between_simulation_ticks() {
    let mut demo = demo_with_seed(Some(4.0));
    for _ in 0..60 {
        tick(&mut demo, None);
        if number(&demo, "ticks") == 2.0 && number(&demo, "clock") > 0.10 {
            break;
        }
    }
    let before = demo.instance().capture(&demo.app.world).unwrap();
    tick(&mut demo, None);
    let after = demo.instance().capture(&demo.app.world).unwrap();
    let moving = before
        .objects
        .iter()
        .filter(|o| o.name == "Moving item")
        .find(|object| {
            let Some(next) = after.objects.iter().find(|o| o.id == object.id) else {
                return false;
            };
            let travel: f32 = object
                .transform
                .translation
                .iter()
                .zip(next.transform.translation)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                .sqrt();
            travel > 0.005 && travel < 0.15
        })
        .expect("a persistent item must move a fraction of a cell per frame");
    let id = moving.id.clone();
    for _ in 0..20 {
        tick(&mut demo, None);
    }
    assert!(
        demo.instance()
            .capture(&demo.app.world)
            .unwrap()
            .objects
            .iter()
            .any(|o| o.id == id),
        "an item must retain its identity when it reaches the next conveyor cell"
    );
}

#[test]
fn tooltip_labels_the_landing_pod_only_at_home_and_real_deposits_elsewhere() {
    let mut demo = factory_with_mode(Some(1.), false);
    tick(&mut demo, None);
    move_cursor(&mut demo, 0, 0);
    settle(&mut demo, 30);
    let tooltip = |demo: &SceneDemo| {
        demo.instance()
            .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1280., 800.])
            .unwrap()
            .element("nearby-tooltip")
            .map(|element| element.text.clone())
    };
    assert_eq!(tooltip(&demo).as_deref(), Some("Landing pod  [J] Journal"));
    for region in 1..=2 {
        move_cursor(&mut demo, 7, 0);
        press(&mut demo, "D");
        assert_eq!(number(&demo, "chunk_x"), region as f32);
        move_cursor(&mut demo, 0, 0);
        let nodes = numbers(&demo, "nodes");
        // Sparse regions may have any resource near their center. Select actual
        // empty ground to check that the landing-pod label cannot follow us.
        let empty = (0_i32..225)
            .find(|&cell| {
                let x = cell % 15 - 7;
                let z = cell / 15 - 7;
                x.abs() <= 5
                    && z.abs() <= 5
                    && nodes.iter().enumerate().all(|(other, kind)| {
                        let dx = other as i32 % 15 - 7 - x;
                        let dz = other as i32 / 15 - 7 - z;
                        *kind == 0. || dx * dx + dz * dz > 2
                    })
            })
            .unwrap();
        move_cursor(&mut demo, empty % 15 - 7, empty / 15 - 7);
        settle(&mut demo, 30);
        assert!(tooltip(&demo).is_none(), "empty ground must have no label");
        let deposit = nodes.iter().position(|&kind| kind > 0.).unwrap();
        let names = [
            "",
            "Iron",
            "Copper",
            "Limestone",
            "Coal",
            "Quartz",
            "Oil",
            "Water",
            "Stone",
            "Sand",
            "Silver",
        ];
        let expected = format!("{} deposit", names[nodes[deposit] as usize]);
        move_cursor(&mut demo, deposit as i32 % 15 - 7, deposit as i32 / 15 - 7);
        settle(&mut demo, 30);
        assert_eq!(tooltip(&demo).as_deref(), Some(expected.as_str()));
    }
    // Revisiting home must restore the actual pod label.
    for _ in 0..2 {
        move_cursor(&mut demo, -7, 0);
        press(&mut demo, "A");
    }
    move_cursor(&mut demo, 0, 0);
    settle(&mut demo, 30);
    assert_eq!(tooltip(&demo).as_deref(), Some("Landing pod  [J] Journal"));
}

#[test]
fn nearby_tooltip_fades_out_after_leaving_and_hud_tracks_deliveries() {
    let mut demo = demo_with_seed(Some(4.0));
    tick(&mut demo, None);
    let nodes = numbers(&demo, "nodes");
    let builds = numbers(&demo, "builds");
    let occupied: Vec<_> = (0..225)
        .filter(|&i| nodes[i] != 0.0 || builds[i] != 0.0)
        .collect();
    let mut route = None;
    for &cell in &occupied {
        for (dx, dz, key) in [(1, 0, "D"), (-1, 0, "A"), (0, 1, "S"), (0, -1, "W")] {
            let x = cell as i32 % 15 - 7;
            let z = cell as i32 / 15 - 7;
            let end_x = x + dx * 2;
            let end_z = z + dz * 2;
            if (-7..=7).contains(&end_x)
                && (-7..=7).contains(&end_z)
                && occupied.iter().all(|&other| {
                    let ox = other as i32 % 15 - 7;
                    let oz = other as i32 / 15 - 7;
                    (end_x - ox).pow(2) + (end_z - oz).pow(2) > 2
                })
            {
                route = Some((x, z, key));
                break;
            }
        }
        if route.is_some() {
            break;
        }
    }
    let (x, z, key) = route.expect("test layout needs a deposit with room to step away");
    move_cursor(&mut demo, x, z);
    for _ in 0..40 {
        tick(&mut demo, None);
    }
    assert!(number(&demo, "tooltip_alpha") > 0.99);
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1280., 800.])
        .unwrap();
    assert!(frame.element("nearby-tooltip").unwrap().widget.text_color[3] > 0.99);
    press(&mut demo, key);
    press(&mut demo, key);
    let fading = number(&demo, "tooltip_alpha");
    assert!(
        fading > 0.0 && fading < 1.0,
        "label must fade over several frames"
    );
    for _ in 0..25 {
        tick(&mut demo, None);
    }
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1280., 800.])
        .unwrap();
    assert!(frame.element("nearby-tooltip").is_none());
    let delivered = numbers(&demo, "counts")[20] as i32;
    assert_eq!(
        frame.element("objective-count").unwrap().text,
        format!("{} / 8", delivered.min(8))
    );
    // Removing a machine while an item is moving must cancel its visual safely.
    let cell = builds.iter().position(|b| *b == 2.0).unwrap();
    move_cursor(&mut demo, cell as i32 % 15 - 7, cell as i32 / 15 - 7);
    press(&mut demo, "X");
    for _ in 0..30 {
        tick(&mut demo, None);
    }
    press(&mut demo, "N");
    for _ in 0..30 {
        tick(&mut demo, None);
    }
}

#[test]
fn r_rotates_an_existing_machine_and_its_output_without_replacing_it() {
    let mut demo = demo_with_seed(Some(4.0));
    tick(&mut demo, None);
    let nodes = numbers(&demo, "nodes");
    let builds = numbers(&demo, "builds");
    let cell = (0..195)
        .find(|&i| {
            nodes[i] != 0.
                && builds[i] == 0.
                && [i + 15, i + 30]
                    .iter()
                    .all(|&next| nodes[next] == 0. && builds[next] == 0.)
        })
        .expect("a free deposit with two empty tiles to its south");
    let x = cell as i32 % 15 - 7;
    let z = cell as i32 / 15 - 7;
    move_cursor(&mut demo, x, z);
    press(&mut demo, "1");
    press(&mut demo, "Space");
    let capture = demo.instance().capture(&demo.app.world).unwrap();
    let miner = capture
        .objects
        .iter()
        .find(|o| {
            o.name == "machine-miner" && o.transform.translation == [x as f32, 0.08, z as f32]
        })
        .unwrap()
        .id
        .clone();
    for turn in 1..=5 {
        press(&mut demo, "R");
        for _ in 0..36 {
            tick(&mut demo, None);
        }
        let facing = (turn % 4) as f32;
        assert_eq!(numbers(&demo, "facings")[cell], facing);
        let capture = demo.instance().capture(&demo.app.world).unwrap();
        let machine = capture.objects.iter().find(|o| o.id == miner).unwrap();
        assert_eq!(machine.transform.rotation_degrees, [0., -90. * facing, 0.]);
    }
    // The fifth turn faces south. The model's +X outlet and simulation agree.
    move_cursor(&mut demo, x, z + 1);
    press(&mut demo, "2");
    press(&mut demo, "Space");
    move_cursor(&mut demo, x, z + 2);
    press(&mut demo, "4");
    press(&mut demo, "Space");
    let pole = builds.iter().rposition(|&kind| kind == 9.).unwrap();
    tick_keys(&mut demo, &["Ctrl", "3"]);
    tick(&mut demo, None);
    press(&mut demo, "3");
    move_cursor(&mut demo, x, z);
    press(&mut demo, "Space");
    move_cursor(&mut demo, pole as i32 % 15 - 7, pole as i32 / 15 - 7);
    press(&mut demo, "Space");
    for _ in 0..140 {
        tick(&mut demo, None);
    }
    assert!(
        numbers(&demo, "counts")[nodes[cell] as usize] > 0.,
        "the rotated miner must feed the new southern conveyor and storage"
    );
}

#[test]
fn ctrl_r_eases_and_queues_quarter_turns_without_rotating_machines() {
    let mut demo = demo_with_seed(Some(4.0));
    // Fill the production pipeline before checking that camera motion leaves it running.
    for _ in 0..360 {
        tick(&mut demo, None);
    }
    let produced = numbers(&demo, "counts")[20];
    move_cursor(&mut demo, 0, 0);
    settle(&mut demo, 40);
    let original = demo.instance().global_transforms(&demo.app.world).unwrap()["camera"];
    let facings = numbers(&demo, "facings");
    let direction = number(&demo, "direction");
    tick_keys(&mut demo, &["Ctrl", "R"]);
    for _ in 0..12 {
        tick_keys(&mut demo, &["Ctrl", "R"]);
    }
    let midway = demo.instance().global_transforms(&demo.app.world).unwrap()["camera"];
    assert!(!midway.abs_diff_eq(original, 0.01));
    let t = number(&demo, "camera_progress");
    assert!(
        t > 0.2 && t < 0.8,
        "camera must animate across multiple frames"
    );
    assert_eq!(
        number(&demo, "camera_pending"),
        0.,
        "holding R must not repeat"
    );
    tick_keys(&mut demo, &["Ctrl"]);
    tick_keys(&mut demo, &["Ctrl", "R"]);
    assert_eq!(
        number(&demo, "camera_pending"),
        1.,
        "a second press queues a turn"
    );
    for _ in 0..75 {
        tick_keys(&mut demo, &["Ctrl", "R"]);
    }
    assert_eq!(number(&demo, "camera_heading"), 180.);
    assert_eq!(number(&demo, "camera_progress"), 1.);
    assert_eq!(number(&demo, "camera_pending"), 0.);
    assert_eq!(numbers(&demo, "facings"), facings);
    assert_eq!(number(&demo, "direction"), direction);
    let camera = demo.instance().global_transforms(&demo.app.world).unwrap()["camera"];
    let p = camera.w_axis;
    assert!((p.x + 20.).abs() < 0.001 && (p.z + 20.).abs() < 0.001 && (p.y - 20.).abs() < 0.001);
    tick(&mut demo, None);
    press(&mut demo, "D");
    assert_eq!(
        number(&demo, "cursor_x"),
        -1.,
        "movement follows the new view"
    );
    for _ in 0..2 {
        tick_keys(&mut demo, &["Ctrl"]);
        tick_keys(&mut demo, &["Ctrl", "R"]);
    }
    for _ in 0..75 {
        tick(&mut demo, None);
    }
    assert_eq!(number(&demo, "camera_heading"), 0.);
    let camera = demo.instance().global_transforms(&demo.app.world).unwrap()["camera"];
    assert!(
        camera.abs_diff_eq(
            glam::Mat4::from_translation(glam::Vec3::new(-1., 0., 0.)) * original,
            0.0001
        ),
        "four turns preserve orientation while the camera follows the moved player"
    );
    assert!(
        numbers(&demo, "counts")[20] > produced,
        "production continues during orbits"
    );
}

fn ui_point(demo: &SceneDemo, id: &str) -> [f32; 2] {
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap();
    let rect = frame
        .element(id)
        .unwrap_or_else(|| panic!("missing {id}"))
        .rect;
    [
        rect.min[0] + rect.size[0] * 0.5,
        rect.min[1] + rect.size[1] * 0.5,
    ]
}

fn ui_event(demo: &mut SceneDemo, input: bozzard_scene::middleware::ui::Input) {
    demo.ui_input(bozzard_scene::Layer::ThreeD, [1080., 600.], input)
        .unwrap();
}

fn click_widget(demo: &mut SceneDemo, id: &str) {
    use bozzard_scene::middleware::ui::Input;
    let point = ui_point(demo, id);
    ui_event(demo, Input::PointerDown(point));
    ui_event(demo, Input::PointerUp(point));
    tick(demo, None);
}

fn inventory(demo: &SceneDemo, cell: usize) -> Vec<(f32, f32)> {
    (0..16)
        .map(|slot| {
            let page = slot / 4;
            let offset = cell * 4 + slot % 4;
            (
                numbers(demo, &format!("storage_kinds_{page}"))[offset],
                numbers(demo, &format!("storage_amounts_{page}"))[offset],
            )
        })
        .collect()
}

#[test]
fn storage_grid_animates_and_drags_splits_deletes_actual_container_items() {
    use bozzard_scene::middleware::ui::{Input, Runtime};
    let mut demo = demo_with_seed(Some(4.));
    for _ in 0..620 {
        tick(&mut demo, None);
    }
    let builds = numbers(&demo, "builds");
    let storage = builds.iter().position(|v| *v == 4.).unwrap();
    let assembler = builds.iter().position(|v| *v == 5.).unwrap();
    // Stop new deliveries so exact conservation assertions remain deterministic.
    move_cursor(
        &mut demo,
        assembler as i32 % 15 - 7,
        assembler as i32 / 15 - 7,
    );
    press(&mut demo, "X");
    assert!((storage as i32 % 15 - assembler as i32 % 15).abs() <= 1);
    let amount = inventory(&demo, storage)[0].1;
    assert!(amount >= 3.);
    assert_eq!(amount, numbers(&demo, "counts")[20]);
    // Adjacent to storage is enough; there need not be a machine under the cursor.
    press(&mut demo, "E");
    assert_eq!(number(&demo, "storage_cell"), storage as f32);
    assert!(number(&demo, "storage_alpha") > 0. && number(&demo, "storage_alpha") < 1.);
    let panel = &demo.app.world.resource::<Runtime>().unwrap().widgets["storage-panel"];
    assert!(panel.offset.unwrap()[1] > 0.);
    for _ in 0..16 {
        tick(&mut demo, None);
    }
    let cursor = [number(&demo, "cursor_x"), number(&demo, "cursor_z")];
    press(&mut demo, "D");
    press(&mut demo, "X");
    assert_eq!(
        [number(&demo, "cursor_x"), number(&demo, "cursor_z")],
        cursor
    );
    let from = ui_point(&demo, "inventory-slot-0");
    let to = ui_point(&demo, "inventory-slot-3");
    ui_event(&mut demo, Input::PointerDown(from));
    tick(&mut demo, None);
    assert_eq!(number(&demo, "storage_drag"), 0.);
    ui_event(&mut demo, Input::PointerMove(to));
    ui_event(&mut demo, Input::PointerUp(to));
    tick(&mut demo, None);
    assert_eq!(inventory(&demo, storage)[0], (0., 0.));
    assert_eq!(inventory(&demo, storage)[3], (20., amount));
    assert_eq!(numbers(&demo, "counts")[20], amount);
    // A complete drag between ticks still works, and dropping outside cancels it.
    ui_event(&mut demo, Input::PointerDown(to));
    ui_event(&mut demo, Input::PointerUp([4., 4.]));
    tick(&mut demo, None);
    assert_eq!(inventory(&demo, storage)[3], (20., amount));
    ui_event(&mut demo, Input::PointerDown(to));
    tick(&mut demo, None);
    ui_event(&mut demo, Input::CancelPointer);
    tick(&mut demo, None);
    assert_eq!(number(&demo, "storage_drag"), -1.);
    ui_event(&mut demo, Input::SecondaryDown(to));
    tick(&mut demo, None);
    assert!(number(&demo, "storage_menu_alpha") > 0. && number(&demo, "storage_menu_alpha") < 1.);
    let menu = ui_point(&demo, "stack-menu");
    assert!(
        menu[0] >= to[0],
        "menu opens at the pointer, clamped inside the viewport"
    );
    click_widget(&mut demo, "stack-split");
    let stacks = inventory(&demo, storage);
    assert_eq!(stacks[0], (20., (amount / 2.).floor()));
    assert_eq!(stacks[3], (20., (amount / 2.).ceil()));
    assert_eq!(numbers(&demo, "counts")[20], amount);
    for _ in 0..8 {
        tick(&mut demo, None);
    }
    ui_event(&mut demo, Input::SecondaryDown(to));
    tick(&mut demo, None);
    click_widget(&mut demo, "stack-delete");
    assert!(
        inventory(&demo, storage)
            .iter()
            .all(|(_, amount)| *amount == 0.)
    );
    assert_eq!(numbers(&demo, "counts")[20], 0.);
    // The authored E shortcut closes through UI events even while pointer input is captured.
    ui_event(&mut demo, Input::Key("E".into()));
    tick(&mut demo, None);
    assert!(number(&demo, "storage_alpha") > 0. && number(&demo, "storage_alpha") < 1.);
    assert!(
        !demo.app.world.resource::<Runtime>().unwrap().widgets["storage-panel"]
            .enabled
            .unwrap()
    );
    for _ in 0..14 {
        tick(&mut demo, None);
    }
    assert_eq!(number(&demo, "storage_alpha"), 0.);
    assert!(
        !demo.app.world.resource::<Runtime>().unwrap().widgets["storage-overlay"]
            .visible
            .unwrap()
    );
    move_cursor(&mut demo, storage as i32 % 15 - 7, storage as i32 / 15 - 7);
    press(&mut demo, "E");
    assert_eq!(
        number(&demo, "storage_cell"),
        storage as f32,
        "standing on storage also opens it"
    );
}

fn script_fixture(setup: &str) -> SceneDemo {
    let mut demo = demo_with_seed(Some(4.));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../earth-factory/scenes/scripts/earth_factory.rs");
    let source = std::fs::read_to_string(path)
        .unwrap()
        .replace("fn on_start(me)", "fn factory_start(me)");
    let source = format!(
        "{source}\nfn on_start(me) {{ factory_start(me); let cell = -1; let builds = get_scene_list(\"builds\"); for i in 0..225 {{ if builds[i] == 4.0 {{ cell = i; break; }} }} {setup} }}"
    );
    demo.with_instance(|instance, _| instance.register_script("earth-factory".into(), source))
        .unwrap();
    tick(&mut demo, None);
    demo
}

fn storage_fixture(setup: &str) -> (SceneDemo, usize) {
    let demo = script_fixture(setup);
    let cell = numbers(&demo, "builds")
        .iter()
        .position(|v| *v == 4.)
        .unwrap();
    (demo, cell)
}

#[test]
fn storage_stacks_swap_merge_with_overflow_and_remain_container_local() {
    use bozzard_scene::middleware::ui::Input;
    let (mut demo, cell) = storage_fixture(
        r#"
        let inventory = grid::empty_numbers(32);
        inventory[0] = 20.0; inventory[1] = 7.0;
        inventory[2] = 11.0; inventory[3] = 98.0;
        inventory[4] = 11.0; inventory[5] = 9.0;
        inventory[6] = 12.0; inventory[7] = 1.0;
        inventory::storage_write(cell, inventory);
        let counts = get_scene_list("counts");
        counts[20] = 7.0; counts[11] = 107.0; counts[12] = 1.0;
        set_scene_list("counts", counts);
        set_scene_variable("cursor_x", grid::cell_x(cell).to_float());
        set_scene_variable("cursor_z", grid::cell_z(cell).to_float());
    "#,
    );
    press(&mut demo, "E");
    for _ in 0..15 {
        tick(&mut demo, None);
    }
    let drag = |demo: &mut SceneDemo, from: usize, to: usize| {
        let a = ui_point(demo, &format!("inventory-slot-{from}"));
        let b = ui_point(demo, &format!("inventory-slot-{to}"));
        ui_event(demo, Input::PointerDown(a));
        ui_event(demo, Input::PointerMove(b));
        ui_event(demo, Input::PointerUp(b));
        tick(demo, None);
    };
    drag(&mut demo, 2, 1);
    assert_eq!(inventory(&demo, cell)[1], (11., 100.));
    assert_eq!(
        inventory(&demo, cell)[2],
        (11., 7.),
        "overflow remains in the source"
    );
    drag(&mut demo, 0, 3);
    assert_eq!(inventory(&demo, cell)[0], (12., 1.));
    assert_eq!(inventory(&demo, cell)[3], (20., 7.));
    let point = ui_point(&demo, "inventory-slot-0");
    ui_event(&mut demo, Input::SecondaryDown(point));
    tick(&mut demo, None);
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        !frame.element("stack-split").unwrap().enabled,
        "one item cannot split"
    );
    click_widget(&mut demo, "storage-close");
    for _ in 0..15 {
        tick(&mut demo, None);
    }
    move_cursor(&mut demo, 7, 7);
    press(&mut demo, "4");
    press(&mut demo, "Space");
    press(&mut demo, "E");
    assert_eq!(number(&demo, "storage_cell"), 224.);
    assert!(
        inventory(&demo, 224)
            .iter()
            .all(|(_, amount)| *amount == 0.),
        "a new container does not share the first one's contents"
    );
    assert_eq!(inventory(&demo, cell)[1], (11., 100.));
    assert_eq!(numbers(&demo, "counts")[11], 107.);
}

#[test]
fn full_storage_blocks_delivery_until_a_stack_is_deleted_while_factory_keeps_running() {
    use bozzard_scene::middleware::ui::Input;
    let (mut demo, cell) = storage_fixture(
        r#"
        let inventory = grid::empty_numbers(32);
        for i in 0..16 { inventory[i * 2] = 20.0; inventory[i * 2 + 1] = 100.0; }
        inventory::storage_write(cell, inventory);
        let counts = get_scene_list("counts"); counts[20] = 1600.0;
        set_scene_list("counts", counts);
        set_scene_variable("cursor_x", grid::cell_x(cell).to_float());
        set_scene_variable("cursor_z", grid::cell_z(cell).to_float());
    "#,
    );
    for _ in 0..360 {
        tick(&mut demo, None);
    }
    assert_eq!(numbers(&demo, "counts")[20], 1600.);
    assert!(
        numbers(&demo, "items").contains(&20.),
        "finished parts wait upstream of a full container"
    );
    press(&mut demo, "E");
    for _ in 0..15 {
        tick(&mut demo, None);
    }
    let point = ui_point(&demo, "inventory-slot-0");
    ui_event(&mut demo, Input::SecondaryDown(point));
    tick(&mut demo, None);
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        !frame.element("stack-split").unwrap().enabled,
        "splitting requires a free slot"
    );
    click_widget(&mut demo, "stack-delete");
    for _ in 0..80 {
        tick(&mut demo, None);
    }
    let delivered = numbers(&demo, "counts")[20];
    assert!(
        delivered > 0. && delivered < 10.,
        "delivery resumes while the inventory stays open"
    );
    assert_eq!(
        inventory(&demo, cell)
            .iter()
            .map(|(_, amount)| *amount)
            .sum::<f32>(),
        delivered
    );
    assert_eq!(number(&demo, "storage_alpha"), 1.);
    click_widget(&mut demo, "storage-close");
    for _ in 0..15 {
        tick(&mut demo, None);
    }
    press(&mut demo, "X");
    assert_eq!(
        numbers(&demo, "counts")[20],
        0.,
        "demolishing storage removes its contents from totals"
    );
    press(&mut demo, "4");
    press(&mut demo, "Space");
    assert!(
        inventory(&demo, cell)
            .iter()
            .all(|(_, amount)| *amount == 0.),
        "rebuilding storage cannot resurrect deleted contents"
    );
}

#[test]
fn idle_item_effects_do_no_work_and_clear_the_final_motion_once() {
    let mut demo = stellar_fixture("");
    factory_code(
        &mut demo,
        r#"import "factory-item_fx" as item_fx; item_fx::retire_previous(); item_fx::write_motions([]);"#,
        2,
    );
    let commands = |demo: &SceneDemo| {
        demo.app
            .world
            .resource::<bozzard_scene::ScriptRuntime>()
            .unwrap()
            .stats
            .attachments[&("controller".into(), 0)]
            .commands
    };
    assert_eq!(
        commands(&demo),
        0,
        "idle effects must not rebuild empty pages"
    );
    factory_code(
        &mut demo,
        r#"
        import "factory-item_fx" as item_fx;
        let handle=spawn_prefab("item",[0.0,0.5,0.0]);
        item_fx::write_motions([[0,1,handle,0.5,0.5,true]]);
        if get_scene_list_item("motion_visuals",867)=="" {throw "missing active motion";}
        item_fx::retire_previous();
        if get_scene_list("item_pool").len()!=1 {throw "consumed item was not recycled";}
        item_fx::write_motions([]);
        for name in ["motion_visuals","motion_from","motion_to","motion_from_y","motion_to_y","retired_visuals"] {
            if get_scene_list_item(name,0)!="" {throw "stale animation page";}
        }
        item_fx::retire_previous();
        if get_scene_list("item_pool").len()!=1 {throw "consumed item recycled twice";}
        if get_scene_list_item("motion_visuals",867)!="" {throw "stale active motion";}
        "#,
        1,
    );
    factory_code(
        &mut demo,
        r#"import "factory-item_fx" as item_fx; item_fx::retire_previous(); item_fx::write_motions([]);"#,
        2,
    );
    assert_eq!(commands(&demo), 0);
}

#[test]
fn steady_production_reuses_item_visuals_without_changing_scene_membership() {
    let mut demo = demo_with_seed(Some(4.));
    for _ in 0..600 {
        tick(&mut demo, None);
    }
    let ids = |demo: &SceneDemo| -> std::collections::BTreeSet<String> {
        demo.instance()
            .document()
            .objects
            .iter()
            .filter(|o| o.name == "Moving item")
            .map(|o| o.id.clone())
            .collect()
    };
    let before = ids(&demo);
    let produced = numbers(&demo, "counts")[20];
    assert!(!before.is_empty());
    for _ in 0..180 {
        tick(&mut demo, None);
    }
    assert!(numbers(&demo, "counts")[20] > produced);
    assert_eq!(
        ids(&demo),
        before,
        "steady production must not allocate new prefab hierarchies"
    );
}

#[test]
fn material_models_follow_pooled_items_and_rest_on_the_belt_deck() {
    use bozzard_scene::{Drawable, Mesh, Transform};
    let mut demo = buffer_layout(
        "[[112,2,0,11,1],[113,4,0,0,0]]",
        "simulation::factory_step();",
    );
    let item_ids = |demo: &SceneDemo| -> Vec<String> {
        demo.instance()
            .document()
            .objects
            .iter()
            .filter(|object| object.name == "Moving item")
            .map(|object| object.id.clone())
            .collect()
    };
    let ids = item_ids(&demo);
    assert_eq!(ids.len(), 1);
    let entity = demo.instance().entity(&ids[0]).unwrap();
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/factory-materials/manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path).unwrap()).unwrap();
    for item in manifest["items"].as_array().unwrap() {
        if item["fluid_sample"].as_bool().unwrap() {
            continue;
        }
        let kind = item["kind"].as_u64().unwrap();
        factory_code(
            &mut demo,
            r#"
            // The previous transfer finished in storage; the next beat returns
            // its visual to the pool. Make room for the next material sample.
            inventory::storage_write(113,grid::empty_numbers(32));
            simulation::factory_step();
        "#,
            1,
        );
        factory_code(
            &mut demo,
            &format!(
                r#"
            let items=get_scene_list("items");items[112]={kind}.0;set_scene_list("items",items);
            let amounts=get_scene_list("item_amounts");amounts[112]=1.0;set_scene_list("item_amounts",amounts);
            simulation::factory_step();
        "#
            ),
            1,
        );
        assert_eq!(
            item_ids(&demo),
            ids,
            "kind {kind} must reuse the item entity"
        );
        let drawable = demo.app.world.get::<Drawable>(entity).unwrap();
        assert_eq!(drawable.mesh, Mesh::Asset(format!("item-model-{kind}")));
        assert_eq!(drawable.color, [1., 1., 1.]);
        let transform = demo.app.world.get::<Transform>(entity).unwrap();
        assert_eq!(transform.scale, [1., 1., 1.]);
        let bottom = transform.translation[1] + item["bounds"][0][1].as_f64().unwrap() as f32;
        assert!(
            (bottom - 0.424).abs() < 0.00001,
            "kind {kind} floats or clips into the belt: {bottom}"
        );
    }
}

fn controller_value(demo: &SceneDemo, name: &str) -> BlackboardValue {
    demo.app
        .world
        .resource::<BlueprintRuntime>()
        .unwrap()
        .object_blackboard("controller")
        .unwrap()[name]
        .clone()
}

fn controller_number(demo: &SceneDemo, name: &str) -> f32 {
    let BlackboardValue::Scalar(Value::Number(value)) = controller_value(demo, name) else {
        panic!("{name}")
    };
    value
}

fn controller_numbers(demo: &SceneDemo, name: &str) -> Vec<f32> {
    let BlackboardValue::List { values, .. } = controller_value(demo, name) else {
        panic!("{name}")
    };
    values.iter().map(|v| v.number().unwrap()).collect()
}

fn settle(demo: &mut SceneDemo, ticks: usize) {
    for _ in 0..ticks {
        tick(demo, None);
    }
}

#[test]
fn action_bars_switch_by_ctrl_chord_and_remember_each_selection() {
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    assert_eq!(controller_number(&demo, "bar"), 1.);
    tick_keys(&mut demo, &["Ctrl", "2"]);
    assert_eq!(controller_number(&demo, "bar"), 2.);
    assert_eq!(number(&demo, "selected"), 2.);
    tick(&mut demo, None);
    press(&mut demo, "2");
    assert_eq!(number(&demo, "selected"), 4.);
    tick_keys(&mut demo, &["Ctrl", "3"]);
    assert_eq!(number(&demo, "selected"), 6.);
    tick(&mut demo, None);
    tick_keys(&mut demo, &["Ctrl", "2"]);
    assert_eq!(number(&demo, "selected"), 4.);
    tick(&mut demo, None);
    tick_keys(&mut demo, &["Ctrl", "4"]);
    assert_eq!(controller_number(&demo, "bar"), 4.);
    assert_eq!(number(&demo, "selected"), 16.);
    tick(&mut demo, None);
    tick_keys(&mut demo, &["Ctrl", "5"]);
    assert_eq!(controller_number(&demo, "bar"), 5.);
    assert_eq!(number(&demo, "selected"), 24.);
    tick(&mut demo, None);
    tick_keys(&mut demo, &["Ctrl", "6"]);
    assert_eq!(controller_number(&demo, "bar"), 6.);
    assert_eq!(number(&demo, "selected"), 30.);
    press(&mut demo, "8");
    assert_eq!(number(&demo, "selected"), 39.);
    tick_keys(&mut demo, &["Ctrl", "7"]);
    assert_eq!(number(&demo, "selected"), 36.);
    tick(&mut demo, None);
    tick_keys(&mut demo, &["Ctrl", "8"]);
    assert_eq!(
        number(&demo, "selected"),
        36.,
        "unsupported chords cannot select tools"
    );
    press(&mut demo, "Space");
    assert!(
        numbers(&demo, "builds").iter().all(|v| *v == 0.),
        "locked tools cannot be built"
    );
}

#[test]
fn machine_rotation_lifts_then_turns_then_lands_and_queues_presses() {
    let mut demo = demo_with_seed(Some(4.));
    tick(&mut demo, None);
    let cell = numbers(&demo, "builds")
        .iter()
        .position(|v| *v == 1.)
        .unwrap();
    let x = cell as i32 % 15 - 7;
    let z = cell as i32 / 15 - 7;
    move_cursor(&mut demo, x, z);
    let captured = demo.instance().capture(&demo.app.world).unwrap();
    let id = captured
        .objects
        .iter()
        .find(|o| {
            o.name == "machine-miner" && o.transform.translation == [x as f32, 0.08, z as f32]
        })
        .unwrap()
        .id
        .clone();
    press(&mut demo, "R");
    settle(&mut demo, 5);
    let captured = demo.instance().capture(&demo.app.world).unwrap();
    let machine = captured.objects.iter().find(|o| o.id == id).unwrap();
    assert!(machine.transform.translation[1] > 0.2);
    assert_eq!(
        machine.transform.rotation_degrees[1], 0.,
        "lift happens before rotation"
    );
    assert_eq!(numbers(&demo, "facings")[cell], 0.);
    settle(&mut demo, 12);
    let captured = demo.instance().capture(&demo.app.world).unwrap();
    let machine = captured.objects.iter().find(|o| o.id == id).unwrap();
    assert!(
        machine.transform.rotation_degrees[1] < -10.
            && machine.transform.rotation_degrees[1] > -80.
    );
    press(&mut demo, "R");
    settle(&mut demo, 65);
    let captured = demo.instance().capture(&demo.app.world).unwrap();
    let machine = captured.objects.iter().find(|o| o.id == id).unwrap();
    assert!((machine.transform.translation[1] - 0.08).abs() < 0.0001);
    assert_eq!(numbers(&demo, "facings")[cell], 2.);
    assert_eq!(machine.transform.rotation_degrees[1], -180.);
}

#[test]
fn demolition_clears_buffered_ingredients_and_pending_rotations() {
    let (mut demo, _) = storage_fixture(
        r#"
        let builds = get_scene_list("builds"); let cell = -1;
        for i in 0..225 { if builds[i] == 5.0 { cell = i; break; } }
        for name in ["item_amounts", "input_items", "input_amounts", "assembler_iron", "assembler_copper", "progress", "split_state"] {
            let values = get_scene_list(name); values[cell] = 2.0; set_scene_list(name, values);
        }
        set_scene_variable("cursor_x", grid::cell_x(cell).to_float());
        set_scene_variable("cursor_z", grid::cell_z(cell).to_float());
        building::rotate_selected();
        building::remove_selected();
        set_scene_variable("selected", 5.0); building::place_selected();
    "#,
    );
    let cell = numbers(&demo, "builds")
        .iter()
        .position(|v| *v == 5.)
        .unwrap();
    for name in [
        "item_amounts",
        "input_items",
        "input_amounts",
        "assembler_iron",
        "assembler_copper",
        "progress",
        "split_state",
    ] {
        assert_eq!(numbers(&demo, name)[cell], 0.);
    }
    assert!(controller_numbers(&demo, "rotation_cells").is_empty());
    settle(&mut demo, 40);
}

#[test]
fn journal_pages_block_world_input_and_first_unlock_works_from_gathering() {
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    assert_eq!(controller_number(&demo, "phase"), 0.);
    for (kind, amount) in [(1, 15), (2, 11), (8, 8)] {
        let cell = numbers(&demo, "nodes")
            .iter()
            .position(|v| *v == kind as f32)
            .unwrap();
        move_cursor(&mut demo, cell as i32 % 15 - 7, cell as i32 / 15 - 7);
        for _ in 0..amount * 19 {
            tick(&mut demo, Some("F"));
        }
        tick(&mut demo, None);
    }
    move_cursor(&mut demo, 0, 0);
    press(&mut demo, "J");
    settle(&mut demo, 15);
    let stock = controller_numbers(&demo, "stock");
    tick_keys(&mut demo, &["D", "F", "Space", "Ctrl", "2"]);
    assert_eq!(number(&demo, "cursor_x"), 0.);
    assert_eq!(controller_number(&demo, "bar"), 1.);
    assert_eq!(controller_numbers(&demo, "stock"), stock);
    tick(&mut demo, None);
    click_widget(&mut demo, "journal-deliver");
    assert_eq!(controller_number(&demo, "phase"), 1.);
    click_widget(&mut demo, "journal-tab-2");
    assert_eq!(controller_number(&demo, "journal_page"), 2.);
    click_widget(&mut demo, "journal-recipe-11");
    for _ in 0..2 {
        click_widget(&mut demo, "journal-craft-one");
    }
    click_widget(&mut demo, "journal-recipe-12");
    for _ in 0..2 {
        click_widget(&mut demo, "journal-craft-one");
    }
    assert_eq!(controller_numbers(&demo, "stock")[11], 2.);
    assert_eq!(controller_numbers(&demo, "stock")[12], 2.);
    click_widget(&mut demo, "journal-tab-3");
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        frame
            .element("journal-left-body")
            .unwrap()
            .text
            .contains("0% / Not assembled")
    );
    press(&mut demo, "J");
    settle(&mut demo, 12);
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        frame.element("journal-book").is_none(),
        "closed journal leaves no input blocker"
    );
    press(&mut demo, "D");
    assert_eq!(number(&demo, "cursor_x"), 1.);
}

#[test]
fn neighboring_regions_preserve_factories_inventory_and_seeded_nodes() {
    let (mut demo, storage) = storage_fixture(
        r#"
        let inventory = grid::empty_numbers(32); inventory[0] = 20.0; inventory[1] = 37.0;
        inventory::storage_write(cell, inventory);
        let counts = get_scene_list("counts"); counts[20] = 37.0; set_scene_list("counts", counts);
    "#,
    );
    let before = numbers(&demo, "builds");
    move_cursor(&mut demo, 6, 0);
    assert_eq!(
        controller_numbers(&demo, "visited")[145],
        1.,
        "neighbor appears before crossing"
    );
    press(&mut demo, "D");
    press(&mut demo, "D");
    assert_eq!(number(&demo, "chunk_x"), 1.);
    assert_eq!(number(&demo, "cursor_x"), -7.);
    let nodes = numbers(&demo, "nodes");
    assert!((2..=4).contains(&nodes.iter().filter(|&&kind| kind > 0.).count()));
    let camera = demo.instance().capture(&demo.app.world).unwrap();
    let pan_x = camera
        .objects
        .iter()
        .find(|o| o.id == "camera-rig")
        .unwrap()
        .transform
        .translation[0];
    assert!(
        pan_x > 0. && pan_x < 15.,
        "camera should glide across the seam"
    );
    press(&mut demo, "A");
    assert_eq!(number(&demo, "chunk_x"), 0.);
    assert_eq!(numbers(&demo, "builds"), before);
    assert!(inventory(&demo, storage)[0].1 >= 37.);
    press(&mut demo, "D");
    assert_eq!(
        numbers(&demo, "nodes"),
        nodes,
        "revisiting cannot reroll nodes"
    );
    // Inspect the reset tick itself: the next tick can discover a fresh neighbor
    // when the new demonstration happens to start near an edge.
    tick(&mut demo, Some("N"));
    assert_eq!(number(&demo, "chunk_x"), 0.);
    assert_eq!(
        controller_numbers(&demo, "visited")
            .iter()
            .filter(|v| **v != 0.)
            .count(),
        1
    );
}

#[test]
fn player_camera_follows_steps_seams_and_reversals_while_orbiting() {
    use bozzard_scene::Transform;
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    let position = |demo: &SceneDemo| {
        demo.app
            .world
            .get::<Transform>(demo.instance().entity("camera-rig").unwrap())
            .unwrap()
            .translation
    };
    assert_eq!(position(&demo), [0., 0., 0.]);
    press(&mut demo, "D");
    assert_eq!(number(&demo, "chunk_x"), 0.);
    assert!(
        position(&demo)[0] > 0. && position(&demo)[0] < 1.,
        "camera follows an ordinary step smoothly"
    );
    settle(&mut demo, 40);
    assert_eq!(position(&demo), [1., 0., 0.]);
    move_cursor(&mut demo, 7, 0);
    settle(&mut demo, 40);
    assert_eq!(position(&demo), [7., 0., 0.]);
    press(&mut demo, "D");
    assert_eq!(
        (number(&demo, "chunk_x"), number(&demo, "cursor_x")),
        (1., -7.)
    );
    assert!(
        position(&demo)[0] > 7. && position(&demo)[0] < 8.,
        "seam target is the player's tile, rather than region center 15"
    );
    settle(&mut demo, 40);
    assert_eq!(position(&demo), [8., 0., 0.]);
    press(&mut demo, "A");
    assert!(
        position(&demo)[0] > 7. && position(&demo)[0] < 8.,
        "reversal eases from the existing camera position"
    );
    settle(&mut demo, 40);
    assert_eq!(position(&demo), [7., 0., 0.]);
    press(&mut demo, "D");
    tick_keys(&mut demo, &["Ctrl", "R"]);
    settle(&mut demo, 40);
    assert_eq!(position(&demo), [8., 0., 0.]);
    assert_eq!(number(&demo, "camera_heading"), 90.);
    assert!(
        number(&demo, "ticks") > 0.,
        "production continues while following and orbiting"
    );
    tick(&mut demo, Some("N"));
    settle(&mut demo, 40);
    assert_eq!(
        position(&demo),
        [0., 0., 0.],
        "a new world cancels the previous player target"
    );
}

#[test]
fn player_camera_damping_is_frame_rate_independent_and_load_centers_on_the_player() {
    use bozzard_scene::Transform;
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    let position = |demo: &SceneDemo| {
        demo.app
            .world
            .get::<Transform>(demo.instance().entity("camera-rig").unwrap())
            .unwrap()
            .translation
    };
    let mut results = Vec::new();
    // Use an exactly equal simulation interval at each rate.
    for fps in [30, 60, 120] {
        factory_code(
            &mut demo,
            &format!(
                r#"
            set_scene_variable("cursor_x",6.0);set_scene_variable("cursor_z",-4.0);
            set_position("camera-rig",[0.0,0.0,0.0]);
            for frame in 0..{} {{ environment::update_camera(1.0/{}.0); }}
        "#,
                fps / 2,
                fps
            ),
            1,
        );
        let p = position(&demo);
        assert!(p[0] > 5.98 && p[0] < 6. && p[2] > -4. && p[2] < -3.98);
        results.push(p);
    }
    for p in &results {
        assert!((p[0] - results[0][0]).abs() < 0.0001 && (p[2] - results[0][2]).abs() < 0.0001);
    }
    factory_code(
        &mut demo,
        r#"
        world::enter_chunk(-1,1);set_scene_variable("cursor_x",-5.0);set_scene_variable("cursor_z",6.0);
        set_position("camera-rig",[100.0,0.0,-100.0]);persistence::restore_view();
    "#,
        1,
    );
    assert_eq!(
        position(&demo),
        [-20., 0., 21.],
        "restore uses saved world coordinates, including negative chunks"
    );
    factory_code(
        &mut demo,
        r#"
        set_scene_variable("cursor_x",-4.0);environment::update_camera(0.0);
    "#,
        1,
    );
    assert_eq!(
        position(&demo),
        [-20., 0., 21.],
        "zero elapsed time cannot move the camera"
    );
}

#[test]
fn chunk_pan_retains_visible_terrain_and_drains_residency_work_over_ticks() {
    use bozzard_scene::Transform;
    let mut demo = script_fixture(
        r#"
        for x in -3..8 { chunks::discover_chunk(x, 0); }
        world::enter_chunk(4, 0);
        set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 0.0);
    "#,
    );
    for _ in 0..45 {
        let before = controller_numbers(&demo, "resident");
        let camera = demo
            .app
            .world
            .get::<Transform>(demo.instance().entity("camera-rig").unwrap())
            .unwrap()
            .translation[0];
        if camera < 30. {
            assert_eq!(
                before[144], 1.,
                "home terrain must survive the start of the pan"
            );
        }
        tick(&mut demo, None);
        let after = controller_numbers(&demo, "resident");
        assert!(
            before.iter().zip(&after).filter(|(a, b)| a != b).count() <= 1,
            "surrounding residency changes are budgeted across ticks"
        );
    }
    assert_eq!(controller_numbers(&demo, "resident")[144], 0.);
    assert_eq!(controller_numbers(&demo, "resident")[148], 1.);
}

#[test]
fn distant_chunks_unload_restore_factory_state_and_follow_the_zoom_footprint() {
    use bozzard_diagnostics::RenderDiagnostics;
    use bozzard_scene::middleware::ui::Input;
    let mut demo = script_fixture(
        r#"
        let inventory = grid::empty_numbers(32); inventory[0] = 20.0; inventory[1] = 37.0;
        inventory::storage_write(cell, inventory);
        let facings = get_scene_list("facings"); facings[cell] = 3.0; set_scene_list("facings", facings);
        let progress = get_scene_list("progress");
        let inputs = get_scene_list("input_items");
        let input_amounts = get_scene_list("input_amounts");
        let buffered = get_scene_list("assembler_iron");
        for i in 0..225 {
            if builds[i] == 3.0 { progress[i] = 1.0; inputs[i] = 1.0; input_amounts[i] = 1.0; }
            if builds[i] == 5.0 { buffered[i] = 1.0; }
        }
        set_scene_list("input_items", inputs); set_scene_list("input_amounts", input_amounts);
        set_scene_list("progress", progress); set_scene_list("assembler_iron", buffered);
        for entry in get_scene_list("machine_cells") {
            let id=power::power_id(entry.to_int());
            if power::power_demand(builds[entry.to_int()].to_int())>0 { power::disconnect_power(id,-1); }
        }
        for x in 1..7 { chunks::discover_chunk(x, 0); }
        world::enter_chunk(4, 0);
        set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 0.0);
    "#,
    );
    // Let the camera finish following this fixture's sixty-tile teleport.
    settle(&mut demo, 60);
    let occupied = |demo: &SceneDemo| {
        controller_numbers(demo, "resident")
            .iter()
            .filter(|&&v| v > 0.)
            .count()
    };
    assert_eq!(occupied(&demo), 5);
    assert_eq!(controller_numbers(&demo, "resident")[144], 0.);
    assert_eq!(
        controller_numbers(&demo, "visited")
            .iter()
            .filter(|&&v| v > 0.)
            .count(),
        7
    );
    let home_storage = |demo: &SceneDemo| {
        let BlackboardValue::List { values, .. } =
            controller_value(demo, "cache_storage_amounts_0")
        else {
            panic!("storage archive");
        };
        values[144].clone()
    };
    let archived = home_storage(&demo);
    // A wider viewport and zoom-out must restore the discovered terrain it exposes.
    let mut diagnostics = RenderDiagnostics::default();
    diagnostics.metrics.counters.viewport_aspect = 4.;
    demo.app.world.insert_resource(diagnostics);
    ui_event(
        &mut demo,
        Input::ScrollAt {
            point: [700., 300.],
            delta: 1000.,
        },
    );
    tick(&mut demo, None);
    assert_eq!(controller_number(&demo, "camera_zoom_target"), 32.);
    settle(&mut demo, 8);
    assert_eq!(occupied(&demo), 7);
    assert_eq!(controller_numbers(&demo, "resident")[144], 1.);
    assert_eq!(home_storage(&demo), archived);
    demo.app
        .world
        .resource_mut::<RenderDiagnostics>()
        .unwrap()
        .metrics
        .counters
        .viewport_aspect = 1.6;
    ui_event(
        &mut demo,
        Input::ScrollAt {
            point: [700., 300.],
            delta: -1000.,
        },
    );
    settle(&mut demo, 50);
    assert_eq!(occupied(&demo), 5);
    assert_eq!(controller_numbers(&demo, "resident")[144], 0.);
    while number(&demo, "chunk_x") > 0. {
        press(&mut demo, "A");
    }
    let builds = numbers(&demo, "builds");
    let storage = builds.iter().position(|&kind| kind == 4.).unwrap();
    let assembler = builds.iter().position(|&kind| kind == 5.).unwrap();
    let smelter = builds.iter().position(|&kind| kind == 3.).unwrap();
    assert_eq!(inventory(&demo, storage)[0], (20., 37.));
    assert_eq!(numbers(&demo, "facings")[storage], 3.);
    assert_eq!(numbers(&demo, "progress")[smelter], 1.);
    assert_eq!(numbers(&demo, "input_items")[smelter], 1.);
    assert_eq!(numbers(&demo, "input_amounts")[smelter], 1.);
    assert_eq!(numbers(&demo, "assembler_iron")[assembler], 1.);
    assert_eq!(home_storage(&demo), archived);
    assert_eq!(controller_numbers(&demo, "resident")[144], 1.);
    factory_code(
        &mut demo,
        r#"
        for entry in get_scene_list("machine_cells") {
            if get_scene_list("builds")[entry.to_int()]==6.0 { power::disconnect_power(power::power_id(entry.to_int()),-1); }
        }
        // Restore the container inlet after checking that its deliberately
        // rotated facing survived unloading; this line feeds from the assembler.
        let builds=get_scene_list("builds"); let facings=get_scene_list("facings");
        let facing=0.0; for i in 0..225 { if builds[i]==5.0 { facing=facings[i]; } }
        for i in 0..225 { if builds[i]==4.0 { facings[i]=facing; } }
        set_scene_list("facings",facings);
        power::wire_demo(); power::update_power();
    "#,
        1,
    );
    restore_factory_update(&mut demo);
    settle(&mut demo, 620);
    assert!(
        numbers(&demo, "counts")[20] > 0.,
        "restored factory must resume production"
    );
    tick(&mut demo, Some("N"));
    assert_eq!(occupied(&demo), 1);
    assert_eq!(
        controller_numbers(&demo, "visited")
            .iter()
            .filter(|&&v| v > 0.)
            .count(),
        1
    );
}

#[test]
fn exploration_stops_after_eight_chunks_in_each_cardinal_direction() {
    // Starting close to each outer border avoids thousands of repeated input ticks;
    // normal traversal and restoration are independently covered above.
    for (cx, cz, x, z, key, field) in [
        (8, 0, 7, 0, "D", "chunk_x"),
        (-8, 0, -7, 0, "A", "chunk_x"),
        (0, 8, 0, 7, "S", "chunk_z"),
        (0, -8, 0, -7, "W", "chunk_z"),
    ] {
        let mut demo = script_fixture(&format!(
            r#"
            world::enter_chunk({cx}, {cz});
            set_scene_variable("cursor_x", {x}.0);
            set_scene_variable("cursor_z", {z}.0);
        "#
        ));
        let before = number(&demo, field);
        press(&mut demo, key);
        assert_eq!(number(&demo, field), before);
        assert_eq!(number(&demo, "cursor_x"), x as f32);
        assert_eq!(number(&demo, "cursor_z"), z as f32);
        assert_eq!(controller_numbers(&demo, "visited").len(), 289);
    }
}

#[test]
fn storage_transfers_to_the_backpack_once_and_updates_totals() {
    let (mut demo, cell) = storage_fixture(
        r#"
        let inventory = grid::empty_numbers(32); inventory[0] = 20.0; inventory[1] = 37.0;
        inventory::storage_write(cell, inventory);
        let counts = get_scene_list("counts"); counts[20] = 37.0; set_scene_list("counts", counts);
        set_scene_variable("cursor_x", grid::cell_x(cell).to_float());
        set_scene_variable("cursor_z", grid::cell_z(cell).to_float());
    "#,
    );
    press(&mut demo, "E");
    settle(&mut demo, 14);
    click_widget(&mut demo, "storage-take");
    assert_eq!(controller_numbers(&demo, "stock")[20], 37.);
    assert_eq!(numbers(&demo, "counts")[20], 0.);
    assert!(
        inventory(&demo, cell)
            .iter()
            .all(|(_, amount)| *amount == 0.)
    );
    click_widget(&mut demo, "storage-take");
    assert_eq!(controller_numbers(&demo, "stock")[20], 37.);
}

fn collection_fixture(kind: u32, item: u32, setup: &str) -> SceneDemo {
    script_fixture(&format!(
        r#"
        for i in 0..225 {{
            if get_scene_list("builds")[i] != 0.0 {{
                set_scene_variable("cursor_x", grid::cell_x(i).to_float());
                set_scene_variable("cursor_z", grid::cell_z(i).to_float());
                building::remove_selected();
            }}
        }}
        set_scene_variable("cursor_x", 7.0);
        set_scene_variable("cursor_z", 7.0);
        let nodes = get_scene_list("nodes"); nodes[224] = {node}.0; set_scene_list("nodes", nodes);
        set_scene_variable("selected", {kind}.0); set_scene_variable("direction", 0.0);
        building::place_selected();
        power::power_register(power::power_id(0),6); power::power_register(power::power_id(1),9);
        power::connect_power(power::power_id(0),power::power_id(1)); power::connect_power(power::power_id(1),power::power_id(224));
        if {kind} == 5 {{ let recipes = get_object_list("recipes"); recipes[224] = 20.0; set_object_list("recipes",recipes); }}
        let item_list = if {kind} == 3 && {item} < 10 {{ "input_items" }} else {{ "items" }};
        let amount_list = if item_list == "items" {{ "item_amounts" }} else {{ "input_amounts" }};
        let items = get_scene_list(item_list); items[224] = {item}.0; set_scene_list(item_list, items);
        let amounts = get_scene_list(amount_list); amounts[224] = 1.0; set_scene_list(amount_list, amounts);
        simulation::factory_step();
        {setup}
        "#,
        node = if kind == 1 { item } else { 0 },
    ))
}

#[test]
fn e_collects_machine_output_once_and_recycles_its_visual() {
    for (kind, item) in [(1, 1), (1, 2), (3, 11), (3, 12), (5, 20)] {
        let mut demo = collection_fixture(kind, item, "");
        assert_eq!(numbers(&demo, "builds")[224], kind as f32);
        assert_eq!(numbers(&demo, "items")[224], item as f32);
        let counts = numbers(&demo, "counts");
        let before = demo
            .instance()
            .view(&demo.app.world, bozzard_scene::Layer::ThreeD, 1.)
            .unwrap()
            .objects
            .len();
        press(&mut demo, "E");
        if kind == 5 {
            click_widget(&mut demo, "assembler-take");
        }
        assert_eq!(controller_numbers(&demo, "stock")[item as usize], 1.);
        assert_eq!(numbers(&demo, "items")[224], 0.);
        assert_eq!(
            numbers(&demo, "counts"),
            counts,
            "pickup is not a storage delivery"
        );
        let after = demo
            .instance()
            .view(&demo.app.world, bozzard_scene::Layer::ThreeD, 1.)
            .unwrap()
            .objects
            .len();
        assert!(after < before, "collected output must stop rendering");
        press(&mut demo, "E");
        assert_eq!(
            controller_numbers(&demo, "stock")[item as usize],
            1.,
            "no duplicate pickup"
        );
    }
}

#[test]
fn collection_preserves_ingredients_and_production_resumes() {
    let mut demo = collection_fixture(3, 1, "");
    press(&mut demo, "E");
    assert_eq!(
        numbers(&demo, "input_items")[224],
        1.,
        "raw ore is still processing"
    );
    assert_eq!(numbers(&demo, "progress")[224], 1.);
    assert_eq!(controller_numbers(&demo, "stock")[1], 0.);
    settle(&mut demo, 20);
    assert_eq!(numbers(&demo, "items")[224], 11.);
    // E opened the empty smelter's loading interface; output appears while it stays open.
    click_widget(&mut demo, "assembler-take");
    assert_eq!(controller_numbers(&demo, "stock")[11], 1.);

    let mut demo = collection_fixture(
        5,
        20,
        r#"
        for name in ["assembler_iron", "assembler_copper"] {
            let values = get_scene_list(name); values[224] = 2.0; set_scene_list(name, values);
        }
    "#,
    );
    press(&mut demo, "E");
    click_widget(&mut demo, "assembler-take");
    assert_eq!(numbers(&demo, "assembler_iron")[224], 2.);
    assert_eq!(numbers(&demo, "assembler_copper")[224], 2.);
    settle(&mut demo, 40);
    assert_eq!(
        numbers(&demo, "items")[224],
        20.,
        "freed output buffer resumes assembly"
    );
    assert_eq!(numbers(&demo, "assembler_iron")[224], 1.);
    click_widget(&mut demo, "assembler-take");
    assert_eq!(controller_numbers(&demo, "stock")[20], 2.);
}

#[test]
fn collection_obeys_range_modals_and_rotation() {
    let mut demo = collection_fixture(3, 11, "");
    press(&mut demo, "J");
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "stock")[11], 0.);
    press(&mut demo, "J");
    settle(&mut demo, 15);
    press(&mut demo, "Escape");
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "stock")[11], 0.);
    press(&mut demo, "Escape");
    press(&mut demo, "R");
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "stock")[11], 0.);
    settle(&mut demo, 40);
    move_cursor(&mut demo, 5, 5);
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "stock")[11], 0.);
    move_cursor(&mut demo, 6, 6);
    press(&mut demo, "E");
    assert_eq!(
        controller_numbers(&demo, "stock")[11],
        1.,
        "diagonal neighbor is reachable"
    );
}

#[test]
fn standing_on_storage_takes_priority_over_an_adjacent_machine() {
    let mut demo = collection_fixture(
        3,
        11,
        r#"
        set_scene_variable("cursor_x", 6.0);
        let nodes = get_scene_list("nodes"); nodes[223] = 0.0; set_scene_list("nodes", nodes);
        set_scene_variable("selected", 4.0); building::place_selected();
    "#,
    );
    press(&mut demo, "E");
    assert_eq!(number(&demo, "storage_cell"), 223.);
    assert_eq!(controller_numbers(&demo, "stock")[11], 0.);
    press(&mut demo, "E");
    settle(&mut demo, 15);
    move_cursor(&mut demo, 7, 7);
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "stock")[11], 1.);
}

fn machine_load(demo: &SceneDemo, cell: usize) -> f32 {
    [
        "item_amounts",
        "input_amounts",
        "assembler_iron",
        "assembler_copper",
    ]
    .iter()
    .map(|name| numbers(demo, name)[cell])
    .sum()
}

// Layout rows: cell, machine kind, direction, output item kind, output quantity.
// Run the real simulation beat directly to test large buffers without thousands
// of unrelated UI/camera ticks. Input/collection still uses ordinary key events.
fn buffer_layout(layout: &str, setup: &str) -> SceneDemo {
    collection_fixture(
        3,
        11,
        &format!(
            r#"
        building::remove_selected();
        for row in {layout} {{
            let cell = row[0];
            let nodes = get_scene_list("nodes"); nodes[cell] = 0.0; set_scene_list("nodes", nodes);
            set_scene_variable("cursor_x", grid::cell_x(cell).to_float());
            set_scene_variable("cursor_z", grid::cell_z(cell).to_float());
            set_scene_variable("selected", row[1].to_float());
            set_scene_variable("direction", row[2].to_float());
            building::place_selected();
            if power::power_demand(row[1]) > 0 {{ power::connect_power(power::power_id(1),power::power_id(cell)); }}
            if row[1] == 5 {{ let recipes = get_object_list("recipes"); recipes[cell] = 20.0; set_object_list("recipes",recipes); }}
            let items = get_scene_list("items"); items[cell] = row[3].to_float(); set_scene_list("items", items);
            let amounts = get_scene_list("item_amounts"); amounts[cell] = row[4].to_float(); set_scene_list("item_amounts", amounts);
        }}
        {setup}
    "#
        ),
    )
}

#[test]
fn miner_stops_at_100_collects_the_whole_stack_and_resumes_without_extra_entities() {
    let mut demo = collection_fixture(1, 1, "");
    factory_code(&mut demo, "simulation::factory_step();", 250);
    restore_factory_update(&mut demo);
    assert_eq!(machine_load(&demo, 224), 100.);
    assert_eq!(numbers(&demo, "item_amounts")[224], 100.);
    let item_entities = demo
        .instance()
        .document()
        .objects
        .iter()
        .filter(|o| o.name == "Moving item")
        .count();
    assert_eq!(
        item_entities, 1,
        "100 buffered items share one visible representative"
    );
    settle(&mut demo, 40);
    assert_eq!(machine_load(&demo, 224), 100.);
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "stock")[1], 100.);
    assert_eq!(machine_load(&demo, 224), 0.);
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "stock")[1], 100.);
    settle(&mut demo, 40);
    assert!(
        numbers(&demo, "item_amounts")[224] > 0.,
        "freed capacity resumes mining"
    );
    assert_eq!(
        demo.instance()
            .document()
            .objects
            .iter()
            .filter(|o| o.name == "Moving item")
            .count(),
        item_entities
    );
}

#[test]
fn smelter_shares_100_spaces_between_ore_and_ingots_and_preserves_processing_on_collection() {
    let mut demo = buffer_layout(
        "[[112, 3, 0, 11, 60], [111, 2, 0, 1, 1]]",
        r#"
        let inputs = get_scene_list("input_items"); inputs[112] = 1.0; set_scene_list("input_items", inputs);
        let amounts = get_scene_list("input_amounts"); amounts[112] = 40.0; set_scene_list("input_amounts", amounts);
        for beat in 0..21 { simulation::factory_step(); }
        set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 0.0);
    "#,
    );
    assert_eq!(machine_load(&demo, 112), 100.);
    assert_eq!(
        numbers(&demo, "item_amounts")[111],
        1.,
        "full smelter rejects incoming ore"
    );
    assert_eq!(numbers(&demo, "item_amounts")[112], 70.);
    assert_eq!(numbers(&demo, "input_amounts")[112], 30.);
    assert_eq!(numbers(&demo, "progress")[112], 1.);
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "stock")[11], 70.);
    assert_eq!(numbers(&demo, "input_amounts")[112], 30.);
    assert_eq!(
        numbers(&demo, "progress")[112],
        1.,
        "collecting output does not restart the recipe"
    );
    settle(&mut demo, 20);
    assert!(
        numbers(&demo, "item_amounts")[111] < 1.,
        "feed resumes after collection"
    );
    assert!(machine_load(&demo, 112) <= 100.);
}

#[test]
fn competing_assembler_inputs_share_capacity_and_leave_room_for_the_missing_ingredient() {
    let mut demo = buffer_layout(
        "[[112, 5, 0, 20, 98], [111, 2, 0, 11, 1], [97, 2, 1, 11, 1], [113, 2, 2, 12, 1]]",
        r#"simulation::factory_step(); set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 0.0);"#,
    );
    assert_eq!(machine_load(&demo, 112), 100.);
    assert_eq!(numbers(&demo, "assembler_iron")[112], 1.);
    assert_eq!(numbers(&demo, "assembler_copper")[112], 1.);
    assert_eq!(
        numbers(&demo, "item_amounts")[111] + numbers(&demo, "item_amounts")[97],
        1.,
        "second iron input cannot consume copper's space"
    );
    assert_eq!(numbers(&demo, "item_amounts")[113], 0.);
    press(&mut demo, "E");
    click_widget(&mut demo, "assembler-take");
    assert_eq!(controller_numbers(&demo, "stock")[20], 98.);
    assert_eq!(machine_load(&demo, 112), 2.);
    settle(&mut demo, 40);
    assert!(numbers(&demo, "item_amounts")[112] > 0.);
    assert!(machine_load(&demo, 112) <= 100.);

    let mut demo = buffer_layout("[[112, 5, 0, 0, 0], [111, 3, 0, 11, 100]]", "");
    factory_code(&mut demo, "simulation::factory_step();", 110);
    assert_eq!(numbers(&demo, "assembler_iron")[112], 99.);
    assert_eq!(numbers(&demo, "item_amounts")[111], 1.);
    assert_eq!(
        machine_load(&demo, 112),
        99.,
        "one space stays available for copper"
    );
}

#[test]
fn logistics_capacities_back_up_and_transfers_conserve_items() {
    for (kind, capacity) in [(2, 1), (7, 10), (8, 10)] {
        let layout = format!("[[111, 3, 0, 20, 5], [112, {kind}, 0, 20, {capacity}]]");
        let demo = buffer_layout(&layout, "for beat in 0..4 { simulation::factory_step(); }");
        assert_eq!(machine_load(&demo, 112), capacity as f32);
        assert_eq!(numbers(&demo, "item_amounts")[111], 5.);
        let demo = buffer_layout(
            &layout,
            r#"
            let nodes = get_scene_list("nodes"); nodes[113] = 0.0; set_scene_list("nodes", nodes);
            set_scene_variable("cursor_x",1.0); set_scene_variable("cursor_z",0.0);
            set_scene_variable("selected",4.0); building::place_selected();
            for beat in 0..12 { simulation::factory_step(); }
            "#,
        );
        assert_eq!(
            numbers(&demo, "item_amounts").iter().sum::<f32>() + numbers(&demo, "counts")[20],
            (capacity + 5) as f32,
        );
        assert_eq!(
            numbers(&demo, "counts")[20],
            if kind == 2 { 6. } else { 12. }
        );
        assert!(machine_load(&demo, 112) <= capacity as f32);
    }
}

#[test]
fn storage_outputs_opposite_its_input_and_preserves_blocked_stock_in_every_rotation() {
    for facing in 0..4 {
        let output = [113, 127, 111, 97][facing];
        let mut demo = buffer_layout(
            &format!("[[112,4,{facing},0,0],[{output},2,{facing},12,1]]"),
            r#"
            let slots=grid::empty_numbers(32); slots[6]=11.0; slots[7]=2.0;
            slots[10]=12.0; slots[11]=1.0; inventory::storage_write(112,slots);
            let counts=get_scene_list("counts"); counts[11]=2.0; counts[12]=1.0; set_scene_list("counts",counts);
            for beat in 0..3 { simulation::factory_step(); }
            "#,
        );
        assert_eq!(inventory(&demo, 112)[3], (11., 2.));
        assert_eq!(numbers(&demo, "counts")[11], 2.);
        for (kind, remaining) in [(11, 1.), (11, 0.), (12, 0.)] {
            factory_code(
                &mut demo,
                &format!(
                    r#"
                    let amounts=get_scene_list("item_amounts"); amounts[{output}]=0.0; set_scene_list("item_amounts",amounts);
                    let items=get_scene_list("items"); items[{output}]=0.0; set_scene_list("items",items);
                    simulation::factory_step(); visuals::animate_items(0.5);
                    let handle=get_scene_list("item_visuals")[{output}];
                    if handle=="" || abs(get_position(handle)[1]-visuals::item_height(2.0,get_scene_list("items")[{output}]))>0.006 {{ throw "storage output misses belt deck"; }}
                    "#
                ),
                1,
            );
            assert_eq!(numbers(&demo, "items")[output], kind as f32);
            assert_eq!(numbers(&demo, "item_amounts")[output], 1.);
            assert_eq!(numbers(&demo, "counts")[kind], remaining);
            assert_eq!(
                machine_load(&demo, 112),
                0.,
                "storage must not duplicate its slots into machine buffers"
            );
        }
        assert_eq!(inventory(&demo, 112), vec![(0., 0.); 16]);
    }
}

#[test]
fn storage_receives_and_sends_once_per_beat_without_forwarding_new_arrivals() {
    let mut demo = buffer_layout(
        "[[111,2,0,12,1],[112,4,0,0,0],[113,2,0,0,0]]",
        r#"
        let slots=grid::empty_numbers(32); slots[0]=11.0; slots[1]=1.0;
        inventory::storage_write(112,slots);
        let counts=get_scene_list("counts"); counts[11]=1.0; set_scene_list("counts",counts);
        simulation::factory_step();
        "#,
    );
    assert_eq!(numbers(&demo, "items")[113], 11.);
    assert_eq!(numbers(&demo, "counts")[11], 0.);
    assert_eq!(inventory(&demo, 112)[1], (12., 1.));
    assert_eq!(numbers(&demo, "counts")[12], 1.);
    factory_code(&mut demo, "simulation::factory_step();", 3);
    assert_eq!(
        inventory(&demo, 112)[1],
        (12., 1.),
        "full output belt must preserve new arrivals"
    );

    let demo = buffer_layout(
        "[[111,2,0,11,1],[112,4,0,0,0],[113,2,0,0,0]]",
        "simulation::factory_step();",
    );
    assert_eq!(numbers(&demo, "item_amounts")[113], 0.);
    assert_eq!(inventory(&demo, 112)[0], (11., 1.));
}

#[test]
fn logistics_splitters_use_three_rotating_outputs_and_skip_blocked_branches() {
    for facing in 0..4 {
        for blocked in [false, true] {
            let demo = buffer_layout(
                &format!(
                    "[[112, 7, {facing}, 20, 9], [97, 4, 3, 0, 0], [113, 4, 0, 0, 0], [127, 4, 1, 0, 0], [111, 4, 2, 0, 0]]"
                ),
                &format!(
                    r#"
                    if {blocked} {{
                        let front = grid::neighbor(112,{facing});
                        let inventory = []; for slot in 0..16 {{ inventory.push(20.0); inventory.push(100.0); }} inventory::storage_write(front,inventory);
                    }}
                    for beat in 0..9 {{ simulation::factory_step(); }}
                    let totals = [];
                    for offset in [0,1,2,3] {{
                        let cell = grid::neighbor(112,({facing}+offset)%4);
                        let total = 0.0;
                        let inventory = inventory::storage_read(cell); for slot in 0..16 {{ total += inventory[slot*2+1]; }}
                        totals.push(total);
                    }}
                    for i in 0..4 {{ data::session_set(50+i,totals[i]); }}
                "#
                ),
            );
            let state = controller_numbers(&demo, "session");
            let totals = &state[50..54];
            if blocked {
                assert_eq!(&totals[..4], &[1600., 5., 0., 4.]);
            } else {
                assert_eq!(&totals[..4], &[3., 3., 0., 3.]);
            }
            assert_eq!(machine_load(&demo, 112), 0.);
            assert_eq!(numbers(&demo, "counts")[20], 9.);
        }
    }
}

#[test]
fn logistics_inlets_match_facing_in_all_rotations() {
    for kind in [2, 4, 7, 8] {
        for facing in 0..4 {
            for side in 0..4 {
                let cell = [113, 127, 111, 97][side];
                let demo = buffer_layout(
                    &format!(
                        "[[112,{kind},{facing},0,0],[{cell},2,{},20,1]]",
                        (side + 2) % 4
                    ),
                    "simulation::factory_step();",
                );
                let accepts = if kind == 4 || kind == 7 {
                    side == (facing + 2) % 4
                } else {
                    side != facing
                };
                assert_eq!(
                    if kind == 4 {
                        inventory(&demo, 112)[0].1
                    } else {
                        numbers(&demo, "item_amounts")[112]
                    },
                    if accepts { 1. } else { 0. },
                    "kind={kind}, facing={facing}, side={side}"
                );
                assert_eq!(
                    numbers(&demo, "item_amounts")[cell],
                    if accepts { 0. } else { 1. }
                );
            }
        }
    }
}

#[test]
fn logistics_mergers_share_input_turns_and_release_mixed_material_batches() {
    for facing in 0..4 {
        for mixed in [false, true] {
            let mut demo = buffer_layout(
                &format!("[[112,8,{facing},20,10]]"),
                &format!(
                    r#"
                    for inlet in 0..3 {{
                        let side = ({facing} + [2,3,1][inlet])%4;
                        let cell = grid::neighbor(112,side);
                        let nodes = get_scene_list("nodes"); nodes[cell]=0.0; set_scene_list("nodes",nodes);
                        set_scene_variable("cursor_x",grid::cell_x(cell).to_float()); set_scene_variable("cursor_z",grid::cell_z(cell).to_float());
                        set_scene_variable("selected",3.0); set_scene_variable("direction",((side+2)%4).to_float()); building::place_selected();
                        let items = get_scene_list("items"); items[cell]=if {mixed} {{ [11.0,12.0,14.0][inlet] }} else {{ 20.0 }}; set_scene_list("items",items);
                        let amounts = get_scene_list("item_amounts"); amounts[cell]=10.0; set_scene_list("item_amounts",amounts);
                    }}
                    let front = grid::neighbor(112,{facing});
                    let nodes = get_scene_list("nodes"); nodes[front]=0.0; set_scene_list("nodes",nodes);
                    set_scene_variable("cursor_x",grid::cell_x(front).to_float()); set_scene_variable("cursor_z",grid::cell_z(front).to_float());
                    set_scene_variable("selected",4.0); set_scene_variable("direction",{facing}.0); building::place_selected();
                "#
                ),
            );
            let turn_check = number(&demo, "ticks") + 4.;
            factory_code(
                &mut demo,
                &format!(
                    r#"
                simulation::factory_step();
                if !{mixed} && get_scene_variable("ticks")=={turn_check:.1} {{
                    for side in [2,3,1] {{
                        if get_scene_list("item_amounts")[grid::neighbor(112,({facing}+side)%4)]!=9.0 {{ throw "merger input did not get its turn"; }}
                    }}
                }}
            "#
                ),
                40,
            );
            let counts = numbers(&demo, "counts");
            assert_eq!(
                numbers(&demo, "item_amounts").iter().sum::<f32>() + counts.iter().sum::<f32>(),
                40.
            );
            if mixed {
                for item in [11, 12, 14] {
                    assert!(
                        counts[item] >= 5.,
                        "every mixed inlet must make progress: {counts:?}"
                    );
                }
            } else {
                assert_eq!(counts[20], 40.);
            }
        }
    }
}

#[test]
fn logistics_single_item_belts_preserve_corners_and_ignore_placement_order() {
    for reverse in [false, true] {
        let layout = if reverse {
            "[[127,4,1,0,0],[112,2,1,20,1],[111,2,0,20,1]]"
        } else {
            "[[111,2,0,20,1],[112,2,1,20,1],[127,4,1,0,0,0]]"
        };
        let demo = buffer_layout(
            layout,
            r#"
            simulation::factory_step();
            if get_scene_list("item_amounts")[111]!=1.0 || get_scene_list("item_amounts")[112]!=0.0 { throw "occupied conveyor accepted an item"; }
            simulation::factory_step();
            if get_scene_list("counts")[20]!=1.0 || get_scene_list("item_amounts")[112]!=1.0 { throw "item crossed more than one tile per beat"; }
            simulation::factory_step();
        "#,
        );
        assert_eq!(numbers(&demo, "counts")[20], 2.);
        assert_eq!(numbers(&demo, "item_amounts").iter().sum::<f32>(), 0.);
    }
}

#[test]
fn logistics_routing_turns_and_buffers_survive_chunk_streaming_and_demolition() {
    let demo = buffer_layout(
        "[[112,7,0,11,10],[180,8,1,12,10],[113,2,0,20,1]]",
        r#"
        let turns=get_scene_list("split_state"); turns[112]=2.0; turns[180]=1.0; set_scene_list("split_state",turns);
        world::enter_chunk(4,0); chunks::unload_chunk_visuals(144); world::enter_chunk(0,0);
        "#,
    );
    assert_eq!(numbers(&demo, "split_state")[112], 2.);
    assert_eq!(numbers(&demo, "split_state")[180], 1.);
    assert_eq!(numbers(&demo, "items")[112], 11.);
    assert_eq!(numbers(&demo, "items")[180], 12.);
    assert_eq!(machine_load(&demo, 112), 10.);
    assert_eq!(machine_load(&demo, 180), 10.);
    assert_eq!(machine_load(&demo, 113), 1.);

    let demo = buffer_layout(
        "[[112,7,0,20,10],[113,4,0,0,0],[127,4,1,0,0,0]]",
        r#"
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",0.0);
        building::rotate_selected(); simulation::factory_step();
        if get_scene_list("item_amounts")[112]!=10.0 { throw "rotating splitter transferred an item"; }
        visuals::finish_rotations(); simulation::factory_step();
        if get_scene_list("counts")[20]!=1.0 { throw "splitter did not resume after rotation"; }
        building::remove_selected(); set_scene_variable("selected",8.0); building::place_selected();
        "#,
    );
    assert_eq!(numbers(&demo, "builds")[112], 8.);
    assert_eq!(numbers(&demo, "split_state")[112], 0.);
    assert_eq!(machine_load(&demo, 112), 0.);
}

#[test]
fn mixed_buffers_survive_chunk_unloading_and_reset_clears_every_quantity() {
    let mut demo = buffer_layout(
        "[[112, 3, 0, 11, 45], [110, 5, 0, 20, 20]]",
        r#"
        let inputs = get_scene_list("input_items"); inputs[112] = 1.0; set_scene_list("input_items", inputs);
        let amounts = get_scene_list("input_amounts"); amounts[112] = 55.0; set_scene_list("input_amounts", amounts);
        let iron = get_scene_list("assembler_iron"); iron[110] = 50.0; set_scene_list("assembler_iron", iron);
        let copper = get_scene_list("assembler_copper"); copper[110] = 30.0; set_scene_list("assembler_copper", copper);
        world::enter_chunk(4, 0); chunks::unload_chunk_visuals(144); world::enter_chunk(0, 0);
    "#,
    );
    assert_eq!(machine_load(&demo, 112), 100.);
    assert_eq!(numbers(&demo, "item_amounts")[112], 45.);
    assert_eq!(numbers(&demo, "input_amounts")[112], 55.);
    assert_eq!(numbers(&demo, "input_items")[112], 1.);
    assert_eq!(machine_load(&demo, 110), 100.);
    assert_eq!(numbers(&demo, "item_amounts")[110], 20.);
    assert_eq!(numbers(&demo, "assembler_iron")[110], 50.);
    assert_eq!(numbers(&demo, "assembler_copper")[110], 30.);
    tick(&mut demo, Some("N"));
    for name in [
        "item_amounts",
        "input_items",
        "input_amounts",
        "assembler_iron",
        "assembler_copper",
    ] {
        assert!(numbers(&demo, name).iter().all(|value| *value == 0.));
    }
}

#[test]
fn map_toggles_by_key_and_button_blocks_world_input_and_keeps_production_running() {
    use bozzard_scene::{Layer, middleware::ui::Input};
    let mut demo = demo_with_seed(Some(4.));
    tick(&mut demo, None);
    let hud = |demo: &SceneDemo| {
        demo.instance()
            .ui_frame(&demo.app.world, Layer::ThreeD, [1080., 600.])
            .unwrap()
    };
    assert!(hud(&demo).element("map-panel").is_none());
    let builds = numbers(&demo, "builds");
    let cursor = [number(&demo, "cursor_x"), number(&demo, "cursor_z")];
    let seed = number(&demo, "seed");
    let zoom = controller_number(&demo, "camera_zoom_target");
    tick(&mut demo, Some("M"));
    for _ in 0..6 {
        tick(&mut demo, Some("M"));
    }
    assert!(
        hud(&demo).element("map-panel").is_some(),
        "holding M must not repeat the toggle"
    );
    tick_keys(
        &mut demo,
        &["D", "N", "Space", "X", "R", "Ctrl", "2", "F", "E", "J"],
    );
    assert_eq!(numbers(&demo, "builds"), builds);
    assert_eq!(
        [number(&demo, "cursor_x"), number(&demo, "cursor_z")],
        cursor
    );
    assert_eq!(number(&demo, "seed"), seed);
    assert_eq!(controller_number(&demo, "bar"), 0.);
    assert!(hud(&demo).element("journal-book").is_none());
    assert!(hud(&demo).element("storage-panel").is_none());
    assert!(
        demo.ui_input(
            Layer::ThreeD,
            [1080., 600.],
            Input::ScrollAt {
                point: [500., 300.],
                delta: -400.
            }
        )
        .unwrap()
    );
    tick(&mut demo, None);
    assert_eq!(controller_number(&demo, "camera_zoom_target"), zoom);
    settle(&mut demo, 620);
    assert!(
        numbers(&demo, "counts")[20] > 0.,
        "factories keep running behind the map"
    );
    tick_keys(&mut demo, &["M", "D", "Space"]);
    assert!(hud(&demo).element("map-panel").is_none());
    assert_eq!(
        [number(&demo, "cursor_x"), number(&demo, "cursor_z")],
        cursor
    );
    assert_eq!(
        numbers(&demo, "builds"),
        builds,
        "closing the map consumes simultaneous build input"
    );
    tick(&mut demo, None);
    click_widget(&mut demo, "map-open");
    assert!(hud(&demo).element("map-panel").is_some());
    click_widget(&mut demo, "map-close");
    assert!(
        hud(&demo).element("map-cell-144").is_none(),
        "closed map cells must not render"
    );
    ui_event(&mut demo, Input::Key("M".into()));
    tick(&mut demo, None);
    assert!(
        hud(&demo).element("map-panel").is_some(),
        "native UI shortcut opens the map"
    );
    ui_event(&mut demo, Input::Key("Escape".into()));
    tick(&mut demo, None);
    assert!(hud(&demo).element("map-panel").is_none());
    assert!(hud(&demo).element("menu-panel").is_some());
    press(&mut demo, "M");
    assert!(hud(&demo).element("map-panel").is_none());
    click_widget(&mut demo, "menu-continue");
    press(&mut demo, "J");
    settle(&mut demo, 15);
    press(&mut demo, "M");
    assert!(hud(&demo).element("map-panel").is_some());
    assert!(hud(&demo).element("journal-book").is_none());
}

#[test]
fn map_tracks_loaded_explored_and_current_chunks_and_clears_on_new_world() {
    use bozzard_scene::Layer;
    let mut demo = script_fixture(
        r#"
        for x in 1..7 { chunks::discover_chunk(x, 0); }
        chunks::discover_chunk(-1, -1);
        world::enter_chunk(4, 0);
        set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 0.0);
    "#,
    );
    settle(&mut demo, 40);
    let check_map = |demo: &SceneDemo| {
        let frame = demo
            .instance()
            .ui_frame(&demo.app.world, Layer::ThreeD, [1080., 600.])
            .unwrap();
        let visited = controller_numbers(demo, "visited");
        let resident = controller_numbers(demo, "resident");
        let x = number(demo, "chunk_x") as i32;
        let z = number(demo, "chunk_z") as i32;
        let current = ((z + 8) * 17 + x + 8) as usize;
        assert_eq!(
            frame.element("map-region").unwrap().text,
            format!("Region {x}, {z}")
        );
        assert_eq!(
            frame.element("map-counts").unwrap().text,
            format!(
                "{} loaded\n{} explored / 289 regions",
                resident.iter().filter(|&&n| n > 0.).count(),
                visited.iter().filter(|&&n| n > 0.).count()
            )
        );
        for id in 0..289 {
            let color = if id == current {
                [0.64, 0.29, 0.085, 1.]
            } else if resident[id] > 0. {
                [0.13, 0.38, 0.26, 1.]
            } else if visited[id] > 0. {
                [0.11, 0.17, 0.20, 1.]
            } else {
                [0.018, 0.032, 0.026, 1.]
            };
            assert_eq!(
                frame
                    .element(&format!("map-cell-{id}"))
                    .unwrap()
                    .widget
                    .background,
                color,
                "region {id}"
            );
        }
        assert_eq!(frame.element("map-cell-144").unwrap().text, "H");
        // Negative Z is north and positive X is east, independent of camera heading.
        assert!(
            frame.element("map-cell-126").unwrap().rect.min[1]
                < frame.element("map-cell-144").unwrap().rect.min[1]
        );
        assert!(
            frame.element("map-cell-148").unwrap().rect.min[0]
                > frame.element("map-cell-144").unwrap().rect.min[0]
        );
    };
    press(&mut demo, "M");
    check_map(&demo);
    assert_eq!(controller_numbers(&demo, "resident")[144], 0.);
    press(&mut demo, "M");
    tick_keys(&mut demo, &["Ctrl", "R"]);
    settle(&mut demo, 40);
    press(&mut demo, "M");
    check_map(&demo);
    press(&mut demo, "M");
    press(&mut demo, "N");
    press(&mut demo, "M");
    check_map(&demo);
    assert_eq!(
        controller_numbers(&demo, "visited")[148],
        0.,
        "new planet forgets the old route"
    );
}

fn stellar_fixture(setup: &str) -> SceneDemo {
    let mut demo = factory_with_mode(Some(4.), false);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../earth-factory/scenes/scripts/earth_factory.rs");
    let source = std::fs::read_to_string(path)
        .unwrap()
        .replace("fn on_start(me)", "fn factory_start(me)");
    let source = format!(
        r#"{source}
        fn test_build(cell, kind) {{
            let nodes = get_scene_list("nodes"); nodes[cell] = if kind == 6.0 {{ 4.0 }} else {{ 0.0 }};
            set_scene_list("nodes", nodes);
            set_scene_variable("cursor_x", grid::cell_x(cell).to_float());
            set_scene_variable("cursor_z", grid::cell_z(cell).to_float());
            set_scene_variable("selected", kind); building::place_selected();
        }}
        fn on_start(me) {{ factory_start(me); {setup} }}
    "#
    );
    demo.with_instance(|instance, _| instance.register_script("earth-factory".into(), source))
        .unwrap();
    tick(&mut demo, None);
    demo
}

#[test]
fn moon_deposits_are_sparse_seeded_and_always_include_two_techtorium() {
    use std::collections::BTreeMap;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../earth-factory/scenes/scripts/earth_factory.rs");
    let source = std::fs::read_to_string(path)
        .unwrap()
        .replace("fn on_start(me)", "fn factory_start(me)")
        .replace("fn on_update(me, dt)", "fn factory_update(me, dt)");
    let mut first = BTreeMap::new();
    for (seed, reverse) in [
        (1., false),
        (4., false),
        (2_000_000_000., false),
        (1., true),
    ] {
        let mut demo = factory_with_mode(Some(seed), false);
        let script = format!(
            r#"{source}
            fn on_start(me) {{ data::session_set(7, 1.0); }}
            fn on_update(me, dt) {{
                let step = get_scene_variable("ticks").to_int();
                let id = if {reverse} {{ 168 - step }} else {{ step }};
                set_scene_list("nodes", deposits::generate_region_nodes(get_scene_variable("seed").to_int(), id % 13 - 6, id / 13 - 6));
                set_scene_variable("ticks", (step + 1).to_float());
            }}"#
        );
        demo.with_instance(|instance, _| instance.register_script("earth-factory".into(), script))
            .unwrap();
        let mut planet = BTreeMap::new();
        let mut counts = [0; 3];
        let mut empty = 0;
        let mut rare_positions: Vec<(i32, i32)> = Vec::new();
        for step in 0..169 {
            tick(&mut demo, None);
            let id = if reverse { 168 - step } else { step };
            let nodes = numbers(&demo, "nodes");
            let deposits = nodes.iter().filter(|&&kind| kind > 0.).count();
            if id == 84 {
                assert_eq!(deposits, 2);
                assert!(nodes.contains(&24.) && nodes.contains(&25.));
                for z in -1i32..=2 {
                    for x in -1i32..=2 {
                        assert_eq!(nodes[((z + 7) * 15 + x + 7) as usize], 0.);
                    }
                }
            } else {
                assert!(deposits <= 1);
            }
            if deposits == 0 {
                empty += 1;
            }
            for &kind in nodes.iter().filter(|&&kind| kind > 0.) {
                assert!([24., 25., 26.].contains(&kind), "Earth resource on Moon");
                counts[kind as usize - 24] += 1;
                if kind == 26. {
                    rare_positions.push((id % 13 - 6, id / 13 - 6));
                }
            }
            planet.insert(id, nodes);
        }
        assert!(empty > 84, "most lunar regions should be empty");
        assert!(counts[0] >= 10 && counts[1] >= 10);
        assert_eq!(counts[2], 2, "two exceptionally rare, guaranteed deposits");
        assert!(
            rare_positions[0].0.abs_diff(rare_positions[1].0)
                + rare_positions[0].1.abs_diff(rare_positions[1].1)
                >= 8
        );
        println!("Moon seed={seed} empty={empty}/169 deposits={counts:?}");
        if reverse {
            assert_eq!(planet, first);
        } else if seed == 1. {
            first = planet;
        } else {
            assert_ne!(planet, first);
        }
    }
}

#[test]
fn moon_round_trips_preserve_both_factories_power_storage_and_carried_stock() {
    let demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        test_build(113,9.0); test_build(115,11.0); test_build(117,4.0);
        power::connect_power(power::power_id(112),power::power_id(113)); power::connect_power(power::power_id(113),power::power_id(115));
        machines::select_recipe(115,16.0); machines::feed_assembler(115); power::update_power();
        let earth_power = get_object_list("power_data");
        let inventory = grid::empty_numbers(32); inventory[0]=11.0; inventory[1]=37.0; inventory::storage_write(117,inventory);
        let counts=grid::empty_numbers(64); counts[11]=37.0; set_scene_list("counts",counts);
        let stock=get_object_list("stock"); stock[11]=29.0; backpack::set_stock(stock);
        world::enter_chunk(2,1); test_build(114,5.0); machines::select_recipe(114,18.0); machines::feed_assembler(114);
        world::enter_chunk(0,0);
        let earth_nodes = get_scene_list("nodes"); power::update_power(); earth_power = get_object_list("power_data");
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0);
        world::travel_to_other_planet();
        if !data::on_moon() || get_object_variable("phase") != 7.0 || get_object_list("stock")[11] != 29.0 { throw "arrival lost progression or inventory"; }
        if get_scene_list("counts")[11] != 0.0 || get_scene_list("machine_cells").len() != 0 { throw "Earth factory leaked onto Moon"; }
        if get_object_list("visited")[grid::chunk_id(2,1)] != 0.0 { throw "Earth exploration leaked onto Moon"; }
        let moon_nodes = get_scene_list("nodes");
        test_build(113,9.0); test_build(115,11.0); test_build(117,4.0);
        power::connect_power(power::power_id(112),power::power_id(113)); power::connect_power(power::power_id(113),power::power_id(115));
        machines::select_recipe(115,17.0); machines::feed_assembler(115); power::update_power();
        let moon_power=get_object_list("power_data");
        let inventory=grid::empty_numbers(32); inventory[0]=24.0; inventory[1]=19.0; inventory::storage_write(117,inventory);
        let counts=grid::empty_numbers(64); counts[24]=19.0; set_scene_list("counts",counts);
        let stock=get_object_list("stock"); stock[26]=2.0; backpack::set_stock(stock);
        chunks::discover_chunk(-3,2);
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0); world::travel_to_other_planet();
        if data::on_moon() || get_scene_list("nodes") != earth_nodes { throw "Earth rerolled"; }
        if get_object_list("power_data") != earth_power { throw "Earth wiring lost"; }
        if get_object_list("recipes")[115] != 16.0 || inventory::storage_read(117)[1] != 37.0 || get_scene_list("counts")[11] != 37.0 { throw "Earth factory state lost"; }
        if get_object_list("stock")[26] != 2.0 { throw "Moon cargo lost"; }
        world::enter_chunk(2,1);
        if get_scene_list("builds")[114] != 5.0 || get_object_list("recipes")[114] != 18.0 || get_scene_list("assembler_iron")[114] != 20.0 { throw "remote Earth factory lost"; }
        world::enter_chunk(0,0); set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0); world::travel_to_other_planet();
        if get_scene_list("nodes") != moon_nodes || get_object_list("power_data") != moon_power { throw "Moon rerolled or lost wiring"; }
        if get_object_list("recipes")[115] != 17.0 || inventory::storage_read(117)[1] != 19.0 || get_scene_list("counts")[24] != 19.0 { throw "Moon factory state lost"; }
        if get_object_list("visited")[grid::chunk_id(-3,2)] != 1.0 { throw "Moon exploration forgotten"; }
        world::begin_world(4);
        if data::on_moon() || data::region_data("chunk_nodes")[grid::chunk_id(2,1)] != "" { throw "new world retained Earth archive"; }
        data::session_set(7,1.0);
        for page in data::region_data("chunk_nodes") { if page != "" { throw "new world retained Moon archive"; } }
        data::session_set(7,0.0);
    "#,
    );
    assert_eq!(controller_numbers(&demo, "session")[7], 0.);
}

#[test]
fn moon_boundaries_gathering_and_mining_use_only_lunar_resources() {
    let mut demo = stellar_fixture(
        r#"
        world::travel_to_other_planet(); if data::on_moon() { throw "unfinished rocket launched"; }
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0); world::travel_to_other_planet();
        for corner in [[6,6],[-6,-6],[6,-6],[-6,6]] {
            world::enter_chunk(corner[0],corner[1]); world::explore(corner[0]*2,corner[1]*2);
            if get_scene_variable("chunk_x") != corner[0].to_float() || get_scene_variable("chunk_z") != corner[1].to_float() { throw "crossed Moon boundary"; }
        }
        for id in 0..289 {
            if abs(id%17-8)>6 || abs(id/17-8)>6 {
                if get_object_list("visited")[id] != 0.0 { throw "generated outside lunar bounds"; }
            }
        }
        world::enter_chunk(0,0);
        // Same transport/storage path supports all three new material IDs.
        test_build(113,9.0);
        for row in [[114,24.0],[115,25.0],[116,26.0]] {
            let nodes=get_scene_list("nodes"); nodes[row[0]]=row[1]; set_scene_list("nodes",nodes);
            set_scene_variable("cursor_x",grid::cell_x(row[0]).to_float()); set_scene_variable("cursor_z",grid::cell_z(row[0]).to_float());
            set_scene_variable("selected",1.0); building::place_selected(); power::connect_power(power::power_id(113),power::power_id(row[0]));
        }
        power::connect_power(power::power_id(112),power::power_id(113)); power::update_power();
        for i in 0..4 { simulation::factory_step(); }
        for cell in 114..117 { if get_scene_list("items")[cell] != (cell-90).to_float() || get_scene_list("item_amounts")[cell] <= 0.0 { throw "Moon extraction failed"; } }
        set_scene_variable("cursor_x",-4.0); set_scene_variable("cursor_z",3.0);
    "#,
    );
    press(&mut demo, "F");
    assert_eq!(controller_numbers(&demo, "stock")[24], 1.);
    press(&mut demo, "I");
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        frame
            .element("player-slot-0")
            .unwrap()
            .text
            .contains("Amorium\n1")
    );
    assert_eq!(frame.element("player-slot-1").unwrap().text, "");
}

#[test]
fn moon_rocket_ui_travels_both_ways_and_map_has_169_regions() {
    use bozzard_scene::{Layer, middleware::ui::Input};
    let mut demo = stellar_fixture(
        r#"set_object_variable("phase",7.0); set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0);"#,
    );
    press(&mut demo, "E");
    ui_event(&mut demo, Input::ActivateObject("rocket-launch".into()));
    tick(&mut demo, None);
    settle(&mut demo, 270);
    assert_eq!(controller_numbers(&demo, "session")[7], 1.);
    assert_eq!(controller_numbers(&demo, "session")[2], 0.);
    press(&mut demo, "M");
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        frame
            .element("map-title")
            .unwrap()
            .text
            .contains("STELLA-Z2")
    );
    assert!(
        frame
            .element("map-counts")
            .unwrap()
            .text
            .contains("169 regions")
    );
    assert_eq!(
        (0..289)
            .filter(|id| frame.element(&format!("map-cell-{id}")).is_some())
            .count(),
        169
    );
    assert!(
        frame
            .element("world-status")
            .unwrap()
            .text
            .contains("STELLA-Z2 / NIGHT")
    );
    press(&mut demo, "M");
    press(&mut demo, "E");
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        frame
            .element("rocket-current")
            .unwrap()
            .text
            .contains("STELLA-Z2")
    );
    assert!(
        frame
            .element("rocket-moon")
            .unwrap()
            .text
            .contains("STELLAR-BX")
    );
    ui_event(&mut demo, Input::ActivateObject("rocket-launch".into()));
    tick(&mut demo, None);
    settle(&mut demo, 270);
    assert_eq!(controller_numbers(&demo, "session")[7], 0.);
    press(&mut demo, "M");
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        frame
            .element("map-counts")
            .unwrap()
            .text
            .contains("289 regions")
    );
    assert_eq!(
        (0..289)
            .filter(|id| frame.element(&format!("map-cell-{id}")).is_some())
            .count(),
        289
    );
}

#[test]
fn stellar_title_creates_survival_and_creative_worlds_and_blocks_hidden_shortcuts() {
    use bozzard_scene::middleware::ui::Input;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../earth-factory/scenes/earth.json");
    let scene = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut demo = SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
    tick(&mut demo, None);
    tick_keys(&mut demo, &["Space", "D", "N", "E", "J", "Escape"]);
    tick(&mut demo, None);
    assert_eq!(number(&demo, "ticks"), 0.);
    assert!(numbers(&demo, "builds").is_empty());
    assert!(
        !demo
            .ui_input(
                bozzard_scene::Layer::ThreeD,
                [1080., 600.],
                Input::ActivateObject("menu-open".into())
            )
            .unwrap()
    );
    click_widget(&mut demo, "title-create");
    assert_eq!(controller_number(&demo, "phase"), 0.);
    assert_eq!(
        numbers(&demo, "builds").iter().filter(|&&v| v > 0.).count(),
        0
    );
    assert_eq!(controller_numbers(&demo, "stock").iter().sum::<f32>(), 0.);
    press(&mut demo, "Escape");
    click_widget(&mut demo, "menu-main-menu");
    click_widget(&mut demo, "title-creative");
    click_widget(&mut demo, "title-create");
    assert_eq!(controller_number(&demo, "phase"), 7.);
    assert_eq!(
        numbers(&demo, "builds").iter().filter(|&&v| v > 0.).count(),
        0,
        "Creative starts clean, not with the demo factory"
    );
    move_cursor(&mut demo, 1, 0);
    press(&mut demo, "Space");
    assert_eq!(numbers(&demo, "builds")[113], 3.);
    assert_eq!(
        controller_numbers(&demo, "power_live")[113],
        0.,
        "Creative still requires wiring"
    );
    assert_eq!(controller_numbers(&demo, "stock").iter().sum::<f32>(), 0.);
    press(&mut demo, "Escape");
    click_widget(&mut demo, "menu-main-menu");
    click_widget(&mut demo, "title-survival");
    click_widget(&mut demo, "title-create");
    assert_eq!(controller_number(&demo, "phase"), 0.);
    assert_eq!(numbers(&demo, "builds")[113], 0.);
}

fn developer_world() -> SceneDemo {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../earth-factory/scenes/earth.json");
    let scene = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut demo = SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
    tick(&mut demo, None);
    click_widget(&mut demo, "title-dev");
    for _ in 0..24 {
        tick(&mut demo, None);
    }
    demo
}

#[test]
fn dev_world_catalog_is_spaced_complete_and_bounded_without_generation() {
    use std::collections::BTreeSet;
    let mut demo = developer_world();
    assert_eq!(controller_number(&demo, "phase"), 7.);
    assert_eq!(controller_numbers(&demo, "session")[63], 1.);
    assert_eq!(
        controller_numbers(&demo, "resident").iter().sum::<f32>(),
        4.
    );
    let (world, player) = coop_world(&demo);
    assert!(world.state().dev_world());
    assert_eq!(
        player.backpack.iter().filter(|s| s.amount == 10).count(),
        25
    );
    let mut deposits = BTreeSet::new();
    let mut machine_types = BTreeSet::new();
    let mut samples = BTreeSet::new();
    for chunk in [144, 145, 161, 162] {
        let nodes = coop_page(&world, "chunk_nodes", 0, chunk, 225);
        let builds = coop_page(&world, "cache_builds", 0, chunk, 225);
        let items = coop_page(&world, "cache_items", 0, chunk, 225);
        for cell in 0..225 {
            let x = (chunk as i32 % 17 - 8) * 15 + cell as i32 % 15 - 7;
            let z = (chunk as i32 / 17 - 8) * 15 + cell as i32 / 15 - 7;
            if nodes[cell] > 0. {
                assert_eq!(z, -5);
                assert_eq!((x + 6) % 2, 0);
                assert!(deposits.insert(nodes[cell] as u8), "duplicate deposit");
            }
            if builds[cell] > 0. {
                assert!([-1, 3, 5, 8, 12, 16, 20].contains(&z));
                assert_eq!((x + 6) % 2, 0);
                machine_types.insert(builds[cell] as u8);
            }
            if items[cell] > 0. {
                assert!([2., 26.].contains(&builds[cell]));
                assert!(samples.insert(items[cell] as u8), "duplicate sample");
            }
        }
    }
    assert_eq!(deposits, (1..=10).chain(24..=26).chain([40, 45]).collect());
    assert_eq!(machine_types, (1..=9).chain(11..=29).collect());
    assert_eq!(samples, (1..=18).chain(20..=50).collect());
    let original_nodes = world.state().controller["chunk_nodes"].clone();
    factory_code(
        &mut demo,
        r#"
        for pair in [[-1,0],[2,0],[0,-1],[0,2],[8,8]] {chunks::discover_chunk(pair[0],pair[1]);world::enter_chunk(pair[0],pair[1]);}
        if get_scene_variable("chunk_x")!=0.0 || get_scene_variable("chunk_z")!=0.0 {throw "escaped showroom";}
        if world::explore(-8,0)!=[-7,0] {throw "west boundary";}
        world::enter_chunk(1,1);
        if world::explore(8,8)!=[7,7] {throw "southeast boundary";}
        world::enter_chunk(0,0);world::travel_to_other_planet();
    "#,
        1,
    );
    assert_eq!(controller_numbers(&demo, "visited").iter().sum::<f32>(), 4.);
    assert_eq!(
        coop_world(&demo).0.state().controller["chunk_nodes"],
        original_nodes
    );
    restore_factory_update(&mut demo);
    press(&mut demo, "M");
    assert!(
        demo.instance()
            .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
            .unwrap()
            .element("map-counts")
            .unwrap()
            .text
            .contains("4 explored / 4")
    );
    press(&mut demo, "M");
    press(&mut demo, "N");
    assert_eq!(controller_numbers(&demo, "session")[63], 1.);
    assert_eq!(
        coop_world(&demo).0.state().controller["chunk_nodes"],
        original_nodes
    );
    press(&mut demo, "Escape");
    click_widget(&mut demo, "menu-main-menu");
    click_widget(&mut demo, "title-survival");
    click_widget(&mut demo, "title-create");
    assert_eq!(controller_numbers(&demo, "session")[63], 0.);
    assert_eq!(controller_number(&demo, "phase"), 0.);
    assert!(numbers(&demo, "builds").iter().all(|v| *v == 0.));
    factory_code(&mut demo, "world::enter_chunk(-1,0);", 1);
    assert_eq!(number(&demo, "chunk_x"), -1.);
}

#[test]
fn dev_world_survives_save_load_and_keeps_coop_guests_inside_the_showroom() {
    use bozzard_demo::factory::{
        Session, authority::Executor, replication::requests::Action, shared::Position,
    };
    use std::time::Duration;
    let mut demo = developer_world();
    let (mut world, mut player) = coop_world(&demo);
    let mut executor = Executor::new(demo.instance()).unwrap();
    player.position = Position {
        planet: 0,
        x: -7,
        z: 0,
    };
    let before = world.clone();
    assert!(
        executor
            .apply(
                &mut world,
                10,
                &mut player,
                &Action::Move { x: -1, z: 0 },
                Duration::ZERO
            )
            .is_err()
    );
    assert_eq!(world, before);
    assert_eq!(player.position.x, -7);
    player.position = Position {
        planet: 0,
        x: 0,
        z: 1,
    };
    assert!(
        executor
            .apply(&mut world, 10, &mut player, &Action::Travel, Duration::ZERO)
            .is_err()
    );
    player.position = Position {
        planet: 0,
        x: 7,
        z: 0,
    };
    assert!(
        executor
            .apply(
                &mut world,
                10,
                &mut player,
                &Action::Move { x: 1, z: 0 },
                Duration::ZERO
            )
            .unwrap()
            .accepted
    );
    assert_eq!(player.position.x, 8);
    let directory = save_directory(&mut demo);
    factory_code(
        &mut demo,
        "world::enter_chunk(1,1);persistence::prepare_save(1);",
        1,
    );
    factory_code(&mut demo, "persistence::update(0.0);", 1);
    finish_save_io(&mut demo);
    let mut loaded = factory_with_mode(Some(17.), false);
    tick(&mut loaded, None);
    loaded
        .app
        .world
        .resource_mut::<Session>()
        .unwrap()
        .directory = directory.clone();
    factory_code(
        &mut loaded,
        "data::session_set(117,1.0);data::session_set(116,2.0);",
        1,
    );
    factory_code(&mut loaded, "persistence::update(0.0);", 1);
    finish_save_io(&mut loaded);
    assert_eq!(controller_numbers(&loaded, "session")[63], 1.);
    assert_eq!(
        controller_numbers(&loaded, "resident").iter().sum::<f32>(),
        4.
    );
    assert_eq!(number(&loaded, "chunk_x"), 1.);
    assert_eq!(number(&loaded, "chunk_z"), 1.);
    assert!(coop_world(&loaded).0.state().dev_world());
    assert_eq!(
        coop_world(&loaded).0.state().controller["chunk_nodes"],
        before.state().controller["chunk_nodes"]
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn early_manual_crafting_builds_a_pole_without_power_and_preserves_alloy() {
    let demo = stellar_fixture(
        r#"
        let stock = get_object_list("stock"); stock[1]=30.0; stock[2]=12.0; stock[8]=8.0; stock[9]=2.0; backpack::set_stock(stock);
        progression::deliver_phase();
        for kind in [1.0,2.0,3.0,9.0,10.0] { if !data::tool_unlocked(kind) { throw "early unlock missing"; } }
        if data::tool_unlocked(4.0) || data::tool_unlocked(5.0) || data::tool_unlocked(6.0) || data::tool_unlocked(11.0) { throw "later tools unlocked early"; }
        progression::craft_batch(11,10); progression::craft_batch(12,2); progression::craft(13);
        let before = get_object_list("stock");
        if data::pay_for_build(9.0) || get_object_list("stock") != before { throw "partial pole payment"; }
        progression::craft_batch(16,5); progression::craft_batch(15,4); progression::craft_batch(14,2);
        test_build(113,9.0);
    "#,
    );
    assert_eq!(numbers(&demo, "builds")[113], 9.);
    let stock = controller_numbers(&demo, "stock");
    assert_eq!([stock[14], stock[15], stock[16]], [0.; 3]);
    assert_eq!(stock[13], 1.);
}

#[test]
fn wired_power_enforces_ports_and_independent_overload_and_removal() {
    let demo = stellar_fixture(
        r#"
        set_object_variable("creative", true); set_object_variable("phase", 4.0);
        test_build(113, 9.0); test_build(114, 9.0);
        for cell in [98,99,100,115,116] { test_build(cell, 3.0); }
        let pod = power::power_id(112); let a = power::power_id(113); let b = power::power_id(114);
        if power::connect_power(pod, power::power_id(98)) == "" { throw "machine bypassed pole"; }
        if power::connect_power(a,a) == "" { throw "self connection"; }
        if power::connect_power(pod,a) != "" || power::connect_power(a,b) != "" { throw "pole chain"; }
        for cell in [98,99,100] { if power::connect_power(a,power::power_id(cell)) != "" { throw "port refused"; } }
        if power::connect_power(a,b) == "" { throw "duplicate cable"; }
        if power::connect_power(a,power::power_id(115)) == "" { throw "sixth port accepted"; }
        power::connect_power(b,power::power_id(115)); power::update_power();
        if get_object_list("power_live")[98] != 1.0 { throw "eight-power network should run"; }
        if get_object_list("power_live")[116] != 0.0 { throw "unwired machine ran"; }
        power::connect_power(b,power::power_id(116)); power::update_power();
        if get_object_list("power_live")[98] != 0.0 { throw "overloaded network ran"; }
        test_build(130,6.0); test_build(129,9.0); test_build(128,3.0);
        power::connect_power(power::power_id(130),power::power_id(129)); power::connect_power(power::power_id(129),power::power_id(128)); power::update_power();
        if get_object_list("power_live")[128] != 1.0 { throw "other circuit affected by overload"; }
        set_scene_variable("cursor_x", grid::cell_x(116).to_float()); set_scene_variable("cursor_z", grid::cell_z(116).to_float());
        set_scene_variable("selected",3.0); building::remove_selected(); power::update_power();
        if get_object_list("power_live")[98] != 1.0 { throw "removing load did not restore power"; }
        set_scene_variable("cursor_x", grid::cell_x(100).to_float()); set_scene_variable("cursor_z", grid::cell_z(100).to_float()); building::remove_selected();
        if power::connect_power(a,power::power_id(115)) == "" { throw "machine accepted second cable"; }
        test_build(100,3.0);
        if power::connect_power(a,power::power_id(100)) != "" { throw "deleted port not released"; }
        power::update_power();
    "#,
    );
    assert_eq!(controller_numbers(&demo, "power_live")[128], 1.);
    assert_eq!(controller_numbers(&demo, "power_live")[98], 1.);
}

#[test]
fn wires_cross_chunk_seams_survive_unloading_and_clear_on_reset() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative", true); set_object_variable("phase", 4.0);
        test_build(118,9.0); power::connect_power(power::power_id(112), power::power_id(118));
        set_object_variable("wire_start", power::power_id(118).to_float());
        world::enter_chunk(1,0);
        test_build(106,9.0); let b = power::power_id(106);
        if power::connect_power(get_object_variable("wire_start").to_int(),b) != "" { throw "cross chunk cable failed"; }
        test_build(107,5.0); power::connect_power(b,power::power_id(107));
        machines::select_recipe(107,20.0); power::update_power();
        if get_object_list("power_live")[107] != 1.0 { throw "remote pod failed to supply power"; }
        world::enter_chunk(6,0); chunks::unload_chunk_visuals(144); chunks::unload_chunk_visuals(145);
        world::enter_chunk(1,0); power::update_power();
        if get_object_list("recipes")[107] != 20.0 || get_object_list("power_live")[107] != 1.0 { throw "state lost across residency"; }
    "#,
    );
    assert_eq!(controller_numbers(&demo, "power_live")[107], 1.);
    press(&mut demo, "N");
    assert_eq!(controller_numbers(&demo, "recipes"), vec![0.; 225]);
    assert_eq!(
        controller_numbers(&demo, "power_live").iter().sum::<f32>(),
        1.
    ); // pod alone
}

#[test]
fn assembler_e_selects_real_output_preserves_inputs_and_obeys_shared_capacity() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",4.0);
        test_build(113,9.0); test_build(114,5.0);
        power::connect_power(power::power_id(112),power::power_id(113)); power::connect_power(power::power_id(113),power::power_id(114));
        power::update_power();
    "#,
    );
    press(&mut demo, "E");
    assert_eq!(controller_number(&demo, "assembler_cell"), 114.);
    click_widget(&mut demo, "assembler-alloy");
    click_widget(&mut demo, "assembler-feed");
    settle(&mut demo, 40);
    assert_eq!(numbers(&demo, "items")[114], 13.);
    let before = controller_numbers(&demo, "recipes");
    // A disabled recipe button cannot transmute an existing output stack.
    assert!(
        !demo
            .ui_input(
                bozzard_scene::Layer::ThreeD,
                [1080., 600.],
                bozzard_scene::middleware::ui::Input::ActivateObject("assembler-parts".into())
            )
            .unwrap()
    );
    assert_eq!(controller_numbers(&demo, "recipes"), before);
    click_widget(&mut demo, "assembler-take");
    assert!(controller_numbers(&demo, "stock")[13] > 0.);
    click_widget(&mut demo, "assembler-parts");
    assert_eq!(controller_numbers(&demo, "recipes")[114], 20.);
    settle(&mut demo, 40);
    assert_eq!(numbers(&demo, "items")[114], 20.);
    for _ in 0..10 {
        click_widget(&mut demo, "assembler-feed");
    }
    assert!(machine_load(&demo, 114) <= 100.);
    let cursor = number(&demo, "cursor_x");
    tick_keys(&mut demo, &["D", "Space", "X", "R", "N"]);
    assert_eq!(number(&demo, "cursor_x"), cursor);
    assert_eq!(numbers(&demo, "builds")[114], 5.);
    tick(&mut demo, None);
    press(&mut demo, "E");
    assert_eq!(controller_number(&demo, "assembler_cell"), -1.);
}

#[test]
fn redesigned_machine_lamps_follow_power_rotation_and_chunk_residency() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",4.0);
        test_build(113,9.0); test_build(128,3.0); test_build(129,11.0); test_build(130,5.0);
        let nodes=get_scene_list("nodes"); nodes[114]=1.0; set_scene_list("nodes",nodes);
        set_scene_variable("cursor_x",2.0); set_scene_variable("cursor_z",0.0);
        set_scene_variable("selected",1.0); building::place_selected(); power::update_power();
        for cell in [114,128,129,130] {
            let fx=power::power_effects(power::power_id(cell));
            if !fx.contains("off") || fx.contains("on") || fx.contains("heat") { throw "unpowered indicator"; }
        }
        "#,
    );
    factory_code(
        &mut demo,
        r#"
        power::connect_power(power::power_id(112),power::power_id(113));
        for cell in [114,128,129,130] { power::connect_power(power::power_id(113),power::power_id(cell)); }
        power::update_power();
        for cell in [114,128,129,130] {
            let fx=power::power_effects(power::power_id(cell));
            if !fx.contains("on") || fx.contains("off") { throw "powered indicator"; }
            if (cell==128)!=fx.contains("heat") { throw "furnace heat"; }
        }
        let slots=get_object_list("factory-transports","machine_light_slots");
        let count=0;for slot in slots {if slot!="" {count+=1;}}
        if count!=5 {throw "machine illumination slots";}
    "#,
        1,
    );
    restore_factory_update(&mut demo);
    let count = demo.instance().document().objects.len();
    settle(&mut demo, 20);
    assert_eq!(
        demo.instance().document().objects.len(),
        count,
        "steady power must reuse effects"
    );
    move_cursor(&mut demo, 1, 1);
    press(&mut demo, "R");
    settle(&mut demo, 12);
    factory_code(
        &mut demo,
        r#"
        let id=power::power_id(128);let machine=power::machine_handle(id);
        for role in ["on","heat"] {
            let effect=power::power_effects(id)[role];
            if get_position(effect)!=get_position(machine) || get_rotation(effect)!=get_rotation(machine) {throw "detached machine effect";}
        }
        visuals::finish_rotations();
        world::enter_chunk(1,0); power::update_power();
        let fx=power::power_effects(144*225+128);
        if !fx.contains("on") || !fx.contains("heat") {throw "resident neighbor lost power effects";}
        chunks::unload_chunk_visuals(144);power::update_power();
        if power::power_effects(144*225+128).len()!=0 {throw "unloaded effects leaked";}
        world::enter_chunk(0,0);power::update_power();
        if !power::power_effects(power::power_id(128)).contains("heat") {throw "heat not restored";}
        power::disconnect_power(power::power_id(113),power::power_id(128));power::update_power();
        let fx=power::power_effects(power::power_id(128));
        if !fx.contains("off") || fx.contains("on") || fx.contains("heat") {throw "power loss kept heat";}
        power::clear_power();
        if power::read_power_fx().len()!=0 {throw "effects survived reset";}
    "#,
        1,
    );
}

#[test]
fn redesigned_generator_lamp_tracks_supply_overload_and_rotation() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",4.0);
        let nodes=get_scene_list("nodes"); nodes[114]=4.0; set_scene_list("nodes",nodes);
        set_scene_variable("cursor_x",2.0); set_scene_variable("cursor_z",0.0);
        set_scene_variable("selected",6.0); building::place_selected();
        test_build(113,9.0); power::connect_power(power::power_id(114),power::power_id(113));
        power::update_power();
        if !power::power_effects(power::power_id(114)).contains("on") { throw "generator lamp not on"; }
        if abs(power::power_anchor(power::power_id(114),6)[1]-1.35)>0.001 { throw "generator terminal height"; }
        for cell in [128,129,130,131] {
            test_build(cell,5.0); power::connect_power(power::power_id(113),power::power_id(cell));
        }
        power::update_power();
        let fx=power::power_effects(power::power_id(114));
        if !fx.contains("off") || fx.contains("on") { throw "overloaded generator lamp"; }
        power::disconnect_power(power::power_id(113),power::power_id(131)); power::update_power();
        if !power::power_effects(power::power_id(114)).contains("on") { throw "generator power recovery"; }
        set_scene_variable("cursor_x",2.0); set_scene_variable("cursor_z",0.0);
        building::rotate_selected(); visuals::finish_rotations();
        let id=power::power_id(114); let machine=power::machine_handle(id);
        let lamp=power::power_effects(id)["on"];
        if get_rotation(machine)!=get_rotation(lamp) { throw "generator lamp detached during rotation"; }
        "#,
    );
    restore_factory_update(&mut demo);
    settle(&mut demo, 5);
    assert_eq!(controller_numbers(&demo, "power_live")[114], 1.);
}

#[test]
fn dense_power_grid_reuses_32_surface_lights_and_releases_all_effects() {
    use bozzard_scene::Layer;
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        // Build the topology in one transaction; this exceeds the renderer's light budget.
        let graph = power::power_graph(); let pod = power::power_id(112); let previous = pod;
        let builds = get_scene_list("builds"); let visuals = get_scene_list("build_visuals"); let cells = [];
        for cell in 0..40 {
            builds[cell]=9.0; cells.push(cell.to_float());
            visuals[cell]=grid::spawn_build(9.0,grid::cell_x(cell),grid::cell_z(cell),0);
            let id = power::power_id(cell); graph[id.to_string()] = [9,0,previous];
            graph[previous.to_string()].push(id); previous=id;
        }
        set_scene_list("builds",builds); set_scene_list("build_visuals",visuals); set_scene_list("machine_cells",cells);
        power::write_power(graph); set_object_variable("power_dirty",true); power::update_power();
    "#,
    );
    let lights = demo
        .instance()
        .view(&demo.app.world, Layer::ThreeD, 1.6)
        .unwrap()
        .lights;
    assert_eq!(lights.len(), 32);
    assert!(lights.iter().all(|light| light.light.intensity > 0.));
    assert_eq!(
        controller_numbers(&demo, "power_live")
            .iter()
            .filter(|&&v| v > 0.)
            .count(),
        41
    );
    // Stable membership proves lights and wires are reused while the world runs.
    let count = demo.instance().document().objects.len();
    settle(&mut demo, 40);
    assert_eq!(demo.instance().document().objects.len(), count);
    press(&mut demo, "N");
    let reset_lights = demo
        .instance()
        .view(&demo.app.world, Layer::ThreeD, 1.6)
        .unwrap()
        .lights;
    assert_eq!(
        reset_lights.len(),
        1,
        "only the new world's powered rocket remains lit"
    );
    assert_eq!(reset_lights[0].light.intensity, 2.5);
    assert!(
        !demo
            .instance()
            .document()
            .objects
            .iter()
            .any(|o| o.name == "power-lamp" || o.name == "power-wire")
    );
}

#[test]
fn rocket_navigation_lights_follow_power_blink_and_reuse_the_light_pool() {
    use bozzard_scene::{BlueprintHidden, Light, Transform};
    let lamp = |demo: &SceneDemo| {
        let entity = demo.instance().entity("pole-light-31").unwrap();
        (
            *demo.app.world.get::<Light>(entity).unwrap(),
            *demo.app.world.get::<Transform>(entity).unwrap(),
        )
    };
    let hidden = |demo: &SceneDemo, index: usize| {
        let entity = demo
            .instance()
            .entity(&format!("rocket-light-{index}"))
            .unwrap();
        demo.app.world.get::<BlueprintHidden>(entity).unwrap().0
    };
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        test_build(113,9.0); power::connect_power(power::power_id(112),power::power_id(113));
        for cell in [98,99,100] { test_build(cell,5.0); power::connect_power(power::power_id(113),power::power_id(cell)); }
        power::update_power();
    "#,
    );
    assert_eq!(
        controller_numbers(&demo, "session")[6],
        0.,
        "nine demand overloads the pod's eight supply"
    );
    assert_eq!(lamp(&demo).0.intensity, 0.);
    assert!(hidden(&demo, 0) && hidden(&demo, 1));
    press(&mut demo, "X"); // Remove the last assembler; the remaining demand is six.
    assert_eq!(controller_numbers(&demo, "session")[6], 1.);
    assert_eq!(lamp(&demo).0.intensity, 2.5);
    assert_ne!(hidden(&demo, 0), hidden(&demo, 1));
    let count = demo.instance().document().objects.len();
    press(&mut demo, "Escape");
    let (before, position) = lamp(&demo);
    settle(&mut demo, 31);
    let (after, next_position) = lamp(&demo);
    assert_ne!(
        before.color, after.color,
        "navigation lights keep blinking behind the menu"
    );
    assert_ne!(position.translation[0], next_position.translation[0]);
    assert_eq!(
        demo.instance().document().objects.len(),
        count,
        "blinking must not spawn effects"
    );
    press(&mut demo, "Escape");
    press(&mut demo, "Space"); // Rebuild the removed assembler, then wire it back in.
    tick_keys(&mut demo, &["Ctrl", "3"]);
    tick(&mut demo, None);
    press(&mut demo, "3");
    press(&mut demo, "Space");
    move_cursor(&mut demo, 1, 0);
    press(&mut demo, "Space");
    assert_eq!(controller_numbers(&demo, "session")[6], 0.);
    assert_eq!(lamp(&demo).0.intensity, 0.);
    assert!(
        hidden(&demo, 0) && hidden(&demo, 1),
        "overload extinguishes both lamps"
    );

    let no_source = stellar_fixture(
        r#"
        set_object_variable("phase",7.0); power::power_remove(power::power_id(112)); power::update_power();
    "#,
    );
    assert_eq!(lamp(&no_source).0.intensity, 0.);
    assert!(hidden(&no_source, 0) && hidden(&no_source, 1));
    let streamed = stellar_fixture(
        r#"
        set_object_variable("phase",7.0); power::update_power();
        world::enter_chunk(8,0); chunks::unload_chunk_visuals(144); power::update_power();
        if data::session_value(6) != 0.0 { throw "unloaded home kept navigation light"; }
        world::enter_chunk(0,0); power::update_power();
    "#,
    );
    assert_eq!(lamp(&streamed).0.intensity, 2.5);
    assert_ne!(hidden(&streamed, 0), hidden(&streamed, 1));
}

#[test]
fn assembler_manual_feed_fills_a_missing_ingredient_and_never_overfills() {
    let demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",4.0); test_build(114,5.0);
        set_object_variable("creative",false);
        let stock = get_object_list("stock"); stock[12]=5.0; backpack::set_stock(stock);
        let iron = get_scene_list("assembler_iron"); iron[114]=99.0; set_scene_list("assembler_iron",iron);
        machines::feed_assembler(114); machines::feed_assembler(114);
    "#,
    );
    assert_eq!(machine_load(&demo, 114), 100.);
    assert_eq!(numbers(&demo, "assembler_copper")[114], 1.);
    assert_eq!(controller_numbers(&demo, "stock")[12], 4.);
}

#[test]
fn cables_attach_in_all_eight_directions_and_follow_machine_lift() {
    use glam::{EulerRot, Quat, Vec3};
    for (dx, dz) in [
        (1, 0),
        (0, 1),
        (-1, 0),
        (0, -1),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ] {
        let target = 144 + dx + dz * 15;
        let mut demo = stellar_fixture(&format!(
            r#"
            set_object_variable("creative",true); set_object_variable("phase",4.0);
            test_build(144,9.0); test_build({target},3.0);
            power::connect_power(power::power_id(112),power::power_id(144)); power::connect_power(power::power_id(144),power::power_id({target}));
            power::update_power();
        "#
        ));
        let pole = Vec3::new(2., 1.47, 2.);
        let endpoint = Vec3::new((2 + dx) as f32, 1.15, (2 + dz) as f32);
        let middle = (pole + endpoint) * 0.5;
        let document = demo.instance().capture(&demo.app.world).unwrap();
        let wire = document
            .objects
            .iter()
            .find(|o| {
                o.name == "power-wire"
                    && (Vec3::from(o.transform.translation) - middle).length() < 0.001
            })
            .unwrap()
            .id
            .clone();
        let check = |demo: &SceneDemo, endpoint: Vec3| {
            let document = demo.instance().capture(&demo.app.world).unwrap();
            let obj = document.objects.iter().find(|o| o.id == wire).unwrap();
            let [x, y, z] = obj.transform.rotation_degrees.map(f32::to_radians);
            let half =
                Quat::from_euler(EulerRot::YXZ, y, x, z) * Vec3::X * obj.transform.scale[0] * 0.5;
            let center = Vec3::from(obj.transform.translation);
            assert!(
                ((center - half) - pole).length() < 0.001
                    || ((center + half) - pole).length() < 0.001
            );
            assert!(
                ((center - half) - endpoint).length() < 0.001
                    || ((center + half) - endpoint).length() < 0.001
            );
        };
        check(&demo, endpoint);
        press(&mut demo, "R");
        settle(&mut demo, 12);
        check(&demo, endpoint + Vec3::Y * 0.55);
        settle(&mut demo, 30);
        check(&demo, endpoint);
    }
}

#[test]
fn long_cable_stays_visible_between_unloaded_endpoints() {
    let demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",4.0);
        test_build(113,9.0); let a=power::power_id(113); power::connect_power(power::power_id(112),a);
        world::enter_chunk(8,0); test_build(113,9.0); let b=power::power_id(113); power::connect_power(a,b);
        world::enter_chunk(4,0); set_position("camera-rig",[60.0,0.0,0.0]); set_object_variable("camera_pan_progress",1.0);
        chunks::unload_chunk_visuals(144); chunks::unload_chunk_visuals(152); power::update_power();
    "#,
    );
    let resident = controller_numbers(&demo, "resident");
    assert_eq!(resident[144], 0.);
    assert_eq!(resident[152], 0.);
    let scene = demo.instance().capture(&demo.app.world).unwrap();
    assert!(
        scene
            .objects
            .iter()
            .any(|o| o.name == "power-wire" && o.transform.scale[0] > 100.)
    );
}

#[test]
fn progression_deliveries_are_atomic_and_unlock_all_eight_phases_in_order() {
    let demo = stellar_fixture(
        r#"
        let deliveries = [
            [1,12.0,2,8.0,8,8.0], [11,16.0,12,8.0,14,4.0],
            [15,12.0,16,40.0,17,8.0], [15,20.0,16,80.0,17,16.0,14,8.0],
            [18,80.0,15,40.0,16,80.0], [15,160.0,16,240.0,17,80.0,14,40.0],
            [15,200.0,16,320.0,17,120.0,14,80.0]
        ];
        for phase in 0..7 {
            let stock = grid::empty_numbers(64); let cost = deliveries[phase];
            for i in 0..cost.len()/2 { stock[cost[i*2]] = cost[i*2+1]; }
            stock[cost[0]] -= 1.0; backpack::set_stock(stock); progression::deliver_phase();
            if get_object_variable("phase") != phase.to_float() || get_object_list("stock") != stock { throw "partial delivery was charged"; }
            stock[cost[0]] += 1.0; backpack::set_stock(stock); progression::deliver_phase();
            if get_object_variable("phase") != (phase+1).to_float() { throw "phase did not unlock"; }
            if get_object_list("stock") != grid::empty_numbers(64) { throw "wrong delivery cost"; }
            if data::tool_unlocked(4.0) != (phase>=1) || data::tool_unlocked(6.0) != (phase>=2) || data::tool_unlocked(11.0) != (phase>=3) || data::tool_unlocked(5.0) != (phase>=3) { throw "wrong unlock sequence"; }
            for kind in [7.0,8.0,12.0,13.0,14.0,15.0,16.0,17.0,18.0,19.0,20.0,21.0,22.0,23.0,24.0,25.0,26.0,27.0,28.0,29.0] {
                if data::tool_unlocked(kind)!=(phase>=3) {throw "wrong expansion unlock sequence";}
            }
        }
        progression::deliver_phase();
    "#,
    );
    assert_eq!(controller_number(&demo, "phase"), 7.);
    assert_eq!(controller_numbers(&demo, "stock"), vec![0.; 64]);
}

#[test]
fn progression_inventory_preserves_carried_materials_and_blocks_world_input() {
    let mut locked = factory_with_mode(Some(4.), false);
    tick(&mut locked, None);
    press(&mut locked, "I");
    assert_eq!(controller_numbers(&locked, "session")[1], 0.);
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("phase",1.0);
        let stock = get_object_list("stock"); stock[11]=16.0; stock[12]=8.0; stock[14]=4.0; stock[10]=37.0; backpack::set_stock(stock);
        progression::deliver_phase();
    "#,
    );
    press(&mut demo, "I");
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(frame.element("player-slot-0").unwrap().text.contains("37"));
    let before = controller_numbers(&demo, "stock");
    let ticks = number(&demo, "ticks");
    tick_keys(&mut demo, &["D", "Space", "X", "N", "F", "J"]);
    assert_eq!(number(&demo, "cursor_x"), 0.);
    assert_eq!(controller_numbers(&demo, "stock"), before);
    settle(&mut demo, 30);
    assert!(number(&demo, "ticks") > ticks);
    tick_keys(&mut demo, &["I", "D", "Space"]);
    assert_eq!(controller_numbers(&demo, "session")[1], 0.);
    assert_eq!(
        number(&demo, "cursor_x"),
        0.,
        "closing consumes simultaneous world input"
    );
    tick(&mut demo, None);
    press(&mut demo, "I");
    press(&mut demo, "Escape");
    assert_eq!(controller_numbers(&demo, "session")[1], 0.);
}

#[test]
fn progression_new_recipes_run_on_powered_machines_and_collect_real_outputs() {
    let demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",4.0);
        test_build(113,9.0); test_build(120,3.0); test_build(121,11.0); test_build(122,5.0);
        power::connect_power(power::power_id(112),power::power_id(113));
        for cell in [120,121,122] { power::connect_power(power::power_id(113),power::power_id(cell)); }
        power::update_power(); set_object_variable("creative",false);
        machines::select_recipe(120,14.0); machines::select_recipe(121,16.0); machines::select_recipe(122,18.0);
        let stock = get_object_list("stock"); stock[9]=1.0; stock[11]=1.0; stock[8]=2.0; stock[3]=1.0; backpack::set_stock(stock);
        for cell in [120,121,122] { machines::feed_assembler(cell); }
        simulation::factory_step(); simulation::factory_step();
        if get_scene_list("items")[120] != 14.0 || get_scene_list("item_amounts")[120] != 1.0 { throw "glass recipe failed"; }
        if get_scene_list("items")[121] != 16.0 || get_scene_list("item_amounts")[121] != 4.0 { throw "fastener yield failed"; }
        if get_scene_list("items")[122] != 18.0 || get_scene_list("item_amounts")[122] != 1.0 { throw "concrete recipe failed"; }
        for cell in [120,121,122] { inventory::collect_machine(cell); }
    "#,
    );
    let stock = controller_numbers(&demo, "stock");
    assert_eq!([stock[14], stock[16], stock[18]], [1., 4., 1.]);
    assert_eq!([stock[9], stock[11], stock[8], stock[3]], [0.; 4]);
}

#[test]
fn progression_constructor_reserves_whole_batches_and_recipe_switch_refunds_inputs() {
    let demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",4.0);
        test_build(113,9.0); test_build(120,11.0); test_build(122,5.0);
        power::connect_power(power::power_id(112),power::power_id(113)); power::connect_power(power::power_id(113),power::power_id(120)); power::update_power();
        machines::select_recipe(120,16.0);
        let items = get_scene_list("items"); items[120]=16.0; set_scene_list("items",items);
        let amounts = get_scene_list("item_amounts"); amounts[120]=96.0; set_scene_list("item_amounts",amounts);
        let inputs = get_scene_list("input_items"); inputs[120]=11.0; set_scene_list("input_items",inputs);
        let input_amounts = get_scene_list("input_amounts"); input_amounts[120]=1.0; set_scene_list("input_amounts",input_amounts);
        simulation::factory_step(); simulation::factory_step();
        if get_scene_list("item_amounts")[120] != 100.0 || get_scene_list("input_amounts")[120] != 0.0 { throw "batch must fit exactly"; }
        amounts = get_scene_list("item_amounts"); amounts[120]=97.0; set_scene_list("item_amounts",amounts);
        input_amounts[120]=1.0; set_scene_list("input_amounts",input_amounts); set_scene_list("input_items",inputs);
        simulation::factory_step(); simulation::factory_step();
        if get_scene_list("item_amounts")[120] != 97.0 || get_scene_list("input_amounts")[120] != 1.0 { throw "partial or overflowing batch"; }
        inventory::collect_machine(120); machines::select_recipe(120,15.0);
        input_amounts[120]=100.0; set_scene_list("input_amounts",input_amounts); set_scene_list("input_items",inputs);
        machines::select_recipe(120,16.0);
        if get_scene_list("input_amounts")[120] != 0.0 { throw "new output yield can deadlock a full input buffer"; }
        machines::select_recipe(122,18.0); machines::feed_assembler(122); machines::select_recipe(122,13.0);
        if get_object_list("stock")[8] != 20.0 || get_object_list("stock")[3] != 10.0 { throw "changed recipe lost ingredients"; }
        if get_scene_list("assembler_iron")[122] != 0.0 || get_scene_list("assembler_copper")[122] != 0.0 { throw "old ingredient types remained"; }
    "#,
    );
    assert!(machine_load(&demo, 120) <= 100.);
}

#[test]
fn progression_generator_consumes_crafted_miners_and_cables_charge_only_on_success() {
    let demo = stellar_fixture(
        r#"
        set_object_variable("phase",3.0);
        let stock = get_object_list("stock"); stock[16]=80.0; stock[15]=20.0; stock[21]=1.0; stock[12]=6.0; stock[11]=4.0; stock[17]=2.0;
        backpack::set_stock(stock);
        if data::pay_for_build(6.0) || get_object_list("stock") != stock { throw "generator partial payment"; }
        progression::craft(21); test_build(130,6.0);
        for item in [11,12,15,16,21] { if get_object_list("stock")[item] != 0.0 { throw "wrong generator cost"; } }
        set_object_variable("creative",true); test_build(113,9.0); set_object_variable("creative",false);
        set_scene_variable("selected",10.0);
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",0.0); power::wire_selected();
        set_scene_variable("cursor_x",1.0); power::wire_selected();
        if get_object_list("stock")[17] != 1.0 { throw "cable was not paid"; }
        set_scene_variable("cursor_x",0.0); power::wire_selected(); set_scene_variable("cursor_x",1.0); power::wire_selected();
        if get_object_list("stock")[17] != 1.0 { throw "duplicate cable consumed material"; }
    "#,
    );
    assert_eq!(numbers(&demo, "builds")[130], 6.);
    assert_eq!(controller_numbers(&demo, "stock")[21], 0.);
}

#[test]
fn progression_landing_expansion_preserves_obstacles_and_rocket_stages_and_modal_work() {
    use bozzard_scene::{BlueprintHidden, Layer};
    let hidden = |demo: &SceneDemo, id: &str| {
        demo.app
            .world
            .get::<BlueprintHidden>(demo.instance().entity(id).unwrap())
            .unwrap()
            .0
    };
    for (phase, site, lower, upper) in [
        (4., false, false, false),
        (5., true, false, false),
        (6., true, true, false),
        (7., true, true, true),
    ] {
        let demo = stellar_fixture(&format!(
            "set_object_variable(\"phase\",{phase:.1}); visuals::update_projects();"
        ));
        assert_eq!(!hidden(&demo, "site-0"), site);
        assert_eq!(!hidden(&demo, "rocket-lower-0"), lower);
        assert_eq!(!hidden(&demo, "rocket-upper-0"), upper);
        for i in 0..3 {
            assert_eq!(
                hidden(&demo, &format!("pod-{i}")),
                site,
                "service pad replaces the starter pod without overlapping it"
            );
        }
    }
    let demo = stellar_fixture(
        r#"
        set_object_variable("phase",4.0); set_object_variable("creative",true); test_build(127,11.0); set_object_variable("creative",false);
        let stock=get_object_list("stock"); stock[18]=80.0; stock[15]=40.0; stock[16]=80.0; backpack::set_stock(stock);
        world::enter_chunk(1,0); progression::deliver_phase();
        if get_object_variable("phase") != 4.0 || get_object_list("stock") != stock { throw "expansion destroyed an archived machine or consumed payment"; }
        world::enter_chunk(0,0);
    "#,
    );
    assert_eq!(numbers(&demo, "builds")[127], 11.);
    let mut demo = stellar_fixture(
        r#"set_object_variable("phase",7.0); visuals::update_projects(); set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0);"#,
    );
    let before = hidden(&demo, "rocket-light-0");
    settle(&mut demo, 31);
    assert_ne!(hidden(&demo, "rocket-light-0"), before);
    press(&mut demo, "E");
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        frame
            .element("rocket-current")
            .unwrap()
            .text
            .contains("STELLAR-BX")
    );
    assert!(
        frame
            .element("rocket-moon")
            .unwrap()
            .text
            .contains("STELLA-Z2")
    );
    for i in 0..8 {
        assert!(
            frame
                .element(&format!("rocket-future-{i}"))
                .unwrap()
                .text
                .contains("Coming soon")
        );
    }
    assert!(frame.element("rocket-launch").unwrap().enabled);
    let ticks = number(&demo, "ticks");
    tick_keys(&mut demo, &["D", "Space", "X", "N"]);
    settle(&mut demo, 30);
    assert_eq!(number(&demo, "cursor_x"), 0.);
    assert!(number(&demo, "ticks") > ticks);
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "session")[2], 0.);
    press(&mut demo, "N");
    assert!(
        hidden(&demo, "site-0")
            && hidden(&demo, "rocket-lower-0")
            && hidden(&demo, "rocket-upper-0")
    );
}

#[test]
fn progression_new_nodes_are_seeded_and_constructor_recipe_survives_chunk_streaming() {
    let mut demo = stellar_fixture(
        r#"
        for kind in [1,2,3,8,9] { if !get_scene_list("nodes").contains(kind.to_float()) { throw "starter resource missing"; } }
        set_object_variable("phase",4.0); set_object_variable("creative",true); test_build(114,11.0);
        machines::select_recipe(114,17.0); machines::feed_assembler(114);
        world::enter_chunk(5,0); chunks::update_chunk_residency();
        let deposits = 0; for kind in get_scene_list("nodes") { if kind > 0.0 { deposits += 1; } }
        if deposits < 2 || deposits > 4 { throw "neighbor resource density"; }
        world::enter_chunk(0,0);
    "#,
    );
    assert_eq!(numbers(&demo, "builds")[114], 11.);
    assert_eq!(controller_numbers(&demo, "recipes")[114], 17.);
    assert_eq!(numbers(&demo, "input_items")[114], 12.);
    assert_eq!(numbers(&demo, "input_amounts")[114], 10.);
    move_cursor(&mut demo, 2, 0);
    press(&mut demo, "E");
    let frame = demo
        .instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap();
    assert!(
        frame
            .element("assembler-title")
            .unwrap()
            .text
            .contains("CONSTRUCTOR")
    );
}

#[test]
fn journal_cached_recipe_page_updates_on_hover_selection_crafting_and_reopening() {
    use bozzard_scene::{Layer, middleware::ui::Input};
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("phase",1.0);
        let stock=get_object_list("stock"); stock[1]=2.0; stock[2]=1.0; backpack::set_stock(stock);
        set_object_variable("journal_open",true); set_object_variable("journal_page",2.0);
    "#,
    );
    settle(&mut demo, 20);
    let frame = |demo: &SceneDemo| {
        demo.instance()
            .ui_frame(&demo.app.world, Layer::ThreeD, [1080., 600.])
            .unwrap()
    };
    let initial = frame(&demo);
    assert!(
        initial
            .element("journal-right-body")
            .unwrap()
            .text
            .contains("have 2")
    );
    let rect = initial.element("journal-recipe-12").unwrap().rect;
    demo.ui_input(
        Layer::ThreeD,
        [1080., 600.],
        Input::PointerMove([
            rect.min[0] + rect.size[0] * 0.5,
            rect.min[1] + rect.size[1] * 0.5,
        ]),
    )
    .unwrap();
    let hovered = frame(&demo);
    assert!(hovered.element("journal-recipe-12").unwrap().hovered);
    assert_eq!(
        hovered.element("journal-right-title").unwrap().text,
        "iron ingot"
    );
    click_widget(&mut demo, "journal-craft-one");
    assert_eq!(controller_numbers(&demo, "stock")[1], 1.);
    assert!(
        frame(&demo)
            .element("journal-right-body")
            .unwrap()
            .text
            .contains("have 1")
    );
    click_widget(&mut demo, "journal-craft-ten");
    assert_eq!(controller_numbers(&demo, "stock")[1], 0.);
    assert!(!frame(&demo).element("journal-craft-one").unwrap().enabled);
    click_widget(&mut demo, "journal-recipe-12");
    assert_eq!(
        frame(&demo).element("journal-right-title").unwrap().text,
        "copper ingot"
    );
    assert!(frame(&demo).element("journal-craft-one").unwrap().enabled);
    click_widget(&mut demo, "journal-craft-one");
    assert_eq!(controller_numbers(&demo, "stock")[2], 0.);
    press(&mut demo, "J");
    settle(&mut demo, 15);
    assert!(frame(&demo).element("journal-book").is_none());
    press(&mut demo, "J");
    settle(&mut demo, 15);
    assert_eq!(
        frame(&demo).element("journal-right-title").unwrap().text,
        "copper ingot"
    );
    assert!(!frame(&demo).element("journal-craft-one").unwrap().enabled);
    click_widget(&mut demo, "journal-recipe-11");
    assert_eq!(
        frame(&demo).element("journal-right-title").unwrap().text,
        "iron ingot"
    );
}

fn world_point(demo: &SceneDemo, position: [f32; 3]) -> [f32; 2] {
    let camera = demo
        .app
        .world
        .get::<bozzard_scene::Camera>(demo.instance().entity("camera").unwrap())
        .unwrap();
    let matrix = camera.projection(1080. / 600.).unwrap()
        * demo.instance().global_transforms(&demo.app.world).unwrap()["camera"].inverse();
    let p = matrix * glam::Vec3::from(position).extend(1.);
    [
        (p.x / p.w * 0.5 + 0.5) * 1080.,
        (0.5 - p.y / p.w * 0.5) * 600.,
    ]
}

fn click_world(demo: &mut SceneDemo, position: [f32; 3], secondary: bool) {
    use bozzard_scene::middleware::ui::Input;
    let point = world_point(demo, position);
    ui_event(demo, Input::PointerMove(point));
    if secondary {
        ui_event(demo, Input::SecondaryDown(point));
    } else {
        ui_event(demo, Input::PointerDown(point));
        ui_event(demo, Input::PointerUp(point));
    }
    tick(demo, None);
}

fn shown(demo: &SceneDemo, id: &str) -> bool {
    demo.instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap()
        .element(id)
        .is_some()
}

#[test]
fn mouse_placement_hover_body_picking_and_modal_blocking() {
    use bozzard_scene::middleware::ui::Input;
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        let nodes=get_scene_list("nodes"); nodes[157]=0.0; set_scene_list("nodes",nodes);
    "#,
    );
    click_widget(&mut demo, "slot-1");
    assert_eq!(controller_numbers(&demo, "session")[40], 1.);
    assert_eq!(number(&demo, "selected"), 3.);
    let point = world_point(&demo, [0., 0.11, 3.]);
    ui_event(&mut demo, Input::PointerMove(point));
    tick(&mut demo, None);
    let hover = demo.instance().global_transforms(&demo.app.world).unwrap()["hover-tile"]
        .transform_point3(glam::Vec3::ZERO);
    assert!(
        (hover.x - 0.).abs() < 0.01 && (hover.z - 3.).abs() < 0.01,
        "{hover:?}"
    );
    click_world(&mut demo, [0., 0.11, 3.], false);
    assert_eq!(numbers(&demo, "builds")[157], 3.);
    assert_eq!(
        (number(&demo, "cursor_x"), number(&demo, "cursor_z")),
        (0., 3.)
    );
    // Clicking the elevated silhouette still picks its own tile, including after orbit/zoom.
    click_world(&mut demo, [0., 1.1, 3.], false);
    assert_eq!(
        (number(&demo, "cursor_x"), number(&demo, "cursor_z")),
        (0., 3.)
    );
    tick_keys(&mut demo, &["Ctrl", "R"]);
    settle(&mut demo, 60);
    click_world(&mut demo, [0., 1.1, 3.], false);
    assert_eq!(
        (number(&demo, "cursor_x"), number(&demo, "cursor_z")),
        (0., 3.)
    );
    let before = numbers(&demo, "builds");
    click_widget(&mut demo, "menu-open");
    click_world(&mut demo, [3., 0.11, 3.], false);
    assert_eq!(numbers(&demo, "builds"), before);
    assert_eq!(
        (number(&demo, "cursor_x"), number(&demo, "cursor_z")),
        (0., 3.)
    );
}

#[test]
fn mouse_power_context_links_unlinks_and_hides_unavailable_actions() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        test_build(154,9.0); test_build(160,9.0);
    "#,
    );
    click_world(&mut demo, [-3., 0.8, 3.], true);
    assert!(shown(&demo, "world-link"));
    assert!(!shown(&demo, "world-unlink"));
    click_widget(&mut demo, "world-link");
    assert_eq!(number(&demo, "selected"), 10.);
    click_world(&mut demo, [3., 0.8, 3.], false);
    click_world(&mut demo, [0., 1.47, 3.], true);
    assert_eq!(controller_numbers(&demo, "session")[41], 2.);
    assert!(shown(&demo, "world-unlink"));
    click_widget(&mut demo, "world-unlink");
    click_world(&mut demo, [-3., 0.8, 3.], true);
    assert!(!shown(&demo, "world-unlink"));
    click_widget(&mut demo, "world-cancel");
    assert!(!shown(&demo, "world-context"));
    // Before the rocket is assembled, the exposed pod body is an electrical source.
    let mut demo = stellar_fixture(r#"set_object_variable("phase",4.0);"#);
    click_world(&mut demo, [0., 0.7, 0.], true);
    assert!(shown(&demo, "world-link"));
}

#[test]
fn mouse_inspect_dispatches_every_machine_buffer_to_its_interface() {
    let expansion_manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/factory-machines/expansion-manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let cases = [
        (1, "machine-inspect-overlay"),
        (2, "machine-inspect-overlay"),
        (3, "assembler-overlay"),
        (4, "storage-panel"),
        (5, "assembler-overlay"),
        (7, "machine-inspect-overlay"),
        (8, "machine-inspect-overlay"),
        (11, "assembler-overlay"),
    ]
    .into_iter()
    .chain((12..=25).map(|kind| (kind, "assembler-overlay")))
    .chain((26..=29).map(|kind| (kind, "machine-inspect-overlay")));
    for (kind, panel) in cases {
        let setup = format!(
            r#"
            set_object_variable("creative",true); set_object_variable("phase",7.0);
            let nodes=get_scene_list("nodes");nodes[157]=if {kind}==1 {{1.0}}else if {kind}==12 {{7.0}}else if {kind}==13 {{6.0}}else{{0.0}};
            set_scene_list("nodes",nodes);set_scene_variable("cursor_x",0.0);set_scene_variable("cursor_z",3.0);
            set_scene_variable("selected",{kind}.0);building::place_selected();
        "#
        );
        let mut demo = stellar_fixture(&setup);
        assert_eq!(numbers(&demo, "builds")[157], kind as f32);
        let height = if (12..=25).contains(&kind) {
            expansion_manifest["machines"][(kind - 12) as usize]["bounds"][1][1]
                .as_f64()
                .unwrap() as f32
                * 0.9
        } else {
            0.3
        };
        // Click the raised body, then inspect it through real pointer/UI events.
        // The projected ground tile may lie behind tall machine silhouettes.
        click_world(&mut demo, [0., height, 3.], false);
        assert_eq!(
            (number(&demo, "cursor_x"), number(&demo, "cursor_z")),
            (0., 3.),
            "body picking for kind {kind}"
        );
        click_world(&mut demo, [0., height, 3.], true);
        assert!(shown(&demo, "world-inspect"), "kind {kind}");
        assert!(!shown(&demo, "world-unlink"));
        click_widget(&mut demo, "world-inspect");
        settle(&mut demo, 20);
        assert!(shown(&demo, panel), "missing {panel} for {kind}");
        let ticks = number(&demo, "ticks");
        settle(&mut demo, 25);
        assert!(number(&demo, "ticks") > ticks);
        press(&mut demo, "E");
        settle(&mut demo, 20);
        assert!(!shown(&demo, panel));
    }
}

#[test]
fn mouse_selects_a_machine_in_another_loaded_region_with_camera_zoom() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        world::enter_chunk(1,0); test_build(150,2.0); world::enter_chunk(0,0);
        set_position("camera-rig",[0.0,0.0,0.0]); set_object_variable("camera_pan_progress",1.0);
        set_camera_size("camera",32.0); set_object_variable("camera_zoom",32.0); set_object_variable("camera_zoom_target",32.0);
    "#,
    );
    click_world(&mut demo, [8., 0.18, 3.], false);
    assert_eq!(
        (number(&demo, "chunk_x"), number(&demo, "chunk_z")),
        (1., 0.)
    );
    assert_eq!(
        (number(&demo, "cursor_x"), number(&demo, "cursor_z")),
        (-7., 3.)
    );
    assert_eq!(numbers(&demo, "builds")[150], 2.);
    assert!(controller_number(&demo, "camera_pan_progress") < 1.);
}

#[test]
fn mouse_wired_machine_inspects_and_cable_unlink_preserves_other_edges() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        test_build(154,9.0); test_build(157,3.0); test_build(160,9.0);
        power::connect_power(power::power_id(154),power::power_id(157));
        power::connect_power(power::power_id(154),power::power_id(160)); power::update_power();
        power::disconnect_power(power::power_id(154),power::power_id(160));
        let graph=power::power_graph();
        if graph[power::power_id(154).to_string()].len()!=3 || graph[power::power_id(160).to_string()].len()!=2 { throw "unlink damaged other edges"; }
    "#,
    );
    click_world(&mut demo, [0., 1.08, 3.], true);
    assert!(shown(&demo, "world-inspect"));
    assert!(!shown(&demo, "world-link"));
}

#[test]
fn backpack_drag_swap_merge_split_destroy_and_cancel_preserve_stacks() {
    use bozzard_scene::middleware::ui::Input;
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("phase",7.0);
        backpack::give(1.0,150.0); backpack::give(2.0,21.0);
    "#,
    );
    press(&mut demo, "I");
    let drag = |demo: &mut SceneDemo, from: usize, to: usize| {
        let a = ui_point(demo, &format!("player-slot-{from}"));
        let b = ui_point(demo, &format!("player-slot-{to}"));
        ui_event(demo, Input::PointerDown(a));
        tick(demo, None);
        assert!(shown(demo, "backpack-drag"));
        ui_event(demo, Input::PointerMove(b));
        ui_event(demo, Input::PointerUp(b));
        tick(demo, None);
        assert!(!shown(demo, "backpack-drag"));
    };
    drag(&mut demo, 0, 24);
    assert_eq!(&controller_numbers(&demo, "session")[112..114], &[1., 100.]);
    drag(&mut demo, 1, 2); // swap iron and copper
    assert_eq!(
        &controller_numbers(&demo, "session")[66..70],
        &[2., 21., 1., 50.]
    );
    drag(&mut demo, 24, 2); // partial merge
    assert_eq!(&controller_numbers(&demo, "session")[112..114], &[1., 50.]);
    assert_eq!(&controller_numbers(&demo, "session")[68..70], &[1., 100.]);
    let point = ui_point(&demo, "player-slot-1");
    ui_event(&mut demo, Input::SecondaryDown(point));
    tick(&mut demo, None);
    click_widget(&mut demo, "backpack-split");
    assert_eq!(
        &controller_numbers(&demo, "session")[64..68],
        &[2., 10., 2., 11.]
    );
    let stock = controller_numbers(&demo, "stock");
    let point = ui_point(&demo, "player-slot-0");
    ui_event(&mut demo, Input::PointerDown(point));
    tick(&mut demo, None);
    ui_event(&mut demo, Input::CancelPointer);
    tick(&mut demo, None);
    assert_eq!(controller_numbers(&demo, "stock"), stock);
    ui_event(&mut demo, Input::SecondaryDown(point));
    tick(&mut demo, None);
    click_widget(&mut demo, "backpack-destroy");
    assert_eq!(controller_numbers(&demo, "stock")[2], 11.);
    click_widget(&mut demo, "backpack-destroy-all");
    assert_eq!(controller_numbers(&demo, "stock"), vec![0.; 64]);
    assert!(
        controller_numbers(&demo, "session")[64..114]
            .iter()
            .all(|v| *v == 0.)
    );
}

#[test]
fn backpack_capacity_transactions_are_atomic_and_collection_leaves_overflow() {
    let demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        if backpack::give(1.0,2600.0)!=2500.0 { throw "capacity not 25 stacks of 100"; }
        let slots=backpack::slots();
        if backpack::split(0) { throw "split without space"; }
        let stock=get_object_list("stock"); stock[2]=1.0;
        if backpack::set_stock(stock) || backpack::slots()!=slots { throw "overflow transaction changed inventory"; }
        stock[1]=2400.0;
        if !backpack::set_stock(stock) || get_object_list("stock")[2]!=1.0 { throw "net transaction did not free a slot"; }
        test_build(157,3.0);
        let items=get_scene_list("items"); items[157]=11.0; set_scene_list("items",items);
        let amounts=get_scene_list("item_amounts"); amounts[157]=20.0; set_scene_list("item_amounts",amounts);
        inventory::collect_machine(157);
        if get_scene_list("item_amounts")[157]!=20.0 { throw "full inventory consumed output"; }
        backpack::destroy(0); backpack::give(11.0,95.0);
        inventory::collect_machine(157);
        set_object_variable("creative",false);
        let before=backpack::slots(); progression::craft(16);
        if backpack::slots()!=before { throw "crafting lost ingredients when outputs did not fit"; }
        set_object_variable("creative",true);
        test_build(158,4.0);
        let stored=grid::empty_numbers(32); stored[0]=11.0; stored[1]=40.0; inventory::storage_write(158,stored);
        let counts=get_scene_list("counts"); counts[11]=40.0; set_scene_list("counts",counts);
        inventory::take_storage(158);
        if inventory::storage_read(158)[1]!=40.0 || get_scene_list("counts")[11]!=40.0 { throw "full backpack consumed storage"; }
        if get_scene_list("item_amounts")[157]!=15.0 || get_object_list("stock")[11]!=100.0 { throw "partial collection lost items"; }
        // Recipe changes may refund buffers, but must leave everything intact if it cannot fit.
        let inputs=get_scene_list("input_items"); inputs[157]=9.0; set_scene_list("input_items",inputs);
        let amounts=get_scene_list("input_amounts"); amounts[157]=10.0; set_scene_list("input_amounts",amounts);
        machines::select_recipe(157,12.0);
        if get_scene_list("input_amounts")[157]!=10.0 { throw "recipe refund lost ingredients"; }
        backpack::clear();
        if inventory::storage_read(158)[1]!=40.0 || get_scene_list("counts")[11]!=40.0 || get_scene_list("item_amounts")[157]!=15.0 { throw "destroy all affected factory contents"; }
    "#,
    );
    assert_eq!(controller_numbers(&demo, "stock")[11], 0.);
}

#[test]
fn rocket_flight_closes_ui_lifts_swaps_offscreen_lands_and_preserves_inventory() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("phase",7.0);
        backpack::give(1.0,131.0); backpack::move_stack(0,24);
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0);
    "#,
    );
    let stacks = controller_numbers(&demo, "session")[64..114].to_vec();
    let height = |demo: &SceneDemo| {
        demo.instance().global_transforms(&demo.app.world).unwrap()["rocket-rig"]
            .transform_point3(glam::Vec3::ZERO)
            .y
    };
    for destination in [1., 0.] {
        let cursor = (number(&demo, "cursor_x"), number(&demo, "cursor_z"));
        press(&mut demo, "E");
        assert!(shown(&demo, "player-rocket-panel"));
        click_widget(&mut demo, "rocket-launch");
        assert!(!shown(&demo, "player-rocket-panel"));
        assert_eq!(controller_numbers(&demo, "session")[44], 1.);
        let ticks = number(&demo, "ticks");
        settle(&mut demo, 60);
        assert!(height(&demo) > 5. && height(&demo) < 12.);
        let transforms = demo.instance().global_transforms(&demo.app.world).unwrap();
        for part in ["rocket-lower-0", "rocket-upper-0", "rocket-exhaust"] {
            let position = transforms[part].transform_point3(glam::Vec3::ZERO);
            assert!(
                (position.y - height(&demo) - 0.10).abs() < 0.001,
                "ship part {part} did not follow the shared flight pivot"
            );
            assert_eq!(position.z, 1.);
        }
        assert_eq!(
            transforms["site-0"].transform_point3(glam::Vec3::ZERO),
            glam::Vec3::ZERO,
            "station lifted off with the ship"
        );
        assert_ne!(controller_numbers(&demo, "session")[7], destination);
        tick_keys(&mut demo, &["E", "I", "M", "Escape", "D", "Space"]);
        assert!(!shown(&demo, "player-rocket-panel") && !shown(&demo, "menu-panel"));
        assert_eq!(
            (number(&demo, "cursor_x"), number(&demo, "cursor_z")),
            cursor
        );
        settle(&mut demo, 61);
        assert_eq!(controller_numbers(&demo, "session")[7], destination);
        assert_eq!(controller_numbers(&demo, "session")[44], 2.);
        assert!(height(&demo) > 30.);
        settle(&mut demo, 75);
        assert!(height(&demo) > 5. && height(&demo) < 20.);
        settle(&mut demo, 75);
        assert_eq!(controller_numbers(&demo, "session")[44], 0.);
        assert_eq!(height(&demo), 0.);
        assert!(number(&demo, "ticks") > ticks);
        assert_eq!(&controller_numbers(&demo, "session")[64..114], &stacks);
    }
}

// Run one simulation beat per hook invocation, as in the game. Large test batches
// must not consume the entire script safety budget in a single on_start callback.
fn factory_code(demo: &mut SceneDemo, code: &str, steps: usize) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../earth-factory/scenes/scripts/earth_factory.rs");
    let source = std::fs::read_to_string(path)
        .unwrap()
        .replace("fn on_update(me, dt)", "fn normal_update(me, dt)");
    let script = format!("{source}\nfn on_update(me,dt) {{ {code} }}");
    demo.with_instance(|instance, _| instance.register_script("earth-factory".into(), script))
        .unwrap();
    for _ in 0..steps {
        tick(demo, None);
    }
}
fn restore_factory_update(demo: &mut SceneDemo) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../earth-factory/scenes/scripts/earth_factory.rs");
    demo.with_instance(|instance, _| {
        instance.register_script(
            "earth-factory".into(),
            std::fs::read_to_string(path).unwrap(),
        )
    })
    .unwrap();
}

// World-coordinate fixture: production and transport may straddle any chunk seam.
fn multi_region_factory(layout: &str, setup: &str) -> SceneDemo {
    stellar_fixture(&format!(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        let terminals=[];
        for row in {layout} {{
            let wx=row[0]; let wz=row[1]; let kind=row[2].to_float();
            let cx=floor((wx+7).to_float()/15.0).to_int(); let cz=floor((wz+7).to_float()/15.0).to_int();
            world::enter_chunk(cx,cz);
            let x=wx-cx*15; let z=wz-cz*15; let cell=grid::index(x,z);
            let nodes=get_scene_list("nodes"); nodes[cell]=row[4].to_float(); set_scene_list("nodes",nodes);
            grid::cache_put("chunk_nodes",grid::current_chunk(),grid::pack_numbers(nodes));
            set_scene_variable("cursor_x",x.to_float()); set_scene_variable("cursor_z",z.to_float());
            set_scene_variable("selected",kind); set_scene_variable("direction",row[3].to_float()); building::place_selected();
            if get_scene_list("builds")[cell]!=kind {{ throw "test machine not placed"; }}
            if power::power_demand(row[2])>0 {{ terminals.push(power::power_id(cell)); }}
        }}
        world::enter_chunk(0,0); test_build(157,9.0);
        power::connect_power(power::power_id(112),power::power_id(157));
        for id in terminals {{ power::connect_power(power::power_id(157),id); }} power::update_power();
        {setup}
    "#
    ))
}

#[test]
fn storage_exports_across_chunk_seams_while_its_planet_is_unloaded() {
    let mut demo = multi_region_factory(
        "[[7,0,4,0,0],[8,0,2,0,0],[9,0,4,0,0]]",
        r#"
        let slots=grid::empty_numbers(32); slots[0]=11.0; slots[1]=100.0;
        inventory::storage_write(119,slots);
        let counts=get_scene_list("counts"); counts[11]=100.0; set_scene_list("counts",counts);
        "#,
    );
    board_other_planet(&mut demo);
    bozzard_demo::factory::host::HostRuntime::start(
        &mut demo.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    factory_code(&mut demo, "simulation::factory_step();", 7);
    let runtime = demo.app.world.resource::<BlueprintRuntime>().unwrap();
    let motions =
        bozzard_demo::factory::transport::capture(runtime, number(&demo, "ticks")).unwrap();
    assert_eq!(motions.len(), 1);
    assert_eq!(
        (motions[0].planet, motions[0].source, motions[0].target),
        (0, 4, 2)
    );
    assert_ne!(motions[0].from / 225, motions[0].to / 225);
    // The exact inventory and its transport events must remain valid for Steam replicas.
    coop_world(&demo);
    assert_eq!(controller_numbers(&demo, "session")[8 + 11], 99.);
    board_other_planet(&mut demo);
    assert_eq!(inventory(&demo, 119)[0], (11., 96.));
    factory_code(&mut demo, "world::enter_chunk(1,0);", 1);
    assert_eq!(inventory(&demo, 106)[0], (11., 3.));
    assert_eq!(numbers(&demo, "item_amounts")[105], 1.);
    assert_eq!(numbers(&demo, "counts")[11], 99.);
}

#[test]
fn all_regions_mine_smelt_and_deliver_across_seams_in_every_direction() {
    for direction in 0..4 {
        let rows: Vec<String> = [
            (6, 1, 1),
            (7, 2, 0),
            (8, 2, 0),
            (9, 3, 0),
            (10, 2, 0),
            (11, 4, 0),
        ]
        .into_iter()
        .map(|(x, kind, node)| {
            let (wx, wz) = match direction {
                0 => (x, 0),
                1 => (0, x),
                2 => (-x, 0),
                _ => (0, -x),
            };
            format!("[{wx},{wz},{kind},{direction},{node}]")
        })
        .collect();
        for away in [false, true] {
            let mut demo = multi_region_factory(
                &format!("[{}]", rows.join(",")),
                &format!("if {away} {{ world::enter_chunk(-4,-4); }}"),
            );
            factory_code(&mut demo, "simulation::factory_step();", 80);
            assert!(
                numbers(&demo, "counts")[11] >= 10.,
                "direction {direction}, away={away}"
            );
            assert_eq!(controller_numbers(&demo, "session")[114], 2.);
        }
    }
}

#[test]
fn all_regions_continue_unloaded_and_keep_models_bounded_on_return() {
    let mut demo = multi_region_factory(
        "[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,3,0,0],[10,0,2,0,0],[11,0,4,0,0]]",
        "",
    );
    factory_code(&mut demo, "simulation::factory_step();", 35);
    factory_code(
        &mut demo,
        r#"
        world::enter_chunk(5,5);
        chunks::unload_chunk_visuals(144); chunks::unload_chunk_visuals(145);
    "#,
        1,
    );
    let before = numbers(&demo, "counts")[11];
    let entities = demo.instance().document().objects.len();
    factory_code(&mut demo, "simulation::factory_step();", 40);
    assert!(numbers(&demo, "counts")[11] > before);
    assert!(
        demo.instance().document().objects.len() <= entities,
        "dormant models must stay unloaded"
    );
    assert_eq!(controller_numbers(&demo, "resident")[144], 0.);
    assert_eq!(controller_numbers(&demo, "resident")[145], 0.);
    factory_code(
        &mut demo,
        r#"
        world::enter_chunk(1,0);
        let stored=inventory::storage_read(grid::index(-4,0)); let total=0.0;
        for slot in 0..16 { if stored[slot*2]==11.0 { total+=stored[slot*2+1]; } }
        if total!=get_scene_list("counts")[11] { throw "return restored stale storage"; }
    "#,
        1,
    );
    restore_factory_update(&mut demo);
    let before = numbers(&demo, "counts")[11];
    settle(&mut demo, 80);
    assert!(numbers(&demo, "counts")[11] > before);
    press(&mut demo, "N");
    assert!(numbers(&demo, "counts").iter().all(|n| *n == 0.));
    assert_eq!(controller_numbers(&demo, "session")[114], 0.);
}

#[test]
fn all_regions_share_one_transfer_snapshot_and_preserve_backpressure() {
    let demo = multi_region_factory(
        "[[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]",
        r#"
        let amounts=get_scene_list("item_amounts"); amounts[119]=1.0; set_scene_list("item_amounts",amounts);
        let items=get_scene_list("items"); items[119]=11.0; set_scene_list("items",items);
        world::enter_chunk(1,0);
        let amounts=get_scene_list("item_amounts"); amounts[105]=1.0; set_scene_list("item_amounts",amounts);
        let items=get_scene_list("items"); items[105]=11.0; set_scene_list("items",items);
        let slots=[]; for slot in 0..16 { slots.push(11.0); slots.push(100.0); } inventory::storage_write(106,slots);
        let counts=get_scene_list("counts"); counts[11]=1600.0; set_scene_list("counts",counts);
        world::enter_chunk(0,0); simulation::factory_step();
        if get_scene_list("item_amounts")[119]!=1.0 { throw "full destination consumed source"; }
        world::enter_chunk(1,0);
        if get_scene_list("item_amounts")[105]!=1.0 { throw "full storage consumed belt"; }
        let slots=inventory::storage_read(106); slots[1]=99.0; inventory::storage_write(106,slots);
        let counts=get_scene_list("counts"); counts[11]=1599.0; set_scene_list("counts",counts);
        simulation::factory_step();
        if get_scene_list("item_amounts")[105]!=0.0 || get_scene_list("counts")[11]!=1600.0 { throw "storage did not accept one item"; }
        world::enter_chunk(0,0);
        if get_scene_list("item_amounts")[119]!=1.0 { throw "source jumped into occupied belt in same beat"; }
        simulation::factory_step();
        if get_scene_list("item_amounts")[119]!=0.0 { throw "seam did not release after backpressure cleared"; }
        world::enter_chunk(1,0);
        if get_scene_list("item_amounts")[105]!=1.0 { throw "incoming item jumped directly into storage"; }
        simulation::factory_step();
        if get_scene_list("item_amounts")[105]!=1.0 || get_scene_list("counts")[11]!=1600.0 { throw "full storage lost overflow"; }
    "#,
    );
    assert_eq!(numbers(&demo, "counts")[11], 1600.);
}

#[test]
fn all_regions_animate_remote_items_and_keep_their_position_when_crossing() {
    let demo = multi_region_factory(
        "[[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]",
        r#"
        let items=get_scene_list("items"); items[119]=12.0; set_scene_list("items",items);
        let amounts=get_scene_list("item_amounts"); amounts[119]=1.0; set_scene_list("item_amounts",amounts);
        simulation::factory_step(); visuals::animate_items(0.25);
        set_scene_variable("clock",0.08);
        world::enter_chunk(1,0);
        let handle=get_scene_list("item_visuals")[105];
        if handle=="" || abs(get_position(handle)[0]-7.25)>0.001 { throw "crossing snapped the moving item"; }
        if get_scene_variable("clock")!=0.08 { throw "crossing reset factory time"; }
        visuals::animate_items(0.75);
        if abs(get_position(handle)[0]-7.75)>0.001 { throw "item did not continue across seam"; }
        world::enter_chunk(0,0); visuals::animate_items(1.0);
        if abs(get_position(handle)[0]-8.0)>0.001 { throw "remote item animation froze"; }
        simulation::factory_step(); visuals::animate_items(0.5);
        if abs(get_position(handle)[0]-8.5)>0.001 { throw "remote delivery animation froze"; }
        if get_scene_list("counts")[12]!=1.0 { throw "remote storage delivery failed"; }
        world::enter_chunk(1,0);
        if inventory::storage_read(106)[1]!=1.0 { throw "return lost delivery"; }
    "#,
    );
    assert_eq!(numbers(&demo, "counts")[12], 1.);
}

#[test]
fn all_regions_constructor_and_assembler_finish_and_deliver_while_unloaded() {
    let mut demo = multi_region_factory(
        "[[6,2,11,0,0],[7,2,2,0,0],[8,2,4,0,0],[6,4,5,0,0],[7,4,2,0,0],[8,4,4,0,0]]",
        r#"
        let constructor=grid::index(6,2); let assembler=grid::index(6,4);
        let recipes=get_object_list("recipes"); recipes[constructor]=16.0; recipes[assembler]=18.0; set_object_list("recipes",recipes);
        let inputs=get_scene_list("input_items"); inputs[constructor]=11.0; set_scene_list("input_items",inputs);
        let amounts=get_scene_list("input_amounts"); amounts[constructor]=4.0; set_scene_list("input_amounts",amounts);
        let first=get_scene_list("assembler_iron"); first[assembler]=8.0; set_scene_list("assembler_iron",first);
        let second=get_scene_list("assembler_copper"); second[assembler]=4.0; set_scene_list("assembler_copper",second);
        world::enter_chunk(5,5); chunks::unload_chunk_visuals(144); chunks::unload_chunk_visuals(145);
    "#,
    );
    factory_code(&mut demo, "simulation::factory_step();", 40);
    assert_eq!(numbers(&demo, "counts")[16], 16.);
    assert_eq!(numbers(&demo, "counts")[18], 4.);
    factory_code(
        &mut demo,
        r#"
        world::enter_chunk(0,0);
        let cell=grid::index(6,2);
        if get_object_list("recipes")[cell]!=16.0 || get_scene_list("input_amounts")[cell]!=0.0 { throw "stale constructor archive"; }
        let cell=grid::index(6,4);
        if get_scene_list("assembler_iron")[cell]!=0.0 || get_scene_list("assembler_copper")[cell]!=0.0 { throw "stale assembler archive"; }
    "#,
        1,
    );
}

#[test]
fn transport_addresses_cross_negative_seams_but_never_wrap_planet_edges() {
    stellar_fixture(
        r#"
        for moon in [0.0,1.0] {
            data::session_set(7,moon); let radius=data::planet_radius();
            for dir in 0..4 {
                let cx=grid::step_x(dir)*radius; let cz=grid::step_z(dir)*radius;
                let cell=grid::index(grid::step_x(dir)*7,grid::step_z(dir)*7);
                let id=grid::chunk_id(cx,cz)*225+cell;
                if grid::global_neighbor(id,dir)!=-1 { throw "planet edge wrapped"; }
                let a=grid::chunk_id(-1,-1)*225+cell;
                let b=grid::global_neighbor(a,dir);
                if b<0 || grid::global_neighbor(b,(dir+2)%4)!=a { throw "negative seam not reversible"; }
                if abs(grid::global_x(a)-grid::global_x(b))+abs(grid::global_z(a)-grid::global_z(b))!=1.0 { throw "seam distance not one tile"; }
            }
        }
        data::session_set(7,0.0);
    "#,
    );
}

fn board_other_planet(demo: &mut SceneDemo) {
    factory_code(
        demo,
        r#"
        world::enter_chunk(0,0);
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0);
        world::travel_to_other_planet();
    "#,
        1,
    );
}

fn build_lunar_test_line(demo: &mut SceneDemo) {
    factory_code(
        demo,
        r#"
        if !data::on_moon() { throw "lunar fixture on wrong planet"; }
        for row in [[6,1],[7,2],[8,2],[9,4]] {
            let cx=if row[0]>7 { 1 } else { 0 }; world::enter_chunk(cx,0);
            let x=row[0]-cx*15; let cell=grid::index(x,0);
            let nodes=get_scene_list("nodes"); nodes[cell]=if row[1]==1 { 24.0 } else { 0.0 };
            set_scene_list("nodes",nodes); grid::cache_put("chunk_nodes",grid::current_chunk(),grid::pack_numbers(nodes));
            set_scene_variable("cursor_x",x.to_float()); set_scene_variable("cursor_z",0.0);
            set_scene_variable("selected",row[1].to_float()); set_scene_variable("direction",0.0); building::place_selected();
            if get_scene_list("builds")[cell]!=row[1].to_float() { throw "lunar machine not placed"; }
        }
        world::enter_chunk(0,0);
        let nodes=get_scene_list("nodes"); nodes[157]=0.0; set_scene_list("nodes",nodes);
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",3.0); set_scene_variable("selected",9.0); building::place_selected();
        if power::connect_power(power::power_id(112),power::power_id(157))!="" ||
            power::connect_power(power::power_id(157),power::power_id(118))!="" { throw "lunar power failed"; }
        power::update_power();
    "#,
        1,
    );
}

#[test]
fn planets_produce_during_flight_and_on_both_sides_of_repeated_round_trips() {
    let mut demo = multi_region_factory("[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]", "");
    factory_code(&mut demo, "simulation::factory_step();", 40);
    let earth_before = numbers(&demo, "counts")[1];
    factory_code(
        &mut demo,
        r#"
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0); flight::start();
    "#,
        1,
    );
    restore_factory_update(&mut demo);
    settle(&mut demo, 160);
    assert_eq!(controller_numbers(&demo, "session")[7], 1.);
    assert_eq!(
        controller_numbers(&demo, "session")[44],
        2.,
        "still descending"
    );
    let earth_during_landing = controller_numbers(&demo, "session")[9];
    assert!(earth_during_landing > earth_before);
    settle(&mut demo, 140);
    assert_eq!(controller_numbers(&demo, "session")[44], 0.);
    assert!(controller_numbers(&demo, "session")[9] > earth_during_landing);
    assert!(numbers(&demo, "counts").iter().all(|v| *v == 0.));
    build_lunar_test_line(&mut demo);
    factory_code(&mut demo, "simulation::factory_step();", 12);
    let backpack = controller_numbers(&demo, "stock");
    for trip in 0..3 {
        let models = demo.instance().document().objects.len();
        let earth = controller_numbers(&demo, "session")[9];
        let moon = numbers(&demo, "counts")[24];
        let beats = number(&demo, "ticks");
        factory_code(&mut demo, "simulation::factory_step();", 40);
        assert_eq!(
            number(&demo, "ticks"),
            beats + 40.,
            "one shared clock, not one tick per planet"
        );
        assert_eq!(controller_numbers(&demo, "session")[9], earth + 20.);
        assert_eq!(numbers(&demo, "counts")[24], moon + 20.);
        assert_eq!(
            numbers(&demo, "counts")[1],
            0.,
            "Earth items must not appear on Moon"
        );
        assert_eq!(controller_numbers(&demo, "session")[114], 4.);
        assert_eq!(
            demo.instance().document().objects.len(),
            models,
            "background production must not spawn models (trip {trip})"
        );
        let earth = controller_numbers(&demo, "session")[9];
        let moon = numbers(&demo, "counts")[24];
        board_other_planet(&mut demo);
        assert_eq!(numbers(&demo, "counts")[1], earth);
        assert_eq!(controller_numbers(&demo, "session")[32], moon);
        factory_code(&mut demo, "simulation::factory_step();", 40);
        assert_eq!(numbers(&demo, "counts")[1], earth + 20.);
        assert_eq!(controller_numbers(&demo, "session")[32], moon + 20.);
        assert_eq!(
            numbers(&demo, "counts")[24],
            0.,
            "Moon items must not appear on Earth"
        );
        factory_code(
            &mut demo,
            r#"
            world::enter_chunk(1,0);
            let slots=inventory::storage_read(106); let total=0.0;
            for slot in 0..16 { if slots[slot*2]==1.0 { total+=slots[slot*2+1]; } }
            if total!=get_scene_list("counts")[1] { throw "stale Earth storage after return"; }
        "#,
            1,
        );
        board_other_planet(&mut demo);
        factory_code(
            &mut demo,
            r#"
            world::enter_chunk(1,0);
            let slots=inventory::storage_read(106); let total=0.0;
            for slot in 0..16 { if slots[slot*2]==24.0 { total+=slots[slot*2+1]; } }
            if total!=get_scene_list("counts")[24] { throw "stale Moon storage after return"; }
            world::enter_chunk(0,0);
        "#,
            1,
        );
        // Refill only visible item models after arrival before checking the pool.
        factory_code(&mut demo, "simulation::factory_step();", 12);
        assert_eq!(controller_numbers(&demo, "stock"), backpack);
    }
    factory_code(
        &mut demo,
        "world::begin_world(4); simulation::factory_step();",
        1,
    );
    assert_eq!(controller_numbers(&demo, "session")[114], 0.);
    assert!(
        controller_numbers(&demo, "session")[8..40]
            .iter()
            .all(|v| *v == 0.)
    );
}

#[test]
fn threaded_frames_keep_both_planets_producing_after_travel() {
    let mut demo = multi_region_factory("[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]", "");
    board_other_planet(&mut demo);
    build_lunar_test_line(&mut demo);
    // Fill both transport lines before measuring their steady production rate.
    factory_code(&mut demo, "simulation::factory_step();", 40);
    demo.set_threaded_simulation(true).unwrap();

    for planet in [1., 0.] {
        if planet == 0. {
            board_other_planet(&mut demo);
        }
        restore_factory_update(&mut demo);
        demo.set_gameplay_input(GameplayInput::default());
        let local_kind = if planet == 1. { 24 } else { 1 };
        let remote_kind = if planet == 1. { 1 } else { 24 };
        let local_before = numbers(&demo, "counts")[local_kind];
        let remote_before = controller_numbers(&demo, "session")[8 + remote_kind];
        let beats_before = number(&demo, "ticks") as usize;
        let step = demo.app.timestep();
        // Exercise the same worker/frame entry point as the native player,
        // with the normal game update rather than a direct factory_step hook.
        for _ in 0..192 {
            demo.advance_with_frame(step, || ()).unwrap();
        }
        let beats_after = number(&demo, "ticks") as usize;
        assert!(beats_after - beats_before >= 9);
        let produced = (beats_after / 2 - beats_before / 2) as f32;
        assert_eq!(
            numbers(&demo, "counts")[local_kind],
            local_before + produced
        );
        assert_eq!(
            controller_numbers(&demo, "session")[8 + remote_kind],
            remote_before + produced,
            "background factory must produce at the same rate on planet {planet}"
        );
        assert_eq!(controller_numbers(&demo, "session")[7], planet);
        assert_eq!(controller_numbers(&demo, "session")[114], 4.);
        assert!(
            demo.app
                .world
                .resource::<bozzard_diagnostics::SimulationMetrics>()
                .unwrap()
                .threaded
        );
    }
}

#[test]
fn planets_keep_power_and_full_storage_independent_while_away() {
    let mut demo = multi_region_factory(
        "[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]",
        r#"
        power::disconnect_power(power::power_id(118),-1);
        // Deliberately leave power_dirty set: launch must preserve the updated circuit.
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0); world::travel_to_other_planet();
    "#,
    );
    build_lunar_test_line(&mut demo);
    factory_code(&mut demo, "simulation::factory_step();", 40);
    assert_eq!(controller_numbers(&demo, "session")[9], 0.);
    assert!(numbers(&demo, "counts")[24] > 0.);
    board_other_planet(&mut demo);
    assert_eq!(
        numbers(&demo, "item_amounts")[118],
        0.,
        "unpowered Earth miner must not borrow Moon power"
    );
    factory_code(
        &mut demo,
        r#"
        power::connect_power(power::power_id(157),power::power_id(118));
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0); world::travel_to_other_planet();
    "#,
        1,
    );
    factory_code(&mut demo, "simulation::factory_step();", 40);
    assert!(controller_numbers(&demo, "session")[9] > 0.);
    board_other_planet(&mut demo);
    factory_code(
        &mut demo,
        r#"
        let items=get_scene_list("items"); items[118]=1.0; items[119]=1.0; set_scene_list("items",items);
        let amounts=get_scene_list("item_amounts"); amounts[118]=100.0; amounts[119]=1.0; set_scene_list("item_amounts",amounts);
        world::enter_chunk(1,0);
        let items=get_scene_list("items"); items[105]=1.0; set_scene_list("items",items);
        let amounts=get_scene_list("item_amounts"); amounts[105]=1.0; set_scene_list("item_amounts",amounts);
        let slots=[]; for i in 0..16 { slots.push(1.0); slots.push(100.0); } inventory::storage_write(106,slots);
        let counts=get_scene_list("counts"); counts[1]=1600.0; set_scene_list("counts",counts);
    "#,
        1,
    );
    board_other_planet(&mut demo);
    factory_code(&mut demo, "simulation::factory_step();", 40);
    assert_eq!(controller_numbers(&demo, "session")[9], 1600.);
    board_other_planet(&mut demo);
    assert_eq!(numbers(&demo, "item_amounts")[118], 100.);
    assert_eq!(numbers(&demo, "item_amounts")[119], 1.);
    factory_code(
        &mut demo,
        r#"
        world::enter_chunk(1,0);
        if get_scene_list("item_amounts")[105]!=1.0 { throw "full storage lost its waiting belt item"; }
        let slots=inventory::storage_read(106); slots[1]=99.0; inventory::storage_write(106,slots);
        let counts=get_scene_list("counts"); counts[1]=1599.0; set_scene_list("counts",counts);
    "#,
        1,
    );
    board_other_planet(&mut demo);
    factory_code(&mut demo, "simulation::factory_step();", 6);
    assert_eq!(controller_numbers(&demo, "session")[9], 1600.);
}

#[test]
fn planets_use_their_own_boundaries_and_processing_recipes_while_away() {
    let mut demo = multi_region_factory(
        "[[96,0,1,0,1],[97,0,2,0,0],[98,0,3,0,0],[99,0,11,0,0],[100,0,4,0,0]]",
        r#"
        world::enter_chunk(7,0); machines::select_recipe(106,16.0); world::enter_chunk(0,0);
        set_scene_variable("cursor_x",0.0); set_scene_variable("cursor_z",1.0); world::travel_to_other_planet();
    "#,
    );
    let models = demo.instance().document().objects.len();
    factory_code(&mut demo, "simulation::factory_step();", 100);
    assert!(
        controller_numbers(&demo, "session")[24] > 30.,
        "Earth's region 6→7 seam must still work from the smaller Moon"
    );
    assert!(numbers(&demo, "counts").iter().all(|v| *v == 0.));
    assert!(demo.instance().document().objects.len() <= models);
    board_other_planet(&mut demo);
    factory_code(
        &mut demo,
        r#"
        world::enter_chunk(7,0);
        if get_object_list("recipes")[106]!=16.0 || inventory::storage_read(107)[0]!=16.0 { throw "remote processing recipe changed"; }
        let slots=inventory::storage_read(107); let total=0.0;
        for i in 0..16 { total+=slots[i*2+1]; }
        if total!=get_scene_list("counts")[16] { throw "return overwrote background storage"; }
    "#,
        1,
    );
}

#[test]
#[ignore = "manual release-mode profile of active and unloaded factory regions"]
fn profile_world_factories() {
    use std::{collections::BTreeMap, time::Instant};
    for regions in [1, 8] {
        let mut demo = stellar_fixture("set_object_variable(\"creative\",true);");
        factory_code(
            &mut demo,
            r#"
            let cx=get_scene_variable("ticks").to_int()+1; world::enter_chunk(cx,0);
            for row in [[112,1,1],[113,2,0],[114,4,0],[142,9,0],[157,6,4]] {
                let cell=row[0]; let nodes=get_scene_list("nodes"); nodes[cell]=row[2].to_float(); set_scene_list("nodes",nodes);
                set_scene_variable("cursor_x",grid::cell_x(cell).to_float()); set_scene_variable("cursor_z",grid::cell_z(cell).to_float());
                set_scene_variable("selected",row[1].to_float()); set_scene_variable("direction",0.0); building::place_selected();
                if get_scene_list("builds")[cell]!=row[1].to_float() { throw "profile machine not placed"; }
            }
            grid::cache_put("chunk_nodes",grid::current_chunk(),grid::pack_numbers(get_scene_list("nodes")));
            power::connect_power(power::power_id(157),power::power_id(142));
            power::connect_power(power::power_id(142),power::power_id(112));
            set_scene_variable("ticks",cx.to_float());
        "#,
            regions,
        );
        factory_code(&mut demo, "world::enter_chunk(0,0);", 1);
        restore_factory_update(&mut demo);
        settle(&mut demo, 100);
        let mut samples: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        for _ in 0..240 {
            let beat = number(&demo, "ticks");
            let start = Instant::now();
            tick(&mut demo, None);
            samples
                .entry(if number(&demo, "ticks") != beat {
                    "production"
                } else {
                    "ordinary"
                })
                .or_default()
                .push(start.elapsed().as_secs_f64() * 1000.);
        }
        assert_eq!(controller_numbers(&demo, "session")[114], regions as f32);
        assert!(numbers(&demo, "counts")[1] >= regions as f32);
        for (kind, mut values) in samples {
            values.sort_by(f64::total_cmp);
            println!(
                "{regions} regions / {kind}: median={:.3}ms p95={:.3}ms max={:.3}ms",
                values[values.len() / 2],
                values[values.len() * 95 / 100],
                values.last().unwrap()
            );
        }
    }
}

#[test]
#[ignore = "manual release-mode profile of production on one and two planets"]
fn profile_planet_factories() {
    use std::{collections::BTreeMap, time::Instant};
    let mut demo = multi_region_factory("[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]", "");
    for planets in [1, 2] {
        if planets == 2 {
            board_other_planet(&mut demo);
            build_lunar_test_line(&mut demo);
        }
        restore_factory_update(&mut demo);
        settle(&mut demo, 100);
        let mut samples: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        for _ in 0..240 {
            let beat = number(&demo, "ticks");
            let start = Instant::now();
            tick(&mut demo, None);
            samples
                .entry(if number(&demo, "ticks") != beat {
                    "production"
                } else {
                    "ordinary"
                })
                .or_default()
                .push(start.elapsed().as_secs_f64() * 1000.);
        }
        assert_eq!(
            controller_numbers(&demo, "session")[114],
            (planets * 2) as f32
        );
        for (kind, mut values) in samples {
            values.sort_by(f64::total_cmp);
            println!(
                "{planets} planets / {kind}: median={:.3}ms p95={:.3}ms max={:.3}ms",
                values[values.len() / 2],
                values[values.len() * 95 / 100],
                values.last().unwrap()
            );
        }
    }
}

fn save_directory(demo: &mut SceneDemo) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "stellar-saves-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    demo.app
        .world
        .resource_mut::<bozzard_demo::factory::Session>()
        .unwrap()
        .directory = path.clone();
    path
}
fn finish_save_io(demo: &mut SceneDemo) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        tick(demo, None);
        let state = controller_numbers(demo, "session");
        if state[125] == 0. && state[116] == 0. {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "save operation timed out: {:?}",
            &state[116..]
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn saves_restore_both_planets_power_storage_inventory_and_clock_in_a_fresh_session() {
    use bozzard_demo::factory::{Session, saves};
    let mut demo = multi_region_factory("[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]", "");
    let directory = save_directory(&mut demo);
    factory_code(&mut demo, "simulation::factory_step();", 40);
    board_other_planet(&mut demo);
    build_lunar_test_line(&mut demo);
    factory_code(&mut demo, "simulation::factory_step();", 40);
    factory_code(
        &mut demo,
        r#"
        backpack::give(1.0,131.0);backpack::move_stack(0,24);
        data::session_set(120,101.0);
        if !persistence::prepare_save(1) {throw "save was not requested";}
    "#,
        1,
    );
    factory_code(&mut demo, "persistence::update(0.0);", 1);
    finish_save_io(&mut demo);
    let saved = saves::read(&directory, 1).unwrap();
    assert!(
        saved
            .description()
            .unwrap()
            .contains("Tier 2 / Phase 4 · Day 1 / Night 08:06:44")
    );
    let serialized = serde_json::to_string(&saved).unwrap();
    assert!(
        !serialized.contains("prefab:")
            && !serialized.contains("script_manager")
            && !serialized.contains("cache_visuals")
    );
    let earth = controller_numbers(&demo, "session")[9];
    let moon = numbers(&demo, "counts")[24];
    let stock = controller_numbers(&demo, "stock");
    let inventory = controller_numbers(&demo, "session")[64..114].to_vec();
    drop(demo);
    // New process-equivalent runtime, with no retained models or scripts from the first world.
    let mut loaded = factory_with_mode(Some(17.), false);
    tick(&mut loaded, None);
    loaded
        .app
        .world
        .resource_mut::<Session>()
        .unwrap()
        .directory = directory.clone();
    factory_code(
        &mut loaded,
        "data::session_set(117,1.0);data::session_set(116,2.0);",
        1,
    );
    factory_code(&mut loaded, "persistence::update(0.0);", 1);
    finish_save_io(&mut loaded);
    assert_eq!(controller_numbers(&loaded, "session")[7], 1.);
    assert_eq!(controller_numbers(&loaded, "session")[120], 101.);
    assert_eq!(controller_numbers(&loaded, "session")[9], earth);
    assert_eq!(numbers(&loaded, "counts")[24], moon);
    assert_eq!(controller_numbers(&loaded, "stock"), stock);
    assert_eq!(controller_numbers(&loaded, "session")[64..114], inventory);
    assert_eq!(controller_number(&loaded, "phase"), 7.);
    factory_code(&mut loaded, "simulation::factory_step();", 40);
    assert_eq!(numbers(&loaded, "counts")[24], moon + 20.);
    assert_eq!(controller_numbers(&loaded, "session")[9], earth + 20.);
    board_other_planet(&mut loaded);
    assert_eq!(numbers(&loaded, "counts")[1], earth + 20.);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn loading_from_a_fresh_title_initializes_machine_rotation_animation() {
    use bozzard_demo::factory::Session;
    // Match the reported cell, near the end of the 225-cell animation tables.
    let mut original = buffer_layout(
        "[[203,11,0,0,0]]",
        r#"set_scene_variable("cursor_x",1.0);set_scene_variable("cursor_z",6.0);"#,
    );
    let directory = save_directory(&mut original);
    factory_code(&mut original, "persistence::prepare_save(1);", 1);
    factory_code(&mut original, "persistence::update(0.0);", 1);
    finish_save_io(&mut original);
    drop(original);

    // Load directly from the authored title, without creating a new world first.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../earth-factory/scenes/earth.json");
    let scene = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut loaded = SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
    tick(&mut loaded, None);
    assert!(controller_numbers(&loaded, "rotation_from").is_empty());
    loaded
        .app
        .world
        .resource_mut::<Session>()
        .unwrap()
        .directory = directory.clone();
    click_widget(&mut loaded, "title-load");
    finish_save_io(&mut loaded);
    click_widget(&mut loaded, "save-slot-1");
    finish_save_io(&mut loaded);
    assert_eq!(numbers(&loaded, "builds")[203], 11.);
    assert_eq!(number(&loaded, "cursor_x"), 1.);
    assert_eq!(number(&loaded, "cursor_z"), 6.);

    press(&mut loaded, "R");
    assert_eq!(controller_numbers(&loaded, "rotation_cells"), vec![203.]);
    for name in ["rotation_time", "rotation_from", "rotation_turns"] {
        assert_eq!(controller_numbers(&loaded, name).len(), 225, "{name}");
    }
    settle(&mut loaded, 45);
    assert_eq!(numbers(&loaded, "facings")[203], 1.);
    assert!(controller_numbers(&loaded, "rotation_cells").is_empty());
    assert_eq!(controller_numbers(&loaded, "rotation_turns")[203], 0.);
    assert_eq!(controller_numbers(&loaded, "rotation_time")[203], 1.);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn saves_autosave_after_twenty_minutes_defers_flight_and_rejects_guest_requests() {
    use bozzard_demo::factory::{Authority, Session, saves};
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    let directory = save_directory(&mut demo);
    factory_code(
        &mut demo,
        "data::session_set(121,1199.0);persistence::update(0.5);",
        1,
    );
    assert!(!directory.exists());
    factory_code(
        &mut demo,
        "data::session_set(44,1.0);persistence::update(1.0);",
        1,
    );
    assert!(!directory.exists(), "wait until the ship lands");
    factory_code(
        &mut demo,
        "data::session_set(44,0.0);persistence::update(0.0);",
        1,
    );
    factory_code(&mut demo, "persistence::update(0.0);", 1);
    finish_save_io(&mut demo);
    assert!(saves::read(&directory, 0).is_ok());
    assert_eq!(controller_numbers(&demo, "session")[121], 0.);
    demo.app.world.resource_mut::<Session>().unwrap().authority = Authority::Guest;
    factory_code(
        &mut demo,
        "data::session_set(117,1.0);data::session_set(116,1.0);",
        1,
    );
    assert!(!directory.join("slot-1.json").exists());
    let seed = number(&demo, "seed");
    factory_code(
        &mut demo,
        "data::session_set(117,0.0);data::session_set(116,2.0);",
        1,
    );
    assert_eq!(number(&demo, "seed"), seed);
    assert_eq!(controller_numbers(&demo, "session")[116], 0.);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn saves_invalid_files_never_replace_the_running_world_and_browser_loads_from_title() {
    use bozzard_demo::factory::saves;
    let hud = |demo: &SceneDemo| {
        demo.instance()
            .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
            .unwrap()
    };
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    let directory = save_directory(&mut demo);
    press(&mut demo, "Escape");
    click_widget(&mut demo, "menu-save");
    finish_save_io(&mut demo);
    assert!(shown(&demo, "saves-panel"));
    assert!(!hud(&demo).element("save-slot-0").unwrap().enabled);
    click_widget(&mut demo, "save-slot-1");
    finish_save_io(&mut demo);
    assert!(saves::read(&directory, 1).is_ok());
    assert!(
        hud(&demo)
            .element("save-info-1")
            .unwrap()
            .text
            .contains("Tier 1 / Phase 1")
    );
    click_widget(&mut demo, "saves-close");
    click_widget(&mut demo, "menu-main-menu");
    click_widget(&mut demo, "title-load");
    finish_save_io(&mut demo);
    assert!(hud(&demo).element("save-slot-1").unwrap().enabled);
    assert!(!hud(&demo).element("save-slot-2").unwrap().enabled);
    click_widget(&mut demo, "save-slot-1");
    finish_save_io(&mut demo);
    assert!(!shown(&demo, "saves-panel") && !shown(&demo, "title-content"));
    assert_eq!(number(&demo, "seed"), 4.);
    press(&mut demo, "J");
    settle(&mut demo, 16);
    click_widget(&mut demo, "journal-tab-2");
    assert_eq!(
        controller_number(&demo, "journal_page"),
        2.,
        "loaded interfaces remain interactive"
    );
    press(&mut demo, "J");
    settle(&mut demo, 16);
    let before = numbers(&demo, "builds");
    std::fs::write(directory.join("slot-2.json"), b"{broken").unwrap();
    factory_code(
        &mut demo,
        "data::session_set(117,2.0);data::session_set(116,2.0);",
        1,
    );
    factory_code(&mut demo, "persistence::update(0.0);", 1);
    finish_save_io(&mut demo);
    assert_eq!(numbers(&demo, "builds"), before);
    assert_eq!(number(&demo, "seed"), 4.);
    assert!(!saves::catalog(&directory)[2].loadable);
    // A well-formed JSON file with invalid RLE is also rejected before touching the world.
    let mut corrupt = serde_json::to_value(saves::read(&directory, 1).unwrap()).unwrap();
    corrupt["state"]["controller"]["cache_builds"]["list"]["values"][144]["text"] =
        serde_json::json!("1:999999999");
    std::fs::write(
        directory.join("slot-2.json"),
        serde_json::to_vec(&corrupt).unwrap(),
    )
    .unwrap();
    assert!(saves::read(&directory, 2).is_err());
    assert_eq!(numbers(&demo, "builds"), before);
    std::fs::remove_dir_all(directory).unwrap();
}

fn coop_world(
    demo: &SceneDemo,
) -> (
    bozzard_demo::factory::shared::World,
    bozzard_demo::factory::shared::Player,
) {
    use bozzard_demo::factory::{shared::World, state::State};
    World::from_local(
        State::capture_live(demo.app.world.resource::<BlueprintRuntime>().unwrap()).unwrap(),
    )
    .unwrap()
}

#[test]
fn coop_live_snapshot_separates_planets_and_private_player_state_without_archiving() {
    use bozzard_demo::factory::{
        shared::{Player, Position, World},
        state::State,
    };
    let mut demo = multi_region_factory("[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]", "");
    board_other_planet(&mut demo);
    build_lunar_test_line(&mut demo);
    factory_code(&mut demo, "simulation::factory_step();", 40);
    factory_code(
        &mut demo,
        r#"
        backpack::give(1.0,37.0);
        let nodes=get_scene_list("nodes"); nodes[111]=0.0; set_scene_list("nodes",nodes);
        set_scene_variable("cursor_x",-1.0); set_scene_variable("cursor_z",0.0);
        set_scene_variable("selected",2.0); building::place_selected();
        "#,
        1,
    );
    let runtime = demo.app.world.resource::<BlueprintRuntime>().unwrap();
    let before_scene = runtime.scene_blackboard().clone();
    let before_controller = runtime.object_blackboard("controller").unwrap().clone();
    let archived = State::capture(runtime).unwrap();
    let live = State::capture_live(runtime).unwrap();
    assert_ne!(
        archived.controller["cache_builds"], live.controller["cache_builds"],
        "snapshot must include this tick's unarchived build"
    );
    let (world, host) = World::from_local(live.clone()).unwrap();
    assert_eq!(world.project(&host).unwrap(), live);
    assert_eq!(host.position.planet, 1);
    assert_eq!(
        host.backpack
            .iter()
            .map(|s| s.amount as usize)
            .sum::<usize>(),
        37
    );
    assert_eq!(Player::capture(world.state()).unwrap(), Player::default());
    let guest = Player::default();
    let view = world.project(&guest).unwrap();
    assert_eq!(
        view.scene["counts"].values()[1],
        live.controller["session"].values()[9]
    );
    assert_eq!(
        view.controller["power_data"],
        live.controller["power_other"]
    );
    assert_eq!(
        view.controller["power_other"],
        live.controller["power_data"]
    );
    assert_eq!(Player::capture(&view).unwrap(), guest);
    let mut unknown = guest;
    unknown.position = Position {
        planet: 1,
        x: 90,
        z: 90,
    };
    assert!(world.project(&unknown).is_err());
    assert_eq!(runtime.scene_blackboard(), &before_scene);
    assert_eq!(
        runtime.object_blackboard("controller").unwrap(),
        &before_controller
    );
}

#[test]
fn coop_four_peers_receive_private_snapshots_and_atomic_incremental_updates() {
    use bozzard_demo::factory::{
        replication::{
            Host, Update,
            stream::{MAX_PACKET, Receiver, Sender},
        },
        shared::{Player, Stack},
    };
    use std::{collections::BTreeMap, time::Duration};
    let mut demo = multi_region_factory("[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]", "");
    factory_code(
        &mut demo,
        "backpack::give(1.0,37.0);simulation::factory_step();",
        1,
    );
    let (world, player) = coop_world(&demo);
    let mut returning = Player::default();
    returning.backpack[4] = Stack { kind: 2, amount: 9 };
    let mut host = Host::new(
        1,
        10,
        world,
        player.clone(),
        [(20, returning.clone())].into(),
    )
    .unwrap();
    let names: BTreeMap<_, _> = [
        (10, "Host".into()),
        (20, "Red".into()),
        (30, "Orange".into()),
        (40, "Green".into()),
    ]
    .into();
    host.members(&names).unwrap();
    let mut clients = BTreeMap::new();
    for (peer, expected_slot) in [(20, 1), (30, 2), (40, 3)] {
        let expected = host.snapshot(peer).unwrap();
        assert_eq!(
            expected
                .members
                .iter()
                .find(|m| m.peer == peer)
                .unwrap()
                .slot,
            expected_slot
        );
        assert_eq!(
            expected.player,
            if peer == 20 {
                returning.clone()
            } else {
                Player::default()
            }
        );
        let transfer = Sender::new(&Update::Full(expected.clone())).unwrap();
        assert!(transfer.packets() > 1);
        let mut client = Receiver::new(10, peer).unwrap();
        assert!(
            client
                .receive(99, &transfer.packet(0).unwrap(), Duration::ZERO)
                .is_err()
        );
        assert_eq!(client.buffered_bytes(), 0);
        // Late join snapshots remain invisible until every fragment arrives;
        // reliable retransmissions and out-of-order delivery cannot double apply.
        for i in (0..transfer.packets()).rev() {
            let packet = transfer.packet(i).unwrap();
            assert!(packet.len() <= MAX_PACKET);
            assert_eq!(client.receive(10, &packet, Duration::ZERO).unwrap(), i == 0);
            assert!(!client.receive(10, &packet, Duration::ZERO).unwrap());
            if i > 0 {
                assert!(client.current().is_none());
            }
        }
        assert_eq!(client.current(), Some(&expected));
        clients.insert(peer, client);
    }
    let mut fifth = names.clone();
    fifth.insert(50, "Fifth".into());
    assert!(host.members(&fifth).is_err());
    let prior = host.snapshot(20).unwrap();
    factory_code(&mut demo, "simulation::factory_step();", 40);
    let (world, player) = coop_world(&demo);
    let mut players = host.players().clone();
    players.insert(10, player);
    host.publish(world, players).unwrap();
    for (peer, client) in &mut clients {
        let next = host.snapshot(*peer).unwrap();
        let delta = Update::between(client.current().unwrap(), &next).unwrap();
        assert!(
            serde_json::to_vec(&delta).unwrap().len()
                < serde_json::to_vec(&Update::Full(next.clone()))
                    .unwrap()
                    .len()
                    / 4
        );
        let transfer = Sender::new(&delta).unwrap();
        for i in 0..transfer.packets() {
            client
                .receive(10, &transfer.packet(i).unwrap(), Duration::from_secs(1))
                .unwrap();
        }
        assert_eq!(client.current(), Some(&next));
    }
    let next = host.snapshot(20).unwrap();
    // Corrupt a delta without changing its valid base: no partial mutation, no
    // schema extension, no script/render injection and no out-of-range archive.
    for change in [
        serde_json::json!({"controller": true, "field": "script_manager", "index": null, "value": {"text": "bad"}}),
        serde_json::json!({"controller": true, "field": "chunk_nodes", "index": 99999, "value": {"text": "1"}}),
        serde_json::json!({"controller": true, "field": "phase", "index": null, "value": {"number": 900}}),
        serde_json::json!({"controller": true, "field": "session", "index": 116, "value": {"number": 2}}),
    ] {
        let mut invalid = serde_json::to_value(Update::between(&prior, &next).unwrap()).unwrap();
        invalid["Delta"]["changes"] = serde_json::json!([change]);
        let invalid: Update = serde_json::from_value(invalid).unwrap();
        assert!(invalid.apply(Some(&prior), 10, 20).is_err());
    }
    assert!(
        Update::between(&prior, &next)
            .unwrap()
            .apply(Some(&next), 10, 20)
            .is_err()
    );
    let mut missing = prior.clone();
    missing.revision -= 1;
    assert!(
        Update::between(&prior, &next)
            .unwrap()
            .apply(Some(&missing), 10, 20)
            .is_err()
    );
    // Guest departure does not recolor connected players or discard inventory.
    let mut fewer = names.clone();
    fewer.remove(&20);
    host.members(&fewer).unwrap();
    assert!(host.snapshot(20).is_err());
    assert_eq!(
        host.snapshot(30)
            .unwrap()
            .members
            .iter()
            .find(|m| m.peer == 30)
            .unwrap()
            .slot,
        2
    );
    host.members(&names).unwrap();
    assert_eq!(host.snapshot(20).unwrap().player, returning);
    // A host load/new world changes the epoch. Old buffered packets cannot undo it.
    let mut reset = host.snapshot(20).unwrap();
    reset.epoch = 2;
    reset.revision = 1;
    let transfer = Sender::new(&Update::Full(reset.clone())).unwrap();
    let client = clients.get_mut(&20).unwrap();
    for i in 0..transfer.packets() {
        client
            .receive(10, &transfer.packet(i).unwrap(), Duration::from_secs(2))
            .unwrap();
    }
    let old = Sender::new(&Update::Full(prior)).unwrap();
    assert!(
        !client
            .receive(10, &old.packet(0).unwrap(), Duration::from_secs(3))
            .unwrap()
    );
    assert_eq!(client.current(), Some(&reset));
}

#[test]
fn coop_world_transfer_bounds_timeouts_and_bad_fragments_preserve_the_replica() {
    use bozzard_demo::factory::replication::{
        Host, Update,
        stream::{MAX_BYTES, Receiver, Sender},
    };
    use std::time::Duration;
    let demo = stellar_fixture("");
    let (world, player) = coop_world(&demo);
    let mut host = Host::new(1, 10, world, player, Default::default()).unwrap();
    host.members(&[(10, "Host".into()), (20, "Guest".into())].into())
        .unwrap();
    let snapshot = host.snapshot(20).unwrap();
    let sender = Sender::new(&Update::Full(snapshot.clone())).unwrap();
    let mut receiver = Receiver::new(10, 20).unwrap();
    let packet = sender.packet(0).unwrap();
    let mut oversized = packet.clone();
    oversized[20..24].copy_from_slice(&((MAX_BYTES + 1) as u32).to_le_bytes());
    assert!(receiver.receive(10, &oversized, Duration::ZERO).is_err());
    assert_eq!(receiver.buffered_bytes(), 0);
    receiver.receive(10, &packet, Duration::ZERO).unwrap();
    let buffered = receiver.buffered_bytes();
    let mut conflict = packet.clone();
    *conflict.last_mut().unwrap() ^= 1;
    assert!(receiver.receive(10, &conflict, Duration::ZERO).is_err());
    assert_eq!(receiver.buffered_bytes(), buffered);
    // Duplicate traffic must not keep an abandoned large allocation alive.
    assert!(
        !receiver
            .receive(10, &packet, Duration::from_secs(14))
            .unwrap()
    );
    assert!(receiver.expire(Duration::from_secs(15)));
    assert_eq!(receiver.buffered_bytes(), 0);
    assert!(receiver.current().is_none());
    for i in 0..sender.packets() {
        receiver
            .receive(10, &sender.packet(i).unwrap(), Duration::from_secs(16))
            .unwrap();
    }
    assert_eq!(receiver.current(), Some(&snapshot));
    let mut invalid = snapshot.clone();
    invalid.revision += 1;
    invalid.members[0].slot = 3;
    let broken = Sender::new(&Update::Full(invalid)).unwrap();
    for i in 0..broken.packets() {
        let result = receiver.receive(10, &broken.packet(i).unwrap(), Duration::from_secs(17));
        if i + 1 == broken.packets() {
            assert!(result.is_err());
        } else {
            assert!(!result.unwrap());
        }
    }
    assert_eq!(receiver.current(), Some(&snapshot));
}

#[test]
fn saves_retain_independent_steam_players_and_read_existing_solo_files() {
    use bozzard_demo::factory::{
        Authority, Session, saves,
        shared::{Player, Position, Stack},
    };
    let mut demo = multi_region_factory("[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]", "");
    board_other_planet(&mut demo);
    let directory = save_directory(&mut demo);
    let mut guest = Player {
        position: Position {
            planet: 0,
            x: 2,
            z: 0,
        },
        ..Player::default()
    };
    guest.backpack[12] = Stack {
        kind: 11,
        amount: 99,
    };
    let session = demo.app.world.resource_mut::<Session>().unwrap();
    session.authority = Authority::Host;
    session.local_peer = Some(10);
    session.players.insert(20, guest.clone());
    factory_code(
        &mut demo,
        "backpack::give(1.0,37.0);persistence::prepare_save(1);",
        1,
    );
    factory_code(&mut demo, "persistence::update(0.0);", 1);
    finish_save_io(&mut demo);
    let saved = saves::read(&directory, 1).unwrap();
    assert_eq!(saved.owner, Some(10));
    assert_eq!(saved.players[&20], guest);
    assert_eq!(
        saved.players[&10].backpack[0],
        Stack {
            kind: 1,
            amount: 37
        }
    );
    assert_eq!(saved.players[&10].position.planet, 1);
    // Optional new fields must not break pre-co-op solo saves.
    let mut legacy = serde_json::to_value(&saved).unwrap();
    legacy.as_object_mut().unwrap().remove("owner");
    legacy.as_object_mut().unwrap().remove("players");
    serde_json::from_value::<saves::Save>(legacy)
        .unwrap()
        .validate()
        .unwrap();
    let mut loaded = factory_with_mode(Some(17.), false);
    tick(&mut loaded, None);
    let session = loaded.app.world.resource_mut::<Session>().unwrap();
    session.directory = directory.clone();
    session.authority = Authority::Host;
    // The file owner changes; do not clone the old host's backpack into another record.
    session.local_peer = Some(11);
    factory_code(
        &mut loaded,
        "data::session_set(117,1.0);data::session_set(116,2.0);",
        1,
    );
    factory_code(&mut loaded, "persistence::update(0.0);", 1);
    finish_save_io(&mut loaded);
    let session = loaded.app.world.resource::<Session>().unwrap();
    assert_eq!(session.players.len(), 2);
    assert!(!session.players.contains_key(&10));
    assert_eq!(session.players[&20], guest);
    assert_eq!(
        session.players[&11].backpack[0],
        Stack {
            kind: 1,
            amount: 37
        }
    );
    assert_eq!(controller_numbers(&loaded, "stock")[1], 37.);
    // Loading locally first and opening a lobby afterwards must perform the
    // same identity transfer as loading while already hosting.
    let mut solo = factory_with_mode(Some(19.), false);
    tick(&mut solo, None);
    solo.app.world.resource_mut::<Session>().unwrap().directory = directory.clone();
    factory_code(
        &mut solo,
        "data::session_set(117,1.0);data::session_set(116,2.0);",
        1,
    );
    factory_code(&mut solo, "persistence::update(0.0);", 1);
    finish_save_io(&mut solo);
    assert_eq!(
        solo.app.world.resource::<Session>().unwrap().local_peer,
        Some(10)
    );
    bozzard_demo::factory::host::HostRuntime::start(
        &mut solo.app.world,
        77,
        1,
        [(77, "New host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    let registry = &solo.app.world.resource::<Session>().unwrap().players;
    assert_eq!(registry.len(), 2);
    assert!(!registry.contains_key(&10));
    assert_eq!(registry[&77].backpack[0].amount, 37);
    assert_eq!(registry[&20], guest);
    let mut invalid = saved;
    invalid.players.get_mut(&20).unwrap().backpack[12].amount = 101;
    assert!(invalid.validate().is_err());
    factory_code(&mut loaded, "world::begin_world(4);", 1);
    assert!(
        loaded
            .app
            .world
            .resource::<Session>()
            .unwrap()
            .players
            .is_empty()
    );
    std::fs::remove_dir_all(directory).unwrap();
}

fn coop_page(
    world: &bozzard_demo::factory::shared::World,
    name: &str,
    planet: usize,
    chunk: usize,
    count: usize,
) -> Vec<f32> {
    let Value::Text(text) = &world.state().controller[name].values()[planet * 289 + chunk] else {
        panic!("invalid page");
    };
    if text.is_empty() {
        return vec![0.; count];
    }
    let mut result = Vec::new();
    for token in text.split(',') {
        let (value, run) = token.split_once(':').unwrap_or((token, "1"));
        result.extend(std::iter::repeat_n(
            value.parse::<f32>().unwrap(),
            run.parse::<usize>().unwrap(),
        ));
    }
    assert_eq!(result.len(), count);
    result
}

#[test]
fn coop_authority_builds_use_solo_costs_and_conflicts_never_duplicate_or_spend_twice() {
    use bozzard_demo::factory::{
        authority::Executor,
        replication::requests::Action,
        shared::{Player, Position, Stack},
    };
    use std::time::Duration;
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("phase",1.0); backpack::give(1.0,12.0);
        set_scene_variable("cursor_x",1.0);set_scene_variable("cursor_z",0.0);
    "#,
    );
    let models = demo.instance().document().objects.len();
    let (mut world, mut player) = coop_world(&demo);
    let mut executor = Executor::new(demo.instance()).unwrap();
    let action = Action::Place {
        kind: 3,
        direction: 0,
    };
    let outcome = executor
        .apply(&mut world, 10, &mut player, &action, Duration::ZERO)
        .unwrap();
    assert!(outcome.accepted, "{outcome:?}");
    assert_eq!(player.backpack[0], Stack { kind: 1, amount: 8 });
    assert_eq!(coop_page(&world, "cache_builds", 0, 144, 225)[113], 3.);
    assert_eq!(coop_page(&world, "cache_recipes", 0, 144, 225)[113], 11.);
    assert_eq!(
        demo.instance().document().objects.len(),
        models,
        "remote actions must not spawn models"
    );
    let mut second = Player {
        position: Position {
            planet: 0,
            x: 1,
            z: 0,
        },
        ..Default::default()
    };
    second.backpack[0] = Stack {
        kind: 1,
        amount: 12,
    };
    let before = world.clone();
    let inventory = second.backpack;
    assert!(
        !executor
            .apply(&mut world, 20, &mut second, &action, Duration::ZERO)
            .unwrap()
            .accepted
    );
    assert_eq!(world, before);
    assert_eq!(second.backpack, inventory);
    factory_code(
        &mut demo,
        r#"set_scene_variable("selected",3.0);building::place_selected();power::update_power();"#,
        1,
    );
    let (solo, solo_player) = coop_world(&demo);
    assert_eq!(player.backpack, solo_player.backpack);
    for field in [
        "cache_builds",
        "cache_facings",
        "cache_recipes",
        "cache_items",
        "cache_item_amounts",
    ] {
        assert_eq!(
            coop_page(&world, field, 0, 144, 225),
            coop_page(&solo, field, 0, 144, 225),
            "{field}"
        );
    }
    assert_eq!(
        world.state().controller["power_data"],
        solo.state().controller["power_data"]
    );
    assert!(
        !executor
            .apply(
                &mut world,
                20,
                &mut second,
                &Action::Place {
                    kind: 5,
                    direction: 0
                },
                Duration::ZERO
            )
            .unwrap()
            .accepted,
        "assembler is locked"
    );
    assert!(
        executor
            .apply(
                &mut world,
                20,
                &mut second,
                &Action::Collect {
                    at: Position {
                        planet: 0,
                        x: 90,
                        z: 90
                    }
                },
                Duration::ZERO
            )
            .is_err()
    );
    assert_eq!(world, before);
    assert!(
        executor
            .apply(&mut world, 10, &mut player, &Action::Remove, Duration::ZERO)
            .unwrap()
            .accepted
    );
    assert_eq!(coop_page(&world, "cache_builds", 0, 144, 225)[113], 0.);
}

#[test]
fn coop_authority_processing_matches_solo_capacity_refunds_and_collection() {
    use bozzard_demo::factory::{
        authority::Executor, replication::requests::Action, shared::Position,
    };
    use std::time::Duration;
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",4.0);test_build(114,11.0);
        machines::select_recipe(114,16.0);set_object_variable("creative",false);backpack::give(11.0,10.0);
        let items=get_scene_list("items");items[114]=16.0;set_scene_list("items",items);
        let amounts=get_scene_list("item_amounts");amounts[114]=94.0;set_scene_list("item_amounts",amounts);
    "#,
    );
    let (mut world, mut player) = coop_world(&demo);
    let at = Position {
        planet: 0,
        x: 2,
        z: 0,
    };
    let mut executor = Executor::new(demo.instance()).unwrap();
    for (action, solo_code) in [
        (Action::Feed { at }, "machines::feed_assembler(114);"),
        (Action::Collect { at }, "inventory::collect_machine(114);"),
        (
            Action::Configure { at, recipe: 15 },
            "machines::select_recipe(114,15.0);",
        ),
    ] {
        let result = executor
            .apply(&mut world, 10, &mut player, &action, Duration::ZERO)
            .unwrap();
        assert!(result.accepted, "{action:?}: {result:?}");
        factory_code(&mut demo, solo_code, 1);
        let (solo, solo_player) = coop_world(&demo);
        assert_eq!(player.backpack, solo_player.backpack, "{action:?}");
        for field in [
            "cache_input_items",
            "cache_input_amounts",
            "cache_items",
            "cache_item_amounts",
            "cache_recipes",
            "cache_progress",
        ] {
            assert_eq!(
                coop_page(&world, field, 0, 144, 225),
                coop_page(&solo, field, 0, 144, 225),
                "{field} / {action:?}"
            );
        }
    }
    assert_eq!(
        player
            .backpack
            .iter()
            .filter(|s| s.kind == 11)
            .map(|s| s.amount)
            .sum::<u16>(),
        10
    );
    assert_eq!(
        player
            .backpack
            .iter()
            .filter(|s| s.kind == 16)
            .map(|s| s.amount)
            .sum::<u16>(),
        94
    );
}

#[test]
fn coop_authority_discovers_from_the_actor_and_times_gathering_flights_and_rotations() {
    use bozzard_demo::factory::{
        authority::Executor,
        replication::requests::Action,
        shared::{Player, Position},
    };
    use std::{collections::BTreeMap, time::Duration};
    let demo = stellar_fixture(
        r#"set_object_variable("creative",true);set_object_variable("phase",7.0);test_build(113,3.0);"#,
    );
    let (mut world, _) = coop_world(&demo);
    let mut player = Player::default();
    let mut executor = Executor::new(demo.instance()).unwrap();
    let models = demo.instance().document().objects.len();
    assert!(
        executor
            .apply(
                &mut world,
                20,
                &mut player,
                &Action::Discover {
                    planet: 0,
                    x: 1,
                    z: 0
                },
                Duration::ZERO
            )
            .is_err()
    );
    for i in 0..8 {
        executor
            .apply(
                &mut world,
                20,
                &mut player,
                &Action::Move { x: 1, z: 0 },
                Duration::from_millis(i * 20),
            )
            .unwrap();
    }
    assert_eq!(
        player.position,
        Position {
            planet: 0,
            x: 8,
            z: 2
        }
    );
    assert!(world.discovered(player.position).unwrap());
    assert!(
        coop_page(&world, "chunk_nodes", 0, 145, 225)
            .iter()
            .any(|n| *n > 0.)
    );
    let frozen = world.clone();
    assert!(
        executor
            .apply(
                &mut world,
                20,
                &mut player,
                &Action::Discover {
                    planet: 1,
                    x: 0,
                    z: 0
                },
                Duration::from_secs(1)
            )
            .is_err()
    );
    assert_eq!(world, frozen);
    // Start on a real generated solid deposit; a held key is paced by host time.
    let nodes = coop_page(&world, "chunk_nodes", 0, 144, 225);
    let index = nodes.iter().position(|n| *n == 1.).unwrap();
    player.position = Position {
        planet: 0,
        x: (index % 15) as i16 - 7,
        z: (index / 15) as i16 - 7,
    };
    executor
        .apply(
            &mut world,
            20,
            &mut player,
            &Action::Gather { active: true },
            Duration::from_secs(1),
        )
        .unwrap();
    let mut players: BTreeMap<_, _> = [(20, player)].into();
    executor
        .advance(&mut world, &mut players, Duration::from_secs(1))
        .unwrap();
    executor
        .advance(&mut world, &mut players, Duration::from_millis(1299))
        .unwrap();
    assert_eq!(players[&20].backpack[0].amount, 1);
    executor
        .advance(&mut world, &mut players, Duration::from_millis(1300))
        .unwrap();
    assert_eq!(players[&20].backpack[0].amount, 2);
    players.get_mut(&20).unwrap().position = Position {
        planet: 0,
        x: 1,
        z: 0,
    };
    executor
        .apply(
            &mut world,
            20,
            players.get_mut(&20).unwrap(),
            &Action::Gather { active: false },
            Duration::from_secs(2),
        )
        .unwrap();
    executor
        .apply(
            &mut world,
            20,
            players.get_mut(&20).unwrap(),
            &Action::Rotate,
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(
        executor.rotation_views(Duration::from_millis(2300))[0].progress,
        0.5
    );
    executor
        .advance(&mut world, &mut players, Duration::from_millis(2599))
        .unwrap();
    assert_eq!(coop_page(&world, "cache_facings", 0, 144, 225)[113], 0.);
    executor
        .advance(&mut world, &mut players, Duration::from_millis(2600))
        .unwrap();
    assert_eq!(coop_page(&world, "cache_facings", 0, 144, 225)[113], 1.);
    executor
        .apply(
            &mut world,
            20,
            players.get_mut(&20).unwrap(),
            &Action::Travel,
            Duration::from_secs(3),
        )
        .unwrap();
    assert!(
        executor
            .apply(
                &mut world,
                20,
                players.get_mut(&20).unwrap(),
                &Action::Move { x: 1, z: 0 },
                Duration::from_secs(4)
            )
            .is_err()
    );
    executor
        .advance(&mut world, &mut players, Duration::from_millis(4999))
        .unwrap();
    assert_eq!(players[&20].position.planet, 0);
    executor
        .advance(&mut world, &mut players, Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        players[&20].position,
        Position {
            planet: 1,
            x: 0,
            z: 2
        }
    );
    assert!(!executor.flight_views(Duration::from_secs(6)).is_empty());
    executor
        .advance(&mut world, &mut players, Duration::from_millis(7400))
        .unwrap();
    assert!(
        executor
            .flight_views(Duration::from_millis(7400))
            .is_empty()
    );
    assert_eq!(players[&20].backpack[0].amount, 2);
    assert!(world.project(&players[&20]).is_ok());
    assert_eq!(demo.instance().document().objects.len(), models);
}

fn coop_host_tick(demo: &mut SceneDemo) {
    use bozzard_demo::factory::host::HostRuntime;
    demo.advance_with_frame(demo.app.timestep(), || ()).unwrap();
    assert_eq!(
        demo.app.world.resource::<HostRuntime>().unwrap().error,
        None
    );
}

fn coop_host_action(
    demo: &mut SceneDemo,
    peer: u64,
    action: bozzard_demo::factory::replication::requests::Action,
) -> bool {
    use bozzard_demo::factory::{host::HostRuntime, replication::requests::Request};
    let host = demo.app.world.resource_mut::<HostRuntime>().unwrap();
    let replica = host.snapshot(peer).unwrap();
    let bytes = Request {
        epoch: replica.epoch,
        connection: replica.connection,
        sequence: replica.acknowledged + 1,
        action,
    }
    .encode()
    .unwrap();
    assert!(host.receive(peer, &bytes).unwrap());
    assert!(
        !host.receive(peer, &bytes).unwrap(),
        "duplicate packet queued twice"
    );
    coop_host_tick(demo);
    let host = demo.app.world.resource::<HostRuntime>().unwrap();
    assert_eq!(
        host.snapshot(peer).unwrap().acknowledged,
        replica.acknowledged + 1
    );
    host.outcomes[&peer].accepted
}

#[test]
fn coop_host_guest_construction_enters_live_worker_production_and_preserves_host_view() {
    use bozzard_demo::factory::{
        host::HostRuntime, replication::requests::Action, shared::Position,
    };
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true); set_object_variable("phase",7.0);
        test_build(157,9.0); power::connect_power(power::power_id(112),power::power_id(157));
        let nodes=get_scene_list("nodes");nodes[142]=1.0;nodes[143]=0.0;nodes[144]=0.0;set_scene_list("nodes",nodes);
        set_scene_variable("cursor_x",-4.0);set_scene_variable("cursor_z",0.0);backpack::give(8.0,11.0);
    "#,
    );
    demo.set_threaded_simulation(true).unwrap();
    let stock = controller_numbers(&demo, "stock");
    let names = [
        (10, "Host".into()),
        (20, "Red".into()),
        (30, "Orange".into()),
        (40, "Green".into()),
    ]
    .into();
    HostRuntime::start(&mut demo.app.world, 10, 1, names).unwrap();
    let host = demo.app.world.resource::<HostRuntime>().unwrap();
    assert_eq!(
        host.snapshot(20)
            .unwrap()
            .members
            .iter()
            .map(|m| m.slot)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert!(coop_host_action(
        &mut demo,
        20,
        Action::Place {
            kind: 1,
            direction: 0
        }
    ));
    assert_eq!(numbers(&demo, "builds")[142], 1.);
    assert!(
        !coop_host_action(
            &mut demo,
            30,
            Action::Place {
                kind: 3,
                direction: 0
            }
        ),
        "conflicting guest placement accepted"
    );
    assert!(coop_host_action(
        &mut demo,
        20,
        Action::Wire {
            from: Position {
                planet: 0,
                x: 0,
                z: 2
            },
            to: Position {
                planet: 0,
                x: 0,
                z: 3
            }
        }
    ));
    assert!(coop_host_action(&mut demo, 20, Action::Move { x: 1, z: 0 }));
    assert!(coop_host_action(
        &mut demo,
        20,
        Action::Place {
            kind: 2,
            direction: 0
        }
    ));
    assert!(coop_host_action(&mut demo, 20, Action::Move { x: 1, z: 0 }));
    assert!(coop_host_action(
        &mut demo,
        20,
        Action::Place {
            kind: 4,
            direction: 0
        }
    ));
    for _ in 0..150 {
        coop_host_tick(&mut demo);
    }
    assert!(
        numbers(&demo, "counts")[1] >= 2.,
        "guest-built line never entered real production"
    );
    assert_eq!(controller_numbers(&demo, "stock"), stock);
    assert_eq!(
        (number(&demo, "cursor_x"), number(&demo, "cursor_z")),
        (-4., 0.)
    );
    let transforms = demo.instance().global_transforms(&demo.app.world).unwrap();
    for (x, asset) in [
        (0., "machine-miner"),
        (1., "machine-belt"),
        (2., "machine-storage"),
    ] {
        assert!(
            demo.instance()
                .document()
                .prefabs
                .iter()
                .any(|(root, prefab)| {
                    let pos = transforms[root].transform_point3(glam::Vec3::ZERO);
                    prefab.asset == asset && (pos.x - x).abs() < 0.01 && (pos.z - 2.).abs() < 0.01
                }),
            "missing guest machine model"
        );
    }
    let camera = demo.instance().global_transforms(&demo.app.world).unwrap()["camera-rig"];
    assert!(coop_host_action(
        &mut demo,
        20,
        Action::TakeStorage {
            at: Position {
                planet: 0,
                x: 2,
                z: 2
            }
        }
    ));
    assert!(
        demo.app
            .world
            .resource::<HostRuntime>()
            .unwrap()
            .snapshot(20)
            .unwrap()
            .player
            .backpack
            .iter()
            .any(|s| s.kind == 1 && s.amount > 0)
    );
    assert_eq!(controller_numbers(&demo, "stock"), stock);
    for _ in 0..8 {
        assert!(coop_host_action(&mut demo, 40, Action::Move { x: 1, z: 0 }));
    }
    let snapshot = demo
        .app
        .world
        .resource::<HostRuntime>()
        .unwrap()
        .snapshot(40)
        .unwrap();
    assert!(
        snapshot
            .world
            .discovered(Position {
                planet: 0,
                x: 15,
                z: 0
            })
            .unwrap()
    );
    assert_eq!(controller_numbers(&demo, "visited")[145], 1.);
    assert_eq!(
        demo.instance().global_transforms(&demo.app.world).unwrap()["camera-rig"],
        camera
    );
    assert!(
        demo.app
            .world
            .resource::<bozzard_diagnostics::SimulationMetrics>()
            .unwrap()
            .threaded
    );
}

#[test]
fn coop_host_remote_rotation_pauses_production_and_removal_reconciles_once() {
    use bozzard_demo::factory::{host::HostRuntime, replication::requests::Action};
    let mut demo = multi_region_factory(
        "[[0,2,1,0,1]]",
        "set_scene_variable(\"cursor_x\",-4.0);set_scene_variable(\"cursor_z\",0.0);",
    );
    HostRuntime::start(
        &mut demo.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    assert!(coop_host_action(&mut demo, 20, Action::Rotate));
    let before = numbers(&demo, "item_amounts")[142];
    for _ in 0..20 {
        coop_host_tick(&mut demo);
    }
    assert_eq!(
        numbers(&demo, "item_amounts")[142],
        before,
        "rotating remote machine continued mining"
    );
    let handle = demo
        .instance()
        .document()
        .prefabs
        .iter()
        .find(|(_, p)| p.asset == "machine-miner")
        .unwrap()
        .0
        .clone();
    assert!(
        demo.instance().global_transforms(&demo.app.world).unwrap()[&handle]
            .transform_point3(glam::Vec3::ZERO)
            .y
            > 0.4
    );
    for _ in 0..20 {
        coop_host_tick(&mut demo);
    }
    assert_eq!(numbers(&demo, "facings")[142], 1.);
    let before = numbers(&demo, "item_amounts")[142];
    for _ in 0..40 {
        coop_host_tick(&mut demo);
    }
    assert!(numbers(&demo, "item_amounts")[142] > before);
    assert!(coop_host_action(&mut demo, 20, Action::Remove));
    coop_host_tick(&mut demo);
    assert_eq!(numbers(&demo, "builds")[142], 0.);
    assert!(
        matches!(&demo.app.world.resource::<BlueprintRuntime>().unwrap().scene_blackboard()["build_visuals"].values()[142],Value::Text(s) if s.is_empty())
    );
    let models = demo.instance().document().objects.len();
    for _ in 0..10 {
        coop_host_tick(&mut demo);
    }
    assert_eq!(
        demo.instance().document().objects.len(),
        models,
        "acknowledged edit replayed its model changes"
    );
}

#[test]
fn coop_host_load_and_new_world_replace_epoch_and_reject_previous_requests() {
    use bozzard_demo::factory::{
        host::HostRuntime,
        replication::requests::{Action, Request},
    };
    let mut demo = stellar_fixture(
        "set_object_variable(\"creative\",true);set_object_variable(\"phase\",7.0);",
    );
    let directory = save_directory(&mut demo);
    HostRuntime::start(
        &mut demo.app.world,
        10,
        5,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    assert!(coop_host_action(&mut demo, 20, Action::Move { x: 1, z: 0 }));
    factory_code(&mut demo, "persistence::prepare_save(1);", 1);
    restore_factory_update(&mut demo);
    finish_save_io(&mut demo);
    assert!(coop_host_action(&mut demo, 20, Action::Move { x: 1, z: 0 }));
    let old = demo
        .app
        .world
        .resource::<HostRuntime>()
        .unwrap()
        .snapshot(20)
        .unwrap();
    let stale = Request {
        epoch: old.epoch,
        connection: old.connection,
        sequence: old.acknowledged + 1,
        action: Action::Place {
            kind: 2,
            direction: 0,
        },
    }
    .encode()
    .unwrap();
    factory_code(
        &mut demo,
        "data::session_set(117,1.0);data::session_set(116,2.0);",
        1,
    );
    restore_factory_update(&mut demo);
    finish_save_io(&mut demo);
    coop_host_tick(&mut demo);
    let host = demo.app.world.resource_mut::<HostRuntime>().unwrap();
    let loaded = host.snapshot(20).unwrap();
    assert_eq!(loaded.epoch, 6);
    assert_eq!(loaded.player.position.x, 1);
    assert_eq!(loaded.acknowledged, 0);
    assert!(host.receive(20, &stale).is_err());
    factory_code(&mut demo, "world::begin_world(27);", 1);
    restore_factory_update(&mut demo);
    coop_host_tick(&mut demo);
    let fresh = demo
        .app
        .world
        .resource::<HostRuntime>()
        .unwrap()
        .snapshot(20)
        .unwrap();
    assert_eq!(fresh.epoch, 7);
    assert_eq!(fresh.player.position, Default::default());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn coop_host_guest_on_moon_uses_live_remote_factory_and_rejoins_without_moving_host() {
    use bozzard_demo::factory::{
        Session, host::HostRuntime, replication::requests::Action, shared::Position,
    };
    let mut demo = multi_region_factory("[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]", "");
    board_other_planet(&mut demo);
    build_lunar_test_line(&mut demo);
    board_other_planet(&mut demo);
    restore_factory_update(&mut demo);
    HostRuntime::start(
        &mut demo.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    demo.set_threaded_simulation(true).unwrap();
    let stock = controller_numbers(&demo, "stock");
    assert!(coop_host_action(&mut demo, 20, Action::Travel));
    for _ in 0..270 {
        coop_host_tick(&mut demo);
    }
    assert_eq!(controller_numbers(&demo, "session")[7], 0.);
    assert_eq!(
        demo.app
            .world
            .resource::<HostRuntime>()
            .unwrap()
            .snapshot(20)
            .unwrap()
            .player
            .position
            .planet,
        1
    );
    assert!(
        numbers(&demo, "counts")[1] > 0. && controller_numbers(&demo, "session")[32] > 0.,
        "both planets must keep producing during a guest's flight"
    );
    for _ in 0..8 {
        assert!(coop_host_action(&mut demo, 20, Action::Move { x: 1, z: 0 }));
    }
    for _ in 0..2 {
        assert!(coop_host_action(
            &mut demo,
            20,
            Action::Move { x: 0, z: -1 }
        ));
    }
    assert!(coop_host_action(
        &mut demo,
        20,
        Action::TakeStorage {
            at: Position {
                planet: 1,
                x: 9,
                z: 0
            }
        }
    ));
    let guest = demo
        .app
        .world
        .resource::<HostRuntime>()
        .unwrap()
        .snapshot(20)
        .unwrap()
        .player;
    assert!(guest.backpack.iter().any(|s| s.kind == 24 && s.amount > 0));
    assert_eq!(controller_numbers(&demo, "stock"), stock);
    assert_eq!(
        (number(&demo, "chunk_x"), number(&demo, "chunk_z")),
        (0., 0.)
    );
    assert_eq!(
        (number(&demo, "cursor_x"), number(&demo, "cursor_z")),
        (0., 2.)
    );
    assert!(
        demo.instance()
            .document()
            .prefabs
            .values()
            .all(|p| p.asset != "moon-chunk"
                && !["node-amorium", "node-moondust", "node-techtorium"]
                    .contains(&p.asset.as_str())),
        "remote gameplay spawned off-planet rendering"
    );
    for _ in 0..2 {
        assert!(coop_host_action(
            &mut demo,
            20,
            Action::Move { x: -1, z: 0 }
        ));
    }
    assert!(coop_host_action(
        &mut demo,
        20,
        Action::Gather { active: true }
    ));
    let disconnected = demo
        .app
        .world
        .resource::<HostRuntime>()
        .unwrap()
        .snapshot(20)
        .unwrap();
    demo.app
        .world
        .resource_mut::<HostRuntime>()
        .unwrap()
        .members([(10, "Host".into())].into())
        .unwrap();
    for _ in 0..45 {
        coop_host_tick(&mut demo);
    }
    assert_eq!(
        demo.app.world.resource::<Session>().unwrap().players[&20],
        disconnected.player,
        "disconnected player kept gathering"
    );
    demo.app
        .world
        .resource_mut::<HostRuntime>()
        .unwrap()
        .members([(10, "Host".into()), (20, "Guest returned".into())].into())
        .unwrap();
    let rejoined = demo
        .app
        .world
        .resource::<HostRuntime>()
        .unwrap()
        .snapshot(20)
        .unwrap();
    assert_eq!(rejoined.player, disconnected.player);
    assert_ne!(rejoined.connection, disconnected.connection);
}

#[test]
fn coop_host_guest_storage_edits_refresh_open_panels_and_removed_storage_closes() {
    use bozzard_demo::factory::{
        host::HostRuntime, replication::requests::Action, shared::Position,
    };
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",7.0);
        test_build(142,4.0);let slots=grid::empty_numbers(32);slots[0]=11.0;slots[1]=7.0;inventory::storage_write(142,slots);
        let counts=get_scene_list("counts");counts[11]=7.0;set_scene_list("counts",counts);
        set_scene_variable("cursor_x",0.0);set_scene_variable("cursor_z",3.0);
    "#,
    );
    HostRuntime::start(
        &mut demo.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    press(&mut demo, "E");
    assert!(matches!(
        demo.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .scene_blackboard()["storage_open"],
        BlackboardValue::Scalar(Value::Bool(true))
    ));
    assert_eq!(numbers(&demo, "storage_view")[1], 7.);
    assert!(coop_host_action(
        &mut demo,
        20,
        Action::TakeStorage {
            at: Position {
                planet: 0,
                x: 0,
                z: 2
            }
        }
    ));
    coop_host_tick(&mut demo);
    assert!(matches!(
        demo.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .scene_blackboard()["storage_open"],
        BlackboardValue::Scalar(Value::Bool(true))
    ));
    assert_eq!(numbers(&demo, "storage_view")[1], 0.);
    assert_eq!(numbers(&demo, "counts")[11], 0.);
    assert!(coop_host_action(&mut demo, 20, Action::Remove));
    coop_host_tick(&mut demo);
    assert!(matches!(
        demo.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .scene_blackboard()["storage_open"],
        BlackboardValue::Scalar(Value::Bool(false))
    ));
    assert_eq!(number(&demo, "storage_drag"), -1.);
    assert_eq!(number(&demo, "storage_menu"), -1.);
}

// Two complete game instances, with the exact fragmented host protocol between
// them. Neither these helpers nor GuestRuntime run an alternate factory loop.
fn coop_guest(host: &SceneDemo) -> SceneDemo {
    use bozzard_demo::factory::guest::GuestRuntime;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../earth-factory/scenes/earth.json");
    let scene = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut guest = SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
    tick(&mut guest, None); // Initialize gameplay while the title screen is open.
    GuestRuntime::start(&mut guest.app.world, 10, 20).unwrap();
    coop_guest_snapshot(host, &mut guest);
    for _ in 0..8 {
        tick(&mut guest, None);
    }
    assert!(guest.app.world.resource::<GuestRuntime>().unwrap().ready());
    assert_eq!(controller_numbers(&guest, "session")[122], 2.);
    guest
}
fn coop_guest_snapshot(host: &SceneDemo, guest: &mut SceneDemo) {
    use bozzard_demo::factory::{
        guest::GuestRuntime,
        host::HostRuntime,
        replication::{Update, stream::Sender},
    };
    let next = host
        .app
        .world
        .resource::<HostRuntime>()
        .unwrap()
        .snapshot(20)
        .unwrap();
    let receiver = guest.app.world.resource_mut::<GuestRuntime>().unwrap();
    let update = match receiver.current() {
        Some(previous) if previous.epoch == next.epoch && previous.revision == next.revision => {
            return;
        }
        Some(previous) => Update::between(previous, &next).unwrap(),
        None => Update::Full(next),
    };
    let sender = Sender::new(&update).unwrap();
    for index in 0..sender.packets() {
        assert_eq!(
            receiver
                .receive(
                    10,
                    &sender.packet(index).unwrap(),
                    std::time::Duration::ZERO
                )
                .unwrap(),
            index + 1 == sender.packets()
        );
    }
}
fn coop_guest_requests(host: &mut SceneDemo, guest: &mut SceneDemo) -> usize {
    use bozzard_demo::factory::{
        guest::GuestRuntime, host::HostRuntime, replication::requests::Request,
    };
    let mut count = 0;
    let guest = guest.app.world.resource_mut::<GuestRuntime>().unwrap();
    while let Some(bytes) = guest.next_request().unwrap() {
        let request: Request = serde_json::from_slice(&bytes).unwrap();
        assert!(
            host.app
                .world
                .resource_mut::<HostRuntime>()
                .unwrap()
                .receive(20, &bytes)
                .unwrap()
        );
        guest.mark_sent(request.sequence).unwrap();
        count += 1;
    }
    count
}
fn coop_guest_exchange(host: &mut SceneDemo, guest: &mut SceneDemo) {
    coop_guest_requests(host, guest);
    coop_host_tick(host);
    coop_guest_snapshot(host, guest);
    tick(guest, None);
    tick(guest, None);
}

#[test]
fn coop_guest_animates_authoritative_transfers_across_chunks_on_the_other_planet() {
    use bozzard_demo::factory::host::HostRuntime;
    let mut host = multi_region_factory(
        "[[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]",
        r#"
        let items=get_scene_list("items");items[119]=12.0;set_scene_list("items",items);
        let amounts=get_scene_list("item_amounts");amounts[119]=1.0;set_scene_list("item_amounts",amounts);
    "#,
    );
    // Host is on the Moon; the guest remains on Earth beside the cross-chunk belt.
    board_other_planet(&mut host);
    HostRuntime::start(
        &mut host.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    let mut guest = coop_guest(&host);
    factory_code(&mut host, "simulation::factory_step();", 1);
    let replica = host
        .app
        .world
        .resource::<HostRuntime>()
        .unwrap()
        .snapshot(20)
        .unwrap();
    assert_eq!(replica.effects.items.len(), 1);
    assert_eq!(replica.effects.items[0].planet, 0);
    for invalid in 0..3 {
        let mut candidate = replica.clone();
        match invalid {
            0 => candidate
                .effects
                .items
                .push(candidate.effects.items[0].clone()),
            1 => candidate.effects.items[0].to += 2,
            _ => candidate.effects.items[0].planet = 1,
        }
        assert!(
            candidate.validate(10, 20).is_err(),
            "invalid transfer {invalid} accepted"
        );
    }
    assert_eq!(
        replica.effects.items[0]
            .position(replica.effects.items[0].from)
            .unwrap()
            .x,
        7
    );
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 2);
    let position = |demo: &SceneDemo| {
        let scene = demo.instance().capture(&demo.app.world).unwrap();
        scene
            .objects
            .iter()
            .filter(|o| o.name == "Moving item")
            .filter(|o| {
                !demo
                    .app
                    .world
                    .get::<bozzard_scene::BlueprintHidden>(demo.instance().entity(&o.id).unwrap())
                    .is_some_and(|h| h.0)
            })
            .map(|o| o.transform.translation)
            .find(|p| p[0] >= 7. && p[0] <= 8. && p[1] > 0. && p[2].abs() < 0.01)
            .unwrap()
    };
    let first = position(&guest);
    assert!(
        first[0] > 7. && first[0] < 7.5,
        "{first:?}; age={} clock={} tick={} motions={:?}; items={:?}",
        controller_numbers(&guest, "session")[124],
        number(&guest, "clock"),
        number(&guest, "ticks"),
        guest
            .app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .scene_blackboard()["motion_visuals"]
            .values()
            .last(),
        guest
            .instance()
            .capture(&guest.app.world)
            .unwrap()
            .objects
            .iter()
            .filter(|o| o.name == "Moving item")
            .map(|o| (&o.id, o.transform.translation))
            .collect::<Vec<_>>()
    );
    let beat = number(&guest, "ticks");
    let items = number(&guest, "clock");
    settle(&mut guest, 6); // No host tick and no new packet.
    let second = position(&guest);
    assert!(
        second[0] > first[0] + 0.2 && second[0] < 8.,
        "{first:?} -> {second:?}"
    );
    assert_eq!(number(&guest, "ticks"), beat);
    assert_eq!(
        number(&guest, "clock"),
        items,
        "guest advanced authoritative clock"
    );
    settle(&mut guest, 20);
    assert!((position(&guest)[0] - 8.).abs() < 0.001);
    assert_eq!(number(&guest, "ticks"), beat);
    // The next host beat consumes the belt item into storage, with a final moving visual.
    factory_code(&mut host, "simulation::factory_step();", 1);
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 2);
    assert_eq!(numbers(&guest, "counts")[12], 1.);
    factory_code(
        &mut guest,
        r#"
        let active=get_scene_list_item("motion_visuals",867);
        if active=="" {throw "storage arrival has no animation";}
        let page=grid::unpack_numbers(active,0)[0].to_int();
        let handle="";for h in grid::unpack_text(get_scene_list_item("motion_visuals",page),75) {if h!="" {handle=h;}}
        if handle=="" || get_position(handle)[0]<=8.0 || get_position(handle)[0]>=9.0 {throw "storage arrival snapped";}
    "#,
        1,
    );
    restore_factory_update(&mut guest);
    settle(&mut guest, 24);
    assert_eq!(
        numbers(&guest, "counts")[12],
        1.,
        "guest duplicated delivery"
    );
}

#[test]
fn coop_guest_predicts_movement_but_builds_and_collects_only_after_host_accepts() {
    use bozzard_demo::factory::{guest::GuestRuntime, host::HostRuntime};
    let mut host = stellar_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",7.0);
        test_build(157,9.0);power::connect_power(power::power_id(112),power::power_id(157));
        let nodes=get_scene_list("nodes");nodes[142]=1.0;nodes[143]=0.0;set_scene_list("nodes",nodes);
        set_scene_variable("cursor_x",-4.0);set_scene_variable("cursor_z",0.0);backpack::give(8.0,11.0);
    "#,
    );
    HostRuntime::start(
        &mut host.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    let mut guest = coop_guest(&host);
    assert_eq!(number(&guest, "cursor_x"), 0.);
    assert_eq!(number(&guest, "cursor_z"), 2.);
    assert_eq!(
        controller_numbers(&guest, "stock")[8],
        0.,
        "host inventory leaked"
    );
    let beats = number(&guest, "ticks");
    settle(&mut guest, 30);
    assert_eq!(
        number(&guest, "ticks"),
        beats,
        "guest advanced production independently"
    );

    press(&mut guest, "2"); // production action bar: Miner Mk1
    assert_eq!(number(&guest, "selected"), 1.);
    press(&mut guest, "Space");
    assert_eq!(
        numbers(&guest, "builds")[142],
        0.,
        "unacknowledged build mutated guest world"
    );
    assert_eq!(numbers(&host, "builds")[142], 0.);
    coop_guest_exchange(&mut host, &mut guest);
    assert_eq!(numbers(&guest, "builds")[142], 1.);
    assert_eq!(numbers(&host, "builds")[142], 1.);
    assert!(host.app.world.resource::<HostRuntime>().unwrap().outcomes[&20].accepted);
    // Rejected placement is acknowledged, leaving both worlds and inventories intact.
    press(&mut guest, "Space");
    coop_guest_exchange(&mut host, &mut guest);
    assert!(!host.app.world.resource::<HostRuntime>().unwrap().outcomes[&20].accepted);
    assert_eq!(numbers(&guest, "builds")[142], 1.);

    factory_code(
        &mut guest,
        r#"net::send("wire",#{from:net::position(157),to:net::position(142)});"#,
        1,
    );
    restore_factory_update(&mut guest);
    coop_guest_exchange(&mut host, &mut guest);
    for _ in 0..100 {
        coop_host_tick(&mut host);
    }
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 3);
    assert!(numbers(&guest, "item_amounts")[142] > 0.);
    let amount = numbers(&guest, "item_amounts")[142];
    factory_code(&mut guest, "inventory::collect_machine(142);", 1);
    restore_factory_update(&mut guest);
    assert_eq!(controller_numbers(&guest, "stock")[1], 0.);
    coop_guest_exchange(&mut host, &mut guest);
    assert_eq!(controller_numbers(&guest, "stock")[1], amount);
    assert_eq!(numbers(&host, "item_amounts")[142], 0.);
    assert_eq!(controller_numbers(&host, "stock")[1], 0.);

    press(&mut guest, "D");
    assert_eq!(
        number(&guest, "cursor_x"),
        1.,
        "local movement must not await a host reply"
    );
    coop_guest_exchange(&mut host, &mut guest);
    assert_eq!(number(&guest, "cursor_x"), 1.);
    assert_eq!(number(&host, "cursor_x"), -4.);
    assert_eq!(controller_numbers(&host, "stock")[8], 11.);
    assert!(guest.app.world.resource::<GuestRuntime>().unwrap().ready());
}

#[test]
fn coop_guest_prediction_reconciles_delayed_batched_steps_and_orders_builds() {
    use bozzard_demo::factory::{guest::GuestRuntime, host::HostRuntime};
    let mut host = stellar_fixture(
        r#"set_object_variable("creative",true);set_object_variable("phase",7.0);"#,
    );
    HostRuntime::start(
        &mut host.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    let mut guest = coop_guest(&host);
    let at = |demo: &SceneDemo| {
        (
            number(demo, "chunk_x") * 15. + number(demo, "cursor_x"),
            number(demo, "chunk_z") * 15. + number(demo, "cursor_z"),
        )
    };
    let host_at = |demo: &SceneDemo| {
        demo.app
            .world
            .resource::<HostRuntime>()
            .unwrap()
            .snapshot(20)
            .unwrap()
            .player
            .position
    };

    // No host packets for 200 ms: each key still moves the local marker next tick.
    for x in 1..=3 {
        press(&mut guest, "D");
        assert_eq!(at(&guest), (x as f32, 2.));
        let model = guest
            .instance()
            .global_transforms(&guest.app.world)
            .unwrap()["cursor"];
        assert_eq!((model.w_axis.x, model.w_axis.z), (x as f32, 2.));
    }
    settle(&mut guest, 12);
    assert_eq!(
        at(&guest),
        (3., 2.),
        "pending steps must not repeat on idle frames"
    );
    assert_eq!(host_at(&host).x, 0, "client prediction changed host state");
    assert_eq!(coop_guest_requests(&mut host, &mut guest), 3);
    coop_host_tick(&mut host); // The first step only; a network burst must not lose the rest.
    assert_eq!(host_at(&host).x, 1);
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 2);
    assert_eq!(
        at(&guest),
        (3., 2.),
        "partial acknowledgement rewound newer input"
    );
    press(&mut guest, "W");
    assert_eq!(at(&guest), (3., 1.));
    factory_code(
        &mut guest,
        r#"net::send("place",#{kind:2,direction:0});"#,
        1,
    );
    restore_factory_update(&mut guest);
    assert_eq!(numbers(&guest, "builds")[130], 0.);
    assert_eq!(coop_guest_requests(&mut host, &mut guest), 2);
    for x in [2, 3] {
        coop_host_tick(&mut host);
        assert_eq!(host_at(&host).x, x);
        assert_eq!(host_at(&host).z, 2);
        assert_eq!(
            numbers(&host, "builds")[130],
            0.,
            "build overtook pending movement"
        );
        coop_guest_snapshot(&host, &mut guest);
        settle(&mut guest, 2);
        assert_eq!(at(&guest), (3., 1.), "acknowledged step was applied twice");
    }
    coop_host_tick(&mut host);
    assert_eq!((host_at(&host).x, host_at(&host).z), (3, 1));
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 2);
    assert_eq!(numbers(&host, "builds")[130], 2.);
    assert_eq!(numbers(&guest, "builds")[130], 2.);
    assert_eq!(at(&guest), (3., 1.));
    assert!(
        !guest
            .app
            .world
            .resource::<GuestRuntime>()
            .unwrap()
            .has_pending_requests()
    );
}

#[test]
fn coop_guest_prediction_reconciles_mouse_keyboard_and_rejected_inflight_moves() {
    use bozzard_demo::factory::host::HostRuntime;
    let mut host = stellar_fixture(
        r#"set_object_variable("creative",true);set_object_variable("phase",7.0);"#,
    );
    HostRuntime::start(
        &mut host.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    let mut guest = coop_guest(&host);
    press(&mut guest, "D");
    factory_code(&mut guest, "pointer::select_tile([0,0,-2,1]);", 1);
    restore_factory_update(&mut guest);
    press(&mut guest, "W");
    assert_eq!(
        (number(&guest, "cursor_x"), number(&guest, "cursor_z")),
        (-2., 0.)
    );
    coop_guest_requests(&mut host, &mut guest);
    for _ in 0..2 {
        coop_host_tick(&mut host);
        coop_guest_snapshot(&host, &mut guest);
        settle(&mut guest, 2);
        assert_eq!(
            (number(&guest, "cursor_x"), number(&guest, "cursor_z")),
            (-2., 0.)
        );
    }
    factory_code(&mut guest, "pointer::select_tile([0,0,0,1]);", 1);
    restore_factory_update(&mut guest);
    coop_guest_exchange(&mut host, &mut guest);
    // A launch and a stale queued move can reach the host together. Never
    // predict past the launch, and reconcile the rejected move's acknowledgement.
    factory_code(
        &mut guest,
        r#"net::send("travel",#{});net::send("move",#{x:1,z:0});"#,
        1,
    );
    restore_factory_update(&mut guest);
    settle(&mut guest, 2);
    assert_eq!(number(&guest, "cursor_x"), 0.);
    coop_guest_exchange(&mut host, &mut guest);
    assert!(!host.app.world.resource::<HostRuntime>().unwrap().outcomes[&20].accepted);
    assert_eq!(number(&guest, "cursor_x"), 0.);
    assert_eq!(controller_numbers(&guest, "session")[44], 1.);
    press(&mut guest, "D");
    assert_eq!(number(&guest, "cursor_x"), 0.);
}

#[test]
fn coop_guest_crosses_chunks_travels_and_resets_on_a_new_host_world() {
    use bozzard_demo::factory::{
        Session,
        guest::GuestRuntime,
        host::HostRuntime,
        shared::{Player, Stack},
    };
    let mut host = multi_region_factory("[[6,0,1,0,1],[7,0,2,0,0],[8,0,2,0,0],[9,0,4,0,0]]", "");
    let mut player = Player::default();
    player.backpack[4] = Stack {
        kind: 8,
        amount: 37,
    };
    host.app
        .world
        .resource_mut::<Session>()
        .unwrap()
        .players
        .insert(20, player);
    HostRuntime::start(
        &mut host.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    let mut guest = coop_guest(&host);
    guest.set_threaded_simulation(true).unwrap();
    host.set_threaded_simulation(true).unwrap();
    // Host prefetch supplies terrain near the edge; crossing that known seam
    // is then predicted without waiting for the crossing's acknowledgement.
    for x in 1..=8 {
        press(&mut guest, "D");
        assert_eq!(
            number(&guest, "chunk_x") * 15. + number(&guest, "cursor_x"),
            x as f32
        );
        coop_guest_exchange(&mut host, &mut guest);
    }
    assert_eq!(number(&guest, "chunk_x"), 1.);
    assert_eq!(number(&guest, "cursor_x"), -7.);
    assert_eq!(number(&host, "chunk_x"), 0.);
    assert!(controller_numbers(&host, "visited")[145] > 0.);
    // Looking at a shared chunk must not archive an older client copy over the host's production.
    for _ in 0..60 {
        coop_host_tick(&mut host);
    }
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 3);
    assert_eq!(numbers(&guest, "counts")[1], numbers(&host, "counts")[1]);
    factory_code(
        &mut guest,
        r#"net::send("point",#{at:#{planet:0,x:0,z:1}});"#,
        1,
    );
    restore_factory_update(&mut guest);
    coop_guest_exchange(&mut host, &mut guest);
    assert_eq!(number(&guest, "chunk_x"), 0.);
    assert_eq!(number(&guest, "cursor_z"), 1.);
    factory_code(&mut guest, "flight::start();", 1);
    restore_factory_update(&mut guest);
    coop_guest_exchange(&mut host, &mut guest);
    assert_eq!(controller_numbers(&guest, "session")[44], 1.);
    let before = numbers(&host, "counts")[1];
    for _ in 0..145 {
        coop_host_tick(&mut host);
    }
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 8);
    assert!(guest.app.world.resource::<GuestRuntime>().unwrap().ready());
    assert_eq!(controller_numbers(&guest, "session")[7], 1.);
    assert_eq!(controller_numbers(&host, "session")[7], 0.);
    assert!(numbers(&host, "counts")[1] > before);
    assert_eq!(
        controller_numbers(&guest, "session")[9],
        numbers(&host, "counts")[1]
    );
    assert_eq!(controller_numbers(&guest, "stock")[8], 37.);
    assert_eq!(
        controller_numbers(&guest, "session")[44],
        2.,
        "arrival lost descending animation"
    );
    for _ in 0..130 {
        coop_host_tick(&mut host);
    }
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 3);
    assert_eq!(controller_numbers(&guest, "session")[44], 0.);
    assert!(
        numbers(&guest, "nodes")
            .iter()
            .all(|kind| *kind == 0. || (24. ..=26.).contains(kind))
    );
    let snapshot = guest
        .app
        .world
        .resource::<GuestRuntime>()
        .unwrap()
        .current()
        .unwrap()
        .clone();
    factory_code(&mut guest, r#"net::send("move",#{x:1,z:0});"#, 1);
    restore_factory_update(&mut guest);
    assert!(
        guest
            .app
            .world
            .resource::<GuestRuntime>()
            .unwrap()
            .next_request()
            .unwrap()
            .is_some()
    );
    factory_code(&mut host, "world::begin_world(99);", 1);
    restore_factory_update(&mut host);
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 8);
    let service = guest.app.world.resource::<GuestRuntime>().unwrap();
    assert!(service.ready());
    assert!(service.current().unwrap().epoch > snapshot.epoch);
    assert!(
        service.next_request().unwrap().is_none(),
        "old-world input survived reset"
    );
    assert_eq!(controller_numbers(&guest, "session")[7], 0.);
    assert_eq!(number(&guest, "seed"), 99.);
    assert!(controller_numbers(&guest, "stock").iter().all(|n| *n == 0.));
}

#[test]
fn coop_link_retries_queue_pressure_and_uses_only_acknowledged_delta_bases() {
    use bozzard_demo::factory::{guest::GuestRuntime, host::HostRuntime, link::Link};
    use std::time::Duration;
    let mut host = stellar_fixture(
        "set_object_variable(\"creative\",true);set_object_variable(\"phase\",7.0);",
    );
    HostRuntime::start(
        &mut host.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    let mut guest = coop_guest(&host);
    let mut server = Link::default();
    let mut client = Link::default();
    let mut now = Duration::ZERO;
    // A failed Steam send does not advance the fragment index.
    let mut failed = Vec::new();
    assert!(
        server
            .pump(&mut host.app.world, now, |peer, bytes| {
                assert_eq!(peer, 20);
                failed = bytes.to_vec();
                anyhow::bail!("send queue full")
            })
            .is_err()
    );
    let mut first = true;
    for _ in 0..32 {
        now += Duration::from_millis(100);
        let mut sent = 0;
        server
            .pump(&mut host.app.world, now, |peer, bytes| {
                assert_eq!(peer, 20);
                if first {
                    assert_eq!(bytes, failed);
                    first = false;
                }
                client.receive(&mut guest.app.world, 10, bytes, now)?;
                sent += 1;
                Ok(())
            })
            .unwrap();
        assert!(sent <= 8);
        if sent == 0 {
            break;
        }
    }
    // Hold the completed snapshot's acknowledgement while the host advances.
    coop_host_tick(&mut host);
    now += Duration::from_millis(200);
    server
        .pump(&mut host.app.world, now, |_, _| {
            anyhow::bail!("published a delta before its base was acknowledged")
        })
        .unwrap();
    client
        .pump(&mut guest.app.world, now, |peer, bytes| {
            assert_eq!(peer, 10);
            server.receive(&mut host.app.world, 20, bytes, now)
        })
        .unwrap();
    assert!(
        server
            .receive(&mut host.app.world, 30, b"SA01", now)
            .is_err(),
        "nonmember acknowledgement accepted"
    );
    now += Duration::from_millis(200);
    let old = guest
        .app
        .world
        .resource::<GuestRuntime>()
        .unwrap()
        .current()
        .unwrap()
        .revision;
    let mut packets = 0;
    server
        .pump(&mut host.app.world, now, |_, bytes| {
            packets += 1;
            client.receive(&mut guest.app.world, 10, bytes, now)
        })
        .unwrap();
    assert!(packets > 0);
    assert!(
        guest
            .app
            .world
            .resource::<GuestRuntime>()
            .unwrap()
            .current()
            .unwrap()
            .revision
            > old
    );
    settle(&mut guest, 3);
    let x = number(&guest, "cursor_x");
    press(&mut guest, "D");
    assert_eq!(
        number(&guest, "cursor_x"),
        x + 1.,
        "movement waited for the send queue"
    );
    let mut calls = 0;
    assert!(
        client
            .pump(&mut guest.app.world, now, |_, bytes| {
                calls += 1;
                if bytes.starts_with(b"SA01") {
                    server.receive(&mut host.app.world, 20, bytes, now)
                } else {
                    anyhow::bail!("request queue full")
                }
            })
            .is_err()
    );
    assert!(calls > 0);
    assert!(
        guest
            .app
            .world
            .resource::<GuestRuntime>()
            .unwrap()
            .next_request()
            .unwrap()
            .is_some()
    );
    now += Duration::from_millis(100);
    client
        .pump(&mut guest.app.world, now, |_, bytes| {
            server.receive(&mut host.app.world, 20, bytes, now)
        })
        .unwrap();
    coop_host_tick(&mut host);
    now += Duration::from_millis(100);
    server
        .pump(&mut host.app.world, now, |_, bytes| {
            client.receive(&mut guest.app.world, 10, bytes, now)
        })
        .unwrap();
    settle(&mut guest, 3);
    assert_eq!(number(&guest, "cursor_x"), x + 1.);
    // Steam accepted a request, but its session broke before delivery. Retrying
    // unacknowledged requests must recover it without applying a step twice.
    press(&mut guest, "D");
    assert_eq!(number(&guest, "cursor_x"), x + 2.);
    let mut lost = Vec::new();
    client
        .pump(&mut guest.app.world, now, |_, bytes| {
            if bytes.starts_with(b"SA01") {
                server.receive(&mut host.app.world, 20, bytes, now)?;
            } else {
                lost = bytes.to_vec();
            }
            Ok(())
        })
        .unwrap();
    assert!(!lost.is_empty());
    now += Duration::from_millis(1100);
    client
        .pump(&mut guest.app.world, now, |_, bytes| {
            assert_eq!(bytes, lost);
            server.receive(&mut host.app.world, 20, bytes, now)
        })
        .unwrap();
    // A delayed duplicate is harmless even before the action completes.
    server.receive(&mut host.app.world, 20, &lost, now).unwrap();
    coop_host_tick(&mut host);
    server
        .pump(&mut host.app.world, now, |_, bytes| {
            client.receive(&mut guest.app.world, 10, bytes, now)
        })
        .unwrap();
    settle(&mut guest, 3);
    assert_eq!(number(&guest, "cursor_x"), x + 2.);
}

#[test]
#[cfg(not(feature = "steam"))]
fn coop_native_menu_reports_unavailable_steam_and_preserves_solo_play() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../earth-factory/scenes/earth.json");
    let scene = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut demo = SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
    tick(&mut demo, None);
    demo.enable_multiplayer(None).unwrap();
    demo.pump_multiplayer().unwrap();
    click_widget(&mut demo, "coop-open-title");
    assert!(shown(&demo, "coop-overlay"));
    click_widget(&mut demo, "coop-create");
    assert!(
        demo.app
            .world
            .resource::<bozzard_scene::middleware::ui::Runtime>()
            .unwrap()
            .widgets["coop-status"]
            .text
            .as_ref()
            .unwrap()
            .contains("unavailable")
    );
    click_widget(&mut demo, "coop-close");
    assert!(!shown(&demo, "coop-overlay"));
    click_widget(&mut demo, "title-create");
    demo.pump_multiplayer().unwrap();
    assert!(!shown(&demo, "title-overlay"));
    assert!(!demo.multiplayer_active());
    demo.set_threaded_simulation(true).unwrap();
    press(&mut demo, "Escape");
    click_widget(&mut demo, "coop-open-menu");
    assert!(shown(&demo, "coop-overlay"));
    click_widget(&mut demo, "coop-close");
    click_widget(&mut demo, "menu-continue");
    let x = number(&demo, "cursor_x");
    press(&mut demo, "D");
    assert_eq!(number(&demo, "cursor_x"), x + 1.);
}

#[test]
fn coop_players_show_slot_colors_and_only_nearby_names() {
    use bozzard_demo::factory::{
        Session,
        host::HostRuntime,
        shared::{PLAYER_COLORS, Player, Position},
    };
    let mut host = stellar_fixture(
        r#"set_object_variable("creative",true);set_object_variable("phase",7.0);set_scene_variable("cursor_x",0.0);set_scene_variable("cursor_z",2.0);"#,
    );
    for (peer, x) in [(20, 2), (30, 4), (40, 6)] {
        let player = Player {
            position: Position { planet: 0, x, z: 2 },
            ..Default::default()
        };
        host.app
            .world
            .resource_mut::<Session>()
            .unwrap()
            .players
            .insert(peer, player);
    }
    HostRuntime::start(
        &mut host.app.world,
        10,
        1,
        [
            (10, "Host".into()),
            (20, "Red".into()),
            (30, "Orange".into()),
            (40, "Green".into()),
        ]
        .into(),
    )
    .unwrap();
    coop_host_tick(&mut host);
    for (slot, id) in [
        (0, "cursor"),
        (1, "coop-player-1"),
        (2, "coop-player-2"),
        (3, "coop-player-3"),
    ] {
        let drawable = host
            .app
            .world
            .get::<bozzard_scene::Drawable>(host.instance().entity(id).unwrap())
            .unwrap();
        assert_eq!(drawable.color, PLAYER_COLORS[slot]);
    }
    assert!(!shown(&host, "coop-name-0"));
    for slot in 1..4 {
        assert!(shown(&host, &format!("coop-name-{slot}")));
    }
    let mut guest = coop_guest(&host);
    let color = guest
        .app
        .world
        .get::<bozzard_scene::Drawable>(guest.instance().entity("cursor").unwrap())
        .unwrap()
        .color;
    assert_eq!(color, PLAYER_COLORS[1]);
    assert!(shown(&guest, "coop-name-0"));
    assert!(!shown(&guest, "coop-name-1"));
    assert!(coop_host_action(
        &mut host,
        40,
        bozzard_demo::factory::replication::requests::Action::Move { x: 1, z: 0 }
    ));
    coop_host_tick(&mut host);
    assert!(!shown(&host, "coop-name-3"));
    host.app
        .world
        .resource_mut::<HostRuntime>()
        .unwrap()
        .members([(10, "Host".into()), (20, "Red".into())].into())
        .unwrap();
    coop_host_tick(&mut host);
    coop_host_tick(&mut host);
    assert!(!shown(&host, "coop-name-2"));
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 3);
    assert!(!shown(&guest, "coop-name-2"));
}

#[test]
fn expansion_every_machine_and_recipe_produces_all_outputs_only_with_power() {
    // Explicit game recipes: the assertions do not derive expected products
    // from the implementation's recipe table.
    type RecipeCase<'a> = (u8, u8, &'a [(u8, u8)], &'a [(u8, u8)], usize);
    let recipes: &[RecipeCase<'_>] = &[
        (12, 7, &[], &[(7, 1)], 2),
        (13, 6, &[], &[(6, 1)], 2),
        (14, 27, &[(8, 1)], &[(27, 1)], 4),
        (14, 28, &[(1, 1)], &[(28, 1)], 4),
        (14, 29, &[(2, 1)], &[(29, 1)], 4),
        (15, 30, &[(28, 1), (7, 1)], &[(30, 1)], 4),
        (15, 31, &[(29, 1), (7, 1)], &[(31, 1)], 4),
        (16, 32, &[(11, 2), (4, 1)], &[(32, 2)], 4),
        (16, 52, &[(30, 1)], &[(11, 3)], 4),
        (16, 53, &[(31, 1)], &[(12, 3)], 4),
        (17, 33, &[(6, 3)], &[(33, 2), (34, 1)], 4),
        (18, 35, &[(34, 2)], &[(35, 1)], 4),
        (18, 36, &[(33, 2)], &[(36, 1)], 4),
        (18, 37, &[(34, 1), (3, 1)], &[(37, 1)], 4),
        (19, 38, &[(7, 2)], &[(38, 2), (39, 1)], 4),
        (20, 41, &[(40, 1)], &[(41, 1)], 4),
        (20, 42, &[(3, 1)], &[(42, 1)], 4),
        (20, 43, &[(5, 1)], &[(43, 1)], 4),
        (21, 14, &[(9, 1)], &[(14, 1)], 2),
        (21, 44, &[(14, 2)], &[(44, 1)], 4),
        (22, 46, &[(45, 1), (7, 2), (37, 1)], &[(46, 4), (45, 1)], 8),
        (23, 47, &[(12, 1), (43, 1), (35, 1)], &[(47, 1)], 4),
        (24, 48, &[(32, 1), (17, 2)], &[(48, 1)], 4),
        (
            24,
            49,
            &[(48, 2), (47, 2), (32, 2), (36, 2)],
            &[(49, 1), (50, 1)],
            6,
        ),
        (25, 50, &[(50, 2)], &[(11, 1), (35, 1)], 4),
        (25, 51, &[(49, 1)], &[(11, 2), (12, 1), (35, 1)], 4),
    ];
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",7.0);
        test_build(157,9.0);power::connect_power(power::power_id(112),power::power_id(157));
    "#,
    );
    for &(kind, recipe, inputs, outputs, beats) in recipes {
        let pairs: Vec<_> = inputs.iter().map(|&(k, a)| vec![k, a]).collect();
        factory_code(
            &mut demo,
            &format!(
                r#"
            set_scene_variable("cursor_x",3.0);set_scene_variable("cursor_z",1.0);
            building::remove_selected();
            let nodes=get_scene_list("nodes");nodes[130]={node}.0;set_scene_list("nodes",nodes);
            set_scene_variable("selected",{kind}.0);set_scene_variable("direction",0.0);building::place_selected();
            let recipes=get_object_list("recipes");recipes[130]={recipe}.0;set_object_list("recipes",recipes);
            let slots=grid::empty_numbers(32);let pairs={pairs:?};
            for i in 0..pairs.len() {{slots[i*2]=pairs[i][0].to_float();slots[i*2+1]=pairs[i][1].to_float();}}
            inventory::storage_write(130,slots);
        "#,
                node = if kind == 12 {
                    7
                } else if kind == 13 {
                    6
                } else {
                    0
                }
            ),
            1,
        );
        assert_eq!(numbers(&demo, "builds")[130], kind as f32);
        factory_code(&mut demo, "simulation::factory_step();", 2);
        assert!(
            inventory(&demo, 130)[8..].iter().all(|&(_, a)| a == 0.),
            "unpowered kind {kind}"
        );
        factory_code(
            &mut demo,
            "power::connect_power(power::power_id(157),power::power_id(130));simulation::factory_step();",
            1,
        );
        assert_eq!(controller_numbers(&demo, "power_live")[130], 1.);
        factory_code(&mut demo, "simulation::factory_step();", beats - 1);
        let slots = inventory(&demo, 130);
        assert!(
            slots[..8].iter().all(|&(_, a)| a == 0.),
            "inputs not consumed for recipe {recipe}"
        );
        let got: Vec<_> = slots[8..]
            .iter()
            .copied()
            .filter(|&(_, a)| a > 0.)
            .collect();
        let expected: Vec<_> = outputs.iter().map(|&(k, a)| (k as f32, a as f32)).collect();
        assert_eq!(got, expected, "machine {kind}, recipe {recipe}");
        assert_eq!(numbers(&demo, "progress")[130], 0.);
        assert_eq!(
            numbers(&demo, "counts").iter().sum::<f32>(),
            0.,
            "production buffers are not container stock"
        );
    }
}

#[test]
fn expansion_fluid_pipes_conserve_units_lock_material_and_forward_once_per_beat() {
    let mut demo = buffer_layout("[[112,26,0,7,40],[113,26,0,0,0],[114,4,0,0,0]]", "");
    factory_code(&mut demo, "simulation::factory_step();", 1);
    assert_eq!(numbers(&demo, "item_amounts")[112], 35.);
    assert_eq!(numbers(&demo, "item_amounts")[113], 5.);
    assert_eq!(inventory(&demo, 114).iter().map(|s| s.1).sum::<f32>(), 0.);
    factory_code(&mut demo, "simulation::factory_step();", 20);
    let stored = inventory(&demo, 114).iter().map(|s| s.1).sum::<f32>();
    assert!(stored > 30.);
    assert_eq!(
        numbers(&demo, "item_amounts")[112] + numbers(&demo, "item_amounts")[113] + stored,
        40.
    );
    assert_eq!(numbers(&demo, "counts")[7], stored);
    let visual = demo
        .app
        .world
        .resource::<BlueprintRuntime>()
        .unwrap()
        .scene_blackboard()["item_visuals"]
        .values();
    assert!(matches!(&visual[112],Value::Text(t) if t.is_empty()));
    assert!(matches!(&visual[113],Value::Text(t) if t.is_empty()));
    factory_code(
        &mut demo,
        r#"
        let items=get_scene_list("items");items[112]=7.0;items[113]=6.0;set_scene_list("items",items);
        let amounts=get_scene_list("item_amounts");amounts[112]=80.0;amounts[113]=10.0;set_scene_list("item_amounts",amounts);
        set_scene_variable("cursor_x",2.0);set_scene_variable("cursor_z",0.0);building::remove_selected();
        simulation::factory_step();
    "#,
        1,
    );
    assert_eq!(&numbers(&demo, "item_amounts")[112..114], &[80., 10.]);
    assert_eq!(&numbers(&demo, "items")[112..114], &[7., 6.]);
}

#[test]
fn expansion_pipe_elbows_and_corner_belts_match_ports_in_all_rotations() {
    for facing in 0..4 {
        let mut demo = buffer_layout("[]", "");
        factory_code(
            &mut demo,
            &format!(
                r#"
            let center=112;let source=grid::index(grid::layout_x(0,1,0,{facing}),grid::layout_z(0,1,0,{facing}));
            let target=grid::index(grid::layout_x(1,0,0,{facing}),grid::layout_z(1,0,0,{facing}));
            for row in [[center,27,{facing}],[source,26,({facing}+1)%4],[target,4,{facing}]] {{
                let nodes=get_scene_list("nodes");nodes[row[0]]=0.0;set_scene_list("nodes",nodes);
                set_scene_variable("cursor_x",grid::cell_x(row[0]).to_float());set_scene_variable("cursor_z",grid::cell_z(row[0]).to_float());
                set_scene_variable("selected",row[1].to_float());set_scene_variable("direction",row[2].to_float());building::place_selected();
            }}
            let items=get_scene_list("items");items[source]=7.0;set_scene_list("items",items);
            let amounts=get_scene_list("item_amounts");amounts[source]=20.0;set_scene_list("item_amounts",amounts);
            simulation::factory_step();
        "#
            ),
            1,
        );
        assert_eq!(
            numbers(&demo, "item_amounts")[112],
            5.,
            "elbow rotation {facing}"
        );
        factory_code(&mut demo, "simulation::factory_step();", 5);
        assert!(numbers(&demo, "counts")[7] > 0.);
        for kind in [28, 29] {
            let side = if kind == 28 { -1 } else { 1 };
            let mut belt_demo = buffer_layout("[]", "");
            factory_code(
                &mut belt_demo,
                &format!(
                    r#"
                let source=grid::index(grid::layout_x(0,{side},0,{facing}),grid::layout_z(0,{side},0,{facing}));
                let target=grid::index(grid::layout_x(1,0,0,{facing}),grid::layout_z(1,0,0,{facing}));
                let wrong=grid::index(grid::layout_x(-1,0,0,{facing}),grid::layout_z(-1,0,0,{facing}));
                for row in [[112,{kind},{facing}],[source,2,({facing}+{inlet})%4],[target,4,{facing}],[wrong,2,{facing}]] {{
                    let nodes=get_scene_list("nodes");nodes[row[0]]=0.0;set_scene_list("nodes",nodes);
                    set_scene_variable("cursor_x",grid::cell_x(row[0]).to_float());set_scene_variable("cursor_z",grid::cell_z(row[0]).to_float());
                    set_scene_variable("selected",row[1].to_float());set_scene_variable("direction",row[2].to_float());building::place_selected();
                }}
                let items=get_scene_list("items");items[source]=11.0;items[wrong]=12.0;set_scene_list("items",items);
                let amounts=get_scene_list("item_amounts");amounts[source]=1.0;amounts[wrong]=1.0;set_scene_list("item_amounts",amounts);
                simulation::factory_step();
            "#,
                    inlet = if kind == 28 { 1 } else { 3 }
                ),
                1,
            );
            assert_eq!(numbers(&belt_demo, "items")[112], 11.);
            assert_eq!(
                numbers(&belt_demo, "counts")[11],
                0.,
                "new arrival must wait"
            );
            factory_code(&mut belt_demo, "simulation::factory_step();", 1);
            assert_eq!(numbers(&belt_demo, "counts")[11], 1.);
            assert_eq!(
                numbers(&belt_demo, "item_amounts").iter().sum::<f32>(),
                1.,
                "wrong inlet stays blocked"
            );
        }
    }
}

#[test]
fn expansion_mixed_ingredients_reserve_capacity_and_recipe_changes_refund_atomically() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",7.0);test_build(130,24.0);
        machines::select_recipe(130,49.0);
        import "factory-buffers" as buffers;
        let slots=grid::empty_numbers(32);
        for i in 0..100 {let result=buffers::accept(slots,49,48.0);if result.ok {slots=result.slots;}}
        inventory::storage_write(130,slots);
    "#,
    );
    assert_eq!(inventory(&demo, 130)[0], (48., 94.));
    factory_code(
        &mut demo,
        r#"
        set_object_variable("creative",false);backpack::give(47.0,2.0);backpack::give(32.0,2.0);backpack::give(36.0,2.0);
        machines::feed_assembler(130);
    "#,
        1,
    );
    assert_eq!(inventory(&demo, 130).iter().map(|s| s.1).sum::<f32>(), 100.);
    assert_eq!(controller_numbers(&demo, "stock").iter().sum::<f32>(), 0.);
    factory_code(&mut demo, "machines::select_recipe(130,48.0);", 1);
    assert_eq!(controller_numbers(&demo, "stock")[48], 94.);
    assert_eq!(controller_numbers(&demo, "stock")[47], 2.);
    assert!(inventory(&demo, 130).iter().all(|s| s.1 == 0.));
    factory_code(
        &mut demo,
        r#"
        let slots=grid::empty_numbers(32);slots[0]=32.0;slots[1]=3.0;slots[16]=48.0;slots[17]=2.0;
        inventory::storage_write(130,slots);machines::select_recipe(130,49.0);
    "#,
        1,
    );
    assert_eq!(controller_numbers(&demo, "recipes")[130], 48.);
    assert_eq!(inventory(&demo, 130)[8], (48., 2.));
    factory_code(
        &mut demo,
        "inventory::collect_machine(130);machines::select_recipe(130,49.0);",
        1,
    );
    assert_eq!(controller_numbers(&demo, "recipes")[130], 49.);
    assert_eq!(controller_numbers(&demo, "stock")[48], 96.);
    assert_eq!(controller_numbers(&demo, "stock")[32], 5.);
}

#[test]
fn expansion_saves_round_trip_buffers_fluids_and_upgrade_original_material_arrays() {
    use bozzard_demo::factory::{
        saves,
        shared::{Player, World},
        state::State,
    };
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",7.0);
        test_build(130,17.0);test_build(131,26.0);
        let slots=grid::empty_numbers(32);slots[0]=6.0;slots[1]=3.0;slots[16]=33.0;slots[17]=20.0;slots[18]=34.0;slots[19]=10.0;
        inventory::storage_write(130,slots);backpack::give(47.0,5.0);
        let items=get_scene_list("items");items[131]=33.0;set_scene_list("items",items);
        let amounts=get_scene_list("item_amounts");amounts[131]=37.0;set_scene_list("item_amounts",amounts);
        let counts=get_scene_list("counts");counts[49]=7.0;set_scene_list("counts",counts);
        let other=grid::empty_numbers(64);other[49]=11.0;data::set_other_counts(other);
    "#,
    );
    let state =
        State::capture_live(demo.app.world.resource::<BlueprintRuntime>().unwrap()).unwrap();
    let root = save_directory(&mut demo);
    saves::write(&root, 1, &saves::Save::new(state.clone())).unwrap();
    assert_eq!(saves::read(&root, 1).unwrap().state, state);
    let mut oversized = state.clone();
    let mut amounts = vec![0; 900];
    amounts[130 * 4] = 90;
    oversized
        .controller
        .get_mut("cache_storage_amounts_0")
        .unwrap()
        .values_mut()[144] = Value::Text(
        amounts
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
    );
    assert!(
        oversized.validate().is_err(),
        "oversized saved production buffers must be rejected"
    );
    let (_, mut player) = World::from_local(state.clone()).unwrap();
    player.position.planet = 1;
    // The lunar home has not been discovered in this fixture. Test swapping via
    // a discovered copy while preserving all expansion fields.
    let mut explored = state.clone();
    explored
        .controller
        .get_mut("chunk_nodes")
        .unwrap()
        .values_mut()[433] = Value::Text("0:225".into());
    let (world, _) = World::from_local(explored).unwrap();
    let moon = world.project(&player).unwrap();
    assert_eq!(moon.scene["counts"].values()[49], Value::Number(11.));
    assert_eq!(moon.controller["session"].values()[145], Value::Number(7.));
    assert_eq!(
        world
            .project(&Player::capture(&state).unwrap())
            .unwrap()
            .controller["stock"]
            .values()[47],
        Value::Number(5.)
    );
    let mut old = stellar_fixture("");
    let original =
        State::capture_live(old.app.world.resource::<BlueprintRuntime>().unwrap()).unwrap();
    let mut legacy = saves::Save::new(original.clone());
    for (board, key, length) in [
        (&mut legacy.state.scene, "counts", 32),
        (&mut legacy.state.controller, "stock", 32),
    ] {
        if let BlackboardValue::List {
            values, capacity, ..
        } = board.get_mut(key).unwrap()
        {
            values.truncate(length);
            *capacity = length;
        }
    }
    if let BlackboardValue::List {
        values, capacity, ..
    } = legacy.state.controller.get_mut("session").unwrap()
    {
        values.truncate(128);
        *capacity = 128;
    }
    let old_root = save_directory(&mut old);
    std::fs::create_dir_all(&old_root).unwrap();
    std::fs::write(
        old_root.join("slot-1.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    assert_eq!(saves::read(&old_root, 1).unwrap().state, original);
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(old_root).unwrap();
}

#[test]
fn expansion_refinery_backpressure_retains_both_products_and_separate_outlets_drain_them() {
    let mut demo = stellar_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",7.0);
        for row in [[157,9.0],[146,17.0],[145,26.0]] {test_build(row[0],row[1]);}
        power::connect_power(power::power_id(112),power::power_id(157));power::connect_power(power::power_id(157),power::power_id(146));
        let items=get_scene_list("items");items[145]=6.0;set_scene_list("items",items);
        let amounts=get_scene_list("item_amounts");amounts[145]=30.0;set_scene_list("item_amounts",amounts);
        let slots=grid::empty_numbers(32);slots[16]=33.0;slots[17]=50.0;slots[18]=34.0;slots[19]=50.0;
        inventory::storage_write(146,slots);
    "#,
    );
    factory_code(&mut demo, "simulation::factory_step();", 8);
    assert_eq!(numbers(&demo, "item_amounts")[145], 30.);
    assert_eq!(inventory(&demo, 146)[8..10], [(33., 50.), (34., 50.)]);
    factory_code(
        &mut demo,
        r#"
        inventory::storage_write(146,grid::empty_numbers(32));
        for row in [[147,26.0,0.0],[148,4.0,0.0],[161,26.0,1.0],[176,4.0,1.0]] {
            let cell=row[0];let nodes=get_scene_list("nodes");nodes[cell]=0.0;set_scene_list("nodes",nodes);
            set_scene_variable("cursor_x",grid::cell_x(cell).to_float());set_scene_variable("cursor_z",grid::cell_z(cell).to_float());
            set_scene_variable("direction",row[2]);set_scene_variable("selected",row[1]);building::place_selected();
            if get_scene_list("builds")[cell]!=row[1] {throw "refinery outlet not placed";}
        }
    "#,
        1,
    );
    factory_code(&mut demo, "simulation::factory_step();", 60);
    assert_eq!(numbers(&demo, "counts")[33], 20.);
    assert_eq!(numbers(&demo, "counts")[34], 10.);
    assert!(inventory(&demo, 146).iter().all(|s| s.1 == 0.));
    assert_eq!(
        numbers(&demo, "item_amounts")[145]
            + numbers(&demo, "item_amounts")[147]
            + numbers(&demo, "item_amounts")[161],
        0.
    );
    assert_eq!(inventory(&demo, 148)[0], (33., 20.));
    assert_eq!(inventory(&demo, 176)[0], (34., 10.));
}

#[test]
fn expansion_factories_and_fluid_lines_cross_seams_and_continue_on_other_planets() {
    let mut demo = multi_region_factory(
        "[[6,0,16,0,0],[7,0,2,0,0],[8,0,4,0,0]]",
        r#"let slots=grid::empty_numbers(32);slots[0]=11.0;slots[1]=20.0;slots[2]=4.0;slots[3]=10.0;inventory::storage_write(118,slots);"#,
    );
    factory_code(&mut demo, "simulation::factory_step();", 10);
    let stored = numbers(&demo, "counts")[32];
    assert!(stored > 0.);
    factory_code(
        &mut demo,
        r#"
        set_scene_variable("cursor_x",0.0);set_scene_variable("cursor_z",1.0);world::travel_to_other_planet();
    "#,
        1,
    );
    assert_eq!(controller_numbers(&demo, "session")[7], 1.);
    factory_code(&mut demo, "simulation::factory_step();", 45);
    assert_eq!(controller_numbers(&demo, "session")[128], 20.);
    assert_eq!(numbers(&demo, "counts")[32], 0.);
    factory_code(
        &mut demo,
        r#"
        set_scene_variable("cursor_x",0.0);set_scene_variable("cursor_z",1.0);world::travel_to_other_planet();
    "#,
        1,
    );
    assert_eq!(numbers(&demo, "counts")[32], 20.);
    assert!(inventory(&demo, 118).iter().all(|s| s.1 == 0.));
    let mut fluid = multi_region_factory(
        "[[6,0,12,0,7],[7,0,26,0,0],[8,0,26,0,0],[9,0,27,1,0],[9,1,26,1,0],[9,2,4,1,0]]",
        "",
    );
    factory_code(&mut fluid, "simulation::factory_step();", 40);
    assert!(
        numbers(&fluid, "counts")[7] > 0.,
        "water must cross the region seam and elbow"
    );
    let stored = numbers(&fluid, "counts")[7];
    factory_code(
        &mut fluid,
        r#"set_scene_variable("cursor_x",0.0);set_scene_variable("cursor_z",1.0);world::travel_to_other_planet();"#,
        1,
    );
    factory_code(&mut fluid, "simulation::factory_step();", 40);
    assert!(controller_numbers(&fluid, "session")[15] > stored);
}

#[test]
fn expansion_coop_guest_builds_feeds_and_collects_manufacturer_through_a_corner_belt() {
    use bozzard_demo::factory::{
        host::HostRuntime, replication::requests::Action, shared::Position,
    };
    let mut host = stellar_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",7.0);
        test_build(157,9.0);power::connect_power(power::power_id(112),power::power_id(157));
        let nodes=get_scene_list("nodes");for cell in [142,143,128,113] {nodes[cell]=0.0;}set_scene_list("nodes",nodes);
        set_scene_variable("cursor_x",-4.0);set_scene_variable("cursor_z",0.0);backpack::give(8.0,11.0);
    "#,
    );
    host.set_threaded_simulation(true).unwrap();
    HostRuntime::start(
        &mut host.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    assert!(coop_host_action(
        &mut host,
        20,
        Action::Place {
            kind: 24,
            direction: 0
        }
    ));
    let machine = Position {
        planet: 0,
        x: 0,
        z: 2,
    };
    assert!(coop_host_action(
        &mut host,
        20,
        Action::Configure {
            at: machine,
            recipe: 49
        }
    ));
    assert!(coop_host_action(
        &mut host,
        20,
        Action::Feed { at: machine }
    ));
    assert!(coop_host_action(
        &mut host,
        20,
        Action::Wire {
            from: machine,
            to: Position {
                planet: 0,
                x: 0,
                z: 3
            }
        }
    ));
    for (x, z, kind, direction) in [(1, 0, 28, 3), (0, -1, 2, 3), (0, -1, 4, 3)] {
        assert!(coop_host_action(&mut host, 20, Action::Move { x, z }));
        assert!(coop_host_action(
            &mut host,
            20,
            Action::Place { kind, direction }
        ));
    }
    let mut guest = coop_guest(&host);
    for i in 0..360 {
        coop_host_tick(&mut host);
        if i % 12 == 0 {
            coop_guest_snapshot(&host, &mut guest);
            tick(&mut guest, None);
        }
    }
    assert!(numbers(&host, "counts")[49] > 0.);
    assert!(
        numbers(&host, "counts")[50] > 0.,
        "scrap byproduct must also leave the manufacturer"
    );
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 3);
    assert_eq!(numbers(&guest, "counts")[49], numbers(&host, "counts")[49]);
    assert_eq!(numbers(&guest, "counts")[50], numbers(&host, "counts")[50]);
    assert!(coop_host_action(
        &mut host,
        20,
        Action::TakeStorage {
            at: Position {
                planet: 0,
                x: 1,
                z: 0
            }
        }
    ));
    let player = host
        .app
        .world
        .resource::<HostRuntime>()
        .unwrap()
        .snapshot(20)
        .unwrap()
        .player;
    assert!(player.backpack.iter().any(|s| s.kind == 49 && s.amount > 0));
    assert!(player.backpack.iter().any(|s| s.kind == 50 && s.amount > 0));
    assert_eq!(
        controller_numbers(&host, "stock")[8],
        11.,
        "guest transactions preserve the host's backpack"
    );
}

fn foundation_fixture() -> SceneDemo {
    script_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",7.0);
        let pieces=grid::empty_numbers(900);
        for z in 3..6 {for x in 3..6 {
            let cell=grid::index(x,z);pieces[cell*4]=30.0;pieces[cell*4+1]=36.0;
            for dir in 0..4 {
                let nx=x+grid::step_x(dir);let nz=z+grid::step_z(dir);
                if nx>=3 && nx<=5 && nz>=3 && nz<=5 {continue;}
                let a=architecture::address(x,z,32.0,dir);pieces[a.slot]=32.0;
            }
        }}
        pieces[architecture::address(4,5,38.0,1).slot]=38.0;
        pieces[architecture::address(5,4,39.0,0).slot]=39.0;
        grid::cache_put("cache_structures",144,grid::pack_numbers(pieces));
        set_scene_variable("cursor_x",4.0);set_scene_variable("cursor_z",4.0);
        set_scene_variable("selected",3.0);building::remove_selected();building::place_selected();
        set_scene_variable("cursor_x",4.0);set_scene_variable("cursor_z",6.0);
        interiors::load(144);interiors::update(0.0);
    "#,
    )
}
fn foundation_mesh_visible(demo: &SceneDemo, asset: &str, x: f32, z: f32) -> bool {
    demo.instance()
        .view(&demo.app.world, bozzard_scene::Layer::ThreeD, 1.6)
        .unwrap()
        .objects
        .iter()
        .any(|(m, d)| {
            matches!(&d.mesh,bozzard_scene::Mesh::Asset(id) if id.ends_with(asset))
                && (m.w_axis.x - x).abs() < 0.001
                && (m.w_axis.z - z).abs() < 0.001
        })
}
#[test]
fn foundations_hide_sealed_rooms_reveal_only_window_sightlines_and_cut_away_inside() {
    let mut demo = foundation_fixture();
    assert!(!foundation_mesh_visible(&demo, "smelter-mk1", 4., 4.));
    assert!(foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        4.,
        4.
    ));
    factory_code(
        &mut demo,
        "set_scene_variable(\"cursor_x\",6.0);set_scene_variable(\"cursor_z\",4.0);interiors::update(0.0);",
        1,
    );
    assert!(
        foundation_mesh_visible(&demo, "smelter-mk1", 4., 4.),
        "window exposes two tiles inward"
    );
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        4.,
        4.
    ));
    assert!(
        foundation_mesh_visible(&demo, "foundation-concrete-roof", 3., 4.),
        "window cannot expose the whole room"
    );
    factory_code(
        &mut demo,
        "set_scene_variable(\"cursor_x\",4.0);set_scene_variable(\"cursor_z\",4.0);interiors::update(0.0);",
        1,
    );
    assert!(foundation_mesh_visible(&demo, "smelter-mk1", 4., 4.));
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        3.,
        3.
    ));
    let view = demo
        .instance()
        .view(&demo.app.world, bozzard_scene::Layer::ThreeD, 1.6)
        .unwrap();
    let (_,ground)=view.objects.iter().find(|(_,d)|matches!(&d.mesh,bozzard_scene::Mesh::Asset(id) if id.ends_with("earth-ground"))).unwrap();
    assert!(
        ground.color.iter().all(|v| *v <= 0.1),
        "outdoors is darkened"
    );
    let (_,floor)=view.objects.iter().find(|(m,d)|m.w_axis.x==4. && m.w_axis.z==4. && matches!(&d.mesh,bozzard_scene::Mesh::Asset(id) if id.ends_with("foundation-concrete-floor"))).unwrap();
    assert_eq!(floor.color, [1.; 3], "indoor materials retain clear view");
    let at = demo
        .instance()
        .document()
        .objects
        .iter()
        .find(|o| o.name == "Concrete foundation")
        .unwrap();
    assert_eq!(
        demo.app
            .world
            .get::<bozzard_scene::Drawable>(demo.instance().entity(&at.id).unwrap())
            .unwrap()
            .color,
        [1.; 3],
        "cutaway must not alter persistent material colors"
    );
}
#[test]
fn foundations_doors_wait_before_entry_walls_block_keyboard_and_click_teleports() {
    let mut demo = foundation_fixture();
    press(&mut demo, "W");
    assert_eq!(number(&demo, "cursor_z"), 6., "door is still opening");
    settle(&mut demo, 20);
    press(&mut demo, "W");
    assert_eq!(
        number(&demo, "cursor_z"),
        6.,
        "partly open doors still block entry"
    );
    settle(&mut demo, 20);
    press(&mut demo, "W");
    assert_eq!(number(&demo, "cursor_z"), 5.);
    let count = demo.instance().document().objects.len();
    settle(&mut demo, 20);
    assert_eq!(
        demo.instance().document().objects.len(),
        count,
        "door motion reuses its leaves"
    );
    press(&mut demo, "D");
    assert_eq!(number(&demo, "cursor_x"), 5.);
    press(&mut demo, "D");
    assert_eq!(
        number(&demo, "cursor_x"),
        5.,
        "concrete wall blocks movement"
    );
    factory_code(&mut demo, "pointer::select_tile([0,0,6,4]);", 1);
    assert_eq!(
        number(&demo, "cursor_x"),
        5.,
        "pointer cannot jump across the window wall"
    );
    factory_code(
        &mut demo,
        "let pieces=architecture::page(get_object_list(\"cache_structures\"),0,144);pieces[grid::index(3,3)*4+1]=0.0;grid::cache_put(\"cache_structures\",144,grid::pack_numbers(pieces));interiors::update(0.0);",
        1,
    );
    assert!(
        foundation_mesh_visible(&demo, "foundation-concrete-roof", 4., 4.),
        "missing roof makes the factory unenclosed"
    );
}
#[test]
fn foundations_layers_preserve_machines_and_survive_save_travel_and_reset() {
    use bozzard_demo::factory::state::State;
    let mut demo = foundation_fixture();
    let cell = (4 + 7) * 15 + 4 + 7;
    let builds = numbers(&demo, "builds");
    assert_eq!(builds[cell], 3.);
    let saved =
        State::capture_live(demo.app.world.resource::<BlueprintRuntime>().unwrap()).unwrap();
    saved.validate().unwrap();
    assert!(
        !saved.controller["cache_structures"].values()[144]
            .text()
            .unwrap()
            .is_empty()
    );
    let decoded: State = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    assert_eq!(saved, decoded);
    let mut legacy = saved.clone();
    legacy.controller.remove("cache_structures");
    legacy.upgrade_legacy();
    legacy.validate().unwrap();
    factory_code(
        &mut demo,
        "set_scene_variable(\"demo_mode\",false);set_scene_variable(\"cursor_x\",0.0);set_scene_variable(\"cursor_z\",1.0);world::travel_to_other_planet();interiors::update(0.0);",
        1,
    );
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        4.,
        4.
    ));
    factory_code(
        &mut demo,
        "set_scene_variable(\"demo_mode\",false);set_scene_variable(\"cursor_x\",0.0);set_scene_variable(\"cursor_z\",1.0);world::travel_to_other_planet();interiors::update(0.0);",
        1,
    );
    assert!(foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        4.,
        4.
    ));
    assert_eq!(numbers(&demo, "builds")[cell], 3.);
    factory_code(
        &mut demo,
        "world::begin_world(19);interiors::update(0.0);",
        1,
    );
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        4.,
        4.
    ));
    assert!(
        demo.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .object_blackboard("controller")
            .unwrap()["cache_structures"]
            .values()
            .iter()
            .all(|v| v.text().unwrap().is_empty())
    );
}
#[test]
fn foundations_build_and_remove_every_layer_through_the_normal_controls() {
    let mut demo = foundation_fixture();
    let machines = numbers(&demo, "builds");
    factory_code(
        &mut demo,
        "set_scene_variable(\"cursor_x\",6.0);set_scene_variable(\"cursor_z\",3.0);set_scene_variable(\"selected\",31.0);set_scene_variable(\"direction\",0.0);",
        1,
    );
    restore_factory_update(&mut demo);
    press(&mut demo, "Space");
    assert!(
        foundation_mesh_visible(&demo, "foundation-wood-floor", 6., 3.),
        "selected={}, message={:?}",
        number(&demo, "selected"),
        demo.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .scene_blackboard()["message"]
    );
    factory_code(&mut demo, "set_scene_variable(\"selected\",37.0);", 1);
    restore_factory_update(&mut demo);
    press(&mut demo, "Space");
    assert!(foundation_mesh_visible(
        &demo,
        "foundation-wood-roof",
        6.,
        3.
    ));
    factory_code(&mut demo, "set_scene_variable(\"selected\",31.0);", 1);
    restore_factory_update(&mut demo);
    press(&mut demo, "X");
    assert!(
        foundation_mesh_visible(&demo, "foundation-wood-floor", 6., 3.),
        "remove the roof before its supporting floor"
    );
    for (kind, asset) in [
        (32, "concrete-wall"),
        (33, "brick-wall"),
        (34, "metal-wall"),
        (35, "wood-wall"),
        (38, "sliding-door"),
        (39, "window"),
    ] {
        factory_code(
            &mut demo,
            &format!("set_scene_variable(\"selected\",{kind}.0);"),
            1,
        );
        restore_factory_update(&mut demo);
        press(&mut demo, "Space");
        assert!(foundation_mesh_visible(
            &demo,
            &format!("foundation-{asset}"),
            6.5,
            3.
        ));
        press(&mut demo, "X");
        assert!(!foundation_mesh_visible(
            &demo,
            &format!("foundation-{asset}"),
            6.5,
            3.
        ));
    }
    press(&mut demo, "R");
    assert_eq!(number(&demo, "direction"), 1.);
    press(&mut demo, "Space");
    assert!(
        foundation_mesh_visible(&demo, "foundation-window", 6., 3.5),
        "R chooses the south edge"
    );
    press(&mut demo, "X");
    factory_code(&mut demo, "set_scene_variable(\"selected\",37.0);", 1);
    restore_factory_update(&mut demo);
    press(&mut demo, "X");
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-wood-roof",
        6.,
        3.
    ));
    factory_code(&mut demo, "set_scene_variable(\"selected\",31.0);", 1);
    restore_factory_update(&mut demo);
    press(&mut demo, "X");
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-wood-floor",
        6.,
        3.
    ));
    assert_eq!(
        numbers(&demo, "builds"),
        machines,
        "structure controls must preserve machines"
    );
}
#[test]
fn foundations_coop_uses_shared_layers_costs_and_authoritative_door_waits() {
    use bozzard_demo::factory::{
        authority::Executor,
        replication::requests::Action,
        shared::{Position, Stack},
    };
    use std::time::Duration;
    let mut demo = foundation_fixture();
    factory_code(
        &mut demo,
        "set_object_variable(\"creative\",false);set_scene_variable(\"demo_mode\",false);",
        1,
    );
    let (mut world, mut player) = coop_world(&demo);
    let mut executor = Executor::new(demo.instance()).unwrap();
    let before = player.position;
    assert!(
        executor
            .apply(
                &mut world,
                10,
                &mut player,
                &Action::Move { x: 0, z: -1 },
                Duration::ZERO
            )
            .is_err()
    );
    assert_eq!(player.position, before);
    executor
        .apply(
            &mut world,
            10,
            &mut player,
            &Action::Move { x: 0, z: -1 },
            Duration::from_millis(700),
        )
        .unwrap();
    assert_eq!(player.position.z, 5);
    assert!(
        executor
            .apply(
                &mut world,
                10,
                &mut player,
                &Action::Point {
                    at: Position {
                        x: 6,
                        z: 4,
                        ..before
                    }
                },
                Duration::from_millis(720)
            )
            .is_err()
    );
    player.position = Position {
        x: 6,
        z: 3,
        ..before
    };
    player.backpack[0] = Stack {
        kind: 18,
        amount: 6,
    };
    let build = Action::Structure {
        kind: 30,
        direction: 0,
        remove: false,
    };
    let placed = executor
        .apply(&mut world, 10, &mut player, &build, Duration::from_secs(1))
        .unwrap();
    assert!(placed.accepted, "{}", placed.message);
    assert_eq!(player.backpack[0].amount, 4);
    let committed = world.clone();
    assert!(
        !executor
            .apply(&mut world, 10, &mut player, &build, Duration::from_secs(2))
            .unwrap()
            .accepted
    );
    assert_eq!(player.backpack[0].amount, 4);
    assert_eq!(world, committed);
    executor
        .apply(
            &mut world,
            10,
            &mut player,
            &Action::Structure {
                kind: 36,
                direction: 0,
                remove: false,
            },
            Duration::from_secs(3),
        )
        .unwrap();
    assert_eq!(player.backpack[0].amount, 2);
    assert!(
        !executor
            .apply(
                &mut world,
                10,
                &mut player,
                &Action::Structure {
                    kind: 30,
                    direction: 0,
                    remove: true
                },
                Duration::from_secs(4)
            )
            .unwrap()
            .accepted,
        "floor with a roof cannot be removed"
    );
    world.validate().unwrap();
    let handles = demo
        .app
        .world
        .resource::<BlueprintRuntime>()
        .unwrap()
        .scene_blackboard()["build_visuals"]
        .clone();
    demo.app
        .world
        .resource_mut::<BlueprintRuntime>()
        .unwrap()
        .patch_blackboards(
            &Default::default(),
            &[(
                "controller".into(),
                [(
                    "cache_structures".into(),
                    world.state().controller["cache_structures"].clone(),
                )]
                .into(),
            )]
            .into(),
        )
        .unwrap();
    assert_eq!(
        demo.app
            .world
            .resource::<BlueprintRuntime>()
            .unwrap()
            .scene_blackboard()["build_visuals"],
        handles
    );
    factory_code(&mut demo, "interiors::update(0.0);", 1);
    assert!(foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        6.,
        3.
    ));
}
#[test]
fn foundations_enclosure_crosses_chunk_seams_and_door_edges_are_canonical() {
    use std::collections::BTreeMap;
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    let module = demo
        .instance()
        .script_module("factory-architecture")
        .unwrap();
    let mut data: BTreeMap<usize, Vec<f32>> = BTreeMap::new();
    let mut assign = |x: i64, z: i64, kind: f32, dir: i64| {
        let at: serde_json::Value = module.call_args("address", (x, z, kind, dir)).unwrap();
        let region = at["region"].as_i64().unwrap() as usize;
        let slot = at["slot"].as_i64().unwrap() as usize;
        data.entry(region).or_insert_with(|| vec![0.; 900])[slot] = kind;
    };
    for x in 7..=8 {
        assign(x, 4, 30., 0);
        assign(x, 4, 36., 0);
        assign(x, 4, 32., 1);
        assign(x, 4, 32., 3);
    }
    assign(7, 4, 38., 2);
    assign(8, 4, 39., 0);
    let pages = |data: &BTreeMap<usize, Vec<f32>>| {
        let mut pages = vec![String::new(); 578];
        for (region, values) in data {
            pages[*region] = values
                .iter()
                .map(|v| (*v as u32).to_string())
                .collect::<Vec<_>>()
                .join(",");
        }
        pages
    };
    let plan: serde_json::Value = module.call_args("topology", (pages(&data), 0i64)).unwrap();
    assert_eq!(plan["rooms"].as_array().unwrap().len(), 1);
    assert_eq!(plan["membership"]["7,4"], plan["membership"]["8,4"]);
    for (x, dir) in [(6i64, 0i64), (7, 2)] {
        let kind: f32 = module
            .call_args("barrier", (pages(&data), 0i64, x, 4i64, dir))
            .unwrap();
        assert_eq!(kind, 38.);
    }
    let at: serde_json::Value = module
        .call_args("address", (8i64, 4i64, 36f32, 0i64))
        .unwrap();
    data.get_mut(&(at["region"].as_i64().unwrap() as usize))
        .unwrap()[at["slot"].as_i64().unwrap() as usize] = 0.;
    let open: serde_json::Value = module.call_args("topology", (pages(&data), 0i64)).unwrap();
    assert!(open["rooms"].as_array().unwrap().is_empty());
}
#[test]
fn foundations_cached_doors_and_cutaways_survive_crossing_and_reloading_a_chunk_seam() {
    let mut demo = script_fixture(
        r#"
        set_object_variable("creative",true);set_object_variable("phase",7.0);
        let parts=#{};
        for z in 4..6 {for x in 7..9 {
            for kind in [30.0,36.0] {
                let a=architecture::address(x,z,kind,0);let key=a.region.to_string();
                if !parts.contains(key) {parts[key]=grid::empty_numbers(900);}
                parts[key][a.slot]=kind;
            }
            for dir in 0..4 {
                let nx=x+grid::step_x(dir);let nz=z+grid::step_z(dir);
                if nx>=7 && nx<=8 && nz>=4 && nz<=5 {continue;}
                let a=architecture::address(x,z,32.0,dir);let key=a.region.to_string();
                if !parts.contains(key) {parts[key]=grid::empty_numbers(900);}
                parts[key][a.slot]=32.0;
            }
        }}
        let west=architecture::address(7,4,38.0,2);parts[west.region.to_string()][west.slot]=38.0;
        let south=architecture::address(8,5,38.0,1);parts[south.region.to_string()][south.slot]=38.0;
        let window=architecture::address(8,5,39.0,0);parts[window.region.to_string()][window.slot]=39.0;
        for key in parts.keys() {grid::cache_put("cache_structures",parse_int(key),grid::pack_numbers(parts[key]));}
        chunks::discover_chunk(1,0);interiors::load(144);interiors::load(145);
        spawn_prefab(data::build_asset(3.0),[8.0,0.08,5.0]);
        set_scene_variable("cursor_x",6.0);set_scene_variable("cursor_z",4.0);interiors::update(0.0);
    "#,
    );
    assert!(!foundation_mesh_visible(&demo, "smelter-mk1", 8., 5.));
    press(&mut demo, "D");
    assert_eq!(number(&demo, "cursor_x"), 6.);
    settle(&mut demo, 40);
    press(&mut demo, "D");
    press(&mut demo, "D");
    assert_eq!(number(&demo, "chunk_x"), 1.);
    assert_eq!(number(&demo, "cursor_x"), -7.);
    assert!(foundation_mesh_visible(&demo, "smelter-mk1", 8., 5.));
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        7.,
        5.
    ));
    factory_code(
        &mut demo,
        "chunks::unload_chunk_visuals(144);interiors::update(0.0);",
        1,
    );
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-concrete-floor",
        7.,
        5.
    ));
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        8.,
        5.
    ));
    factory_code(
        &mut demo,
        "chunks::load_chunk_visuals(0,0);interiors::update(0.0);if interiors::board(\"active\").len()!=2 {throw \"duplicate active regions\";}",
        1,
    );
    assert!(foundation_mesh_visible(
        &demo,
        "foundation-concrete-floor",
        7.,
        5.
    ));
    restore_factory_update(&mut demo);
    settle(&mut demo, 40);
    press(&mut demo, "S");
    press(&mut demo, "S");
    assert_eq!(
        number(&demo, "cursor_z"),
        6.,
        "the independently cached south door opens"
    );
    assert!(foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        8.,
        4.
    ));
    factory_code(
        &mut demo,
        r#"
        let pieces=architecture::page(get_object_list("cache_structures"),0,144);
        let roof=architecture::address(7,5,36.0,0);pieces[roof.slot]=0.0;
        grid::cache_put("cache_structures",144,grid::pack_numbers(pieces));
        set_scene_variable("cursor_z",4.0);interiors::update(0.0);
    "#,
        1,
    );
    assert!(
        foundation_mesh_visible(&demo, "foundation-concrete-roof", 8., 5.),
        "a change in the other chunk unseals the whole room"
    );
}

#[test]
fn foundations_showroom_starts_inside_a_powered_factory_using_the_normal_game() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../earth-factory/scenes/foundations-showroom.json");
    let scene = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut demo = SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap();
    tick(&mut demo, None);
    assert_eq!(controller_number(&demo, "bar"), 6.);
    assert!(foundation_mesh_visible(
        &demo,
        "foundation-concrete-floor",
        -5.,
        -3.
    ));
    assert!(!foundation_mesh_visible(
        &demo,
        "foundation-concrete-roof",
        -5.,
        -3.
    ));
    assert!(foundation_mesh_visible(&demo, "assembler-mk1", -5., -3.));
    assert!(controller_numbers(&demo, "power_live")[(4 * 15 + 2) as usize] > 0.);
    let before = demo.instance().document().objects.len();
    settle(&mut demo, 30);
    assert_eq!(
        before,
        demo.instance().document().objects.len(),
        "indoor view must reuse its models"
    );
}

fn renewables_fixture(setup: &str) -> SceneDemo {
    stellar_fixture(&format!(
        r#"
        import "factory-renewables" as renewables;
        set_object_variable("creative",true);set_object_variable("phase",7.0);
        set_scene_variable("demo_mode",true);
        let empty=grid::empty_numbers(225);
        set_scene_list("nodes",empty);grid::cache_put("chunk_nodes",144,grid::pack_numbers(empty));
        chunks::discover_chunk(1,0);grid::cache_put("chunk_nodes",145,grid::pack_numbers(empty));
        data::session_set(120,15.7);
        {setup}
    "#
    ))
}

fn hud_text(demo: &SceneDemo, id: &str) -> String {
    demo.instance()
        .ui_frame(&demo.app.world, bozzard_scene::Layer::ThreeD, [1080., 600.])
        .unwrap()
        .element(id)
        .unwrap()
        .text
        .clone()
}

#[test]
fn game_clock_runs_four_times_faster_preserves_its_rate_after_hours_and_pauses_at_title() {
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    assert_eq!(hud_text(&demo, "world-clock"), "08:00:00");
    for (elapsed, expected) in [(0., "08:00:04"), (20000., "06:13:24")] {
        factory_code(
            &mut demo,
            &format!("data::session_set(120,{elapsed:.1});"),
            1,
        );
        restore_factory_update(&mut demo);
        settle(&mut demo, 60);
        assert!((controller_numbers(&demo, "session")[120] - elapsed - 1.).abs() < 0.001);
        assert_eq!(hud_text(&demo, "world-clock"), expected);
    }
    factory_code(&mut demo, "set_object_variable(\"title_open\",true);", 1);
    restore_factory_update(&mut demo);
    let elapsed = controller_numbers(&demo, "session")[120];
    settle(&mut demo, 60);
    assert_eq!(controller_numbers(&demo, "session")[120], elapsed);
    factory_code(&mut demo, "world::begin_world(17);", 1);
    assert_eq!(controller_numbers(&demo, "session")[120], 0.);
    assert_eq!(hud_text(&demo, "world-clock"), "08:00:00");
}

#[test]
fn game_clock_daylight_solar_and_save_metadata_agree_at_dawn_dusk_and_midnight() {
    use bozzard_demo::factory::clock;
    let demo = renewables_fixture("");
    let time = demo.instance().script_module("factory-clock").unwrap();
    let weather = demo.instance().script_module("factory-renewables").unwrap();
    for (elapsed, label, day, sunny) in [
        (0., "08:00:00", 1, true),
        (3600., "12:00:00", 1, true),
        (8999.75, "17:59:59", 1, true),
        (9000., "18:00:00", 1, false),
        (14400., "00:00:00", 2, false),
        (19799.75, "05:59:59", 2, false),
        (19800., "06:00:00", 2, true),
        (21600., "08:00:00", 2, true),
        (216000., "08:00:00", 11, true),
    ] {
        assert_eq!(
            time.call_args::<_, String>("label", (elapsed,)).unwrap(),
            label
        );
        assert_eq!(clock::label(elapsed), label);
        assert_eq!(clock::day_number(elapsed), day);
        let seconds: f32 = time.call_args("day_seconds", (elapsed,)).unwrap();
        assert_eq!(clock::day_seconds(elapsed), seconds);
        for planet in [0i64, 1] {
            let expected = sunny && planet == 0;
            assert_eq!(
                time.call_args::<_, bool>("daytime", (elapsed, planet))
                    .unwrap(),
                expected
            );
            assert_eq!(
                weather
                    .call_args::<_, bool>("solar_active", (elapsed, planet))
                    .unwrap(),
                expected
            );
            assert_eq!(clock::daytime(elapsed, planet == 1), expected);
            let daylight: f32 = time.call_args("daylight", (elapsed, planet)).unwrap();
            if planet == 1 {
                assert_eq!(daylight, -1.);
            } else {
                let expected = ((seconds - 21600.) / 86400. * std::f32::consts::TAU).sin();
                assert!((daylight - expected).abs() < 0.00001);
            }
        }
    }
}

// Populate in separate hooks, as individual placements happen across frames in
// the player. The measured hook still uses the normal controller and its budget.
fn dense_factory_fixture(kind: i32) -> SceneDemo {
    let mut demo = renewables_fixture("");
    for (cx, cz) in [(1, 0), (0, 1)] {
        factory_code(
            &mut demo,
            &format!(
                r#"
                world::enter_chunk({cx},{cz});
                let builds=grid::empty_numbers(225);let nodes=grid::empty_numbers(225);
                let recipes=grid::empty_numbers(225);let inputs=grid::empty_numbers(225);let amounts=grid::empty_numbers(225);
                let visuals=grid::empty_text(225);let cells=[];let graph=power::power_graph();
                for group in 0..40 {{
                    let base=group*5;let pole=power::power_id(base+1);
                    let peers=[9,0];
                    for offset in 0..5 {{
                        let cell=base+offset;let kind=if offset==0 {{6}} else if offset==1 {{9}} else {{{kind}}};
                        builds[cell]=kind.to_float();cells.push(cell.to_float());
                        visuals[cell]=grid::spawn_build(kind.to_float(),grid::cell_x(cell),grid::cell_z(cell),0);
                        if offset==1 {{continue;}}
                        let id=power::power_id(cell);graph[id.to_string()]=[kind,0,pole];peers.push(id);
                        if kind==6 {{nodes[cell]=4.0;}}
                        else {{recipes[cell]=data::recipe_choices(kind.to_float())[0];inputs[cell]=1.0;amounts[cell]=100.0;}}
                    }}
                    graph[pole.to_string()]=peers;
                }}
                set_scene_list("builds",builds);set_scene_list("nodes",nodes);
                set_scene_list("build_visuals",visuals);set_scene_list("machine_cells",cells);
                set_scene_list("input_items",inputs);set_scene_list("input_amounts",amounts);set_object_list("recipes",recipes);
                if {kind}>=12 {{
                    for page in 0..4 {{
                        let kinds=grid::empty_numbers(900);let amounts=grid::empty_numbers(900);
                        for cell in 0..200 {{
                            if builds[cell]!={kind}.0 {{continue;}}
                            let cost=data::recipe_cost(recipes[cell].to_int());
                            for slot in 0..4 {{
                                let input=page*4+slot;
                                if input>=cost.len()/2 {{continue;}}
                                kinds[cell*4+slot]=cost[input*2].to_float();amounts[cell*4+slot]=8.0;
                            }}
                        }}
                        set_scene_list("storage_kinds_"+page.to_string(),kinds);set_scene_list("storage_amounts_"+page.to_string(),amounts);
                    }}
                }}
                power::write_power(graph);set_object_variable("power_dirty",true);
                "#
            ),
            1,
        );
    }
    demo
}

#[test]
fn four_hundred_powered_machines_fit_the_normal_controller_operation_budget() {
    let mut demo = dense_factory_fixture(3);
    factory_code(&mut demo, "power::update_power();", 1);
    assert_eq!(number(&demo, "power_demand"), 480.);
    // Match the previous nearest-light scan, including stable tie order and the
    // shared status/heat slots. Sorting must not change which surfaces are lit.
    let mut lamps = Vec::new();
    for (chunk, cx, cz) in [(145i64, 1i64, 0i64), (161, 0, 1)] {
        for cell in 0..200i64 {
            let id = chunk * 225 + cell;
            let x = cx * 15 + cell % 15 - 7;
            let z = cz * 15 + cell / 15 - 7;
            let distance = x * x + (z - 15) * (z - 15);
            let kind = if cell % 5 == 0 {
                6
            } else if cell % 5 == 1 {
                9
            } else {
                3
            };
            let slot = if kind == 9 {
                String::new()
            } else {
                format!("{id},{kind},status")
            };
            lamps.push((distance, lamps.len(), slot));
            if kind == 3 {
                lamps.push((distance, lamps.len(), format!("{id},3,heat")));
            }
        }
    }
    lamps.sort_by_key(|(distance, index, _)| (*distance, *index));
    let expected: Vec<_> = lamps
        .into_iter()
        .take(32)
        .map(|(_, _, slot)| Value::Text(slot))
        .collect();
    let slots = demo
        .app
        .world
        .resource::<BlueprintRuntime>()
        .unwrap()
        .object_blackboard("factory-transports")
        .unwrap()["machine_light_slots"]
        .values();
    assert_eq!(slots, expected);
    factory_code(&mut demo, "simulation::factory_step();", 1);
    factory_code(
        &mut demo,
        "set_object_variable(\"power_dirty\",true);set_scene_variable(\"clock\",0.32);normal_update(me,dt);",
        1,
    );
    restore_factory_update(&mut demo);
    settle(&mut demo, 40);
    assert!(number(&demo, "ticks") > 1.);
}

#[test]
fn four_hundred_expansion_machines_fit_the_normal_controller_operation_budget() {
    let mut demo = dense_factory_fixture(21);
    // Existing saves can have literal-heavy pages. They must work before any
    // production beat has had a chance to rewrite them with the new encoder.
    factory_code(
        &mut demo,
        r#"
        for name in ["builds","facings","recipes","items","item_amounts","input_items","input_amounts","progress","assembler_iron","assembler_copper","split_state",
            "storage_kinds_0","storage_amounts_0","storage_kinds_1","storage_amounts_1","storage_kinds_2","storage_amounts_2","storage_kinds_3","storage_amounts_3"] {
            let count=if name.starts_with("storage_") {900}else{225};
            let values=grid::unpack_numbers(get_object_list_item("cache_"+name,145),count);
            let text="";
            for value in values {if text!="" {text+=",";}text+=value.to_int().to_string();}
            grid::cache_put("cache_"+name,145,text);
        }
        "#,
        1,
    );
    factory_code(
        &mut demo,
        "set_object_variable(\"power_dirty\",true);set_scene_variable(\"clock\",0.32);normal_update(me,dt);",
        5,
    );
    // Place the next machine through actual input on a producing frame, while
    // all existing machines refresh their power effects and finish their batch.
    factory_code(
        &mut demo,
        "set_scene_variable(\"cursor_x\",-2.0);set_scene_variable(\"cursor_z\",6.0);set_scene_variable(\"selected\",3.0);set_scene_variable(\"clock\",0.32);",
        1,
    );
    restore_factory_update(&mut demo);
    tick(&mut demo, Some("Space"));
    assert_eq!(numbers(&demo, "builds")[200], 3.);
    settle(&mut demo, 4);
    assert_eq!(number(&demo, "ticks"), 6.);
    let (world, _) = coop_world(&demo);
    for chunk in [145, 161] {
        let inputs = coop_page(&world, "cache_storage_amounts_0", 0, chunk, 900);
        let outputs = coop_page(&world, "cache_storage_amounts_2", 0, chunk, 900);
        let kinds = coop_page(&world, "cache_storage_kinds_2", 0, chunk, 900);
        for cell in 0..200 {
            if cell % 5 >= 2 {
                assert_eq!(inputs[cell * 4], 5.);
                assert_eq!(outputs[cell * 4], 3.);
                assert_eq!(kinds[cell * 4], 14.);
            }
        }
    }
}

#[test]
fn renewables_generate_daylight_solar_and_gust_driven_wind_through_real_cables() {
    let mut demo = renewables_fixture(
        r#"
        for row in [[2,3,40],[4,3,41],[2,5,42],[3,4,9],[3,5,3]] {
            set_scene_variable("cursor_x",row[0].to_float());set_scene_variable("cursor_z",row[1].to_float());
            set_scene_variable("selected",row[2].to_float());set_scene_variable("direction",0.0);building::place_selected();
        }
        let pole=power::power_id(grid::index(3,4));
        for at in [[2,3],[4,3],[2,5],[3,5]] {power::connect_power(pole,power::power_id(grid::index(at[0],at[1])));}
        power::update_power();
    "#,
    );
    let consumer = ((5 + 7) * 15 + 3 + 7) as usize;
    assert_eq!(number(&demo, "power_supply"), 20.);
    assert_eq!(controller_numbers(&demo, "power_live")[consumer], 1.);
    factory_code(
        &mut demo,
        "data::session_set(120,9001.0);power::update_power();",
        1,
    );
    assert_eq!(
        number(&demo, "power_supply"),
        8.,
        "calm night leaves only the isolated landing pod"
    );
    assert_eq!(controller_numbers(&demo, "power_live")[consumer], 0.);
    factory_code(
        &mut demo,
        "data::session_set(120,10032.0);power::update_power();",
        1,
    );
    assert_eq!(number(&demo, "power_supply"), 14.);
    assert_eq!(
        controller_numbers(&demo, "power_live")[consumer],
        1.,
        "a night gust powers the circuit"
    );
    factory_code(
        &mut demo,
        r#"
        set_scene_variable("cursor_x",2.0);set_scene_variable("cursor_z",5.0);
        set_scene_variable("selected",42.0);building::remove_selected();power::update_power();
    "#,
        1,
    );
    assert_eq!(
        controller_numbers(&demo, "power_live")[consumer],
        0.,
        "solar-only circuit stops at night"
    );
    factory_code(
        &mut demo,
        "data::session_set(120,15.7);power::update_power();",
        1,
    );
    assert_eq!(controller_numbers(&demo, "power_live")[consumer], 1.);
    coop_world(&demo); // Renewable machines and terminals pass persistent-state validation.
}

#[test]
fn renewables_array_reserves_two_spots_rotates_without_overlap_and_removes_from_either() {
    let mut demo = renewables_fixture(
        r#"
        set_scene_variable("cursor_x",4.0);set_scene_variable("cursor_z",3.0);
        set_scene_variable("selected",41.0);set_scene_variable("direction",0.0);building::place_selected();
        set_scene_variable("cursor_x",5.0);set_scene_variable("selected",42.0);building::place_selected();
        if get_scene_list_item("builds",grid::index(5,3))!=0.0 {throw "array second spot was overwritten";}
        set_scene_variable("cursor_x",4.0);set_scene_variable("cursor_z",4.0);
        set_scene_variable("selected",4.0);building::place_selected();
        set_scene_variable("cursor_z",3.0);building::rotate_selected();
        if get_scene_list_item("facings",grid::index(4,3))!=0.0 {throw "array rotated into storage";}
        set_scene_variable("cursor_z",4.0);building::remove_selected();
        set_scene_variable("cursor_z",3.0);building::rotate_selected();
    "#,
    );
    assert_eq!(numbers(&demo, "facings")[(3 + 7) * 15 + 4 + 7], 1.);
    factory_code(
        &mut demo,
        r#"
        set_scene_variable("cursor_x",5.0);set_scene_variable("cursor_z",3.0);set_scene_variable("selected",42.0);building::place_selected();
        set_scene_variable("cursor_x",4.0);set_scene_variable("cursor_z",4.0);building::remove_selected();
    "#,
        1,
    );
    assert_eq!(numbers(&demo, "builds")[(3 + 7) * 15 + 4 + 7], 0.);
    assert_eq!(
        numbers(&demo, "builds")[(3 + 7) * 15 + 5 + 7],
        42.,
        "rotation released the original second spot"
    );
    factory_code(
        &mut demo,
        r#"
        set_scene_variable("cursor_x",5.0);set_scene_variable("cursor_z",3.0);building::remove_selected();
        set_scene_variable("cursor_x",4.0);set_scene_variable("cursor_z",3.0);set_scene_variable("selected",41.0);set_scene_variable("direction",0.0);
        let nodes=get_scene_list("nodes");nodes[grid::index(5,3)]=1.0;set_scene_list("nodes",nodes);building::place_selected();
    "#,
        1,
    );
    assert_eq!(
        numbers(&demo, "builds")[(3 + 7) * 15 + 4 + 7],
        0.,
        "blocked placement is atomic"
    );
}

#[test]
fn renewables_array_crosses_region_seams_and_survives_saves_and_remote_demolition() {
    use bozzard_demo::factory::state::State;
    let mut demo = renewables_fixture(
        r#"
        set_scene_variable("cursor_x",7.0);set_scene_variable("cursor_z",2.0);
        set_scene_variable("selected",41.0);set_scene_variable("direction",0.0);building::place_selected();
        world::archive_chunk();world::enter_chunk(1,0);
        set_scene_variable("cursor_x",-7.0);set_scene_variable("cursor_z",2.0);set_scene_variable("selected",42.0);building::place_selected();
        if get_scene_list_item("builds",grid::index(-7,2))!=0.0 {throw "seam reservation missing";}
    "#,
    );
    let state =
        State::capture_live(demo.app.world.resource::<BlueprintRuntime>().unwrap()).unwrap();
    let restored: State = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    restored.validate().unwrap();
    let mut damaged = restored.clone();
    damaged
        .controller
        .get_mut("cache_builds")
        .unwrap()
        .values_mut()[145] = Value::Text("0:135,42,0:89".into());
    assert!(
        damaged.validate().is_err(),
        "loading rejects a machine on the reserved second spot"
    );
    factory_code(
        &mut demo,
        "building::remove_selected();world::enter_chunk(0,0);",
        1,
    );
    assert_eq!(numbers(&demo, "builds")[(2 + 7) * 15 + 7 + 7], 0.);
    coop_world(&demo);
}

#[test]
fn renewables_coop_owns_array_footprints_and_rejects_invalid_edits_without_payment() {
    use bozzard_demo::factory::{
        authority::Executor, replication::requests::Action, shared::Position,
    };
    use std::time::Duration;
    let mut demo = renewables_fixture("");
    factory_code(
        &mut demo,
        r#"
        import "factory-backpack" as backpack;
        set_scene_variable("demo_mode",false);set_object_variable("creative",false);
        for kind in [11.0,12.0,14.0,17.0] {backpack::give(kind,50.0);}
    "#,
        1,
    );
    let (mut world, mut player) = coop_world(&demo);
    let mut executor = Executor::new(demo.instance()).unwrap();
    player.position = Position {
        planet: 0,
        x: 7,
        z: 2,
    };
    let outcome = executor
        .apply(
            &mut world,
            10,
            &mut player,
            &Action::Place {
                kind: 41,
                direction: 0,
            },
            Duration::ZERO,
        )
        .unwrap();
    assert!(outcome.accepted, "{}", outcome.message);
    player.position.x = 8;
    let before = world.clone();
    let inventory = player.backpack;
    let rejected = executor
        .apply(
            &mut world,
            20,
            &mut player,
            &Action::Place {
                kind: 42,
                direction: 0,
            },
            Duration::ZERO,
        )
        .unwrap();
    assert!(!rejected.accepted);
    assert_eq!(player.backpack, inventory);
    assert_eq!(world, before);
    assert!(
        executor
            .apply(&mut world, 20, &mut player, &Action::Remove, Duration::ZERO)
            .unwrap()
            .accepted
    );
    player.position.x = 7;
    assert!(
        executor
            .apply(
                &mut world,
                10,
                &mut player,
                &Action::Place {
                    kind: 42,
                    direction: 0
                },
                Duration::ZERO
            )
            .unwrap()
            .accepted
    );
    world.validate().unwrap();
}

#[test]
fn renewables_earth_solar_circuits_follow_dawn_and_dusk_while_on_the_other_planet() {
    let mut demo = renewables_fixture(
        r#"
        for row in [[2,3,41],[2,5,9],[3,5,3]] {
            set_scene_variable("cursor_x",row[0].to_float());set_scene_variable("cursor_z",row[1].to_float());
            set_scene_variable("selected",row[2].to_float());set_scene_variable("direction",0.0);building::place_selected();
        }
        let pole=power::power_id(grid::index(2,5));
        power::connect_power(pole,power::power_id(grid::index(2,3)));
        power::connect_power(pole,power::power_id(grid::index(3,5)));power::update_power();
        set_scene_variable("demo_mode",false);set_scene_variable("cursor_x",0.0);set_scene_variable("cursor_z",1.0);
        world::travel_to_other_planet();
    "#,
    );
    assert_eq!(controller_numbers(&demo, "session")[7], 1.);
    for (time, expected) in [(9000., 0.), (19800., 1.)] {
        factory_code(
            &mut demo,
            &format!(
                r#"
            data::session_set(120,{time:.1});simulation::factory_step();
            let graph=power::power_graph("power_other");
            set_scene_variable("power_demand",graph[(144*225+grid::index(3,5)).to_string()][1].to_float());
        "#
            ),
            1,
        );
        assert_eq!(
            number(&demo, "power_demand"),
            expected,
            "departed Earth circuit at time {time}"
        );
    }
    coop_world(&demo);
}

fn gust_fixture() -> SceneDemo {
    let mut demo = renewables_fixture(
        r#"
        for row in [[2,3,42],[2,4,9],[3,4,3]] {
            set_scene_variable("cursor_x",row[0].to_float());set_scene_variable("cursor_z",row[1].to_float());
            set_scene_variable("selected",row[2].to_float());building::place_selected();
        }
        let pole=power::power_id(grid::index(2,4));
        power::connect_power(pole,power::power_id(grid::index(2,3)));
        power::connect_power(pole,power::power_id(grid::index(3,4)));
        let input=get_scene_list("input_items");input[grid::index(3,4)]=1.0;set_scene_list("input_items",input);
        let amounts=get_scene_list("input_amounts");amounts[grid::index(3,4)]=4.0;set_scene_list("input_amounts",amounts);
        data::session_set(120,0.0);power::update_power();
    "#,
    );
    factory_code(&mut demo, "wind::update();", 2); // Start the rotor's lifetime hook.
    demo
}

fn gust_times(demo: &SceneDemo, cycle: i64) -> (f32, f32) {
    let weather = demo.instance().script_module("factory-renewables").unwrap();
    let length: f32 = weather.call("wind_cycle_seconds", vec![]).unwrap();
    let window: serde_json::Value = weather
        .call_args("wind_window", (number(demo, "seed") as i64, 0i64, cycle))
        .unwrap();
    let start = cycle as f32 * length + window["start"].as_f64().unwrap() as f32;
    (
        start + 4.,
        start + window["duration"].as_f64().unwrap() as f32 + 1.,
    )
}

fn gust_rotor(demo: &SceneDemo) -> bozzard_scene::Transform {
    let id = &demo
        .instance()
        .document()
        .objects
        .iter()
        .find(|o| o.name == "Wind turbine rotor")
        .expect("resident rotor")
        .id;
    *demo
        .app
        .world
        .get::<bozzard_scene::Transform>(demo.instance().entity(id).unwrap())
        .unwrap()
}

fn gust_rotor_count(demo: &SceneDemo) -> f32 {
    let BlackboardValue::Scalar(Value::Number(count)) = demo
        .app
        .world
        .resource::<BlueprintRuntime>()
        .unwrap()
        .object_blackboard("renewable-view")
        .unwrap()["rotor_count"]
    else {
        panic!("rotor count")
    };
    count
}

#[test]
fn wind_gusts_have_calm_intervals_smooth_ramps_and_deterministic_angles() {
    let demo = renewables_fixture("");
    let weather = demo.instance().script_module("factory-renewables").unwrap();
    let length: f32 = weather.call("wind_cycle_seconds", vec![]).unwrap();
    assert_eq!(length, 900.);
    // These worlds exercise both real-time duration limits, independent of the
    // four-times-faster displayed clock.
    for (seed, duration) in [(1692i64, 20.), (254i64, 600.)] {
        let window: serde_json::Value = weather
            .call_args("wind_window", (seed, 0i64, 0i64))
            .unwrap();
        assert_eq!(window["duration"].as_f64().unwrap() as f32, duration);
        let start = window["start"].as_f64().unwrap() as f32;
        for (offset, expected) in [
            (duration / 4. + 1., true),
            (duration - 0.1, true),
            (duration, false),
        ] {
            assert_eq!(
                weather
                    .call_args::<_, bool>("wind_active", (seed, 0i64, start + offset))
                    .unwrap(),
                expected
            );
        }
    }
    for seed in [1i64, 4, 2_000_000_000] {
        for planet in [0i64, 1] {
            for cycle in [0i64, 1, 6, 7, 55] {
                let window: serde_json::Value = weather
                    .call_args("wind_window", (seed, planet, cycle))
                    .unwrap();
                let start = cycle as f32 * length + window["start"].as_f64().unwrap() as f32;
                let duration = window["duration"].as_f64().unwrap() as f32;
                assert!((20.0..=600.0).contains(&duration));
                let motion = |time: f32| -> (f32, f32) {
                    let result: serde_json::Value = weather
                        .call_args("wind_motion", (seed, planet, time))
                        .unwrap();
                    (
                        result["angle"].as_f64().unwrap() as f32,
                        result["strength"].as_f64().unwrap() as f32,
                    )
                };
                for (time, active) in [
                    (start, false),
                    (start + 0.1, true),
                    (start + 4., true),
                    (start + duration, false),
                    (cycle as f32 * length + length - 1., false),
                ] {
                    let running: bool = weather
                        .call_args("wind_active", (seed, planet, time))
                        .unwrap();
                    assert_eq!(running, active);
                    assert_eq!(
                        motion(time).1 > 0.,
                        active,
                        "power and visible motion disagree"
                    );
                    assert_eq!(
                        motion(time),
                        motion(time),
                        "weather must not advance on reads"
                    );
                }
                assert_eq!(motion(start).1, 0.);
                assert!((motion(start + 1.).1 - 0.5).abs() < 0.001);
                assert_eq!(motion(start + 4.).1, 1.);
                assert!((motion(start + duration - 1.).1 - 0.5).abs() < 0.001);
                assert_eq!(
                    motion(start + duration).0,
                    motion(cycle as f32 * length + length - 1.).0
                );
                let slow = (motion(start + 0.2).0 - motion(start).0).rem_euclid(360.);
                let fast = (motion(start + 4.2).0 - motion(start + 4.).0).rem_euclid(360.);
                assert!(slow < fast * 0.2, "rotor must accelerate into a gust");
                assert!((motion(start - 0.1).0 - motion(start).0).abs() < 0.001);
            }
        }
    }
}

#[test]
fn wind_gusts_spin_the_child_rotor_and_power_production_only_while_spinning() {
    let mut demo = gust_fixture();
    let (gust, calm) = gust_times(&demo, 0);
    let (next_gust, _) = gust_times(&demo, 1);
    let consumer = (11 * 15 + 10) as usize; // World tile (3,4).
    assert_eq!(gust_rotor_count(&demo), 1.);
    let stopped = gust_rotor(&demo);
    factory_code(
        &mut demo,
        "data::session_set(120,10.0);wind::update();simulation::factory_step();",
        1,
    );
    assert_eq!(gust_rotor(&demo), stopped);
    assert_eq!(controller_numbers(&demo, "power_live")[consumer], 0.);
    assert_eq!(numbers(&demo, "progress")[consumer], 0.);
    factory_code(
        &mut demo,
        &format!("data::session_set(120,{gust:.1});wind::update();simulation::factory_step();"),
        1,
    );
    let moving = gust_rotor(&demo);
    assert_ne!(moving.rotation_degrees[2], stopped.rotation_degrees[2]);
    assert_eq!(
        moving.translation,
        [0., 1.7, 0.25],
        "rotate around the axle"
    );
    assert_eq!(moving.rotation_degrees[..2], [0., 0.]);
    assert_eq!(controller_numbers(&demo, "power_live")[consumer], 1.);
    assert_eq!(numbers(&demo, "progress")[consumer], 1.);
    let membership = demo.instance().document().objects.len();
    factory_code(
        &mut demo,
        &format!("data::session_set(120,{:.1});wind::update();", gust + 0.1),
        1,
    );
    assert_ne!(
        gust_rotor(&demo).rotation_degrees[2],
        moving.rotation_degrees[2]
    );
    assert_eq!(
        membership,
        demo.instance().document().objects.len(),
        "gust effects must reuse their objects"
    );
    factory_code(
        &mut demo,
        &format!("data::session_set(120,{calm:.1});wind::update();power::update_power();"),
        1,
    );
    let calm = gust_rotor(&demo);
    assert_eq!(controller_numbers(&demo, "power_live")[consumer], 0.);
    factory_code(&mut demo, "wind::update();power::update_power();", 3);
    assert_eq!(gust_rotor(&demo), calm);
    assert_eq!(
        demo.app
            .world
            .resource::<bozzard_scene::ScriptRuntime>()
            .unwrap()
            .stats
            .commands,
        0,
        "calm weather must not enqueue transforms or circuit writes"
    );
    factory_code(
        &mut demo,
        &format!("data::session_set(120,{next_gust:.1});wind::update();power::update_power();"),
        1,
    );
    assert_eq!(
        controller_numbers(&demo, "power_live")[consumer],
        1.,
        "the next gust restarts power"
    );
    factory_code(
        &mut demo,
        r#"
        set_scene_variable("cursor_x",2.0);set_scene_variable("cursor_z",3.0);
        building::remove_selected();wind::update();power::update_power();
    "#,
        1,
    );
    assert_eq!(
        gust_rotor_count(&demo),
        0.,
        "demolition unregisters the rotor"
    );
    factory_code(&mut demo, "wind::update();", 1);
}

#[test]
fn wind_gusts_restore_from_saves_and_stream_rotors_without_stale_handles() {
    let mut demo = gust_fixture();
    let (gust, calm) = gust_times(&demo, 0);
    let (next_gust, next_calm) = gust_times(&demo, 1);
    factory_code(
        &mut demo,
        &format!(
            "data::session_set(120,{gust:.1});wind::update();power::update_power();world::archive_chunk();"
        ),
        1,
    );
    let angle = gust_rotor(&demo).rotation_degrees[2];
    factory_code(
        &mut demo,
        "chunks::unload_chunk_visuals(144);wind::update();",
        1,
    );
    assert_eq!(gust_rotor_count(&demo), 0.);
    factory_code(
        &mut demo,
        "chunks::load_chunk_visuals(0,0);world::restore_chunk(0,0);wind::update();",
        2,
    );
    assert_eq!(gust_rotor_count(&demo), 1.);
    assert_eq!(gust_rotor(&demo).rotation_degrees[2], angle);
    let directory = save_directory(&mut demo);
    factory_code(&mut demo, "persistence::prepare_save(1);", 1);
    factory_code(&mut demo, "persistence::update(0.0);", 1);
    finish_save_io(&mut demo);
    factory_code(
        &mut demo,
        "world::begin_world(17);data::session_set(117,1.0);data::session_set(116,2.0);",
        1,
    );
    factory_code(&mut demo, "persistence::update(0.0);", 1);
    finish_save_io(&mut demo);
    factory_code(&mut demo, "wind::update();power::update_power();", 2);
    assert_eq!(number(&demo, "seed"), 4.);
    assert_eq!(controller_numbers(&demo, "session")[120], gust);
    assert_eq!(
        hud_text(&demo, "world-clock"),
        bozzard_demo::factory::clock::label(gust)
    );
    assert_eq!(gust_rotor_count(&demo), 1.);
    assert_eq!(
        gust_rotor(&demo).rotation_degrees[2],
        angle,
        "save must not reroll or reset the gust"
    );
    assert_eq!(controller_numbers(&demo, "power_live")[11 * 15 + 10], 1.);
    // Earth keeps observing its own weather while the player visits Stella-Z2.
    factory_code(
        &mut demo,
        r#"
        set_scene_variable("demo_mode",false);set_scene_variable("cursor_x",0.0);set_scene_variable("cursor_z",1.0);
        world::travel_to_other_planet();
    "#,
        1,
    );
    assert_eq!(gust_rotor_count(&demo), 0.);
    for (time, expected) in [(calm, 0.), (next_gust, 1.), (next_calm, 0.)] {
        factory_code(
            &mut demo,
            &format!(
                r#"
            data::session_set(120,{time:.1});simulation::factory_step();wind::update();
            let graph=power::power_graph("power_other");
            set_scene_variable("power_demand",graph[(144*225+grid::index(3,4)).to_string()][1].to_float());
        "#
            ),
            1,
        );
        assert_eq!(
            number(&demo, "power_demand"),
            expected,
            "off-world gust at {time}"
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn wind_gusts_native_multiplayer_wiring_uses_the_same_seeded_world_clock() {
    use bozzard_demo::factory::{
        authority::Executor, replication::requests::Action, shared::Position,
    };
    use std::time::Duration;
    let mut demo = gust_fixture();
    let (gust, calm) = gust_times(&demo, 0);
    let (next_gust, _) = gust_times(&demo, 1);
    for (time, expected) in [(10., 0), (gust, 1), (calm, 0), (next_gust, 1)] {
        factory_code(
            &mut demo,
            &format!(
                r#"
            data::session_set(120,{time:.1});
            power::disconnect_power(power::power_id(grid::index(2,3)),power::power_id(grid::index(2,4)));
            power::update_power();wind::update();
        "#
            ),
            1,
        );
        let (mut world, mut player) = coop_world(&demo);
        let mut executor = Executor::new(demo.instance()).unwrap();
        let turbine = Position {
            planet: 0,
            x: 2,
            z: 3,
        };
        let pole = Position {
            planet: 0,
            x: 2,
            z: 4,
        };
        player.position = turbine;
        let result = executor
            .apply(
                &mut world,
                20,
                &mut player,
                &Action::Wire {
                    from: turbine,
                    to: pole,
                },
                Duration::ZERO,
            )
            .unwrap();
        assert!(result.accepted, "{}", result.message);
        let id = 144 * 225 + 11 * 15 + 10;
        let Value::Text(page) = &world.state().controller["power_data"].values()[id / 75] else {
            panic!("power page")
        };
        let powered = page
            .split('|')
            .nth(id % 75)
            .unwrap()
            .split(',')
            .nth(1)
            .unwrap()
            .parse::<i32>()
            .unwrap();
        assert_eq!(powered, expected, "native wiring at world time {time}");
        let restored: bozzard_demo::factory::shared::World =
            serde_json::from_slice(&serde_json::to_vec(&world).unwrap()).unwrap();
        assert_eq!(restored, world);
    }
}

#[test]
fn wind_gusts_coop_guests_follow_weather_and_reconcile_remote_rotor_lifetimes() {
    use bozzard_demo::factory::{host::HostRuntime, replication::requests::Action};
    let mut host = gust_fixture();
    let (gust, calm) = gust_times(&host, 0);
    let (next_gust, _) = gust_times(&host, 1);
    HostRuntime::start(
        &mut host.app.world,
        10,
        1,
        [(10, "Host".into()), (20, "Guest".into())].into(),
    )
    .unwrap();
    factory_code(
        &mut host,
        "host_view::synchronize();data::session_set(120,10.0);power::update_power();wind::update();",
        1,
    );
    coop_host_tick(&mut host);
    let mut guest = coop_guest(&host);
    assert_eq!(gust_rotor_count(&guest), 1.);
    for (time, expected) in [(gust, 1.), (calm, 0.), (next_gust, 1.)] {
        factory_code(
            &mut host,
            &format!(
                "host_view::synchronize();data::session_set(120,{time:.1});power::update_power();wind::update();"
            ),
            1,
        );
        coop_host_tick(&mut host);
        coop_guest_snapshot(&host, &mut guest);
        settle(&mut guest, 3);
        assert_eq!(controller_numbers(&guest, "session")[120], time);
        assert_eq!(
            hud_text(&guest, "world-clock"),
            bozzard_demo::factory::clock::label(time)
        );
        assert_eq!(
            controller_numbers(&guest, "power_live")[11 * 15 + 10],
            expected
        );
        let before = gust_rotor(&guest);
        settle(&mut guest, 2);
        if expected > 0. {
            assert_ne!(
                gust_rotor(&guest).rotation_degrees[2],
                before.rotation_degrees[2]
            );
        } else {
            assert_eq!(gust_rotor(&guest), before);
        }
    }
    assert!(coop_host_action(
        &mut host,
        20,
        Action::Place {
            kind: 42,
            direction: 0
        }
    ));
    coop_host_tick(&mut host); // Present the accepted edit on the host.
    coop_host_tick(&mut host); // Start the newly spawned rotor's lifetime hook.
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 3);
    assert_eq!(gust_rotor_count(&host), 2.);
    assert_eq!(gust_rotor_count(&guest), 2.);
    assert!(coop_host_action(&mut host, 20, Action::Remove));
    coop_host_tick(&mut host);
    coop_guest_snapshot(&host, &mut guest);
    settle(&mut guest, 3);
    assert_eq!(gust_rotor_count(&host), 1.);
    assert_eq!(gust_rotor_count(&guest), 1.);
    settle(&mut guest, 3);
}
