// Earth factory prototype. This is Rhai gameplay source, loaded by Bozzard's Script Manager.
// Numeric and text blackboard lists hold the factory grid across ticks and editor Play saves.

fn index(x, z) { (z + 7) * 15 + x + 7 }
fn cell_x(i) { (i % 15) - 7 }
fn cell_z(i) { (i / 15) - 7 }
fn step_x(dir) { if dir == 0 { 1 } else if dir == 2 { -1 } else { 0 } }
fn step_z(dir) { if dir == 1 { 1 } else if dir == 3 { -1 } else { 0 } }
fn layout_x(x, z, origin, rotation) {
    if rotation == 0 { origin + x }
    else if rotation == 1 { origin - z }
    else if rotation == 2 { origin - x }
    else { origin + z }
}
fn layout_z(x, z, origin, rotation) {
    if rotation == 0 { origin + z }
    else if rotation == 1 { origin + x }
    else if rotation == 2 { origin - z }
    else { origin - x }
}
fn layout_cell(x, z, origin_x, origin_z, rotation) {
    index(layout_x(x, z, origin_x, rotation), layout_z(x, z, origin_z, rotation))
}

fn empty_numbers(count) {
    let values = [];
    for i in 0..count { values.push(0.0); }
    values
}

fn empty_text(count) {
    let values = [];
    for i in 0..count { values.push(""); }
    values
}

fn node_asset(kind) {
    if kind == 1.0 { "node-iron" }
    else if kind == 2.0 { "node-copper" }
    else if kind == 3.0 { "node-limestone" }
    else if kind == 4.0 { "node-coal" }
    else if kind == 5.0 { "node-quartz" }
    else if kind == 6.0 { "node-oil" }
    else { "node-water" }
}

fn node_name(kind) {
    if kind == 1.0 { "iron" }
    else if kind == 2.0 { "copper" }
    else if kind == 3.0 { "limestone" }
    else if kind == 4.0 { "coal" }
    else if kind == 5.0 { "quartz" }
    else if kind == 6.0 { "oil" }
    else if kind == 7.0 { "water" }
    else { "none" }
}

fn build_asset(kind) {
    if kind == 1.0 { "machine-miner" }
    else if kind == 2.0 { "machine-belt" }
    else if kind == 3.0 { "machine-smelter" }
    else if kind == 4.0 { "machine-storage" }
    else if kind == 5.0 { "machine-assembler" }
    else if kind == 6.0 { "machine-generator" }
    else if kind == 7.0 { "machine-splitter" }
    else { "machine-merger" }
}

fn build_name(kind) {
    if kind == 1.0 { "MINER" }
    else if kind == 2.0 { "BELT" }
    else if kind == 3.0 { "SMELTER" }
    else if kind == 4.0 { "STORAGE" }
    else if kind == 5.0 { "ASSEMBLER" }
    else if kind == 6.0 { "GENERATOR" }
    else if kind == 7.0 { "SPLITTER" }
    else if kind == 8.0 { "MERGER" }
    else { "EMPTY" }
}

fn item_name(kind) {
    if kind == 1.0 { "iron ore" }
    else if kind == 2.0 { "copper ore" }
    else if kind == 3.0 { "limestone" }
    else if kind == 4.0 { "coal" }
    else if kind == 5.0 { "quartz" }
    else if kind == 6.0 { "oil" }
    else if kind == 7.0 { "water" }
    else if kind == 11.0 { "iron ingot" }
    else if kind == 12.0 { "copper ingot" }
    else if kind == 20.0 { "machine part" }
    else { "empty" }
}

fn item_color(kind) {
    if kind == 1.0 { [0.42, 0.65, 0.73] }
    else if kind == 2.0 { [0.92, 0.47, 0.21] }
    else if kind == 11.0 { [0.69, 0.84, 0.88] }
    else if kind == 12.0 { [0.98, 0.69, 0.36] }
    else if kind == 20.0 { [0.80, 0.72, 1.0] }
    else { [0.91, 0.86, 0.64] }
}

fn spawn_build(kind, x, z, facing) {
    let target = spawn_prefab(build_asset(kind), [x.to_float(), 0.08, z.to_float()]);
    set_rotation(target, [0.0, -facing.to_float() * 90.0, 0.0]);
    target
}

fn clear_visuals(name) {
    for target in get_scene_list(name) {
        if target != "" { destroy_prefab(target); }
    }
}

