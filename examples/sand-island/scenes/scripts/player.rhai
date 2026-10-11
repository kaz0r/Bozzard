// First-person movement: walking the island, then sailing the raft.
//
// On foot the body walks on island::ground, the analytic surface the voxels were cut
// from, so stepping over 0.25 m voxel terraces is a smooth slope rather than a stair.
// Speed eases in and out, the eye dips on landing and bobs while walking, the sea
// slows wading and floats a swimmer, and the current keeps the castaway near the
// island. Low energy slows you down.
//
// Once the raft is launched the castaway stands on its deck: W/S sail and A/D steer,
// and the view turns with the raft. The game script (game.rhai) decides the mode and
// whether the player has control; the menus write the look settings.
import "island-lib" as island;

const EYE = 1.62;
const WALK = 3.4;
const SPRINT = 6.0;
const SWIM = 1.9;
const GRAVITY = 20.0;
const JUMP = 6.2;
const RADIUS = 0.32;
const TRUNK = 0.34;

fn approach(current, target, rate, dt) {
    current + (target - current) * (1.0 - exp(-rate * dt))
}

fn on_start(me) {
    set_camera_fov("camera", get_scene_variable("fov"));
}

// The raft: slides down the beach while launching, then sails where it is steered.
fn sail(dt, mode, control, yaw, pitch) {
    let t = elapsed_time();
    let site = island::site();
    let x = get_object_variable("raft_x");
    let z = get_object_variable("raft_z");
    let heading = get_object_variable("raft_yaw");
    let speed = get_object_variable("raft_speed");
    let float_y = 0.0;
    let tilt = 0.0;
    if mode == "launch" {
        let k = island::smooth(0.0, 1.0, (t - get_scene_variable("launch_start")) / 3.2);
        let to = island::launch();
        x = site[0] + (to[0] - site[0]) * k;
        z = site[2] + (to[1] - site[2]) * k;
        float_y = site[1] * (1.0 - k) - 0.05 * k;
        tilt = site[4] * (1.0 - k);
        if k >= 1.0 {
            set_scene_variable("mode", "raft");
        }
    } else {
        let throttle = if control { move_y() } else { 0.0 };
        let steer = if control { move_x() } else { 0.0 };
        let want = if throttle > 0.1 { 6.0 } else if throttle < -0.1 { -1.2 } else { 0.0 };
        speed = approach(speed, want, if want == 0.0 { 0.35 } else { 0.55 }, dt);
        let turn = -steer * 26.0 * clamp(abs(speed) / 2.5 + 0.3, 0.0, 1.0) * dt;
        heading += turn;
        yaw += turn;
        let a = to_radians(heading);
        x += -sin(a) * speed * dt;
        z += -cos(a) * speed * dt;
        // The lagoon is too shallow for the raft: it grounds and slides back out.
        if island::ground(x, z) > -0.8 {
            let r = sqrt(x * x + z * z);
            x += x / r * 0.08;
            z += z / r * 0.08;
            speed *= 0.85;
            if t - get_scene_variable("toast_time") > 5.0 {
                set_scene_variable("toast", "Too shallow here. Steer back out to sea.");
                set_scene_variable("toast_time", t);
            }
        }
        let r = sqrt(x * x + z * z);
        if r > 2600.0 {
            x *= 2600.0 / r;
            z *= 2600.0 / r;
        }
        float_y = -0.05;
    }
    let bob = 0.09 * sin(t * 1.3) + 0.04 * sin(t * 2.7 + 1.0);
    let roll = 2.4 * sin(t * 1.1) + 1.2 * sin(t * 0.43);
    let rock = 1.6 * sin(t * 0.9 + 0.5);
    set_position("raft", [x, float_y + bob, z]);
    set_rotation("raft", [tilt + rock, heading, roll]);
    // Sit at the stern beside the tiller, a little off the mast, low enough to see the
    // sea under the sail.
    let a = to_radians(heading);
    let seat = [0.55, 1.45, 1.35];
    let eye = [x + seat[0] * cos(a) + seat[2] * sin(a), float_y + bob + seat[1],
        z - seat[0] * sin(a) + seat[2] * cos(a)];
    set_object_variable("raft_x", x);
    set_object_variable("raft_z", z);
    set_object_variable("raft_yaw", heading);
    set_object_variable("raft_speed", speed);
    set_object_variable("yaw", yaw);
    set_object_variable("pitch", pitch);
    set_position("player", eye);
    set_position("camera", eye);
    set_rotation("camera", [pitch + rock * 0.3, yaw, roll * 0.35]);
}

