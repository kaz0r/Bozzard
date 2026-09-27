// Rhai gameplay: world.
import "factory-item_fx" as item_fx;
import "factory-chunks" as chunks;
import "factory-data" as data;
import "factory-deposits" as deposits;
import "factory-environment" as environment;
import "factory-grid" as grid;
import "factory-panels" as panels;
import "factory-power" as power;
import "factory-visuals" as visuals;
import "factory-transports" as transports;

fn begin_world(seed) {
    transports::clear();
    let generation=data::session_value(126)+1.0;
    let authority=data::session_value(122);
    item_fx::clear();
    power::clear_power();
    reset_planet();
    data::session_set(7, 0.0);
    let session = get_object_list("session");
    for i in 8..128 { session[i] = 0.0; }
    session[122]=authority;session[126]=generation;
    set_object_list("session", session);
    set_object_list("power_other", grid::empty_text(867));
    set_visible("ground", true);
    set_object_list("recipes", grid::empty_numbers(225));
    set_object_list("cache_recipes", grid::empty_text(578));
    panels::set_assembler(-1);
    visuals::clear_visuals("node_visuals");
    visuals::clear_visuals("build_visuals");
    set_scene_variable("tooltip_cell", -1.0);
    set_scene_variable("tooltip_alpha", 0.0);
    set_scene_variable("objective_display", 0.0);
    set_ui_visible("nearby-tooltip", false);
    for page in 0..4 {
        set_scene_list("storage_kinds_" + page.to_string(), grid::empty_numbers(900));
        set_scene_list("storage_amounts_" + page.to_string(), grid::empty_numbers(900));
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
    set_scene_list("storage_view", grid::empty_numbers(32));
    set_ui_visible("storage-overlay", false);
    set_ui_visible("stack-menu", false);
    set_ui_visible("stack-drag", false);

    let nodes = grid::empty_numbers(225);
    let builds = grid::empty_numbers(225);
    let facings = grid::empty_numbers(225);
    let node_visuals = grid::empty_text(225);
    let build_visuals = grid::empty_text(225);
    let item_visuals = grid::empty_text(225);

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
    if get_scene_variable("demo_mode") {
    nodes[grid::layout_cell(-6, 0, origin_x, origin_z, rotation)] = 1.0;
    nodes[grid::layout_cell(-6, 2, origin_x, origin_z, rotation)] = 2.0;
    nodes[grid::layout_cell(2, 1, origin_x, origin_z, rotation)] = 4.0;
    // Two working smelting lines feed an assembler, which sends parts to storage. The coal
    // generator powers the extra machines. Each new world keeps this runnable test layout.
    let iron_builds = [1.0, 2.0, 3.0, 2.0, 5.0, 4.0];
    for offset in 0..6 {
        let cell = grid::layout_cell(-6 + offset, 0, origin_x, origin_z, rotation);
        builds[cell] = iron_builds[offset];
        facings[cell] = rotation.to_float();
    }
    let copper_builds = [1.0, 2.0, 3.0, 2.0, 2.0];
    for offset in 0..5 {
        let cell = grid::layout_cell(-6 + offset, 2, origin_x, origin_z, rotation);
        builds[cell] = copper_builds[offset];
        facings[cell] = rotation.to_float();
    }
    let corner = grid::layout_cell(-2, 2, origin_x, origin_z, rotation);
    let upward_belt = grid::layout_cell(-2, 1, origin_x, origin_z, rotation);
    builds[upward_belt] = 2.0;
    facings[corner] = ((rotation + 3) % 4).to_float();
    facings[upward_belt] = ((rotation + 3) % 4).to_float();
    builds[grid::layout_cell(2, 1, origin_x, origin_z, rotation)] = 6.0;

    // The optional production test fixture retains every resource beside its
    // working machines. Survival and Creative use the sparse planet generator.
    let other_nodes = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
    for kind in other_nodes {
        let tries = 0;
        let placed = false;
        while tries < 200 {
            state = (state * 48271) % 2147483647;
            let x = (state % 13) - 6;
            state = (state * 48271) % 2147483647;
            let z = (state % 13) - 6;
            let cell = grid::index(x, z);
            if nodes[cell] == 0.0 && builds[cell] == 0.0 && !visuals::landing_footprint(cell) {
                nodes[cell] = kind;
                placed = true;
                break;
            }
            tries += 1;
        }
        if !placed {
            for cell in 0..225 {
                if nodes[cell] == 0.0 && builds[cell] == 0.0 && !visuals::landing_footprint(cell) {
                    nodes[cell] = kind;
                    break;
                }
            }
        }
    }

    } else { nodes = deposits::generate_region_nodes(seed, 0, 0); }
    for cell in 0..225 {
        let x = grid::cell_x(cell);
        let z = grid::cell_z(cell);
        if nodes[cell] != 0.0 {
            node_visuals[cell] = spawn_prefab(
                data::node_asset(nodes[cell]), [x.to_float(), 0.08, z.to_float()]
            );
        }
        if builds[cell] != 0.0 {
            build_visuals[cell] = grid::spawn_build(builds[cell], x, z, facings[cell].to_int());
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
    set_scene_list("items", grid::empty_numbers(225));
    set_scene_list("item_amounts", grid::empty_numbers(225));
    set_scene_list("input_items", grid::empty_numbers(225));
    set_scene_list("input_amounts", grid::empty_numbers(225));
    set_scene_list("progress", grid::empty_numbers(225));
    set_scene_list("assembler_iron", grid::empty_numbers(225));
    set_scene_list("assembler_copper", grid::empty_numbers(225));
    set_scene_list("split_state", grid::empty_numbers(225));
    set_scene_list("counts", grid::empty_numbers(32));
    set_scene_list("node_visuals", node_visuals);
    set_scene_list("build_visuals", build_visuals);
    set_scene_list("item_visuals", item_visuals);
    let visited = get_object_list("visited");
    visited[144] = 1.0;
    set_object_list("visited", visited);
    set_object_list("resident", visited);
    grid::cache_put("chunk_nodes", 144, grid::pack_numbers(nodes));
    grid::cache_put("chunk_node_visuals", 144, grid::pack_text(node_visuals));
    panels::reset_interactions();
    set_scene_variable("started", true);
    set_scene_variable("seed", seed.to_float());
    let cursor_x = if get_scene_variable("demo_mode") { grid::layout_x(-3, 1, origin_x, rotation) } else { 0 };
    let cursor_z = if get_scene_variable("demo_mode") { grid::layout_z(-3, 1, origin_z, rotation) } else { 0 };
    set_scene_variable("cursor_x", cursor_x.to_float());
    set_scene_variable("cursor_z", cursor_z.to_float());
    set_scene_variable("clock", 0.0);
    set_scene_variable("ticks", 0.0);
    set_scene_variable("message", if get_scene_variable("demo_mode") { "Iron + copper feed the assembler. Follow the moving items to storage." } else { "Gather iron, copper and stone [F]. J shows your first delivery." });
    set_position("cursor", [cursor_x.to_float(), 0.115, cursor_z.to_float()]);
    let recipes = grid::empty_numbers(225);
    for entry in machine_cells { if builds[entry.to_int()] == 5.0 { recipes[entry.to_int()] = 20.0; } }
    set_object_list("recipes", recipes);
    power::power_register(144 * 225 + grid::index(0, 0), 10);
    for entry in machine_cells {
        let cell = entry.to_int();
        if power::electrical(builds[cell].to_int()) { power::power_register(144 * 225 + cell, builds[cell].to_int()); }
    }
    if get_scene_variable("demo_mode") { power::wire_demo(); }
    power::update_power();
}

fn reset_planet() {
    let active = grid::chunk_id(get_scene_variable("chunk_x").to_int(), get_scene_variable("chunk_z").to_int());
    let visited = get_object_list("visited");
    let grounds = get_object_list("grounds");
    let nodes = get_object_list("chunk_node_visuals");
    for id in 0..visited.len() {
        if visited[id] == 0.0 { continue; }
        if grounds[id] != "" { destroy_prefab(grounds[id]); }
        if id != active {
            for target in grid::unpack_text(nodes[id], 225) {
                if target != "" { destroy_prefab(target); }
            }
            for page in 0..3 {
                for target in grid::unpack_text(get_object_list("cache_visuals_" + page.to_string())[id], 75) {
                    if target != "" { destroy_prefab(target); }
                }
            }
        }
    }
    set_object_list("visited", grid::empty_numbers(289));
    set_object_list("resident", grid::empty_numbers(289));
    set_object_variable("residency_view", "");
    for name in ["grounds", "chunk_nodes", "chunk_node_visuals", "cache_builds", "cache_facings", "cache_items", "cache_item_amounts", "cache_input_items", "cache_input_amounts", "cache_progress", "cache_assembler_iron", "cache_assembler_copper", "cache_split_state", "cache_visuals_0", "cache_visuals_1", "cache_visuals_2"] {
        set_object_list(name, grid::empty_text(if data::archived_region_data(name) { 578 } else { 289 }));
    }
    for page in 0..4 {
        set_object_list("cache_storage_kinds_" + page.to_string(), grid::empty_text(578));
        set_object_list("cache_storage_amounts_" + page.to_string(), grid::empty_text(578));
    }
    set_scene_variable("chunk_x", 0.0);
    set_scene_variable("chunk_z", 0.0);
    set_object_variable("camera_pan_progress", 1.0);
    set_object_list("camera_pan_from", [0.0, 0.0, 0.0]);
    set_position("camera-rig", [0.0, 0.0, 0.0]);
}

fn archive_chunk() {
    visuals::finish_rotations();
    let id = grid::chunk_id(get_scene_variable("chunk_x").to_int(), get_scene_variable("chunk_z").to_int());
    let cells = get_scene_list("machine_cells");
    cells.sort();
    grid::cache_put("cache_recipes", id, grid::pack_cells(get_object_list("recipes"), cells, 1));
    let storage = [];
    let builds = get_scene_list("builds");
    for entry in cells { if builds[entry.to_int()] == 4.0 { storage.push(entry); } }
    for name in ["builds", "facings", "items", "item_amounts", "input_items", "input_amounts", "progress", "assembler_iron", "assembler_copper", "split_state"] {
        grid::cache_put("cache_" + name, id, if cells.len() == 0 { "" } else { grid::pack_cells(get_scene_list(name), cells, 1) });
    }
    for page in 0..4 {
        for prefix in ["storage_kinds_", "storage_amounts_"] {
            let name = prefix + page.to_string();
            grid::cache_put("cache_" + name, id, if storage.len() == 0 { "" } else { grid::pack_cells(get_scene_list(name), storage, 4) });
        }
    }
    let visuals = get_scene_list("build_visuals");
    for page in 0..3 {
        let part = visuals.extract(page * 75, 75);
        grid::cache_put("cache_visuals_" + page.to_string(), id, grid::pack_text(part));
    }
    // Keep resident item models and in-flight transfers alive across the seam.
    item_fx::archive_current();
}

fn enter_chunk(cx, cz) {
    if abs(cx) > data::planet_radius() || abs(cz) > data::planet_radius() { return; }
    chunks::discover_chunk(cx, cz);
    archive_chunk();
    restore_chunk(cx, cz);
}

fn restore_chunk(cx, cz) {
    set_scene_variable("chunk_x", cx.to_float());
    set_scene_variable("chunk_z", cz.to_float());
    let id = grid::chunk_id(cx, cz);
    set_object_list("recipes", grid::unpack_numbers(data::region_data("cache_recipes")[id], 225));
    set_object_variable("power_dirty", true);
    set_scene_list("nodes", grid::unpack_numbers(data::region_data("chunk_nodes")[id], 225));
    set_scene_list("node_visuals", grid::unpack_text(get_object_list("chunk_node_visuals")[id], 225));
    for name in ["builds", "facings", "items", "item_amounts", "input_items", "input_amounts", "progress", "assembler_iron", "assembler_copper", "split_state"] {
        set_scene_list(name, grid::unpack_numbers(data::region_data("cache_" + name)[id], 225));
    }
    for page in 0..4 {
        for prefix in ["storage_kinds_", "storage_amounts_"] {
            let name = prefix + page.to_string();
            set_scene_list(name, grid::unpack_numbers(data::region_data("cache_" + name)[id], 900));
        }
    }
    let visuals = [];
    for page in 0..3 {
        for target in grid::unpack_text(get_object_list("cache_visuals_" + page.to_string())[id], 75) { visuals.push(target); }
    }
    set_scene_list("build_visuals", visuals);
    let builds = get_scene_list("builds");
    let cells = [];
    for cell in 0..225 { if builds[cell] != 0.0 { cells.push(cell.to_float()); } }
    set_scene_list("machine_cells", cells);
    item_fx::restore_current();
    set_scene_variable("tooltip_cell", -1.0);
    set_scene_variable("tooltip_alpha", 0.0);
    set_ui_visible("nearby-tooltip", false);
    set_object_list("camera_pan_from", get_position("camera-rig"));
    set_object_variable("camera_pan_progress", 0.0);
    set_scene_variable("message", "Region " + cx.to_string() + ", " + cz.to_string() + ". Your factories remain where you built them.");
}

fn travel_to_other_planet() {
    // Only boarding the completed rocket at the landing site permits a trip.
    if get_object_variable("phase") < 7.0 || get_scene_variable("demo_mode") ||
        get_scene_variable("chunk_x") != 0.0 || get_scene_variable("chunk_z") != 0.0 ||
        abs(get_scene_variable("cursor_x")) > 1.0 || abs(get_scene_variable("cursor_z") - 1.0) > 1.0 { return; }
    archive_chunk();
    // Settle pending wiring changes before retaining this planet's circuit state.
    // With no remote construction or fuel consumption, it stays valid while away.
    power::update_power();
    let resident = get_object_list("resident");
    for id in 0..289 { if resident[id] > 0.0 { chunks::unload_chunk_visuals(id); } }
    item_fx::clear();
    let outgoing_power = get_object_list("power_data");
    let incoming_power = get_object_list("power_other");
    power::clear_power();
    set_object_list("power_other", outgoing_power);
    set_object_list("power_data", incoming_power);
    let session = get_object_list("session");
    let counts = get_scene_list("counts");
    let incoming_counts = session.extract(8, 32);
    for i in 0..32 { session[8 + i] = counts[i]; }
    session[7] = 1.0 - session[7];
    session[4] = -1.0; session[5] = -2.0;
    set_object_list("session", session);
    set_scene_list("counts", incoming_counts);
    // Region archives contain only data; the departed planet keeps no live models.
    let visited = grid::empty_numbers(289);
    let nodes = data::region_data("chunk_nodes");
    let first_visit = nodes[144] == "";
    for id in 0..289 { if nodes[id] != "" { visited[id] = 1.0; } }
    set_object_list("visited", visited);
    set_object_variable("residency_view", "");
    set_scene_variable("chunk_x", 0.0); set_scene_variable("chunk_z", 0.0);
    set_visible("ground", !data::on_moon());
    chunks::discover_chunk(0, 0);
    restore_chunk(0, 0);
    if first_visit {
        power::power_register(144 * 225 + grid::index(0, 0), 10);
    }
    panels::set_extra_panel(""); panels::set_map(false); panels::set_menu(false);
    set_object_variable("map_state", "");
    set_object_list("ui_views", ["", "", "", "", "", ""]);
    set_scene_variable("cursor_x", 0.0); set_scene_variable("cursor_z", 2.0);
    set_position("cursor", [0.0, 0.115, 2.0]);
    set_position("camera-rig", [0.0, 0.0, 0.0]);
    set_object_variable("camera_pan_progress", 1.0);
    set_object_variable("gather_clock", 0.0);
    power::update_power(); visuals::update_projects();
    environment::update_daylight(if data::on_moon() { -1.0 } else { sin(data::session_value(120) * 0.10) });
    set_scene_variable("message", "Arrived on " + data::planet_name() + ". Inventory and unlocks carried over. Return by rocket [E].");
}

fn explore(x, z) {
    let cx = get_scene_variable("chunk_x").to_int();
    let cz = get_scene_variable("chunk_z").to_int();
    // Reveal ground and deposits before the cursor crosses the seam.
    if x >= 6 { chunks::discover_chunk(cx + 1, cz); }
    if x <= -6 { chunks::discover_chunk(cx - 1, cz); }
    if z >= 6 { chunks::discover_chunk(cx, cz + 1); }
    if z <= -6 { chunks::discover_chunk(cx, cz - 1); }
    let nx = cx;
    let nz = cz;
    let radius = data::planet_radius();
    if x > 7 && cx < radius { nx += 1; x -= 15; }
    if x < -7 && cx > -radius { nx -= 1; x += 15; }
    if z > 7 && cz < radius { nz += 1; z -= 15; }
    if z < -7 && cz > -radius { nz -= 1; z += 15; }
    if nx != cx || nz != cz { enter_chunk(nx, nz); }
    if x < -7 || x > 7 || z < -7 || z > 7 {
        set_scene_variable("message", "Planet boundary: " + radius.to_string() + " regions from the landing site in each direction.");
    }
    [if x < -7 { -7 } else if x > 7 { 7 } else { x }, if z < -7 { -7 } else if z > 7 { 7 } else { z }]
}
