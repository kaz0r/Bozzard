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

3. In the editor, Import a `.rs` file to add it to the catalog (or write the catalog entry by hand).
   The catalog keeps paths relative to the scene file, so a project folder stays portable and the
   export packs scripts the same way it packs prefabs.

The example scene `examples/demo/scenes/script-lab.json` shows four scripts (`scenes/scripts/*.rs`)
next to a blueprint graph that reads the same scene variable they write.

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
`input_pressed("jump")` inside `on_update` answers it directly. Top-level statements run once when
Play starts; Rhai functions cannot read top-level `let`/`const` values, so tuning values live inside
the hook that uses them and anything that must outlive a tick belongs on a blackboard variable.

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
| `overlap_count(target)` | number of overlapping objects |
| `delta_time()`, `elapsed_time()` | seconds |
| `input_held(key)`, `input_pressed(key)` | `bool` — any name the Input Held node accepts (`"jump"`, `"fire"`, `"interact"`, `"w"`, …) |
| `move_x()`, `move_y()`, `mouse_x()`, `mouse_y()` | the same frame deltas the movement nodes report |
| `get_object_variable(name)`, `get_scene_variable(name)` | the declared variable's value |
| `raycast(origin, direction, distance, ignore)` | `#{ hit, object, position, normal, distance }` |
| `sphere_overlap(center, radius, ignore)`, `box_overlap(center, size, ignore)` | array of object IDs |
| `line_of_sight(from, to, ignore)` | `bool` |

**Writes** (queued, then applied in call order)

| Function | Effect |
| --- | --- |
| `set_position`, `set_rotation`, `set_scale`, `translate`, `rotate` | transform writes; `rotate` takes a degrees delta |
| `set_velocity(target, v)`, `jump(target, speed)`, `move_with_collision(target, v)` | rigidbody actions; grounding is readable as `is_grounded(target)` |
| `set_color(target, rgb)`, `set_visible(target, visible)`, `set_text(target, text)` | drawable, visibility and text |
| `set_light_intensity(target, intensity)` | light |
| `set_focus_distance`, `set_aperture`, `set_fog_density`, `set_fog_light_intensity`, `set_exposure`, `set_bloom_intensity`, `set_saturation`, `set_heat_strength`, `set_grain_intensity`, `set_vignette_intensity` | display overrides |
| `spawn_prefab(asset, position)`, `destroy_prefab(target)` | returns a spawn handle |
| `set_graph_enabled(target, index, enabled)`, `set_script_enabled(target, index, enabled)` | enable/disable another attachment |
| `lock_cursor()`, `unlock_cursor()` | pointer capture |
| `end_game(message)` | requires Game Flow in scene settings |
| `load_scene(name)`, `add_scene(name)`, `restart_scene()`, `save_game(slot)`, `load_game(slot)` | runtime scene control |
| `set_object_variable(name, value)`, `set_scene_variable(name, value)` | blackboards, type-checked against the declaration |
| `print(value)` | one line to stdout and the runtime's message list |

**Math Rhai does not provide**: `lerp`, `lerp_vector`, `clamp`, `length`, `normalize`, `dot`,
`cross`, `distance`, `add_vector`, `scale_vector`, `vector_x/y/z`, `modulo`, `pow`, `atan2`,
`ceil`, `to_radians`, `to_degrees`, `random(min, max)` (seeded per attachment, like the Random node).

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
- **Spawn handles** returned by `spawn_prefab` are stable IDs that resolve to the created object for
  the rest of the run (the same object the Spawn Prefab node's `Instance` pin addresses).
- **Variables are shared with graphs.** `get/set_object_variable` use the same object blackboard a
  graph declares, and `get/set_scene_variable` the scene blackboard — this is how a script and a
  graph hand state to one another. Scripts have no *graph* scope: they are not attachments of a
  graph, and Rhai arrays cover script-local growth, so blackboard lists stay graph-only.
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
