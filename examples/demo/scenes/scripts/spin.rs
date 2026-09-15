// Rotates its object, the coding half of the Spin component.
//
// Rhai functions cannot read top-level `let`/`const`, so tuning values live inside the hook that
// uses them; anything that must outlive a tick belongs on an object or scene variable.

fn on_start(me) {
    print("spin: turning " + me);
}

fn on_update(me, dt) {
    const SPIN_SPEED = [0.0, 45.0, 0.0];
    // `rotate` takes a degrees delta, exactly like the Rotate blueprint node.
    rotate(me, scale_vector(SPIN_SPEED, dt));
}
