import "factory-interiors" as interiors;
import "factory-item_fx" as item_fx;
import "factory-net_input" as net;
import "factory-backpack" as backpack;
// Rhai gameplay: inventory.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-panels" as panels;
import "factory-buffers" as buffers;

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
            let cell = grid::index(x + dx, z + dz);
            let kind = builds[cell];
            if kind!=1.0 && !data::production(kind) && kind!=4.0 && !data::pipe(kind) { continue; }
            if !interiors::accessible(cell) {continue;}
            let distance = (dx * dx + dz * dz).to_float();
            if distance < best { best = distance; result = cell; }
        }
    }
    result
}

fn machine_output(cell) {
    let kind = get_scene_list("builds")[cell];
    if data::expansion(kind) {let slots=storage_read(cell);for i in 8..16 {if slots[i*2+1]>0.0 {return slots[i*2];}}return 0.0;}
    let item = get_scene_list("items")[cell];
    if get_scene_list("item_amounts")[cell] == 0.0 { return 0.0; }
    if kind == 1.0 || kind == 3.0 || kind == 5.0 || kind == 11.0 || kind == 7.0 || kind == 8.0 { item } else { 0.0 }
}

fn collect_machine(cell) {
    if net::guest() {net::send("collect",#{at:net::position(cell)});return;}
    if get_object_list("rotation_turns")[cell] > 0.0 {
        set_scene_variable("message", "Wait for the machine to finish turning.");
        return;
    }
    if data::expansion(get_scene_list("builds")[cell]) {
        let result=buffers::collect(storage_read(cell),backpack::slots());
        if result.moved==0.0 {set_scene_variable("message","No finished output or inventory is full.");return;}
        backpack::commit(result.inventory);storage_write(cell,result.slots);
        set_scene_variable("message","Collected "+result.moved.to_int().to_string()+" products.");return;
    }
    let item = machine_output(cell);
    if item == 0.0 {
        set_scene_variable("message", "No finished output to collect yet.");
        return;
    }
    let amounts = get_scene_list("item_amounts");
    let amount = backpack::give(item,amounts[cell]);
    if amount == 0.0 { set_scene_variable("message","Inventory is full. Make room before collecting output."); return; }
    amounts[cell] -= amount; set_scene_list("item_amounts",amounts);
    set_scene_variable("message","Collected " + amount.to_int().to_string() + " " + data::item_name(item) + ".  [I] Inventory");
    if amounts[cell] > 0.0 { return; }
    let items = get_scene_list("items"); items[cell] = 0.0; set_scene_list("items",items);
    // Cancel this item's travel before returning its visual to the pool. Otherwise
    // a reused visual could keep following the collected item's old conveyor path.
    let visuals = get_scene_list("item_visuals");
    if visuals[cell] != "" {
        item_fx::cancel_motion([visuals[cell]]);
        item_fx::recycle([visuals[cell]]);
        visuals[cell] = "";
        set_scene_list("item_visuals", visuals);
    }
    set_scene_variable("message", "Collected " + amount.to_int().to_string() + " " + data::item_name(item) + " into your backpack.  [J] Journal");
}

fn inventory_slot(target) {
    for slot in 0..16 {
        if target == "inventory-slot-" + slot.to_string() { return slot; }
    }
    -1
}

fn move_stack(cell, from, to) {
    if from == to || to < 0 { return; }
    if net::guest() {net::send("storage_move",#{at:net::position(cell),from:from,to:to});return;}
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
    if net::guest() {net::send(if action=="stack-delete" {"storage_delete_kind"}else{"storage_split"},#{at:net::position(cell),slot:slot});return;}
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
        set_ui_text("storage-help", "Deleted all " + data::item_name(kind) + " from this storage.");
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
        if get_scene_variable("storage_open") { panels::close_storage(); }
        else if get_scene_variable("storage_alpha") <= 0.0 {
            cell = nearby_interactable();
            if cell >= 0 && (data::expansion(get_scene_list("builds")[cell]) || get_scene_list("builds")[cell] == 5.0 || get_scene_list("builds")[cell] == 11.0 ||
                (get_scene_list("builds")[cell] == 3.0 && machine_output(cell) == 0.0)) {
                panels::set_assembler(cell);
            } else if cell>=0 && data::pipe(get_scene_list("builds")[cell]) {
                data::session_set(46,cell.to_float());set_ui_visible("machine-inspect-overlay",true);set_ui_enabled("machine-inspect-overlay",true);
            } else if cell >= 0 && get_scene_list("builds")[cell] != 4.0 {
                collect_machine(cell);
            } else if cell >= 0 {
                set_scene_variable("storage_cell", cell.to_float());
                set_scene_variable("storage_open", true);
                set_ui_text("storage-title", "Storage  /  " + grid::cell_x(cell).to_string() + ", " + grid::cell_z(cell).to_string());
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
            if event.target == "storage-close" { panels::close_storage(); drag = -1; menu = -1; }
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
    let used = data::session_value(115).to_int();
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
            set_ui_text(name + "-name", if amount > 0 { data::item_name(kind) } else { "Empty" });
            set_ui_text(name + "-count", if amount > 0 { amount.to_string() } else { "" });
            let color = data::item_color(kind);
            set_ui_background(name + "-icon", if amount > 0 { [color[0], color[1], color[2], 1.0] } else { [0.18, 0.198, 0.31, 1.0] });
            set_ui_background(name, if slot == menu { [0.157, 0.173, 0.27, 1.0] } else { [0.087, 0.096, 0.15, 1.0] });
            set_ui_opacity(name, if slot == drag { 0.45 } else { 1.0 });
        }
        set_ui_text("storage-subtitle", used.to_string() + " / 16 slots   •   " + total.to_string() + " items   •   100 per stack");
        data::session_set(115, used.to_float());
    }
    let pointer = ui_pointer();
    set_ui_visible("stack-drag", drag >= 0 && pointer[0] >= 0.0);
    if drag >= 0 {
        set_ui_text("stack-drag", data::item_name(inventory[drag * 2]) + "\nx " + inventory[drag * 2 + 1].to_int().to_string());
        set_ui_screen_position("stack-drag", pointer[0], pointer[1]);
    }
    let menu_alpha = get_scene_variable("storage_menu_alpha");
    menu_alpha = clamp(menu_alpha + if menu >= 0 { dt / 0.12 } else { -dt / 0.10 }, 0.0, 1.0);
    set_scene_variable("storage_menu_alpha", menu_alpha);
    set_ui_visible("stack-menu", menu_alpha > 0.0);
    set_ui_opacity("stack-menu", menu_alpha * menu_alpha * (3.0 - 2.0 * menu_alpha));
    set_ui_offset("stack-menu", 0.0, 6.0 * (1.0 - menu_alpha));
    if menu >= 0 {
        set_ui_text("stack-menu-title", data::item_name(inventory[menu * 2]));
        let can_split = inventory[menu * 2 + 1] >= 2.0 && used < 16;
        set_ui_enabled("stack-split", can_split);
        set_ui_opacity("stack-split", if can_split { 1.0 } else { 0.4 });
    }
}

fn take_storage(cell) {
    if net::guest() {net::send("take_storage",#{at:net::position(cell)});return;}
    let inventory = storage_read(cell); let counts = get_scene_list("counts");
    for slot in 0..16 {
        let kind = inventory[slot*2]; let amount = inventory[slot*2+1];
        if amount <= 0.0 { continue; }
        let moved = backpack::give(kind,amount);
        counts[kind.to_int()] -= moved; inventory[slot*2+1] -= moved;
        if inventory[slot*2+1] == 0.0 { inventory[slot*2] = 0.0; }
    }
    set_scene_list("counts",counts); storage_write(cell,inventory);
}
