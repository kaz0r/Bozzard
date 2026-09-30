// Rhai gameplay: hud.
import "factory-interiors" as interiors;
import "factory-renewables" as renewables;
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-inventory" as inventory;
import "factory-power" as power;
import "factory-visuals" as visuals;

fn nearby_label(cell, nodes, builds) {
    let names = ["", "Iron", "Copper", "Limestone", "Coal", "Quartz", "Oil", "Water", "Stone", "Sand", "Silver"];
    let machines = ["", "Miner", "Conveyor", "Smelter", "Storage", "Assembler", "Generator", "Splitter", "Merger", "Power pole Mk1", "Cable", "Constructor"];
    let kind = builds[cell].to_int();
    if renewables::renewable(builds[cell]) {
        let time=data::session_value(120);let planet=data::session_value(7).to_int();
        let windy=renewables::wind_active(get_scene_variable("seed").to_int(),planet,time);
        let output=renewables::supply(kind,renewables::solar_active(time,planet),windy);
        return data::build_name(builds[cell])+" / "+output.to_string()+" power"+
            (if kind==41 {" / 2 spots"}else if kind==42 {if windy {" / Wind gust"}else{" / Calm"}}else{""})+"  Ctrl+3 / cable to a pole";
    }
    if data::expansion(builds[cell]) {return data::build_name(builds[cell])+"  [E] Recipe / ingredients / collect";}
    if data::pipe(builds[cell]) {return data::build_name(builds[cell])+" / "+data::item_name(get_scene_list_item("items",cell))+" "+get_scene_list_item("item_amounts",cell).to_int().to_string()+" / 100  [E] Inspect";}
    if kind==28 || kind==29 {return data::build_name(builds[cell])+" / 90° turn";}
    let resource = if nodes[cell] >= 24.0 { data::node_name(nodes[cell]) } else { names[nodes[cell].to_int()] };
    if kind == 0 && nodes[cell] == 0.0 { let project = visuals::project_label(cell); if project != "" { project } else { "Landing pod  [J] Journal" } }
    else if kind == 0 { resource + " deposit" }
    else if kind == 5 || kind == 11 { machines[kind] + "  [E] Choose recipe / collect" }
    else if kind == 9 { "Power pole Mk1  |  " + power::pole_status(power::power_id(cell)) }
    else if kind == 1 || kind == 3 {
        let item = inventory::machine_output(cell);
        let amount = get_scene_list("item_amounts")[cell];
        let load = amount + get_scene_list("input_amounts")[cell] +
            get_scene_list("assembler_iron")[cell] + get_scene_list("assembler_copper")[cell];
        machines[kind] + "  " + load.to_int().to_string() + "/100" +
            if item > 0.0 { "  [E] Collect " + amount.to_int().to_string() + " " + data::item_name(item) }
            else { if kind == 3 { "  [E] Load ingredients" } else { "  [E] No output yet" } }
    }
    else if kind == 6 { machines[kind] + ": " + resource }
    else if kind == 4 { "Storage  [E] Open" }
    else if kind == 2 {
        let item=get_scene_list_item("items",cell);
        if data::dev_world() && item>0.0 { data::item_name(item) + "  |  Conveyor sample" }
        else { "Conveyor  |  One item in transit" }
    }
    else {
        machines[kind] + "  " + get_scene_list("item_amounts")[cell].to_int().to_string() + "/10" +
            if kind == 7 { "  |  1 input / 3 outputs" } else { "  |  3 inputs / 1 output" }
    }
}

