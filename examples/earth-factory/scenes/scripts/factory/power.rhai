import "factory-backpack" as backpack;
import "factory-net_input" as net;
// Rhai gameplay: power.
import "factory-data" as data;
import "factory-grid" as grid;

fn electrical(kind) { kind == 1 || kind == 3 || kind == 5 || kind == 6 || kind == 9 || kind == 10 || kind == 11 }

fn power_id(cell) { grid::chunk_id(get_scene_variable("chunk_x").to_int(), get_scene_variable("chunk_z").to_int()) * 225 + cell }

fn power_position(id) {
    let chunk = id / 225;
    let cell = id % 225;
    [((chunk % 17 - 8) * 15 + grid::cell_x(cell)).to_float(), 1.47, ((chunk / 17 - 8) * 15 + grid::cell_z(cell)).to_float()]
}

fn power_graph() {
    power_graph("power_data")
}

fn power_graph(name) {
    let graph = #{};
    let pages = get_object_list(name);
    for page in 0..pages.len() {
        if pages[page] == "" { continue; }
        let cells = pages[page].split("|");
        for i in 0..cells.len() {
            if cells[i] != "" {
                let entry = [];
                for value in cells[i].split(",") { entry.push(parse_int(value)); }
                graph[(page * 75 + i).to_string()] = entry;
            }
        }
    }
    graph
}

fn write_power(graph) {
    let pages = grid::empty_text(867);
    let decoded = #{};
    for key in graph.keys() {
        let id = parse_int(key);
        let page = (id / 75).to_string();
        if !decoded.contains(page) { decoded[page] = grid::empty_text(75); }
        let values = graph[key];
        let text = "";
        for value in values { if text != "" { text += ","; } text += value.to_string(); }
        decoded[page][id % 75] = text;
    }
    for page in decoded.keys() { pages[parse_int(page)] = grid::pack_text(decoded[page]); }
    set_object_list("power_data", pages);
}

fn power_register(id, kind) {
    let graph = power_graph();
    graph[id.to_string()] = [kind, 0];
    write_power(graph);
    set_object_variable("power_dirty", true);
}

fn power_remove(id) {
    let graph = power_graph();
    let key = id.to_string();
    if !graph.contains(key) { return; }
    for peer in graph[key].extract(2) {
        let other = peer.to_string();
        if !graph.contains(other) { continue; }
        let entry = graph[other].extract(0, 2);
        for target in graph[other].extract(2) { if target != id { entry.push(target); } }
        graph[other] = entry;
    }
    graph.remove(key);
    write_power(graph);
    if get_object_variable("wire_start").to_int() == id { set_object_variable("wire_start", -1.0); }
    set_object_variable("power_dirty", true);
}

fn connect_power(a, b) {
    let graph = power_graph();
    let error=connection_error(graph,a,b); if error!="" { return error; }
    let ak = a.to_string(); let bk = b.to_string();
    let left = graph[ak]; let right = graph[bk];
    left.push(b); right.push(a);
    graph[ak] = left; graph[bk] = right;
    write_power(graph);
    set_object_variable("power_dirty", true);
    ""
}
fn connection_error(graph,a,b) {
    let ak=a.to_string(); let bk=b.to_string();
    if a == b || !graph.contains(ak) || !graph.contains(bk) { return "Choose two different power terminals."; }
    let left=graph[ak]; let right=graph[bk];
    if left[0] != 9 && right[0] != 9 { return "A cable needs a power pole at one end."; }
    if left.extract(2).contains(b) { return "These terminals are already connected."; }
    if left.len() - 2 >= (if left[0] == 9 { 5 } else { 1 }) || right.len() - 2 >= (if right[0] == 9 { 5 } else { 1 }) {
        return "No free port: poles allow 5 cables; machines and sources allow 1.";
    }
    ""
}

