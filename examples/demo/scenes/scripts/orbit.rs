// Circles the origin, keeping its phase in a scene variable shared with every other script.

fn on_update(me, dt) {
    const RADIUS = 3.0;
    const HEIGHT = 1.6;
    const RADIANS_PER_SECOND = 1.1;
    let phase = get_scene_variable("orbit-phase") + RADIANS_PER_SECOND * dt;
    set_scene_variable("orbit-phase", phase);
    set_position(me, [
        RADIUS * cos(phase),
        HEIGHT + sin(phase * 2.0) * 0.35,
        RADIUS * sin(phase),
    ]);
}
