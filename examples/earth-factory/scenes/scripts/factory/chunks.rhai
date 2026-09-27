// Rhai gameplay: chunks.
import "factory-item_fx" as item_fx;
import "factory-data" as data;
import "factory-deposits" as deposits;
import "factory-grid" as grid;

fn discover_chunk(cx, cz) {
    if abs(cx) > data::planet_radius() || abs(cz) > data::planet_radius() { return; }
    let id = grid::chunk_id(cx, cz);
    let visited = get_object_list("visited");
    if visited[id] != 0.0 { load_chunk_visuals(cx, cz); return; }
    let nodes = deposits::generate_region_nodes(get_scene_variable("seed").to_int(), cx, cz);
    grid::cache_put("chunk_nodes", id, grid::pack_numbers(nodes));
    visited[id] = 1.0;
    set_object_list("visited", visited);
    load_chunk_visuals(cx, cz);
}

fn load_chunk_visuals(cx, cz) {
    let id = grid::chunk_id(cx, cz);
    let resident = get_object_list("resident");
    if resident[id] != 0.0 { return; }
    let grounds = get_object_list("grounds");
    if id != 144 || data::on_moon() {
        grounds[id] = spawn_prefab(if data::on_moon() { "moon-chunk" } else { "earth-chunk" }, [(cx * 15).to_float(), 0.0, (cz * 15).to_float()]);
    }
    let nodes = grid::unpack_numbers(data::region_data("chunk_nodes")[id], 225);
    let visuals = grid::empty_text(225);
    for cell in 0..225 {
        if nodes[cell] == 0.0 { continue; }
        visuals[cell] = spawn_prefab(data::node_asset(nodes[cell]),
            [(cx * 15 + grid::cell_x(cell)).to_float(), 0.08, (cz * 15 + grid::cell_z(cell)).to_float()]);
    }
    grid::cache_put("chunk_node_visuals", id, grid::pack_text(visuals));
    set_object_list("grounds", grounds);
    let builds = grid::unpack_numbers(data::region_data("cache_builds")[id], 225);
    let facings = grid::unpack_numbers(data::region_data("cache_facings")[id], 225);
    for page in 0..3 {
        let machines = grid::empty_text(75);
        for i in 0..75 {
            let cell = page * 75 + i;
            if builds[cell] == 0.0 { continue; }
            let target = spawn_prefab(data::build_asset(builds[cell]),
                [(cx * 15 + grid::cell_x(cell)).to_float(), 0.08, (cz * 15 + grid::cell_z(cell)).to_float()]);
            machines[i] = target;
        }
        // Keep the spawn commands together so the engine validates this page once.
        for i in 0..75 {
            if machines[i] != "" { set_rotation(machines[i], [0.0, -facings[page * 75 + i] * 90.0, 0.0]); }
        }
        grid::cache_put("cache_visuals_" + page.to_string(), id, grid::pack_text(machines));
    }
    resident[id] = 1.0;
    set_object_list("resident", resident);
}

fn unload_chunk_visuals(id) {
    let resident = get_object_list("resident");
    if resident[id] == 0.0 { return; }
    item_fx::unload_region(id);
    for target in grid::unpack_text(get_object_list("chunk_node_visuals")[id], 225) {
        if target != "" { destroy_prefab(target); }
    }
    grid::cache_put("chunk_node_visuals", id, "");
    for page in 0..3 {
        let name = "cache_visuals_" + page.to_string();
        for target in grid::unpack_text(get_object_list(name)[id], 75) {
            if target != "" { destroy_prefab(target); }
        }
        grid::cache_put(name, id, "");
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
