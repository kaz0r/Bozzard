// One deterministic snapshot of every built region on an explicit planet.
// Only the occupied region uses live UI arrays; every other region uses archives.
import "factory-data" as data;
import "factory-grid" as grid;
import "factory-power" as power;
import "factory-host_view" as host_view;

fn read(planet) {
    let active=planet==data::session_value(7).to_int();
    let current=if active { grid::current_chunk() } else { -1 };
    let names=["builds","facings","nodes","recipes","items","item_amounts","input_items","input_amounts","progress","assembler_iron","assembler_copper","split_state"];
    let built_pages=get_object_list("cache_builds").extract(planet*289,289); let columns=#{};
    for name in names { columns[name]=[]; }
    let state=#{planet:planet, active:active, current:current, radius:data::planet_radius(planet),
        ids:[], regions:[], storage:#{}, storage_changed:#{}, lookup:#{}, columns:columns,
        counts:if active { get_scene_list("counts") } else { data::other_counts() }, original:#{}};
    state.columns.rotating=[]; state.columns.powered=[];
    // An unbuilt destination adds no graph decoding or simulation work.
    if !active {
        let built=false; for page in built_pages { if page!="" { built=true; break; } }
        if !built { return state; }
    }
    let graph=power::power_graph(if active { "power_data" } else { "power_other" });
    let remote_rotations=host_view::rotation_mask(planet);
    for id in 0..289 {
        if id!=current && built_pages[id]=="" { continue; }
        let builds=if id==current { get_scene_list("builds") } else { grid::unpack_numbers(built_pages[id],225) };
        let cells=[];
        for cell in 0..225 { if builds[cell]>0.0 { cells.push(cell); } }
        if cells.len()==0 { continue; }
        let first=state.ids.len(); state.regions.push([id,first,cells]);
        for cell in cells {
            let global=id*225+cell;
            state.lookup[global.to_string()]=state.ids.len(); state.ids.push(global);
            state.columns.powered.push(if graph.contains(global.to_string()) { graph[global.to_string()][1].to_float() } else { 0.0 });
        }
        for name in names {
            let compact=[];
            if id==current {
                let values=if name=="recipes" { get_object_list(name) } else { get_scene_list(name) };
                for cell in cells { compact.push(values[cell]); }
            } else { compact=grid::unpack_cells(data::planet_region_entry(if name=="nodes" { "chunk_nodes" } else { "cache_"+name },planet,id),cells,1); }
            state.columns[name]+=compact;
        }
        let turns=if id==current { get_object_list("rotation_turns") } else { grid::empty_numbers(225) };
        for cell in cells { state.columns.rotating.push(if remote_rotations.contains((id*225+cell).to_string()) { 1.0 } else { turns[cell] }); }
        let storage_cells=[];
        for cell in cells { if data::has_slots(builds[cell]) { storage_cells.push(cell); } }
        if storage_cells.len()==0 {continue;}
        // Read each page once. Reading map-held pages inside the machine loop
        // copied all 900 live slots for every machine, even when none changed.
        let kind_pages=[]; let amount_pages=[];
        for page in 0..4 {
            let suffix=page.to_string();
            let kinds=if id==current {get_scene_list("storage_kinds_"+suffix)}
                else {grid::unpack_cells(data::planet_region_entry("cache_storage_kinds_"+suffix,planet,id),storage_cells,4)};
            let amounts=if id==current {get_scene_list("storage_amounts_"+suffix)}
                else {grid::unpack_cells(data::planet_region_entry("cache_storage_amounts_"+suffix,planet,id),storage_cells,4)};
            kind_pages.push(kinds); amount_pages.push(amounts);
        }
        let selected=if id==current {storage_cells} else {
            let rows=[]; for row in 0..storage_cells.len() {rows.push(row);} rows
        };
        let slots=gather_numeric_pairs(kind_pages,amount_pages,selected,4);
        let storage_index=0;
        for i in 0..cells.len() {
            let cell=cells[i]; if !data::has_slots(builds[cell]) { continue; }
            state.storage[(first+i).to_string()]=slots[storage_index];
            storage_index+=1;
        }
    }
    // A snapshot also records the writable columns. Backpressure, empty inputs
    // and overload often leave them unchanged; those columns need no host write
    // or archive reconstruction. Storage already has explicit dirty indices.
    for name in ["items","item_amounts","input_items","input_amounts","progress","assembler_iron","assembler_copper","split_state"] {
        state.original[name]=state.columns[name];
    }
    state.original.counts=state.counts;
    state
}

