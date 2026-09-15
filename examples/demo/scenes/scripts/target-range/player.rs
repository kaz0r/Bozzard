// First-person controller: mouse-look, movement, gravity, jump, respawn, pointer capture and the
// eye. The script twin of the four "Player ..." graphs that used to drive this scene — the yaw,
// pitch, vertical speed and recoil live on the object blackboard, so the weapon and shoot scripts
// read and write exactly the state a graph would.
//
// The camera follows the body one tick behind, because a move applies after every script of the
// tick has run: `get_position` sees the position the body had when the tick started. At walking
// speed that is a few centimetres of eye lag, and the body the collider moves is the same one.

fn on_start(me) {
    // First person: the camera is the eye, so the body itself must not be in shot.
    set_visible(me, false);
    lock_cursor();
    print("target-range: WASD to move, mouse to look, Space to jump, left-click to shoot, E to swap");
}

fn on_update(me, dt) {
    const SENSITIVITY = 0.2;
    const MIN_PITCH = -75.0;
    const MAX_PITCH = 15.0;
    const SPEED = 4.5;
    const GRAVITY = 9.81;
    const JUMP_SPEED = 6.0;
    const EYE_HEIGHT = 0.5;
    const RECOIL_DECAY = 0.82;
    const FALL_LIMIT = -8.0;
    const START = [0.0, 0.65, 7.0];

    let camera = get_object_variable("camera");

    // Mouse deltas turn the view; the pitch is clamped so looking up or down never rolls over.
    let yaw = get_object_variable("yaw") + mouse_x() * -SENSITIVITY;
    let pitch = clamp(
        get_object_variable("pitch") + mouse_y() * -SENSITIVITY,
        MIN_PITCH,
        MAX_PITCH,
    );
    set_object_variable("yaw", yaw);
    set_object_variable("pitch", pitch);
    // Recoil is added on top of the aim by the shoot script and settles back here.
    let kick = get_object_variable("kick");
    set_rotation(camera, [pitch + kick, yaw, 0.0]);

    // Walk along the camera's own axes: forward flattened to the ground, and its right vector.
    let forward = forward_vector(camera);
    let flat = [forward[0], 0.0, forward[2]];
    let right = [-forward[2], 0.0, forward[0]];
    let step = scale_vector(
        add_vector(scale_vector(flat, move_y()), scale_vector(right, move_x())),
        SPEED * dt,
    );

    // Gravity, then the move. Grounded is the floor contact of the previous tick's move: a move
    // applies after this script has run, so it cannot be read back inside the same tick.
    let grounded = is_grounded(me);
    let vy = get_object_variable("vy") - GRAVITY * dt;
    if grounded {
        // Keep pressing into the floor while it rests, the way the Gravity component does. A tick
        // that moves nothing reports no contact at all, so cancelling the fall outright left the
        // body "not grounded" every other tick and threw away every second jump press.
        vy = -GRAVITY * dt;
        if input_pressed("jump") {
            vy = JUMP_SPEED;
        }
    }
    set_object_variable("vy", vy);
    move_with_collision(me, add_vector(step, [0.0, vy * dt, 0.0]));

    // The eye sits at the body's head and looks wherever the mouse points.
    set_position(camera, add_vector(get_position(me), [0.0, EYE_HEIGHT, 0.0]));

    // Recoil decays on every tick, whether or not anything was fired.
    set_object_variable("kick", kick * RECOIL_DECAY);

    // Below the arena: back to the start, the same rule the respawn graph used.
    if get_position(me)[1] < FALL_LIMIT {
        set_position(me, START);
        set_object_variable("vy", 0.0);
    }
}
