// Rhai gameplay: building.
import "factory-interiors" as interiors;
import "factory-renewables" as renewables;
import "factory-architecture" as architecture;
import "factory-item_fx" as item_fx;
import "factory-data" as data;
import "factory-debris" as debris;
import "factory-grid" as grid;
import "factory-inventory" as inventory;
import "factory-power" as power;
import "factory-net_input" as net;

fn place_selected() {
    let x = get_scene_variable("cursor_x").to_int();
    let z = get_scene_variable("cursor_z").to_int();
    let cell = grid::index(x, z);
    let nodes = get_scene_list("nodes");
    let builds = get_scene_list("builds");
    let facings = get_scene_list("facings");
    let visuals = get_scene_list("build_visuals");
    let selected = get_scene_variable("selected");
    if architecture::structure(selected) {interiors::build(selected,false);return;}
    if selected == 10.0 { power::wire_selected(); return; }
    if net::guest() {net::send("place",#{kind:selected.to_int(),direction:get_scene_variable("direction").to_int()});return;}
    let footprint=renewables::local_error(power::power_id(cell),selected,get_scene_variable("direction").to_int(),-1);
    if footprint!="" {set_scene_variable("message",footprint);return;}
    if debris::blocked(x,z) {
        set_scene_variable("message", "Keep the spaceship wreckage clear.");
        return;
    }
    if !get_scene_variable("demo_mode") && get_scene_variable("chunk_x") == 0.0 && get_scene_variable("chunk_z") == 0.0 && (cell == grid::index(0, 0) || (get_object_variable("phase") >= 5.0 && (cell == grid::index(0, 1) || cell == grid::index(2, 1)))) {
        set_scene_variable("message", "Keep the landing pod, rocket and fuel dock clear.");
        return;
    }
    if builds[cell] != 0.0 {
        set_scene_variable("message", "Remove the existing machine with X first.");
        return;
    }
    let error=data::deposit_error(selected,nodes[cell]);
    if error!="" {set_scene_variable("message",error);return;}
    if !data::pay_for_build(selected) { return; }
    let facing = get_scene_variable("direction").to_int();
    let machine_cells = get_scene_list("machine_cells");
    machine_cells.push(cell.to_float());
    set_scene_list("machine_cells", machine_cells);
    builds[cell] = selected;
    facings[cell] = facing.to_float();
    visuals[cell] = grid::spawn_build(selected, x, z, facing);
    set_scene_list("builds", builds);
    set_scene_list("facings", facings);
    set_scene_list("build_visuals", visuals);
    if data::production(selected) {
        let recipes = get_object_list("recipes"); recipes[cell] = data::recipe_choices(selected)[0]; set_object_list("recipes", recipes);
    }
    if power::electrical(selected.to_int()) { power::power_register(power::power_id(cell), selected.to_int()); }
    set_scene_variable("message", "Built " + data::build_name(selected) + "." + if power::electrical(selected.to_int()) { " Ctrl+3, slot 3: cable to a pole." } else { "" });
}

fn remove_selected() {
    let x = get_scene_variable("cursor_x").to_int();
    let z = get_scene_variable("cursor_z").to_int();
    let cell = grid::index(x, z);
    let builds = get_scene_list("builds");
    if architecture::structure(get_scene_variable("selected")) {interiors::build(get_scene_variable("selected"),true);return;}
    if get_scene_variable("selected") == 10.0 { power::disconnect_selected(); return; }
    if net::guest() {net::send("remove",#{});return;}
    let owner=renewables::local_owner(power::power_id(cell));
    if renewables::array_kind(owner)==41.0 {
        power::power_remove(owner);renewables::edit_array(owner,true);
        set_scene_variable("message","Solar array removed. Both spots are clear.");return;
    }
    if builds[cell] == 0.0 { return; }
    power::power_remove(power::power_id(cell));
    let recipes = get_object_list("recipes"); recipes[cell] = 0.0; set_object_list("recipes", recipes);
    if builds[cell] == 4.0 {
        let inventory = inventory::storage_read(cell);
        let counts = get_scene_list("counts");
        for slot in 0..16 {
            counts[inventory[slot * 2].to_int()] -= inventory[slot * 2 + 1];
        }
        set_scene_list("counts", counts);
        inventory::storage_write(cell, grid::empty_numbers(32));
    }
    if data::expansion(builds[cell]) {inventory::storage_write(cell,grid::empty_numbers(32));}
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
        item_fx::cancel_motion([item_visuals[cell]]);
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
    if architecture::structure(get_scene_variable("selected")) {set_scene_variable("direction",((get_scene_variable("direction").to_int()+1)%4).to_float());return;}
    import "factory-host_view" as host_view;
    let id=grid::current_chunk()*225+grid::index(get_scene_variable("cursor_x").to_int(),get_scene_variable("cursor_z").to_int());
    if host_view::rotating(data::session_value(7).to_int(),id) { set_scene_variable("message","Wait for the machine to finish turning."); return; }
    let cell = grid::index(get_scene_variable("cursor_x").to_int(), get_scene_variable("cursor_z").to_int());
    let owner=renewables::local_owner(power::power_id(cell));
    if renewables::array_kind(owner)==41.0 {
        if net::guest() {net::send("rotate",#{});return;}
        let facing=(renewables::array_facing(owner).to_int()+1)%4;
        let error=renewables::local_error(owner,41.0,facing,owner);
        if error!="" {set_scene_variable("message",error);return;}
        renewables::edit_array(owner,false);set_object_variable("power_dirty",true);
        set_scene_variable("direction",facing.to_float());
        set_scene_variable("message","Solar array turned. Both spots reserved.");return;
    }
    let builds = get_scene_list("builds");
    let direction = get_scene_variable("direction").to_int();
    if builds[cell] != 0.0 {
        if net::guest() {net::send("rotate",#{});set_scene_variable("direction",((get_scene_list("facings")[cell].to_int()+1)%4).to_float());return;}
        let turns = get_object_list("rotation_turns");
        let cells = get_object_list("rotation_cells");
        if turns[cell] == 0.0 { cells.push(cell.to_float()); }
        turns[cell] = min(8.0, turns[cell] + 1.0);
        set_object_list("rotation_turns", turns);
        set_object_list("rotation_cells", cells);
        direction = (get_scene_list("facings")[cell].to_int() + turns[cell].to_int()) % 4;
        set_scene_variable("message", "Turning " + data::build_name(builds[cell]) + ".");
    } else {
        direction = (direction + 1) % 4;
    }
    set_scene_variable("direction", direction.to_float());
}

fn action_bar_input() {
    let bar = get_object_variable("bar").to_int();
    let slots = get_object_list("bar_slots");
    let ctrl = input_held("Ctrl") || input_pressed("Ctrl");
    for key in 1..9 {
        if !input_pressed(key.to_string()) { continue; }
        if ctrl {
            if key <= 7 {
                bar = key;
                set_object_variable("bar", bar.to_float());
                set_scene_variable("selected", data::bar_tools(bar)[if bar<=3 {slots[bar-1].to_int()-1}else{0}]);
            }
        } else {
            let kind = data::bar_tools(bar)[key - 1];
            if kind != 0.0 {
                set_scene_variable("selected", kind);
                if bar > 0 && bar<=3 { slots[bar - 1] = key.to_float(); }
                if !data::tool_unlocked(kind) { set_scene_variable("message", "Locked. Open J to see the next delivery."); }
            }
        }
    }
    for event in ui_events() {
        if event.kind!="activate" { continue; }
        for key in 1..9 {
            if event.target!="slot-"+key.to_string() { continue; }
            let kind=data::bar_tools(bar)[key-1];
            if kind==0.0 { continue; }
            set_scene_variable("selected",kind);
            if bar>0 && bar<=3 { slots[bar-1]=key.to_float(); }
            data::session_set(40,if data::tool_unlocked(kind) { 1.0 } else { 0.0 });
            clear_ui_focus();
            set_scene_variable("message",if data::tool_unlocked(kind) { "Click a highlighted tile to build. Right-click cancels." } else { "Locked. Open J to see the next delivery." });
        }
    }
    set_object_list("bar_slots", slots);
    if get_scene_variable("selected") != 10.0 { set_object_variable("wire_start", -1.0); }
}
