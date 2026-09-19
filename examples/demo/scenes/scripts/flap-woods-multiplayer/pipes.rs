// Presentation only. The host's round script owns movement and recycling.
fn on_update(me, dt) {
    if !network_active() { return; }
    let pipe = network_object(me);
    if pipe.len() == 0 { return; }
    set_position(me, [pipe.x, 0.0, 0.0]);
    set_position(me + "-bottom", [0.0, pipe.gap - 9.05, 0.0]);
    set_position(me + "-top", [0.0, pipe.gap + 9.05, 0.0]);
}
