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
        "Triangles 4567   Simulating 1"
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
    assert!(!frame.element("menu-save").unwrap().enabled);
    assert!(!frame.element("menu-load").unwrap().enabled);
    assert!(!demo.app.is_paused());
    assert!(bozzard_scene::game_flow::simulation_running(
        &demo.app.world
    ));
    for action in ["menu-save", "menu-load"] {
        assert!(
            !demo
                .ui_input(
                    bozzard_scene::Layer::ThreeD,
                    [1080., 600.],
                    Input::ActivateObject(action.into())
                )
                .unwrap()
        );
    }
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
        assert!(
            (0..225).all(|cell| {
                let x = cell as i32 % 15 - 7;
                let z = cell as i32 / 15 - 7;
                x * x + z * z > 2 || nodes[cell] == 0.
            }),
            "fixture needs an empty region centre"
        );
        settle(&mut demo, 30);
        assert!(tooltip(&demo).is_none(), "empty ground must have no label");
        let iron = nodes.iter().position(|&kind| kind == 1.).unwrap();
        move_cursor(&mut demo, iron as i32 % 15 - 7, iron as i32 / 15 - 7);
        settle(&mut demo, 30);
        assert_eq!(tooltip(&demo).as_deref(), Some("Iron deposit"));
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
        camera.abs_diff_eq(original, 0.0001),
        "four turns return without drift"
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
        let inventory = empty_numbers(32);
        inventory[0] = 20.0; inventory[1] = 7.0;
        inventory[2] = 11.0; inventory[3] = 98.0;
        inventory[4] = 11.0; inventory[5] = 9.0;
        inventory[6] = 12.0; inventory[7] = 1.0;
        storage_write(cell, inventory);
        let counts = get_scene_list("counts");
        counts[20] = 7.0; counts[11] = 107.0; counts[12] = 1.0;
        set_scene_list("counts", counts);
        set_scene_variable("cursor_x", cell_x(cell).to_float());
        set_scene_variable("cursor_z", cell_z(cell).to_float());
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
        let inventory = empty_numbers(32);
        for i in 0..16 { inventory[i * 2] = 20.0; inventory[i * 2 + 1] = 100.0; }
        storage_write(cell, inventory);
        let counts = get_scene_list("counts"); counts[20] = 1600.0;
        set_scene_list("counts", counts);
        set_scene_variable("cursor_x", cell_x(cell).to_float());
        set_scene_variable("cursor_z", cell_z(cell).to_float());
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
    assert_eq!(controller_number(&demo, "bar"), 2.);
    assert_eq!(
        number(&demo, "selected"),
        4.,
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
        set_scene_variable("cursor_x", cell_x(cell).to_float());
        set_scene_variable("cursor_z", cell_z(cell).to_float());
        rotate_selected();
        remove_selected();
        set_scene_variable("selected", 5.0); place_selected();
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
fn journal_pages_block_world_input_and_tier_one_can_be_completed_from_scratch() {
    let mut demo = factory_with_mode(Some(4.), false);
    tick(&mut demo, None);
    assert_eq!(controller_number(&demo, "phase"), 0.);
    for (kind, amount) in [(1, 36), (2, 24)] {
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
    for _ in 0..24 {
        click_widget(&mut demo, "journal-craft-iron");
    }
    for _ in 0..16 {
        click_widget(&mut demo, "journal-craft-copper");
    }
    assert_eq!(controller_numbers(&demo, "stock")[11], 24.);
    assert_eq!(controller_numbers(&demo, "stock")[12], 16.);
    click_widget(&mut demo, "journal-tab-1");
    for phase in 2..=4 {
        click_widget(&mut demo, "journal-deliver");
        assert_eq!(controller_number(&demo, "phase"), phase as f32);
    }
    assert_eq!(controller_numbers(&demo, "stock")[11], 0.);
    assert_eq!(controller_numbers(&demo, "stock")[12], 0.);
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
            .contains("0 / 4")
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
        let inventory = empty_numbers(32); inventory[0] = 20.0; inventory[1] = 37.0;
        storage_write(cell, inventory);
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
    for kind in 1..=7 {
        assert!(nodes.contains(&(kind as f32)));
    }
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
fn chunk_camera_glides_retargets_and_finishes_while_orbiting() {
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
    move_cursor(&mut demo, 7, 0);
    assert_eq!(position(&demo), [0., 0., 0.]);
    press(&mut demo, "D");
    let first = position(&demo)[0];
    assert!(
        first > 0. && first < 0.1,
        "pan eases in instead of snapping"
    );
    let mut previous = first;
    for _ in 0..10 {
        tick(&mut demo, None);
        let x = position(&demo)[0];
        assert!(x > previous && x < 15.);
        previous = x;
    }
    press(&mut demo, "A");
    assert!(
        position(&demo)[0] > previous - 0.1 && position(&demo)[0] < previous,
        "reversing at the seam starts from the current view"
    );
    settle(&mut demo, 40);
    assert_eq!(position(&demo), [0., 0., 0.]);
    press(&mut demo, "D");
    tick_keys(&mut demo, &["Ctrl", "R"]);
    settle(&mut demo, 40);
    assert_eq!(position(&demo), [15., 0., 0.]);
    assert_eq!(number(&demo, "camera_heading"), 90.);
    assert!(
        number(&demo, "ticks") > 0.,
        "factory clock keeps advancing during pans"
    );
    tick(&mut demo, Some("N"));
    assert_eq!(position(&demo), [0., 0., 0.]);
    settle(&mut demo, 40);
    assert_eq!(
        position(&demo),
        [0., 0., 0.],
        "reset cancels the old destination"
    );
}

#[test]
fn chunk_pan_retains_visible_terrain_and_drains_residency_work_over_ticks() {
    use bozzard_scene::Transform;
    let mut demo = script_fixture(
        r#"
        for x in -3..8 { discover_chunk(x, 0); }
        enter_chunk(4, 0);
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
        let inventory = empty_numbers(32); inventory[0] = 20.0; inventory[1] = 37.0;
        storage_write(cell, inventory);
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
        for x in 1..7 { discover_chunk(x, 0); }
        enter_chunk(4, 0);
        set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 0.0);
    "#,
    );
    settle(&mut demo, 40);
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
            enter_chunk({cx}, {cz});
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
        let inventory = empty_numbers(32); inventory[0] = 20.0; inventory[1] = 37.0;
        storage_write(cell, inventory);
        let counts = get_scene_list("counts"); counts[20] = 37.0; set_scene_list("counts", counts);
        set_scene_variable("cursor_x", cell_x(cell).to_float());
        set_scene_variable("cursor_z", cell_z(cell).to_float());
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
                set_scene_variable("cursor_x", cell_x(i).to_float());
                set_scene_variable("cursor_z", cell_z(i).to_float());
                remove_selected();
            }}
        }}
        set_scene_variable("cursor_x", 7.0);
        set_scene_variable("cursor_z", 7.0);
        let nodes = get_scene_list("nodes"); nodes[224] = {node}.0; set_scene_list("nodes", nodes);
        set_scene_variable("selected", {kind}.0); set_scene_variable("direction", 0.0);
        place_selected();
        let item_list = if {kind} == 3 && {item} < 10 {{ "input_items" }} else {{ "items" }};
        let amount_list = if item_list == "items" {{ "item_amounts" }} else {{ "input_amounts" }};
        let items = get_scene_list(item_list); items[224] = {item}.0; set_scene_list(item_list, items);
        let amounts = get_scene_list(amount_list); amounts[224] = 1.0; set_scene_list(amount_list, amounts);
        factory_step();
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
    press(&mut demo, "E");
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
    assert_eq!(numbers(&demo, "assembler_iron")[224], 2.);
    assert_eq!(numbers(&demo, "assembler_copper")[224], 2.);
    settle(&mut demo, 40);
    assert_eq!(
        numbers(&demo, "items")[224],
        20.,
        "freed output buffer resumes assembly"
    );
    assert_eq!(numbers(&demo, "assembler_iron")[224], 1.);
    press(&mut demo, "E");
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
        set_scene_variable("selected", 4.0); place_selected();
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
        remove_selected();
        for row in {layout} {{
            let cell = row[0];
            let nodes = get_scene_list("nodes"); nodes[cell] = 0.0; set_scene_list("nodes", nodes);
            set_scene_variable("cursor_x", cell_x(cell).to_float());
            set_scene_variable("cursor_z", cell_z(cell).to_float());
            set_scene_variable("selected", row[1].to_float());
            set_scene_variable("direction", row[2].to_float());
            place_selected();
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
    let mut demo = collection_fixture(1, 1, "for beat in 0..250 { factory_step(); }");
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
        "[[112, 3, 0, 11, 60], [111, 2, 0, 1, 10]]",
        r#"
        let inputs = get_scene_list("input_items"); inputs[112] = 1.0; set_scene_list("input_items", inputs);
        let amounts = get_scene_list("input_amounts"); amounts[112] = 40.0; set_scene_list("input_amounts", amounts);
        for beat in 0..21 { factory_step(); }
        set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 0.0);
    "#,
    );
    assert_eq!(machine_load(&demo, 112), 100.);
    assert_eq!(
        numbers(&demo, "item_amounts")[111],
        10.,
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
        numbers(&demo, "item_amounts")[111] < 10.,
        "feed resumes after collection"
    );
    assert!(machine_load(&demo, 112) <= 100.);
}

