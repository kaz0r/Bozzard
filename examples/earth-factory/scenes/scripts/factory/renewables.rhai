// Array occupancy is derived from its saved anchor and facing. There are no
// placeholder machines, extra circuit terminals or duplicated production cells.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-debris" as debris;
import "factory-clock" as clock;

fn renewable(kind) { kind>=40.0 && kind<=42.0 }
fn value(pages,planet,id) {
    if id<0 {return 0.0;}
    grid::packed_value(pages[planet*289+id/225],id%225)
}
fn owner(builds,facings,planet,id,radius) {
    if value(builds,planet,id)>0.0 {return id;}
    for dir in 0..4 {
        let anchor=grid::global_neighbor(id,dir,radius);
        if anchor>=0 && value(builds,planet,anchor)==41.0 &&
            grid::global_neighbor(anchor,value(facings,planet,anchor).to_int(),radius)==id {return anchor;}
    }
    -1
}
fn placement_error(builds,facings,nodes,planet,id,kind,direction,ignore,seed,free,dev,phase) {
    let radius=if planet==1 {6}else{8};let cells=[id];
    if kind==41.0 {
        let other=grid::global_neighbor(id,direction,radius);
        if other<0 {return "The solar array needs two spots inside the planet.";}
        cells.push(other);
    }
    for cell in cells {
        let cx=cell/225%17-8;let cz=cell/225/17-8;
        if dev && (cx<0 || cz<0 || cx>1 || cz>1) {return "Both array spots must be inside Dev World.";}
        let claimed=owner(builds,facings,planet,cell,radius);
        if claimed>=0 && claimed!=ignore {return "A machine or solar array already occupies this spot.";}
        let x=grid::global_x(cell).to_int();let z=grid::global_z(cell).to_int();
        if !free && !dev && ((x==0 && z==0) || ((kind==41.0 || phase>=5.0) && z==1 && (x==0 || x==2))) {return "Keep the landing pod, rocket and dock clear.";}
        if !free && !dev && debris::blocked(seed,planet,x,z) {return "Keep the spaceship wreckage clear.";}
        if kind==41.0 {
            if nodes[planet*289+cell/225]=="" {return "Explore both array spots before building.";}
            if value(nodes,planet,cell)!=0.0 {return "Both solar array spots need clear ground.";}
        }
    }
    ""
}
fn live_pages(name,column) {
    let pages=get_object_list(name);
    pages[(if data::on_moon(){289}else{0})+grid::current_chunk()]=grid::pack_numbers(get_scene_list(column));
    pages
}
fn local_owner(id) {
    owner(live_pages("cache_builds","builds"),live_pages("cache_facings","facings"),
        if data::on_moon(){1}else{0},id,data::planet_radius())
}
fn local_error(id,kind,direction,ignore) {
    placement_error(live_pages("cache_builds","builds"),live_pages("cache_facings","facings"),
        live_pages("chunk_nodes","nodes"),if data::on_moon(){1}else{0},id,kind,direction,ignore,
        get_scene_variable("seed").to_int(),get_scene_variable("demo_mode"),data::dev_world(),get_object_variable("phase"))
}
fn array_facing(id) {
    if id/225==grid::current_chunk() {get_scene_list_item("facings",id%225)}
    else {grid::packed_value(data::region_entry("cache_facings",id/225),id%225)}
}
fn array_kind(id) {
    if id<0 {return 0.0;}
    if id/225==grid::current_chunk() {get_scene_list_item("builds",id%225)}
    else {grid::packed_value(data::region_entry("cache_builds",id/225),id%225)}
}
fn array_handle(id) {
    if id/225==grid::current_chunk() {get_scene_list_item("build_visuals",id%225)}
    else {grid::unpack_text(get_object_list_item("cache_visuals_"+(id%225/75).to_string(),id/225),75)[id%75]}
}
fn edit_array(id,remove) {
    let region=id/225;let cell=id%225;let current=region==grid::current_chunk();
    let builds=if current {get_scene_list("builds")}else{grid::unpack_numbers(data::region_entry("cache_builds",region),225)};
    let facings=if current {get_scene_list("facings")}else{grid::unpack_numbers(data::region_entry("cache_facings",region),225)};
    let handle=array_handle(id);
    if remove {
        builds[cell]=0.0;facings[cell]=0.0;
        if handle!="" {destroy_prefab(handle);}
        let page=cell/75;let handles=grid::unpack_text(get_object_list_item("cache_visuals_"+page.to_string(),region),75);
        handles[cell%75]="";grid::cache_put("cache_visuals_"+page.to_string(),region,grid::pack_text(handles));
        if current {
            let visuals=get_scene_list("build_visuals");visuals[cell]="";set_scene_list("build_visuals",visuals);
            let machines=[];for entry in get_scene_list("machine_cells") {if entry.to_int()!=cell {machines.push(entry);}}
            set_scene_list("machine_cells",machines);
        }
    } else {
        facings[cell]=((facings[cell].to_int()+1)%4).to_float();
        if handle!="" {set_rotation(handle,[0.0,-facings[cell]*90.0,0.0]);}
    }
    grid::cache_put("cache_builds",region,grid::pack_numbers(builds));
    grid::cache_put("cache_facings",region,grid::pack_numbers(facings));
    if current {set_scene_list("builds",builds);set_scene_list("facings",facings);}
}

