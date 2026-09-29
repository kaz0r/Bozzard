import "factory-backpack" as backpack;
import "factory-net_input" as net;
// Rhai gameplay: machines.
import "factory-data" as data;
import "factory-inventory" as inventory;
import "factory-panels" as panels;
import "factory-buffers" as buffers;

fn select_recipe(cell, recipe) {
    if net::guest() {net::send("configure",#{at:net::position(cell),recipe:recipe.to_int()});return;}
    let kind = get_scene_list("builds")[cell];
    if !data::recipe_choices(kind).contains(recipe) || !data::recipe_unlocked(recipe.to_int()) { return; }
    let recipes = get_object_list("recipes");
    if recipes[cell] == recipe { return; }
    if data::expansion(kind) {
        let slots=inventory::storage_read(cell);
        if buffers::output_load(slots)>0.0 {set_scene_variable("message","Collect every finished output before changing the recipe.");return;}
        let refunded=buffers::refund(slots,backpack::slots());
        if !refunded.ok {set_scene_variable("message","Make room for unused ingredients first.");return;}
        backpack::commit(refunded.inventory); inventory::storage_write(cell,buffers::empty());
        recipes[cell]=recipe; set_object_list("recipes",recipes);
        let progress=get_scene_list("progress");progress[cell]=0.0;set_scene_list("progress",progress);return;
    }
    if get_scene_list("item_amounts")[cell] > 0.0 { return; }
    let previous = data::recipe_cost(recipes[cell].to_int()); let next = data::recipe_cost(recipe.to_int());
    if previous != next || data::recipe_yield(recipes[cell].to_int()) != data::recipe_yield(recipe.to_int()) {
        let stock = get_object_list("stock");
        if kind == 5.0 {
            let a = get_scene_list("assembler_iron"); let b = get_scene_list("assembler_copper");
            if previous.len() == 4 { stock[previous[0]] += a[cell]; stock[previous[2]] += b[cell]; }
            if !backpack::set_stock(stock) { set_scene_variable("message","Inventory is full. Make room for the unused ingredients first."); return; }
            a[cell] = 0.0; b[cell] = 0.0;
            set_scene_list("assembler_iron",a); set_scene_list("assembler_copper",b);
        } else {
            let amounts = get_scene_list("input_amounts"); let items = get_scene_list("input_items");
            stock[items[cell].to_int()] += amounts[cell];
            if !backpack::set_stock(stock) { set_scene_variable("message","Inventory is full. Make room for the unused ingredients first."); return; }
            amounts[cell] = 0.0; items[cell] = 0.0;
            set_scene_list("input_amounts",amounts); set_scene_list("input_items",items);
        }
    }
    let progress = get_scene_list("progress"); progress[cell] = 0.0; set_scene_list("progress",progress);
    recipes[cell] = recipe; set_object_list("recipes",recipes);
}

fn feed_assembler(cell) {
    if net::guest() {net::send("feed",#{at:net::position(cell)});return;}
    let kind = get_scene_list("builds")[cell];
    let recipe = get_object_list("recipes")[cell]; let cost = data::recipe_cost(recipe.to_int());
    if cost.len() == 0 || !data::recipe_unlocked(recipe.to_int()) || get_object_list("rotation_turns")[cell] > 0.0 { return; }
    if data::expansion(kind) {
        let limit=0.0;for i in 0..cost.len()/2 {limit+=cost[i*2+1]*10.0;}
        let result=buffers::feed(inventory::storage_read(cell),recipe.to_int(),backpack::slots(),data::free_build(),0.0,limit.to_int());
        if result.moved==0.0 {set_scene_variable("message","Missing ingredients or machine buffer is full.");return;}
        backpack::commit(result.inventory);inventory::storage_write(cell,result.slots);return;
    }
    let stock = get_object_list("stock"); let output = get_scene_list("item_amounts")[cell];
    if kind == 5.0 {
        let a = get_scene_list("assembler_iron"); let b = get_scene_list("assembler_copper");
        for n in 0..(10.0*(cost[1]+cost[3])).to_int() {
            let order = if a[cell]/cost[1] <= b[cell]/cost[3] { [0,1] } else { [1,0] };
            let added = false;
            for side in order {
                let item = cost[side*2];
                let reserve = if side == 0 { max(0.0,cost[3]-b[cell]) } else { max(0.0,cost[1]-a[cell]) };
                if output+a[cell]+b[cell]+1.0+reserve > data::buffer_capacity() || (!data::free_build() && stock[item] < 1.0) { continue; }
                if side == 0 { a[cell] += 1.0; } else { b[cell] += 1.0; }
                if !data::free_build() { stock[item] -= 1.0; }
                added = true; break;
            }
            if !added { break; }
        }
        set_scene_list("assembler_iron",a); set_scene_list("assembler_copper",b);
    } else {
        let items = get_scene_list("input_items"); let amounts = get_scene_list("input_amounts");
        if amounts[cell] > 0.0 && items[cell] != cost[0].to_float() { return; }
        let reserve = max(0.0,data::recipe_yield(recipe.to_int())-1.0);
        let amount = max(0.0,min(10.0,min(data::buffer_capacity()-output-amounts[cell]-reserve,if data::free_build() { 100.0 } else { stock[cost[0]] })));
        if amount > 0.0 { items[cell] = cost[0].to_float(); amounts[cell] += amount; if !data::free_build() { stock[cost[0]] -= amount; } }
        set_scene_list("input_items",items); set_scene_list("input_amounts",amounts);
    }
    backpack::set_stock(stock);
}

fn update_assembler() {
    let cell = get_object_variable("assembler_cell").to_int(); if cell < 0 { return; }
    if input_pressed("E") { panels::set_assembler(-1); return; }
    for event in ui_events() {
        if event.kind != "activate" { continue; }
        if event.target == "assembler-close" { panels::set_assembler(-1); return; }
        let choices = data::recipe_choices(get_scene_list("builds")[cell]);
        let names = ["assembler-alloy","assembler-parts","assembler-concrete"];
        for i in 0..choices.len() { if event.target == names[i] { select_recipe(cell,choices[i]); } }
        if event.target == "assembler-feed" { feed_assembler(cell); }
        if event.target == "assembler-take" { inventory::collect_machine(cell); }
    }
    panels::draw_assembler();
}
