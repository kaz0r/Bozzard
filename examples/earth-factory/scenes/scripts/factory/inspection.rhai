import "factory-data" as data;
import "factory-grid" as grid;
import "factory-inventory" as inventory;
import "factory-panels" as panels;

fn close() {
    data::session_set(46,-1.0); set_ui_visible("machine-inspect-overlay",false); set_ui_enabled("machine-inspect-overlay",false);
}
fn open(cell) {
    let kind=get_scene_list("builds")[cell];
    panels::set_extra_panel(""); panels::set_assembler(-1); panels::close_storage();
    set_object_variable("journal_open",false); set_object_variable("journal_alpha",0.0); set_ui_visible("journal-overlay",false);
    close();
    if kind==3.0 || kind==5.0 || kind==11.0 { panels::set_assembler(cell); }
    else if kind==4.0 {
        set_scene_variable("storage_cell",cell.to_float()); set_scene_variable("storage_open",true);
        set_ui_text("storage-title","Storage / " + grid::cell_x(cell).to_string()+", "+grid::cell_z(cell).to_string());
        set_ui_text("storage-help","Drag to move or merge / Right-click for stack actions");
    } else if kind==1.0 || kind==7.0 || kind==8.0 {
        data::session_set(46,cell.to_float());
        set_ui_visible("machine-inspect-overlay",true); set_ui_enabled("machine-inspect-overlay",true);
        draw(cell);
    }
}
fn draw(cell) {
    let kind=get_scene_list("builds")[cell]; let item=get_scene_list("items")[cell]; let amount=get_scene_list("item_amounts")[cell];
    set_ui_text("machine-inspect-title",data::build_name(kind)+" / BUFFER");
    set_ui_text("machine-inspect-body",if amount>0.0 { data::item_name(item)+"\n"+amount.to_int().to_string()+" / "+(if kind==1.0 { 100 } else { 10 }).to_string()+" items" } else { "Buffer empty" });
    set_ui_text("machine-inspect-status",if kind==1.0 { if get_object_list("power_live")[cell]>0.0 { "POWERED / Mining continues while inspecting" } else { "NO POWER / Connect to a supplied pole" } } else { "Transfers continue while inspecting" });
    set_ui_enabled("machine-inspect-take",amount>0.0 && get_object_list("rotation_turns")[cell]==0.0);
}
fn update() {
    let cell=data::session_value(46).to_int(); if cell<0 { return false; }
    if input_pressed("E") { close(); return true; }
    for event in ui_events() {
        if event.kind!="activate" { continue; }
        if event.target=="machine-inspect-close" { close(); return true; }
        if event.target=="machine-inspect-take" { inventory::collect_machine(cell); }
    }
    draw(cell); true
}
