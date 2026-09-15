// Left-click fires the equipped gun: one projectile from the eye, sized and launched by the
// weapon's own numbers, plus the recoil the player script settles back down.
//
// The prefab carries the arena-exit rule that stops a miss from stretching the shadow map, so a
// shot that hits nothing cleans itself up.

fn on_update(me, dt) {
    const MUZZLE_OFFSET = 0.8;

    if !input_pressed("fire") {
        return;
    }
    let camera = get_object_variable("camera");
    let forward = forward_vector(camera);
    let shot = spawn_prefab(
        "target-range-projectile",
        add_vector(get_position(camera), scale_vector(forward, MUZZLE_OFFSET)),
    );
    let size = get_object_variable("size");
    set_scale(shot, [size, size, size]);
    set_velocity(shot, scale_vector(forward, get_object_variable("speed")));
    // Recoil is added here and decayed by the player script, exactly like the shoot graph did.
    set_object_variable(
        "kick",
        get_object_variable("kick") + get_object_variable("kick_amount"),
    );
    print("target-range: fired, recoil " + get_object_variable("kick").to_string());
}