fn begin_world(seed) {
    clear_visuals("node_visuals");
    clear_visuals("build_visuals");
    clear_visuals("item_visuals");
    clear_visuals("retired_visuals");
    clear_visuals("item_pool");
    for name in ["motion_visuals", "retired_visuals", "item_pool", "motion_from", "motion_to", "motion_from_y", "motion_to_y"] {
        set_scene_list(name, []);
    }
    set_scene_variable("tooltip_cell", -1.0);
    set_scene_variable("tooltip_alpha", 0.0);
    set_scene_variable("objective_display", 0.0);
    set_ui_visible("nearby-tooltip", false);
    for page in 0..4 {
        set_scene_list("storage_kinds_" + page.to_string(), empty_numbers(900));
        set_scene_list("storage_amounts_" + page.to_string(), empty_numbers(900));
    }
    set_scene_variable("storage_open", false);
    set_scene_variable("storage_cell", -1.0);
    set_scene_variable("storage_alpha", 0.0);
    set_scene_variable("storage_drag", -1.0);
    set_scene_variable("storage_menu", -1.0);
    set_scene_variable("storage_menu_alpha", 0.0);
    set_scene_variable("storage_revision", 0.0);
    set_scene_variable("storage_render_revision", -1.0);
    set_scene_variable("storage_render_cell", -1.0);
    set_scene_variable("storage_render_drag", -1.0);
    set_scene_variable("storage_render_menu", -1.0);
    set_scene_list("storage_view", empty_numbers(32));
    set_ui_visible("storage-overlay", false);
    set_ui_visible("stack-menu", false);
    set_ui_visible("stack-drag", false);

    let nodes = empty_numbers(225);
    let builds = empty_numbers(225);
    let facings = empty_numbers(225);
    let node_visuals = empty_text(225);
    let build_visuals = empty_text(225);
    let item_visuals = empty_text(225);

    // A fresh Play session starts with a populated test factory. N supplies a later seed so
    // the resource deposits can be reshuffled without editing the authored scene.
    let state = seed;
    let rotation = seed % 4;
    state = (state * 48271) % 2147483647;
    let origin_x_min = if rotation == 0 { -1 } else if rotation == 1 || rotation == 2 { -5 } else { -7 };
    let origin_x = origin_x_min + state % (if rotation % 2 == 0 { 7 } else { 13 });
    state = (state * 48271) % 2147483647;
    let origin_z_min = if rotation == 0 { -7 } else if rotation == 1 { -1 } else { -5 };
    let origin_z = origin_z_min + state % (if rotation % 2 == 0 { 13 } else { 7 });
    nodes[layout_cell(-6, 0, origin_x, origin_z, rotation)] = 1.0;
    nodes[layout_cell(-6, 2, origin_x, origin_z, rotation)] = 2.0;
    nodes[layout_cell(2, 1, origin_x, origin_z, rotation)] = 4.0;

    // Two working smelting lines feed an assembler, which sends parts to storage. The coal
    // generator powers the extra machines. Each new world keeps this runnable test layout.
    let iron_builds = [1.0, 2.0, 3.0, 2.0, 5.0, 4.0];
    for offset in 0..6 {
        let cell = layout_cell(-6 + offset, 0, origin_x, origin_z, rotation);
        builds[cell] = iron_builds[offset];
        facings[cell] = rotation.to_float();
    }
    let copper_builds = [1.0, 2.0, 3.0, 2.0, 2.0];
    for offset in 0..5 {
        let cell = layout_cell(-6 + offset, 2, origin_x, origin_z, rotation);
        builds[cell] = copper_builds[offset];
        facings[cell] = rotation.to_float();
    }
    let corner = layout_cell(-2, 2, origin_x, origin_z, rotation);
    let upward_belt = layout_cell(-2, 1, origin_x, origin_z, rotation);
    builds[upward_belt] = 2.0;
    facings[corner] = ((rotation + 3) % 4).to_float();
    facings[upward_belt] = ((rotation + 3) % 4).to_float();
    builds[layout_cell(2, 1, origin_x, origin_z, rotation)] = 6.0;

    // The demonstration iron line moves with the seed, and every other deposit is scattered
    // independently. All seven Earth resource types are guaranteed in every generated world.
    let other_nodes = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
    for kind in other_nodes {
        let tries = 0;
        let placed = false;
        while tries < 200 {
            state = (state * 48271) % 2147483647;
            let x = (state % 13) - 6;
            state = (state * 48271) % 2147483647;
            let z = (state % 13) - 6;
            let cell = index(x, z);
            if nodes[cell] == 0.0 && builds[cell] == 0.0 {
                nodes[cell] = kind;
                placed = true;
                break;
            }
            tries += 1;
        }
        if !placed {
            for cell in 0..225 {
                if nodes[cell] == 0.0 && builds[cell] == 0.0 {
                    nodes[cell] = kind;
                    break;
                }
            }
        }
    }

    for cell in 0..225 {
        let x = cell_x(cell);
        let z = cell_z(cell);
        if nodes[cell] != 0.0 {
            node_visuals[cell] = spawn_prefab(
                node_asset(nodes[cell]), [x.to_float(), 0.08, z.to_float()]
            );
        }
        if builds[cell] != 0.0 {
            build_visuals[cell] = spawn_build(builds[cell], x, z, facings[cell].to_int());
        }
    }

    let machine_cells = [];
    for cell in 0..225 {
        if builds[cell] != 0.0 { machine_cells.push(cell.to_float()); }
    }
    set_scene_list("machine_cells", machine_cells);
    set_scene_list("nodes", nodes);
    set_scene_list("builds", builds);
    set_scene_list("facings", facings);
    set_scene_list("items", empty_numbers(225));
    set_scene_list("progress", empty_numbers(225));
    set_scene_list("assembler_iron", empty_numbers(225));
    set_scene_list("assembler_copper", empty_numbers(225));
    set_scene_list("split_state", empty_numbers(225));
    set_scene_list("counts", empty_numbers(32));
    set_scene_list("node_visuals", node_visuals);
    set_scene_list("build_visuals", build_visuals);
    set_scene_list("item_visuals", item_visuals);
    set_scene_variable("started", true);
    set_scene_variable("seed", seed.to_float());
    let cursor_x = layout_x(-3, 1, origin_x, rotation);
    let cursor_z = layout_z(-3, 1, origin_z, rotation);
    set_scene_variable("cursor_x", cursor_x.to_float());
    set_scene_variable("cursor_z", cursor_z.to_float());
    set_scene_variable("clock", 0.0);
    set_scene_variable("ticks", 0.0);
    set_scene_variable("message", "Iron + copper feed the assembler. Follow the moving items to storage.");
    set_position("cursor", [cursor_x.to_float(), 0.115, cursor_z.to_float()]);
}

