// Earth factory prototype. This is Rhai gameplay source, loaded by Bozzard's Script Manager.
// Numeric and text blackboard lists hold the factory grid across ticks and editor Play saves.

fn buffer_capacity() { 100.0 }

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
    values.pad(count, 0.0);
    values
}

fn empty_text(count) {
    let values = [];
    values.pad(count, "");
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
    let target = spawn_prefab(build_asset(kind), [world_x(x), 0.08, world_z(z)]);
    set_rotation(target, [0.0, -facing.to_float() * 90.0, 0.0]);
    target
}

fn clear_visuals(name) {
    for target in get_scene_list(name) {
        if target != "" { destroy_prefab(target); }
    }
}

fn begin_world(seed) {
    reset_planet();
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

    if get_scene_variable("demo_mode") {
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

    }

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
            if nodes[cell] == 0.0 && builds[cell] == 0.0 && cell != index(0, 0) {
                nodes[cell] = kind;
                placed = true;
                break;
            }
            tries += 1;
        }
        if !placed {
            for cell in 0..225 {
                if nodes[cell] == 0.0 && builds[cell] == 0.0 && cell != index(0, 0) {
                    nodes[cell] = kind;
                    break;
                }
            }
        }
    }

    if !get_scene_variable("demo_mode") { nodes[index(0, 0)] = 0.0; }
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
    set_scene_list("item_amounts", empty_numbers(225));
    set_scene_list("input_items", empty_numbers(225));
    set_scene_list("input_amounts", empty_numbers(225));
    set_scene_list("progress", empty_numbers(225));
    set_scene_list("assembler_iron", empty_numbers(225));
    set_scene_list("assembler_copper", empty_numbers(225));
    set_scene_list("split_state", empty_numbers(225));
    set_scene_list("counts", empty_numbers(32));
    set_scene_list("node_visuals", node_visuals);
    set_scene_list("build_visuals", build_visuals);
    set_scene_list("item_visuals", item_visuals);
    let visited = get_object_list("visited");
    visited[144] = 1.0;
    set_object_list("visited", visited);
    set_object_list("resident", visited);
    cache_put("chunk_nodes", 144, pack_numbers(nodes));
    cache_put("chunk_node_visuals", 144, pack_text(node_visuals));
    reset_interactions();
    set_scene_variable("started", true);
    set_scene_variable("seed", seed.to_float());
    let cursor_x = if get_scene_variable("demo_mode") { layout_x(-3, 1, origin_x, rotation) } else { 0 };
    let cursor_z = if get_scene_variable("demo_mode") { layout_z(-3, 1, origin_z, rotation) } else { 0 };
    set_scene_variable("cursor_x", cursor_x.to_float());
    set_scene_variable("cursor_z", cursor_z.to_float());
    set_scene_variable("clock", 0.0);
    set_scene_variable("ticks", 0.0);
    set_scene_variable("message", if get_scene_variable("demo_mode") { "Iron + copper feed the assembler. Follow the moving items to storage." } else { "Hold F on iron and copper nodes. J shows your first delivery." });
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

// E and the tooltip share a target: the closest production machine or storage,
// within one tile (including diagonals). Standing on one always wins.
fn nearby_interactable() {
    let x = get_scene_variable("cursor_x").to_int();
    let z = get_scene_variable("cursor_z").to_int();
    let builds = get_scene_list("builds");
    let best = 2.1;
    let result = -1;
    for dz in -1..2 {
        for dx in -1..2 {
            if x + dx < -7 || x + dx > 7 || z + dz < -7 || z + dz > 7 { continue; }
            let cell = index(x + dx, z + dz);
            let kind = builds[cell];
            if kind != 1.0 && kind != 3.0 && kind != 4.0 && kind != 5.0 { continue; }
            let distance = (dx * dx + dz * dz).to_float();
            if distance < best { best = distance; result = cell; }
        }
    }
    result
}

fn machine_output(cell) {
    let kind = get_scene_list("builds")[cell];
    let item = get_scene_list("items")[cell];
    if get_scene_list("item_amounts")[cell] == 0.0 { return 0.0; }
    if (kind == 1.0 && item >= 1.0 && item <= 7.0) ||
        (kind == 3.0 && (item == 11.0 || item == 12.0)) ||
        (kind == 5.0 && item == 20.0) { item } else { 0.0 }
}

fn collect_machine(cell) {
    if get_object_list("rotation_turns")[cell] > 0.0 {
        set_scene_variable("message", "Wait for the machine to finish turning.");
        return;
    }
    let item = machine_output(cell);
    if item == 0.0 {
        set_scene_variable("message", "No finished output to collect yet.");
        return;
    }
    let stock = get_object_list("stock");
    let amounts = get_scene_list("item_amounts");
    let amount = amounts[cell];
    stock[item.to_int()] += amount;
    set_object_list("stock", stock);
    let items = get_scene_list("items");
    items[cell] = 0.0;
    set_scene_list("items", items);
    amounts[cell] = 0.0;
    set_scene_list("item_amounts", amounts);
    // Cancel this item's travel before returning its visual to the pool. Otherwise
    // a reused visual could keep following the collected item's old conveyor path.
    let visuals = get_scene_list("item_visuals");
    if visuals[cell] != "" {
        let motions = get_scene_list("motion_visuals");
        for i in 0..motions.len() {
            if motions[i] == visuals[cell] { motions[i] = ""; }
        }
        set_scene_list("motion_visuals", motions);
        set_visible(visuals[cell], false);
        let pool = get_scene_list("item_pool");
        pool.push(visuals[cell]);
        set_scene_list("item_pool", pool);
        visuals[cell] = "";
        set_scene_list("item_visuals", visuals);
    }
    set_scene_variable("message", "Collected " + amount.to_int().to_string() + " " + item_name(item) + " into your backpack.  [J] Journal");
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
    if !input_pressed("E") && !get_scene_variable("storage_open") && get_scene_variable("storage_alpha") <= 0.0 { return; }
    let cell = get_scene_variable("storage_cell").to_int();
    if input_pressed("E") && !get_object_variable("journal_open") && get_object_variable("journal_alpha") <= 0.0 {
        if get_scene_variable("storage_open") { close_storage(); }
        else if get_scene_variable("storage_alpha") <= 0.0 {
            cell = nearby_interactable();
            if cell >= 0 && get_scene_list("builds")[cell] != 4.0 {
                collect_machine(cell);
            } else if cell >= 0 {
                set_scene_variable("storage_cell", cell.to_float());
                set_scene_variable("storage_open", true);
                set_ui_text("storage-title", "Storage  /  " + cell_x(cell).to_string() + ", " + cell_z(cell).to_string());
                set_ui_text("storage-help", "Drag to move or merge  •  Right-click for stack actions");
            } else {
                set_scene_variable("message", "Move next to a machine to collect output, or storage to open it.  [E]");
            }
        }
    }
    let drag = get_scene_variable("storage_drag").to_int();
    let menu = get_scene_variable("storage_menu").to_int();
    for event in ui_events() {
        if !get_scene_variable("storage_open") { break; }
        if event.kind == "activate" && event.target == "storage-take" { take_storage(cell); }
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
            lerp(world_x(cell_x(from)), world_x(cell_x(to)), fraction),
            lerp(from_y[i], to_y[i], fraction),
            lerp(world_z(cell_z(from)), world_z(cell_z(to)), fraction)
        ]);
    }
}

