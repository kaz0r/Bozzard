# Scene documents

`examples/demo/scenes/scene-lab.json` is the editable reference. The same data is embedded in the demo for standalone runs. Bozzard currently supports schema version 1 and rejects unknown versions and fields.

## Coordinates and objects

Scenes use right-handed coordinates with Y up. Cameras look down local negative Z; WebGPU clip depth is 0..1 on all backends. `glam::camera::rh::proj::directx` supplies the Y-up, 0..1 projections, including when wgpu uses Vulkan underneath. Perspective FOV is vertical; orthographic size is the visible world-space height. Width follows window aspect, so resizing does not stretch objects.

Each object has a persistent string `id`, a display `name`, an optional parent ID, and a transform. IDs are unique within the document. Runtime ECS handles are newly allocated for each instance and never serialized.

Transforms store translation, Euler rotation in degrees, and scale. The matrix is translation × rotation × scale; Euler composition is Y × X × Z. Child world matrices are parent world × child local. Document order does not matter. Parent links are fixed for the current instance API; structural editing/reparenting requires respawning from a validated document for now.

Validation rejects missing parents, cycles, duplicate/empty IDs, invalid camera references, non-finite values, near-zero scales, and singular/overflowed composed matrices. Validation completes before any entities are spawned. Rotation angles are a first editable representation; a future animation system may use quaternions directly.

## Optional components

- `camera`: `orthographic` with `vertical_size`, or `perspective` with `vertical_fov_degrees`; both have positive `near` and `far > near`. A scene's `views` maps `2d`/`3d` to camera object IDs. Either view may be omitted.
- `drawable`: a `2d`/`3d` layer, `quad`/`cube` mesh, `white`/`checker` texture, linear RGB tint, and positive UV scale. Either mesh or texture can instead be `{"asset":"stable-id"}` referencing the document’s `assets` catalog; the referenced kind must match. Imported model drawables may include optional `material_overrides` entries keyed by surface index and source signature, with tint and opt-in metallic/roughness replacements; mismatched signatures remain inactive.
- `mesh_collider`: `{ "enabled": true, "mesh": [[[x,y,z], [x,y,z], [x,y,z]], ...] }`. Cooked local-space triangle surfaces, independent of the renderer/source assets; adding enabled `gravity` (Rigidbody) uses its solid convex hull. No Box Collider, Player Controller or Trigger on the same object. See [Mesh Collider](mesh-colliders.md) for authoring, limits and runtime behavior.
- `player_controller`: optional single-player movement/jump/follow-camera settings with a validated active 3D camera ID.
- `trigger`: optional local box `volume` plus collectible/checkpoint/goal `action`; separate from solid colliders. See [gameplay format, constraints and runtime-state rules](playable-demo.md#authoring-contract). Both additions remain optional in schema v1.
- `spin`: X/Y/Z angular rates in degrees per second. The demo registers a fixed-step system that updates local rotation, so children inherit parent motion.

An override uses a zero-based surface index and an importer-generated 16-character lowercase hexadecimal source signature. The signature is not an authored asset ID and includes structural node/mesh/primitive/material-slot identity, names, and indexed geometry; texture pixels and material factors are excluded. Tint is linear RGB and defaults to `[1, 1, 1]`; optional metallic and roughness values replace the source factors while retaining the source maps. Values are finite and constrained to 0..1:

```json
"material_overrides": [
  {
    "surface": 0,
    "source": "0123456789abcdef",
    "tint": [1.0, 0.9, 0.8],
    "metallic": 0.65,
    "roughness": 0.35
  }
]
```

Quads and cubes have unit dimensions centered at the origin; scale determines their size. Quad UVs start at the top-left. Both geometry types use indexed buffers. The renderer has per-object uniform buffers and a recreated-on-resize depth target. Nearer surfaces win using a strict less-than depth test. Equal-depth overlap has no stable layering promise: give overlapping sprites distinct Z positions.

Materials support opaque, masked, and blended alpha modes where the imported format provides them; transparent surfaces are sorted and blended without depth writes. The checker palette is procedural linear color data sampled with nearest/repeat filtering; imported PNG/JPEG images use sRGB decoding. Imported glTF/GLB materials support the renderer's PBR, lighting, shadow, environment, and display paths; per-object surface overrides multiply authored maps and are not global material edits. 2D is unlit. See [asset imports](assets.md) for supported formats and rendering limits.

## Save and reload contract

Saving captures this instance's supported component values and preserves its IDs, names, parent links, view references, and the asset catalog. Arbitrary runtime entities/components added outside the instance are not included. Missing required instance transforms cause saving to fail; removed optional components are omitted. The resulting document is validated again.

Files are written to a sibling temporary file, synced, and renamed over the destination. Parse/validation failures leave the old destination untouched. Failed interactive reloads retain the existing world and renderer. The player/server rebase asset paths relative to the destination when saving elsewhere; asset files remain in their original location. Cross-drive relative saves are rejected. See `assets.md` for the catalog and import contract. This is scene snapshotting, not a full editor undo stack or authored/play-world separation.

To extend the format, update version handling and tests deliberately. Do not serialize Rust type IDs, ECS entity handles, or GPU resources into scene documents.
