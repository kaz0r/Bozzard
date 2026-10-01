"""A playable building example using the production game and its enclosure rules."""
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
SCENES=ROOT/'scenes'
SETUP='''// Playable indoor factory example. The normal game owns all movement and cutaways.
import "factory-architecture" as architecture;
import "factory-interiors" as interiors;
import "factory-grid" as grid;
import "factory-data" as data;
import "factory-building" as building;
import "factory-power" as power;
import "factory-backpack" as backpack;
fn on_start(me) {
    let pieces=grid::empty_numbers(900);let nodes=get_scene_list("nodes");let node_visuals=get_scene_list("node_visuals");
    for z in -5..-1 {for x in -6..-2 {
        let cell=grid::index(x,z);nodes[cell]=0.0;if node_visuals[cell]!="" {destroy_prefab(node_visuals[cell]);node_visuals[cell]="";}pieces[cell*4]=if (x+z)%2==0 {30.0}else{31.0};pieces[cell*4+1]=if x==-6 {37.0}else{36.0};
        for dir in 0..4 {
            let nx=x+grid::step_x(dir);let nz=z+grid::step_z(dir);
            if nx>=-6 && nx<=-3 && nz>=-5 && nz<=-2 {continue;}
            let a=architecture::address(x,z,32.0,dir);pieces[a.slot]=32.0+dir.to_float();
        }
    }}
    pieces[architecture::address(-5,-2,38.0,1).slot]=38.0;
    pieces[architecture::address(-3,-3,39.0,0).slot]=39.0;
    set_scene_list("nodes",nodes);set_scene_list("node_visuals",node_visuals);grid::cache_put("chunk_node_visuals",144,grid::pack_text(node_visuals));
    grid::cache_put("cache_structures",144,grid::pack_numbers(pieces));
    for row in [[-5,-3,5],[-4,-3,11],[-4,-2,3],[-3,-3,4],[-4,-4,9],[-5,0,9]] {
        set_scene_variable("cursor_x",row[0].to_float());set_scene_variable("cursor_z",row[1].to_float());
        set_scene_variable("selected",row[2].to_float());building::place_selected();
    }
    let pole=power::power_id(grid::index(-4,-4));let outside=power::power_id(grid::index(-5,0));
    power::connect_power(power::power_id(grid::index(0,0)),outside);power::connect_power(outside,pole);
    for at in [[-5,-3],[-4,-3],[-4,-2]] {power::connect_power(pole,power::power_id(grid::index(at[0],at[1])));}
    power::update_power();
    backpack::give(11.0,60.0);backpack::give(12.0,30.0);backpack::give(18.0,40.0);
    set_scene_variable("cursor_x",-5.0);set_scene_variable("cursor_z",-3.0);
    set_scene_variable("selected",30.0);set_object_variable("bar",6.0);
    set_position("camera-rig",[-4.5,0.0,-3.5]);set_object_variable("camera_pan_progress",1.0);
    set_camera_size("camera",12.0);set_object_variable("camera_zoom",12.0);set_object_variable("camera_zoom_target",12.0);
    data::session_set(120,15.7);interiors::load(144);interiors::update(0.0);
    set_scene_variable("message","Indoor factory: WASD to the south door, wait, then step out. Ctrl+6 foundations / Ctrl+7 roofs.");
}
'''
def main():
    (SCENES/'scripts/foundation_showroom.rhai').write_text(SETUP)
    scene=json.loads((SCENES/'earth.json').read_text())
    scene['name']='Indoor factory foundations showroom'
    scene['blackboard']['seed']['scalar']['number']=4
    controller=next(o for o in scene['objects'] if o['id']=='controller')
    controller['blackboard']['title_open']['scalar']['bool']=False
    controller['blackboard']['creative']['scalar']['bool']=True
    controller['blackboard']['phase']['scalar']['number']=7
    controller['script_manager']['scripts'].append({'enabled':True,'script':'foundation-showroom'})
    scene['assets']['foundation-showroom']={'kind':'script','path':'scripts/foundation_showroom.rhai'}
    (SCENES/'foundations-showroom.json').write_text(json.dumps(scene,indent=2)+'\n')
if __name__=='__main__':main()
