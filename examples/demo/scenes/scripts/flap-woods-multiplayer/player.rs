// Flap Woods Together player. The host and local prediction/replay call this
// same code at 60 Hz; on_update only presents the replicated result in the scene.
fn network_input(key) { key == "Space" }

fn network_spawn(slot) {
    #{ slot: slot, x: -5.0 + slot.to_float() * 0.6, y: 0.65,
       velocity: 0.0, alive: true, score: 0, input_ack: 0 }
}

fn network_predict(player, pressed, dt) {
    if !player.alive { return player; }
    if pressed { player.velocity = 6.5; }
    player.velocity -= 22.0 * dt;
    player.y += player.velocity * dt;
    player
}

fn on_update(me, dt) {
    if !network_active() { return; }
    let player = network_object(me);
    if player.len() == 0 {
        set_position(me, [0.0, 100.0, 0.0]);
        return;
    }
    set_position(me, [player.x, player.y, player.slot.to_float() * 0.05]);
    set_rotation(me, [0.0, 0.0, player.velocity * 4.0]);
    let size = if player.alive { 0.8 } else { 0.45 };
    set_scale(me, [size, size, size]);
}
