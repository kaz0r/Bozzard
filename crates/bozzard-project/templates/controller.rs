// Rhai gameplay script. Shared state lives in the scene blackboard and survives saves.
fn on_start(me) { unlock_cursor(); }

fn on_update(me, dt) {
    let p = get_position(me);
    p[0] = max(-5.0, min(5.0, p[0] + move_x() * dt * 5.0));
    p[1] = max(-5.0, min(5.0, p[1] + move_y() * dt * 5.0));
    set_position(me, p);
    let coin = get_position("coin");
    let dx = coin[0] - p[0];
    let dy = coin[1] - p[1];
    if dx * dx + dy * dy < 0.36 {
        let score = get_scene_variable("score") + 1.0;
        set_scene_variable("score", score);
        set_text("score", "Score: " + score.to_int());
        set_position("coin", [sin(score * 2.1) * 4.0, cos(score * 1.3) * 4.0, 0.0]);
    }
}
