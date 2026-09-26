# Gameplay Scripts (Rhai)

Blueprints are the engine's no-code authoring path. Scripts are the other half of the same idea:
the **Script Manager** component runs [Rhai](https://rhai.rs) functions that call *exactly the
engine actions a blueprint node calls*. A scene may use either, or both at once — they share the
same object and scene blackboards, so a graph and a script can drive one piece of state together.

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/script-lab.json
cargo run -p bozzard-player -- --scene examples/demo/scenes/script-lab.json
cargo run -p bozzard-server -- --scene examples/demo/scenes/script-lab.json --ticks 180
```

The **[Target Range](blueprints.md#target-range-example) game is ported to scripts** as
`examples/demo/scenes/target-range-rs.json`: the same arena, weapons, recoil, respawn and win, with
the player, the weapon table, the shot, the cubes and the win condition all in
`scenes/scripts/target-range/*.rs` and no graph of its own. `examples/demo/tests/target_range_rs.rs`
runs the Blueprint scene's assertions against it, which is the point of the pair: a game ported from
graphs to scripts keeps its behaviour.

Rhai is a small, Rust-like, dynamically typed language with its own `if`/`while`/`for`, functions,
arrays, maps, strings and math (`sin`, `sqrt`, `min`, `abs`, …). Anything Rhai already provides is
not re-exposed: a script uses `sin(t)` and `a + b`, not an engine node name.

Scripts can also [dispatch WGSL compute shaders](compute.md), display generated textures and
collect asynchronous buffer results. Compute uses dedicated named resource/job state, separate
from blackboards. Its CPU reservations are visible immediately to later compute calls in the
same hook; completion results become visible only at a later simulation boundary.

## Attach a script

1. Put the source next to the scene and add it to the scene's asset catalog:

   ```json
   "assets": {
     "spin": { "kind": "script", "path": "scripts/spin.rs" }
   }
   ```

   Script files use the `.rs` extension by project convention (`.rhai` is also accepted on import).
   They are *not* Rust: nothing compiles them with cargo. Keep them outside any crate's `src/`.
   Like a prefab, a script is gameplay data with nothing to put on the GPU: the viewer loads it,
   skips it in the asset residency pass, and never waits on it before drawing a frame.

2. Select the object, choose **Properties → Add Component → Script Manager**, and pick the script
   asset. **+ Add script** adds another attachment; **↑**/**↓** change the order and **Remove**
   detaches one. Up to 16 scripts per object, run top to bottom.

   **Open source** opens the attached asset in the dockable Script pane. The pane shows line
   numbers, generated hook and engine function help, completion at the cursor, and live hook and
   command counts per attachment during Play. A dot marks unsaved source. **Save source** writes
   the script file; **Apply to Play** requests a separate live reload. If the file changes outside
   the editor, review the external copy before reloading or overwriting it. Switching scripts or
   closing a dirty pane asks whether to save, discard, or keep the draft. Compile errors retain
   the draft and offer a file/line link; the last valid runtime program continues.

3. In the editor, Import a `.rs` file to add it to the catalog (or write the catalog entry by hand).
   The catalog keeps paths relative to the scene file, so a project folder stays portable and the
   export packs scripts the same way it packs prefabs.

The example scene `examples/demo/scenes/script-lab.json` shows four scripts (`scenes/scripts/*.rs`)
next to a blueprint graph that reads the same scene variable they write.

**Flap Woods Together** is also a scripted example. Its player, round and pipe
scripts are in `scenes/scripts/flap-woods-multiplayer/`, attached through Script
Manager. It uses the same Rhai runtime for host rules, prediction/replay and
ordinary presentation hooks. See [the multiplayer scripting contract](multiplayer.md#scenes-scripts-and-export).

## Hooks

A hook is a script function the engine calls. Missing hooks are simply not called; a hook declared
with the wrong number of parameters fails when the scene opens, not on the first tick.

| Hook | When |
| --- | --- |
| `on_enable(me)` | The attachment became enabled (fires before `on_start` on the first tick) |
| `on_start(me)` | The attachment ran for the first time |
| `on_update(me, dt)` | Every simulation tick, with the fixed timestep in seconds |
| `on_object_enter(me, other)` | Another object began overlapping this one |
| `on_object_exit(me, other)` | Another object stopped overlapping this one |
| `on_overlap_enter(me)` | The overlap set became non-empty |
| `on_overlap_exit(me)` | The overlap set became empty again |
| `on_collision_enter(me, other, normal, impulse)` | A solid contact began; static contacts report a zero impulse |
| `on_disable(me)` | The attachment was disabled (the simulation keeps running) |
| `on_destroy(me)` | The owning object is being destroyed, before it is removed |

`me` is the owning object's ID. `On Input Pressed` has no hook: scripts run every tick, so
`input_pressed("jump")` inside `on_update` answers it directly. Top-level statements initialize
each attachment's script-local scope once when Play starts. Hooks can read that scope across ticks;
use blackboard variables for state shared with other attachments or blueprints.

## Live reload and editor integration

An editor can call `Editor::request_script_reload(asset, source)` during Play. This starts a worker
compile and returns a revision. `Editor::script_reload_feedback(asset)` reports `Compiling`,
`Applied`, `Failed` or `Stale`; advancing the editor polls the worker and publishes a valid
candidate between simulation ticks. The source pane owns saving the file: applying source to
running Play does not save it. A compile or hook-signature error names the asset and Rhai line,
and the last valid program keeps running.

A successful replacement preserves the scene, blackboards, queued actions and each attachment's
enabled and started state. It resets script-local top-level scope for every live attachment of the
asset, including attachments spawned while compilation was in progress. It does not call
`on_start` again. Stop, scene replacement, attachment removal and a newer edit invalidate older
results. Active multiplayer Play rejects live replacement; all peers must stop and restart with
the same script revision.

For completion and help, use `bozzard_scene::script_function_descriptions()` and
`bozzard_scene::script_hook_signatures()` instead of a separate handwritten function catalog.
The existing `script_hook_descriptions()` API exposes the same names with argument counts.
For runtime counters, `ScriptRuntime::stats` is the last tick's `ScriptRuntimeStats`: aggregate
hook and command counts plus `attachments`, keyed by `(object_id, Script Manager index)`.
The attachment snapshot keeps at most 4096 entries and sets `truncated` if more ran. These are
runtime counters, not edits to the authored scene.

## Functions

Object arguments are IDs (or `me`). Missing objects, wrong types and out-of-range values are
errors: a thrown script stops the simulation and reports the hook, the object and the Rhai line.

**Reads**

| Function | Result |
| --- | --- |
| `is_valid_object(target)`, `same_object(a, b)` | `bool` |
| `is_rigidbody(target)`, `is_grounded(target)` | `bool` |
| `get_position(target)`, `get_rotation(target)`, `get_scale(target)`, `forward_vector(target)` | `[x, y, z]` |
| `get_text(target)` | `string` |
| `network_active()` | Whether a multiplayer presentation frame is available; false in ordinary solo/editor simulation |
| `network_object(target)` | Read-only map for a locally bound network object; empty when its slot has no player/state |
| `network_state()` | Read-only session presentation map; the current reference provides `players`, including each player's `local` flag |
| `overlap_count(target)` | number of overlapping objects |
| `delta_time()`, `elapsed_time()` | seconds |
| `fresh_seed()` | new integer seed for procedural scenes; varies across runs, so store it if a world must be reproduced |
| `input_held(key)`, `input_pressed(key)` | `bool` — any name the Input Held node accepts (`"jump"`, `"fire"`, `"interact"`, `"w"`, …) |
| `render_stats()` | Runtime map: `available`, `rate_ready`, `fps`, `frame_ms`, `cpu_draw_ms`, `visible_entities`, `draw_calls`, `triangles`, `viewport_aspect`. The aspect ratio is width/height of the last completed viewport (zero before a native frame). Native player and editor Play publish completed renders. FPS/frame time average wall-clock frame intervals over at least 250 ms; CPU draw time covers renderer preparation, encoding and submission, not GPU execution. Entities count frustum-visible world objects before GPU occlusion, excluding HUD. Draws/triangles count submitted color-pass meshes, excluding HUD, shadows, sky and particles. Headless runs return unavailable counters; the rate needs two or more frames. |
| `simulation_stats()` | Last completed native simulation batch: `available`, `threaded`, `cpu_ms`, `wait_ms`, `steps`. CPU time includes every fixed tick in that batch; wait is the main thread's remaining join time after frame submission. Native local Play/player use a dedicated worker by default; `--single-threaded` enables the serial comparison path. Uninstrumented headless stepping returns unavailable values. This is separate from renderer CPU/GPU time. |
| `move_x()`, `move_y()`, `mouse_x()`, `mouse_y()` | the same frame deltas the movement nodes report |
| `get_object_variable(name)`, `get_scene_variable(name)` | the declared variable's value |
| `get_object_list(name)`, `get_scene_list(name)` | a copy of a declared typed blackboard list as a Rhai array |
| `raycast(origin, direction, distance, ignore)` | `#{ hit, object, position, normal, distance }` |
| `sphere_overlap(center, radius, ignore)`, `box_overlap(center, size, ignore)` | array of object IDs |
| `line_of_sight(from, to, ignore)` | `bool` |

**Writes** (queued, then applied in call order)

| Function | Effect |
| --- | --- |
| `set_position`, `set_rotation`, `set_scale`, `translate`, `rotate` | transform writes; `rotate` takes a degrees delta |
| `set_velocity(target, v)`, `jump(target, speed)`, `move_with_collision(target, v)` | rigidbody actions; grounding is readable as `is_grounded(target)` |
| `set_color(target, rgb)`, `set_visible(target, visible)`, `set_text(target, text)` | drawable, visibility and text |
| `set_ui_text(target, text)`, `set_ui_visible(target, visible)` | text (up to 4096 UTF-8 bytes) and visibility of a UI widget |
| `set_ui_enabled(target, enabled)` | enable or disable input for a widget and its descendants (useful during closing animations) |
| `set_ui_opacity(target, opacity)` | widget and descendant opacity multiplier, `0.0`–`1.0` |
| `set_ui_size(target, width, height)` | widget anchor size in canvas units, each `0.0`–`10000.0` |
| `set_ui_background(target, [r, g, b, a])` | widget background color, each channel `0.0`–`1.0` |
| `set_ui_world_position(target, [x, y, z])` | attach a screen UI widget to a world position using its layer's active camera; its pivot and offset position the label around that point |
| `set_ui_screen_position(target, x, y)` | position a popup at normalized viewport coordinates (`0..1`); apply its pivot and canvas offset, then clamp its rectangle inside the viewport |
| `set_ui_offset(target, x, y)` | replace the widget's anchor offset in canvas units, e.g. to animate a panel sliding into view |
| `set_light_intensity(target, intensity)` | light |
| `set_sun_light(rgb, intensity)`, `set_ambient_light(rgb, intensity)` | scene light color (linear RGB 0..1) and intensity (0..100000); preserves sun direction/shadow settings |
| `set_environment(zenith, horizon, ground, intensity)` | live sky/IBL colors (linear RGB 0..1), intensity 0..1000; preserves background visibility and stars |
| `set_star_intensity(intensity)` | background-only stars (0..1000, default 0); perspective direction field / fixed distant field for orthographic cameras |
| `set_focus_distance`, `set_aperture`, `set_fog_density`, `set_fog_light_intensity`, `set_exposure`, `set_bloom_intensity`, `set_saturation`, `set_heat_strength`, `set_grain_intensity`, `set_vignette_intensity` | display overrides |
| `spawn_prefab(asset, position)`, `destroy_prefab(target)` | returns a spawn handle |
| `set_graph_enabled(target, index, enabled)`, `set_script_enabled(target, index, enabled)` | enable/disable another attachment |
| `lock_cursor()`, `unlock_cursor()` | pointer capture |
| `end_game(message)` | requires Game Flow in scene settings |
| `quit_game()` | requests Exit: closes the native player or stops editor Play; also available to script scenes without Game Flow |
| `set_camera_size(target, size)` | sets an orthographic camera's vertical world span; positive finite size, smaller values zoom in |
| `load_scene(name)`, `add_scene(name)`, `restart_scene()`, `save_game(slot)`, `load_game(slot)` | runtime scene control |
| `load_scene_async(name)`, `add_scene_async(name)`, `cancel_scene_load()`, `unload_scene(handle)` | background scene preparation and additive-instance lifetime |
| `scene_loading()`, `scene_load_progress()`, `loaded_scene_handle()`, `scene_load_error()` | latest loading operation: active flag, 0–1 progress, result handle and failure text |
| `set_object_variable(name, value)`, `set_scene_variable(name, value)` | blackboards, type-checked against the declaration |
| `set_object_list(name, values)`, `set_scene_list(name, values)` | replace a declared list with an array, checked against its element type and capacity |
| `print(value)` | one line to stdout and the runtime's message list |

Sun, ambient, environment, and star setters are transient Play overrides. They do not edit the authored scene and reset on scene restart/Stop. Stars require an enabled environment background and do not contribute to surface lighting.

**Math Rhai does not provide**: `lerp`, `lerp_vector`, `clamp`, `length`, `normalize`, `dot`,
`cross`, `distance`, `add_vector`, `scale_vector`, `vector_x/y/z`, `modulo`, `pow`, `atan2`,
`ceil`, `to_radians`, `to_degrees`, `random(min, max)` (seeded per attachment, like the Random node).

UI writes target an object with a `ui_widget` component beneath a `ui_canvas`; they override
runtime state without changing the authored scene. World labels keep their canvas-scaled size,
are projected at the current viewport's aspect ratio, and hide outside the camera's depth range.
For example, a script can call `set_ui_world_position("label", [x, y + 1.0, z])` and vary
`set_ui_opacity("label", alpha)` during `on_update` to fade a nearby object's name. Use
`set_ui_visible("label", false)` when hiding an interactive widget; opacity alone does not disable
its input. See the Earth Factory example for proximity labels and a delivery progress bar.

`ui_events()` returns this tick's ordered widget events as maps with `kind`, `target`, `x`,
`y`, and `delta`. Unconsumed mouse-wheel input over the world emits `scroll` with an empty
target and a delta in logical points (positive down, 40 points per wheel notch). Panels and
widgets block these world scroll events; scrollable widgets handle their own scrolling.
Other event kinds have a zero delta: `down`, `up` (left button), `secondary` (right button), `activate`
(click/keyboard/accessibility), and `cancel` (pointer or focus loss). The target is the hit
interactive widget, or an empty string for a miss. Child labels route to their parent button.
Coordinates are normalized to the game viewport, including inside editor Play.
`ui_pointer()` returns the latest normalized `[x, y]`, or `[-1, -1]` outside the viewport.
Use these with `set_ui_screen_position` for a cursor-following drag preview or context menu.
Events are delivered once per simulation tick to all scripts, are not saved, and are bounded
to 256 entries; overflow emits `cancel` before subsequent events. A cancelled drag should
leave its original stack intact. The Earth Factory inventory implements these behaviors in Rhai.

## Semantics

- **Reads see the tick's state.** Every script reads one snapshot taken at the start of the tick.
- **Writes are queued and applied after every script has run**, in call order across all scripts.
  A queued write is visible to later reads *in the same tick*, so `set_position` followed by
  `get_position` agrees with a blueprint graph, and the ECS is only touched once per tick.
- **`is_grounded` reports the last completed move**, not the one you are about to make: a move
  applies at the end of the step, so a hook cannot read back its own result. Decide gravity and
  jumping from it at the top of `on_update`, as `scenes/scripts/target-range/player.rs` does.
  A resting body still has to move a little way down every tick, again as the Gravity component
  does: a tick whose move is empty touches nothing, so it reports *not* grounded and a press on
  that tick is lost. Cancel the accumulated fall on a resting tick, not the tick's own step.
- **A restart, a scene change or a loaded save rebuilds the world without losing the scripts.**
  Loaded sources and the compiled engine are runtime state that follows the replacement scene, while
  attachment state (started, held keys, overlap sets) starts over, so `on_start` runs again in the
  new world. Retry on the win screen and the runtime `Restart Scene`, `Load Scene` and `Load Game`
  actions all go through it.
- **Scripts step before blueprints** in a tick, so a graph reads a variable a script wrote in the
  same tick. Each step samples one snapshot of the world for its own events, and running scripts
  first also means a graph's `Destroy Prefab` cannot hide a hit the scripts were meant to see.
- **Async scene transitions** prepare validated component state in a worker and publish after
  queued actions finish. `scene_load_error()` reports preparation/publication failures without
  stopping gameplay; cancellation leaves current objects unchanged. At most one worker runs
  per runtime, including while a cancelled worker finishes its current validation call.
  See [scene loading](scene-loading.md) for ownership and retry rules.
- **Spawn handles** returned by `spawn_prefab` are stable IDs that resolve to the created object for
  the rest of the run (the same object the Spawn Prefab node's `Instance` pin addresses).
- **Variables are shared with graphs.** `get/set_object_variable` use the same object blackboard a
  graph declares, and `get/set_scene_variable` the scene blackboard — this is how a script and a
  graph hand state to one another. The corresponding `*_list` functions copy or replace a
  declared bounded list. Scripts have no *graph* scope: they are not attachments of a graph.
  Local Rhai arrays are useful within a hook; use blackboard lists when state must persist between
  hooks or be shared with other attachments and graphs.
- **Failure is loud.** A syntax error or a bad hook signature fails when the scene opens; a runtime
  error stops the simulation with `script hook on_update on 'thing': … (line 2, position 5)`.

## Limits

A prefab a script can spawn has to be in the scene's asset catalog: a graph names it on a node, but
script source is opaque to the loader, so a scene that runs scripts preloads **every** prefab in its
catalog before the first tick.

A prefab member may carry scripts of its own. The loader merges each prefab's catalog into the scene
before it reads sources, so a scripted prefab keeps its hooks when it is placed, spawned from a
graph or spawned by a script — including a script it carries into the scene from the prefab file.

1 MiB per script source, 1024 compiled scripts per scene, 2,000,000 interpreter operations per hook
call (a runaway loop fails the tick instead of hanging), 32 call levels, 1024 results per overlap
query, 16 attachments per object, 64 kept `print` lines, and 32 MiB of script source per scene.

## Debug console

`print(message)` and `log_info(message)` write informational messages to **Debug → Console**. `log_warning(message)` and `log_error(message)` set their corresponding severity. The console retains the object source; runtime failures also include the script asset and attachment. Logging an error does not throw. See [debugging](debugging.md).
