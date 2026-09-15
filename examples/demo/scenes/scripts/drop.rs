// Bounces off whatever it hits, using the same rigidbody actions a graph would call.

fn on_start(me) {
    print("drop: falling");
    set_velocity(me, [0.0, 0.0, 0.0]);
}

fn on_collision_enter(me, other, normal, impulse) {
    print("drop: hit " + other + " at " + impulse.to_string() + " N·s");
    // The normal points from the other body towards this one, so a landing reads as +Y.
    if normal[1] > 0.5 {
        set_velocity(me, [0.0, 6.5, 0.0]);
    }
}
