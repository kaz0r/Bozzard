fn on_update(me, dt) {
    let t = elapsed_time();
    set_position("moving-support", [1.2 + 0.08*sin(t*0.9), 0.12 + 0.13*sin(t*0.7), -0.9]);
    set_rotation("moving-support", [4.0*sin(t*0.5), 0.0, 4.0*cos(t*0.5)]);
}
