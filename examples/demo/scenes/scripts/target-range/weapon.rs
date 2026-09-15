// The weapon table: stand at a bay and press E to equip the gun racked there.
//
// Speed, recoil and bullet size are object variables, so the shoot script fires with exactly the
// numbers the HUD names. A bay is a stretch of the table's right-hand side, one per weapon.

fn equip(hud, name, speed, kick, size) {
    set_object_variable("speed", speed);
    set_object_variable("kick_amount", kick);
    set_object_variable("size", size);
    set_text(hud, "WEAPON: " + name);
}

fn on_start(me) {
    set_text(get_object_variable("hud"), "WEAPON: AR");
}

fn on_update(me, dt) {
    const BAY_MIN_X = 6.0;
    const BAY_MAX_X = 6.9;
    const AR_MIN_Z = 4.3;
    const AR_MAX_Z = 5.7;
    const PISTOL_MIN_Z = 2.3;
    const PISTOL_MAX_Z = 3.7;
    const SHOTGUN_MIN_Z = 0.3;
    const SHOTGUN_MAX_Z = 1.7;

    // E only picks up what the player is standing at.
    if !input_pressed("interact") {
        return;
    }
    let at = get_position(me);
    if at[0] < BAY_MIN_X || at[0] > BAY_MAX_X {
        return;
    }
    let hud = get_object_variable("hud");
    if at[2] > AR_MIN_Z && at[2] < AR_MAX_Z {
        equip(hud, "AR", 45.0, 0.8, 0.16);
    } else if at[2] > PISTOL_MIN_Z && at[2] < PISTOL_MAX_Z {
        equip(hud, "PISTOL", 22.0, 1.6, 0.22);
    } else if at[2] > SHOTGUN_MIN_Z && at[2] < SHOTGUN_MAX_Z {
        equip(hud, "SHOTGUN", 60.0, 3.4, 0.34);
    }
}