// Sixteen stacks per container, stored in four bounded blackboard pages.
// Each page contains four slots for each of the 225 build cells.
fn storage_read(cell) {
    let result = [];
    for page in 0..4 {
        let kinds = get_scene_list("storage_kinds_" + page.to_string());
        let amounts = get_scene_list("storage_amounts_" + page.to_string());
        for slot in 0..4 {
            result.push(kinds[cell * 4 + slot]);
            result.push(amounts[cell * 4 + slot]);
        }
    }
    result
}

fn storage_write(cell, inventory) {
    set_scene_variable("storage_revision", get_scene_variable("storage_revision") + 1.0);
    for page in 0..4 {
        let kinds = get_scene_list("storage_kinds_" + page.to_string());
        let amounts = get_scene_list("storage_amounts_" + page.to_string());
        for slot in 0..4 {
            kinds[cell * 4 + slot] = inventory[(page * 4 + slot) * 2];
            amounts[cell * 4 + slot] = inventory[(page * 4 + slot) * 2 + 1];
        }
        set_scene_list("storage_kinds_" + page.to_string(), kinds);
        set_scene_list("storage_amounts_" + page.to_string(), amounts);
    }
}

fn storage_deposit(cell, item) {
    let inventory = storage_read(cell);
    let target = -1;
    for slot in 0..16 {
        if inventory[slot * 2] == item && inventory[slot * 2 + 1] < 100.0 {
            target = slot;
            break;
        }
    }
    if target < 0 {
        for slot in 0..16 {
            if inventory[slot * 2 + 1] == 0.0 { target = slot; break; }
        }
    }
    if target < 0 { return false; }
    inventory[target * 2] = item;
    inventory[target * 2 + 1] += 1.0;
    storage_write(cell, inventory);
    true
}

fn nearby_storage() {
    let x = get_scene_variable("cursor_x").to_int();
    let z = get_scene_variable("cursor_z").to_int();
    let builds = get_scene_list("builds");
    let best = 2.1;
    let result = -1;
    for cell in 0..225 {
        if builds[cell] != 4.0 { continue; }
        let dx = (cell_x(cell) - x).to_float();
        let dz = (cell_z(cell) - z).to_float();
        let distance = dx * dx + dz * dz;
        if distance < best { best = distance; result = cell; }
    }
    result
}

fn close_storage() {
    set_scene_variable("storage_open", false);
    set_scene_variable("storage_drag", -1.0);
    set_scene_variable("storage_menu", -1.0);
}

fn inventory_slot(target) {
    for slot in 0..16 {
        if target == "inventory-slot-" + slot.to_string() { return slot; }
    }
    -1
}

fn move_stack(cell, from, to) {
    if from == to || to < 0 { return; }
    let inventory = storage_read(cell);
    let a = from * 2;
    let b = to * 2;
    if inventory[a + 1] == 0.0 { return; }
    if inventory[b] == inventory[a] {
        let moved = min(inventory[a + 1], 100.0 - inventory[b + 1]);
        inventory[b + 1] += moved;
        inventory[a + 1] -= moved;
        if inventory[a + 1] == 0.0 { inventory[a] = 0.0; }
    } else {
        let kind = inventory[b];
        let amount = inventory[b + 1];
        inventory[b] = inventory[a];
        inventory[b + 1] = inventory[a + 1];
        inventory[a] = kind;
        inventory[a + 1] = amount;
    }
    storage_write(cell, inventory);
}

fn stack_action(cell, slot, action) {
    let inventory = storage_read(cell);
    let kind = inventory[slot * 2];
    if kind == 0.0 { return; }
    if action == "stack-delete" {
        let removed = 0.0;
        for i in 0..16 {
            if inventory[i * 2] == kind {
                removed += inventory[i * 2 + 1];
                inventory[i * 2] = 0.0;
                inventory[i * 2 + 1] = 0.0;
            }
        }
        let counts = get_scene_list("counts");
        counts[kind.to_int()] -= removed;
        set_scene_list("counts", counts);
        set_ui_text("storage-help", "Deleted all " + item_name(kind) + " from this storage.");
    } else {
        let target = -1;
        for i in 0..16 {
            if inventory[i * 2 + 1] == 0.0 { target = i; break; }
        }
        if target < 0 || inventory[slot * 2 + 1] < 2.0 {
            set_ui_text("storage-help", "Split needs an empty slot and at least 2 items.");
            return;
        }
        let half = (inventory[slot * 2 + 1].to_int() / 2).to_float();
        inventory[slot * 2 + 1] -= half;
        inventory[target * 2] = kind;
        inventory[target * 2 + 1] = half;
        set_ui_text("storage-help", "Stack split. Drag matching stacks together to merge them.");
    }
    storage_write(cell, inventory);
}