#[test]
fn competing_assembler_inputs_share_capacity_and_leave_room_for_the_missing_ingredient() {
    let mut demo = buffer_layout(
        "[[112, 5, 0, 20, 98], [111, 2, 0, 11, 10], [97, 2, 1, 11, 10], [113, 2, 2, 12, 10]]",
        r#"factory_step(); set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 0.0);"#,
    );
    assert_eq!(machine_load(&demo, 112), 100.);
    assert_eq!(numbers(&demo, "assembler_iron")[112], 1.);
    assert_eq!(numbers(&demo, "assembler_copper")[112], 1.);
    assert_eq!(numbers(&demo, "item_amounts")[111], 9.);
    assert_eq!(
        numbers(&demo, "item_amounts")[97],
        10.,
        "second iron input cannot consume copper's space"
    );
    assert_eq!(numbers(&demo, "item_amounts")[113], 9.);
    press(&mut demo, "E");
    assert_eq!(controller_numbers(&demo, "stock")[20], 98.);
    assert_eq!(machine_load(&demo, 112), 2.);
    settle(&mut demo, 40);
    assert!(numbers(&demo, "item_amounts")[112] > 0.);
    assert!(machine_load(&demo, 112) <= 100.);

    let demo = buffer_layout(
        "[[112, 5, 0, 0, 0], [111, 2, 0, 11, 100]]",
        r#"
        for beat in 0..110 { factory_step(); }
    "#,
    );
    assert_eq!(numbers(&demo, "assembler_iron")[112], 99.);
    assert_eq!(numbers(&demo, "item_amounts")[111], 1.);
    assert_eq!(
        machine_load(&demo, 112),
        99.,
        "one space stays available for copper"
    );
}

