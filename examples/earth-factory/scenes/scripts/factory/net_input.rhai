// Leaf module: UI/gameplay modules can import this without import cycles.
fn guest() { network_active() && network_state().contains("factory_guest") }
fn blocked() {network_active() && network_state().contains("session_panel") && network_state().session_panel}
fn send(kind,args) {
    if !guest() { return false; }
    if network_state().phase!="live" {set_scene_variable("message","Synchronizing with the host…");return false;}
    let sent=network_send("factory."+kind,args);
    if !sent {set_scene_variable("message","Waiting for the host to process your actions…");}
    sent
}
fn position(cell) {
    #{planet:get_object_list_item("session",7).to_int(),
      x:get_scene_variable("chunk_x").to_int()*15+cell%15-7,
      z:get_scene_variable("chunk_z").to_int()*15+cell/15-7}
}
fn address(id) {
    #{planet:get_object_list_item("session",7).to_int(),
      x:(id/225%17-8)*15+id%225%15-7,
      z:(id/225/17-8)*15+id%225/15-7}
}
fn gathering(active) {
    let session=get_object_list("session");
    let requested=if active {1.0}else{0.0};
    if session[127]!=requested && send("gather",#{active:active}) {session[127]=requested;set_object_list("session",session);}
}