fn update_storage(dt) {
    let cell = get_scene_variable("storage_cell").to_int();
    if input_pressed("E") {
        if get_scene_variable("storage_open") { close_storage(); }
        else if get_scene_variable("storage_alpha") <= 0.0 {
            cell = nearby_storage();
            if cell >= 0 {
                set_scene_variable("storage_cell", cell.to_float());
                set_scene_variable("storage_open", true);
                set_ui_text("storage-title", "Storage  /  " + cell_x(cell).to_string() + ", " + cell_z(cell).to_string());
                set_ui_text("storage-help", "Drag to move or merge  •  Right-click for stack actions");
            } else {
                set_scene_variable("message", "Move next to a storage container to open it with E.");
            }
        }
    }
    let drag = get_scene_variable("storage_drag").to_int();
    let menu = get_scene_variable("storage_menu").to_int();
    for event in ui_events() {
        if !get_scene_variable("storage_open") { break; }
        let slot = inventory_slot(event.target);
        if event.kind == "cancel" { drag = -1; menu = -1; }
        else if event.kind == "secondary" {
            drag = -1;
            menu = -1;
            if slot >= 0 {
                let inventory = storage_read(cell);
                if inventory[slot * 2 + 1] > 0.0 {
                    menu = slot;
                    set_ui_screen_position("stack-menu", event.x, event.y);
                    set_scene_variable("storage_menu_alpha", 0.0);
                }
            }
        } else if event.kind == "down" {
            if event.target != "stack-split" && event.target != "stack-delete" {
                menu = -1;
                if slot >= 0 {
                    let inventory = storage_read(cell);
                    if inventory[slot * 2 + 1] > 0.0 { drag = slot; }
                }
            }
        } else if event.kind == "up" {
            if drag >= 0 { move_stack(cell, drag, slot); }
            drag = -1;
        } else if event.kind == "activate" {
            if event.target == "storage-close" { close_storage(); drag = -1; menu = -1; }
            else if menu >= 0 && (event.target == "stack-delete" || event.target == "stack-split") {
                stack_action(cell, menu, event.target);
                menu = -1;
            }
        }
    }
    let opened = get_scene_variable("storage_open");
    if !opened { drag = -1; menu = -1; }
    set_scene_variable("storage_drag", drag.to_float());
    set_scene_variable("storage_menu", menu.to_float());
    let alpha = get_scene_variable("storage_alpha");
    alpha = clamp(alpha + if opened { dt / 0.22 } else { -dt / 0.18 }, 0.0, 1.0);
    set_scene_variable("storage_alpha", alpha);
    set_ui_visible("storage-overlay", alpha > 0.0);
    let eased = alpha * alpha * (3.0 - 2.0 * alpha);
    set_ui_opacity("storage-overlay", eased);
    set_ui_offset("storage-panel", 0.0, 24.0 * (1.0 - eased));
    // Disable all button input during closing while leaving the visual fade alive.
    set_ui_enabled("storage-panel", opened);
    set_ui_enabled("stack-menu", opened && menu >= 0);
    if alpha <= 0.0 || cell < 0 { return; }

    let revision = get_scene_variable("storage_revision");
    let changed = revision != get_scene_variable("storage_render_revision") ||
        cell.to_float() != get_scene_variable("storage_render_cell") ||
        drag.to_float() != get_scene_variable("storage_render_drag") ||
        menu.to_float() != get_scene_variable("storage_render_menu");
    let inventory = if changed { storage_read(cell) } else { get_scene_list("storage_view") };
    let used = get_scene_variable("storage_used").to_int();
    if changed {
        set_scene_list("storage_view", inventory);
        set_scene_variable("storage_render_revision", revision);
        set_scene_variable("storage_render_cell", cell.to_float());
        set_scene_variable("storage_render_drag", drag.to_float());
        set_scene_variable("storage_render_menu", menu.to_float());
        used = 0;
        let total = 0;
        for slot in 0..16 {
            let name = "inventory-slot-" + slot.to_string();
            let kind = inventory[slot * 2];
            let amount = inventory[slot * 2 + 1].to_int();
            if amount > 0 { used += 1; total += amount; }
            set_ui_text(name + "-name", if amount > 0 { item_name(kind) } else { "Empty" });
            set_ui_text(name + "-count", if amount > 0 { amount.to_string() } else { "" });
            let color = item_color(kind);
            set_ui_background(name + "-icon", if amount > 0 { [color[0], color[1], color[2], 1.0] } else { [0.24, 0.31, 0.26, 1.0] });
            set_ui_background(name, if slot == menu { [0.27, 0.24, 0.14, 1.0] } else { [0.10, 0.15, 0.12, 1.0] });
            set_ui_opacity(name, if slot == drag { 0.45 } else { 1.0 });
        }
        set_ui_text("storage-subtitle", used.to_string() + " / 16 slots   •   " + total.to_string() + " items   •   100 per stack");
        set_scene_variable("storage_used", used.to_float());
    }
    let pointer = ui_pointer();
    set_ui_visible("stack-drag", drag >= 0 && pointer[0] >= 0.0);
    if drag >= 0 {
        set_ui_text("stack-drag", item_name(inventory[drag * 2]) + "\nx " + inventory[drag * 2 + 1].to_int().to_string());
        set_ui_screen_position("stack-drag", pointer[0], pointer[1]);
    }
    let menu_alpha = get_scene_variable("storage_menu_alpha");
    menu_alpha = clamp(menu_alpha + if menu >= 0 { dt / 0.12 } else { -dt / 0.10 }, 0.0, 1.0);
    set_scene_variable("storage_menu_alpha", menu_alpha);
    set_ui_visible("stack-menu", menu_alpha > 0.0);
    set_ui_opacity("stack-menu", menu_alpha * menu_alpha * (3.0 - 2.0 * menu_alpha));
    set_ui_offset("stack-menu", 0.0, 6.0 * (1.0 - menu_alpha));
    if menu >= 0 {
        set_ui_text("stack-menu-title", item_name(inventory[menu * 2]));
        let can_split = inventory[menu * 2 + 1] >= 2.0 && used < 16;
        set_ui_enabled("stack-split", can_split);
        set_ui_opacity("stack-split", if can_split { 1.0 } else { 0.4 });
    }
}

