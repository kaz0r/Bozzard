// Story wreckage: one deterministic, sparse layout for the entire planet.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-deposits" as deposits;

fn asset(variant) {
    ["debris-cockpit", "debris-hull", "debris-wing", "debris-engine"][variant]
}

fn layout(seed, planet) {
    let state = (abs(seed) % 2147483646 + 1 + (planet+1)*32452843) % 2147483647;
    if state==0 {state=1;}
    let count=2;
    // Four rare extra slots. The two guaranteed slots never depend on a roll.
    for slot in 0..4 {
        state=(state*48271)%2147483647;
        if state%100<12 {count+=1;}
    }
    let radius=data::planet_radius(planet);
    let regions=[];
    for cz in -radius..radius+1 {
        for cx in -radius..radius+1 {
            if max(abs(cx),abs(cz))>=2 {regions.push(grid::chunk_id(cx,cz));}
        }
    }
    // Partial shuffle samples unique regions, independent of discovery order.
    let result=[count.to_float()];
    state=(state*48271)%2147483647;
    let first_variant=state%4;
    for slot in 0..count {
        state=(state*48271)%2147483647;
        let chosen=slot+state%(regions.len()-slot);
        let region=regions[chosen];regions[chosen]=regions[slot];regions[slot]=region;
        let nodes=deposits::generate_region_nodes(seed,region%17-8,region/17-8,planet);
        state=(state*48271)%2147483647;
        let start=state%81;
        let x=0;let z=0;
        // At most four deposits exclude at most 36 of the 81 candidates, so a
        // clear 3x3 footprint always exists. Keep it inside its own region.
        for attempt in 0..81 {
            let candidate=(start+attempt)%81;
            x=candidate%9-4;z=candidate/9-4;
            let clear=true;
            for dz in -1..2 {
                for dx in -1..2 {
                    if nodes[grid::index(x+dx,z+dz)]!=0.0 {clear=false;}
                }
            }
            if clear {break;}
        }
        state=(state*48271)%2147483647;
        result += [region.to_float(),x.to_float(),z.to_float(),
                   ((first_variant+slot)%4).to_float(),(state%4*90).to_float()];
    }
    result
}

fn current_layout() {
    let planet=data::session_value(7).to_int();
    let pages=get_object_list("factory-transports","debris_layouts");
    if pages[planet]=="" {
        pages[planet]=grid::pack_numbers(layout(get_scene_variable("seed").to_int(),planet));
        set_object_list("factory-transports","debris_layouts",pages);
    }
    grid::unpack_numbers(pages[planet],31)
}

fn occupies(plan, region, x, z) {
    for slot in 0..plan[0].to_int() {
        let at=1+slot*5;
        if plan[at].to_int()==region && abs(x.to_float()-plan[at+1])<=1.0 && abs(z.to_float()-plan[at+2])<=1.0 {return true;}
    }
    false
}

fn blocked(x,z) {
    if data::dev_world() || get_scene_variable("demo_mode") {return false;}
    occupies(current_layout(),grid::current_chunk(),x,z)
}

// The native co-op authority calls the same pure generator for remote building.
fn blocked(seed,planet,x,z) {
    let cx=floor((x.to_float()+7.0)/15.0).to_int();
    let cz=floor((z.to_float()+7.0)/15.0).to_int();
    occupies(layout(seed,planet),grid::chunk_id(cx,cz),x-cx*15,z-cz*15)
}

fn load_region(region) {
    if data::dev_world() || get_scene_variable("demo_mode") {return;}
    let plan=current_layout();
    let handles=get_object_list("factory-transports","debris_handles");
    for slot in 0..plan[0].to_int() {
        let at=1+slot*5;
        if plan[at].to_int()!=region || handles[slot]!="" {continue;}
        handles[slot]=spawn_prefab(asset(plan[at+3].to_int()),
            [(region%17-8).to_float()*15.0+plan[at+1],0.0,(region/17-8).to_float()*15.0+plan[at+2]]);
        set_rotation(handles[slot],[0.0,plan[at+4],0.0]);
    }
    set_object_list("factory-transports","debris_handles",handles);
}

fn unload_region(region) {
    if data::dev_world() || get_scene_variable("demo_mode") {return;}
    let plan=current_layout();
    let handles=get_object_list("factory-transports","debris_handles");
    for slot in 0..plan[0].to_int() {
        if plan[1+slot*5].to_int()!=region || handles[slot]=="" {continue;}
        destroy_prefab(handles[slot]);handles[slot]="";
    }
    set_object_list("factory-transports","debris_handles",handles);
}

fn clear() {
    for target in get_object_list("factory-transports","debris_handles") {
        if target!="" {destroy_prefab(target);}
    }
    set_object_list("factory-transports","debris_handles",grid::empty_text(6));
    set_object_list("factory-transports","debris_layouts",grid::empty_text(2));
}
