import "factory-backpack" as backpack;
import "factory-net_input" as net;
// Rhai gameplay: progression.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-visuals" as visuals;

fn gather(dt, blocked) {
    if net::guest() {net::gathering(!blocked && input_held("F"));return;}
    let clock = max(0.0, get_object_variable("gather_clock") - dt);
    if !blocked && input_held("F") && clock <= 0.0 {
        let cell = grid::index(get_scene_variable("cursor_x").to_int(), get_scene_variable("cursor_z").to_int());
        let kind = get_scene_list("nodes")[cell].to_int();
        if data::solid_node(kind.to_float()) {
            let added = backpack::give(kind.to_float(),1.0);
            set_scene_variable("message",if added > 0.0 { "+1 " + data::item_name(kind.to_float()) + " in your backpack. J opens crafting and deliveries." } else { "Inventory is full. Move, use or destroy a stack first." });
        } else {
            set_scene_variable("message", if kind >= 6 { "Liquids need extraction equipment." } else { "Stand on a solid resource node and hold F to gather." });
        }
        clock = 0.30;
    }
    set_object_variable("gather_clock", clock);
}

fn delivery_recipe(phase) {
    if phase == 0 { [1, 12.0, 2, 8.0, 8, 8.0] }
    else if phase == 1 { [11, 16.0, 12, 8.0, 14, 4.0] }
    else if phase == 2 { [15, 12.0, 16, 40.0, 17, 8.0] }
    else if phase == 3 { [15, 20.0, 16, 80.0, 17, 16.0, 14, 8.0] }
    else if phase == 4 { [18, 80.0, 15, 40.0, 16, 80.0] }
    else if phase == 5 { [15, 160.0, 16, 240.0, 17, 80.0, 14, 40.0] }
    else if phase == 6 { [15, 200.0, 16, 320.0, 17, 120.0, 14, 80.0] }
    else { [] }
}

fn deliver_phase() {
    if net::guest() {net::send("deliver",#{});return;}
    let phase = get_object_variable("phase").to_int();
    if phase >= 7 { return; }
    let recipe = delivery_recipe(phase);
    let stock = get_object_list("stock");
    if !data::can_pay(stock, recipe) { return; }
    if phase == 4 && !visuals::landing_clear() { set_scene_variable("message", "Clear the rocket and dock tiles (0,1 and 2,1) before expanding the landing site."); return; }
    backpack::set_stock( data::spend(stock, recipe));
    set_object_variable("phase", (phase + 1).to_float());
    visuals::update_projects();
    set_scene_variable("message", data::phase_label(phase + 1) + " unlocked: " + data::phase_name(phase + 1));
}

fn at_workbench() {
    get_scene_variable("chunk_x") == 0.0 && get_scene_variable("chunk_z") == 0.0 &&
        abs(get_scene_variable("cursor_x")) <= 1.0 && abs(get_scene_variable("cursor_z")) <= 1.0
}

fn craft(item) { craft_batch(item, 1); }

fn craft_batch(item, count) {
    if net::guest() {net::send("craft",#{recipe:item,count:count});return;}
    if !at_workbench() || !data::recipe_unlocked(item) || !data::workbench_recipe(item) { return; }
    let cost = data::recipe_cost(item);
    if cost.len() == 0 { return; }
    for batch in 0..count {
        let stock = get_object_list("stock");
        if !data::free_build() && !data::can_pay(stock,cost) { break; }
        if !data::free_build() { stock = data::spend(stock,cost); }
        stock[item] += data::recipe_yield(item);
        if !backpack::set_stock(stock) { set_scene_variable("message","Inventory is full. Crafting needs space for its output."); break; }
    }
}
