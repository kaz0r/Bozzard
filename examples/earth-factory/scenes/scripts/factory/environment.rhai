// Rhai gameplay: environment.
import "factory-data" as data;

fn update_camera(dt) {
    let pan = get_object_variable("camera_pan_progress");
    if pan < 1.0 {
        pan = min(1.0, pan + dt / 0.55);
        let eased = pan * pan * pan * (pan * (6.0 * pan - 15.0) + 10.0);
        let from = get_object_list("camera_pan_from");
        set_position("camera-rig", [
            lerp(from[0], get_scene_variable("chunk_x") * 15.0, eased), 0.0,
            lerp(from[2], get_scene_variable("chunk_z") * 15.0, eased)
        ]);
        set_object_variable("camera_pan_progress", pan);
    }
    let heading = get_scene_variable("camera_heading");
    let t = get_scene_variable("camera_progress");
    let pending = get_scene_variable("camera_pending");
    if t >= 1.0 && pending >= 1.0 {
        t = 0.0;
        pending -= 1.0;
    }
    if t < 1.0 {
        t = min(1.0, t + dt / 0.55);
        // Smootherstep has zero speed and acceleration at each end of the orbit.
        let eased = t * t * t * (t * (6.0 * t - 15.0) + 10.0);
        set_rotation("camera-rig", [0.0, heading + 90.0 * eased, 0.0]);
        if t >= 1.0 {
            heading = (heading + 90.0) % 360.0;
            set_rotation("camera-rig", [0.0, heading, 0.0]);
        }
    }
    set_scene_variable("camera_heading", heading);
    set_scene_variable("camera_progress", t);
    set_scene_variable("camera_pending", pending);
}

fn update_zoom(dt, blocked) {
    let target = get_object_variable("camera_zoom_target");
    if !blocked {
        for event in ui_events() {
            if event.kind == "scroll" {
                target = clamp(target * pow(1.12, clamp(event.delta / 40.0, -10.0, 10.0)), 9.0, 32.0);
            }
        }
    }
    let zoom = get_object_variable("camera_zoom");
    zoom += (target - zoom) * min(1.0, dt * 12.0);
    if abs(target - zoom) < 0.001 { zoom = target; }
    set_object_variable("camera_zoom", zoom);
    set_object_variable("camera_zoom_target", target);
    set_camera_size("camera", zoom);
}

fn sky_color(night, day, light) {
    [lerp(night[0], day[0], light), lerp(night[1], day[1], light), lerp(night[2], day[2], light)]
}

fn update_daylight(daylight) {
    if data::on_moon() {
        set_sun_light([0.72, 0.78, 0.90], 0.32);
        set_ambient_light([0.65, 0.71, 0.82], 0.075);
        set_environment([0.001, 0.002, 0.007], [0.006, 0.008, 0.018], [0.013, 0.016, 0.025], 0.50);
        set_star_intensity(1.9); set_exposure(0.0);
        return;
    }
    // Smooth dawn/dusk over a few seconds, then hold true daylight or moonlight.
    let light = clamp(daylight * 2.5 + 0.5, 0.0, 1.0);
    light = light * light * (3.0 - 2.0 * light);
    set_sun_light(sky_color([0.36, 0.50, 0.86], [1.0, 0.94, 0.82], light), lerp(0.14, 2.6, light));
    set_ambient_light(sky_color([0.36, 0.50, 0.86], [0.86, 0.94, 1.0], light), lerp(0.035, 0.18, light));
    set_environment(
        sky_color([0.008, 0.014, 0.036], [0.14, 0.37, 0.68], light),
        sky_color([0.018, 0.028, 0.062], [0.65, 0.77, 0.86], light),
        sky_color([0.012, 0.020, 0.045], [0.24, 0.30, 0.24], light), 0.50);
    let stars = clamp((0.45 - light) / 0.45, 0.0, 1.0);
    set_star_intensity(stars * stars * (3.0 - 2.0 * stars) * 1.6);
    set_exposure(lerp(-0.15, 0.20, light));
}
