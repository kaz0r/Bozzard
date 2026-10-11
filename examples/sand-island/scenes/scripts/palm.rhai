// The palm crown sways in the breeze, and shakes when the castaway pulls a frond.
fn on_update(me, dt) {
    let t = get_scene_variable("world_time");
    let shake = t - get_scene_variable("shake_time");
    let jolt = if shake >= 0.0 && shake < 1.2 { 4.0 * exp(-shake * 3.5) * sin(shake * 28.0) } else { 0.0 };
    set_rotation(me, [1.5 * sin(t * 0.9) + 0.4 * sin(t * 2.3 + 1.0) + jolt, 0.6 * sin(t * 0.37),
        1.1 * cos(t * 0.7) + jolt * 0.6]);
}
