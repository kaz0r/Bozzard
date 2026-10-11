# Trail Renderer

**Add Component → Trail Renderer** draws a ribbon behind a moving object: sword swings, tracer
rounds, comet tails, a car's tail lights, the wake of a magic orb. The ribbon always faces the
camera, follows the object's path in Play, and fades out behind it.

| Field | Meaning |
| --- | --- |
| Lifetime (s) | How long a point stays on the trail, 0.01–30 s. Longer trails reach further back. |
| Point spacing | World distance the object moves before another point is laid. Smaller is smoother on tight curves. |
| Start / End width | Width at the object and at the oldest point, in world units. |
| Start / End color | Linear RGB at the object and at the oldest point, 0–1. |
| Start / End opacity | Opacity at the object and at the oldest point. |
| Intensity | Multiplies the colour. Above 1 the trail reaches the bloom threshold and glows. |
| Blend | **Alpha** covers what is behind it; **Additive** adds light, for glows and sparks. |
| Offset | Where the trail leaves the object, in its local space: a sword tip, an exhaust. |

Width, colour and opacity blend from the start values at the object to the end values at the
oldest point, by each point's age. The long edges are softened, so even a wide ribbon reads as a
streak rather than a strip.

## How it works

- The simulation lays a point whenever the object has moved **Point spacing** since the last one
  and drops points older than **Lifetime**. A trail keeps at most 256 points, so a very fast object
  with a long lifetime lays its oldest points further apart instead of growing without bound.
- Each rendered frame adds the object's current, interpolated position as the newest point, so the
  ribbon stays attached to the object between simulation ticks.
- An object standing still lets its trail expire; one point marks where it stands, which draws
  nothing until it moves again.
- Trails are decorative, like particles: they step wherever particles step, pause with the
  simulation, are not saved in checkpoints, and are dropped when their object is removed.

The renderer turns every trail into a camera-facing strip on the CPU, two vertices per point so
neighbouring segments share their joint, and draws alpha ribbons then additive ones in a short pass
after transparent surfaces and particles. Ribbons are depth tested against the opaque scene, so a
wall hides them. `FrameStats::trail_triangles` counts the frame's ribbon triangles. The raw
diagnostic path skips them, as it skips particles.

`crates/bozzard-scene/src/middleware/trail.rs` and `crates/bozzard-render/tests/trails.rs` cover
point laying, expiry, blending, soft edges, occlusion and both blend modes.

## Limits

- Trails are unlit and unfogged: they keep their colour in shadow and in fog. Use a lower intensity
  or a fog-coloured end colour for distant trails.
- Ribbons are not depth sorted against transparent surfaces or particles; they draw after both.
- Trails grow only in Play. In the editor, moving an object by hand does not leave a trail.