fn item_height(kind) {
    if kind == 2.0 || kind == 7.0 || kind == 8.0 { 0.52 }
    else if kind == 4.0 { 0.88 }
    else { 1.10 }
}

fn animate_items(fraction) {
    let visuals = get_scene_list("motion_visuals");
    let starts = get_scene_list("motion_from");
    let ends = get_scene_list("motion_to");
    let from_y = get_scene_list("motion_from_y");
    let to_y = get_scene_list("motion_to_y");
    for i in 0..visuals.len() {
        if visuals[i] == "" { continue; }
        let from = starts[i].to_int();
        let to = ends[i].to_int();
        set_position(visuals[i], [
            lerp(cell_x(from).to_float(), cell_x(to).to_float(), fraction),
            lerp(from_y[i], to_y[i], fraction),
            lerp(cell_z(from).to_float(), cell_z(to).to_float(), fraction)
        ]);
    }
}

fn factory_step() {
    animate_items(1.0);
    // Recycle delivered item visuals. Spawning/destroying prefabs changes the scene hierarchy;
    // it should not happen on every production beat after the conveyor line has warmed up.
    let pool = get_scene_list("item_pool");
    for target in get_scene_list("retired_visuals") {
        set_visible(target, false);
        pool.push(target);
    }
    let motion_visuals = [];
    let motion_from = [];
    let motion_to = [];
    let motion_from_y = [];
    let motion_to_y = [];
    let retired = [];
    let nodes = get_scene_list("nodes");
    let builds = get_scene_list("builds");
    let facings = get_scene_list("facings");
    let items = get_scene_list("items");
    let machine_cells = get_scene_list("machine_cells");
    let progress = get_scene_list("progress");
    let iron = get_scene_list("assembler_iron");
    let copper = get_scene_list("assembler_copper");
    let split = get_scene_list("split_state");
    let counts = get_scene_list("counts");
    let visuals = get_scene_list("item_visuals");
    let tick = get_scene_variable("ticks").to_int() + 1;

    let supply = 8;
    let demand = 0;
    for entry in machine_cells {
        let cell = entry.to_int();
        let kind = builds[cell];
        if kind == 1.0 { demand += 1; }
        else if kind == 3.0 { demand += 2; }
        else if kind == 5.0 { demand += 3; }
        if kind == 6.0 && nodes[cell] == 4.0 { supply += 10; }
    }
    let powered = demand <= supply;
    set_scene_variable("power_supply", supply.to_float());
    set_scene_variable("power_demand", demand.to_float());

    if powered {
        for entry in machine_cells {
            let cell = entry.to_int();
            let kind = builds[cell];
            if kind == 1.0 && nodes[cell] != 0.0 && items[cell] == 0.0 && tick % 2 == 0 {
                items[cell] = nodes[cell];
            } else if kind == 3.0 && (items[cell] == 1.0 || items[cell] == 2.0) {
                progress[cell] += 1.0;
                if progress[cell] >= 2.0 {
                    items[cell] += 10.0;
                    progress[cell] = 0.0;
                }
            } else if kind == 5.0 && items[cell] == 0.0 &&
                    iron[cell] >= 1.0 && copper[cell] >= 1.0 && tick % 2 == 0 {
                iron[cell] -= 1.0;
                copper[cell] -= 1.0;
                items[cell] = 20.0;
            }
        }
    }

    // An item owns one visual throughout its journey. Processing only changes its color;
    // the presentation interpolates every frame between the simulation's cell transfers.
    for entry in machine_cells {
        let cell = entry.to_int();
        if items[cell] == 0.0 { continue; }
        if visuals[cell] == "" {
            let position = [cell_x(cell).to_float(), item_height(builds[cell]), cell_z(cell).to_float()];
            if pool.len() > 0 {
                visuals[cell] = pool.pop();
                set_position(visuals[cell], position);
                set_visible(visuals[cell], true);
            } else { visuals[cell] = spawn_prefab("item", position); }
        }
        set_color(visuals[cell], item_color(items[cell]));
    }
    let next_items = items;
    let next_visuals = visuals;
    for entry in machine_cells {
        let cell = entry.to_int();
        let kind = builds[cell];
        let item = items[cell];
        if item == 0.0 || next_items[cell] == 0.0 { continue; }
        if kind == 3.0 && item < 10.0 { continue; }
        if kind != 1.0 && kind != 2.0 && kind != 3.0 &&
                kind != 5.0 && kind != 7.0 && kind != 8.0 { continue; }

        let direction = facings[cell].to_int();
        if kind == 7.0 {
            if split[cell] == 0.0 { direction = (direction + 3) % 4; }
            else { direction = (direction + 1) % 4; }
        }
        let x = cell_x(cell) + step_x(direction);
        let z = cell_z(cell) + step_z(direction);
        if x < -7 || x > 7 || z < -7 || z > 7 { continue; }
        let destination = index(x, z);
        let target = builds[destination];
        if target == 4.0 && storage_deposit(destination, item) {
            counts[item.to_int()] += 1.0;
            next_items[cell] = 0.0;
            retired.push(visuals[cell]);
        } else if target == 5.0 {
            if item == 11.0 && iron[destination] < 3.0 {
                iron[destination] += 1.0;
                next_items[cell] = 0.0;
                retired.push(visuals[cell]);
            } else if item == 12.0 && copper[destination] < 3.0 {
                copper[destination] += 1.0;
                next_items[cell] = 0.0;
                retired.push(visuals[cell]);
            }
        } else if (target == 2.0 || target == 3.0 || target == 7.0 || target == 8.0) &&
                items[destination] == 0.0 && next_items[destination] == 0.0 {
            if target != 3.0 || item == 1.0 || item == 2.0 {
                next_items[destination] = item;
                next_items[cell] = 0.0;
                next_visuals[destination] = visuals[cell];
            }
        }
        if next_items[cell] == 0.0 {
            next_visuals[cell] = "";
            motion_visuals.push(visuals[cell]);
            motion_from.push(cell.to_float());
            motion_to.push(destination.to_float());
            motion_from_y.push(item_height(kind));
            motion_to_y.push(item_height(target));
        }
        if kind == 7.0 && next_items[cell] == 0.0 { split[cell] = 1.0 - split[cell]; }
    }

    set_scene_list("motion_visuals", motion_visuals);
    set_scene_list("motion_from", motion_from);
    set_scene_list("motion_to", motion_to);
    set_scene_list("motion_from_y", motion_from_y);
    set_scene_list("motion_to_y", motion_to_y);
    set_scene_list("retired_visuals", retired);
    set_scene_list("item_pool", pool);
    set_scene_list("items", next_items);
    set_scene_list("progress", progress);
    set_scene_list("assembler_iron", iron);
    set_scene_list("assembler_copper", copper);
    set_scene_list("split_state", split);
    set_scene_list("counts", counts);
    set_scene_list("item_visuals", next_visuals);
    set_scene_variable("ticks", tick.to_float());
}

