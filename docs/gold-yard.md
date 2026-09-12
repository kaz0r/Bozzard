# Gold Yard

A 21-object playground on a flat plane. No downloaded models or additional game code.

Open `examples/demo/scenes/gold-yard.json` in the editor and press **Play**, or run:

```sh
cargo run -p bozzard-player -- --scene examples/demo/scenes/gold-yard.json
```

## Play

- **WASD:** roam. **Space:** jump. **Hold right mouse and drag:** look around.
- Walk into the **gold block** ahead of you to collect it. It disappears and the collection counter updates. This is a simple pickup, not a carry/throw inventory.
- Step onto the **blue pad** beyond it to drop a copper block onto the ramp. Step off and back on to drop another. Watch convex mesh bodies tumble into the stacked boxes.
- The two orange bodies on the left demonstrate toppling and different inertia. Walk up the ramp or jump onto the props to inspect them.
- Keep exploring after collecting gold. The **green pad** in the far-left corner is an optional finish: reach it with the gold to win.
- Falling off the plane respawns you. **R** restarts in the player; **Stop / Play** restarts in the editor. Editor Play controls work while the viewport is hovered and no text field has keyboard focus.

The player retains its upright swept-box controller, not a grabber or force-driven character. The pad is the reliable way to disturb the physics props. Static ramp/floor meshes retain triangle collision; dropped blocks use Rigidbody + convex Mesh Collider.

## Inspect the Blueprints

- **Gold → Gold - mouse delta demo:** `Mouse Delta X/Y` → Make Vector → Scale Vector → Rotate. Right-drag turns the gold while the existing controller orbits the camera. The two axes are also available in the Blueprint node menu for your own graphs.
- **Blue pad → Blue pad - drop a physics block:** checks that the entering object is the player, enforces a one-second cooldown, then Spawn Prefab.
- **yard-drop prefab → Drop block - twelve second lifetime:** records its own birth time and calls Destroy Prefab after twelve seconds, removing both the ECS entity and solver body.

Portable copies are in `examples/demo/scenes/assets/Blueprints/yard-*.blueprint.json`; the physics prefab is in `assets/gold-yard/`. Cooldown and expiration keep repeated drops bounded (at most 13 live drops). Stop restores all authored objects and clears progress and runtime physics.

```sh
cargo test -p bozzard-editor --test gold_yard
cargo test -p bozzard-demo --test blueprints mouse_deltas
```
