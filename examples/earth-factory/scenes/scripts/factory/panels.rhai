import "factory-backpack" as backpack;
// Rhai gameplay: panels.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-power" as power;
import "factory-visuals" as visuals;

fn close_storage() {
    set_scene_variable("storage_open", false);
    set_scene_variable("storage_drag", -1.0);
    set_scene_variable("storage_menu", -1.0);
}

fn reset_interactions() {
    set_menu(false);
    set_map(false);
    set_object_variable("map_state", "");
    set_object_variable("camera_zoom", 19.0);
    set_object_variable("camera_zoom_target", 19.0);
    set_camera_size("camera", 19.0);
    set_object_variable("debug_clock", 0.25);
    set_object_list("ui_views", ["", "", "", "", "", ""]);
    set_object_list("rotation_cells", []);
    set_object_list("rotation_time", []);
    set_object_list("rotation_from", grid::empty_numbers(225));
    set_object_list("rotation_turns", grid::empty_numbers(225));
    let times = [];
    for cell in 0..225 { times.push(1.0); }
    set_object_list("rotation_time", times);
    set_object_variable("journal_open", false);
    set_object_variable("journal_alpha", 0.0);
    set_ui_visible("journal-overlay", false);
    set_object_variable("phase", if get_scene_variable("demo_mode") || get_object_variable("creative") { 7.0 } else { 0.0 });
    set_object_variable("bar", if get_scene_variable("demo_mode") { 0.0 } else { 1.0 });
    set_object_list("bar_slots", [1.0, 1.0, 1.0]);
    backpack::clear();
    set_object_variable("gather_clock", 0.0);
    for i in 1..7 { data::session_set(i, 0.0); }
    data::session_set(3, 11.0); data::session_set(4, -1.0); data::session_set(5, -1.0);
    for i in 40..64 { data::session_set(i,0.0); }
    data::session_set(46,-1.0); data::session_set(47,-1.0); data::session_set(48,-1.0);
    set_position("rocket-rig",[0.0,0.0,0.0]);
    set_visible("rocket-exhaust",false); set_visible("hover-tile",false); set_visible("cursor",true);
    set_ui_visible("world-context",false); set_ui_visible("machine-inspect-overlay",false);
    set_extra_panel(""); visuals::update_projects();
    set_scene_variable("selected", if get_scene_variable("demo_mode") { 1.0 } else { 3.0 });
}

fn set_menu(open) {
    set_object_variable("menu_open", open);
    set_ui_visible("menu-overlay", open);
    set_ui_enabled("menu-overlay", open);
    set_ui_visible("menu-open", !open);
    set_ui_visible("map-open", !open && !get_object_variable("map_open"));
    if open {
        data::session_set(46,-1.0); set_ui_visible("machine-inspect-overlay",false); set_ui_enabled("machine-inspect-overlay",false);
        set_extra_panel("");
        set_assembler(-1);
        set_map(false);
        close_storage();
        set_scene_variable("storage_alpha", 0.0);
        set_ui_visible("storage-overlay", false);
        set_ui_enabled("storage-panel", false);
        set_object_variable("journal_open", false);
        set_object_variable("journal_alpha", 0.0);
        set_ui_visible("journal-overlay", false);
        set_ui_enabled("journal-book", false);
    }
}

fn set_map(open) {
    set_object_variable("map_open", open);
    set_ui_visible("map-overlay", open);
    set_ui_enabled("map-overlay", open);
    set_ui_visible("map-open", !open && !get_object_variable("menu_open"));
    if open {
        data::session_set(46,-1.0); set_ui_visible("machine-inspect-overlay",false); set_ui_enabled("machine-inspect-overlay",false);
        set_extra_panel("");
        set_assembler(-1);
        close_storage();
        set_scene_variable("storage_alpha", 0.0);
        set_ui_visible("storage-overlay", false);
        set_ui_enabled("storage-panel", false);
        set_object_variable("journal_open", false);
        set_object_variable("journal_alpha", 0.0);
        set_ui_visible("journal-overlay", false);
        set_ui_enabled("journal-book", false);
    }
}

fn set_assembler(cell) {
    set_object_variable("assembler_cell", cell.to_float());
    set_ui_visible("assembler-overlay", cell >= 0);
    set_ui_enabled("assembler-overlay", cell >= 0);
    if cell >= 0 {
        data::session_set(46,-1.0); set_ui_visible("machine-inspect-overlay",false); set_ui_enabled("machine-inspect-overlay",false);
        close_storage();
        let recipes = get_object_list("recipes");
        if recipes[cell] == 0.0 { recipes[cell] = data::recipe_choices(get_scene_list("builds")[cell])[0]; set_object_list("recipes",recipes); }
        draw_assembler();
    }
}