fn wire_selected() {
    if !data::tool_unlocked(10.0) { set_scene_variable("message", "Unlock smelting to use poles and cables."); return; }
    let id = power_id(grid::index(get_scene_variable("cursor_x").to_int(), get_scene_variable("cursor_z").to_int()));
    let graph = power_graph();
    if !graph.contains(id.to_string()) { set_scene_variable("message", "Stand on a pole, powered machine, generator or landing pod."); return; }
    let start = get_object_variable("wire_start").to_int();
    if start < 0 {
        set_object_variable("wire_start", id.to_float());
        set_scene_variable("message", "Cable start set. Walk to the other terminal; Space connects. R cancels.");
    } else {
        if net::guest() {if net::send("wire",#{from:net::address(start),to:net::address(id)}) {set_object_variable("wire_start",-1.0);}return;}
        let stock = get_object_list("stock");
        if !data::free_build() && stock[17] < 1.0 { set_scene_variable("message", "Needs 1 cable. Craft copper ingots into cable in J."); return; }
        let error = connect_power(start, id);
        if error == "" {
            if !data::free_build() { stock[17] -= 1.0; backpack::set_stock( stock); }
            set_object_variable("wire_start", -1.0);
            set_scene_variable("message", "Cable connected. Space selects another terminal; X disconnects this one.");
        } else { set_scene_variable("message", error); }
    }
}

// Remove one cable, or all cables at a terminal, without demolishing either end.
fn disconnect_power(id, peer) {
    if net::guest() {net::send("unwire",#{from:net::address(id),to:if peer<0 {()}else{net::address(peer)}});return;}
    let graph=power_graph(); let key=id.to_string(); if !graph.contains(key) { return; }
    let kept=graph[key].extract(0,2);
    for other in graph[key].extract(2) {
        if peer>=0 && other!=peer { kept.push(other); continue; }
        let other_key=other.to_string(); if !graph.contains(other_key) { continue; }
        let connections=graph[other_key].extract(0,2);
        for target in graph[other_key].extract(2) { if target!=id { connections.push(target); } }
        graph[other_key]=connections;
    }
    graph[key]=kept; write_power(graph); set_object_variable("power_dirty",true);
    set_scene_variable("message","Cable unlinked. Machines and poles remain in place.");
}

fn disconnect_selected() {
    let id = power_id(grid::index(get_scene_variable("cursor_x").to_int(), get_scene_variable("cursor_z").to_int()));
    if net::guest() {net::send("unwire",#{from:net::address(id),to:()});return;}
    let graph = power_graph();
    if !graph.contains(id.to_string()) { return; }
    let kind = graph[id.to_string()][0];
    power_remove(id);
    power_register(id, kind);
    set_scene_variable("message", "Cables disconnected. The machine remains.");
}

fn pole_status(id) {
    let entry = power_entry(get_object_list("power_data"),id);
    if entry.len() == 0 { return "0 / 5 cables"; }
    (entry.len() - 2).to_string() + " / 5 cables  " + if entry[1] > 0 { "Powered" } else { "No power / overload" }
}

fn power_demand(kind) { if kind == 1 { 1 } else if kind == 3 { 2 } else if kind == 5 { 3 } else if kind == 11 { 2 } else { 0 } }

fn resolve_graph(graph) {
    let visited = #{};
    let total_supply = 0;
    let total_demand = 0;
    for start in graph.keys() {
        if visited.contains(start) { continue; }
        let queue = [start];
        let members = [];
        let supply = 0; let demand = 0;
        visited[start] = true;
        let at = 0;
        while at < queue.len() {
            let key = queue[at]; at += 1;
            let entry = graph[key]; members.push(key);
            if entry[0] == 10 { supply += 8; }
            if entry[0] == 6 { supply += 10; }
            demand += power_demand(entry[0]);
            for target in entry.extract(2) {
                let peer = target.to_string();
                if graph.contains(peer) && !visited.contains(peer) { visited[peer] = true; queue.push(peer); }
            }
        }
        let powered = supply > 0 && supply >= demand;
        for key in members { graph[key][1] = if powered { 1 } else { 0 }; }
        total_supply += supply; total_demand += demand;
    }
    #{graph:graph,supply:total_supply,demand:total_demand}
}

fn update_power() {
    let dirty = get_object_variable("power_dirty");
    let aspect = render_stats().viewport_aspect;
    let view = get_object_list("resident").to_string() + "|" + get_scene_variable("chunk_x").to_string() + "," + get_scene_variable("chunk_z").to_string() + "|" + ceil(aspect).to_string() + "|" + get_object_variable("phase").to_string();
    if !dirty && view == get_object_list("ui_views")[4] { return; }
    let graph = power_graph();
    if dirty {
        let resolved=resolve_graph(graph); graph=resolved.graph;
        let total_supply=resolved.supply; let total_demand=resolved.demand;
        write_power(graph);
        let live = grid::empty_numbers(225);
        let base = power_id(0);
        for key in graph.keys() {
            let id = parse_int(key);
            if id >= base && id < base + 225 { live[id - base] = graph[key][1].to_float(); }
        }
        set_object_list("power_live", live);
        set_scene_variable("power_supply", total_supply.to_float());
        set_scene_variable("power_demand", total_demand.to_float());
        set_object_variable("power_dirty", false);
    }
    // The landing pod supplies this circuit itself; overload or removing its
    // power terminal also disables navigation lamps on the finished rocket.
    let pod = (144 * 225 + grid::index(0,0)).to_string();
    let navigation = !get_scene_variable("demo_mode") && get_object_variable("phase") >= 7.0 &&
        get_object_list("resident")[144] > 0.0 && graph.contains(pod) && graph[pod][1] > 0;
    data::session_set(6, if navigation { 1.0 } else { 0.0 });
    sync_power_visuals(graph);
    // A grid refresh may hand the pooled light between a pole and the rocket.
    data::session_set(5, -2.0);
    data::ui_view_changed(4, view);
}

// Effects are reconciled by endpoint, so wiring one machine never rebuilds the
// existing grid. Fifteen cells per text page stay below the blackboard byte limit.
fn read_power_fx() {
    let fx = #{};
    for group in 0..5 {
        for page in get_object_list("power_fx_" + group.to_string()) {
            if page == "" { continue; }
            for record in page.split("|") {
                let fields = record.split(";");
                let entry = #{};
                for i in 1..fields.len() {
                    let pair = fields[i].split(",");
                    entry[pair[0]] = pair[1];
                }
                fx[fields[0]] = entry;
            }
        }
    }
    fx
}

fn sync_power_visuals(graph) {
    let old = read_power_fx();
    let next = #{};
    let resident = get_object_list("resident");
    let lamps = [];
    let aspect = render_stats().viewport_aspect;
    if aspect <= 0.0 { aspect = 16.0 / 9.0; }
    // Cover maximum zoom, every orbit angle and the complete chunk pan. A long
    // cable may cross the view even when both endpoint chunks have unloaded.
    let reach = 32.0 * sqrt(aspect * aspect * 0.25 + 0.75) + 25.0;
    let cx = get_scene_variable("chunk_x") * 15.0;
    let cz = get_scene_variable("chunk_z") * 15.0;
    for key in graph.keys() {
        let id = parse_int(key);
        let entry = graph[key];
        let effects = #{};
        let previous = if old.contains(key) { old[key] } else { #{} };
        let pos = power_anchor(id, entry[0]);
        if entry[0] == 9 && resident[id / 225] > 0.0 {
            let lamp = if previous.contains("lamp") { previous["lamp"] } else { spawn_prefab("power-lamp", [pos[0], pos[1] + 0.43, pos[2]]) };
            set_position(lamp, [pos[0], pos[1] + 0.43, pos[2]]);
            set_visible(lamp, entry[1] > 0);
            effects["lamp"] = lamp;
            if entry[1] > 0 { lamps.push(id); }
        }
        for peer in entry.extract(2) {
            if peer < id { continue; }
            if resident[id / 225] == 0.0 && resident[peer / 225] == 0.0 {
                let end = power_position(peer);
                if max(pos[0],end[0]) < cx - reach || min(pos[0],end[0]) > cx + reach ||
                    max(pos[2],end[2]) < cz - reach || min(pos[2],end[2]) > cz + reach { continue; }
            }
            let edge = peer.to_string();
            let wire = if previous.contains(edge) { previous[edge] } else { spawn_prefab("power-wire", pos) };
            position_cable(wire, pos, power_anchor(peer, graph[edge][0]));
            set_color(wire, if entry[1] > 0 { [0.44, 0.57, 0.32] } else { [0.13, 0.18, 0.23] });
            effects[edge] = wire;
        }
        if effects.len() > 0 { next[key] = effects; }
    }
    for key in old.keys() {
        for effect in old[key].keys() {
            if !next.contains(key) || !next[key].contains(effect) { destroy_prefab(old[key][effect]); }
        }
    }
    let pages = [];
    for group in 0..5 { pages.push(grid::empty_text(867)); }
    for key in next.keys() {
        let id = parse_int(key); let cell = id % 225;
        let group = cell / 45; let page = id / 225 * 3 + cell % 45 / 15;
        let record = key;
        for effect in next[key].keys() { record += ";" + effect + "," + next[key][effect]; }
        if pages[group][page] != "" { pages[group][page] += "|"; }
        pages[group][page] += record;
    }
    for group in 0..5 { set_object_list("power_fx_" + group.to_string(), pages[group]); }
    // All resident poles retain emissive status lamps. The nearest 32 also cast
    // light; a powered, resident rocket borrows the final slot for its alternating
    // navigation lights, keeping the total inside the engine's fixed budget.
    let center = [get_scene_variable("chunk_x") * 15.0, 0.0, get_scene_variable("chunk_z") * 15.0];
    let used = #{};
    for light in 0..32 {
        if light == 31 && data::session_value(6) > 0.0 {
            set_light_intensity("pole-light-31", 0.0);
            continue;
        }
        let best = -1; let distance = 10000000.0;
        for id in lamps {
            if used.contains(id.to_string()) { continue; }
            let pos = power_position(id);
            let d = pow(pos[0] - center[0], 2.0) + pow(pos[2] - center[2], 2.0);
            if d < distance { distance = d; best = id; }
        }
        let target = "pole-light-" + light.to_string();
        if best >= 0 {
            used[best.to_string()] = true;
            let pos = power_position(best);
            set_position(target, [pos[0], 1.98, pos[2]]);
            set_light_color(target, [0.72, 1.0, 0.59]);
            set_light_intensity(target, 9.0);
        } else { set_light_intensity(target, 0.0); }
    }
}

fn clear_power() {
    for record in read_power_fx().values() { for target in record.values() { destroy_prefab(target); } }
    for group in 0..5 { set_object_list("power_fx_" + group.to_string(), grid::empty_text(867)); }
    set_object_list("power_data", grid::empty_text(867));
    set_object_list("power_live", grid::empty_numbers(225));
    set_object_variable("power_dirty", true);
    data::ui_view_changed(4, "");
    set_object_variable("wire_start", -1.0);
    for i in 0..32 { set_light_intensity("pole-light-" + i.to_string(), 0.0); }
    data::session_set(6, 0.0); data::session_set(5, -2.0);
    set_visible("rocket-light-0", false); set_visible("rocket-light-1", false);
}

fn wire_demo() {
    // A real wired starter circuit for the optional production fixture.
    let builds = get_scene_list("builds");
    let nodes = get_scene_list("nodes");
    let machines = get_scene_list("machine_cells");
    let visuals = get_scene_list("build_visuals");
    let source = -1;
    let consumers = [];
    for entry in machines {
        let cell = entry.to_int();
        if builds[cell] == 6.0 { source = cell; }
        else if power_demand(builds[cell].to_int()) > 0 { consumers.push(cell); }
    }
    let previous = source;
    let port = 0; let pole = -1;
    for cell in consumers {
        if pole < 0 || port >= 3 {
            for candidate in 0..225 {
                if builds[candidate] == 0.0 && nodes[candidate] == 0.0 && candidate != grid::index(0, 0) {
                    pole = candidate; builds[pole] = 9.0; machines.push(pole.to_float());
                    visuals[pole] = grid::spawn_build(9.0, grid::cell_x(pole), grid::cell_z(pole), 0);
                    power_register(power_id(pole), 9);
                    if previous >= 0 { connect_power(power_id(previous), power_id(pole)); }
                    previous = pole; port = 0; break;
                }
            }
        }
        connect_power(power_id(pole), power_id(cell)); port += 1;
    }
    set_scene_list("builds", builds); set_scene_list("machine_cells", machines); set_scene_list("build_visuals", visuals);
}

fn power_anchor(id, kind) {
    let pos = power_position(id);
    pos[1] = if kind == 9 { 1.47 } else if kind == 10 { 1.26 } else { 1.08 };
    if id / 225 == power_id(0) / 225 && kind != 10 {
        let target = get_scene_list("build_visuals")[id % 225];
        if target != "" { pos[1] += get_position(target)[1] - 0.08; }
    }
    pos
}

fn position_cable(wire, start, end) {
    let dx = end[0] - start[0]; let dy = end[1] - start[1]; let dz = end[2] - start[2];
    let ground = sqrt(dx * dx + dz * dz);
    set_position(wire, [(end[0] + start[0]) / 2.0, (end[1] + start[1]) / 2.0, (end[2] + start[2]) / 2.0]);
    set_rotation(wire, [0.0, -atan2(dz, dx) * 57.29578, atan2(dy, ground) * 57.29578]);
    set_scale(wire, [sqrt(ground * ground + dy * dy), 0.035, 0.035]);
}

fn power_entry(pages, id) {
    let page = pages[id / 75];
    if page == "" { return []; }
    let text = page.split("|")[id % 75];
    if text == "" { return []; }
    let entry = [];
    for number in text.split(",") { entry.push(parse_int(number)); }
    entry
}

fn power_effects(id) {
    let cell = id % 225;
    let page = get_object_list("power_fx_" + (cell / 45).to_string())[id / 225 * 3 + cell % 45 / 15];
    let effects = #{};
    if page == "" { return effects; }
    for record in page.split("|") {
        let fields = record.split(";");
        if fields[0] != id.to_string() { continue; }
        for i in 1..fields.len() { let pair = fields[i].split(","); effects[pair[0]] = pair[1]; }
    }
    effects
}

fn animate_power_connections(cells) {
    if cells.len() == 0 { return; }
    let pages = get_object_list("power_data");
    if pages.len() == 0 { return; }
    for cell in cells {
        let id = power_id(cell.to_int()); let entry = power_entry(pages,id);
        if entry.len() == 0 { continue; }
        let pos = power_anchor(id,entry[0]);
        if entry[0] == 9 {
            let effects = power_effects(id);
            if effects.contains("lamp") { set_position(effects["lamp"], [pos[0],pos[1]+0.43,pos[2]]); }
        }
        for peer in entry.extract(2) {
            let effects = power_effects(min(id,peer));
            let key = max(id,peer).to_string();
            if effects.contains(key) {
                let end = power_anchor(peer,power_entry(pages,peer)[0]);
                if id < peer { position_cable(effects[key],pos,end); }
                else { position_cable(effects[key],end,pos); }
            }
        }
    }
}
