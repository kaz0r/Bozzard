// Counts whatever reaches the gate volume and keeps a HUD text object up to date.
//
// The HUD object ID is an object variable, so retargeting it needs no script edit.

fn on_start(me) {
    print("gate: armed at " + me);
    set_scene_variable("passes", 0.0);
}

fn on_object_enter(me, other) {
    print("gate: " + other + " arrived");
    set_scene_variable("passes", get_scene_variable("passes") + 1.0);
}

fn on_update(me, dt) {
    let hud = get_object_variable("hud");
    set_text(hud, "PASSES " + get_scene_variable("passes").to_string()
        + " | overlaps " + overlap_count(me).to_string());
}

fn on_destroy(me) {
    print("gate: torn down");
}