fn place_selected() {
    let x = get_scene_variable("cursor_x").to_int();
    let z = get_scene_variable("cursor_z").to_int();
    let cell = index(x, z);
    let nodes = get_scene_list("nodes");
    let builds = get_scene_list("builds");
    let facings = get_scene_list("facings");
    let visuals = get_scene_list("build_visuals");
    let selected = get_scene_variable("selected");
    if builds[cell] != 0.0 {
        set_scene_variable("message", "Remove the existing machine with X first.");
        return;
    }
    if selected == 1.0 && nodes[cell] == 0.0 {
        set_scene_variable("message", "A miner must sit on a resource node.");
        return;
    }
    if selected == 6.0 && nodes[cell] != 4.0 {
        set_scene_variable("message", "A generator needs a coal node.");
        return;
    }
    if selected != 1.0 && selected != 6.0 && nodes[cell] != 0.0 {
        set_scene_variable("message", "Only a miner or coal generator can cover a node.");
        return;
    }
    let facing = get_scene_variable("direction").to_int();
    let machine_cells = get_scene_list("machine_cells");
    machine_cells.push(cell.to_float());
    set_scene_list("machine_cells", machine_cells);
    builds[cell] = selected;
    facings[cell] = facing.to_float();
    visuals[cell] = spawn_build(selected, x, z, facing);
    set_scene_list("builds", builds);
    set_scene_list("facings", facings);
    set_scene_list("build_visuals", visuals);
    set_scene_variable("message", "Built " + build_name(selected) + ".");
}

