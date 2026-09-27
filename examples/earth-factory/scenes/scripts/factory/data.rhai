import "factory-backpack" as backpack;
// Rhai gameplay: data.

// Earth factory prototype. This is Rhai gameplay source, loaded by Bozzard's Script Manager.
// Numeric and text blackboard lists hold the factory grid across ticks and editor Play saves.

fn buffer_capacity() { 100.0 }

fn transport_capacity(kind) { if kind == 2.0 { 1.0 } else { 10.0 } }

// Compact UI/session state keeps the controller within 64 blackboard variables.
// 0 title mode, 1 inventory open, 2 rocket open, 3 journal recipe, 4 project view,
// 5 lamp beat, 6 powered navigation lights, 7 planet (0 Earth / 1 Moon),
// 8..39 other planet's live storage totals; 40 mouse placement, 41..43 world context menu,
// 44..45 rocket flight phase/time, 46 generic inspector cell, 47..48 inventory drag/menu,
// 49 inventory revision, 64..113 twenty-five kind/amount pairs,
// 114 simulating factory regions across all planets, 115 storage UI occupied-slot count.
// 118 acknowledged native host presentation batch (local; omitted from saves).
// Inventory and unlocks travel with the player.
fn session_value(i) { get_object_list_item("session",i) }

fn session_set(i, value) { let state = get_object_list("session"); state[i] = value; set_object_list("session", state); }

fn on_moon() { session_value(7) == 1.0 }

fn planet_name() { if on_moon() { "STELLA-Z2" } else { "STELLAR-BX" } }

fn planet_count() { 2 }

fn planet_radius() { planet_radius(session_value(7).to_int()) }

fn planet_radius(planet) { if planet==1 { 6 } else { 8 } }

fn archived_region_data(name) { name == "chunk_nodes" || (name.starts_with("cache_") && !name.starts_with("cache_visuals_")) }

fn region_data(name) {
    let pages = get_object_list(name);
    if archived_region_data(name) { pages.extract(if on_moon() { 289 } else { 0 }, 289) } else { pages }
}

fn region_entry(name, id) {
    get_object_list_item(name,id+if archived_region_data(name) && on_moon() { 289 } else { 0 })
}

fn planet_region_entry(name, planet, id) { get_object_list_item(name,planet*289+id) }

// Display strings are rebuilt only when their gameplay inputs change.
// Sharing one text list keeps the controller inside its blackboard variable limit.
fn ui_view_changed(slot, state) {
    let views = get_object_list("ui_views");
    if views[slot] == state { return false; }
    views[slot] = state; set_object_list("ui_views", views); true
}

fn free_build() { get_object_variable("creative") || get_scene_variable("demo_mode") }

fn phase_label(phase) { "Tier " + (phase / 4 + 1).to_string() + " / Phase " + (phase % 4 + 1).to_string() }

fn phase_name(phase) {
    ["Manual gathering", "First automation", "Storage & inventory", "Coal power", "Component automation", "Expand landing site", "Assemble rocket: 50%", "Complete rocket: 100%"][phase]
}

fn recipe_cost(item) {
    if item == 11 { [1, 1.0] } else if item == 12 { [2, 1.0] }
    else if item == 14 { [9, 1.0] } else if item == 15 || item == 16 { [11, 1.0] }
    else if item == 17 { [12, 1.0] } else if item == 18 { [8, 2.0, 3, 1.0] }
    else if item == 21 { [11, 4.0, 12, 2.0] }
    else if item == 22 { [16, 80.0, 15, 20.0, 21, 2.0, 12, 4.0] }
    else if item == 23 { [16, 20.0, 15, 4.0, 14, 2.0] }
    else if item == 13 || item == 20 { [11, 1.0, 12, 1.0] } else { [] }
}

fn recipe_yield(item) { if item == 16 { 4.0 } else { 1.0 } }

fn recipe_unlocked(item) {
    recipe_unlocked(item,get_object_variable("phase"),free_build())
}
fn recipe_unlocked(item,phase,creative) {
    creative || phase >= (if item == 18 || item == 20 { 4.0 } else if item == 22 { 3.0 } else { 1.0 })
}

fn recipe_choices(kind) {
    if kind == 3.0 { [11.0, 12.0, 14.0] }
    else if kind == 11.0 { [15.0, 16.0, 17.0] }
    else { [13.0, 20.0, 18.0] }
}

fn smelt_output(input) { if input == 1.0 { 11.0 } else if input == 2.0 { 12.0 } else if input == 9.0 { 14.0 } else { 0.0 } }

fn can_pay(stock, cost) {
    for i in 0..cost.len()/2 { if stock[cost[i*2]] < cost[i*2+1] { return false; } }
    true
}

fn spend(stock, cost) {
    for i in 0..cost.len()/2 { stock[cost[i*2]] -= cost[i*2+1]; }
    stock
}

// Facing marks the forward outlet. Splitters receive only at the rear;
// belts and mergers receive at the rear and both sides, never through the outlet.
fn accepts_transport_input(kind, facing, direction) {
    if kind == 7.0 { direction == facing }
    else if kind == 2.0 || kind == 8.0 { direction != (facing + 2) % 4 }
    else { true }
}

