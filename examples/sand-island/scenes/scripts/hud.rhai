// The HUD and the settings menu.
//
// Esc (or the Settings button) opens settings; game time, the ship and the player pause
// while it is open. Sliders and toggles are read every tick and applied when they
// change; quality, VSync and window mode go through the engine's player settings, so
// they are also saved for the next run. Everything else here draws what the game
// script decided: the clock and compass, the objective, the interaction prompt, the
// inventory, energy, messages and the labels floating over the raft and lighthouse.
import "island-lib" as island;

const QUALITY = ["low", "medium", "high"];
const WINDOW = ["windowed", "borderless", "fullscreen"];
const DISTANCE = ["Near", "Medium", "Far"];
const HEADINGS = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

fn title(text) {
    text.sub_string(0, 1).to_upper() + text.sub_string(1)
}

fn cycle(list, current) {
    let i = list.index_of(current);
    list[(i + 1) % list.len()]
}

// Rewrite a label only when its text changes; the last text is kept on this object.
fn show(id, key, text) {
    if text != get_object_variable(key) {
        set_object_variable(key, text);
        set_ui_text(id, text);
    }
}

fn part_of_day(hours) {
    if hours < 5.0 || hours >= 21.0 { "Night" } else if hours < 11.0 { "Morning" }
    else if hours < 14.0 { "Midday" } else if hours < 18.0 { "Afternoon" } else { "Evening" }
}

fn set_menu(open) {
    set_scene_variable("menu_open", open);
    set_ui_visible("ui-menu", open);
    clear_ui_focus();
}

// Fade a message in and out over `length` seconds after `since`.
fn flash(id, text, since, length) {
    let visible = text != "" && since >= 0.0 && since < length;
    set_ui_visible(id, visible);
    if visible {
        set_ui_text(id, text);
        set_ui_opacity(id, min(1.0, min(since * 4.0, (length - since) * 1.5)));
    }
}

// Pin a label over a world point; hide it beyond `range` metres.
fn label(id, key, eye, point, range, text) {
    let dx = point[0] - eye[0];
    let dz = point[2] - eye[2];
    let distance = sqrt(dx * dx + dz * dz);
    let visible = distance < range;
    set_ui_visible(id, visible);
    if visible {
        set_ui_world_position(id, point);
        show(id, key, text);
    }
    distance
}

fn on_start(me) {
    set_ui_value("ui-opt-vsync", if get_vsync_setting() { 1.0 } else { 0.0 });
    set_ui_text("ui-opt-quality", `Quality: ${title(get_quality_setting())}`);
    set_ui_text("ui-opt-window", `Window: ${title(get_window_mode_setting())}`);
}

