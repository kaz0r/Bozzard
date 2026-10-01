// Game entry point: coordinate input, simulation and presentation modules.
import "factory-architecture" as architecture;
import "factory-interiors" as interiors;
import "factory-pointer" as pointer;
import "factory-net_input" as net;
import "factory-guest_view" as guest_view;
import "factory-players" as players;
import "factory-host_view" as host_view;
import "factory-persistence" as persistence;
import "factory-flight" as flight;
import "factory-guest_items" as guest_items;
import "factory-inspection" as inspection;
import "factory-backpack" as backpack;
import "factory-building" as building;
import "factory-chunks" as chunks;
import "factory-data" as data;
import "factory-deposits" as deposits;
import "factory-debris" as debris;
import "factory-panels" as panels;
import "factory-environment" as environment;
import "factory-grid" as grid;
import "factory-hud" as hud;
import "factory-inventory" as inventory;
import "factory-journal" as journal;
import "factory-machines" as machines;
import "factory-navigation" as navigation;
import "factory-power" as power;
import "factory-progression" as progression;
import "factory-simulation" as simulation;
import "factory-visuals" as visuals;
import "factory-world" as world;
import "factory-wind" as wind;
import "factory-clock" as clock;

fn on_start(me) {
    set_visible("hover-tile",false); set_visible("rocket-exhaust",false);
    for slot in 0..4 {set_visible("coop-player-"+slot.to_string(),false);}
    if net::guest() {return;}
    visuals::update_projects();
    if get_scene_variable("demo_mode") || !get_object_variable("title_open") {
        navigation::show_title(false);
        let seed = get_scene_variable("seed").to_int();
        world::begin_world(if seed > 0 { seed } else { fresh_seed() }, data::dev_world());
    } else { navigation::show_title(true); }
}

fn on_update(me, dt) {
    if guest_view::prepare() {return;}
    host_view::synchronize();
    let guest=net::guest();
    if !guest && persistence::update(dt) { return; }
    if guest {data::session_set(124,data::session_value(124)+dt);}
    let saves_open=data::session_value(123)>0.0;
    if get_object_variable("title_open") { wind::hide();if !saves_open {navigation::update_title();} return; }
    let flight_active=if guest {guest_view::flight(dt)}else{flight::update(dt)};
    let inspecting=data::session_value(46)>=0.0;
    let menu_active = saves_open || if !flight_active { navigation::update_menu() } else { false };
    if get_object_variable("title_open") { return; }
    let map_active = if !flight_active && !menu_active { navigation::update_map_input() } else { false };
    let assembler_active = get_object_variable("assembler_cell") >= 0.0;
    let extra_active = if !flight_active && !menu_active && !map_active && !inspecting { navigation::update_extra_input() } else { false };
    if !flight_active && !menu_active && !map_active && !extra_active && data::session_value(41)==0.0 {
        if inspecting { inspection::update(); }
        else if assembler_active { machines::update_assembler(); }
        else { journal::update_journal(dt); inventory::update_storage(dt); }
    }
    let inventory_active = net::blocked() || flight_active || data::session_value(44)>0.0 || inspecting || data::session_value(46)>=0.0 ||
        menu_active || map_active || extra_active || assembler_active || get_object_variable("assembler_cell")>=0.0 ||
        get_scene_variable("storage_open") || get_scene_variable("storage_alpha")>0.0 ||
        get_object_variable("journal_open") || get_object_variable("journal_alpha")>0.0;
    inventory_active = pointer::update(inventory_active) || inventory_active;
    let x = get_scene_variable("cursor_x").to_int();
    let z = get_scene_variable("cursor_z").to_int();
    let dx = 0;
    let dz = 0;
    if !inventory_active {
        if input_pressed("a") { dx -= 1; }
        if input_pressed("d") { dx += 1; }
        if input_pressed("w") { dz -= 1; }
        if input_pressed("s") { dz += 1; }
    }
    // Keep movement consistent on screen after each completed camera turn.
    let view_turn = (4 - (get_scene_variable("camera_heading") / 90.0).to_int()) % 4;
    let step_x=grid::layout_x(dx,dz,0,view_turn);let step_z=grid::layout_z(dx,dz,0,view_turn);
    if guest {if step_x!=0 || step_z!=0 {net::send("move",#{x:step_x,z:step_z});}}
    else if interiors::path_clear(grid::world_x(x).to_int(),grid::world_z(z).to_int(),grid::world_x(x).to_int()+step_x,grid::world_z(z).to_int()+step_z) {x+=step_x;z+=step_z;}
    if !guest && !inventory_active {
        let position = world::explore(x, z);
        x = position[0]; z = position[1];
    }
    set_scene_variable("cursor_x", x.to_float());
    set_scene_variable("cursor_z", z.to_float());
    set_position("cursor", [grid::world_x(x), 0.115, grid::world_z(z)]);

    if !guest && !inventory_active && input_pressed("n") {
        world::begin_world(fresh_seed(),data::dev_world());
        return;
    }

    if !inventory_active { building::action_bar_input(); }
    progression::gather(dt, inventory_active);
    if !inventory_active && input_pressed("r") {
        if input_held("Ctrl") || input_pressed("Ctrl") {
            set_scene_variable("camera_pending", get_scene_variable("camera_pending") + 1.0);
        } else if get_scene_variable("selected") == 10.0 {
            set_object_variable("wire_start", -1.0); set_scene_variable("message", "Wire cancelled.");
        } else {
            building::rotate_selected();
        }
    }
    environment::update_camera(dt);
    environment::update_zoom(dt, inventory_active);
    chunks::update_chunk_residency();
    navigation::update_map_display();
    visuals::animate_machines(dt);
    host_view::animate();
    if !inventory_active && input_pressed("Space") { building::place_selected(); }
    if !inventory_active && input_pressed("x") { building::remove_selected(); }

    power::update_power();
    let clock = get_scene_variable("clock") + if guest {0.0}else{dt};
    while !guest && clock >= 0.32 {
        simulation::factory_step();
        clock -= 0.32;
    }
    set_scene_variable("clock", clock);
    if guest {guest_items::animate();}else{visuals::animate_items(clock / 0.32);}

    let daylight = clock::daylight(data::session_value(120),data::session_value(7).to_int());
    environment::update_daylight(daylight);

    if inventory_active {
        // The modal covers world interaction: avoid searching machines and
        // rebuilding a world label that would immediately be hidden again.
        set_ui_visible("nearby-tooltip", false);
        set_scene_variable("tooltip_cell", -1.0);
        set_scene_variable("tooltip_alpha", 0.0);
    } else {
        hud::update_tooltip(dt, x, z, get_scene_list("nodes"), get_scene_list("builds"));
    }
    visuals::update_projects();
    interiors::update(dt);
    wind::update();
    players::update(dt,inventory_active);
    hud::update_hud(dt, daylight);
    hud::update_debug_hud(dt);
}