fn on_update(me, dt) {
    let mode = get_scene_variable("mode");
    let control = get_scene_variable("control");
    let p = [get_object_variable("px"), get_object_variable("py"), get_object_variable("pz")];
    let v = [get_object_variable("vx"), get_object_variable("vy"), get_object_variable("vz")];
    let yaw = get_object_variable("yaw");
    let pitch = get_object_variable("pitch");
    let grounded = get_object_variable("grounded");

    // Look.
    if control {
        let turn = get_scene_variable("sensitivity") * 0.1;
        let invert = if get_scene_variable("invert_y") { -1.0 } else { 1.0 };
        yaw -= mouse_x() * turn;
        pitch = clamp(pitch - mouse_y() * turn * invert, -88.0, 88.0);
    }
    let fov = get_scene_variable("fov");
    if get_object_variable("aboard") {
        if abs(fov - get_object_variable("fov_now")) > 0.01 {
            set_object_variable("fov_now", fov);
            set_camera_fov("camera", fov);
        }
        sail(dt, mode, control, yaw, pitch);
        return;
    }

    // Wanted horizontal velocity along the view's flattened axes.
    let mx = 0.0;
    let my = 0.0;
    if control {
        mx = move_x();
        my = move_y();
    }
    let amount = sqrt(mx * mx + my * my);
    if amount > 1.0 {
        mx /= amount;
        my /= amount;
    }
    let a = to_radians(yaw);
    let forward = [-sin(a), -cos(a)];
    let right = [cos(a), -sin(a)];
    let floor_y = island::ground(p[0], p[2]);
    let depth = -floor_y;
    let swimming = depth > island::swim_depth();
    let tired = get_scene_variable("energy") < 20.0;
    let sprinting = control && !tired && input_held("Shift") && my > 0.1;
    let speed = if swimming {
        if sprinting { SWIM * 1.45 } else { SWIM }
    } else {
        let wade = 1.0 - 0.45 * clamp(depth / 1.1, 0.0, 1.0);
        (if sprinting { SPRINT } else { WALK }) * wade
    };
    if tired {
        speed *= 0.7;
    }
    let want = [(forward[0] * my + right[0] * mx) * speed, (forward[1] * my + right[1] * mx) * speed];
    let grip = if swimming { 3.5 } else if grounded { 11.0 } else { 2.2 };
    v[0] = approach(v[0], want[0], grip, dt);
    v[2] = approach(v[2], want[1], grip, dt);
    p[0] += v[0] * dt;
    p[2] += v[2] * dt;

    // The palm trunk is a post to walk around.
    let palm = island::palm();
    let tx = p[0] - palm[0];
    let tz = p[2] - palm[1];
    let reach = RADIUS + TRUNK;
    let d = sqrt(tx * tx + tz * tz);
    if d < reach && d > 0.0001 {
        let nx = tx / d;
        let nz = tz / d;
        p[0] = palm[0] + nx * reach;
        p[2] = palm[1] + nz * reach;
        let into = v[0] * nx + v[2] * nz;
        if into < 0.0 {
            v[0] -= into * nx;
            v[2] -= into * nz;
        }
    }

    // The current turns a swimmer back before the open sea.
    let r = sqrt(p[0] * p[0] + p[2] * p[2]);
    let limit = island::boundary();
    if r > limit {
        p[0] *= limit / r;
        p[2] *= limit / r;
        let out = (v[0] * p[0] + v[2] * p[2]) / limit;
        if out > 0.0 {
            v[0] -= out * p[0] / limit;
            v[2] -= out * p[2] / limit;
        }
        if elapsed_time() - get_scene_variable("toast_time") > 6.0 {
            set_scene_variable("toast", "The current is too strong. You need a raft to get past it.");
            set_scene_variable("toast_time", elapsed_time());
        }
    }

    // Vertical: feet on the sand, a jump, or floating at the surface.
    floor_y = island::ground(p[0], p[2]);
    depth = -floor_y;
    swimming = depth > island::swim_depth();
    let dip = get_object_variable("dip");
    if swimming {
        let float_y = -island::swim_depth() + 0.08 * sin(elapsed_time() * 1.7);
        p[1] = approach(p[1], max(float_y, floor_y), 4.0, dt);
        v[1] = 0.0;
        grounded = false;
    } else {
        let was_grounded = grounded;
        v[1] -= GRAVITY * dt;
        if grounded && control && input_pressed("jump") {
            v[1] = JUMP;
        }
        p[1] += v[1] * dt;
        // Follow the sand down gentle slopes instead of skipping off them.
        if was_grounded && v[1] <= 0.0 && p[1] - floor_y < 0.3 {
            p[1] = floor_y;
        }
        if p[1] <= floor_y {
            if !was_grounded && v[1] < -3.0 {
                dip = max(-0.16, v[1] * 0.018);
            }
            p[1] = floor_y;
            v[1] = 0.0;
            grounded = true;
        } else {
            grounded = false;
        }
    }
    dip = approach(dip, 0.0, 7.0, dt);

    // The eye: head bob while walking, a wider field of view while sprinting.
    let horizontal = sqrt(v[0] * v[0] + v[2] * v[2]);
    let phase = get_object_variable("bob");
    let bob_on = get_scene_variable("bobbing");
    let bob_amount = approach(get_object_variable("bob_amount"),
        if bob_on && grounded && horizontal > 0.4 { min(horizontal / WALK, 1.4) } else { 0.0 }, 8.0, dt);
    phase += dt * horizontal * 1.9;
    let bob_y = sin(phase * 2.0) * 0.032 * bob_amount;
    let bob_x = cos(phase) * 0.022 * bob_amount;
    let eye = if swimming { 0.3 + island::swim_depth() } else { EYE };
    let sprint_fov = approach(get_object_variable("fov_now"),
        fov + if sprinting && horizontal > WALK { 7.0 } else { 0.0 }, 6.0, dt);
    if abs(sprint_fov - get_object_variable("fov_now")) > 0.01 {
        set_camera_fov("camera", sprint_fov);
    }

    set_object_variable("px", p[0]);
    set_object_variable("py", p[1]);
    set_object_variable("pz", p[2]);
    set_object_variable("vx", v[0]);
    set_object_variable("vy", v[1]);
    set_object_variable("vz", v[2]);
    set_object_variable("yaw", yaw);
    set_object_variable("pitch", pitch);
    set_object_variable("grounded", grounded);
    set_object_variable("dip", dip);
    set_object_variable("bob", phase);
    set_object_variable("bob_amount", bob_amount);
    set_object_variable("fov_now", sprint_fov);
    set_position(me, p);
    set_position("camera", [p[0] + right[0] * bob_x, p[1] + eye + bob_y + dip, p[2] + right[1] * bob_x]);
    set_rotation("camera", [pitch, yaw, 0.0]);
}
