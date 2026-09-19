// Open the shutter to reveal the warehouse. Rendering decides visibility;
// gameplay state and object lifetimes remain independent of occlusion.
fn on_update(me, dt) {
    if input_pressed("Space") {
        let open = !get_scene_variable("shutter-open");
        set_scene_variable("shutter-open", open);
        if open { set_position(me, [14.0, 2.0, 4.0]); }
        else { set_position(me, [0.0, 2.0, 4.0]); }
    }
}