fn on_update(me, dt) {
    let now = elapsed_time();
    let mode = get_scene_variable("mode");
    let open = get_scene_variable("menu_open");
    let camp = get_scene_variable("camp_open");
    let fading = get_scene_variable("fade_kind") != "";
    for event in ui_events() {
        if event.kind != "activate" {
            continue;
        }
        switch event.target {
            "ui-settings-button" => { set_menu(true); open = true; }
            "ui-resume" => { set_menu(false); open = false; }
            "ui-quit" => quit_game(),
            "ui-opt-quality" => {
                let next = cycle(QUALITY, get_quality_setting());
                set_quality_setting(next);
                apply_settings();
                save_settings();
                set_ui_text("ui-opt-quality", `Quality: ${title(next)}`);
            }
            "ui-opt-window" => {
                let next = cycle(WINDOW, get_window_mode_setting());
                set_window_mode_setting(next);
                apply_settings();
                save_settings();
                set_ui_text("ui-opt-window", `Window: ${title(next)}`);
            }
        }
    }

    // Settings: apply whatever changed since the last tick.
    let sensitivity = get_ui_value("ui-opt-sensitivity");
    if sensitivity != get_scene_variable("sensitivity") {
        set_scene_variable("sensitivity", sensitivity);
    }
    show("ui-opt-sensitivity", "sensitivity_text", `Mouse sensitivity   ${island::fixed(sensitivity, 2)}x`);
    let fov = round(get_ui_value("ui-opt-fov"));
    if fov != get_scene_variable("fov") {
        set_scene_variable("fov", fov);
    }
    show("ui-opt-fov", "fov_text", `Field of view   ${fov.to_int()}°`);
    set_scene_variable("invert_y", get_ui_value("ui-opt-invert") > 0.5);
    set_scene_variable("bobbing", get_ui_value("ui-opt-bob") > 0.5);
    let distance = round(get_ui_value("ui-opt-distance"));
    set_scene_variable("view_distance", distance);
    show("ui-opt-distance", "distance_text", `View distance   ${DISTANCE[distance.to_int()]}`);
    let shadows = get_ui_value("ui-opt-shadows") > 0.5;
    if shadows != get_scene_variable("shadows") {
        set_scene_variable("shadows", shadows);
        set_sun_shadows(shadows);
    }
    let clouds = get_ui_value("ui-opt-clouds") > 0.5;
    if clouds != get_scene_variable("clouds") {
        set_scene_variable("clouds", clouds);
        for i in 0..island::clouds() {
            set_visible(`cloud-${i}`, clouds);
        }
    }
    let bloom = get_ui_value("ui-opt-bloom") > 0.5;
    if bloom != get_scene_variable("bloom") {
        set_scene_variable("bloom", bloom);
        set_bloom_intensity(if bloom { island::bloom() } else { 0.0 });
    }
    let vsync = get_ui_value("ui-opt-vsync") > 0.5;
    if vsync != get_vsync_setting() {
        set_vsync_setting(vsync);
        apply_settings();
        save_settings();
    }
    let show_fps = get_ui_value("ui-opt-fps") > 0.5;
    set_ui_visible("ui-fps", show_fps);
    if show_fps && now - get_object_variable("fps_at") > 0.25 {
        set_object_variable("fps_at", now);
        let stats = render_stats();
        if stats.available && stats.rate_ready {
            show("ui-fps", "fps_text", `${round(stats.fps).to_int()} FPS   ${island::fixed(stats.frame_ms, 1)} ms`);
        }
    }

    // Which parts of the HUD belong to this moment.
    let playing = mode == "island" || mode == "launch" || mode == "raft";
    let on_foot = mode == "island";
    // Escape must always belong to some button, or the player would quit on it.
    let free = playing && !open && !camp;
    set_ui_visible("ui-settings-button", free);
    set_ui_visible("ui-camp-button", free && on_foot && !fading);
    for id in ["ui-clock", "ui-compass", "ui-crosshair", "ui-objective"] {
        set_ui_visible(id, playing);
    }
    set_ui_visible("ui-hotbar", on_foot);
    set_ui_visible("ui-energy", on_foot);
    let since_start = now - get_scene_variable("start_time");
    set_ui_visible("ui-hint", on_foot && !open && !camp && since_start < 30.0);
    set_ui_opacity("ui-hint", clamp((30.0 - since_start) * 0.5, 0.0, 1.0));
    if !playing {
        for id in ["ui-prompt", "ui-pickup", "ui-label-site", "ui-label-light"] {
            set_ui_visible(id, false);
        }
        flash("ui-toast", if open { "" } else { get_scene_variable("toast") },
            now - get_scene_variable("toast_time"), 6.0);
        return;
    }

    // Clock and compass.
    let hours = get_scene_variable("time_of_day");
    let day = get_scene_variable("day").to_int();
    show("ui-clock", "clock_text", `Day ${day}  ·  ${island::hours_text(hours)}  ·  ${part_of_day(hours)}`);
    let yaw = get_object_variable("player", "yaw");
    let heading = (360.0 - (yaw % 360.0) + 360.0) % 360.0;
    let index = (round(heading / 45.0).to_int()) % 8;
    show("ui-compass", "compass_text", `${HEADINGS[index]}   ${round(heading).to_int() % 360}°`);

    // Inventory and energy.
    for item in island::items() {
        show(`ui-count-${item}`, `count_${item}`, `${get_scene_variable(item).to_int()} ${island::names()[item]}`);
    }
    let energy = get_scene_variable("energy");
    set_ui_size("ui-energy-fill", 236.0 * clamp(energy / 100.0, 0.0, 1.0), 12.0);
    set_ui_background("ui-energy-fill", if energy > 50.0 {
        [0.3, 0.85, 0.45, 1.0]
    } else if energy > 25.0 {
        [0.95, 0.8, 0.25, 1.0]
    } else {
        [0.95, 0.3, 0.25, 1.0]
    });

    // The objective: what to do next, and what it still needs.
    let stage = get_scene_variable("raft_stage");
    let seen = get_scene_variable("lighthouse_seen");
    let eye = get_position("camera");
    let light = island::beacon();
    let ldx = light[0] - eye[0];
    let ldz = light[2] - eye[2];
    let to_light = sqrt(ldx * ldx + ldz * ldz).to_int();
    let objective = if mode != "island" {
        if seen {
            `Sail to the lighthouse  ·  ${to_light} m` + "\nW / S sail   A / D steer"
        } else {
            "Sail out and look for land\nW / S sail   A / D steer"
        }
    } else if stage < 5.0 {
        let next = island::stages()[stage.to_int()];
        let need = island::missing_text(next.cost);
        `Build a raft at the stakes on the south-west beach  (${stage.to_int()}/5)` + "\n"
            + `Next: ${next.name}  ·  ${if need == "" { "ready to build (E)" } else { "need " + need }}`
    } else if seen {
        "Your raft is ready\nLaunch it from the beach (E) and sail for the lighthouse"
    } else {
        "Your raft is ready, but where to?\nLook out to the south-west, or wait for nightfall"
    };
    show("ui-objective", "objective_text", objective);

    // The interaction prompt, pickups and messages.
    let prompt = get_scene_variable("prompt");
    set_ui_visible("ui-prompt", prompt != "" && !open && !camp);
    show("ui-prompt", "prompt_text", prompt);
    flash("ui-pickup", get_scene_variable("pickup"), now - get_scene_variable("pickup_time"), 1.8);
    flash("ui-toast", if open { "" } else { get_scene_variable("toast") }, now - get_scene_variable("toast_time"), 6.0);

    // Labels over the raft site and, once spotted, the lighthouse.
    let site = island::site();
    if on_foot {
        label("ui-label-site", "site_text", eye, [site[0], site[1] + 2.2, site[2]], 60.0,
            if stage < 5.0 { `Raft  ${stage.to_int()}/5` } else { "Raft  ·  ready" });
    } else {
        set_ui_visible("ui-label-site", false);
    }
    if seen {
        label("ui-label-light", "light_text", eye, [light[0], light[1] + 6.0, light[2]], 5000.0,
            `Lighthouse  ·  ${to_light} m`);
    } else {
        set_ui_visible("ui-label-light", false);
    }

    // The camp panel: grey out what cannot be made right now.
    if camp {
        let recipes = island::recipes();
        for item in ["rope", "paddle", "sail"] {
            set_ui_enabled(`ui-craft-${item}`, island::affordable(recipes[item]));
        }
        set_ui_enabled("ui-eat", get_scene_variable("coconuts") >= 1.0 && energy < 99.5);
        let p = get_position("player");
        set_ui_enabled("ui-rest", island::ground(p[0], p[2]) > 0.0);
        set_ui_text("ui-rest", if hours >= 6.5 && hours < 18.5 {
            "Rest in the shade until dusk"
        } else {
            "Sleep under the stars until dawn"
        });
    }
}
