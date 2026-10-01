// Fixed asset showroom. These authored rows never call the resource generator.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-chunks" as chunks;
import "factory-power" as power;
import "factory-backpack" as backpack;

fn deposits() { [1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0,10.0,24.0,25.0,26.0,40.0,45.0] }
fn machines() { [1.0,3.0,11.0,5.0,4.0,2.0,7.0,8.0,6.0,9.0]+[12.0,13.0,14.0,15.0,16.0,17.0,18.0,19.0,20.0,21.0,22.0,23.0,24.0,25.0,26.0,27.0,28.0,29.0] }
fn items() { let types=[1.0,2.0,3.0,4.0,5.0,6.0,7.0,8.0,9.0,10.0,11.0,12.0,13.0,14.0,15.0,16.0,17.0,18.0,20.0,21.0,22.0,23.0,24.0,25.0,26.0];for kind in 27..51 {types.push(kind.to_float());} types }

fn local_cell(x,z,cx,cz) {
    let lx=x-cx*15;let lz=z-cz*15;
    if lx< -7 || lx>7 || lz< -7 || lz>7 { -1 } else { grid::index(lx,lz) }
}

fn populate() {
    set_visible("ground",false);
    let visited=grid::empty_numbers(289);
    set_object_list("resident",grid::empty_numbers(289));
    for cz in 0..2 {
        for cx in 0..2 {
            let id=grid::chunk_id(cx,cz);
            let nodes=grid::empty_numbers(225);let builds=grid::empty_numbers(225);
            let held=grid::empty_numbers(225);let amounts=grid::empty_numbers(225);
            let recipes=grid::empty_numbers(225);
            let types=deposits();
            for i in 0..types.len() {
                let cell=local_cell(-6+i*2,-5,cx,cz);
                if cell>=0 { nodes[cell]=types[i]; }
            }
            let types=machines();
            for i in 0..types.len() {
                let cell=local_cell(if i<10 {-6+i*2}else{-6+(i-10)%13*2},if i<10 {5}else{-1+(i-10)/13*4},cx,cz);
                if cell>=0 {
                    builds[cell]=types[i];
                    if data::production(types[i]) { recipes[cell]=data::recipe_choices(types[i])[0]; }
                }
            }
            // A second pole displays an actual, editable cable connection.
            let pole=local_cell(16,5,cx,cz);
            if pole>=0 { builds[pole]=9.0; }
            // Isolated belts display real items; the empty tile between them
            // keeps the samples in place while ordinary factory simulation runs.
            let types=items();
            for i in 0..types.len() {
                let cell=local_cell(if i<25 {-6+(i%13)*2}else{-6+(i-25)%12*2},if i<25 {12+(i/13)*4}else{8+(i-25)/12*12},cx,cz);
                if cell>=0 { builds[cell]=if data::fluid(types[i]) {26.0}else{2.0};held[cell]=types[i];amounts[cell]=1.0; }
            }
            grid::cache_put("chunk_nodes",id,grid::pack_numbers(nodes));
            grid::cache_put("cache_builds",id,grid::pack_numbers(builds));
            grid::cache_put("cache_facings",id,grid::pack_numbers(grid::empty_numbers(225)));
            grid::cache_put("cache_items",id,grid::pack_numbers(held));
            grid::cache_put("cache_recipes",id,grid::pack_numbers(recipes));
            grid::cache_put("cache_item_amounts",id,grid::pack_numbers(amounts));
            visited[id]=1.0;
            for cell in 0..225 {
                if power::electrical(builds[cell].to_int()) { power::power_register(id*225+cell,builds[cell].to_int()); }
            }
        }
    }
    set_object_list("visited",visited);
    for cz in 0..2 { for cx in 0..2 { chunks::load_chunk_visuals(cx,cz); } }
    power::connect_power(grid::chunk_id(1,0)*225+grid::index(-3,5),grid::chunk_id(1,0)*225+grid::index(1,5));
    let stock=grid::empty_numbers(64);
    for item in items().extract(0,25) { stock[item.to_int()]=10.0; }
    backpack::set_stock(stock);
}
