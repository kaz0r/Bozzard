// Co-op cursors use the same world tiles as the local player. Remote markers and
// labels are presentation only; their positions never enter factory state.
import "factory-interiors" as interiors;
import "factory-data" as data;
import "factory-grid" as grid;

fn update(dt,obscured) {
    let colors=[[0.12,0.44,1.0],[0.95,0.16,0.18],[1.0,0.49,0.08],[0.18,0.83,0.34]];
    let frame=network_state();
    let active=network_active() && (frame.contains("factory_host") || frame.contains("factory_guest"));
    // Clearing a departed session is a one-time presentation change; solo ticks
    // must not repeatedly dirty four hidden models and their UI labels.
    let changed=data::ui_view_changed(5,if active {"co-op"}else{"solo"});
    if !active && !changed {return;}
    let slots=[(),(),(),()];let local=if active {frame.local}else{0};
    if active {for member in frame.members {slots[member.slot]=member;}}
    let x=grid::world_x(get_scene_variable("cursor_x"));let z=grid::world_z(get_scene_variable("cursor_z"));
    for slot in 0..4 {
        let model="coop-player-"+slot.to_string();let label="coop-name-"+slot.to_string();let member=slots[slot];
        let visible=false;let named=false;
        if member!=() {
            if member.peer==local {set_color("cursor",colors[slot]);}
            else if member.position.planet==data::session_value(7).to_int() {
                let at=member.position;
                let cx=floor((at.x.to_float()+7.0)/15.0).to_int();let cz=floor((at.z.to_float()+7.0)/15.0).to_int();
                visible=get_object_list_item("resident",grid::chunk_id(cx,cz))>0.0 && interiors::visible_at(at.x,at.z);
                for flight in frame.flights {if flight.peer==member.peer{visible=false;}}
                if visible {
                    let from=get_position(model);let target=[at.x.to_float(),0.14,at.z.to_float()];
                    let distance=pow(from[0]-target[0],2.0)+pow(from[2]-target[2],2.0);
                    let t=if from[1]<0.0 || distance>16.0 {1.0}else{min(1.0,dt*16.0)};
                    let pos=[lerp(from[0],target[0],t),0.14,lerp(from[2],target[2],t)];
                    set_position(model,pos);set_color(model,colors[slot]);
                    named=!obscured && pow(target[0]-x,2.0)+pow(target[2]-z,2.0)<=36.0;
                    if named {set_ui_text(label,member.name);set_ui_world_position(label,[pos[0],0.7,pos[2]]);}
                }
            }
        }
        set_visible(model,visible);set_ui_visible(label,named);
        if !visible {set_position(model,[0.0,-1000.0,0.0]);}
    }
    if !active {set_color("cursor",[0.16,0.95,0.70]);}
}
