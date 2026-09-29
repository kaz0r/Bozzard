import "factory-backpack" as backpack;
// Rhai gameplay: data.

// Earth factory prototype. This is Rhai gameplay source, loaded by Bozzard's Script Manager.
// Numeric and text blackboard lists hold the factory grid across ticks and editor Play saves.

fn buffer_capacity() { 100.0 }

fn transport_capacity(kind) { if belt(kind) { 1.0 } else if pipe(kind) { 100.0 } else { 10.0 } }
fn belt(kind) { kind==2.0 || kind==28.0 || kind==29.0 }
fn pipe(kind) { kind==26.0 || kind==27.0 }
fn expansion(kind) { kind>=12.0 && kind<=25.0 }
fn production(kind) { kind==3.0 || kind==5.0 || kind==11.0 || expansion(kind) }
fn has_slots(kind) { kind==4.0 || expansion(kind) }
fn fluid(item) { [6.0,7.0,33.0,34.0,38.0,39.0].contains(item) }
fn solid_node(item) { (item>=1.0 && item<=5.0) || (item>=8.0 && item<=10.0) || (item>=24.0 && item<=26.0) || item==40.0 || item==45.0 }
fn deposit_error(kind,node) {
    if kind==1.0 && !solid_node(node) { "A miner needs a solid deposit." }
    else if kind==6.0 && node!=4.0 { "A generator needs a coal deposit." }
    else if kind==12.0 && node!=7.0 { "A water pump needs a water deposit." }
    else if kind==13.0 && node!=6.0 { "An oil extractor needs an oil deposit." }
    else if ![1.0,6.0,12.0,13.0].contains(kind) && node!=0.0 { "Only extraction equipment can cover a deposit." }
    else { "" }
}
// The upper half of the other planet's totals occupies session[128..160].
fn other_counts() { let state=get_object_list("session"); state.extract(8,32)+state.extract(128,32) }
fn set_other_counts(counts) { let state=get_object_list("session"); for i in 0..64 { state[if i<32 {8+i}else{96+i}]=counts[i]; } set_object_list("session",state); }

// Compact UI/session state keeps the controller within 64 blackboard variables.
// 0 title mode, 1 inventory open, 2 rocket open, 3 journal recipe, 4 project view,
// 5 lamp beat, 6 powered navigation lights, 7 planet (0 Earth / 1 Moon),
// 8..39 other planet's live storage totals; 40 mouse placement, 41..43 world context menu,
// 44..45 rocket flight phase/time, 46 generic inspector cell, 47..48 inventory drag/menu,
// 49 inventory revision, 64..113 twenty-five kind/amount pairs,
// 63 persistent Dev World flag (independent of the title's selected game mode).
// 114 simulating factory regions across all planets, 115 storage UI occupied-slot count.
// 118 acknowledged native host presentation batch (local; omitted from saves).
// Inventory and unlocks travel with the player.
fn session_value(i) { get_object_list_item("session",i) }

fn session_set(i, value) { let state = get_object_list("session"); state[i] = value; set_object_list("session", state); }

fn on_moon() { session_value(7) == 1.0 }

fn dev_world() { session_value(63) == 1.0 }

fn planet_name() { if dev_world() { "DEV WORLD" } else if on_moon() { "STELLA-Z2" } else { "STELLAR-BX" } }

fn chunk_allowed(x,z) {
    if dev_world() { x>=0 && x<=1 && z>=0 && z<=1 }
    else { abs(x)<=planet_radius() && abs(z)<=planet_radius() }
}

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
    else if item == 13 || item == 20 { [11, 1.0, 12, 1.0] }
    else if item==27 { [8,1.0] } else if item==28 { [1,1.0] } else if item==29 { [2,1.0] }
    else if item==30 { [28,1.0,7,1.0] } else if item==31 { [29,1.0,7,1.0] }
    else if item==32 { [11,2.0,4,1.0] } else if item==33 { [6,3.0] }
    else if item==35 { [34,2.0] } else if item==36 { [33,2.0] } else if item==37 { [34,1.0,3,1.0] }
    else if item==38 { [7,2.0] } else if item==41 { [40,1.0] } else if item==42 { [3,1.0] }
    else if item==43 { [5,1.0] } else if item==44 { [14,2.0] }
    else if item==46 { [45,1.0,7,2.0,37,1.0] } else if item==47 { [12,1.0,43,1.0,35,1.0] }
    else if item==48 { [32,1.0,17,2.0] } else if item==49 { [48,2.0,47,2.0,32,2.0,36,2.0] }
    else if item==50 { [50,2.0] } else if item==51 { [49,1.0] }
    else if item==52 { [30,1.0] } else if item==53 { [31,1.0] }
    else { [] }
}

