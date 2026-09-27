import "factory-data" as data;
import "factory-net_input" as net;
import "factory-panels" as panels;
import "factory-world" as world;

fn start() {
    if data::dev_world() { set_scene_variable("message","Dev World is a fixed showcase. Create a Survival or Creative world to travel."); return; }
    if net::guest() {net::send("travel",#{});panels::set_extra_panel("");return;}
    if data::session_value(44)>0.0 || get_object_variable("phase")<7.0 || get_scene_variable("demo_mode") ||
        get_scene_variable("chunk_x")!=0.0 || get_scene_variable("chunk_z")!=0.0 ||
        abs(get_scene_variable("cursor_x"))>1.0 || abs(get_scene_variable("cursor_z")-1.0)>1.0 { return; }
    panels::set_extra_panel("");
    data::session_set(44,1.0); data::session_set(45,0.0); data::session_set(40,0.0);
    set_visible("cursor",false); set_visible("rocket-exhaust",true);
    set_scene_variable("message","Lifting off for " + if data::on_moon() { "STELLAR-BX" } else { "STELLA-Z2" } + "...");
}
fn update(dt) {
    let phase=data::session_value(44); if phase==0.0 { return false; }
    let time=data::session_value(45)+dt;
    if phase==1.0 {
        let t=min(1.0,time/2.0); set_position("rocket-rig",[0.0,32.0*t*t,0.0]);
        if time>=2.0 {
            world::travel_to_other_planet();
            phase=2.0; time=0.0; set_position("rocket-rig",[0.0,32.0,0.0]);
            set_scene_variable("message","Descending onto " + data::planet_name() + "...");
        }
    } else {
        let t=min(1.0,time/2.4); let eased=t*t*(3.0-2.0*t);
        set_position("rocket-rig",[0.0,32.0*(1.0-eased),0.0]);
        if time>=2.4 {
            phase=0.0; time=0.0; set_position("rocket-rig",[0.0,0.0,0.0]);
            set_visible("rocket-exhaust",false); set_visible("cursor",true);
            set_scene_variable("message","Landed on " + data::planet_name() + ". Inventory and factories preserved. [E] Board rocket");
        }
    }
    data::session_set(44,phase); data::session_set(45,time);
    true
}
