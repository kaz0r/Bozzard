// Rhai gameplay: deposits.
import "factory-data" as data;
import "factory-grid" as grid;

// Seeded outposts guarantee all resource types somewhere on this finite planet.
// Coal is reachable in one cardinal neighbor; later resources need exploration.
fn resource_outpost(seed, kind) {
    let state = ((seed + kind * 104729) * 48271) % 2147483647;
    let radius = if kind == 4 { 1 } else if kind == 7 { 2 } else if kind == 5 { 3 }
        else if kind == 6 { 4 + state % 3 } else { 6 + state % 3 };
    state = (state * 48271) % 2147483647;
    let side = state % 4;
    state = (state * 48271) % 2147483647;
    let offset = if kind == 4 { 0 } else { state % (radius * 2 + 1) - radius };
    if side == 0 { [radius, offset] } else if side == 1 { [offset, radius] }
    else if side == 2 { [-radius, offset] } else { [offset, -radius] }
}

fn resource_kind(roll) {
    // Common: iron, copper, stone, sand, limestone (85% of initial rolls).
    // Uncommon: coal, water, quartz. Rare: oil and silver.
    if roll < 210 { 1 } else if roll < 380 { 2 } else if roll < 580 { 8 }
    else if roll < 720 { 9 } else if roll < 850 { 3 } else if roll < 920 { 4 }
    else if roll < 960 { 7 } else if roll < 985 { 5 } else if roll < 995 { 6 } else { 10 }
}

fn generate_region_nodes(seed, cx, cz) {
    generate_region_nodes(seed,cx,cz,if data::on_moon() { 1 } else { 0 })
}
fn generate_region_nodes(seed,cx,cz,planet) {
    if planet==1 { return generate_moon_nodes(seed, cx, cz); }
    let home = cx == 0 && cz == 0;
    let state = (seed + (grid::chunk_id(cx, cz) + 1) * 104729) % 2147483647;
    state = (state * 48271) % 2147483647;
    let kinds = if home { [1, 2, 8, 9, 3] } else { [] };
    let count = if home { 5 } else { 2 + state % 3 };
    if !home {
        for kind in [4, 7, 5, 6, 10] {
            let outpost = resource_outpost(seed, kind);
            if outpost[0] == cx && outpost[1] == cz { kinds.push(kind); }
        }
        // No duplicate resource types in a region. Bound retries even for an
        // unusually repetitive random sequence, then fill with starter solids.
        for attempt in 0..32 {
            if kinds.len() >= count { break; }
            state = (state * 48271) % 2147483647;
            let kind = resource_kind(state % 1000);
            if !kinds.contains(kind) { kinds.push(kind); }
        }
        for kind in [1, 2, 8, 9, 3] {
            if kinds.len() >= count { break; }
            if !kinds.contains(kind) { kinds.push(kind); }
        }
    }
    // Nine shuffled, jittered sites provide a three-tile minimum center distance,
    // including across chunk seams, without inspecting or generating neighbors.
    // The middle home site is excluded to keep the entire future landing pad clear.
    let sites = if home { [0, 1, 2, 3, 5, 6, 7, 8] } else { [0, 1, 2, 3, 4, 5, 6, 7, 8] };
    let nodes = grid::empty_numbers(225);
    for i in 0..kinds.len() {
        state = (state * 48271) % 2147483647;
        let chosen = i + state % (sites.len() - i);
        let site = sites[chosen]; sites[chosen] = sites[i]; sites[i] = site;
        state = (state * 48271) % 2147483647;
        let x = (site % 3 - 1) * 5 + state % 3 - 1;
        state = (state * 48271) % 2147483647;
        let z = (site / 3 - 1) * 5 + state % 3 - 1;
        nodes[grid::index(x, z)] = kinds[i].to_float();
    }
    nodes
}

// Two distinct, opposite outposts guarantee Techtorium without filling every
// region. Ordinary lunar regions contain zero or one deposit, with no Earth ores.
fn moon_techtorium_sites(seed) {
    let state = ((seed + 8191) * 48271) % 2147483647;
    let radius = 4 + state % 3;
    state = (state * 48271) % 2147483647;
    let offset = state % 7 - 3;
    if state % 2 == 0 { [[radius, offset], [-radius, -offset]] }
    else { [[offset, radius], [-offset, -radius]] }
}

fn generate_moon_nodes(seed, cx, cz) {
    let nodes = grid::empty_numbers(225);
    if abs(cx) > 6 || abs(cz) > 6 { return nodes; }
    if cx == 0 && cz == 0 {
        nodes[grid::index(-4, 3)] = 24.0; nodes[grid::index(4, -3)] = 25.0;
        return nodes;
    }
    let state = ((seed + (grid::chunk_id(cx, cz) + 1) * 65537) * 48271) % 2147483647;
    // Mix coordinate strides before the density roll to avoid correlated types.
    state = (state * 48271) % 2147483647;
    state = (state * 48271) % 2147483647;
    let kind = 0.0;
    for site in moon_techtorium_sites(seed) {
        if site[0] == cx && site[1] == cz { kind = 26.0; }
    }
    if kind == 0.0 {
        if state % 100 >= 35 { return nodes; }
        state = (state * 48271) % 2147483647;
        kind = if state % 100 < 55 { 25.0 } else { 24.0 };
    }
    state = (state * 48271) % 2147483647;
    let x = state % 9 - 4;
    state = (state * 48271) % 2147483647;
    nodes[grid::index(x, state % 9 - 4)] = kind;
    nodes
}
