// Object and camera move at different speeds so both motion and parallax are visible.
fn on_update(me, dt) {
    translate(me, [3.0 * dt, 0.0, 0.0]);
    translate("camera", [0.6 * dt, 0.0, 0.0]);
    if get_position(me)[0] > 5.0 {
        set_position(me, [-5.0, 0.8, 0.0]);
        reset_interpolation(me);
    }
    if get_position("camera")[0] > 2.5 {
        set_position("camera", [-2.5, 3.0, 10.0]);
        reset_interpolation("camera");
    }
}
