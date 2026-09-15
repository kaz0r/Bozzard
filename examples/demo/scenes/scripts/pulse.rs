// Fades a light with `sin`, the kind of motion a graph would need several nodes for.

fn on_update(me, dt) {
    const BASE_INTENSITY = 4.0;
    const SWING = 3.0;
    const HERTZ_SCALE = 2.0;
    set_light_intensity(me, BASE_INTENSITY + SWING * sin(elapsed_time() * HERTZ_SCALE));
}