fn update_tooltip(dt, x, z, nodes, builds) {
    let current = get_scene_variable("tooltip_cell").to_int();
    let alpha = get_scene_variable("tooltip_alpha");
    let interaction = inventory::nearby_interactable();
    let nearest = interaction;
    let best = 2.1;
    let home = get_scene_variable("chunk_x") == 0.0 && get_scene_variable("chunk_z") == 0.0;
    for dz in -1..2 {
        for dx in -1..2 {
            if interaction >= 0 { continue; }
            if x + dx < -7 || x + dx > 7 || z + dz < -7 || z + dz > 7 { continue; }
            let cell = grid::index(x + dx, z + dz);
            if nodes[cell] == 0.0 && builds[cell] == 0.0 && !(home && (cell == grid::index(0, 0) || visuals::project_label(cell) != "")) { continue; }
            if !interiors::accessible(cell) {continue;}
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
        let height = if home && current == grid::index(0,1) && get_object_variable("phase") >= 6.0 { 3.4 } else if builds[current] == 2.0 || builds[current] == 7.0 || builds[current] == 8.0 { 0.80 } else { 1.35 };
        set_ui_world_position("nearby-tooltip", [grid::world_x(grid::cell_x(current)), height, grid::world_z(grid::cell_z(current))]);
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
    let clock = get_object_variable("factory-transports","debug_clock") + dt;
    if clock < 0.25 { set_object_variable("factory-transports","debug_clock", clock); return; }
    set_object_variable("factory-transports","debug_clock", 0.0);
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
    set_ui_text("debug-triangles", "Triangles " + if stats.available { stats.triangles.to_string() } else { "--" } + "   Simulating " + data::session_value(114).to_int().to_string());
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
    let cell = grid::index(get_scene_variable("cursor_x").to_int(), get_scene_variable("cursor_z").to_int());
    let builds = get_scene_list("builds");
    let facings = get_scene_list("facings");
    let kind = if builds[cell] != 0.0 { builds[cell] } else { selected.to_float() };
    let facing = if builds[cell] != 0.0 { facings[cell] } else { get_scene_variable("direction") };
    let message = get_scene_variable("message");
    // React immediately to gameplay changes, but don't rebuild the same labels and
    // eight action-bar slots on every fixed tick. Include animated objective progress.
    let state = [counts[11], counts[12], counts[20], progress, daylight >= 0.0,
        supply, demand, selected, bar, phase, demonstration, cx, cz, kind, facing, data::on_moon(), counts[24], counts[25], counts[26]].to_string() + "|" + message;
    if !data::ui_view_changed(0, state) { return; }
    set_ui_text("objective-title", if completed { "Objective met" } else { "Objective" });
    set_ui_text("objective-text", if completed { "8 machine parts delivered to storage" } else { "Deliver 8 machine parts to storage" });
    set_ui_text("objective-next", if completed { "NEXT: EXPAND YOUR FACTORY" } else { "ASSEMBLY MILESTONE" });
    set_ui_text("objective-count", min(8.0, counts[20]).to_int().to_string() + " / 8");
    set_ui_size("objective-fill", max(0.01, 356.0 * progress), 12.0);
    set_ui_visible("objective-fill", progress > 0.001);
    set_ui_text("world-status", data::planet_name() + if daylight >= 0.0 && !data::on_moon() { " / DAY" } else { " / NIGHT" });
    set_ui_text("stored-iron", "Iron              " + counts[11].to_int().to_string());
    set_ui_text("stored-copper", "Copper        " + counts[12].to_int().to_string());
    set_ui_text("stored-parts", "Parts            " + counts[20].to_int().to_string());
    if data::on_moon() {
        set_ui_text("stored-iron", "Amorium       " + counts[24].to_int().to_string());
        set_ui_text("stored-copper", "Moondust      " + counts[25].to_int().to_string());
        set_ui_text("stored-parts", "Techtorium    " + counts[26].to_int().to_string());
    }
    set_ui_text("power-status", (if demand > supply { "OVERLOAD  " } else { "POWER  " })
        + demand.to_string() + " / " + supply.to_string());
    let tools = data::bar_tools(bar);
    let titles = ["DEMONSTRATION", "I  /  PRODUCTION", "II  /  LOGISTICS", "III  /  POWER", "IV  /  PROCESSING", "V  /  ADVANCED", "VI  /  FOUNDATIONS", "VII  /  ROOFS"];
    set_ui_text("bar-title", titles[bar]);
    for i in 1..9 {
        let kind = tools[i - 1];
        set_ui_text("slot-name-" + i.to_string(), if kind == 0.0 { "—" } else { data::tool_label(kind) });
        set_ui_text("slot-key-" + i.to_string(), i.to_string() + if kind > 0.0 && !data::tool_unlocked(kind) { "  LOCKED" } else { "" });
        set_ui_opacity("slot-" + i.to_string(), if kind == 0.0 { 0.25 } else if data::tool_unlocked(kind) { 1.0 } else { 0.48 });
        set_ui_background("slot-" + i.to_string(), if kind == selected.to_float() {
            [0.10, 0.17, 0.60, 0.96]
        } else { [0.11, 0.122, 0.19, 0.80] });
    }
    set_ui_text("chunk-status", "Region " + cx.to_string() + ", " + cz.to_string() + "  /  ±" + data::planet_radius().to_string());
    if !demonstration {
        set_ui_text("objective-text", if phase < 7 { data::phase_name(phase + 1) } else { "Rocket ready. Approach and press E." });
        set_ui_text("objective-next", "J  JOURNAL / DELIVER MATERIALS");
        set_ui_text("objective-count", (phase % 4 + 1).to_string() + " / 4");
        set_ui_text("objective-title", if phase < 4 { "First factory" } else { "Space program" });
        set_ui_size("objective-fill", max(0.01, 356.0 * (phase % 4 + 1).to_float() / 4.0), 12.0);
        set_ui_visible("objective-fill", true);
    }
    if get_object_variable("creative") {
        set_ui_text("objective-title", "Creative world");
        set_ui_text("objective-tier", "Creative");
        set_ui_text("objective-text", "All equipment and recipes unlocked");
        set_ui_text("objective-next", "BUILD FREELY / WIRE YOUR FACTORY");
        set_ui_text("objective-count", "");
    } else { set_ui_text("objective-tier", "Tier " + (phase / 4 + 1).to_string()); }
    if data::on_moon() {
        set_ui_text("objective-title", "Lunar outpost"); set_ui_text("objective-tier", if data::free_build() { "Creative" } else { "Moon" });
        set_ui_text("objective-text", "Explore for rare lunar resources");
        set_ui_text("objective-next", "M  MAP / E  BOARD ROCKET"); set_ui_text("objective-count", "");
        set_ui_visible("objective-fill", false);
    }
    if data::dev_world() {
        set_ui_text("objective-title","Dev World");set_ui_text("objective-tier","Dev");
        set_ui_text("objective-text","Deposits / machines / item samples");
        set_ui_text("objective-next","BUILD FREELY / N RESET SHOWCASE");
        set_ui_text("objective-note","Fixed 2 × 2 chunks. No generated deposits.");
        set_ui_text("chunk-status","Region " + cx.to_string() + ", " + cz.to_string() + "  /  2 × 2");
    } else { set_ui_text("objective-note","Production continues while you build."); }
    set_ui_text("controls-hint", if selected>=30 && selected<=39 {"WASD Move   Space Build piece   R Choose edge   X Remove piece"} else if selected == 10 { "Space Select / Connect   X Disconnect   R Cancel wire" } else { "WASD Move   Space Build   R Rotate machine   X Remove" });
    let directions = ["East", "South", "West", "North"];
    set_ui_text("build-status", (if kind==23.0 {"ELECTRONICS"}else{data::build_name(kind)}) + "  /  Facing " + directions[facing.to_int()]);
    set_ui_text("build-message", message);
}