fn draw_assembler() {
    let cell = get_object_variable("assembler_cell").to_int(); if cell < 0 { return; }
    let kind = get_scene_list("builds")[cell]; let recipe = get_object_list("recipes")[cell];
    if kind == 3.0 && get_scene_list("input_amounts")[cell] > 0.0 {
        recipe = data::smelt_output(get_scene_list("input_items")[cell]);
        let recipes = get_object_list("recipes"); recipes[cell] = recipe; set_object_list("recipes",recipes);
    }
    let cost = data::recipe_cost(recipe.to_int()); let output = get_scene_list("item_amounts")[cell];
    let a = if kind == 5.0 { get_scene_list("assembler_iron")[cell] } else { get_scene_list("input_amounts")[cell] };
    let b = if kind == 5.0 { get_scene_list("assembler_copper")[cell] } else { 0.0 };
    let stock = get_object_list("stock");
    let state = [cell, kind, recipe, output, a, b, get_scene_list("input_items")[cell],
        get_scene_list("items")[cell], get_object_list("power_live")[cell],
        get_object_list("rotation_turns")[cell] > 0.0, get_object_variable("phase"), data::free_build()].to_string() + stock.to_string();
    if !data::ui_view_changed(2, state) { return; }
    set_ui_text("assembler-title",data::build_name(kind) + " / OUTPUT RECIPE");
    set_ui_text("assembler-status","Output: " + data::item_name(recipe) + "\n" + if get_object_list("power_live")[cell] > 0.0 { "POWERED / " + power::power_demand(kind.to_int()).to_string() + " power" } else { "NO POWER / Wire to a supplied pole" });
    let input = a.to_int().to_string() + " " + data::item_name(if kind == 5.0 || a == 0.0 { cost[0].to_float() } else { get_scene_list("input_items")[cell] });
    if kind == 5.0 { input += " + " + b.to_int().to_string() + " " + data::item_name(cost[2].to_float()); }
    set_ui_text("assembler-buffer","Input: " + input + "\nOutput: " + output.to_int().to_string() + " " + data::item_name(if output > 0.0 { get_scene_list("items")[cell] } else { recipe }) + "    |    Buffer " + (a+b+output).to_int().to_string() + " / 100");
    let choices = data::recipe_choices(kind); let names = ["assembler-alloy","assembler-parts","assembler-concrete"];
    for i in 0..3 {
        let choice = choices[i]; let ingredients = data::recipe_cost(choice.to_int());
        set_ui_text(names[i],data::item_name(choice) + " x" + data::recipe_yield(choice.to_int()).to_int().to_string() + "\n" + data::cost_label(ingredients,"\n"));
        set_ui_background(names[i],if recipe == choice { [0.18,0.37,0.36,1.0] } else { [0.12,0.19,0.23,1.0] });
        set_ui_enabled(names[i],data::recipe_unlocked(choice.to_int()) && (output == 0.0 || recipe == choice));
    }
    set_ui_enabled("assembler-take",output > 0.0 && get_object_list("rotation_turns")[cell] == 0.0);
    let can_feed = false;
    if kind == 5.0 {
        can_feed = ((data::free_build() || stock[cost[0]] > 0.0) && a+b+output+1.0+max(0.0,cost[3]-b) <= 100.0) ||
            ((data::free_build() || stock[cost[2]] > 0.0) && a+b+output+1.0+max(0.0,cost[1]-a) <= 100.0);
    } else { can_feed = (data::free_build() || stock[cost[0]] > 0.0) && a+output+1.0+max(0.0,data::recipe_yield(recipe.to_int())-1.0) <= 100.0; }
    set_ui_enabled("assembler-feed",can_feed && get_object_list("rotation_turns")[cell] == 0.0);
}

fn set_extra_panel(name) {
    data::session_set(47,-1.0); data::session_set(48,-1.0);
    set_ui_visible("backpack-menu",false); set_ui_visible("backpack-drag",false);
    data::session_set(1,if name == "inventory" { 1.0 } else { 0.0 });
    data::session_set(2,if name == "rocket" { 1.0 } else if name == "dock" { 2.0 } else { 0.0 });
    for panel in ["inventory","rocket","dock"] {
        set_ui_visible("player-"+panel+"-overlay",name == panel);
        set_ui_enabled("player-"+panel+"-overlay",name == panel);
    }
    if name != "" {
        data::session_set(46,-1.0); set_ui_visible("machine-inspect-overlay",false); set_ui_enabled("machine-inspect-overlay",false);
        set_assembler(-1); close_storage();
        set_scene_variable("storage_alpha",0.0); set_ui_visible("storage-overlay",false);
        set_object_variable("journal_open",false); set_object_variable("journal_alpha",0.0);
        set_ui_visible("journal-overlay",false); set_ui_enabled("journal-book",false);
    }
    if name == "rocket" { update_destinations(); }
}

fn update_destinations() {
    set_ui_text("rocket-current", "CURRENT WORLD / " + data::planet_name());
    set_ui_text("rocket-moon", if data::on_moon() { "STELLAR-BX\nEarth / Return home" } else { "STELLA-Z2\nMoon / Next destination" });
    set_ui_text("rocket-launch", if data::on_moon() { "RETURN TO EARTH\nNo fuel required yet" } else { "LAUNCH TO MOON\nNo fuel required yet" });
    set_ui_enabled("rocket-launch", get_object_variable("phase") >= 7.0);
    set_ui_text("rocket-detail", if data::on_moon() { "STELLAR-BX / EARTH\nYour factories and explored regions are preserved.\nInventory travels with you. No fuel required yet." }
        else { "STELLA-Z2 / MOON\nPermanent night / Amorium, Moondust and rare Techtorium\nBring construction supplies. Return trips available without fuel." });
}
