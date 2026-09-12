# Branch effects completion audit

Completed: curling smoke; wind-driven ash, sparks and trails; temporal anti-aliasing; camera/object motion blur; screen-space reflections; accessible editor controls for all branch effects.

- [x] Smoke: light-responsive, curling, soft intersections, bounded simulation, presets and live authoring preview.
- [x] Ash/sparks: wind, size/rotation variation, luminous velocity trails, presets and inspector controls.
- [x] TAA: eight subpixel samples, camera/object reprojection, depth/normal and reactive rejection, history resets.
- [x] Motion blur: camera/object velocities, shutter and radius bounds, silhouette spreading, static foreground protection, pause/cut handling.
- [x] SSR: actual material normals, roughness/Fresnel response, depth tracing, edge/miss fallback, wet-surface controls and showcase.
- [x] Editor: Effects overview, direct toggles, presets/quality, primary and advanced controls, isolated live preview/pause/comparison, focus selection, wet materials, particle creation, effect volumes, save/undo/Play isolation.
- [x] Combined visual/performance checks, workspace tests, Clippy, formatting, diff whitespace and headless dependency check.

## Evidence

Validation ran on native Metal, Apple M2 Pro, in a debug build.

- Full workspace suite: **269 passed, 0 failed, 3 ignored**. After the motion silhouette refinement, the four temporal GPU tests passed, including one additional test, bringing covered passing tests to **270**. The ignored tests are existing explicit soak tests.
- Native particle test: lighting response, full opaque occlusion, soft intersections, velocity trails, raw bypass, ordering, empty/resize/depth rebinding, statistics and global upload budget.
- Native temporal tests: subpixel accumulation, departed-object rejection, paused image stability, rewind/reset, camera and object velocity, spawn identity, solid silhouettes spreading, static foreground occlusion, material reflection hits, roughness/miss fallback and resize.
- CPU/editor tests: deterministic bounded emission/drain, settings validation, serialization/volume blending, shared 2D isolation, preview pause and stable runtime identity, authored-scene preservation, particle creation/undo, wet materials and volume save/undo.
- Native editor smoke passed the existing editing, gizmo, light/GI, prefab, Blueprint, runtime isolation, save/open/import and viewport pixel checks. Its captured UI shows the new Effects panel.
- Native player smoke passed all existing GPU checks, scene save/reload, and combined Bonfire and Atmosphere Lab renders. The final animated Atmosphere Lab frame contains **61 particles**. Particle removal changes **8,257 pixels**; reflection removal changes **848 pixels** (RGB threshold >3 at 800×500).
- Short 800×500 Atmosphere Lab benchmark, 12 synchronized frames per configuration: optimized CPU median **25.704 ms**, CPU+GPU+wait median **33.981 ms**; reference/culling/cached configurations had identical pixels. This is a static-view debug-build sample, not a windowed FPS or GPU-timestamp measurement. It preceded the final velocity-tile silhouette refinement; that refinement was validated separately with the temporal GPU tests.
- Clippy with `-D warnings`, formatting and the headless dependency check passed. No dependencies were added.

## Integration with main

After merging main at `1d89d36` (text rendering and Rapier physics), the full workspace suite passed: **296 passed, 0 failed, 4 ignored**. The hardware text test was then run explicitly and passed atlas growth, opacity, depth, bounded edits, and cleanup. A new temporal regression test verifies that paused text/content and opacity edits refresh the image without stale history.

Clippy with warnings denied, formatting, the headless dependency check, native editor smoke, and the Atmosphere Lab player smoke all passed on Metal. The combined animated frame contains 61 particles; removing particles changes 8,256 pixels and removing reflections changes 848 pixels. These checks use `work/pr-editor` and `work/pr-atmosphere`.

## Reproduce and inspect

```sh
cargo test --workspace --offline --locked
cargo clippy --workspace --all-targets --offline --locked -- -D warnings
python3 tools/check_headless.py
cargo run -p bozzard-player --offline --locked -- --smoke --backend metal --scene examples/demo/scenes/atmosphere-lab.json --output work/effects-atmosphere-final
cargo run -p bozzard-editor-app --offline --locked -- --smoke work/effects-editor --backend metal
cargo run -p bozzard-editor-app --offline --locked -- --scene examples/demo/scenes/atmosphere-lab.json --backend metal
```

Local validation captures (ignored by Git): `work/effects-atmosphere-final/loaded-3d-animated.ppm`, `loaded-3d-without-particles.ppm`, `loaded-3d-without-reflections.ppm`; `work/effects-editor/editor.png`; `work/temporal-gpu/` for isolated reflection, TAA and motion images.

See [atmosphere effects](atmosphere-effects.md) for authoring, renderer behavior and the screen-space reflection limitations.
