// Last authoritative production beat, without render handles or saved state.
// Recording both planets lets guests watch factories where the host has no models.
import "factory-grid" as grid;
fn clear() {
    set_object_list("factory-transports","earth",[]);
    set_object_list("factory-transports","moon",[]);
    set_object_list("factory-transports","beats",[-1.0,-1.0]);
    set_object_list("factory-transports","guest_beat",[-1.0]);
}
fn record(planet,tick,ids,routes,items,builds) {
    if !network_active() || !network_state().contains("factory_host") { return; }
    let pages=grid::empty_text(867);
    for i in 0..ids.len() {
        if routes[i]==0.0 {continue;}
        let to=routes[i].to_int()-1; let page=ids[i]/75;
        pages[page]+=ids[i].to_string()+","+ids[to].to_string()+","+items[i].to_int().to_string()+","+
            builds[i].to_int().to_string()+","+builds[to].to_int().to_string()+";";
    }
    set_object_list("factory-transports",if planet==0 {"earth"}else{"moon"},pages);
    let beats=get_object_list("factory-transports","beats");beats[planet]=tick.to_float();
    set_object_list("factory-transports","beats",beats);
}
