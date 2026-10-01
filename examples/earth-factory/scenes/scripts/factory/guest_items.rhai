// Animate only transfers already accepted by the host. These functions never
// change buffers, production clocks, inventories, or routing decisions.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-item_fx" as item_fx;
import "factory-visuals" as visuals;
fn new_beat() {get_object_list("factory-transports","guest_beat")[0]!=get_scene_variable("ticks")}
fn finish() {
    if !new_beat() {return;}
    item_fx::animate(1.0);item_fx::retire_previous();item_fx::write_motions([]);
}
fn value(name,id) {
    if id/225==grid::current_chunk() {return get_scene_list_item(name,id%225);}
    grid::unpack_numbers(data::region_entry("cache_"+name,id/225),225)[id%225]
}
fn start() {
    if !new_beat() {return;}
    set_object_list("factory-transports","guest_beat",[get_scene_variable("ticks")]);
    let motions=[];let owned=[];let handles=[];let claimed=#{};
    let resident=get_object_list("resident");let pool=get_scene_list("item_pool");
    for item in network_state().items {
        let a=item.from;let b=item.to;
        if item.planet!=data::session_value(7).to_int() ||
            (resident[a/225]==0.0 && resident[b/225]==0.0) {continue;}
        // A later edit in this beat may have removed or replaced an endpoint.
        if value("builds",a)!=item.source.to_float() || value("builds",b)!=item.target.to_float() {continue;}
        let retained=resident[b/225]>0.0 && (data::belt(item.target.to_float()) || [7,8].contains(item.target)) &&
            value("item_amounts",b)>0.0 && !claimed.contains(b.to_string());
        let target=if retained {item_fx::read_handles([b])[0]}else{""};
        if target=="" {
            let pos=item_fx::position(a,item.source.to_float(),value("facings",a),visuals::item_height(item.source.to_float(),item.kind.to_float()));
            if pool.len()>0 {target=pool.pop();set_position(target,pos);set_visible(target,true);}
            else {target=spawn_prefab("item",pos);}
        }
        item_fx::appearance(target,item.kind.to_float());
        if retained {owned.push(b);handles.push(target);claimed[b.to_string()]=true;}
        motions.push([a,b,target,visuals::item_height(item.source.to_float(),item.kind.to_float()),visuals::item_height(item.target.to_float(),item.kind.to_float()),!retained]);
    }
    set_scene_list("item_pool",pool);item_fx::assign(owned,handles);item_fx::write_motions(motions);
}
fn animate() {
    let fraction=min(1.0,(get_scene_variable("clock")+data::session_value(124))/0.32);
    item_fx::animate(fraction);
    if fraction>=1.0 && get_scene_list_item("motion_visuals",867)!="" {
        item_fx::retire_previous();item_fx::write_motions([]);
    }
}