fn node_asset(kind) {
    if kind == 1.0 { "node-iron" }
    else if kind == 2.0 { "node-copper" }
    else if kind == 3.0 { "node-limestone" }
    else if kind == 4.0 { "node-coal" }
    else if kind == 5.0 { "node-quartz" }
    else if kind == 6.0 { "node-oil" }
    else if kind == 8.0 { "node-stone" }
    else if kind == 9.0 { "node-sand" }
    else if kind == 10.0 { "node-silver" }
    else if kind == 24.0 { "node-amorium" }
    else if kind == 25.0 { "node-moondust" }
    else if kind == 26.0 { "node-techtorium" }
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
    else if kind == 8.0 { "stone" }
    else if kind == 9.0 { "sand" }
    else if kind == 10.0 { "silver ore" }
    else if kind == 24.0 { "Amorium" }
    else if kind == 25.0 { "Moondust" }
    else if kind == 26.0 { "Techtorium" }
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
    else if kind == 9.0 { "machine-pole" }
    else if kind == 11.0 { "machine-constructor" }
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
    else if kind == 9.0 { "POLE MK1" }
    else if kind == 10.0 { "CABLE" }
    else if kind == 11.0 { "CONSTRUCTOR" }
    else { "EMPTY" }
}

fn item_name(kind) {
    if kind >= 24.0 && kind <= 26.0 { return node_name(kind); }
    if kind == 1.0 { "iron ore" }
    else if kind == 2.0 { "copper ore" }
    else if kind == 3.0 { "limestone" }
    else if kind == 4.0 { "coal" }
    else if kind == 5.0 { "quartz" }
    else if kind == 6.0 { "oil" }
    else if kind == 7.0 { "water" }
    else if kind == 11.0 { "iron ingot" }
    else if kind == 12.0 { "copper ingot" }
    else if kind == 13.0 { "conductive alloy" }
    else if kind == 14.0 { "glass" }
    else if kind == 15.0 { "iron sheet" }
    else if kind == 16.0 { "nuts and bolts" }
    else if kind == 17.0 { "cable" }
    else if kind == 18.0 { "concrete" }
    else if kind == 21.0 { "Miner Mk1" }
    else if kind == 22.0 { "coal generator" }
    else if kind == 23.0 { "power pole" }
    else if kind == 20.0 { "machine part" }
    else if kind == 8.0 { "stone" }
    else if kind == 9.0 { "sand" }
    else if kind == 10.0 { "silver ore" }
    else { "empty" }
}

fn item_color(kind) {
    if kind == 24.0 { return [0.82, 0.72, 0.51]; }
    if kind == 25.0 { return [0.83, 0.86, 0.90]; }
    if kind == 26.0 { return [1.0, 0.40, 0.08]; }
    if kind == 1.0 { [0.42, 0.65, 0.73] }
    else if kind == 2.0 { [0.92, 0.47, 0.21] }
    else if kind == 11.0 { [0.69, 0.84, 0.88] }
    else if kind == 12.0 { [0.98, 0.69, 0.36] }
    else if kind == 13.0 { [0.54, 0.94, 0.67] }
    else if kind == 20.0 { [0.80, 0.72, 1.0] }
    else { [0.91, 0.86, 0.64] }
}

fn tool_unlocked(kind) {
    tool_unlocked(kind,get_object_variable("phase"),free_build())
}
fn tool_unlocked(kind,phase,creative) {
    if creative { return kind > 0.0; }
    if kind == 1.0 || kind == 3.0 || kind == 2.0 || kind == 9.0 || kind == 10.0 { phase >= 1.0 }
    else if kind == 4.0 { phase >= 2.0 }
    else if kind == 6.0 { phase >= 3.0 }
    else if kind == 5.0 || kind == 11.0 { phase >= 4.0 }
    else { false }
}

fn bar_tools(bar) {
    if bar == 0 { [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0] }
    else if bar == 1 { [3.0, 1.0, 11.0, 5.0, 0.0, 0.0, 0.0, 0.0] }
    else if bar == 2 { [2.0, 4.0, 7.0, 8.0, 0.0, 0.0, 0.0, 0.0] }
    else { [6.0, 9.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0] }
}

fn build_cost(kind) {
    if kind == 3.0 { [1, 4.0] }
    else if kind == 1.0 { recipe_cost(21) }
    else if kind == 6.0 { recipe_cost(22) }
    else if kind == 2.0 { [11, 1.0] }
    else if kind == 4.0 { [11, 4.0] }
    else if kind == 9.0 { recipe_cost(23) }
    else if kind == 10.0 { [17, 1.0] }
    else { [11, 8.0, 12, 4.0] }
}

fn cost_label(cost) { cost_label(cost, " + ") }

fn cost_label(cost, separator) {
    let label = "";
    for i in 0..cost.len() / 2 {
        if i > 0 { label += separator; }
        label += cost[i * 2 + 1].to_int().to_string() + " " + item_name(cost[i * 2].to_float());
    }
    if label == "" { "Free connection" } else { label }
}

fn pay_for_build(kind) {
    if !tool_unlocked(kind) {
        set_scene_variable("message", "Machine locked. J shows your unlocks and next delivery.");
        return false;
    }
    if get_scene_variable("demo_mode") || get_object_variable("creative") { return true; }
    let cost = build_cost(kind);
    let stock = get_object_list("stock");
    if kind == 1.0 && stock[21] > 0.0 { stock[21] -= 1.0; backpack::set_stock( stock); return true; }
    for i in 0..cost.len() / 2 {
        if stock[cost[i * 2]] < cost[i * 2 + 1] {
            set_scene_variable("message", "Needs " + cost_label(cost)); return false;
        }
    }
    for i in 0..cost.len() / 2 { stock[cost[i * 2]] -= cost[i * 2 + 1]; }
    backpack::set_stock( stock);
    true
}

fn carried_cost_label(cost, stock) {
    let label = "";
    for i in 0..cost.len()/2 {
        if i > 0 { label += "\n"; }
        label += cost[i*2+1].to_int().to_string() + " " + item_name(cost[i*2].to_float()) + "  (have " + stock[cost[i*2]].to_int().to_string() + ")";
    }
    label
}
