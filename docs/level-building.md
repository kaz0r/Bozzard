# Level building

The terrain/scatter transactions and procedural mesh formats described here are
available to headless tools. View → Level tools
(also the viewport's Build button) opens terrain, blockout, foliage and grid/
measurement controls. Docking and custom inspector registration are documented in
[editor extensions](editor-extensions.md).

Create a terrain or select an existing terrain and choose Edit selected terrain.
Hold the primary mouse button over its surface to sculpt; the brush contours show
the draft, and releasing publishes the render/collision mesh in one Undo step.
Raise/Lower strength is measured per second. Flatten uses the selected local height.
Apply draft retries a failed publication; Discard draft returns to the saved source.

For blockout, choose Box, Ramp, Stairs or Cylinder, set dimensions and yaw, then
place brushes in the viewport. Dragging places stamps at the chosen spacing;
releasing publishes one Undo step. Matching primitives share mesh assets and keep
older instances' custom colliders. Snap/Ctrl uses the viewport's movement increment.
Placement uses the picked surface or the construction plane when no surface is hit.

For foliage, select the prototype in the hierarchy and choose Use selected
prototype, then select the ground and choose Use selected ground. Configure the
scatter and submit it; progress, cancellation and actual placement counts appear
in the tool window. The prototype's root pivot is its planting point.

Grid settings control spacing, extent and construction-plane height. Measure
records two surface/plane clicks and displays distance and axis deltas. These
overlays and preferences do not add objects to the saved scene.

Terrain files end in `.terrain.json` and are mesh assets. Their versioned schema
stores X/Z vertex counts, total width/depth and row-major heights. Each axis has
2–129 vertices; width/depth are 0.01–100,000 local units and heights are bounded to
±10,000. Larger landscapes can use multiple terrain objects. Generated geometry
has smooth normals and UVs, while sampling follows the exact rendered triangles.

`Terrain::brush` supports Raise, Lower, Flatten and Smooth with circular smooth
falloff. It visits only the brush's grid rectangle. Smooth reads an immutable
neighborhood; other modes allocate no scratch height array. Terrain remains an
ordinary mesh for rendering, picking, compression/cooking and export.

`Editor::terrain_job` prepares a new heightfield source, render mesh and exact
triangle collider in a cancellable worker. `accept_terrain` verifies the scene,
catalog and original source before publishing one history step. It preserves
collision filtering on existing terrain colliders. Multiple objects using the
same terrain asset receive the same geometry revision.

Terrain revisions are immutable files. Undo/Redo swaps the catalog and cached
assets; it does not overwrite a source referenced by another saved scene or
prefab. Failed/cancelled preparations remove only their unpublished directory.
Published older revisions remain on disk because saved documents may refer to
them. Ordinary export includes the active dependencies.
Revision allocation retains its next candidate across strokes, avoiding repeated
scans of every older revision directory. Cancellation is checked during collisions.

`Editor::scatter_foliage_job` copies a prototype's complete subtree onto an enabled
ground collider. The prototype root's origin is the planting point. Meshes and
materials remain shared; internal object references and nested prefab links are
remapped for each copy. Seed, circular area, count, spacing, scale range, maximum
slope and normal alignment control placement. Random yaw varies instances.

Scattering is deterministic and cancellable. A spatial grid tests spacing rather
than comparing every new point with every previous point. Placement stops after
a bounded number of attempts and reports actual/requested counts. Limits are
2,000 copies, 128 objects per prototype and 20,000 new objects per operation,
within the scene's existing object limit. `accept_foliage` publishes one Undo step
only if the scene/catalog still match the preparation inputs.

Blockout source files end in `.brush.json`. Box, Ramp, Stairs (1–128 steps) and
Cylinder (3–64 sides) produce closed mesh primitives with outward face normals.
Their unit shape is centered in X/Z and rests at Y=0; object transforms supply
dimensions. Stair meshes omit hidden internal faces. The same source/mesh cooker
and raw-package paths handle terrain and blockout files.

Headless checks:

```sh
cargo test -p bozzard-assets --lib terrain::tests
cargo test -p bozzard-assets --lib blockout::tests
cargo test -p bozzard-assets --test procedural_meshes
cargo test -p bozzard-editor --test terrain --test foliage --test blockout
cargo test -p bozzard-editor-app level_tools::tests
```

Generate a portable workshop with a sculpted hill, scattered trees and all four
blockout shapes (choose a directory that does not exist):

```sh
cargo run -p bozzard-editor --example level_workshop -- /tmp/bozzard-level-workshop
cargo run -p bozzard-editor-app -- --scene /tmp/bozzard-level-workshop/scene.json
```

Native verification covers terrain strokes and revision Undo/Redo, blockout
placement/history, a 12-instance foliage scatter/history, grid and two-point
measurement, custom inspector edits, docking and Play/Stop. The resulting workshop
exports five cooked mesh assets and runs 20 Metal frames with the authoring source
folder unavailable. Terrain/brush prefab tests also verify that sculpting an
instance preserves previously saved prefab geometry through instantiation and
scene save/reopen. Final whole-branch checks are tracked in
[the completion ledger](scale-pipeline-progress.md).
