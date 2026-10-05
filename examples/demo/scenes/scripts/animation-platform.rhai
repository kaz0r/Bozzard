fn on_update(me, dt) {
    // One complete cycle fits the six-second capture. Each foot rides its own collider.
    let phase = elapsed_time() * 1.0471975512;
    let height = 0.13 * sin(phase);
    set_position("foot-step", [1.36, 0.22 + height, 0.0]);
    set_position("foot-slope", [1.03, 0.22 - height, 0.0]);
    set_rotation("foot-slope", [12.0 * cos(phase), 0.0, 0.0]);
}
