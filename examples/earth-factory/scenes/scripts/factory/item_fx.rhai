// Item rendering follows resident models, independently of factory simulation.
// Seventy-five cell handles per page cover a whole planet without large lists.
// Ownership pages follow destination cells; motion pages follow source cells.
// The extra motion_visuals entry lists active pages so animation skips empty ones.
import "factory-data" as data;
import "factory-grid" as grid;

fn archive_current() {
    let handles=get_scene_list("item_visuals"); let pages=get_scene_list("factory_visuals");
    if handles.len()!=225 || pages.len()!=867 { return; }
    let base=grid::current_chunk()*3;
    for page in 0..3 { pages[base+page]=grid::pack_text(handles.extract(page*75,75)); }
    set_scene_list("factory_visuals",pages);
}
fn restore_current() {
    let pages=get_scene_list("factory_visuals"); let handles=[]; let base=grid::current_chunk()*3;
    for page in 0..3 { handles+=grid::unpack_text(pages[base+page],75); }
    set_scene_list("item_visuals",handles);
}
fn read_handles(ids) {
    let local=get_scene_list("item_visuals");
    let handles=[]; let page=-1; let decoded=[]; let current=grid::current_chunk();
    for id in ids {
        if id/225==current { handles.push(local[id%225]); continue; }
        if id/75!=page { page=id/75; decoded=grid::unpack_text(get_scene_list_item("factory_visuals",page),75); }
        handles.push(decoded[id%75]);
    }
    handles
}
fn recycle(targets) {
    let pool=get_scene_list("item_pool");
    for target in targets {
        if target=="" { continue; }
        if pool.len()<1024 { set_visible(target,false); pool.push(target); }
        else { destroy_prefab(target); }
    }
    set_scene_list("item_pool",pool);
}
fn retire_previous() {
    let active=get_scene_list_item("motion_visuals",867);
    if active=="" { return; }
    let targets=[];
    for entry in grid::unpack_numbers(active,0) {
        let page=get_scene_list_item("retired_visuals",entry.to_int());
        if page!="" { for target in grid::unpack_text(page,75) { if target!="" { targets.push(target); } } }
    }
    recycle(targets);
    set_scene_list("retired_visuals",grid::empty_text(867));
}
fn cancel_motion(targets) {
    if targets.len()==0 { return; }
    let wanted=#{}; for target in targets { if target!="" { wanted[target]=true; } }
    for name in ["motion_visuals","retired_visuals"] {
        let pages=get_scene_list(name); let changed=false;
        for page in 0..pages.len() {
            if pages[page]=="" { continue; }
            let handles=grid::unpack_text(pages[page],75); let dirty=false;
            for i in 0..handles.len() {
                if wanted.contains(handles[i]) { handles[i]=""; dirty=true; }
            }
            if dirty { pages[page]=grid::pack_text(handles); changed=true; }
        }
        if changed { set_scene_list(name,pages); }
    }
}
fn unload_region(id) {
    let pages=get_scene_list("factory_visuals"); let targets=[];
    for page in id*3..id*3+3 {
        for target in grid::unpack_text(pages[page],75) { if target!="" { targets.push(target); } }
        pages[page]="";
    }
    set_scene_list("factory_visuals",pages);
    // Consumed items in transit have no destination owner; release them too.
    let retired=get_scene_list("retired_visuals");
    for page in id*3..id*3+3 {
        for target in grid::unpack_text(retired[page],75) { if target!="" { targets.push(target); } }
    }
    cancel_motion(targets); recycle(targets);
    if id==grid::current_chunk() { set_scene_list("item_visuals",grid::empty_text(225)); }
}
fn clear() {
    archive_current();
    let targets=#{};
    for name in ["factory_visuals","retired_visuals"] {
        for page in get_scene_list(name) {
            if page=="" { continue; }
            for target in grid::unpack_text(page,75) { if target!="" { targets[target]=true; } }
        }
    }
    for target in get_scene_list("item_pool") { if target!="" { targets[target]=true; } }
    for target in targets.keys() { destroy_prefab(target); }
    for name in ["factory_visuals","retired_visuals","motion_visuals","motion_from","motion_to","motion_from_y","motion_to_y"] {
        set_scene_list(name,grid::empty_text(if name=="motion_visuals" { 868 } else { 867 }));
    }
    set_scene_list("item_pool",[]); set_scene_list("item_visuals",grid::empty_text(225));
}
fn animate(fraction) {
    let active=get_scene_list_item("motion_visuals",867);
    if active=="" { return; }
    for entry in grid::unpack_numbers(active,0) {
        let page=entry.to_int();
        let handles=get_scene_list_item("motion_visuals",page);
        if handles=="" { continue; }
        let visual=grid::unpack_text(handles,75);
        let from=grid::unpack_numbers(get_scene_list_item("motion_from",page),75); let to=grid::unpack_numbers(get_scene_list_item("motion_to",page),75);
        // Heights are encoded in centimetres to retain the integer archive format.
        let from_y=grid::unpack_numbers(get_scene_list_item("motion_from_y",page),75); let to_y=grid::unpack_numbers(get_scene_list_item("motion_to_y",page),75);
        for cell in 0..75 {
            if visual[cell]=="" { continue; }
            let a=from[cell].to_int(); let b=to[cell].to_int();
            set_position(visual[cell],[lerp(grid::global_x(a),grid::global_x(b),fraction),
                lerp(from_y[cell],to_y[cell],fraction)/100.0,lerp(grid::global_z(a),grid::global_z(b),fraction)]);
        }
    }
}
// motions: [source address, destination address, handle, start height, end height, retired]
fn write(ids, handles, motions) {
    let pages=get_scene_list("factory_visuals"); let current=grid::empty_text(225);
    let current_chunk=grid::current_chunk();
    let decoded=#{};
    for i in 0..ids.len() {
        let id=ids[i]; let page=(id/75).to_string();
        if !decoded.contains(page) { decoded[page]=grid::empty_text(75); }
        decoded[page][id%75]=handles[i];
        if id/225==current_chunk { current[id%225]=handles[i]; }
    }
    for page in decoded.keys() { pages[parse_int(page)]=grid::pack_text(decoded[page]); }
    set_scene_list("factory_visuals",pages); set_scene_list("item_visuals",current);
    write_motions(motions);
}
// Update only these destination owners, preserving every other resident item.
fn assign(ids,handles) {
    let pages=get_scene_list("factory_visuals");let decoded=#{};
    let current=get_scene_list("item_visuals");let region=grid::current_chunk();
    for i in 0..ids.len() {
        let id=ids[i];let page=(id/75).to_string();
        if !decoded.contains(page) {decoded[page]=grid::unpack_text(pages[id/75],75);}
        decoded[page][id%75]=handles[i];
        if id/225==region {current[id%225]=handles[i];}
    }
    for page in decoded.keys() {pages[parse_int(page)]=grid::pack_text(decoded[page]);}
    set_scene_list("factory_visuals",pages);set_scene_list("item_visuals",current);
}
fn write_motions(motions) {
    // Idle factories have no animation pages to clear. Still clear once after
    // the last moving item so a stopped belt cannot replay its previous motion.
    if motions.len()==0 && get_scene_list_item("motion_visuals",867)=="" { return; }
    let names=["motion_visuals","motion_from","motion_to","motion_from_y","motion_to_y","retired_visuals"];
    let motion_pages=#{};
    for motion in motions {
        let page=(motion[0]/75).to_string(); let cell=motion[0]%75;
        if !motion_pages.contains(page) {
            motion_pages[page]=[grid::empty_text(75),grid::empty_numbers(75),grid::empty_numbers(75),grid::empty_numbers(75),grid::empty_numbers(75),grid::empty_text(75),[]];
        }
        motion_pages[page][0][cell]=motion[2]; motion_pages[page][1][cell]=motion[0].to_float(); motion_pages[page][2][cell]=motion[1].to_float();
        motion_pages[page][3][cell]=round(motion[3]*100.0); motion_pages[page][4][cell]=round(motion[4]*100.0);
        if motion[5] { motion_pages[page][5][cell]=motion[2]; }
        motion_pages[page][6].push(cell);
    }
    for i in 0..names.len() {
        let packed=grid::empty_text(if i==0 { 868 } else { 867 });
        for page in motion_pages.keys() {
            packed[parse_int(page)]=if i==0 || i==5 { grid::pack_text(motion_pages[page][i]) } else { grid::pack_cells(motion_pages[page][i],motion_pages[page][6],1) };
        }
        if i==0 {
            let active=[]; for page in motion_pages.keys() { active.push(parse_int(page).to_float()); }
            packed[867]=grid::pack_numbers(active);
        }
        set_scene_list(names[i],packed);
    }
}
