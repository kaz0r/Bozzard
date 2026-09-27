// Present accepted guest edits without rebuilding the host's world or changing
// its camera. Native code has already validated and patched the data arrays.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-item_fx" as item_fx;
import "factory-visuals" as visuals;
import "factory-panels" as panels;
import "factory-guest_items" as guest_items;
import "factory-power" as power;

fn active() { network_active() && (network_state().contains("factory_host") || network_state().contains("factory_guest")) }
fn value(name,id) {
    if id/225==grid::current_chunk() { return get_scene_list_item(name,id%225); }
    grid::unpack_numbers(data::region_entry("cache_"+name,id/225),225)[id%225]
}
fn machine(id) {
    if id/225==grid::current_chunk() { return get_scene_list_item("build_visuals",id%225); }
    grid::unpack_text(get_object_list_item("cache_visuals_"+((id%225)/75).to_string(),id/225),75)[id%75]
}
fn synchronize() {
    if !active() { return; }
    let frame=network_state();
    if frame.presentation==data::session_value(118).to_int() { return; }
    let guest=frame.contains("factory_guest");
    if guest {guest_items::finish();}
    for change in frame.changes {
        let id=change.id; let region=id/225; let cell=id%225;
        if get_object_list_item("resident",region)==0.0 { continue; }
        if change.build {
            let target=machine(id); if target!="" { destroy_prefab(target); }
            let kind=value("builds",id); target="";
            if kind>0.0 {
                target=spawn_prefab(data::build_asset(kind),[grid::global_x(id),0.08,grid::global_z(id)]);
                set_rotation(target,[0.0,-value("facings",id)*90.0,0.0]);
            }
            let name="cache_visuals_"+(cell/75).to_string();
            let page=grid::unpack_text(get_object_list_item(name,region),75); page[cell%75]=target;
            grid::cache_put(name,region,grid::pack_text(page));
            if region==grid::current_chunk() {
                let handles=get_scene_list("build_visuals"); handles[cell]=target; set_scene_list("build_visuals",handles);
                let turns=get_object_list("rotation_turns"); turns[cell]=0.0; set_object_list("rotation_turns",turns);
                let remaining=[]; for entry in get_object_list("rotation_cells") { if entry.to_int()!=cell { remaining.push(entry); } }
                set_object_list("rotation_cells",remaining);
                if kind!=4.0 && get_scene_variable("storage_cell").to_int()==cell { panels::close_storage(); }
                if kind!=3.0 && kind!=5.0 && kind!=11.0 && get_object_variable("assembler_cell").to_int()==cell { panels::set_assembler(-1); }
                if kind!=1.0 && kind!=7.0 && kind!=8.0 && data::session_value(46).to_int()==cell {
                    data::session_set(46,-1.0);set_ui_visible("machine-inspect-overlay",false);set_ui_enabled("machine-inspect-overlay",false);
                }
            }
        }
        if change.item {
            let page=grid::unpack_text(get_scene_list_item("factory_visuals",id/75),75);
            let target=if region==grid::current_chunk() { get_scene_list_item("item_visuals",cell) } else { page[id%75] };
            if target!="" { item_fx::cancel_motion([target]); }
            if value("item_amounts",id)>0.0 {
                let pos=[grid::global_x(id),visuals::item_height(value("builds",id)),grid::global_z(id)];
                if target=="" { target=spawn_prefab("item",pos); } else { set_position(target,pos); }
                set_color(target,data::item_color(value("items",id)));
            } else if target!="" { item_fx::recycle([target]); target=""; }
            page[id%75]=target; let pages=get_scene_list("factory_visuals"); pages[id/75]=grid::pack_text(page); set_scene_list("factory_visuals",pages);
            if region==grid::current_chunk() { let handles=get_scene_list("item_visuals"); handles[cell]=target; set_scene_list("item_visuals",handles); }
        }
    }
    if guest {guest_items::start();}
    data::session_set(118,frame.presentation.to_float());
}
fn rotating(planet,id) {
    rotation_mask(planet).contains(id.to_string())
}
fn rotation_mask(planet) {
    let mask=#{};
    if !active() { return mask; }
    for entry in network_state().rotations {
        if entry.at.planet!=planet { continue; }
        let cx=floor((entry.at.x.to_float()+7.0)/15.0).to_int(); let cz=floor((entry.at.z.to_float()+7.0)/15.0).to_int();
        let id=grid::chunk_id(cx,cz)*225+grid::index(entry.at.x-cx*15,entry.at.z-cz*15);
        mask[id.to_string()]=true;
    }
    mask
}
fn animate() {
    if !active() { return; }
    for entry in network_state().rotations {
        if entry.at.planet!=data::session_value(7).to_int() { continue; }
        let cx=floor((entry.at.x.to_float()+7.0)/15.0).to_int(); let cz=floor((entry.at.z.to_float()+7.0)/15.0).to_int();
        let region=grid::chunk_id(cx,cz); if get_object_list_item("resident",region)==0.0 { continue; }
        let id=region*225+grid::index(entry.at.x-cx*15,entry.at.z-cz*15); let target=machine(id);
        if target=="" { continue; }
        let t=min(1.0,entry.progress.to_float()+if network_state().contains("factory_guest") {data::session_value(124)/0.6}else{0.0});
        let lift=if t<0.22 { grid::smooth(t/0.22) } else if t<0.78 {1.0} else {1.0-grid::smooth((t-0.78)/0.22)};
        let angle=value("facings",id)+grid::smooth(clamp((t-0.22)/0.56,0.0,1.0));
        set_position(target,[grid::global_x(id),0.08+0.55*lift,grid::global_z(id)]);
        set_rotation(target,[0.0,-90.0*angle,0.0]);
        power::animate_machine_effects(id);
    }
}
