// Data-only save bridge: native code validates and writes files off the game thread.
// session: 116 command, 117 slot, 120 world time, 121 autosave time,
// 122 authority (solo/host/guest), 123 browser mode, 125 I/O busy, 126 world generation.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-world" as world;
import "factory-chunks" as chunks;
import "factory-item_fx" as item_fx;
import "factory-transports" as transports;
import "factory-panels" as panels;
import "factory-power" as power;
import "factory-visuals" as visuals;
import "factory-environment" as environment;

fn open(mode) {
    if data::session_value(122)==2.0 {set_scene_variable("message","Only the host can save or load the world."); return;}
    if data::session_value(125)>0.0 {set_scene_variable("message","A save operation is already running."); return;}
    data::session_set(123,mode.to_float());
    set_ui_visible("saves-overlay",true);set_ui_enabled("saves-overlay",true);
    set_ui_text("saves-title",if mode==1 {"SAVE WORLD"}else{"LOAD WORLD"});
    set_ui_text("saves-status",if mode==1 {"Choose a slot. Saving replaces that slot. Auto-save runs every 20 minutes."}else{"Choose a save to continue. Loading replaces the current session."});
    for slot in 0..6 {set_ui_enabled("save-slot-"+slot.to_string(),false);}
    data::session_set(116,6.0);
}
fn close() {
    data::session_set(123,0.0);
    set_ui_visible("saves-overlay",false);set_ui_enabled("saves-overlay",false);
}
fn prepare_save(slot) {
    if get_object_variable("title_open") || data::session_value(122)==2.0 || data::session_value(44)>0.0 || data::session_value(125)>0.0 {return false;}
    world::archive_chunk();power::update_power();
    data::session_set(117,slot.to_float());data::session_set(116,1.0);
    true
}
fn clear_view() {
    transports::clear();
    item_fx::clear();power::clear_power();
    world::reset_planet();
    visuals::clear_visuals("node_visuals");visuals::clear_visuals("build_visuals");
    panels::set_menu(false);panels::set_map(false);panels::set_assembler(-1);
    panels::set_extra_panel("");panels::close_storage();
    set_scene_variable("storage_alpha",0.0);set_object_variable("journal_open",false);set_object_variable("journal_alpha",0.0);
    for panel in ["journal-overlay","storage-overlay","machine-inspect-overlay","world-context","stack-menu","stack-drag","nearby-tooltip"] {set_ui_visible(panel,false);}
}
fn restore_view() {
    let session=get_object_list("session");
    session[3]=11.0;session[4]=-1.0;session[5]=-2.0;session[46]=-1.0;session[47]=-1.0;session[48]=-1.0;
    session[116]=0.0;set_object_list("session",session);
    set_object_variable("wire_start",-1.0);set_object_variable("assembler_cell",-1.0);
    set_scene_variable("storage_render_revision",-1.0);set_scene_variable("storage_render_cell",-1.0);set_scene_variable("storage_menu_alpha",0.0);
    set_object_variable("title_open",false);set_scene_variable("started",true);
    set_ui_visible("title-overlay",false);set_ui_enabled("title-overlay",false);
    for panel in ["game-hud","game-panels"] {set_ui_visible(panel,true);set_ui_enabled(panel,true);}
    close();
    set_object_list("ui_views",["","","","","",""]);set_object_variable("map_state","");
    let visited=grid::empty_numbers(289);let nodes=data::region_data("chunk_nodes");
    for id in 0..289 {if nodes[id]!="" {visited[id]=1.0;}}
    set_object_list("visited",visited);
    let cx=get_scene_variable("chunk_x").to_int();let cz=get_scene_variable("chunk_z").to_int();
    chunks::load_chunk_visuals(cx,cz);world::restore_chunk(cx,cz);
    set_position("cursor",[grid::world_x(get_scene_variable("cursor_x")),0.115,grid::world_z(get_scene_variable("cursor_z"))]);
    set_position("camera-rig",[(cx*15).to_float(),0.0,(cz*15).to_float()]);
    set_rotation("camera-rig",[0.0,0.0,0.0]);set_scene_variable("camera_heading",0.0);set_scene_variable("camera_pending",0.0);set_scene_variable("camera_progress",1.0);
    set_object_variable("camera_pan_progress",1.0);set_object_variable("camera_zoom",19.0);set_object_variable("camera_zoom_target",19.0);set_camera_size("camera",19.0);
    set_object_list("rotation_cells",[]);set_object_list("rotation_turns",grid::empty_numbers(225));
    set_visible("ground",!data::on_moon());set_visible("cursor",true);set_visible("rocket-exhaust",false);set_visible("hover-tile",false);
    set_position("rocket-rig",[0.0,0.0,0.0]);
    power::update_power();visuals::update_projects();
    environment::update_daylight(if data::on_moon(){-1.0}else{sin(data::session_value(120)*0.10)});
    set_scene_variable("message","World loaded. Your factories are running.");
}
// True means this tick is a consistent snapshot/restore boundary.
fn update(dt) {
    let request=data::session_value(116);
    if request==3.0 {clear_view();data::session_set(116,4.0);return true;}
    if request==5.0 {restore_view();return true;}
    if data::session_value(123)>0.0 {
        if input_pressed("Escape") {close();return true;}
        for event in ui_events() {
            if event.kind!="activate" {continue;}
            if event.target=="saves-close" {close();return true;}
            if event.target.starts_with("save-slot-") && data::session_value(125)==0.0 {
                let slot=parse_int(event.target.sub_string(10));
                if slot<0 || slot>=6 {continue;}
                if data::session_value(123)==1.0 {if slot>0 && prepare_save(slot){return true;}}
                else {data::session_set(117,slot.to_float());data::session_set(116,2.0);return true;}
            }
        }
    }
    if !get_object_variable("title_open") {
        data::session_set(120,data::session_value(120)+dt);
        if data::session_value(122)!=2.0 {
            data::session_set(121,data::session_value(121)+dt);
            if data::session_value(121)>=1200.0 && data::session_value(125)==0.0 && request==0.0 {return prepare_save(0);}
        }
    }
    false
}
