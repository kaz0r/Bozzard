// Rhai gameplay: grid.
import "factory-data" as data;

fn index(x, z) { (z + 7) * 15 + x + 7 }

fn cell_x(i) { (i % 15) - 7 }

fn cell_z(i) { (i / 15) - 7 }

fn step_x(dir) { if dir == 0 { 1 } else if dir == 2 { -1 } else { 0 } }

fn step_z(dir) { if dir == 1 { 1 } else if dir == 3 { -1 } else { 0 } }

fn neighbor(cell, direction) {
    let x = cell_x(cell) + step_x(direction);
    let z = cell_z(cell) + step_z(direction);
    if x < -7 || x > 7 || z < -7 || z > 7 { -1 } else { index(x, z) }
}

fn layout_x(x, z, origin, rotation) {
    if rotation == 0 { origin + x }
    else if rotation == 1 { origin - z }
    else if rotation == 2 { origin - x }
    else { origin + z }
}

fn layout_z(x, z, origin, rotation) {
    if rotation == 0 { origin + z }
    else if rotation == 1 { origin + x }
    else if rotation == 2 { origin - z }
    else { origin - x }
}

fn layout_cell(x, z, origin_x, origin_z, rotation) {
    index(layout_x(x, z, origin_x, rotation), layout_z(x, z, origin_z, rotation))
}

fn empty_numbers(count) {
    let values = [];
    values.pad(count, 0.0);
    values
}

fn empty_text(count) {
    let values = [];
    values.pad(count, "");
    values
}

fn spawn_build(kind, x, z, facing) {
    let target = spawn_prefab(data::build_asset(kind), [world_x(x), 0.08, world_z(z)]);
    set_rotation(target, [0.0, -facing.to_float() * 90.0, 0.0]);
    target
}

// Each region is 15 x 15 cells. Archives use bounded integer/text pages rather than
// enlarging the engine's blackboard limits. All built regions on the planet simulate.
fn chunk_id(x, z) { (z + 8) * 17 + x + 8 }

fn world_x(x) { x.to_float() + get_scene_variable("chunk_x") * 15.0 }

fn world_z(z) { z.to_float() + get_scene_variable("chunk_z") * 15.0 }

fn pack_numbers(values) {
    let text = "";
    let i = 0;
    while i < values.len() {
        let end = i + 1;
        while end < values.len() && values[end] == values[i] { end += 1; }
        if end - i >= 3 {
            if text != "" { text += ","; }
            text += values[i].to_int().to_string() + ":" + (end - i).to_string();
        } else {
            for at in i..end {
                if text != "" { text += ","; }
                text += values[at].to_int().to_string();
            }
        }
        i = end;
    }
    text
}

fn unpack_numbers(text, count) {
    if text == "" { return empty_numbers(count); }
    let values = [];
    for part in text.split(",") {
        let run = part.split(":");
        let value = parse_int(run[0]).to_float();
        let length = if run.len() == 2 { parse_int(run[1]) } else { 1 };
        values.pad(values.len() + length, value);
    }
    values
}

// Read one archived cell without allocating the complete region.
fn packed_value(text,index) {
    if text=="" {return 0.0;}
    let at=0;
    for part in text.split(",") {
        let run=part.split(":");let end=at+if run.len()==2 {parse_int(run[1])}else{1};
        if index<end {return parse_int(run[0]).to_float();}at=end;
    }
    0.0
}

// Read only occupied cells from a numeric archive. Storage pages otherwise
// expand hundreds of empty slots for each container on every production beat.
fn unpack_cells(text, cells, stride) {
    unpack_numeric_cells(text,cells,stride)
}

// The bounded native encoder uses the same integer/run-length archive format.
// It also compresses uniform occupied values, rather than expanding every zero.
fn pack_cells(values, cells, stride) {
    pack_numeric_cells(values,cells,stride)
}

fn pack_text(values) {
    let any = false;
    for value in values { if value != "" { any = true; break; } }
    if !any { return ""; }
    let text = "";
    for i in 0..values.len() {
        if i > 0 { text += "|"; }
        text += values[i];
    }
    text
}

fn unpack_text(text, count) {
    if text == "" { return empty_text(count); }
    text.split("|")
}

fn cache_put(name, id, value) {
    let pages = get_object_list(name);
    pages[id + if data::archived_region_data(name) && data::on_moon() { 289 } else { 0 }] = value;
    set_object_list(name, pages);
}

fn smooth(t) { t * t * (3.0 - 2.0 * t) }

fn current_chunk() { chunk_id(get_scene_variable("chunk_x").to_int(),get_scene_variable("chunk_z").to_int()) }
fn global_x(id) { ((id/225%17-8)*15+cell_x(id%225)).to_float() }
fn global_z(id) { ((id/225/17-8)*15+cell_z(id%225)).to_float() }
// A cell address spans chunk seams but never wraps around a planet edge.
fn global_neighbor(id, direction) {
    global_neighbor(id,direction,data::planet_radius())
}

fn global_neighbor(id, direction, radius) {
    let chunk=id/225; let cx=chunk%17-8; let cz=chunk/17-8;
    let x=cell_x(id%225)+step_x(direction); let z=cell_z(id%225)+step_z(direction);
    if x>7 { cx+=1; x=-7; } else if x< -7 { cx-=1; x=7; }
    if z>7 { cz+=1; z=-7; } else if z< -7 { cz-=1; z=7; }
    if abs(cx)>radius || abs(cz)>radius { -1 } else { chunk_id(cx,cz)*225+index(x,z) }
}
