# Pagoda Garden

A voxel pagoda in a cherry-blossom garden, generated from the "progada" three.js
prototype: a five-tier pagoda on a stone plaza ringed by lanterns, a koi pond
with a red bridge, torii gates over the path, cherry, pine, maple and willow
trees, flowers, drifting clouds and falling petals. About 142,000 voxels become
roughly 190 color draws.

```sh
cargo run --release --locked -p bozzard-editor-app -- --scene examples/pagoda-garden/scenes/pagoda.json
cargo run --release --locked -p bozzard-player -- --scene examples/pagoda-garden/scenes/pagoda.json
```

In the editor, click **Play**: the camera slowly orbits the pagoda, koi swim and
clouds drift. **Space** or **N** toggles night: the sun dims, stars come out, the
haze darkens and the lanterns brighten and flicker.

## How the scene is built

`tools/gen_pagoda.py` ports the original generator call for call (same LCG seed
1337, palette and shapes), so every tree, rock, flower and koi lands where the
original placed it, then bakes the voxels for the batch renderer:

| Content | Representation |
| --- | --- |
| Terrain, pond, plaza, path, lantern posts, torii, stone lanterns, bridge, rocks, pond lanterns | `garden.gltf`: exposed faces only, one primitive per color |
| Five-tier pagoda | `pagoda.gltf` |
| 47 trees | 16 species/scale/palette variants, one single-color glTF per variant color, instanced at each original position with a 90° turn |
| 600 flowers and petal carpets | Stock cubes with per-instance tint; glowing flowers use two emissive cube assets |
| 11 koi | Scaled stock cubes driven by `scripts/koi.rhai` |
| 7 clouds | Translucent glTFs driven by `scripts/cloud.rhai`; they cast no shadows |
| Petals | Four particle emitters |

Object IDs are sorted in render order, so each mesh's instances are contiguous
and the batch planner keeps source order at its lower bound; a perspective
camera then reuses one hidden-surface plan while it orbits. Lantern, lamp, gold
and water-highlight colors are emissive (`KHR_materials_emissive_strength`).

Regenerate after editing the generator; `--check` verifies the committed files:

```sh
python3 tools/gen_pagoda.py
python3 tools/gen_pagoda.py --check
```

## Differences from the original

The original has three bugs. Fixes are on by default and draw from a separate
random stream, so the shared layout is unchanged; `--faithful` reproduces the
original geometry:

- Cherry trunks were built but never added to the scene, leaving floating
  canopies. Tree variants include their trunks.
- Terrain columns lower than y = −3 were left empty. They get a grass floor.
- The window test could never pass. Each wall gets two-voxel paper windows.

Trees are shared variants rather than 47 unique shapes, the flat background is
Bozzard's procedural sky with matching colors, and the original's linear fog is
approximated by exponential distance fog. Point-light intensities keep the
original day/night ratios at a brightness suited to Bozzard's light units.