// Weather uses the saved world clock and seed, so unloaded regions, saves and
// host-authoritative actions all see the same gust. Seven varied 15-minute
// real-time windows repeat, each with a 20–600 second gust and calm on both sides.
fn wind_cycle_seconds() {900.0}
fn wind_cycle(time) {floor(max(time,0.0)/wind_cycle_seconds()).to_int()}
fn wind_window(seed,planet,cycle) {
    let salt=((seed%104729)*48271+planet*15427+(cycle%7)*7919)%104729;
    #{start:(30+salt%181).to_float(),duration:(20+salt/13%581).to_float(),speed:(300+salt/91%121).to_float()}
}
fn wind_active(seed,planet,time) {
    let cycle=wind_cycle(time);let window=wind_window(seed,planet,cycle);
    let elapsed=max(time,0.0)-cycle.to_float()*wind_cycle_seconds()-window.start;
    elapsed>0.0 && elapsed<window.duration
}
fn wind_motion(seed,planet,time) {
    let cycle=wind_cycle(time);let window=wind_window(seed,planet,cycle);
    let elapsed=max(time,0.0)-cycle.to_float()*wind_cycle_seconds()-window.start;
    let area=0.0;let remainder=0.0;
    for i in 0..7 {
        let previous=wind_window(seed,planet,i);let turn=previous.speed*(previous.duration-2.0);
        area+=turn;if i<cycle%7 {remainder+=turn;}
    }
    let t=clamp(elapsed,0.0,window.duration);let integrated=0.0;
    if t<2.0 {integrated=t*t/4.0;}
    else if t<=window.duration-2.0 {integrated=t-1.0;}
    else {let tail=window.duration-t;integrated=window.duration-2.0-tail*tail/4.0;}
    // Integrating the two-second acceleration/deceleration ramps gives a stable
    // angle even after skipped frames or loading a save in the middle of a gust.
    #{angle:((cycle/7).to_float()*area+remainder+integrated*window.speed)%360.0,
      strength:if elapsed>0.0 && elapsed<window.duration {min(1.0,min(elapsed/2.0,(window.duration-elapsed)/2.0))}else{0.0}}
}

fn solar_active(time,planet) {clock::daytime(time,planet)}
fn supply(kind,sunny,windy) {
    if kind==10 {8}else if kind==6 {10}else if kind==40 {if sunny {4}else{0}}
    else if kind==41 {if sunny {8}else{0}}else if kind==42 {if windy {6}else{0}}else{0}
}
