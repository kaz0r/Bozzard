import "factory-backpack" as backpack;
import "factory-net_input" as net;
// Rhai gameplay: power.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-model_ports" as model_ports;
import "factory-renewables" as renewables;

fn electrical(kind) { kind == 1 || kind == 3 || kind == 5 || kind == 6 || kind == 9 || kind == 10 || kind == 11 || kind>=12 && kind<=25 || kind>=40 && kind<=42 }

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
    // Decode the existing save format in one bounded native operation.
    unpack_integer_rows(get_object_list(name),75)
}

fn write_power(graph) {
    write_power("power_data",graph);
}
fn write_power(name,graph) {
    set_object_list(name,pack_integer_rows(graph,75,867));
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
    let owner=renewables::local_owner(id);if renewables::array_kind(owner)==41.0 {id=owner;}
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
    let owner=renewables::local_owner(id);if renewables::array_kind(owner)==41.0 {id=owner;}
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

fn power_demand(kind) { if kind>=12 && kind<=25 {[2,3,2,3,4,5,4,6,3,3,2,4,6,3][kind-12]}else if kind == 1 { 1 } else if kind == 3 { 2 } else if kind == 5 { 3 } else if kind == 11 { 2 } else { 0 } }

fn resolve_graph(graph) {
    resolve_graph(graph,true)
}
fn resolve_graph(graph,sunny) {
    resolve_graph(graph,sunny,false)
}
fn resolve_graph(graph,sunny,windy) {
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
            supply += renewables::supply(entry[0],sunny,windy);
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
    let planet=if data::on_moon(){1}else{0};let time=data::session_value(120);
    let sunny=renewables::solar_active(time,planet);
    let seed=get_scene_variable("seed").to_int();
    let windy=renewables::wind_active(seed,planet,time);
    let other_windy=renewables::wind_active(seed,1-planet,time);
    let daylight=if renewables::solar_active(time,0){1.0}else{0.0};
    let changed=get_object_variable("renewable-view","day")!=daylight || get_object_variable("renewable-view","planet")!=planet.to_float();
    let wind_key=if planet==0 {"earth_wind"}else{"moon_wind"};
    let other_key=if planet==0 {"moon_wind"}else{"earth_wind"};
    let wind_value=if windy {1.0}else{0.0};let other_value=if other_windy {1.0}else{0.0};
    let dirty = get_object_variable("power_dirty") || changed || get_object_variable("renewable-view",wind_key)!=wind_value;
    if changed || get_object_variable("renewable-view",other_key)!=other_value {
        // Weather transitions update off-world circuits as well as the visible one.
        write_power("power_other",resolve_graph(power_graph("power_other"),renewables::solar_active(time,1-planet),other_windy).graph);
        set_object_variable("renewable-view","day",daylight);set_object_variable("renewable-view","planet",planet.to_float());
    }
    if get_object_variable("renewable-view",wind_key)!=wind_value {set_object_variable("renewable-view",wind_key,wind_value);}
    if get_object_variable("renewable-view",other_key)!=other_value {set_object_variable("renewable-view",other_key,other_value);}
    let aspect = render_stats().viewport_aspect;
    // Chunk load/unload invalidates this view. Do not format all 289 residency
    // entries every tick just to rediscover that nothing has changed.
    let view = get_scene_variable("chunk_x").to_string() + "," + get_scene_variable("chunk_z").to_string() + "|" + ceil(aspect).to_string() + "|" + get_object_variable("phase").to_string();
    if !dirty && view == get_object_list("ui_views")[4] { return; }
    let graph = power_graph();
    if dirty {
        let resolved=resolve_graph(graph,sunny,windy); graph=resolved.graph;
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
            if entry[1] > 0 { lamps.push(#{id:id,role:"pole",kind:9,on:true}); }
        }
        if has_machine_indicator(entry[0]) && resident[id / 225] > 0.0 {
            let on = entry[1] > 0;
            let role = if on { "on" } else { "off" };
            let prefix = if entry[0]>=12 && entry[0]<=25 {data::expansion_slug(entry[0].to_float())}else if entry[0] == 1 { "miner" } else if entry[0] == 3 { "smelter" }
                else if entry[0] == 5 { "assembler" } else if entry[0] == 6 { "generator" } else { "constructor" };
            let lamp = if previous.contains(role) { previous[role] }
                else { spawn_prefab(prefix + "-power-" + role, [pos[0],0.08,pos[2]]) };
            align_machine_effect(lamp,id);
            effects[role] = lamp;
            lamps.push(#{id:id,role:"status",kind:entry[0],on:on});
            if [3,16,20,21].contains(entry[0]) && on {
                let heat = if previous.contains("heat") { previous["heat"] }
                    else { spawn_prefab(prefix+"-heat",[pos[0],0.08,pos[2]]) };
                align_machine_effect(heat,id); effects["heat"] = heat;
                lamps.push(#{id:id,role:"heat",kind:entry[0],on:true});
            }
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
    // Fixed light budget shared by poles, faint status LEDs and furnace heat.
    // Emissive lenses remain visible even beyond the nearest illumination slots.
    let center_x = get_scene_variable("chunk_x").to_int() * 15;
    let center_z = get_scene_variable("chunk_z").to_int() * 15;
    let ranked = [];
    for i in 0..lamps.len() {
        let pos = power_position(lamps[i].id);
        let dx = pos[0].to_int() - center_x; let dz = pos[2].to_int() - center_z;
        // Grid coordinates have integral squared distances. Include the original
        // index to preserve the previous selection order for equally near lamps.
        ranked.push((dx * dx + dz * dz) * lamps.len() + i);
    }
    ranked.sort();
    let slots = grid::empty_text(32);
    for light in 0..32 {
        if light == 31 && data::session_value(6) > 0.0 {
            set_light_intensity("pole-light-31", 0.0);
            continue;
        }
        let best = if light < ranked.len() { ranked[light] % lamps.len() } else { -1 };
        let target = "pole-light-" + light.to_string();
        if best >= 0 {
            let lamp = lamps[best]; let pos = power_position(lamp.id);
            if lamp.role == "pole" {
                set_position(target, [pos[0],1.98,pos[2]]);
                set_light_color(target,[0.72,1.0,0.59]); set_light_intensity(target,9.0);
            } else {
                slots[light] = lamp.id.to_string() + "," + lamp.kind.to_string() + "," + lamp.role;
                position_machine_light(target,lamp.id,lamp.kind,lamp.role);
                set_light_color(target, if lamp.role == "heat" { [1.0,0.23,0.045] }
                    else if lamp.on { [0.24,0.90,0.29] } else { [0.80,0.045,0.025] });
                set_light_intensity(target,if lamp.role == "heat" { 1.2 } else { 0.12 });
            }
        } else { set_light_intensity(target, 0.0); }
    }
    set_object_list("factory-transports","machine_light_slots",slots);
}

// Resolve only resident presentation handles. No simulated state or saved data
// is added for cosmetic effects, and remote regions use their own visual cache.
fn has_machine_indicator(kind) { kind == 1 || kind == 3 || kind == 5 || kind == 6 || kind == 11 || kind>=12 && kind<=25 }

fn machine_handle(id) {
    if id / 225 == grid::current_chunk() { return get_scene_list_item("build_visuals",id % 225); }
    grid::unpack_text(get_object_list_item("cache_visuals_"+((id % 225)/75).to_string(),id / 225),75)[id % 75]
}

fn align_machine_effect(effect,id) {
    let machine = machine_handle(id);
    if machine == "" { return; }
    set_position(effect,get_position(machine)); set_rotation(effect,get_rotation(machine));
}

fn position_machine_light(light,id,kind,role) {
    let machine = machine_handle(id); if machine == "" { return; }
    let pos = get_position(machine); let angle = get_rotation(machine)[1] * 0.0174532925;
    let local = if kind>=12 && kind<=25 {if role=="heat" {model_ports::heat(kind)}else{model_ports::indicator(kind)}}else if role == "heat" { [0.51,0.77,0.0] }
        else if kind == 1 { [0.39,1.07,0.287] }
        else if kind == 6 { [0.39,1.055,0.285] }
        else if kind == 3 { [0.49,0.56,0.332] } else { [0.50,0.75,0.32] };
    set_position(light,[pos[0]+cos(angle)*local[0]+sin(angle)*local[2],pos[1]+local[1],
        pos[2]-sin(angle)*local[0]+cos(angle)*local[2]]);
}

fn animate_machine_effects(id) {
    let effects = power_effects(id);
    for role in ["on","off","heat"] {
        if effects.contains(role) { align_machine_effect(effects[role],id); }
    }
    let slots = get_object_list("factory-transports","machine_light_slots");
    for i in 0..slots.len() {
        if slots[i] == "" { continue; }
        let fields=slots[i].split(",");
        if parse_int(fields[0]) == id { position_machine_light("pole-light-"+i.to_string(),id,parse_int(fields[1]),fields[2]); }
    }
}

fn clear_power() {
    for record in read_power_fx().values() { for target in record.values() { destroy_prefab(target); } }
    for group in 0..5 { set_object_list("power_fx_" + group.to_string(), grid::empty_text(867)); }
    set_object_list("power_data", grid::empty_text(867));
    set_object_list("power_live", grid::empty_numbers(225));
    set_object_list("factory-transports","machine_light_slots",[]);
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
    if kind>=12 && kind<=25 || kind>=40 && kind<=42 {
        let local=if kind==40 {[0.0,0.16,0.405]}else if kind==41 {[0.5,0.23,0.40]}else if kind==42 {[0.0,0.28,0.10]}else{model_ports::socket(kind)};
        let pos=power_position(id);let angle=0.0;let lift=0.08;
        let target=machine_handle(id);
        if target!="" {angle=get_rotation(target)[1]*0.0174532925;lift=get_position(target)[1];}
        else {
            let facings=if id/225==grid::current_chunk() {get_scene_list("facings")}else{grid::unpack_numbers(data::region_entry("cache_facings",id/225),225)};
            angle= -facings[id%225]*1.5707963268;
        }
        return [pos[0]+cos(angle)*local[0]+sin(angle)*local[2],lift+local[1],pos[2]-sin(angle)*local[0]+cos(angle)*local[2]];
    }
    let pos = power_position(id);
    pos[1] = if kind == 9 { 1.47 } else if kind == 10 { 1.26 }
        else if kind == 1 || kind == 6 { 1.35 } else if kind == 3 { 1.15 } else { 1.08 };
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
        if has_machine_indicator(entry[0]) { animate_machine_effects(id); }
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
