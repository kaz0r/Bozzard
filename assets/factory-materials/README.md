# Mk1 factory materials

All 49 defined item types have their own low-poly model, matching the factory's
graphite housings, machined steel, copper, cyan glass and subdued industrial
colors. Ores have faceted mineral veins; finished materials use recognizable
silhouettes: cast ingots, stacked sheets, nuts and bolts, cable coils, a gear,
I-beams, bricks, wafer, lenses, circuit board, finned motor and machinery module.
Equipment items are miniature miner/generator modules and bundled pole parts.

- `materials-preview.png`: labeled Blender studio sheet of all 49 models.
- `materials-engine.png`: all models through Bozzard's native renderer.
- `materials-belts.png`: native gameplay preview of materials on conveyors.
- `materials-dev-world.png`: complete native Dev World with the new item art.
- `mk1-materials.blend`: editable named parts, a collection per item, studio
  camera, labels and lights. Studio parents enlarge the items for inspection;
  exported mesh coordinates retain their original conveyor scale.
- `manifest.json`: item IDs, model and prefab paths, triangle/surface counts,
  centered game-space bounds and fluid-sample flags. Item ID 19 is unused.
- Game GLBs: `examples/earth-factory/scenes/assets/models/materials/`.
- Individual `item-*.prefab.json` assets: `examples/earth-factory/scenes/assets/`.
- `examples/earth-factory/scenes/materials-showroom.json`: standalone 49-model
  art scene for the editor or player; open with `bozzard-editor --scene PATH`.

Each GLB is self-contained, textureless, static, Y-up and in tile units. The
origin is the center of the actual mesh bounds. Unit-scale silhouettes fit the
0.46-tile conveyor bed, including corner belts. Geometry ranges from 20 to 652
triangles per item (8,668 across the catalog), with one to five material surfaces.

The game replaces generic cubes with these meshes for stationary machine output,
conveyors, junctions and moving transfers. Items retain the existing reusable
entity pool; changing material swaps the mesh without creating a new hierarchy.
Placement uses each model's half-height to rest above the belt treads. Host and
guest displays use the same models and motion heights, including streamed chunks
and saved worlds.

Crude oil, water, fuel, heavy oil, hydrogen and oxygen have drum, canister or
cylinder sample models in the art catalog. Gameplay continues to carry these
fluids inside pipes without visible solid packages. Dev World's isolated belts
show all solid materials; its pipe samples retain the normal fluid behavior.

Regenerate and validate from the repository root:

```sh
blender --background -noaudio --threads 8 --python examples/earth-factory/tools/generate_material_models.py
python3 examples/earth-factory/tools/generate_scene.py
python3 examples/earth-factory/tools/validate_material_models.py
```

Append `-- --no-render` to the Blender command to export GLBs, prefabs, manifest,
showroom and editable source without rendering the studio sheet. Scene generation
registers the full catalog and derives the gameplay height table from the mesh
bounds. The validator checks all 49 IDs, indices, finite positions, unit normals,
geometry budgets, centered pivots, conveyor fit, prefab references and scene
registration.

Because pooled mesh changes use the native `set_mesh` script action, rebuild and
restart the editor/player when updating an older game build.