fn remove_selected() {
    let x = get_scene_variable("cursor_x").to_int();
    let z = get_scene_variable("cursor_z").to_int();
    let cell = index(x, z);
    let builds = get_scene_list("builds");
    if builds[cell] == 0.0 { return; }
    if builds[cell] == 4.0 {
        let inventory = storage_read(cell);
        let counts = get_scene_list("counts");
        for slot in 0..16 {
            counts[inventory[slot * 2].to_int()] -= inventory[slot * 2 + 1];
        }
        set_scene_list("counts", counts);
        storage_write(cell, empty_numbers(32));
    }
    let visuals = get_scene_list("build_visuals");
    destroy_prefab(visuals[cell]);
    visuals[cell] = "";
    builds[cell] = 0.0;
    let machine_cells = [];
    for entry in get_scene_list("machine_cells") {
        if entry.to_int() != cell { machine_cells.push(entry); }
    }
    set_scene_list("machine_cells", machine_cells);
    let items = get_scene_list("items");
    items[cell] = 0.0;
    let item_visuals = get_scene_list("item_visuals");
    if item_visuals[cell] != "" {
        let motions = get_scene_list("motion_visuals");
        for i in 0..motions.len() {
            if motions[i] == item_visuals[cell] { motions[i] = ""; }
        }
        set_scene_list("motion_visuals", motions);
        destroy_prefab(item_visuals[cell]);
        item_visuals[cell] = "";
    }
    set_scene_list("builds", builds);
    set_scene_list("build_visuals", visuals);
    set_scene_list("items", items);
    set_scene_list("item_visuals", item_visuals);
    set_scene_variable("message", "Machine removed. The resource node remains.");
}

fn rotate_selected() {
    let cell = index(get_scene_variable("cursor_x").to_int(), get_scene_variable("cursor_z").to_int());
    let builds = get_scene_list("builds");
    let direction = get_scene_variable("direction").to_int();
    if builds[cell] != 0.0 {
        let facings = get_scene_list("facings");
        direction = (facings[cell].to_int() + 1) % 4;
        facings[cell] = direction.to_float();
        set_scene_list("facings", facings);
        let visuals = get_scene_list("build_visuals");
        set_rotation(visuals[cell], [0.0, -direction.to_float() * 90.0, 0.0]);
        set_scene_variable("message", "Rotated " + build_name(builds[cell]) + ".");
    } else {
        direction = (direction + 1) % 4;
    }
    set_scene_variable("direction", direction.to_float());
}

fn update_camera(dt) {
    let heading = get_scene_variable("camera_heading");
    let t = get_scene_variable("camera_progress");
    let pending = get_scene_variable("camera_pending");
    if t >= 1.0 && pending >= 1.0 {
        t = 0.0;
        pending -= 1.0;
    }
    if t < 1.0 {
        t = min(1.0, t + dt / 0.55);
        // Smootherstep has zero speed and acceleration at each end of the orbit.
        let eased = t * t * t * (t * (6.0 * t - 15.0) + 10.0);
        set_rotation("camera-rig", [0.0, heading + 90.0 * eased, 0.0]);
        if t >= 1.0 {
            heading = (heading + 90.0) % 360.0;
            set_rotation("camera-rig", [0.0, heading, 0.0]);
        }
    }
    set_scene_variable("camera_heading", heading);
    set_scene_variable("camera_progress", t);
    set_scene_variable("camera_pending", pending);
}

fn nearby_label(cell, nodes, builds) {
    let names = ["", "Iron", "Copper", "Limestone", "Coal", "Quartz", "Oil", "Water"];
    let machines = ["", "Miner", "Conveyor", "Smelter", "Storage", "Assembler", "Generator", "Splitter", "Merger"];
    let kind = builds[cell].to_int();
    let resource = names[nodes[cell].to_int()];
    if kind == 0 { resource + " deposit" }
    else if kind == 1 || kind == 6 { machines[kind] + ": " + resource }
    else if kind == 4 { "Storage  [E] Open" }
    else { machines[kind] }
}

fn update_tooltip(dt, x, z, nodes, builds) {
    let current = get_scene_variable("tooltip_cell").to_int();
    let alpha = get_scene_variable("tooltip_alpha");
    let nearest = -1;
    let best = 2.1;
    for dz in -1..2 {
        for dx in -1..2 {
            if x + dx < -7 || x + dx > 7 || z + dz < -7 || z + dz > 7 { continue; }
            let cell = index(x + dx, z + dz);
            if nodes[cell] == 0.0 && builds[cell] == 0.0 { continue; }
            let squared = (dx * dx + dz * dz).to_float();
            if squared < best || (squared == best && cell == current) {
                best = squared;
                nearest = cell;
            }
        }
    }
    if current != nearest {
        alpha = max(0.0, alpha - dt / 0.20);
        if alpha <= 0.001 {
            current = nearest;
        }
    }
    if current >= 0 && current == nearest {
        let label = nearby_label(current, nodes, builds);
        set_ui_text("nearby-tooltip", label);
        set_ui_size("nearby-tooltip", label.len().to_float() * 8.5 + 28.0, 36.0);
        let height = if builds[current] == 2.0 || builds[current] == 7.0 || builds[current] == 8.0 { 0.80 } else { 1.35 };
        set_ui_world_position("nearby-tooltip", [cell_x(current).to_float(), height, cell_z(current).to_float()]);
        alpha = min(1.0, alpha + dt / 0.18);
    }
    set_scene_variable("tooltip_cell", current.to_float());
    set_scene_variable("tooltip_alpha", alpha);
    set_ui_opacity("nearby-tooltip", alpha * alpha * (3.0 - 2.0 * alpha));
    set_ui_visible("nearby-tooltip", alpha > 0.001);
}

