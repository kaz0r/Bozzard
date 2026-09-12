# Emberwatch: dark bonfire / Spawn & Destroy

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/bonfire-lab.json
# Or run directly:
cargo run -p bozzard-player -- --scene examples/demo/scenes/bonfire-lab.json
```

Press **Play** in the editor. A central cube-built bonfire lights a dark forest clearing, stone hearth, charred logs and two benches. The fire has flickering orange light, real point-light shadows, glowing flame cubes, gentle flame-height animation, anamorphic bloom, filmic color grading, contact occlusion, rising heat shimmer, subtle grain/vignette faint blue distance fog, and wind-driven volumetric firelight. The surrounding geometry is built from cubes too.

## Watch objects spawn and disappear

- The **BONFIRE / Space toggles ember spawning** object's Blueprint spawns one `bonfire-ember` prefab every **0.18 seconds**, at varying positions inside the fire.
- Every ember is a real independent cube entity, not a visibility toggle. Its own Blueprint counts its age, moves it upward, shrinks it and calls **Destroy Prefab → Self** after **2.8 seconds**. The existing Spin component supplies tumbling.
- Click the running viewport and press **Space** to stop emission. All remaining embers disappear within 2.8 seconds. Press Space again to resume. Around **15–16** embers are alive at steady state; they do not accumulate indefinitely.
- **Stop** restores the authored scene without runtime embers. Play starts a fresh simulation.

## Inspect the graphs

Select **BONFIRE**, then open its Blueprint to see the timer, **Spawn Prefab**, spawn position and Space toggle. Select **Firelight** to inspect its separate flicker graph.

The destruction graph is embedded in `assets/bonfire/ember.prefab.json`. To inspect it in the editor, add **bonfire-ember** from the Content Browser's **Prefabs** folder, select the placed ember and open its Blueprint. Undo that temporary placement before running the stock scene. A placed ember also destroys its own instance during Play.

Portable graph copies are in the Content Browser's **Blueprints** folder:

- `bonfire-spawn.blueprint.json`
- `ember-lifetime.blueprint.json`
- `bonfire-flicker.blueprint.json`

The lifetime graph belongs on a **prefab member**: Destroy Prefab intentionally refuses ordinary scene objects. Saved graph files are independent copies, not live links to the scene or prefab.

## Tuning

Change the spawning graph's `interval`, the prefab graph's `lifetime`, or the light graph's `brightness` variable. Keep intervals positive and lifetimes positive; very small intervals or long lifetimes mean more entities and more CPU work. Reposition the unparented BONFIRE root to move its fire and spawn origin together.

Flame/ember geometry uses two tiny, self-contained **unit-cube glTFs** for the renderer's existing emissive material support; no engine features, downloaded models, particle system or new dependencies are needed. The actual illumination comes from the bonfire's point light, not from bloom or a static GI bake. The lighting keeps sun/ambient contribution very low rather than making unlit surroundings uniformly gray.

Regression: `cargo test -p bozzard-editor --test bonfire` checks real ECS creation/removal, independent lifetimes, bounded population, flicker, pause/drain/resume and Stop/Play restoration.

Explicit soak checks (no window):

```sh
cargo test -p bozzard-editor --test bonfire bonfire_soak -- --ignored --nocapture
cargo test -p bozzard-player bonfire_gpu_soak -- --ignored --nocapture
```

The CPU check reports tick cost and, on Linux, resident memory over 40 simulated seconds. The hardware GPU check samples one minute of spawning/destruction and asserts bounded draw counts, buffers, textures and bind groups, then verifies that stopping emission removes all ember draws. It uses low-resolution offscreen captures, not a desktop-sized stress test.

Isolated root transform edits validate only that object's matrix; they cannot affect another object's composition. Parent/child edits still validate the full hierarchy and roll back invalid changes. This avoids rebuilding every scene transform for every ember's movement and shrink action. The debug-build CPU soak measured approximately **14 ms → 2 ms per tick** on the development machine; timings depend on hardware/build and do not measure GPU frame time.

The scene also showcases the [post-processing stack](post-processing.md). Scene Settings exposes each effect and named presets; Play animates shimmer, grain, and the [volumetric haze](volumetrics.md). The authored exposure and bloom threshold remain tuned for this dark clearing.

The [camera effects](camera-effects.md) keep the fire near the focal plane while nearby stones and distant trees soften. Eye adaptation is limited to −0.5…0.4 EV with a dark metering target, preserving the clearing's mood. Tune these under **Camera focus & bokeh** and **Auto exposure** in Post Processing.