fn recipe_outputs(recipe) {
    if recipe==7 || recipe==6 { [recipe,1.0] }
    else if recipe==33 { [33,2.0,34,1.0] }
    else if recipe==38 { [38,2.0,39,1.0] }
    else if recipe==46 { [46,4.0,45,1.0] }
    else if recipe==49 { [49,1.0,50,1.0] }
    else if recipe==50 { [11,1.0,35,1.0] }
    else if recipe==51 { [11,2.0,12,1.0,35,1.0] }
    else if recipe==52 { [11,3.0] } else if recipe==53 { [12,3.0] }
    else if recipe==32 { [32,2.0] }
    else { [recipe,recipe_yield(recipe)] }
}
fn recipe_name(recipe) {
    if recipe==50.0 { "Recycle scrap" } else if recipe==51.0 { "Disassemble machinery" }
    else if recipe==52.0 { "Refined iron ingots" } else if recipe==53.0 { "Refined copper ingots" }
    else { item_name(recipe) }
}
fn recipe_beats(recipe) { if recipe==46 { 8.0 } else if recipe==49 { 6.0 } else if recipe>=27 { 4.0 } else { 2.0 } }
fn workbench_recipe(item) { [11,12,13,14,15,16,17,20,21].contains(item) }

fn recipe_yield(item) { if item == 16 { 4.0 } else { 1.0 } }

fn recipe_unlocked(item) {
    recipe_unlocked(item,get_object_variable("phase"),free_build())
}
fn recipe_unlocked(item,phase,creative) {
    creative || phase >= (if item>=27 || item==6 || item==7 || item == 18 || item == 20 { 4.0 } else if item == 22 { 3.0 } else { 1.0 })
}

