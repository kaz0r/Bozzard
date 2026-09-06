# Scene documents

`examples/demo/scenes/scene-lab.json` is the editable reference. The same data is embedded in the demo for standalone runs. Bozzard currently supports schema version 1 and rejects unknown versions and fields.

## Coordinates and objects

Scenes use right-handed coordinates with Y up. Cameras look down local negative Z; WebGPU clip depth is 0..1 on all backends. `glam::camera::rh::proj::directx` supplies the Y-up, 0..1 projections, including when wgpu uses Vulkan underneath. Perspective FOV is vertical; orthographic size is the visible world-space height. Width follows window aspect, so resizing does not stretch objects.

Each object has a persistent string `id`, a display `name`, an optional parent ID, and a transform. IDs are unique within the document. Runtime ECS handles are newly allocated for each instance and never serialized.

Transforms store translation, Euler rotation in degrees, and scale. The matrix is translation × rotation × scale; Euler composition is Y × X × Z. Child world matrices are parent world × child local. Document order does not matter. Parent links are fixed for the current instance API; structural editing/reparenting requires respawning from a validated document for now.

Validation rejects missing parents, cycles, duplicate/empty IDs, invalid camera references, non-finite values, near-zero scales, and singular/overflowed composed matrices. Validation completes before any entities are spawned. Rotation angles are a first editable representation; a future animation system may use quaternions directly.

## Optional components

- `camera`: `orthographic` with `vertical_size`, or `perspective` with `vertical_fov_degrees`; both have positive `near` and `far > near`. A scene's `views` maps `2d`/`3d` to camera object IDs. Either view may be omitted.
- `drawable`: a `2d`/`3d` layer, `quad`/`cube` mesh, `white`/`checker` texture, linear RGB tint, and positive UV scale. Mesh/texture names are built-ins, not asset file paths.
- `spin`: X/Y/Z angular rates in degrees per second. The demo registers a fixed-step system that updates local rotation, so children inherit parent motion.

Quads and cubes have unit dimensions centered at the origin; scale determines their size. Quad UVs start at the top-left. Both geometry types use indexed buffers. The renderer has per-object uniform buffers and a recreated-on-resize depth target. Nearer surfaces win using a strict less-than depth test. Equal-depth overlap has no stable layering promise: give overlapping sprites distinct Z positions.

Materials are opaque. The checker palette is procedural linear color data sampled with nearest/repeat filtering; imported images, sRGB texture decoding, mipmaps, transparency sorting, and texture streaming are not implemented. 3D uses ambient plus a fixed directional diffuse light and inverse-transpose normal transforms; 2D is unlit. There are no shadows or PBR materials yet.

## Save and reload contract

Saving captures this instance's supported component values and preserves its IDs, names, parent links, and view references. Arbitrary runtime entities/components added outside the instance are not included. Missing required instance transforms cause saving to fail; removed optional components are omitted. The resulting document is validated again.

Files are written to a sibling temporary file, synced, and renamed over the destination. Parse/validation failures leave the old destination untouched. Failed interactive reloads retain the existing world. This is scene snapshotting, not a full editor undo stack or authored/play-world separation.

To extend the format, update version handling and tests deliberately. Do not serialize Rust type IDs, ECS entity handles, or GPU resources into scene documents.