fn write(state) {
    let current=state.current;
    let names=["items","item_amounts","input_items","input_amounts","progress","assembler_iron","assembler_copper","split_state"];
    // Batch archive writes per column, not per machine or item transfer.
    for name in names {
        if state.original.contains(name) && numeric_arrays_equal(state.columns[name],state.original[name]) {continue;}
        let pages=get_object_list("cache_"+name); let changed=false;
        for region in state.regions {
            let id=region[0]; let first=region[1]; let cells=region[2];
            let values=grid::empty_numbers(225);
            for i in 0..cells.len() { values[cells[i]]=state.columns[name][first+i]; }
            if id==current { set_scene_list(name,values); }
            else {
                let at=id+state.planet*289;
                let packed=grid::pack_cells(values,cells,1);
                if pages[at]!=packed { pages[at]=packed; changed=true; }
            }
        }
        if changed { set_object_list("cache_"+name,pages); }
    }
    let storage_pages=#{}; let storage_dirty=#{};
    for region in state.regions {
        let id=region[0]; let first=region[1]; let cells=region[2];
        let storage_cells=[]; let storage_indices=[]; let changed_indices=[];
        for i in 0..cells.len() {
            let key=(first+i).to_string();
            if !state.storage.contains(key) { continue; }
            storage_cells.push(cells[i]);storage_indices.push(i);
            if state.storage_changed.contains(key) {
                changed_indices.push(i);
            }
        }
        if changed_indices.len()==0 { continue; }
        if id==current {
            set_scene_variable("storage_revision",get_scene_variable("storage_revision")+changed_indices.len().to_float());
        }
        // Read and write each live page once, even when every machine finishes
        // together. Remote pages still contain the complete occupied snapshot.
        for page in 0..4 {
            let suffix=page.to_string();
            let kinds=if id==current {get_scene_list("storage_kinds_"+suffix)}else{grid::empty_numbers(900)};
            let amounts=if id==current {get_scene_list("storage_amounts_"+suffix)}else{grid::empty_numbers(900)};
            for i in if id==current {changed_indices}else{storage_indices} {
                let slots=state.storage[(first+i).to_string()];
                for slot in 0..4 {
                    let at=cells[i]*4+slot; let source=(page*4+slot)*2;
                    kinds[at]=slots[source]; amounts[at]=slots[source+1];
                }
            }
            if id==current {
                set_scene_list("storage_kinds_"+suffix,kinds);set_scene_list("storage_amounts_"+suffix,amounts);
                continue;
            }
            for prefix in ["storage_kinds_","storage_amounts_"] {
                let name="cache_"+prefix+page.to_string();
                if !storage_pages.contains(name) { storage_pages[name]=get_object_list(name); }
                let at=id+state.planet*289;
                let packed=grid::pack_cells(if prefix=="storage_kinds_" { kinds } else { amounts },storage_cells,4);
                if storage_pages[name][at]!=packed { storage_pages[name][at]=packed; storage_dirty[name]=true; }
            }
        }
    }
    // One write per changed storage column, even when many remote containers fill.
    for name in storage_dirty.keys() { set_object_list(name,storage_pages[name]); }
    if !state.original.contains("counts") || !numeric_arrays_equal(state.counts,state.original.counts) {
        if state.active { set_scene_list("counts",state.counts); }
        else { data::set_other_counts(state.counts); }
    }
}
