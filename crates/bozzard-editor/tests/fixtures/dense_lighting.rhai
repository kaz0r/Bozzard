// Place 400 builds (80 generators, 80 poles, 240 kilns) in two chunks.
// Separate setup hooks stay within the normal per-hook operation budget.
let dense_stage = 0;
fn on_start(me) {
    normal_start(me);
    set_object_variable("creative", true);
    set_object_variable("phase", 7.0);
    let empty = grid::empty_numbers(225);
    set_scene_list("nodes", empty);
    grid::cache_put("chunk_nodes", 144, grid::pack_numbers(empty));
    data::session_set(120, 15.7);
}
fn on_update(me, dt) {
    if dense_stage < 2 {
        world::enter_chunk(if dense_stage == 0 {1} else {0}, if dense_stage == 0 {0} else {1});
        let builds = grid::empty_numbers(225);
        let nodes = grid::empty_numbers(225);
        let recipes = grid::empty_numbers(225);
        let inputs = grid::empty_numbers(225);
        let amounts = grid::empty_numbers(225);
        let visuals = grid::empty_text(225);
        let cells = [];
        let graph = power::power_graph();
        for group in 0..40 {
            let base = group * 5;
            let pole = power::power_id(base + 1);
            let peers = [9, 0];
            for offset in 0..5 {
                let cell = base + offset;
                let kind = if offset == 0 {6} else if offset == 1 {9} else {21};
                builds[cell] = kind.to_float();
                cells.push(cell.to_float());
                visuals[cell] = grid::spawn_build(kind.to_float(), grid::cell_x(cell), grid::cell_z(cell), 0);
                if offset == 1 {continue;}
                let id = power::power_id(cell);
                graph[id.to_string()] = [kind, 0, pole];
                peers.push(id);
                if kind == 6 {nodes[cell] = 4.0;}
                else {
                    recipes[cell] = data::recipe_choices(kind.to_float())[0];
                    inputs[cell] = 1.0;
                    amounts[cell] = 100.0;
                }
            }
            graph[pole.to_string()] = peers;
        }
        set_scene_list("builds", builds);
        set_scene_list("nodes", nodes);
        set_scene_list("build_visuals", visuals);
        set_scene_list("machine_cells", cells);
        set_scene_list("input_items", inputs);
        set_scene_list("input_amounts", amounts);
        set_object_list("recipes", recipes);
        for page in 0..4 {
            let kinds = grid::empty_numbers(900);
            let amounts = grid::empty_numbers(900);
            for cell in 0..200 {
                if builds[cell] != 21.0 {continue;}
                let cost = data::recipe_cost(recipes[cell].to_int());
                for slot in 0..4 {
                    let input = page * 4 + slot;
                    if input >= cost.len() / 2 {continue;}
                    kinds[cell * 4 + slot] = cost[input * 2].to_float();
                    amounts[cell * 4 + slot] = 8.0;
                }
            }
            set_scene_list("storage_kinds_" + page.to_string(), kinds);
            set_scene_list("storage_amounts_" + page.to_string(), amounts);
        }
        power::write_power(graph);
        set_object_variable("power_dirty", true);
        dense_stage += 1;
        return;
    }
    normal_update(me, dt);
}
