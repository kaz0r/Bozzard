// One script asset for all four cubes: whatever physical body reaches this target pops it.
//
// "Popped" is the target's own position far below the arena, which doubles as the guard — a second
// projectile arriving later cannot count the same cube twice. Popping writes the scene blackboard
// counter the game-rules script watches, so no script needs to name the other cubes.

fn on_object_enter(me, other) {
    const POPPED_Y = -50.0;
    const GONE_Y = -100.0;

    if get_position(me)[1] < POPPED_Y {
        return; // already popped
    }
    if !is_rigidbody(other) {
        return; // the player walking into a cube is not a hit
    }
    set_visible(me, false);
    set_position(me, [0.0, GONE_Y, 0.0]);
    set_scene_variable(
        "targets-down",
        get_scene_variable("targets-down") + 1.0,
    );
    print("target-range: popped " + me);
}
