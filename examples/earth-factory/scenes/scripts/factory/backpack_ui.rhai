import "factory-backpack" as backpack;
import "factory-data" as data;

fn slot_at(target) {
    for slot in 0..25 { if target == "player-slot-"+slot.to_string() { return slot; } }
    -1
}
fn update() {
    let drag=data::session_value(47).to_int(); let menu=data::session_value(48).to_int();
    for event in ui_events() {
        let slot=slot_at(event.target);
        if event.kind=="cancel" { drag=-1; menu=-1; }
        else if event.kind=="secondary" {
            drag=-1; menu=-1;
            if slot>=0 && backpack::slots()[slot*2+1]>0.0 { menu=slot; set_ui_screen_position("backpack-menu",event.x,event.y); }
        } else if event.kind=="down" {
            if !event.target.starts_with("backpack-") { menu=-1; }
            if slot>=0 && backpack::slots()[slot*2+1]>0.0 { drag=slot; }
        } else if event.kind=="up" {
            if drag>=0 { backpack::move_stack(drag,slot); } drag=-1;
        } else if event.kind=="activate" {
            if event.target=="backpack-destroy-all" { backpack::clear(); drag=-1; menu=-1; }
            if event.target=="backpack-destroy" { backpack::destroy(menu); menu=-1; }
            if event.target=="backpack-split" {
                if !backpack::split(menu) { set_ui_text("player-inventory-help","Split needs an empty slot and at least two items."); }
                menu=-1;
            }
            if event.target=="backpack-cancel" { menu=-1; }
        }
    }
    data::session_set(47,drag.to_float()); data::session_set(48,menu.to_float());
    set_ui_visible("backpack-drag",drag>=0); set_ui_visible("backpack-menu",menu>=0);
    let inventory=backpack::slots();
    if drag>=0 {
        let pointer=ui_pointer(); set_ui_screen_position("backpack-drag",pointer[0],pointer[1]);
        set_ui_text("backpack-drag",data::item_name(inventory[drag*2])+" x"+inventory[drag*2+1].to_int().to_string());
    }
    if !data::ui_view_changed(3,[data::session_value(49),drag,menu].to_string()) { return; }
    let used=0;
    for slot in 0..25 {
        let kind=inventory[slot*2]; let amount=inventory[slot*2+1];
        if amount>0.0 { used+=1; }
        set_ui_text("player-slot-"+slot.to_string(),if amount>0.0 { data::item_name(kind)+"\n"+amount.to_int().to_string()+" / 100" } else { "" });
        set_ui_background("player-slot-"+slot.to_string(),if drag==slot { [0.07, 0.077, 0.12, 1.0] } else { [0.11, 0.122, 0.19, 1.0] });
    }
    set_ui_text("player-inventory-footer",used.to_string()+" / 25 slots  /  100 per stack");
    set_ui_enabled("backpack-destroy-all",used>0);
    if menu>=0 { set_ui_enabled("backpack-split",inventory[menu*2+1]>=2.0 && used<25); }
}
