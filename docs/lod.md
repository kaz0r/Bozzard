# Distance LOD

Add **LOD** to a Mesh Renderer in the Inspector. Add/remove levels, set each **From distance**, and choose Cube, Quad, an imported mesh from the scene catalog, or Cull. Changes use normal editor validation and Undo/Redo. Removing Mesh Renderer also removes LOD. Empty levels leave the base mesh unchanged.

```json
"lod": {
  "levels": [
    { "switch": 40, "mesh": { "asset": "low-detail" } },
    { "switch": 100, "mesh": null }
  ]
}
```

`low-detail` must name an `AssetKind::Mesh` entry in the scene catalog. The normal import pipeline loads it, and dependency validation, asset users, prefab remapping and export dependency traversal include every LOD mesh. Missing or wrong-kind catalog entries fail scene validation; missing source files fail editor loading.

The base mesh is used below 40 world units, the replacement from 40 inclusive to 100 exclusive, and no drawable from 100 onward. Up to 32 strictly increasing, positive, finite switch distances are accepted. Distances are measured between composed world-space object and active scene-camera origins, including parent transforms; camera rotation and scale do not change the distance. Both scene layers use this rule. The editor's free inspection camera does not override the scene camera used to select LOD.

Selection happens in `SceneInstance::view()`, shared by editor and player, before color/shadow submission. Culling removes only the extracted drawable, not its ECS entity, collider, text, scripts, or children. The object reappears when the camera returns. Capture/save keeps the authored base mesh and levels. Object identities and submission order remain stable; shader graphs and material components still apply. Surface-specific overrides are cleared when switching to a different mesh, then the object material is applied to that replacement.

Replacement meshes must be authored with compatible origins and scale. JSON also accepts the normal `Mesh::Surface` reference. Converting a whole-model object into independent surfaces requires removing its LOD first, rather than silently losing or duplicating its levels. Objects with an Animator skin binding retain the base mesh and palette, including at cull distances.

## Checks and measurement

```sh
cargo test --locked --offline -p bozzard-scene --test lod
cargo test --locked --offline -p bozzard-editor --test lod -- --nocapture
```

The scene checks cover threshold equality, parented cameras/objects, cull/reappearance, capture isolation, skinned-object exclusion, invalid distances, missing/wrong-kind dependencies, and asset remapping. The editor check creates a one-part OBJ, loads it through the asset store, uploads it, renders all three distances, checks Undo and a missing source, and compares each frame byte-for-byte with explicitly authored reference geometry.

Measured on NVIDIA RTX 3060/Vulkan at 64×64: **12 → 1 → 0 color triangles** for base cube → one-triangle OBJ → culled. This is a deterministic geometry-work measurement, not an FPS or timing claim. Instancing's separate 1,024-cube timing benchmark runs at **320×320**.

Not implemented here: automatic mesh decimation, screen-size LOD, crossfades/hysteresis, independent shadow LOD, or occlusion culling. LOD thresholds are discrete and can visibly pop. Physics and editor picking retain authored base geometry. All replacement assets remain loaded; LOD is not asset streaming or memory eviction.
