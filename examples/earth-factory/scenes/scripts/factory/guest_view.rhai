import "factory-net_input" as net;
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-chunks" as chunks;
import "factory-world" as world;
import "factory-host_view" as host_view;
import "factory-item_fx" as item_fx;
import "factory-persistence" as persistence;

fn prepare() {
    if !net::guest() {return false;}
    let frame=network_state();
    if frame.phase=="clear" {
        if data::session_value(119)!=1.0 {persistence::clear_view();data::session_set(119,1.0);}
        return true;
    }
    if frame.phase=="restore" {
        if data::session_value(119)!=3.0 {persistence::restore_view();data::session_set(119,3.0);}
        return true;
    }
    if frame.phase!="live" {return true;}
    host_view::synchronize();
    if frame.destination!=() {
        let at=frame.destination;let cx=floor((at.x.to_float()+7.0)/15.0).to_int();let cz=floor((at.z.to_float()+7.0)/15.0).to_int();
        if get_scene_variable("chunk_x").to_int()!=cx || get_scene_variable("chunk_z").to_int()!=cz {
            item_fx::archive_current();chunks::load_chunk_visuals(cx,cz);world::restore_chunk(cx,cz);
        }
        set_scene_variable("cursor_x",(at.x-cx*15).to_float());set_scene_variable("cursor_z",(at.z-cz*15).to_float());
    }
    false
}
fn flight(dt) {
    let active=false;
    for entry in network_state().flights {
        if entry.peer!=network_state().local {continue;}
        active=true;
        let elapsed=entry.elapsed.to_float()+data::session_value(124);
        let before=data::session_value(7).to_int()!=entry.destination;
        let t=if before {min(1.0,elapsed/2.0)}else{clamp((elapsed-2.0)/2.4,0.0,1.0)};
        set_position("rocket-rig",[0.0,if before {32.0*t*t}else{32.0*(1.0-grid::smooth(t))},0.0]);
        data::session_set(44,if before {1.0}else{2.0});
    }
    if !active {data::session_set(44,0.0);set_position("rocket-rig",[0.0,0.0,0.0]);}
    set_visible("cursor",!active);set_visible("rocket-exhaust",active);
    active
}
