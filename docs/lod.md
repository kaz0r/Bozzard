# Distance LOD

Add **LOD** to a Mesh Renderer in the Inspector. Add/remove levels, set each **From distance**, and choose Cube, Quad, an imported mesh from the scene catalog, or Cull. Changes use normal editor validation and Undo/Redo. Removing Mesh Renderer also removes LOD. Empty levels leave the base mesh unchanged.

```json
"lod": {
  "levels": [
    { "switch": 40, "mesh": { "asset": "low-detail" } },
    { "switch": 100, "mesh": null }
  ],
  "hysteresis": 0.1
}
```

`low-detail` must name an `AssetKind::Mesh` entry in the scene catalog. The normal import pipeline loads it, and dependency validation, asset users, prefab remapping and export dependency traversal include every LOD mesh. Missing or wrong-kind catalog entries fail scene validation; missing source files fail editor loading.

On the first view, the base mesh is used below 40 world units, the replacement from 40 inclusive to 100 exclusive, and no drawable from 100 onward. Up to 32 strictly increasing, positive, finite switch distances are accepted. Distances are measured between composed world-space object and active camera origins, including parent transforms; camera rotation and scale do not change the distance. Both scene layers use this rule. The 3D editor's free inspection camera selects its own LOD, including in Effects preview, without editing the saved scene camera.

**Switch hysteresis** is a fraction from 0 to 0.49. With 0.1, a 40-unit boundary switches to lower detail at 44 units and back to higher detail below 36. The same rule applies to far culling. It prevents repeated switching when the camera hovers near a threshold. Omitted/zero hysteresis preserves exact thresholds. Histories are separate for gameplay and inspection views, are reset by LOD edits, active-camera changes or world replacement, and are never saved.

Selection happens in `SceneInstance::view()`, shared by editor and player, before color/shadow submission. Culling removes only the extracted drawable, not its ECS entity, collider, text, scripts, or children. The object reappears when the camera returns. Capture/save keeps the authored base mesh and levels. Object identities and submission order remain stable; shader graphs and material components still apply. Surface-specific overrides are cleared when switching to a different mesh, then the object material is applied to that replacement.

Replacement meshes must be authored with compatible origins and scale. JSON also accepts the normal `Mesh::Surface` reference. Converting a whole-model object into independent surfaces requires removing its LOD first, rather than silently losing or duplicating its levels. Objects with an Animator skin binding retain the base mesh and palette, including at cull distances.

## Generate simplified meshes

Select a whole imported static mesh, then open **Generate LODs** in Properties.
Choose one to eight levels, the first switch distance, triangle ratio per level,
maximum relative error, and whether open borders stay fixed. **Generate LOD levels**
creates the assets and attaches the levels in one Undo transaction. If the object
already has LOD, the button explicitly replaces its level list. Source geometry and
previously generated files remain available. Save the scene to retain the new links.

Distances double per level; triangle ratios are relative to the original mesh
(for example, 0.5 produces targets of 50%, 25%, 12.5%). Normal/UV discontinuities,
material boundaries and the configured error limit take precedence over triangle
targets. The completion message reports actual triangle counts. Hysteresis starts
at 0.1 and remains editable in the LOD component.

Generation runs on the existing cancellable asset worker. Cancellation, validation
failure, or a changed scene/catalog removes only the newly reserved directory.
Accepted output is under `assets/generated-lod-*/` and participates in normal
asset dependencies, reload, save, prefab packaging and standalone export. Plain
OBJ output retains the Lambert path; material-bearing output is self-contained
glTF retaining supported PBR maps, samplers, UV sets, tangent data and alpha modes.
The normal importer validates the output before publication. The 32 MiB per-source
import limit applies. The source is never overwritten.

The pinned [meshoptimizer](https://github.com/zeux/meshoptimizer) implementation uses
attribute-aware simplification separately for each material. Identical full vertex
records are welded before reduction; opaque surfaces then optimize vertex-cache
order, and every output compacts unused vertices. Shared images are encoded once
per generated file. Skinned models retain the existing authored geometry because
the runtime does not yet support deformation-aware LOD.

## Checks and measurement

```sh
cargo test --locked --offline -p bozzard-scene --test lod
cargo test --locked --offline -p bozzard-editor --test lod -- --nocapture
cargo test --locked --offline -p bozzard-assets --lib
cargo test --locked --offline -p bozzard-editor --test generated_lod -- --nocapture
cargo test --locked --offline -p bozzard-project --test export automatically_simplified
cargo test --release --locked --offline -p bozzard-assets --lib simplification_scale_benchmark -- --ignored --nocapture
```

The scene checks cover threshold equality, parented cameras/objects, cull/reappearance, capture isolation, skinned-object exclusion, invalid distances, missing/wrong-kind dependencies, and asset remapping. The editor check creates a one-part OBJ, loads it through the asset store, uploads it, renders all three distances, checks Undo and a missing source, and compares each frame byte-for-byte with explicitly authored reference geometry.

Measured on NVIDIA RTX 3060/Vulkan at 64×64: **12 → 1 → 0 color triangles** for base cube → one-triangle OBJ → culled. This is a deterministic geometry-work measurement, not an FPS or timing claim. Instancing's separate 1,024-cube timing benchmark runs at **320×320**.

Not implemented here: screen-size LOD, crossfades, independent shadow LOD, or occlusion culling. LOD thresholds are discrete and can visibly pop. Physics and editor picking retain authored base geometry. Replacement assets remain decoded on the CPU; the editor/player [GPU budget](assets.md#gpu-asset-budget) uploads the selected level and can evict unused levels. A missing GPU level restores through the staged upload queue before its frame is drawn.

The generated-LOD native Metal check on Apple M2 Pro reduces a flat grid from
2,048 to 410 triangles with byte-identical pixels at 128×128. The release offline
benchmark uses a curved 131,072-triangle grid: output levels contain
65,536 / 32,768 / 16,384 triangles, with seven-run CPU medians after warmup of
44.986 / 42.476 / 42.842 ms and combined error at most 0.01. This measures mesh
generation only; image encoding, disk writes and GPU upload are separate work.