fn update_hud(dt, daylight) {
    let counts = get_scene_list("counts");
    let completed = counts[20] >= 8.0;
    let target = min(1.0, counts[20] / 8.0);
    let progress = get_scene_variable("objective_display");
    progress += clamp(target - progress, -dt * 0.8, dt * 0.8);
    set_scene_variable("objective_display", progress);
    set_ui_text("objective-title", if completed { "Objective met" } else { "Objective" });
    set_ui_text("objective-text", if completed { "8 machine parts delivered to storage" } else { "Deliver 8 machine parts to storage" });
    set_ui_text("objective-next", if completed { "NEXT: EXPAND YOUR FACTORY" } else { "ASSEMBLY MILESTONE" });
    set_ui_text("objective-count", min(8.0, counts[20]).to_int().to_string() + " / 8");
    set_ui_size("objective-fill", max(0.01, 356.0 * progress), 12.0);
    set_ui_visible("objective-fill", progress > 0.001);
    set_ui_text("world-status", if daylight >= 0.0 { "EARTH  /  DAY" } else { "EARTH  /  NIGHT" });
    set_ui_text("stored-iron", "Iron              " + counts[11].to_int().to_string());
    set_ui_text("stored-copper", "Copper        " + counts[12].to_int().to_string());
    set_ui_text("stored-parts", "Parts            " + counts[20].to_int().to_string());
    let supply = get_scene_variable("power_supply").to_int();
    let demand = get_scene_variable("power_demand").to_int();
    set_ui_text("power-status", (if demand > supply { "OVERLOAD  " } else { "POWER  " })
        + demand.to_string() + " / " + supply.to_string());
    let selected = get_scene_variable("selected").to_int();
    for i in 1..9 {
        set_ui_background("slot-" + i.to_string(), if i == selected {
            [0.56, 0.22, 0.09, 0.96]
        } else { [0.13, 0.19, 0.16, 0.80] });
    }
    let directions = ["East", "South", "West", "North"];
    let cell = index(get_scene_variable("cursor_x").to_int(), get_scene_variable("cursor_z").to_int());
    let builds = get_scene_list("builds");
    let facings = get_scene_list("facings");
    let kind = if builds[cell] != 0.0 { builds[cell] } else { selected.to_float() };
    let facing = if builds[cell] != 0.0 { facings[cell] } else { get_scene_variable("direction") };
    set_ui_text("build-status", build_name(kind) + "  /  Facing " + directions[facing.to_int()]);
    set_ui_text("build-message", get_scene_variable("message"));
}

fn on_start(me) {
    let seed = get_scene_variable("seed").to_int();
    begin_world(if seed > 0 { seed } else { fresh_seed() });
}

fn on_update(me, dt) {
    update_storage(dt);
    let inventory_active = get_scene_variable("storage_open") || get_scene_variable("storage_alpha") > 0.0;
    let x = get_scene_variable("cursor_x").to_int();
    let z = get_scene_variable("cursor_z").to_int();
    let dx = 0;
    let dz = 0;
    if !inventory_active {
        if input_pressed("a") { dx -= 1; }
        if input_pressed("d") { dx += 1; }
        if input_pressed("w") { dz -= 1; }
        if input_pressed("s") { dz += 1; }
    }
    // Keep movement consistent on screen after each completed camera turn.
    let view_turn = (4 - (get_scene_variable("camera_heading") / 90.0).to_int()) % 4;
    x += layout_x(dx, dz, 0, view_turn);
    z += layout_z(dx, dz, 0, view_turn);
    if x < -7 { x = -7; } else if x > 7 { x = 7; }
    if z < -7 { z = -7; } else if z > 7 { z = 7; }
    set_scene_variable("cursor_x", x.to_float());
    set_scene_variable("cursor_z", z.to_float());
    set_position("cursor", [x.to_float(), 0.115, z.to_float()]);

    if !inventory_active && input_pressed("n") {
        begin_world(fresh_seed());
        return;
    }

    for key in 1..9 {
        if !inventory_active && input_pressed(key.to_string()) {
            set_scene_variable("selected", key.to_float());
        }
    }
    if !inventory_active && input_pressed("r") {
        if input_held("Ctrl") || input_pressed("Ctrl") {
            set_scene_variable("camera_pending", get_scene_variable("camera_pending") + 1.0);
        } else {
            rotate_selected();
        }
    }
    update_camera(dt);
    if !inventory_active && input_pressed("Space") { place_selected(); }
    if !inventory_active && input_pressed("x") { remove_selected(); }

    let clock = get_scene_variable("clock") + dt;
    while clock >= 0.32 {
        factory_step();
        clock -= 0.32;
    }
    set_scene_variable("clock", clock);
    animate_items(clock / 0.32);

    // A prototype day/night cue. The authored sun stays fixed until Bozzard exposes a
    // runtime sun-direction action; exposure rises and falls without stopping production.
    let daylight = sin(elapsed_time() * 0.10);
    set_exposure(-0.28 + daylight * 0.48);

    let nodes = get_scene_list("nodes");
    let builds = get_scene_list("builds");
    update_tooltip(dt, x, z, nodes, builds);
    if inventory_active { set_ui_visible("nearby-tooltip", false); }
    update_hud(dt, daylight);
}
