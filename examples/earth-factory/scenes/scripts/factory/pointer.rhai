import "factory-interiors" as interiors;
import "factory-data" as data;
import "factory-net_input" as net;
import "factory-chunks" as chunks;
import "factory-item_fx" as item_fx;
import "factory-grid" as grid;
import "factory-building" as building;
import "factory-world" as world;
import "factory-power" as power;
import "factory-inspection" as inspection;

fn tile(x,z) {
    let wx=floor(x+0.5).to_int(); let wz=floor(z+0.5).to_int();
    let cx=floor((wx+7).to_float()/15.0).to_int(); let cz=floor((wz+7).to_float()/15.0).to_int();
    if !data::chunk_allowed(cx,cz) { return []; }
    if get_object_list("resident")[grid::chunk_id(cx,cz)]==0.0 { return []; }
    [cx,cz,wx-cx*15,wz-cz*15]
}
fn ground(ray) {
    if !ray.valid || abs(ray.direction[1])<0.0001 { return []; }
    let t=(0.11-ray.origin[1])/ray.direction[1]; if t<0.0 { return []; }
    tile(ray.origin[0]+t*ray.direction[0],ray.origin[2]+t*ray.direction[2])
}
fn box_distance(ray, center, half) {
    let near=0.0; let far=10000.0;
    for axis in 0..3 {
        let distance=ray.origin[axis]-center[axis]; let direction=ray.direction[axis];
        if abs(direction)<0.0001 { if abs(distance)>half[axis] { return -1.0; } }
        else {
            let a=(-half[axis]-distance)/direction; let b=(half[axis]-distance)/direction;
            near=max(near,min(a,b)); far=min(far,max(a,b)); if near>far { return -1.0; }
        }
    }
    near
}
// Pick a machine's raised body, not the ground behind its silhouette.
fn pick(ray) {
    let result=ground(ray); if !ray.valid { return result; }
    let nearest=10000.0; let resident=get_object_list("resident");
    // Grounded model heights, indexed by build kind. Include the full catalog
    // so clicking anywhere in a region containing expansion machines is safe.
    let heights=[0.0,1.37,0.45,1.5,1.10,1.5,1.52,0.55,0.55,2.02,0.0,1.3,
        0.825,1.045,1.085,1.008,1.359,1.467,1.235,1.152,1.2675,1.3525,
        1.13,1.125,1.25,0.8125,0.422,0.422,0.363,0.363,
        0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.56,0.56,2.23];
    let current=grid::chunk_id(get_scene_variable("chunk_x").to_int(),get_scene_variable("chunk_z").to_int());
    for id in 0..289 {
        if resident[id]==0.0 { continue; }
        let cx=id%17-8; let cz=id/17-8;
        if box_distance(ray,[cx.to_float()*15.0,1.5,cz.to_float()*15.0],[8.5,1.5,8.5])<0.0 { continue; }
        let builds=if id==current { get_scene_list("builds") } else { grid::unpack_numbers(data::region_data("cache_builds")[id],225) };
        for cell in 0..225 {
            let kind=builds[cell].to_int(); if kind<=0 || kind>=heights.len() { continue; }
            let height=heights[kind]; if height<=0.0 {continue;}
            let wx=cx*15+grid::cell_x(cell); let wz=cz*15+grid::cell_z(cell);
            let width=if kind>=12 {0.5}else{0.48};
            let center=[wx.to_float(),height/2.0,wz.to_float()];let half=[width,height/2.0,width];
            if kind==41 {
                let facing=if id==current {get_scene_list_item("facings",cell)}else{grid::packed_value(data::region_entry("cache_facings",id),cell)};
                let dx=grid::step_x(facing.to_int());let dz=grid::step_z(facing.to_int());
                center[0]+=dx.to_float()*0.5;center[2]+=dz.to_float()*0.5;
                half[0]=if dx!=0 {1.0}else{0.5};half[2]=if dz!=0 {1.0}else{0.5};
            }
            let t=box_distance(ray,center,half);
            if t>=0.0 && t<nearest { nearest=t; result=[cx,cz,grid::cell_x(cell),grid::cell_z(cell)]; }
        }
    }
    if resident[144]>0.0 && !get_scene_variable("demo_mode") {
        let bodies=[[0.0,0.63,0.0,0.48,0.63,0.48,0,0]];
        if get_object_variable("phase")>=5.0 { bodies.push([2.0,0.35,1.0,0.45,0.35,0.45,2,1]); }
        if get_object_variable("phase")>=6.0 {
            let height=if get_object_variable("phase")>=7.0 { 3.1 } else { 1.8 };
            bodies.push([0.0,height/2.0,1.0,0.65,height/2.0,0.65,0,1]);
        }
        for body in bodies {
            let t=box_distance(ray,body.extract(0,3),body.extract(3,3));
            if t>=0.0 && t<nearest { nearest=t; result=[0,0,body[6],body[7]]; }
        }
    }
    result
}
fn select_tile(at) {
    if at.len()!=4 { return false; }
    if !interiors::path_clear(grid::world_x(get_scene_variable("cursor_x")).to_int(),grid::world_z(get_scene_variable("cursor_z")).to_int(),at[0]*15+at[2],at[1]*15+at[3]) {return false;}
    if net::guest() && !net::send("point",#{at:#{planet:data::session_value(7).to_int(),x:at[0]*15+at[2],z:at[1]*15+at[3]}}) {return false;}
    if get_scene_variable("chunk_x").to_int()!=at[0] || get_scene_variable("chunk_z").to_int()!=at[1] {
        if net::guest() {item_fx::archive_current();chunks::load_chunk_visuals(at[0],at[1]);world::restore_chunk(at[0],at[1]);}
        else {world::enter_chunk(at[0],at[1]);}
    }
    set_scene_variable("cursor_x",at[2].to_float()); set_scene_variable("cursor_z",at[3].to_float());
    set_position("cursor",[(at[0]*15+at[2]).to_float(),0.115,(at[1]*15+at[3]).to_float()]);
    true
}
fn close_menu() { data::session_set(41,0.0); set_ui_visible("world-context",false); }
fn show_menu(mode, id, other, x, y) {
    data::session_set(41,mode); data::session_set(42,id.to_float()); data::session_set(43,other.to_float());
    let entry=power::power_entry(get_object_list("power_data"),id);
    let connected=mode==2.0 || (mode==1.0 && entry.len()>2);
    set_ui_visible("world-context",true); set_ui_screen_position("world-context",x,y);
    set_ui_visible("world-link",mode!=3.0); set_ui_visible("world-unlink",connected);
    set_ui_visible("world-inspect",mode==3.0);
    set_ui_offset("world-cancel",8.0,if connected { 88.0 } else { 48.0 });
    set_ui_size("world-context",170.0,if connected { 128.0 } else { 88.0 });
}
fn cable_at(x,y) {
    let graph=power::power_graph(); let effects=power::read_power_fx(); let closest=0.00010; let hit=[];
    // Distance in normalized screen-height units, independent of camera zoom.
    let aspect=render_stats().viewport_aspect; if aspect<=0.0 { aspect=16.0/9.0; }
    for key in graph.keys() {
        let a=parse_int(key);
        for b in graph[key].extract(2) {
            if a>=b || !effects.contains(key) || !effects[key].contains(b.to_string()) { continue; }
            let left=world_to_screen(power::power_anchor(a,graph[key][0])); let right=world_to_screen(power::power_anchor(b,graph[b.to_string()][0]));
            if left[2]<0.0 || right[2]<0.0 { continue; }
            let dx=(right[0]-left[0])*aspect; let dy=right[1]-left[1]; let length=dx*dx+dy*dy;
            if length<0.000001 { continue; }
            let t=clamp(((x-left[0])*aspect*dx+(y-left[1])*dy)/length,0.0,1.0);
            let ex=(x-left[0])*aspect-t*dx; let ey=y-left[1]-t*dy;
            if ex*ex+ey*ey<closest { closest=ex*ex+ey*ey; hit=if t<0.5 { [a,b] } else { [b,a] }; }
        }
    }
    hit
}
fn right_click(x,y) {
    data::session_set(40,0.0);
    let at=pick(screen_ray(x,y));
    // A cable endpoint on a production machine must not swallow its Inspect action.
    if at.len()==4 {
        let region=grid::chunk_id(at[0],at[1]); let cell=grid::index(at[2],at[3]);
        let current=grid::chunk_id(get_scene_variable("chunk_x").to_int(),get_scene_variable("chunk_z").to_int());
        let builds=if region==current { get_scene_list("builds") } else { grid::unpack_numbers(data::region_data("cache_builds")[region],225) };
        let kind=builds[cell];
        if kind==1.0 || data::production(kind) || kind==4.0 || kind==7.0 || kind==8.0 || data::pipe(kind) || data::belt(kind) {
            if !select_tile(at) {return;} show_menu(3.0,power::power_id(cell),-1,x,y); return;
        }
    }
    let cable=cable_at(x,y);
    if cable.len()==2 { show_menu(2.0,cable[0],cable[1],x,y); return; }
    if at.len()!=4 { close_menu(); return; }
    if !select_tile(at) {return;}
    let id=power::power_id(grid::index(at[2],at[3]));
    if power::power_entry(get_object_list("power_data"),id).len()>0 { show_menu(1.0,id,-1,x,y); }
    else { close_menu(); }
}
fn update(blocked) {
    set_visible("hover-tile",false);
    if blocked { close_menu(); return false; }
    let consumed=false;
    for event in ui_events() {
        let mode=data::session_value(41);
        if event.kind=="activate" && mode>0.0 {
            let id=data::session_value(42).to_int(); let other=data::session_value(43).to_int();
            if event.target=="world-cancel" { close_menu(); consumed=true; }
            else if event.target=="world-link" {
                set_scene_variable("selected",10.0); set_object_variable("wire_start",id.to_float());
                data::session_set(40,1.0); close_menu(); consumed=true;
                set_scene_variable("message","Click another power terminal to link. Right-click cancels.");
            } else if event.target=="world-unlink" {
                power::disconnect_power(id,if mode==2.0 { other } else { -1 }); close_menu(); consumed=true;
            } else if event.target=="world-inspect" {
                let region=id/225; let cell=id%225;
                if !select_tile([region%17-8,region/17-8,grid::cell_x(cell),grid::cell_z(cell)]) {close_menu();return true;}
                inspection::open(cell); close_menu(); return true;
            }
        }
        if event.kind=="cancel" { close_menu(); data::session_set(40,0.0); consumed=true; }
        if event.kind=="secondary" && !event.blocked {
            set_object_variable("wire_start",-1.0); right_click(event.x,event.y); consumed=true;
        }
        if event.kind=="down" && !event.blocked {
            if mode>0.0 { close_menu(); consumed=true; continue; }
            let at=pick(screen_ray(event.x,event.y));
            if at.len()==4 {
                let moved=select_tile(at);
                if moved && data::session_value(40)>0.0 { building::place_selected(); }
            }
        }
    }
    if data::session_value(41)>0.0 { return true; }
    let pointer=world_pointer(); let at=ground(screen_ray(pointer[0],pointer[1]));
    if at.len()==4 && data::session_value(40)>0.0 {
        set_position("hover-tile",[(at[0]*15+at[2]).to_float(),0.14,(at[1]*15+at[3]).to_float()]);
        set_visible("hover-tile",true);
    }
    consumed
}
