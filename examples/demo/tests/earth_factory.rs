//! Editor-Play simulation check for the standalone Earth factory prototype.
use bozzard_demo::SceneDemo;
use bozzard_scene::blueprint::{BlackboardValue, Value};
use bozzard_scene::{BlueprintRuntime, GameplayInput, Scene, keys};
use std::path::PathBuf;

fn demo() -> SceneDemo {
    demo_with_seed(None)
}

fn demo_with_seed(seed: Option<f32>) -> SceneDemo {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../earth-factory/scenes/earth.json");
    let mut scene = Scene::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
    if let Some(seed) = seed {
        scene
            .blackboard
            .insert("seed".into(), BlackboardValue::Scalar(Value::Number(seed)));
    }
    SceneDemo::new_with_prefabs(&scene, Some(&path)).unwrap()
}

fn tick(demo: &mut SceneDemo, key: Option<&str>) {
    demo.set_gameplay_input(GameplayInput {
        keys: key.map_or(0, keys::bit),
        ..Default::default()
    });
    demo.app.step();
    demo.check_simulation().unwrap();
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
    let mut demo = demo();
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

fn storage_fixture(setup: &str) -> (SceneDemo, usize) {
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
