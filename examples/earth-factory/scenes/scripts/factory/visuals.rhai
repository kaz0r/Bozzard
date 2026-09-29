// Rhai gameplay: visuals.
import "factory-item_fx" as item_fx;
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-power" as power;
import "factory-material_models" as material_models;

fn clear_visuals(name) {
    for target in get_scene_list(name) {
        if target != "" { destroy_prefab(target); }
    }
}

fn item_height(kind) {
    // The modeled production machines and junctions share the belt deck.
    if kind == 1.0 || data::belt(kind) || data::production(kind) || kind == 4.0 || kind == 7.0 || kind == 8.0 { 0.52 }
    else { 1.10 }
}
fn item_height(kind,item) {
    // Original cubes had a 0.10 half-height; each new silhouette rests on the
    // same deck (0.34 local + 0.08 machine lift), with a small tread clearance.
    item_height(kind)-0.10+material_models::half_height(item)+0.004
}

fn animate_items(fraction) { item_fx::animate(fraction); }

fn reset_rotations() {
    // Animation state is transient and is not stored in saves. Fresh title-menu
    // loads and co-op restores need the same full tables as a newly created world.
    set_object_list("rotation_cells", []);
    set_object_list("rotation_from", grid::empty_numbers(225));
    set_object_list("rotation_turns", grid::empty_numbers(225));
    let times = [];
    for cell in 0..225 { times.push(1.0); }
    set_object_list("rotation_time", times);
}

fn finish_rotations() {
    let facings = get_scene_list("facings");
    let visuals = get_scene_list("build_visuals");
    let turns = get_object_list("rotation_turns");
    for entry in get_object_list("rotation_cells") {
        let cell = entry.to_int();
        facings[cell] = ((facings[cell].to_int() + turns[cell].to_int()) % 4).to_float();
        if visuals[cell] != "" {
            set_position(visuals[cell], [grid::world_x(grid::cell_x(cell)), 0.08, grid::world_z(grid::cell_z(cell))]);
            set_rotation(visuals[cell], [0.0, -facings[cell] * 90.0, 0.0]);
        }
    }
    set_scene_list("facings", facings);
    power::animate_power_connections(get_object_list("rotation_cells"));
    reset_rotations();
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
        let lift = if t < 0.22 { grid::smooth(t / 0.22) }
            else if t < 0.78 { 1.0 } else { 1.0 - grid::smooth((t - 0.78) / 0.22) };
        let angle = from[cell] + grid::smooth(clamp((t - 0.22) / 0.56, 0.0, 1.0));
        set_position(visuals[cell], [grid::world_x(grid::cell_x(cell)), 0.08 + 0.55 * lift, grid::world_z(grid::cell_z(cell))]);
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
    power::animate_power_connections(cells);
}

// The landing site is a four-tile square, one block thick, centered at (0.5,0.5).
// Buildings can remain on asphalt; only the rocket and dock footprints must be clear.
fn landing_clear() {
    let builds = if get_scene_variable("chunk_x") == 0.0 && get_scene_variable("chunk_z") == 0.0 { get_scene_list("builds") }
        else { grid::unpack_numbers(data::region_data("cache_builds")[144],225) };
    builds[grid::index(0,1)] == 0.0 && builds[grid::index(2,1)] == 0.0
}

fn update_projects() {
    let phase = if get_scene_variable("demo_mode") { 0.0 } else { get_object_variable("phase") };
    if data::session_value(4) != phase {
        data::session_set(4,phase);
        for i in 0..8 { set_visible("site-" + i.to_string(),phase >= 5.0); }
        for i in 0..8 { set_visible("rocket-lower-" + i.to_string(),phase >= 6.0); }
        for i in 0..8 { set_visible("rocket-upper-" + i.to_string(),phase >= 7.0); }
        set_visible("rocket-light-0",false); set_visible("rocket-light-1",false);
    }
    let beat = if phase >= 7.0 && data::session_value(6) > 0.0 { floor(elapsed_time()*2.0) % 2.0 } else { -1.0 };
    if data::session_value(5) != beat {
        data::session_set(5,beat);
        set_visible("rocket-light-0",beat == 0.0); set_visible("rocket-light-1",beat == 1.0);
        if beat >= 0.0 {
            set_position("pole-light-31", [(beat * 2.0 - 1.0) * 0.58, 1.05+get_position("rocket-rig")[1], 1.55]);
            set_light_color("pole-light-31", if beat == 0.0 { [0.95, 0.20, 0.10] } else { [0.20, 0.95, 0.58] });
            set_light_intensity("pole-light-31", 2.5);
        }
    }
    if beat>=0.0 {
        set_position("pole-light-31",[(beat*2.0-1.0)*0.58,1.05+get_position("rocket-rig")[1],1.55]);
    }
}

fn project_label(cell) {
    if get_scene_variable("chunk_x") != 0.0 || get_scene_variable("chunk_z") != 0.0 || get_scene_variable("demo_mode") { return ""; }
    let phase = get_object_variable("phase");
    if phase >= 5.0 && cell == grid::index(2,1) { "Fuel dock / INPUT  [E] Inspect" }
    else if cell == grid::index(0,1) && phase >= 5.0 {
        if phase >= 7.0 { "Rocket 100%  [E] Enter" } else if phase >= 6.0 { "Rocket 50%  [J] Next delivery" } else { "Rocket construction site  [J] Journal" }
    } else { "" }
}

fn landing_footprint(cell) {
    grid::cell_x(cell) >= -1 && grid::cell_x(cell) <= 2 && grid::cell_z(cell) >= -1 && grid::cell_z(cell) <= 2
}
