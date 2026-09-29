import "factory-flight" as flight;
import "factory-net_input" as net;
import "factory-persistence" as persistence;
import "factory-backpack_ui" as backpack_ui;
// Rhai gameplay: navigation.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-inventory" as inventory;
import "factory-panels" as panels;
import "factory-world" as world;

fn update_map_input() {
    let was_open = get_object_variable("map_open");
    let toggle = input_pressed("M");
    for event in ui_events() {
        if event.kind == "activate" && (event.target == "map-open" || (was_open && event.target == "map-close")) {
            toggle = true;
        }
    }
    if toggle { panels::set_map(!was_open); }
    // Closing the map consumes this frame too, so simultaneous build/move keys
    // cannot leak through to the world beneath it.
    was_open || get_object_variable("map_open")
}

fn update_map_display() {
    if !get_object_variable("map_open") { return; }
    let visited = get_object_list("visited");
    let resident = get_object_list("resident");
    let cx = get_scene_variable("chunk_x").to_int();
    let cz = get_scene_variable("chunk_z").to_int();
    let state = data::planet_name() + cx.to_string() + "," + cz.to_string() + "|" + visited.to_string() + "|" + resident.to_string();
    if state == get_object_variable("map_state") { return; }
    set_object_variable("map_state", state);
    let current = grid::chunk_id(cx, cz);
    let loaded = 0;
    let explored = 0;
    let radius = data::planet_radius();
    let width = if data::dev_world() {2}else{radius * 2 + 1};
    let spacing = 391.0 / width.to_float();
    set_ui_text("map-title", data::planet_name() + " / REGION MAP");
    for coordinate in [-8, 0, 8] {
        let text = if data::dev_world() { if coordinate<0 {"0"}else if coordinate>0 {"1"}else{""} }
            else if coordinate < 0 { "-" + radius.to_string() } else if coordinate > 0 { "+" + radius.to_string() } else { "0" };
        set_ui_text("map-axis-x-" + coordinate.to_string(), text);
        set_ui_text("map-axis-z-" + coordinate.to_string(), text);
    }
    for id in 0..289 {
        let x = id % 17 - 8; let z = id / 17 - 8;
        let visible = data::chunk_allowed(x,z);
        let widget = "map-cell-" + id.to_string();
        set_ui_visible(widget, visible);
        if !visible { continue; }
        let origin=if data::dev_world(){0}else{radius};
        set_ui_offset(widget, 50.0 + (x + origin).to_float() * spacing, 102.0 + (z + origin).to_float() * spacing);
        set_ui_size(widget, spacing - 3.0, spacing - 3.0);
        if visited[id] > 0.0 { explored += 1; }
        if resident[id] > 0.0 { loaded += 1; }
        let color = if id == current { [0.64, 0.29, 0.085, 1.0] }
            else if resident[id] > 0.0 { [0.13, 0.38, 0.26, 1.0] }
            else if visited[id] > 0.0 { [0.11, 0.17, 0.20, 1.0] }
            else { [0.018, 0.032, 0.026, 1.0] };
        set_ui_background("map-cell-" + id.to_string(), color);
    }
    set_ui_text("map-region", "Region " + cx.to_string() + ", " + cz.to_string());
    set_ui_text("map-counts", loaded.to_string() + " loaded\n" + explored.to_string() + " explored / " + (width * width).to_string() + " regions");
}

fn update_menu() {
    let was_open = get_object_variable("menu_open");
    let toggle = input_pressed("Escape");
    for event in ui_events() {
        if event.kind != "activate" { continue; }
        if event.target == "menu-open" || (was_open && event.target == "menu-continue") { toggle = true; }
        if was_open && event.target == "menu-exit" { quit_game(); }
        if was_open && event.target == "menu-save" { persistence::open(1); }
        if was_open && event.target == "menu-load" { persistence::open(2); }
        if was_open && event.target == "menu-main-menu" { show_title(true); return true; }
    }
    if toggle { panels::set_menu(!was_open); }
    // Also consume gameplay input on the frame Continue/Escape closes the menu.
    was_open || get_object_variable("menu_open")
}

