// The original scene keeps updating while the annex is acquired and prepared.
fn on_start(me) {
    add_scene_async("annex");
}

fn on_update(me, dt) {
    let handle = loaded_scene_handle();
    if handle != "" && handle != get_scene_variable("seen-load") {
        set_scene_variable("seen-load", handle);
        set_scene_variable("annex-handle", handle);
    }
    if input_pressed("L") && !scene_loading() && get_scene_variable("annex-handle") == "" {
        add_scene_async("annex");
    }
    if input_pressed("C") && scene_loading() {
        cancel_scene_load();
    }
    if input_pressed("U") && !scene_loading() && get_scene_variable("annex-handle") != "" {
        unload_scene(get_scene_variable("annex-handle"));
        set_scene_variable("annex-handle", "");
    }
    let state = "Annex unloaded";
    if get_scene_variable("annex-handle") != "" {
        state = "Annex loaded: " + get_scene_variable("annex-handle");
    }
    if scene_loading() {
        state = "Loading annex: " + (scene_load_progress() * 100.0).to_string() + "%";
    }
    if scene_load_error() != "" {
        state = "Load failed: " + scene_load_error();
    }
    set_text("loading-status", state + "\nL: load  U: unload  C: cancel\nThe center cube keeps spinning while the annex loads.");
}
