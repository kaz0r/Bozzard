// The castaway's game.
//
// Gather driftwood that washes up on the beach, pull fronds from the palm (shaking it
// sometimes drops a coconut), and pick up coconuts. At camp (C) twist rope, carve a
// paddle, weave a frond sail, eat, or rest until dusk or dawn. Build the raft in five
// stages at the stakes on the south-west beach, launch it, and sail past the current
// to the lighthouse. Energy drains as you move; running out means collapsing and
// waking up the next morning.
//
// Modes (scene variable "mode"): title -> island -> launch -> raft -> ended. The
// player script moves the castaway and the raft; the HUD script draws what this
// script decides. The island's layout and the recipe tables live in island.rhai.
import "island-lib" as island;

const REACH = 2.6;
const PALM_STOCK = 10.0;
const PALM_REGROW = 25.0;
const WOOD_EVERY = 30.0;
const WOOD_ON_BEACH = 3;
const DROP_CHANCE = 0.35;
const EAT = 30.0;
const ARRIVE = 75.0;

fn toast(text) {
    set_scene_variable("toast", text);
    set_scene_variable("toast_time", elapsed_time());
}

fn pickup(text) {
    set_scene_variable("pickup", text);
    set_scene_variable("pickup_time", elapsed_time());
}

fn add(item, amount) {
    set_scene_variable(item, get_scene_variable(item) + amount);
}

fn park(id) {
    set_position(id, [0.0, -500.0, 0.0]);
}

fn open_camp(open) {
    set_scene_variable("camp_open", open);
    set_ui_visible("ui-camp", open);
    clear_ui_focus();
}

fn start_fade(kind) {
    set_scene_variable("fade_kind", kind);
    set_scene_variable("fade_start", elapsed_time());
    set_object_variable("fade_applied", false);
}