fn factory_step() {
    animate_items(1.0);
    // Buffer contents are numbers, not entities. Keep one representative item per
    // occupied output plus at most one traveling item per machine per beat.
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
    let amounts = get_scene_list("item_amounts");
    let inputs = get_scene_list("input_items");
    let input_amounts = get_scene_list("input_amounts");
    let machine_cells = get_scene_list("machine_cells");
    let progress = get_scene_list("progress");
    let iron = get_scene_list("assembler_iron");
    let copper = get_scene_list("assembler_copper");
    let split = get_scene_list("split_state");
    let counts = get_scene_list("counts");
    let visuals = get_scene_list("item_visuals");
    let tick = get_scene_variable("ticks").to_int() + 1;
    let rotating = get_object_list("rotation_turns");

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
            if rotating[cell] > 0.0 { continue; }
            if kind == 1.0 && nodes[cell] != 0.0 && amounts[cell] < buffer_capacity() && tick % 2 == 0 {
                items[cell] = nodes[cell];
                amounts[cell] += 1.0;
            } else if kind == 3.0 && input_amounts[cell] > 0.0 &&
                    (amounts[cell] == 0.0 || items[cell] == inputs[cell] + 10.0) {
                progress[cell] += 1.0;
                if progress[cell] >= 2.0 {
                    // One ore becomes one ingot, so processing needs no extra capacity.
                    items[cell] = inputs[cell] + 10.0;
                    amounts[cell] += 1.0;
                    input_amounts[cell] -= 1.0;
                    if input_amounts[cell] == 0.0 { inputs[cell] = 0.0; }
                    progress[cell] = 0.0;
                }
            } else if kind == 5.0 && iron[cell] >= 1.0 && copper[cell] >= 1.0 && tick % 2 == 0 {
                iron[cell] -= 1.0;
                copper[cell] -= 1.0;
                items[cell] = 20.0;
                amounts[cell] += 1.0;
            }
        }
    }

    for entry in machine_cells {
        let cell = entry.to_int();
        if amounts[cell] == 0.0 { continue; }
        if visuals[cell] == "" {
            let position = [world_x(cell_x(cell)), item_height(builds[cell]), world_z(cell_z(cell))];
            if pool.len() > 0 {
                visuals[cell] = pool.pop();
                set_position(visuals[cell], position);
                set_visible(visuals[cell], true);
            } else { visuals[cell] = spawn_prefab("item", position); }
        }
        set_color(visuals[cell], item_color(items[cell]));
    }
    // Only items present at the start of this transfer pass can move. All capacity
    // checks use the updated totals, so competing inputs cannot overfill a machine.
    let next_items = items;
    let next_amounts = amounts;
    let next_visuals = visuals;
    for entry in machine_cells {
        let cell = entry.to_int();
        let kind = builds[cell];
        let item = items[cell];
        if amounts[cell] == 0.0 || rotating[cell] > 0.0 { continue; }
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
        if rotating[destination] > 0.0 { continue; }
        let target = builds[destination];
        let load = next_amounts[destination] + input_amounts[destination] + iron[destination] + copper[destination];
        let accepted = false;
        let output_target = false;
        if target == 4.0 {
            if storage_deposit(destination, item) {
                counts[item.to_int()] += 1.0;
                accepted = true;
            }
        } else if load < buffer_capacity() {
            if target == 5.0 {
                // Leave the last space for a missing recipe ingredient. Without
                // this, 100 iron ingots could permanently keep copper out.
                if item == 11.0 && (copper[destination] > 0.0 || load < buffer_capacity() - 1.0) {
                    iron[destination] += 1.0;
                    accepted = true;
                } else if item == 12.0 && (iron[destination] > 0.0 || load < buffer_capacity() - 1.0) {
                    copper[destination] += 1.0;
                    accepted = true;
                }
            } else if target == 3.0 && (item == 1.0 || item == 2.0) &&
                    (input_amounts[destination] == 0.0 || inputs[destination] == item) {
                inputs[destination] = item;
                input_amounts[destination] += 1.0;
                accepted = true;
            } else if (target == 2.0 || target == 7.0 || target == 8.0) &&
                    (next_amounts[destination] == 0.0 || next_items[destination] == item) {
                next_items[destination] = item;
                next_amounts[destination] += 1.0;
                accepted = true;
                output_target = true;
            }
        }
        if accepted {
            next_amounts[cell] -= 1.0;
            if next_amounts[cell] == 0.0 { next_items[cell] = 0.0; }
            next_visuals[cell] = "";
            if output_target && next_visuals[destination] == "" {
                next_visuals[destination] = visuals[cell];
            } else { retired.push(visuals[cell]); }
            motion_visuals.push(visuals[cell]);
            motion_from.push(cell.to_float());
            motion_to.push(destination.to_float());
            motion_from_y.push(item_height(kind));
            motion_to_y.push(item_height(target));
            if kind == 7.0 { split[cell] = 1.0 - split[cell]; }
        }
    }
    // Sending one item from a stack leaves a single stationary representative.
    for entry in machine_cells {
        let cell = entry.to_int();
        if next_amounts[cell] == 0.0 || next_visuals[cell] != "" { continue; }
        let position = [world_x(cell_x(cell)), item_height(builds[cell]), world_z(cell_z(cell))];
        if pool.len() > 0 {
            next_visuals[cell] = pool.pop();
            set_position(next_visuals[cell], position);
            set_visible(next_visuals[cell], true);
        } else { next_visuals[cell] = spawn_prefab("item", position); }
        set_color(next_visuals[cell], item_color(next_items[cell]));
    }

    set_scene_list("motion_visuals", motion_visuals);
    set_scene_list("motion_from", motion_from);
    set_scene_list("motion_to", motion_to);
    set_scene_list("motion_from_y", motion_from_y);
    set_scene_list("motion_to_y", motion_to_y);
    set_scene_list("retired_visuals", retired);
    set_scene_list("item_pool", pool);
    set_scene_list("items", next_items);
    set_scene_list("item_amounts", next_amounts);
    set_scene_list("input_items", inputs);
    set_scene_list("input_amounts", input_amounts);
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
    if !get_scene_variable("demo_mode") && get_scene_variable("chunk_x") == 0.0 && get_scene_variable("chunk_z") == 0.0 && cell == index(0, 0) {
        set_scene_variable("message", "Keep the landing pod and workbench clear.");
        return;
    }
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
    if !pay_for_build(selected) { return; }
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
    // A demolished machine cannot leave ingredients or progress for its replacement.
    for name in ["item_amounts", "input_items", "input_amounts", "progress", "assembler_iron", "assembler_copper", "split_state"] {
        let values = get_scene_list(name); values[cell] = 0.0; set_scene_list(name, values);
    }
    let turns = get_object_list("rotation_turns");
    turns[cell] = 0.0;
    set_object_list("rotation_turns", turns);
    let times = get_object_list("rotation_time");
    times[cell] = 1.0;
    set_object_list("rotation_time", times);
    let rotating = [];
    for entry in get_object_list("rotation_cells") { if entry.to_int() != cell { rotating.push(entry); } }
    set_object_list("rotation_cells", rotating);
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
        let turns = get_object_list("rotation_turns");
        let cells = get_object_list("rotation_cells");
        if turns[cell] == 0.0 { cells.push(cell.to_float()); }
        turns[cell] = min(8.0, turns[cell] + 1.0);
        set_object_list("rotation_turns", turns);
        set_object_list("rotation_cells", cells);
        direction = (get_scene_list("facings")[cell].to_int() + turns[cell].to_int()) % 4;
        set_scene_variable("message", "Turning " + build_name(builds[cell]) + ".");
    } else {
        direction = (direction + 1) % 4;
    }
    set_scene_variable("direction", direction.to_float());
}