// Stellar-IX session entry. The title owns input; no factory ticks before creation.
fn show_title(open) {
    set_object_variable("title_open", open);
    set_ui_visible("title-overlay", open);
    set_ui_enabled("title-overlay", open);
    set_ui_visible("game-hud", !open);
    set_ui_enabled("game-hud", !open);
    set_ui_visible("game-panels", !open);
    set_ui_enabled("game-panels", !open);
    if open {
        panels::set_menu(false);
        panels::set_map(false);
        panels::set_assembler(-1);
        panels::close_storage();
        set_object_variable("wire_start", -1.0);
        draw_title();
    }
}

fn draw_title() {
    let creative = (data::session_value(0) > 0.0);
    set_ui_text("title-create", if creative { "Create Creative world" } else { "Create Survival world" });
    for mode in ["survival", "creative"] {
        let selected = (mode == "creative") == creative;
        set_ui_background("title-" + mode, if selected { [0.10, 0.16, 0.56, 1.0] } else { [0.093, 0.102, 0.16, 1.0] });
    }
}

fn update_title() {
    if net::guest() {return;}
    for event in ui_events() {
        if event.kind != "activate" { continue; }
        if event.target == "title-exit" { quit_game(); }
        if event.target == "title-load" { persistence::open(2); }
        if event.target == "title-dev" {
            set_scene_variable("demo_mode",false);set_object_variable("creative",true);
            show_title(false);world::begin_world(1,true);return;
        }
        if event.target == "title-survival" || event.target == "title-creative" {
            data::session_set(0, if event.target == "title-creative" { 1.0 } else { 0.0 }); draw_title();
        }
        if event.target == "title-create" {
            set_scene_variable("demo_mode", false);
            set_object_variable("creative", (data::session_value(0) > 0.0));
            show_title(false);
            world::begin_world(fresh_seed());
            return;
        }
    }
}

fn update_extra_input() {
    let was = data::session_value(1) > 0.0 || data::session_value(2) > 0.0;
    if input_pressed("I") {
        if get_object_variable("phase") < 2.0 { set_scene_variable("message","Player inventory unlocks in Tier 1 Phase 3. J shows your carried materials."); return true; }
        panels::set_extra_panel(if data::session_value(1) > 0.0 { "" } else { "inventory" });
    }
    if input_pressed("E") && data::session_value(1) == 0.0 {
        if data::session_value(2) > 0.0 { panels::set_extra_panel(""); }
        else if !get_object_variable("journal_open") && get_object_variable("journal_alpha") <= 0.0 &&
            get_object_variable("assembler_cell") < 0.0 && !get_scene_variable("storage_open") &&
            get_scene_variable("storage_alpha") <= 0.0 && get_scene_variable("chunk_x") == 0.0 && get_scene_variable("chunk_z") == 0.0 {
            let x = get_scene_variable("cursor_x"); let z = get_scene_variable("cursor_z");
            let phase = get_object_variable("phase");
            let exact = z == 1.0 && (x == 0.0 || x == 2.0);
            if exact || inventory::nearby_interactable() < 0 {
                if phase >= 7.0 && abs(x) <= 1.0 && abs(z-1.0) <= 1.0 { panels::set_extra_panel("rocket"); }
                else if phase >= 5.0 && abs(x-2.0) <= 1.0 && abs(z-1.0) <= 1.0 { panels::set_extra_panel("dock"); }
            }
        }
    }
    for event in ui_events() {
        if event.kind != "activate" { continue; }
        if was && (event.target == "player-inventory-close" || event.target == "player-rocket-close" || event.target == "player-dock-close") { panels::set_extra_panel(""); }
        if data::session_value(2) == 1.0 && event.target == "rocket-moon" { panels::update_destinations(); }
        if was && data::session_value(2) == 1.0 && event.target == "rocket-launch" { flight::start(); return true; }
    }
    if data::session_value(1) > 0.0 { backpack_ui::update(); }
    was || data::session_value(1) > 0.0 || data::session_value(2) > 0.0
}
