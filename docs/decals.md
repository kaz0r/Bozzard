# Decals

**Add Component → Decal** projects an image onto whatever opaque surfaces lie inside a box:
bullet holes, scorch marks, puddles, road markings, posters and signs, without editing the meshes
underneath. The box is the object's transform scaled by **Size**; the image is stamped down the
box's local **−Y**, so an unrotated decal paints the floor and a decal rotated 90° about X paints a
wall facing +Z.

| Field | Meaning |
| --- | --- |
| Image | An imported image. Empty projects a solid colour. Transparent pixels stay transparent. |
| Color | Linear RGB multiplied with the image. |
| Opacity | 0–1. |
| Size | Box size in local units. X and Z span the image (X → right, Z → down the image); Y is how far it projects. |
| Angle limit ° | Surfaces tilted further than this from facing the projection are left alone, so a floor decal does not smear down the sides of a step. 0–90. |
| Lit | Shade with the scene's sun, shadows, ambient and local lights, like a painted surface. Off keeps the image's own colour, for glowing signs. |

The decal fades out over the last quarter of the angle limit and near the top and bottom of the box,
so surfaces entering the volume do not get a hard edge. Select a decal in the editor to see its box
in amber, with a line down the projection axis.

## How it works

Decals draw in a pass of their own after the opaque scene and before transparent surfaces and
particles, so glass and water stay in front of them. Each box is rasterized once per covered pixel
(its back faces, which also cover a box the camera stands inside). The shader reads the depth
buffer, rebuilds the surface position under the pixel, and paints the image where that position
falls inside the box. The surface normal comes from the depth buffer's screen-space derivatives.

A lit decal uses the same terms as a simple lit surface — sun with shadows, ambient, local lights
with their shadows — and then the scene fog, so it sits in the same light as its surroundings.
Images are uploaded like any other texture the scene uses; a decal whose image is still uploading is
skipped for that frame instead of holding the frame up. `FrameStats::decals` counts the decals
drawn. The raw diagnostic path skips decals.

`crates/bozzard-render/tests/decals.rs` covers placement, the angle limit, opacity, lighting, a
camera inside the box, a missing image and the raw path.

## Limits

- Decals paint opaque surfaces only, not transparent surfaces, particles or trails.
- Lit decals ignore baked GI and environment lighting, so in a scene lit mostly by GI they read
  slightly darker than the surface around them. Lower **Color** or turn **Lit** off to match.
- The decal replaces the surface's colour under it; it does not change roughness, metalness or
  normals.
- Normals rebuilt from depth are coarse along silhouettes, where a decal can fade a pixel early.
- At most 1024 decals per frame.
