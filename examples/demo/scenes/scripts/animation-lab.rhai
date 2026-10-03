// The same Animator controls run in native Play, the player and the headless server.
fn locomotion(me, x, y, dt) {
    let rate = min(1.0, dt * 9.0);
    let sx = get_object_variable("strafe");
    let fy = get_object_variable("forward");
    sx += (x - sx) * rate;
    fy += (y - fy) * rate;
    set_object_variable("strafe", sx);
    set_object_variable("forward", fy);
    set_animation_parameter(me, "Strafe", sx);
    set_animation_parameter(me, "Forward", fy);
}

fn on_update(me, dt) {
    let t = elapsed_time();
    if me == "interaction-human" {
        // Fade the hand onto the block once the approach is close enough to reach it.
        set_animation_parameter(me, "Reach contact", clamp((animation_progress(me) - 0.35) / 0.45, 0.0, 1.0));
        let cycle = floor(t / 5.0);
        if cycle != get_object_variable("cycle") {
            set_object_variable("cycle", cycle);
            set_position(me, [3.6, 0.0, -0.9]);
            play_animation(me, "Reach", 0.12);
        }
        return;
    }
    if me == "grounded-human" {
        // Alternate the stair/slope and the moving platform, then let grounded IK do its work.
        let platform = (t % 12.0) > 6.0;
        if platform {
            let support = get_position("moving-support");
            set_position(me, [1.2, support[1] + 0.14, -0.9]);
        } else { set_position(me, [1.2, 0.09, 0.0]); }
        set_animation_parameter(me, "Forward", 0.0);
        set_animation_parameter(me, "Wave", 0.0);
        return;
    }
    if me == "retargeted-human" {
        locomotion(me, 0.0, 1.5 + 0.5*sin(t * 0.7), dt);
        set_animation_parameter(me, "Aim", 1.0);
        return;
    }
    if input_pressed("Q") { set_object_variable("wave", !get_object_variable("wave")); }
    if input_pressed("E") { set_object_variable("aim", !get_object_variable("aim")); }
    let x = move_x();
    let y = move_y();
    if abs(x) + abs(y) > 0.01 { set_object_variable("manual", true); }
    if input_pressed("Space") && animation_state(me) != "Jump" {
        play_animation(me, "Jump", 0.12);
    }
    if input_pressed("R") {
        let p = get_position(me);
        set_animation_warp_target(me, "Interact", [p[0], p[1], p[2]+0.9], 180.0);
        play_animation(me, "Reach", 0.12);
    }
    if animation_state(me) == "Reach" && !animation_playing(me) {
        play_animation(me, "Movement", 0.2);
    }
    if get_object_variable("manual") {
        let speed = if input_held("Shift") { 2.0 } else { 1.0 };
        // Keep facing the camera: back/side clips follow world movement relative to the character.
        locomotion(me, -x, -y*speed, dt);
        if animation_state(me) == "Movement" {
            let p = get_position(me);
            set_position(me, [clamp(p[0]+x*dt*1.3*speed, -5.4, -2.3), p[1],
                              clamp(p[2]-y*dt*1.3*speed, -2.0, 2.0)]);
        }
    } else {
        // Starts as a visible showcase; movement input switches to hands-on control.
        let phase = t % 10.0;
        if phase < 5.0 { locomotion(me, 0.0, 1.0 + sin(t*0.6), dt); }
        else { locomotion(me, sin(t*1.2), 0.3, dt); }
    }
    let waving = get_object_variable("wave");
    let aiming = get_object_variable("aim");
    set_animation_parameter(me, "Wave", if waving && !aiming { 1.0 } else { 0.0 });
    set_animation_parameter(me, "Aim", if aiming { 1.0 } else { 0.0 });
}
