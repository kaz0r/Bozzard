// Rhai host script. WGSL runs on the GPU; this file configures resources and dispatches.
fn on_start(me) {
    unlock_cursor();
    if !compute_available() { return; }
    compute_create_texture("surface", 513, 257, "rgba8unorm");
    compute_bind_material(me, "base_color", compute_texture("surface"));
}

fn on_update(me, dt) {
    if !compute_available() { return; }
    compute_dispatch_extent("waves", "main",
        #{ output: compute_texture("surface") },
        #{ time: elapsed_time(), amplitude: 0.85 }, [513, 257]);
}