fn recipe_choices(kind) {
    if kind == 3.0 { [11.0, 12.0, 14.0] }
    else if kind == 11.0 { [15.0, 16.0, 17.0] }
    else if kind==5.0 { [13.0,20.0,18.0] }
    else if kind==12.0 { [7.0] } else if kind==13.0 { [6.0] }
    else if kind==14.0 { [27.0,28.0,29.0] } else if kind==15.0 { [30.0,31.0] }
    else if kind==16.0 { [32.0,52.0,53.0] } else if kind==17.0 { [33.0] }
    else if kind==18.0 { [35.0,36.0,37.0] } else if kind==19.0 { [38.0] }
    else if kind==20.0 { [41.0,42.0,43.0] } else if kind==21.0 { [14.0,44.0] }
    else if kind==22.0 { [46.0] } else if kind==23.0 { [47.0] }
    else if kind==24.0 { [48.0,49.0] } else if kind==25.0 { [50.0,51.0] }
    else { [] }
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

// Facing marks the forward outlet. Storage and splitters receive only at the rear;
// belts and mergers receive at the rear and both sides, never through the outlet.
fn accepts_transport_input(kind, facing, direction) {
    if kind==28.0 { direction==(facing+1)%4 }
    else if kind==29.0 { direction==(facing+3)%4 }
    else if [14.0,15.0,20.0,21.0,22.0,23.0,25.0].contains(kind) {direction==facing}
    else if kind==16.0 {direction==facing || direction==(facing+3)%4}
    else if kind==18.0 {direction==(facing+3)%4}
    else if kind==24.0 {direction!=(facing+2)%4}
    else if expansion(kind) {false}
    else if kind == 4.0 || kind == 7.0 { direction == facing }
    else if kind == 2.0 || kind == 8.0 { direction != (facing + 2) % 4 }
    else { true }
}

fn node_asset(kind) {
    if kind==40.0 { return "node-clay"; } if kind==45.0 { return "node-seeds"; }
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
    if kind==40.0 { return "clay"; } if kind==45.0 { return "wild seeds"; }
    if kind == 1.0 { "iron" }
    else if kind == 2.0 { "copper" }
    else if kind == 3.0 { "limestone" }
    else if kind == 4.0 { "coal" }
    else if kind == 5.0 { "quartz" }
    else if kind == 6.0 { "crude oil" }
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
    if kind>=12.0 && kind<=29.0 { return "machine-"+expansion_slug(kind); }
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
    if kind>=12.0 && kind<=29.0 { return ["WATER PUMP","OIL EXTRACTOR","CRUSHER","ORE WASHER","FOUNDRY","REFINERY","CHEMICAL PLANT","ELECTROLYZER","KILN","GLASSWORKS","GREENHOUSE","ELECTRONICS FABRICATOR","MANUFACTURER","RECYCLER","PIPE","PIPE ELBOW","BELT LEFT","BELT RIGHT"][kind.to_int()-12]; }
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
fn tool_label(kind) {
    if kind==23.0 { return "ELECTRONICS\nFAB"; }
    let label=build_name(kind); label.replace(" ","\n"); label
}

fn item_name(kind) {
    if kind>=27.0 && kind<=50.0 { return ["gravel","crushed iron ore","crushed copper ore","purified iron ore","purified copper ore","steel","fuel","heavy oil","plastic","rubber","fertilizer","hydrogen","oxygen","clay","bricks","lime","silicon","lenses","seeds","biomass","circuits","motor","machinery","scrap"][kind.to_int()-27]; }
    if kind >= 24.0 && kind <= 26.0 { return node_name(kind); }
    if kind == 1.0 { "iron ore" }
    else if kind == 2.0 { "copper ore" }
    else if kind == 3.0 { "limestone" }
    else if kind == 4.0 { "coal" }
    else if kind == 5.0 { "quartz" }
    else if kind == 6.0 { "crude oil" }
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
    if fluid(kind) { return if kind==7.0 {[0.2,0.65,0.95]}else if kind==38.0 || kind==39.0 {[0.6,0.85,0.95]}else{[0.42,0.31,0.12]}; }
    if kind>=27.0 && kind<=50.0 { return if kind==45.0 || kind==46.0 || kind==37.0 {[0.35,0.75,0.35]}else if kind==40.0 || kind==41.0 {[0.78,0.38,0.24]}else if kind==47.0 {[0.15,0.62,0.44]}else if kind==35.0 || kind==36.0 {[0.28,0.3,0.32]}else{[0.67,0.75,0.79]}; }
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

fn item_model(kind) {
    // Kind 19 is unused. Keep unknown legacy/debug values renderable.
    "item-model-"+(if kind<=0.0 || kind==19.0 || kind>50.0 {8.0}else{kind}).to_int().to_string()
}

fn tool_unlocked(kind) {
    tool_unlocked(kind,get_object_variable("phase"),free_build())
}
fn tool_unlocked(kind,phase,creative) {
    if creative { return kind > 0.0 && kind<=29.0; }
    if kind == 1.0 || kind == 3.0 || kind == 2.0 || kind == 9.0 || kind == 10.0 { phase >= 1.0 }
    else if kind == 4.0 { phase >= 2.0 }
    else if kind == 6.0 { phase >= 3.0 }
    else if kind == 5.0 || kind == 11.0 || kind==7.0 || kind==8.0 || kind>=12.0 && kind<=29.0 { phase >= 4.0 }
    else { false }
}

fn bar_tools(bar) {
    if bar == 0 { [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0] }
    else if bar == 1 { [3.0, 1.0, 11.0, 5.0, 12.0, 13.0, 14.0, 15.0] }
    else if bar == 2 { [2.0, 4.0, 7.0, 8.0, 26.0, 27.0, 28.0, 29.0] }
    else if bar == 4 { [16.0,17.0,18.0,19.0,20.0,21.0,22.0,23.0] }
    else if bar == 5 { [24.0,25.0,0.0,0.0,0.0,0.0,0.0,0.0] }
    else { [6.0, 9.0, 10.0, 0.0, 0.0, 0.0, 0.0, 0.0] }
}

fn build_cost(kind) {
    if kind==26.0 || kind==27.0 { return [11,1.0,12,1.0]; }
    if kind==28.0 || kind==29.0 { return [11,1.0]; }
    if expansion(kind) { return [11,8.0,12,4.0,20,2.0]; }
    if kind == 3.0 { [1, 4.0] }
    else if kind == 1.0 { recipe_cost(21) }
    else if kind == 6.0 { recipe_cost(22) }
    else if kind == 2.0 { [11, 1.0] }
    else if kind == 4.0 { [11, 4.0] }
    else if kind == 9.0 { recipe_cost(23) }
    else if kind == 10.0 { [17, 1.0] }
    else { [11, 8.0, 12, 4.0] }
}

fn expansion_slug(kind) { ["water-pump","oil-extractor","crusher","ore-washer","foundry","refinery","chemical-plant","electrolyzer","kiln","glassworks","greenhouse","electronics-fabricator","manufacturer","recycler","pipe-straight","pipe-elbow","belt-turn-left","belt-turn-right"][kind.to_int()-12] }

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
