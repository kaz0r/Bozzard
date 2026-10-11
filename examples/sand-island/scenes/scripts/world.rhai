// The day/night cycle, and the lighthouse's light.
//
// One real second is eight game seconds (a day lasts three real hours); hold T to
// speed time up. The sun rises in the east (+X), crosses the southern sky and sets in
// the west; the moon follows on the opposite side. The scene's one sun light follows
// whichever of the two is up, and the sky, fog, ambient light and stars blend between
// night, dusk and day by the sun's height. Time stands still on the title screen and
// while the settings menu is open. At night the lighthouse lamp burns and its beam turns.
import "island-lib" as island;

const DISTANCE = 520.0;
const TILT = 0.42;

fn mix(a, b, t) {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

// Turn a disc (its face toward local +Z) to face the camera from `distance` along d.
fn face(id, eye, d, distance) {
    set_position(id, [eye[0] + d[0] * distance, eye[1] + d[1] * distance, eye[2] + d[2] * distance]);
    set_rotation(id, [to_degrees(asin(clamp(d[1], -1.0, 1.0))), to_degrees(atan2(-d[0], -d[2])), 0.0]);
}

fn on_update(me, dt) {
    let paused = get_scene_variable("menu_open") || get_scene_variable("mode") == "title";
    let rate = if !paused && input_held("T") { 720.0 } else { island::time_scale() };

    // Game seconds since midnight are kept whole plus a fraction: adding a sixtieth of
    // a second to a float that counts a whole day would round the clock off.
    let clock = get_scene_variable("clock");
    let fraction = get_scene_variable("clock_fraction");
    if !paused {
        set_scene_variable("world_time", get_scene_variable("world_time") + dt);
        fraction += dt * rate;
        let whole = floor(fraction);
        fraction -= whole;
        clock += whole;
        if clock >= 86400.0 {
            clock -= 86400.0;
            set_scene_variable("day", get_scene_variable("day") + 1.0);
        }
        set_scene_variable("clock", clock);
        set_scene_variable("clock_fraction", fraction);
    }
    let hours = (clock + fraction) / 3600.0;
    set_scene_variable("time_of_day", hours);

    let theta = (hours - 6.0) / 12.0 * 3.14159265;
    let sun = [cos(theta), sin(theta) * cos(TILT), sin(theta) * sin(TILT)];
    let moon = [-sun[0], -sun[1], sun[2] * 0.6 + 0.25];
    let e = sun[1];

    // Sky: night -> dusk -> day by the sun's elevation.
    let sky = island::sky();
    let from = if e < 0.0 { sky.night } else { sky.dusk };
    let to = if e < 0.0 { sky.dusk } else { sky.day };
    let t = if e < 0.0 { island::smooth(-0.24, 0.0, e) } else { island::smooth(0.0, 0.32, e) };
    let zenith = mix(from.zenith, to.zenith, t);
    let horizon = mix(from.horizon, to.horizon, t);
    let ground = mix(from.ground, to.ground, t);
    let intensity = island::sky_intensity();
    set_environment(zenith, horizon, ground, intensity);
    let distance = get_scene_variable("view_distance");
    let density = if distance < 0.5 { 0.0026 } else if distance < 1.5 { 0.0012 } else { 0.0006 };
    set_fog(mix([0.0, 0.0, 0.0], horizon, intensity), density);
    set_star_intensity(0.8 * (1.0 - island::smooth(-0.2, -0.02, e)));

    // One directional light: the sun by day, the moon by night, dark in between. The
    // light steps in tenths of a degree, so the shadow map is not redrawn (and its edges
    // do not crawl) every tick while the visible discs and the sky move smoothly.
    let daylight = island::smooth(-0.02, 0.3, e);
    set_scene_variable("daylight", daylight);
    let step = 0.00174533;
    let q = round(theta / step) * step;
    let light_sun = [cos(q), sin(q) * cos(TILT), sin(q) * sin(TILT)];
    let light_moon = [-light_sun[0], -light_sun[1], light_sun[2] * 0.6 + 0.25];
    if e > -0.03 {
        set_sun_direction(light_sun);
        set_sun_light(mix([1.0, 0.42, 0.16], [1.0, 0.96, 0.88], island::smooth(0.0, 0.4, e)),
            island::sun_intensity() * daylight);
    } else {
        set_sun_direction(light_moon);
        set_sun_light([0.55, 0.66, 1.0], 0.32 * island::smooth(-0.02, 0.3, moon[1]));
    }
    set_ambient_light(mix([0.42, 0.52, 0.95], [0.86, 0.93, 1.0], daylight), 0.03 + 0.06 * daylight);

    // The voxel sun and moon sit far out along their directions, facing the eye.
    let eye = get_position("camera");
    face("sun", eye, sun, DISTANCE);
    face("moon", eye, moon, DISTANCE);
    set_visible("sun", e > -0.12);
    let low = e < 0.1;
    if low != get_object_variable("low_sun") {
        set_object_variable("low_sun", low);
        set_mesh("sun", if low { "sun-low" } else { "sun" });
    }
    set_visible("moon", moon[1] > -0.12);

    // Lights that only burn after dark: the ship's, the lighthouse lamp, its turning
    // beam and the keeper's windows.
    let dark = e < 0.06;
    set_visible("ship-lights", dark);
    set_visible("lighthouse-lamp", dark);
    set_visible("lighthouse-beam", dark);
    set_visible("lighthouse-windows", dark);
    if dark {
        // The lamp flares whenever the turning beam sweeps toward the eye.
        let turn = get_scene_variable("world_time") * 40.0;
        set_rotation("lighthouse-beam", [0.0, turn, 0.0]);
        let lamp = island::beacon();
        let dx = eye[0] - lamp[0];
        let dz = eye[2] - lamp[2];
        let r = to_radians(turn);
        let facing = abs((cos(r) * dx - sin(r) * dz) / max(sqrt(dx * dx + dz * dz), 0.001));
        let flare = 1.0 + 4.0 * island::smooth(0.985, 1.0, facing);
        set_scale("lighthouse-lamp", [flare, flare, flare]);
    }
}