#[test]
fn logistics_buffers_stop_at_100_and_transfers_conserve_items() {
    for kind in [2, 7, 8] {
        let demo = buffer_layout(
            &format!("[[111, 2, 0, 20, 5], [112, {kind}, 0, 20, 100]]"),
            "for beat in 0..4 { factory_step(); }",
        );
        assert_eq!(machine_load(&demo, 112), 100.);
        assert_eq!(numbers(&demo, "item_amounts")[111], 5.);
        // Drain toward storage after verifying backpressure. Splitters use north/south.
        let stores = if kind == 7 { "[97, 127]" } else { "[113]" };
        let demo = buffer_layout(
            &format!("[[111, 2, 0, 20, 5], [112, {kind}, 0, 20, 100]]"),
            &format!(
                r#"
            for cell in {stores} {{
                let nodes = get_scene_list("nodes"); nodes[cell] = 0.0; set_scene_list("nodes", nodes);
                set_scene_variable("cursor_x", cell_x(cell).to_float());
                set_scene_variable("cursor_z", cell_z(cell).to_float());
                set_scene_variable("selected", 4.0); place_selected();
            }}
            for beat in 0..12 {{ factory_step(); }}
        "#
            ),
        );
        assert_eq!(
            numbers(&demo, "item_amounts").iter().sum::<f32>() + numbers(&demo, "counts")[20],
            105.
        );
        assert_eq!(
            numbers(&demo, "counts")[20],
            12.,
            "buffer size must not multiply belt throughput"
        );
        assert!(
            numbers(&demo, "item_amounts")
                .iter()
                .all(|amount| *amount <= 100.)
        );
    }
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
        enter_chunk(4, 0); unload_chunk_visuals(144); enter_chunk(0, 0);
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
        for x in 1..7 { discover_chunk(x, 0); }
        discover_chunk(-1, -1);
        enter_chunk(4, 0);
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
