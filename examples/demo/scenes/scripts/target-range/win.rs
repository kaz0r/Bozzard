// The run is over once every cube has been popped. The cubes report themselves through the scene
// blackboard, so this script never names a target and a fifth cube needs no edit here.

fn on_update(me, dt) {
    const TARGETS = 4.0;

    if get_scene_variable("targets-down") < TARGETS {
        return;
    }
    unlock_cursor();
    end_game("You win! All four targets destroyed.");
}