// How squarely the eye looks at a point within reach: the cosine, or -1.
fn aim(eye, forward, target, reach) {
    let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let distance = sqrt(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
    if distance > reach || distance < 0.01 {
        return -1.0;
    }
    (d[0] * forward[0] + d[1] * forward[1] + d[2] * forward[2]) / distance
}

fn on_land() {
    let p = get_position("player");
    island::ground(p[0], p[2]) > 0.0
}

// Jump the clock forward to the next `hour`. The world keeps turning while you
// sleep, so the ship sails on by the same amount of real time.
fn skip_to(hour) {
    let now = get_scene_variable("clock") + get_scene_variable("clock_fraction");
    let skip = hour * 3600.0 - now;
    if skip <= 0.0 {
        skip += 86400.0;
    }
    let next = now + skip;
    if next >= 86400.0 {
        next -= 86400.0;
        set_scene_variable("day", get_scene_variable("day") + 1.0);
    }
    set_scene_variable("clock", floor(next));
    set_scene_variable("clock_fraction", next - floor(next));
    set_scene_variable("time_of_day", next / 3600.0);
    set_scene_variable("world_time", get_scene_variable("world_time") + skip / island::time_scale());
}

fn apply_fade(kind) {
    if kind == "rest" {
        let hours = get_scene_variable("time_of_day");
        let dusk = hours >= 6.5 && hours < 18.5;
        skip_to(if dusk { 18.5 } else { 6.5 });
        set_scene_variable("energy", min(100.0, get_scene_variable("energy") + 50.0)); // resting restores half
        toast(if dusk { "You rest in the shade until dusk." } else { "You sleep under the stars until dawn." });
    } else if kind == "collapse" {
        skip_to(7.0);
        let shade = island::shade();
        set_object_variable("player", "px", shade[0]);
        set_object_variable("player", "py", island::ground(shade[0], shade[1]));
        set_object_variable("player", "pz", shade[1]);
        for name in ["vx", "vy", "vz"] {
            set_object_variable("player", name, 0.0);
        }
        set_scene_variable("energy", 45.0);
        toast("You collapsed from exhaustion and woke up in the shade of the palm.");
    }
}

// Show the raft's built stages and a translucent preview of the next one.
fn show_stage(stage) {
    for i in 1..6 {
        let built = i.to_float() <= stage;
        let next = i.to_float() == stage + 1.0;
        if built { set_position(`raft-stage-${i}`, [0.0, 0.0, 0.0]); } else { park(`raft-stage-${i}`); }
        if next { set_position(`raft-ghost-${i}`, [0.0, 0.0, 0.0]); } else { park(`raft-ghost-${i}`); }
    }
}

fn craft(item) {
    let cost = island::recipes()[item];
    if island::affordable(cost) {
        island::pay(cost);
        add(item, 1.0);
        pickup(`+1 ${island::names()[item]}`);
    }
}

fn time_text(seconds) {
    let m = floor(seconds / 60.0).to_int();
    let s = floor(seconds % 60.0).to_int();
    if s < 10 { `${m}:0${s}` } else { `${m}:${s}` }
}

fn on_update(me, dt) {
    let now = elapsed_time();
    let mode = get_scene_variable("mode");
    let camp = get_scene_variable("camp_open");
    let menu = get_scene_variable("menu_open");

    for event in ui_events() {
        if event.kind != "activate" {
            continue;
        }
        switch event.target {
            "ui-start" => if mode == "title" {
                mode = "island";
                set_scene_variable("mode", mode);
                set_scene_variable("start_time", now);
                set_ui_visible("ui-title", false);
                set_object_variable("tutorial", 1.0);
                set_object_variable("tutorial_time", now);
                toast("You wake up on a tiny island. The ship is already steaming away...");
            },
            "ui-title-quit" => quit_game(),
            "ui-end-quit" => quit_game(),
            "ui-again" => restart_scene(),
            "ui-camp-button" => if mode == "island" { open_camp(true); camp = true; },
            "ui-camp-close" => { open_camp(false); camp = false; },
            "ui-craft-rope" => craft("rope"),
            "ui-craft-paddle" => craft("paddle"),
            "ui-craft-sail" => craft("sail"),
            "ui-eat" => if get_scene_variable("coconuts") >= 1.0 {
                add("coconuts", -1.0);
                set_scene_variable("energy", min(100.0, get_scene_variable("energy") + EAT));
                pickup(`Crunchy. +${EAT.to_int()} energy`);
            },
            "ui-rest" => if on_land() && get_scene_variable("fade_kind") == "" {
                open_camp(false);
                camp = false;
                start_fade("rest");
            }
        }
    }

    // Fades for resting and collapsing: out, the clock jumps, back in.
    let fade = get_scene_variable("fade_kind");
    if fade != "" {
        let f = now - get_scene_variable("fade_start");
        let opacity = if f < 1.0 { f } else if f < 2.0 { 1.0 } else { 1.0 - (f - 2.0) / 1.5 };
        set_ui_visible("ui-fade", true);
        set_ui_opacity("ui-fade", clamp(opacity, 0.0, 1.0));
        if f >= 1.0 && !get_object_variable("fade_applied") {
            set_object_variable("fade_applied", true);
            apply_fade(fade);
        }
        if f >= 3.5 {
            set_scene_variable("fade_kind", "");
            set_ui_visible("ui-fade", false);
            fade = "";
        }
    }

    // Who has the controls, and whether the pointer is captured for looking around.
    let control = (mode == "island" || mode == "raft") && !menu && !camp && fade == "";
    set_scene_variable("control", control);
    let capture = control || mode == "launch";
    if capture != get_object_variable("cursor") {
        set_object_variable("cursor", capture);
        if capture { lock_cursor(); } else { unlock_cursor(); }
    }
    if mode == "title" || mode == "ended" {
        set_scene_variable("prompt", "");
        return;
    }

    // A short tutorial after the start.
    let step = get_object_variable("tutorial");
    if step >= 1.0 && step < 3.0 && now - get_object_variable("tutorial_time") > 6.5 {
        step += 1.0;
        set_object_variable("tutorial", step);
        set_object_variable("tutorial_time", now);
        if step == 2.0 {
            toast("The current is too strong to swim. Build a raft at the stakes on the south-west beach.");
        } else {
            toast("Look at driftwood or the palm and press E to gather. Press C to open your camp.");
        }
    }

    // The island restocks: driftwood washes up, the palm grows new fronds.
    let wood_on = get_object_list("wood_on");
    if mode == "island" && now > get_object_variable("wood_next") {
        set_object_variable("wood_next", now + WOOD_EVERY);
        let active = wood_on.filter(|on| on).len();
        let free = wood_on.index_of(false);
        if active < WOOD_ON_BEACH && free >= 0 {
            let beach = island::beach();
            let me_at = get_position("player");
            let site = island::site();
            for attempt in 0..10 {
                let spot = beach[floor(random(0.0, beach.len().to_float() - 0.001)).to_int()];
                let clear = (spot[0] - me_at[0]) * (spot[0] - me_at[0]) + (spot[1] - me_at[2]) * (spot[1] - me_at[2]) > 25.0;
                for i in 0..wood_on.len() {
                    if wood_on[i] {
                        let p = get_position(`wood-${i}`);
                        if (spot[0] - p[0]) * (spot[0] - p[0]) + (spot[1] - p[2]) * (spot[1] - p[2]) < 16.0 {
                            clear = false;
                        }
                    }
                }
                if clear {
                    set_position(`wood-${free}`, [spot[0], island::surface(spot[0], spot[1]), spot[1]]);
                    set_rotation(`wood-${free}`, [0.0, random(0.0, 360.0), 0.0]);
                    wood_on[free] = true;
                    set_object_list("wood_on", wood_on);
                    toast("A piece of driftwood washed up on the beach.");
                    break;
                }
            }
        }
    }
    let stock = get_object_variable("palm_stock");
    if stock < PALM_STOCK && now > get_object_variable("palm_next") {
        stock += 1.0;
        set_object_variable("palm_stock", stock);
        set_object_variable("palm_next", now + PALM_REGROW);
    }

    let eye = get_position("camera");
    let forward = forward_vector("camera");

    // The lighthouse: spotted by looking at it for a moment, or by its light at night.
    if !get_scene_variable("lighthouse_seen") {
        let beacon = island::beacon();
        let night = get_scene_variable("daylight") < 0.05;
        let look = aim(eye, forward, beacon, 100000.0) > 0.996;
        let looked = if look { get_object_variable("look_time") + dt } else { 0.0 };
        set_object_variable("look_time", looked);
        if looked > 0.8 || night {
            set_scene_variable("lighthouse_seen", true);
            toast(if night {
                "A light is flashing far out to the south-west... a lighthouse!"
            } else {
                "A lighthouse! Far out to the south-west. That's where to sail."
            });
        }
    }

    // Energy: walking, sprinting and swimming tire you; coconuts and rest restore it.
    if mode == "island" && fade == "" && !menu {
        let vx = get_object_variable("player", "vx");
        let vz = get_object_variable("player", "vz");
        let speed = sqrt(vx * vx + vz * vz);
        let p = get_position("player");
        let swimming = island::ground(p[0], p[2]) < -island::swim_depth();
        let rate = 0.06;
        if speed > 4.5 {
            rate += 0.25;
        } else if speed > 0.5 {
            rate += 0.07;
        }
        if swimming {
            rate += 0.25;
        }
        let energy = max(0.0, get_scene_variable("energy") - rate * dt);
        set_scene_variable("energy", energy);
        if energy < 25.0 && !get_object_variable("warned") {
            set_object_variable("warned", true);
            toast("You're exhausted. Eat a coconut or rest at camp (C).");
        } else if energy > 40.0 {
            set_object_variable("warned", false);
        }
        if energy <= 0.0 {
            open_camp(false);
            start_fade("collapse");
        }
    }

    // Sailing: arriving at the lighthouse ends the game.
    if mode == "raft" {
        let light = island::lighthouse();
        let dx = get_object_variable("player", "raft_x") - light[0];
        let dz = get_object_variable("player", "raft_z") - light[1];
        if sqrt(dx * dx + dz * dz) < ARRIVE {
            set_scene_variable("mode", "ended");
            set_scene_variable("control", false);
            set_ui_visible("ui-end", true);
            let days = get_scene_variable("day").to_int();
            let played = time_text(now - get_scene_variable("start_time"));
            let gathered = get_object_variable("gathered").to_int();
            set_ui_text("ui-end-stats", `Days on the island: ${days}` + "\n" + `Time played: ${played}` + "\n"
                + `Things gathered: ${gathered}`);
        }
        set_scene_variable("prompt", "");
        return;
    }
    if mode != "island" || !control {
        set_scene_variable("prompt", "");
        return;
    }

    // What the castaway is looking at, and what E would do with it.
    let best = "";
    let best_aim = 0.0;
    for i in 0..wood_on.len() {
        if wood_on[i] {
            let p = get_position(`wood-${i}`);
            let a = aim(eye, forward, [p[0], p[1] + 0.25, p[2]], REACH);
            if a > 0.82 && a > best_aim {
                best = `wood-${i}`;
                best_aim = a;
            }
        }
    }
    let nut_on = get_object_list("nut_on");
    for i in 0..nut_on.len() {
        if nut_on[i] {
            let p = get_position(`nut-${i}`);
            let a = aim(eye, forward, [p[0], p[1] + 0.2, p[2]], REACH);
            if a > 0.86 && a > best_aim {
                best = `nut-${i}`;
                best_aim = a;
            }
        }
    }
    let palm = island::palm();
    let a = aim(eye, forward, [palm[0], eye[1], palm[1]], 2.2);
    if a > 0.75 && a > best_aim {
        best = "palm";
        best_aim = a;
    }
    let site = island::site();
    let a = aim(eye, forward, [site[0], site[1] + 0.4, site[2]], 4.6);
    if a > 0.45 && a > best_aim {
        best = "raft";
        best_aim = a;
    }

    let stage = get_scene_variable("raft_stage");
    let stages = island::stages();
    let prompt = if best.starts_with("wood") {
        "E   Pick up the driftwood"
    } else if best.starts_with("nut") {
        "E   Pick up the coconut"
    } else if best == "palm" {
        if stock >= 1.0 { "E   Pull palm fronds" } else { "No fronds within reach. Give the palm a little time." }
    } else if best == "raft" {
        if stage < 5.0 {
            let next = stages[stage.to_int()];
            if island::affordable(next.cost) {
                `E   ${next.name}  ·  ${island::cost_text(next.cost)}`
            } else {
                `${next.name}: needs ${island::missing_text(next.cost)}`
            }
        } else if now - get_scene_variable("launch_armed") < 4.0 {
            "E   Launch! There's no coming back."
        } else {
            "E   Launch the raft"
        }
    } else {
        ""
    };
    set_scene_variable("prompt", prompt);

    if best == "" || !input_pressed("interact") {
        return;
    }
    if best.starts_with("wood") {
        let i = best.sub_string(5).parse_int();
        wood_on[i] = false;
        set_object_list("wood_on", wood_on);
        park(best);
        add("wood", 1.0);
        set_object_variable("gathered", get_object_variable("gathered") + 1.0);
        pickup("+1 driftwood");
    } else if best.starts_with("nut") {
        let i = best.sub_string(4).parse_int();
        nut_on[i] = false;
        set_object_list("nut_on", nut_on);
        park(best);
        add("coconuts", 1.0);
        set_object_variable("gathered", get_object_variable("gathered") + 1.0);
        pickup("+1 coconut");
    } else if best == "palm" {
        if stock < 1.0 {
            return;
        }
        set_object_variable("palm_stock", stock - 1.0);
        if stock >= PALM_STOCK {
            set_object_variable("palm_next", now + PALM_REGROW);
        }
        add("fronds", 2.0);
        set_object_variable("gathered", get_object_variable("gathered") + 2.0);
        set_scene_variable("shake_time", get_scene_variable("world_time"));
        let free = nut_on.index_of(false);
        if free >= 0 && random(0.0, 1.0) < DROP_CHANCE {
            let angle = random(0.0, 6.2832);
            let r = random(1.2, 2.2);
            let x = palm[0] + cos(angle) * r;
            let z = palm[1] + sin(angle) * r;
            set_position(`nut-${free}`, [x, island::surface(x, z), z]);
            nut_on[free] = true;
            set_object_list("nut_on", nut_on);
            pickup("+2 palm fronds  ·  a coconut fell!");
        } else {
            pickup("+2 palm fronds");
        }
    } else if best == "raft" {
        if stage < 5.0 {
            let next = stages[stage.to_int()];
            if island::affordable(next.cost) {
                island::pay(next.cost);
                stage += 1.0;
                set_scene_variable("raft_stage", stage);
                show_stage(stage);
                if stage < 5.0 {
                    toast(`${next.done}. (${stage.to_int()}/5)`);
                } else if get_scene_variable("lighthouse_seen") {
                    toast("The raft is ready! Launch it and sail for the lighthouse.");
                } else {
                    toast("The raft is ready! But where to? Look south-west, or wait for nightfall.");
                }
            } else {
                toast(`${next.name}: you need ${island::missing_text(next.cost)}.`);
            }
        } else if now - get_scene_variable("launch_armed") < 4.0 {
            // All aboard: the raft slides down the beach and the castaway stands on it.
            set_scene_variable("mode", "launch");
            set_scene_variable("launch_start", now);
            set_object_variable("player", "aboard", true);
            set_object_variable("player", "raft_yaw", site[3]);
            set_object_variable("player", "yaw", site[3]);
            set_object_variable("player", "pitch", 0.0);
            park("a-raft-site");
            toast(if get_scene_variable("lighthouse_seen") {
                "Out past the current! Steer for the lighthouse: W to sail, A and D to steer."
            } else {
                "Out past the current! W to sail, A and D to steer. Look for a light."
            });
        } else {
            set_scene_variable("launch_armed", now);
        }
    }
}