fn update_camera(dt) {
    let pan = get_object_variable("camera_pan_progress");
    if pan < 1.0 {
        pan = min(1.0, pan + dt / 0.55);
        let eased = pan * pan * pan * (pan * (6.0 * pan - 15.0) + 10.0);
        let from = get_object_list("camera_pan_from");
        set_position("camera-rig", [
            lerp(from[0], get_scene_variable("chunk_x") * 15.0, eased), 0.0,
            lerp(from[2], get_scene_variable("chunk_z") * 15.0, eased)
        ]);
        set_object_variable("camera_pan_progress", pan);
    }
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

fn update_zoom(dt, blocked) {
    let target = get_object_variable("camera_zoom_target");
    if !blocked {
        for event in ui_events() {
            if event.kind == "scroll" {
                target = clamp(target * pow(1.12, clamp(event.delta / 40.0, -10.0, 10.0)), 9.0, 32.0);
            }
        }
    }
    let zoom = get_object_variable("camera_zoom");
    zoom += (target - zoom) * min(1.0, dt * 12.0);
    if abs(target - zoom) < 0.001 { zoom = target; }
    set_object_variable("camera_zoom", zoom);
    set_object_variable("camera_zoom_target", target);
    set_camera_size("camera", zoom);
}

fn nearby_label(cell, nodes, builds) {
    let names = ["", "Iron", "Copper", "Limestone", "Coal", "Quartz", "Oil", "Water"];
    let machines = ["", "Miner", "Conveyor", "Smelter", "Storage", "Assembler", "Generator", "Splitter", "Merger"];
    let kind = builds[cell].to_int();
    let resource = names[nodes[cell].to_int()];
    if kind == 0 && nodes[cell] == 0.0 { "Landing pod  [J] Journal" }
    else if kind == 0 { resource + " deposit" }
    else if kind == 1 || kind == 3 || kind == 5 {
        let item = machine_output(cell);
        let amount = get_scene_list("item_amounts")[cell];
        let load = amount + get_scene_list("input_amounts")[cell] +
            get_scene_list("assembler_iron")[cell] + get_scene_list("assembler_copper")[cell];
        machines[kind] + "  " + load.to_int().to_string() + "/100" +
            if item > 0.0 { "  [E] Collect " + amount.to_int().to_string() + " " + item_name(item) }
            else { "  [E] No output yet" }
    }
    else if kind == 6 { machines[kind] + ": " + resource }
    else if kind == 4 { "Storage  [E] Open" }
    else { machines[kind] + "  " + get_scene_list("item_amounts")[cell].to_int().to_string() + "/100" }
}

fn update_tooltip(dt, x, z, nodes, builds) {
    let current = get_scene_variable("tooltip_cell").to_int();
    let alpha = get_scene_variable("tooltip_alpha");
    let interaction = nearby_interactable();
    let nearest = interaction;
    let best = 2.1;
    let home = get_scene_variable("chunk_x") == 0.0 && get_scene_variable("chunk_z") == 0.0;
    for dz in -1..2 {
        for dx in -1..2 {
            if interaction >= 0 { continue; }
            if x + dx < -7 || x + dx > 7 || z + dz < -7 || z + dz > 7 { continue; }
            let cell = index(x + dx, z + dz);
            if nodes[cell] == 0.0 && builds[cell] == 0.0 && !(home && cell == index(0, 0)) { continue; }
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
        set_ui_world_position("nearby-tooltip", [world_x(cell_x(current)), height, world_z(cell_z(current))]);
        alpha = min(1.0, alpha + dt / 0.18);
    }
    set_scene_variable("tooltip_cell", current.to_float());
    set_scene_variable("tooltip_alpha", alpha);
    set_ui_opacity("nearby-tooltip", alpha * alpha * (3.0 - 2.0 * alpha));
    set_ui_visible("nearby-tooltip", alpha > 0.001);
}

// Presentation timings come from the native renderer, never the fixed simulation dt.
// Limit text changes to four times a second so the small readout stays readable.
fn debug_ms(value) { ((value * 10.0).round() / 10.0).to_string() }
fn update_debug_hud(dt) {
    let clock = get_object_variable("debug_clock") + dt;
    if clock < 0.25 { set_object_variable("debug_clock", clock); return; }
    set_object_variable("debug_clock", 0.0);
    let explored = 0;
    for visited in get_object_list("visited") { if visited > 0.0 { explored += 1; } }
    let loaded = 0;
    for resident in get_object_list("resident") { if resident > 0.0 { loaded += 1; } }
    set_ui_text("debug-chunks", "Chunks " + loaded.to_string() + " loaded / " + explored.to_string() + " explored");
    let stats = render_stats();
    set_ui_text("debug-fps", if stats.rate_ready { stats.fps.round().to_int().to_string() + " FPS" } else { "-- FPS" });
    set_ui_text("debug-timing", "Frame " + if stats.rate_ready { debug_ms(stats.frame_ms) } else { "--" }
        + " ms   CPU draw " + if stats.available { debug_ms(stats.cpu_draw_ms) } else { "--" } + " ms");
    set_ui_text("debug-entities", "Visible entities " + if stats.available { stats.visible_entities.to_string() } else { "--" }
        + "   Draws " + if stats.available { stats.draw_calls.to_string() } else { "--" });
    set_ui_text("debug-triangles", "Triangles " + if stats.available { stats.triangles.to_string() } else { "--" } + "   Simulating 1");
    let sim = simulation_stats();
    set_ui_text("debug-simulation", "Sim " + if sim.available { if sim.threaded { "worker" } else { "main" } } else { "--" }
        + "   CPU " + if sim.available { debug_ms(sim.cpu_ms) } else { "--" }
        + " ms   Wait " + if sim.available { debug_ms(sim.wait_ms) } else { "--" } + " ms");
}

fn update_hud(dt, daylight) {
    let counts = get_scene_list("counts");
    let completed = counts[20] >= 8.0;
    let target = min(1.0, counts[20] / 8.0);
    let progress = get_scene_variable("objective_display");
    progress += clamp(target - progress, -dt * 0.8, dt * 0.8);
    set_scene_variable("objective_display", progress);
    let supply = get_scene_variable("power_supply").to_int();
    let demand = get_scene_variable("power_demand").to_int();
    let selected = get_scene_variable("selected").to_int();
    let bar = get_object_variable("bar").to_int();
    let phase = get_object_variable("phase").to_int();
    let demonstration = get_scene_variable("demo_mode");
    let cx = get_scene_variable("chunk_x").to_int();
    let cz = get_scene_variable("chunk_z").to_int();
    let cell = index(get_scene_variable("cursor_x").to_int(), get_scene_variable("cursor_z").to_int());
    let builds = get_scene_list("builds");
    let facings = get_scene_list("facings");
    let kind = if builds[cell] != 0.0 { builds[cell] } else { selected.to_float() };
    let facing = if builds[cell] != 0.0 { facings[cell] } else { get_scene_variable("direction") };
    let message = get_scene_variable("message");
    // React immediately to gameplay changes, but don't rebuild the same labels and
    // eight action-bar slots on every fixed tick. Include animated objective progress.
    let state = [counts[11], counts[12], counts[20], progress, daylight >= 0.0,
        supply, demand, selected, bar, phase, demonstration, cx, cz, kind, facing].to_string() + "|" + message;
    if state == get_object_variable("hud_state") { return; }
    set_object_variable("hud_state", state);
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
    set_ui_text("power-status", (if demand > supply { "OVERLOAD  " } else { "POWER  " })
        + demand.to_string() + " / " + supply.to_string());
    let tools = bar_tools(bar);
    let titles = ["DEMONSTRATION", "I  /  PRODUCTION", "II  /  LOGISTICS", "III  /  POWER"];
    set_ui_text("bar-title", titles[bar]);
    for i in 1..9 {
        let kind = tools[i - 1];
        set_ui_text("slot-name-" + i.to_string(), if kind == 0.0 { "—" } else { build_name(kind) });
        set_ui_text("slot-key-" + i.to_string(), i.to_string() + if kind > 0.0 && !tool_unlocked(kind) { "  LOCKED" } else { "" });
        set_ui_opacity("slot-" + i.to_string(), if kind == 0.0 { 0.25 } else if tool_unlocked(kind) { 1.0 } else { 0.48 });
        set_ui_background("slot-" + i.to_string(), if kind == selected.to_float() {
            [0.56, 0.22, 0.09, 0.96]
        } else { [0.13, 0.19, 0.16, 0.80] });
    }
    set_ui_text("chunk-status", "Region " + cx.to_string() + ", " + cz.to_string() + "  /  ±8");
    if !demonstration {
        let names = ["Unlock smelting", "Unlock generators", "Unlock miners", "Unlock belts and storage", "First factory unlocked"];
        set_ui_text("objective-text", names[min(4, phase)]);
        set_ui_text("objective-next", "J  JOURNAL / DELIVER MATERIALS");
        set_ui_text("objective-count", min(4, phase).to_string() + " / 4");
        set_ui_text("objective-title", "First factory");
        set_ui_size("objective-fill", max(0.01, 356.0 * min(4, phase).to_float() / 4.0), 12.0);
        set_ui_visible("objective-fill", phase > 0);
    }
    let directions = ["East", "South", "West", "North"];
    set_ui_text("build-status", build_name(kind) + "  /  Facing " + directions[facing.to_int()]);
    set_ui_text("build-message", message);
}

// Each region is 15 x 15 cells. Archives use bounded integer/text pages rather than
// enlarging the engine's blackboard limits. Only the region being visited simulates.
fn chunk_id(x, z) { (z + 8) * 17 + x + 8 }
fn world_x(x) { x.to_float() + get_scene_variable("chunk_x") * 15.0 }
fn world_z(z) { z.to_float() + get_scene_variable("chunk_z") * 15.0 }
fn pack_numbers(values) {
    let text = "";
    let i = 0;
    while i < values.len() {
        let end = i + 1;
        while end < values.len() && values[end] == values[i] { end += 1; }
        if end - i >= 3 {
            if text != "" { text += ","; }
            text += values[i].to_int().to_string() + ":" + (end - i).to_string();
        } else {
            for at in i..end {
                if text != "" { text += ","; }
                text += values[at].to_int().to_string();
            }
        }
        i = end;
    }
    text
}
fn unpack_numbers(text, count) {
    if text == "" { return empty_numbers(count); }
    let values = [];
    for part in text.split(",") {
        let run = part.split(":");
        let value = parse_int(run[0]).to_float();
        let length = if run.len() == 2 { parse_int(run[1]) } else { 1 };
        values.pad(values.len() + length, value);
    }
    values
}
// Only occupied cells can contain machine/storage state. Encode zero gaps in bulk
// instead of walking thousands of empty storage slots whenever we cross a seam.
// This uses the same run-length format as the other numeric chunk archives.
fn pack_cells(values, cells, stride) {
    if cells.len() == 0 { return ""; }
    let text = "";
    let next = 0;
    for entry in cells {
        let start = entry.to_int() * stride;
        if start > next {
            if text != "" { text += ","; }
            text += "0:" + (start - next).to_string();
        }
        for i in start..start + stride {
            if text != "" { text += ","; }
            text += values[i].to_int().to_string();
        }
        next = start + stride;
    }
    if next < values.len() { text += ",0:" + (values.len() - next).to_string(); }
    text
}
fn pack_text(values) {
    let any = false;
    for value in values { if value != "" { any = true; break; } }
    if !any { return ""; }
    let text = "";
    for i in 0..values.len() {
        if i > 0 { text += "|"; }
        text += values[i];
    }
    text
}
fn unpack_text(text, count) {
    if text == "" { return empty_text(count); }
    text.split("|")
}
fn cache_put(name, id, value) {
    let pages = get_object_list(name);
    pages[id] = value;
    set_object_list(name, pages);
}
fn reset_planet() {
    let active = chunk_id(get_scene_variable("chunk_x").to_int(), get_scene_variable("chunk_z").to_int());
    let visited = get_object_list("visited");
    let grounds = get_object_list("grounds");
    let nodes = get_object_list("chunk_node_visuals");
    for id in 0..visited.len() {
        if visited[id] == 0.0 { continue; }
        if grounds[id] != "" { destroy_prefab(grounds[id]); }
        if id != active {
            for target in unpack_text(nodes[id], 225) {
                if target != "" { destroy_prefab(target); }
            }
            for page in 0..3 {
                for target in unpack_text(get_object_list("cache_visuals_" + page.to_string())[id], 75) {
                    if target != "" { destroy_prefab(target); }
                }
            }
        }
    }
    set_object_list("visited", empty_numbers(289));
    set_object_list("resident", empty_numbers(289));
    set_object_variable("residency_view", "");
    for name in ["grounds", "chunk_nodes", "chunk_node_visuals", "cache_builds", "cache_facings", "cache_items", "cache_item_amounts", "cache_input_items", "cache_input_amounts", "cache_progress", "cache_assembler_iron", "cache_assembler_copper", "cache_split_state", "cache_visuals_0", "cache_visuals_1", "cache_visuals_2"] {
        set_object_list(name, empty_text(289));
    }
    for page in 0..4 {
        set_object_list("cache_storage_kinds_" + page.to_string(), empty_text(289));
        set_object_list("cache_storage_amounts_" + page.to_string(), empty_text(289));
    }
    set_scene_variable("chunk_x", 0.0);
    set_scene_variable("chunk_z", 0.0);
    set_object_variable("camera_pan_progress", 1.0);
    set_object_list("camera_pan_from", [0.0, 0.0, 0.0]);
    set_position("camera-rig", [0.0, 0.0, 0.0]);
}
fn archive_chunk() {
    finish_rotations();
    let id = chunk_id(get_scene_variable("chunk_x").to_int(), get_scene_variable("chunk_z").to_int());
    let cells = get_scene_list("machine_cells");
    cells.sort();
    let storage = [];
    let builds = get_scene_list("builds");
    for entry in cells { if builds[entry.to_int()] == 4.0 { storage.push(entry); } }
    for name in ["builds", "facings", "items", "item_amounts", "input_items", "input_amounts", "progress", "assembler_iron", "assembler_copper", "split_state"] {
        cache_put("cache_" + name, id, if cells.len() == 0 { "" } else { pack_cells(get_scene_list(name), cells, 1) });
    }
    for page in 0..4 {
        for prefix in ["storage_kinds_", "storage_amounts_"] {
            let name = prefix + page.to_string();
            cache_put("cache_" + name, id, if storage.len() == 0 { "" } else { pack_cells(get_scene_list(name), storage, 4) });
        }
    }
    let visuals = get_scene_list("build_visuals");
    for page in 0..3 {
        let part = visuals.extract(page * 75, 75);
        cache_put("cache_visuals_" + page.to_string(), id, pack_text(part));
    }
    // Reuse item models across region changes; quantities stay in the archive.
    // Avoid a burst of prefab destruction followed by spawning on the way back.
    let pool = get_scene_list("item_pool");
    for name in ["item_visuals", "retired_visuals"] {
        for target in get_scene_list(name) {
            if target != "" { set_visible(target, false); pool.push(target); }
        }
    }
    set_scene_list("item_pool", pool);
    for name in ["motion_visuals", "retired_visuals", "motion_from", "motion_to", "motion_from_y", "motion_to_y"] { set_scene_list(name, []); }
    set_scene_list("item_visuals", empty_text(225));
}
fn discover_chunk(cx, cz) {
    if cx < -8 || cx > 8 || cz < -8 || cz > 8 { return; }
    let id = chunk_id(cx, cz);
    let visited = get_object_list("visited");
    if visited[id] != 0.0 { load_chunk_visuals(cx, cz); return; }
    let state = (get_scene_variable("seed").to_int() + (id + 1) * 104729) % 2147483647;
    let nodes = empty_numbers(225);
    for kind in 1..8 {
        state = (state * 48271) % 2147483647;
        let cell = state % 225;
        while nodes[cell] != 0.0 { cell = (cell + 1) % 225; }
        nodes[cell] = kind.to_float();
    }
    cache_put("chunk_nodes", id, pack_numbers(nodes));
    visited[id] = 1.0;
    set_object_list("visited", visited);
    load_chunk_visuals(cx, cz);
}
fn load_chunk_visuals(cx, cz) {
    let id = chunk_id(cx, cz);
    let resident = get_object_list("resident");
    if resident[id] != 0.0 { return; }
    let grounds = get_object_list("grounds");
    if id != 144 {
        grounds[id] = spawn_prefab("earth-chunk", [(cx * 15).to_float(), 0.0, (cz * 15).to_float()]);
    }
    let nodes = unpack_numbers(get_object_list("chunk_nodes")[id], 225);
    let visuals = empty_text(225);
    for cell in 0..225 {
        if nodes[cell] == 0.0 { continue; }
        visuals[cell] = spawn_prefab(node_asset(nodes[cell]),
            [(cx * 15 + cell_x(cell)).to_float(), 0.08, (cz * 15 + cell_z(cell)).to_float()]);
    }
    cache_put("chunk_node_visuals", id, pack_text(visuals));
    set_object_list("grounds", grounds);
    let builds = unpack_numbers(get_object_list("cache_builds")[id], 225);
    let facings = unpack_numbers(get_object_list("cache_facings")[id], 225);
    for page in 0..3 {
        let machines = empty_text(75);
        for i in 0..75 {
            let cell = page * 75 + i;
            if builds[cell] == 0.0 { continue; }
            let target = spawn_prefab(build_asset(builds[cell]),
                [(cx * 15 + cell_x(cell)).to_float(), 0.08, (cz * 15 + cell_z(cell)).to_float()]);
            machines[i] = target;
        }
        // Keep the spawn commands together so the engine validates this page once.
        for i in 0..75 {
            if machines[i] != "" { set_rotation(machines[i], [0.0, -facings[page * 75 + i] * 90.0, 0.0]); }
        }
        cache_put("cache_visuals_" + page.to_string(), id, pack_text(machines));
    }
    resident[id] = 1.0;
    set_object_list("resident", resident);
}
fn unload_chunk_visuals(id) {
    let resident = get_object_list("resident");
    if resident[id] == 0.0 { return; }
    for target in unpack_text(get_object_list("chunk_node_visuals")[id], 225) {
        if target != "" { destroy_prefab(target); }
    }
    cache_put("chunk_node_visuals", id, "");
    for page in 0..3 {
        let name = "cache_visuals_" + page.to_string();
        for target in unpack_text(get_object_list(name)[id], 75) {
            if target != "" { destroy_prefab(target); }
        }
        cache_put(name, id, "");
    }
    let grounds = get_object_list("grounds");
    if grounds[id] != "" { destroy_prefab(grounds[id]); grounds[id] = ""; }
    set_object_list("grounds", grounds);
    resident[id] = 0.0;
    set_object_list("resident", resident);
}
fn update_chunk_residency() {
    let cx = get_scene_variable("chunk_x").to_int();
    let cz = get_scene_variable("chunk_z").to_int();
    let stats = render_stats();
    let aspect = if stats.viewport_aspect > 0.0 { stats.viewport_aspect } else { 16.0 / 9.0 };
    // Orthographic ground footprint at the fixed 35.264-degree pitch, conservative
    // through every orbit angle. Include tall models/shadows and the zoom target.
    let zoom = max(get_object_variable("camera_zoom"), get_object_variable("camera_zoom_target"));
    let half_width = zoom * aspect * 0.5;
    let half_depth = zoom * 0.8661;
    let radius = min(16, max(1, ceil((sqrt(half_width * half_width + half_depth * half_depth) + 5.0) / 15.0).to_int()));
    // Retain terrain under the moving camera as well as its destination. Rounding
    // outwards makes residency stable during the pan and covers rapid reversals.
    let camera = get_position("camera-rig");
    let min_x = min(cx, floor(camera[0] / 15.0).to_int()) - radius;
    let max_x = max(cx, ceil(camera[0] / 15.0).to_int()) + radius;
    let min_z = min(cz, floor(camera[2] / 15.0).to_int()) - radius;
    let max_z = max(cz, ceil(camera[2] / 15.0).to_int()) + radius;
    let view = [min_x, max_x, min_z, max_z].to_string();
    if view == get_object_variable("residency_view") { return; }
    let visited = get_object_list("visited");
    let resident = get_object_list("resident");
    let load = -1;
    let unload = -1;
    let distance = 1000000.0;
    for id in 0..289 {
        if visited[id] == 0.0 { continue; }
        let x = id % 17 - 8;
        let z = id / 17 - 8;
        if x >= min_x && x <= max_x && z >= min_z && z <= max_z {
            let d = abs(x.to_float() * 15.0 - camera[0]) + abs(z.to_float() * 15.0 - camera[2]);
            if resident[id] == 0.0 && d < distance { load = id; distance = d; }
        } else if resident[id] != 0.0 && unload < 0 { unload = id; }
    }
    // Apply at most one residency change per tick, closest missing terrain first.
    // Leave the view dirty until the remaining work has drained on later ticks.
    if load >= 0 { load_chunk_visuals(load % 17 - 8, load / 17 - 8); }
    else if unload >= 0 { unload_chunk_visuals(unload); }
    else { set_object_variable("residency_view", view); }
}
fn enter_chunk(cx, cz) {
    discover_chunk(cx, cz);
    archive_chunk();
    set_scene_variable("chunk_x", cx.to_float());
    set_scene_variable("chunk_z", cz.to_float());
    let id = chunk_id(cx, cz);
    set_scene_list("nodes", unpack_numbers(get_object_list("chunk_nodes")[id], 225));
    set_scene_list("node_visuals", unpack_text(get_object_list("chunk_node_visuals")[id], 225));
    for name in ["builds", "facings", "items", "item_amounts", "input_items", "input_amounts", "progress", "assembler_iron", "assembler_copper", "split_state"] {
        set_scene_list(name, unpack_numbers(get_object_list("cache_" + name)[id], 225));
    }
    for page in 0..4 {
        for prefix in ["storage_kinds_", "storage_amounts_"] {
            let name = prefix + page.to_string();
            set_scene_list(name, unpack_numbers(get_object_list("cache_" + name)[id], 900));
        }
    }
    let visuals = [];
    for page in 0..3 {
        for target in unpack_text(get_object_list("cache_visuals_" + page.to_string())[id], 75) { visuals.push(target); }
    }
    set_scene_list("build_visuals", visuals);
    let builds = get_scene_list("builds");
    let cells = [];
    for cell in 0..225 { if builds[cell] != 0.0 { cells.push(cell.to_float()); } }
    set_scene_list("machine_cells", cells);
    set_scene_variable("clock", 0.0);
    set_scene_variable("tooltip_cell", -1.0);
    set_scene_variable("tooltip_alpha", 0.0);
    set_ui_visible("nearby-tooltip", false);
    set_object_list("camera_pan_from", get_position("camera-rig"));
    set_object_variable("camera_pan_progress", 0.0);
    set_scene_variable("message", "Region " + cx.to_string() + ", " + cz.to_string() + ". Your factories remain where you built them.");
}
fn explore(x, z) {
    let cx = get_scene_variable("chunk_x").to_int();
    let cz = get_scene_variable("chunk_z").to_int();
    // Reveal ground and deposits before the cursor crosses the seam.
    if x >= 6 { discover_chunk(cx + 1, cz); }
    if x <= -6 { discover_chunk(cx - 1, cz); }
    if z >= 6 { discover_chunk(cx, cz + 1); }
    if z <= -6 { discover_chunk(cx, cz - 1); }
    let nx = cx;
    let nz = cz;
    if x > 7 && cx < 8 { nx += 1; x -= 15; }
    if x < -7 && cx > -8 { nx -= 1; x += 15; }
    if z > 7 && cz < 8 { nz += 1; z -= 15; }
    if z < -7 && cz > -8 { nz -= 1; z += 15; }
    if nx != cx || nz != cz { enter_chunk(nx, nz); }
    if x < -7 || x > 7 || z < -7 || z > 7 {
        set_scene_variable("message", "Planet boundary: eight regions from the landing site in each direction.");
    }
    [if x < -7 { -7 } else if x > 7 { 7 } else { x }, if z < -7 { -7 } else if z > 7 { 7 } else { z }]
}

fn smooth(t) { t * t * (3.0 - 2.0 * t) }
fn reset_interactions() {
    set_menu(false);
    set_map(false);
    set_object_variable("map_state", "");
    set_object_variable("camera_zoom", 19.0);
    set_object_variable("camera_zoom_target", 19.0);
    set_camera_size("camera", 19.0);
    set_object_variable("debug_clock", 0.25);
    set_object_variable("hud_state", "");
    set_object_list("rotation_cells", []);
    set_object_list("rotation_time", []);
    set_object_list("rotation_from", empty_numbers(225));
    set_object_list("rotation_turns", empty_numbers(225));
    let times = [];
    for cell in 0..225 { times.push(1.0); }
    set_object_list("rotation_time", times);
    set_object_variable("journal_open", false);
    set_object_variable("journal_alpha", 0.0);
    set_ui_visible("journal-overlay", false);
    set_object_variable("phase", if get_scene_variable("demo_mode") { 5.0 } else { 0.0 });
    set_object_variable("bar", if get_scene_variable("demo_mode") { 0.0 } else { 1.0 });
    set_object_list("bar_slots", [1.0, 1.0, 1.0]);
    set_object_list("stock", empty_numbers(32));
    set_object_variable("gather_clock", 0.0);
    set_scene_variable("selected", if get_scene_variable("demo_mode") { 1.0 } else { 3.0 });
}
fn finish_rotations() {
    let facings = get_scene_list("facings");
    let visuals = get_scene_list("build_visuals");
    let turns = get_object_list("rotation_turns");
    for entry in get_object_list("rotation_cells") {
        let cell = entry.to_int();
        facings[cell] = ((facings[cell].to_int() + turns[cell].to_int()) % 4).to_float();
        if visuals[cell] != "" {
            set_position(visuals[cell], [world_x(cell_x(cell)), 0.08, world_z(cell_z(cell))]);
            set_rotation(visuals[cell], [0.0, -facings[cell] * 90.0, 0.0]);
        }
    }
    set_scene_list("facings", facings);
    set_object_list("rotation_cells", []);
    set_object_list("rotation_turns", empty_numbers(225));
    let times = [];
    for cell in 0..225 { times.push(1.0); }
    set_object_list("rotation_time", times);
}
fn animate_machines(dt) {
    let cells = get_object_list("rotation_cells");
    if cells.len() == 0 { return; }
    let next = [];
    let times = get_object_list("rotation_time");
    let turns = get_object_list("rotation_turns");
    let from = get_object_list("rotation_from");
    let facings = get_scene_list("facings");
    let visuals = get_scene_list("build_visuals");
    for entry in cells {
        let cell = entry.to_int();
        if visuals[cell] == "" { times[cell] = 1.0; turns[cell] = 0.0; continue; }
        if times[cell] >= 1.0 { times[cell] = 0.0; from[cell] = facings[cell]; }
        let t = min(1.0, times[cell] + dt / 0.60);
        let lift = if t < 0.22 { smooth(t / 0.22) }
            else if t < 0.78 { 1.0 } else { 1.0 - smooth((t - 0.78) / 0.22) };
        let angle = from[cell] + smooth(clamp((t - 0.22) / 0.56, 0.0, 1.0));
        set_position(visuals[cell], [world_x(cell_x(cell)), 0.08 + 0.55 * lift, world_z(cell_z(cell))]);
        set_rotation(visuals[cell], [0.0, -90.0 * angle, 0.0]);
        times[cell] = t;
        if t >= 1.0 {
            facings[cell] = ((from[cell].to_int() + 1) % 4).to_float();
            turns[cell] -= 1.0;
            set_rotation(visuals[cell], [0.0, -90.0 * facings[cell], 0.0]);
        }
        if turns[cell] > 0.0 { next.push(entry); }
    }
    set_scene_list("facings", facings);
    set_object_list("rotation_cells", next);
    set_object_list("rotation_time", times);
    set_object_list("rotation_turns", turns);
    set_object_list("rotation_from", from);
}
fn tool_unlocked(kind) {
    let phase = get_object_variable("phase");
    if kind == 3.0 { phase >= 1.0 }
    else if kind == 6.0 { phase >= 2.0 }
    else if kind == 1.0 { phase >= 3.0 }
    else if kind == 2.0 || kind == 4.0 { phase >= 4.0 }
    else if kind > 0.0 { phase >= 5.0 }
    else { false }
}
fn bar_tools(bar) {
    if bar == 0 { [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0] }
    else if bar == 1 { [3.0, 1.0, 5.0, 0.0, 0.0, 0.0, 0.0, 0.0] }
    else if bar == 2 { [2.0, 4.0, 7.0, 8.0, 0.0, 0.0, 0.0, 0.0] }
    else { [6.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] }
}
fn action_bar_input() {
    let bar = get_object_variable("bar").to_int();
    let slots = get_object_list("bar_slots");
    let ctrl = input_held("Ctrl") || input_pressed("Ctrl");
    for key in 1..9 {
        if !input_pressed(key.to_string()) { continue; }
        if ctrl {
            if key <= 3 {
                bar = key;
                set_object_variable("bar", bar.to_float());
                set_scene_variable("selected", bar_tools(bar)[slots[bar - 1].to_int() - 1]);
            }
        } else {
            let kind = bar_tools(bar)[key - 1];
            if kind != 0.0 {
                set_scene_variable("selected", kind);
                if bar > 0 { slots[bar - 1] = key.to_float(); }
                if !tool_unlocked(kind) { set_scene_variable("message", "Locked. Open J to see the next delivery."); }
            }
        }
    }
    set_object_list("bar_slots", slots);
}
fn build_cost(kind) {
    // [item type, amount, item type, amount]; ore pays for the first smelter.
    if kind == 3.0 { [1, 4.0, 0, 0.0] }
    else if kind == 1.0 || kind == 6.0 { [11, 4.0, 12, 2.0] }
    else if kind == 2.0 { [11, 1.0, 0, 0.0] }
    else if kind == 4.0 { [11, 4.0, 0, 0.0] }
    else { [11, 8.0, 12, 4.0] }
}
fn pay_for_build(kind) {
    if get_scene_variable("demo_mode") { return true; }
    if !tool_unlocked(kind) {
        set_scene_variable("message", "Machine locked. J shows your unlocks and next delivery.");
        return false;
    }
    let cost = build_cost(kind);
    let stock = get_object_list("stock");
    if stock[cost[0]] < cost[1] || stock[cost[2]] < cost[3] {
        set_scene_variable("message", "Needs " + cost[1].to_int().to_string() + " " + item_name(cost[0].to_float())
            + if cost[3] > 0.0 { " + " + cost[3].to_int().to_string() + " " + item_name(cost[2].to_float()) } else { "" });
        return false;
    }
    stock[cost[0]] -= cost[1];
    stock[cost[2]] -= cost[3];
    set_object_list("stock", stock);
    true
}
fn gather(dt, blocked) {
    let clock = max(0.0, get_object_variable("gather_clock") - dt);
    if !blocked && input_held("F") && clock <= 0.0 {
        let cell = index(get_scene_variable("cursor_x").to_int(), get_scene_variable("cursor_z").to_int());
        let kind = get_scene_list("nodes")[cell].to_int();
        if kind >= 1 && kind <= 5 {
            let stock = get_object_list("stock");
            stock[kind] = min(1000000.0, stock[kind] + 1.0);
            set_object_list("stock", stock);
            set_scene_variable("message", "+1 " + item_name(kind.to_float()) + " in your backpack. J opens crafting and deliveries.");
        } else {
            set_scene_variable("message", if kind >= 6 { "Liquids need extraction equipment." } else { "Stand on a solid resource node and hold F to gather." });
        }
        clock = 0.30;
    }
    set_object_variable("gather_clock", clock);
}
fn delivery_recipe(phase) {
    if phase == 0 { [1, 4.0, 2, 4.0] }
    else if phase == 1 { [11, 4.0, 12, 4.0] }
    else if phase == 2 { [11, 8.0, 12, 4.0] }
    else { [11, 12.0, 12, 8.0] }
}
fn deliver_phase() {
    let phase = get_object_variable("phase").to_int();
    if phase >= 4 { return; }
    let recipe = delivery_recipe(phase);
    let stock = get_object_list("stock");
    if stock[recipe[0]] < recipe[1] || stock[recipe[2]] < recipe[3] { return; }
    stock[recipe[0]] -= recipe[1];
    stock[recipe[2]] -= recipe[3];
    set_object_list("stock", stock);
    set_object_variable("phase", (phase + 1).to_float());
    set_scene_variable("message", "Delivery complete! New equipment unlocked. Check your action bars.");
}
fn at_workbench() {
    get_scene_variable("chunk_x") == 0.0 && get_scene_variable("chunk_z") == 0.0 &&
        abs(get_scene_variable("cursor_x")) <= 1.0 && abs(get_scene_variable("cursor_z")) <= 1.0
}
fn craft(item) {
    if !at_workbench() { return; }
    let phase = get_object_variable("phase");
    let stock = get_object_list("stock");
    if (item == 11 || item == 12) && phase >= 1.0 && stock[item - 10] >= 1.0 {
        stock[item - 10] -= 1.0;
        stock[item] += 1.0;
    } else if item == 20 && phase >= 5.0 && stock[11] >= 1.0 && stock[12] >= 1.0 {
        stock[11] -= 1.0; stock[12] -= 1.0; stock[20] += 1.0;
    }
    set_object_list("stock", stock);
}
fn take_storage(cell) {
    let inventory = storage_read(cell);
    let stock = get_object_list("stock");
    let counts = get_scene_list("counts");
    for slot in 0..16 {
        let kind = inventory[slot * 2].to_int();
        let amount = inventory[slot * 2 + 1];
        stock[kind] += amount;
        counts[kind] -= amount;
        inventory[slot * 2] = 0.0; inventory[slot * 2 + 1] = 0.0;
    }
    set_object_list("stock", stock);
    set_scene_list("counts", counts);
    storage_write(cell, inventory);
}
fn update_journal(dt) {
    if !input_pressed("J") && !get_object_variable("journal_open") && get_object_variable("journal_alpha") <= 0.0 { return; }
    let open = get_object_variable("journal_open");
    let page = get_object_variable("journal_page").to_int();
    if input_pressed("J") {
        open = !open;
        if open { close_storage(); }
    }
    if open {
        if input_pressed("ArrowLeft") { page = max(1, page - 1); }
        if input_pressed("ArrowRight") { page = min(3, page + 1); }
        for event in ui_events() {
            if event.kind != "activate" { continue; }
            if event.target == "journal-close" { open = false; }
            for i in 1..4 { if event.target == "journal-tab-" + i.to_string() { page = i; } }
            if page == 1 && event.target == "journal-deliver" { deliver_phase(); }
            if page == 2 {
                if event.target == "journal-craft-iron" { craft(11); }
                if event.target == "journal-craft-copper" { craft(12); }
                if event.target == "journal-craft-parts" { craft(20); }
            }
        }
    }
    set_object_variable("journal_open", open);
    set_object_variable("journal_page", page.to_float());
    let alpha = clamp(get_object_variable("journal_alpha") + if open { dt / 0.20 } else { -dt / 0.16 }, 0.0, 1.0);
    set_object_variable("journal_alpha", alpha);
    set_ui_visible("journal-overlay", alpha > 0.0);
    set_ui_enabled("journal-book", open);
    set_ui_opacity("journal-overlay", smooth(alpha));
    set_ui_offset("journal-book", 0.0, (1.0 - smooth(alpha)) * 24.0);
    if alpha <= 0.0 { return; }
    let stock = get_object_list("stock");
    let phase = get_object_variable("phase").to_int();
    for i in 1..4 {
        set_ui_background("journal-tab-" + i.to_string(), if i == page { [0.64, 0.47, 0.24, 1.0] } else { [0.77, 0.67, 0.48, 1.0] });
    }
    set_ui_visible("journal-deliver", page == 1 && phase < 4);
    for name in ["journal-craft-iron", "journal-craft-copper", "journal-craft-parts"] { set_ui_visible(name, page == 2); }
    set_ui_text("journal-backpack", "BACKPACK  Ore " + stock[1].to_int().to_string() + " Fe / " + stock[2].to_int().to_string() + " Cu\nIngots " + stock[11].to_int().to_string() + " Fe / " + stock[12].to_int().to_string() + " Cu   Parts " + stock[20].to_int().to_string());
    if page == 1 {
        set_ui_text("journal-left-title", "I  /  Unlocked equipment");
        let body = "Hand tools and landing-pod workbench\n\n";
        for kind in [3.0, 6.0, 1.0, 2.0, 4.0, 5.0, 7.0, 8.0] {
            body += (if tool_unlocked(kind) { "Ready   " } else { "Locked  " }) + build_name(kind) + "\n";
        }
        set_ui_text("journal-left-body", body);
        set_ui_text("journal-right-title", if phase < 4 { "Tier 1 / Delivery " + (phase + 1).to_string() } else { "Tier 1 complete" });
        if phase < 4 {
            let recipe = delivery_recipe(phase);
            let unlocks = ["Smelter + ingot recipes", "Coal generator", "Miner", "Conveyors + storage"];
            set_ui_text("journal-right-body", "Deliver from your backpack:\n\n" + recipe[1].to_int().to_string() + " " + item_name(recipe[0].to_float()) + "\n" + recipe[3].to_int().to_string() + " " + item_name(recipe[2].to_float()) + "\n\nUnlocks: " + unlocks[phase] + "\n\nHold F on a node to gather.\nPage II shows the crafting order.");
            set_ui_enabled("journal-deliver", stock[recipe[0]] >= recipe[1] && stock[recipe[2]] >= recipe[3] && open);
        } else {
            set_ui_text("journal-right-body", "Your first automated factory is unlocked.\n\nBuild miners, smelters and conveyors.\nCollect their output in storage.\n\nAdvanced automation and the space\nprogram follow in later tiers.");
        }
    } else if page == 2 {
        set_ui_text("journal-left-title", "II  /  Recipe book");
        set_ui_text("journal-left-body", "Iron ingot  " + (if phase >= 1 { "UNLOCKED" } else { "LOCKED" }) + "\n  1 iron ore -> 1 iron ingot\n\nCopper ingot  " + (if phase >= 1 { "UNLOCKED" } else { "LOCKED" }) + "\n  1 copper ore -> 1 copper ingot\n\nMachine part  " + (if phase >= 5 { "UNLOCKED" } else { "LOCKED: Tier 2" }) + "\n  1 iron ingot + 1 copper ingot\n  -> 1 machine part");
        set_ui_text("journal-right-title", "Crafting order");
        let cost = build_cost(get_scene_variable("selected"));
        set_ui_text("journal-right-body", "1. Gather iron and copper ore (F).\n2. Deliver ore to unlock smelting.\n3. Smelt ingots at the landing pod\n    or in a placed smelter.\n4. Combine ingots in an assembler\n    after unlocking Tier 2.\n\nSelected: " + build_name(get_scene_variable("selected")) + "\nCost: " + cost[1].to_int().to_string() + " " + item_name(cost[0].to_float()) + if cost[3] > 0.0 { " + " + cost[3].to_int().to_string() + " " + item_name(cost[2].to_float()) } else { "" });
        set_ui_text("journal-subtitle", if at_workbench() { "LANDING POD WORKBENCH  /  READY" } else { "RETURN TO THE LANDING POD TO HAND-CRAFT" });
        set_ui_enabled("journal-craft-iron", open && at_workbench() && phase >= 1 && stock[1] >= 1.0);
        set_ui_enabled("journal-craft-copper", open && at_workbench() && phase >= 1 && stock[2] >= 1.0);
        set_ui_enabled("journal-craft-parts", open && at_workbench() && phase >= 5 && stock[11] >= 1.0 && stock[12] >= 1.0);
    } else {
        set_ui_text("journal-left-title", "III  /  Spaceship");
        set_ui_text("journal-left-body", "EARTH DEPARTURE\n\nHull                 Not built\nFlight systems       Not built\nLaunchpad            Not built\nFuel                 Not produced\n\nLaunch readiness     0 / 4");
        set_ui_text("journal-right-title", "The path to the stars");
        set_ui_text("journal-right-body", "1. Establish your first factory.\n    Tier 1 deliveries: " + min(4, phase).to_string() + " / 4\n2. Unlock advanced materials.\n3. Manufacture hull and systems.\n4. Build a launchpad and make fuel.\n\nThe space program is still locked.\nShip construction arrives in Tier 4.");
    }
    if page != 2 { set_ui_text("journal-subtitle", "EARTH  /  THE FIRST FACTORY"); }
}

fn on_start(me) {
    let seed = get_scene_variable("seed").to_int();
    begin_world(if seed > 0 { seed } else { fresh_seed() });
}

fn set_menu(open) {
    set_object_variable("menu_open", open);
    set_ui_visible("menu-overlay", open);
    set_ui_enabled("menu-overlay", open);
    set_ui_visible("menu-open", !open);
    set_ui_visible("map-open", !open && !get_object_variable("map_open"));
    if open {
        set_map(false);
        close_storage();
        set_scene_variable("storage_alpha", 0.0);
        set_ui_visible("storage-overlay", false);
        set_ui_enabled("storage-panel", false);
        set_object_variable("journal_open", false);
        set_object_variable("journal_alpha", 0.0);
        set_ui_visible("journal-overlay", false);
        set_ui_enabled("journal-book", false);
    }
}

fn set_map(open) {
    set_object_variable("map_open", open);
    set_ui_visible("map-overlay", open);
    set_ui_enabled("map-overlay", open);
    set_ui_visible("map-open", !open && !get_object_variable("menu_open"));
    if open {
        close_storage();
        set_scene_variable("storage_alpha", 0.0);
        set_ui_visible("storage-overlay", false);
        set_ui_enabled("storage-panel", false);
        set_object_variable("journal_open", false);
        set_object_variable("journal_alpha", 0.0);
        set_ui_visible("journal-overlay", false);
        set_ui_enabled("journal-book", false);
    }
}

fn update_map_input() {
    let was_open = get_object_variable("map_open");
    let toggle = input_pressed("M");
    for event in ui_events() {
        if event.kind == "activate" && (event.target == "map-open" || (was_open && event.target == "map-close")) {
            toggle = true;
        }
    }
    if toggle { set_map(!was_open); }
    // Closing the map consumes this frame too, so simultaneous build/move keys
    // cannot leak through to the world beneath it.
    was_open || get_object_variable("map_open")
}

fn update_map_display() {
    if !get_object_variable("map_open") { return; }
    let visited = get_object_list("visited");
    let resident = get_object_list("resident");
    let cx = get_scene_variable("chunk_x").to_int();
    let cz = get_scene_variable("chunk_z").to_int();
    let state = cx.to_string() + "," + cz.to_string() + "|" + visited.to_string() + "|" + resident.to_string();
    if state == get_object_variable("map_state") { return; }
    set_object_variable("map_state", state);
    let current = chunk_id(cx, cz);
    let loaded = 0;
    let explored = 0;
    for id in 0..289 {
        if visited[id] > 0.0 { explored += 1; }
        if resident[id] > 0.0 { loaded += 1; }
        let color = if id == current { [0.64, 0.29, 0.085, 1.0] }
            else if resident[id] > 0.0 { [0.13, 0.38, 0.26, 1.0] }
            else if visited[id] > 0.0 { [0.11, 0.17, 0.20, 1.0] }
            else { [0.018, 0.032, 0.026, 1.0] };
        set_ui_background("map-cell-" + id.to_string(), color);
    }
    set_ui_text("map-region", "Region " + cx.to_string() + ", " + cz.to_string());
    set_ui_text("map-counts", loaded.to_string() + " loaded\n" + explored.to_string() + " explored / 289 regions");
}

fn update_menu() {
    let was_open = get_object_variable("menu_open");
    let toggle = input_pressed("Escape");
    for event in ui_events() {
        if event.kind != "activate" { continue; }
        if event.target == "menu-open" || (was_open && event.target == "menu-continue") { toggle = true; }
        if was_open && event.target == "menu-exit" { quit_game(); }
    }
    if toggle { set_menu(!was_open); }
    // Also consume gameplay input on the frame Continue/Escape closes the menu.
    was_open || get_object_variable("menu_open")
}

fn sky_color(night, day, light) {
    [lerp(night[0], day[0], light), lerp(night[1], day[1], light), lerp(night[2], day[2], light)]
}

fn update_daylight(daylight) {
    // Smooth dawn/dusk over a few seconds, then hold true daylight or moonlight.
    let light = clamp(daylight * 2.5 + 0.5, 0.0, 1.0);
    light = light * light * (3.0 - 2.0 * light);
    set_sun_light(sky_color([0.36, 0.50, 0.86], [1.0, 0.94, 0.82], light), lerp(0.14, 2.6, light));
    set_ambient_light(sky_color([0.36, 0.50, 0.86], [0.86, 0.94, 1.0], light), lerp(0.035, 0.18, light));
    set_environment(
        sky_color([0.008, 0.014, 0.036], [0.14, 0.37, 0.68], light),
        sky_color([0.018, 0.028, 0.062], [0.65, 0.77, 0.86], light),
        sky_color([0.012, 0.020, 0.045], [0.24, 0.30, 0.24], light), 0.50);
    let stars = clamp((0.45 - light) / 0.45, 0.0, 1.0);
    set_star_intensity(stars * stars * (3.0 - 2.0 * stars) * 1.6);
    set_exposure(lerp(-0.15, 0.20, light));
}

fn on_update(me, dt) {
    let menu_active = update_menu();
    let map_active = if !menu_active { update_map_input() } else { false };
    if !menu_active && !map_active {
        update_journal(dt);
        update_storage(dt);
    }
    let inventory_active = menu_active || map_active || get_scene_variable("storage_open") || get_scene_variable("storage_alpha") > 0.0 || get_object_variable("journal_open") || get_object_variable("journal_alpha") > 0.0;
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
    if !inventory_active {
        let position = explore(x, z);
        x = position[0]; z = position[1];
    }
    set_scene_variable("cursor_x", x.to_float());
    set_scene_variable("cursor_z", z.to_float());
    set_position("cursor", [world_x(x), 0.115, world_z(z)]);

    if !inventory_active && input_pressed("n") {
        begin_world(fresh_seed());
        return;
    }

    if !inventory_active { action_bar_input(); }
    gather(dt, inventory_active);
    if !inventory_active && input_pressed("r") {
        if input_held("Ctrl") || input_pressed("Ctrl") {
            set_scene_variable("camera_pending", get_scene_variable("camera_pending") + 1.0);
        } else {
            rotate_selected();
        }
    }
    update_camera(dt);
    update_zoom(dt, inventory_active);
    update_chunk_residency();
    update_map_display();
    animate_machines(dt);
    if !inventory_active && input_pressed("Space") { place_selected(); }
    if !inventory_active && input_pressed("x") { remove_selected(); }

    let clock = get_scene_variable("clock") + dt;
    while clock >= 0.32 {
        factory_step();
        clock -= 0.32;
    }
    set_scene_variable("clock", clock);
    animate_items(clock / 0.32);

    let daylight = sin(elapsed_time() * 0.10);
    update_daylight(daylight);

    let nodes = get_scene_list("nodes");
    let builds = get_scene_list("builds");
    update_tooltip(dt, x, z, nodes, builds);
    if inventory_active { set_ui_visible("nearby-tooltip", false); }
    update_hud(dt, daylight);
    update_debug_hud(dt);
}
