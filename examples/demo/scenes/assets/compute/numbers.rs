// Allocate once; each update polls a named ticket instead of blocking or busy-waiting.
fn on_start(me) {
    unlock_cursor();
    if !compute_available() {
        print("Compute is unavailable: skipping the optional numeric GPU demonstration.");
        return;
    }
    compute_create_buffer("numbers", "numbers", "main", "values", 5);
    compute_write(compute_buffer("numbers"), [1.0, 2.0, 3.0, 4.0, 5.0]);
    compute_dispatch_extent("numbers", "main",
        #{ values: compute_buffer("numbers") }, #{ scale: 2.0, add: 1.0 }, [5]);
    // A second dispatch snapshots different parameters and reads the first dispatch's output.
    compute_dispatch_extent("numbers", "main",
        #{ values: compute_buffer("numbers") }, #{ scale: 3.0, add: -2.0 }, [5]);
    compute_readback("answer", compute_buffer("numbers"));
}

fn on_update(me, dt) {
    if !compute_available() { return; }
    if compute_poll("answer") == "complete" {
        let result = compute_take_result("answer");
        // Expected: [7, 13, 19, 25, 31]. The console records the values; the cube turns green.
        print("Compute result: " + result.to_string());
        set_color(me, [0.1, 0.85, 0.35]);
    } else if compute_poll("answer") == "failed" {
        print("Compute failed: " + compute_error("answer"));
        compute_cancel("answer");
        set_color(me, [1.0, 0.1, 0.1]);
    }
}
