// One animation pass over registered resident rotors, plus eight pooled gust
// streaks. Calm frames do not write transforms or scan region/machine archives.
import "factory-grid" as grid;
import "factory-renewables" as renewables;

fn motion() {
    let seed=get_scene_variable("seed").to_int();let planet=get_object_list_item("controller","session",7).to_int();
    let time=get_object_list_item("controller","session",120);
    // Smooth between host snapshots without extrapolating into a calm period.
    if get_object_list_item("controller","session",122)==2.0 && renewables::wind_active(seed,planet,time) {
        let cycle=renewables::wind_cycle(time);let window=renewables::wind_window(seed,planet,cycle);
        time=min(time+min(get_object_list_item("controller","session",124),0.25),cycle.to_float()*renewables::wind_cycle_seconds()+window.start+window.duration);
    }
    renewables::wind_motion(seed,planet,time)
}
fn hide() {
    if get_object_variable("renewable-view","gust_visible") {
        for i in 0..8 {set_visible("wind-streak-"+i.to_string(),false);}
        set_object_variable("renewable-view","gust_visible",false);
    }
}
fn register_rotor(target) {
    for page in 0..3 {
        let name="rotors_"+page.to_string();let rotors=get_object_list("renewable-view",name);
        if rotors.len()<1024 {
            rotors.push(target);set_object_list("renewable-view",name,rotors);
            set_object_variable("renewable-view","rotor_count",get_object_variable("renewable-view","rotor_count")+1.0);
            set_rotation(target,[0.0,0.0,motion().angle]);return;
        }
    }
    throw "Resident wind rotor capacity exceeded";
}
fn unregister_rotor(target) {
    for page in 0..3 {
        let name="rotors_"+page.to_string();let rotors=get_object_list("renewable-view",name);
        let next=[];for rotor in rotors {if rotor!=target {next.push(rotor);}}
        if next.len()!=rotors.len() {
            set_object_list("renewable-view",name,next);
            set_object_variable("renewable-view","rotor_count",get_object_variable("renewable-view","rotor_count")-1.0);return;
        }
    }
}
fn update() {
    let seed=get_scene_variable("seed").to_int();let planet=get_object_list_item("controller","session",7).to_int();
    let time=get_object_list_item("controller","session",120);
    let outdoors=get_object_variable("architecture-view","inside")==0.0;
    let gust=outdoors && renewables::wind_active(seed,planet,time);
    if get_object_variable("renewable-view","gust_visible")!=gust {
        for i in 0..8 {set_visible("wind-streak-"+i.to_string(),gust);}
        set_object_variable("renewable-view","gust_visible",gust);
    }
    let count=get_object_variable("renewable-view","rotor_count");
    if count==0.0 && !gust {return;}
    let weather=motion();let angle=weather.angle;
    if count>0.0 && get_object_variable("renewable-view","angle")!=angle {
        for page in 0..3 {
            for rotor in get_object_list("renewable-view","rotors_"+page.to_string()) {set_rotation(rotor,[0.0,0.0,angle]);}
        }
        set_object_variable("renewable-view","angle",angle);
    }
    if !gust {return;}
    let x=grid::world_x(get_scene_variable("cursor_x").to_int());
    let z=grid::world_z(get_scene_variable("cursor_z").to_int());
    for i in 0..8 {
        let phase=(time*0.34+i.to_float()*0.173)%1.0;
        let spread=(i%4).to_float()*2.2-3.3;
        set_position("wind-streak-"+i.to_string(),[x+(phase-0.5)*10.0,0.13,z+spread+(phase-0.5)*3.5]);
        let size=max(0.001,sin(phase*3.14159265)*weather.strength);
        set_scale("wind-streak-"+i.to_string(),[size*(0.45+(i%3).to_float()*0.15),0.003,0.012]);
    }
}
