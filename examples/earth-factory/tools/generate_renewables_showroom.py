"""Playable, wired solar/wind example using the regular game and power graph."""
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
SCENES=ROOT/'scenes'
SETUP='''import "factory-building" as building;
import "factory-grid" as grid;
import "factory-data" as data;
import "factory-power" as power;
import "factory-debris" as debris;
fn on_start(me) {
    set_scene_variable("demo_mode",true);debris::unload_region(144);
    let nodes=get_scene_list("nodes");let visuals=get_scene_list("node_visuals");
    for at in [[-3,-4],[0,-4],[1,-4],[3,-4],[0,-2],[2,-2]] {
        let cell=grid::index(at[0],at[1]);nodes[cell]=0.0;
        if visuals[cell]!="" {destroy_prefab(visuals[cell]);visuals[cell]="";}
    }
    set_scene_list("nodes",nodes);set_scene_list("node_visuals",visuals);
    grid::cache_put("chunk_nodes",144,grid::pack_numbers(nodes));
    grid::cache_put("chunk_node_visuals",144,grid::pack_text(visuals));
    for row in [[-3,-4,40],[0,-4,41],[3,-4,42],[0,-2,9],[2,-2,3]] {
        set_scene_variable("cursor_x",row[0].to_float());set_scene_variable("cursor_z",row[1].to_float());
        set_scene_variable("selected",row[2].to_float());set_scene_variable("direction",0.0);building::place_selected();
    }
    let pole=power::power_id(grid::index(0,-2));
    for at in [[-3,-4],[0,-4],[3,-4],[2,-2]] {power::connect_power(pole,power::power_id(grid::index(at[0],at[1])));}
    data::session_set(120,15.7);power::update_power();
    set_scene_variable("cursor_x",-3.0);set_scene_variable("cursor_z",-2.0);
    set_scene_variable("selected",40.0);set_object_variable("bar",3.0);
    set_position("camera-rig",[0.0,0.0,-3.5]);set_object_variable("camera_pan_progress",1.0);
    set_camera_size("camera",10.0);set_object_variable("camera_zoom",10.0);set_object_variable("camera_zoom_target",10.0);
    set_scene_variable("message","Solar + wind: Ctrl+3, slots 4 / 5 / 6. Arrays use 2 spots. Solar runs by day; wind runs in gusts.");
}
'''
def main():
    (SCENES/'scripts/renewables_showroom.rhai').write_text(SETUP)
    scene=json.loads((SCENES/'earth.json').read_text());scene['name']='Renewable power showroom'
    scene['blackboard']['seed']['scalar']['number']=4
    controller=next(o for o in scene['objects'] if o['id']=='controller')
    controller['blackboard']['title_open']['scalar']['bool']=False
    controller['blackboard']['creative']['scalar']['bool']=True
    controller['blackboard']['phase']['scalar']['number']=7
    controller['script_manager']['scripts'].append({'enabled':True,'script':'renewables-showroom'})
    scene['assets']['renewables-showroom']={'kind':'script','path':'scripts/renewables_showroom.rhai'}
    (SCENES/'renewables-showroom.json').write_text(json.dumps(scene,indent=2)+'\n')
if __name__=='__main__':main()
