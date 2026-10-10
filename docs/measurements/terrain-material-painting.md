# Terrain material painting verification

Measured on Apple M2 Pro, macOS 26.6, arm64, Rust 1.95.0. Benchmarks use the
optimized release profile and assert identical final paint weights. These compare
implementations of the new painting feature; they do not measure a speedup in
existing batch rendering or total editor frame time.

## Additional optimization pass

| Workload | Optimized | Reference | Speedup |
| --- | ---: | ---: | ---: |
| 500 continuous paths, 64 stamps each, 129×129 terrain | 9.698 ms | 227.439 ms | 23.45× |
| 10,000 small discrete stamps, 129×129 terrain | 162.419 ms | 377.718 ms | 2.33× |

The continuous-path result is the median of five alternating measurement pairs.
An earlier run measured 23.16× and 2.12× respectively; the table records the final
source run. Timing varies with machine load and brush coverage.
The editor uses this batched path: it checks the terrain snapshot once per input
frame, reuses brush storage, and visits each stamp's covered rectangle. The
reference calls the same continuous brush separately for all 64 stamps, checking
the full snapshot each time. Both retain fractional coverage and produce identical
serialized weights.

The discrete reference independently scans all 16,641 vertices. The optimized
brush considers nine vertices for this workload. Both include full input
validation and produce identical weights. A separate correctness test compares
edge, corner, out-of-bounds and differently scaled strokes with this reference.

Reproduce both measurements:

```sh
cargo test -p bozzard-assets --release --lib terrain::paint::tests::benchmark \
  --locked --offline -- --ignored --nocapture --test-threads=1
```

The additional pass also removes collider/BVH reconstruction for material-only
transactions. Heights and sizes are compared by their exact float bits before
reusing collision. Painting still prepares and publishes an immutable material
revision, with one Undo step per accepted stroke.

## Code quality and regression checks

Continuous strokes retain sub-byte coverage and apply exponential brush rates,
including soft falloff. Regression tests compare 30, 60 and 144 FPS, weak strokes
and eventual saturation. This avoids frame-dependent rounding and stalled paint.
Paths interpolate between pointer samples, reset after misses and validate all
stamps before mutation.

Six editor UI regression tests, driven by egui input events, verify sculpting,
painting, release outside the 3D view, drag interpolation, missed-surface gaps,
pointer ownership and Undo.
Terrain transaction tests verify stale/cancelled jobs, immutable source revisions,
shared instances, authored material overrides, collision and cooked triangle
attributes. Legacy terrain sources remain covered by the existing tests.

The final checks passed: 13 asset tests, seven editor transaction tests, six editor
UI tests and one native GPU test, plus the two release benchmarks. Clippy passed
for all targets of `bozzard-assets`, `bozzard-editor` and `bozzard-editor-app`, with
their default features, using `-D warnings`. Formatting and whitespace checks also
passed.

See [terrain material painting](../terrain-material-painting.md) for the feature,
screenshots and reproducible workshop.

## Native renderer proof

The GPU regression passed on Apple M2 Pro / Metal at 128×128. Painting changed
4,382 pixels while preserving 289 mesh vertices, 512 triangles and the collider.
Undo, Redo and save/reopen restored exact RGBA. Lossless cooking, relocation and
removal of all authoring terrain revisions also preserved exact painted RGBA.

The first restricted run could not enumerate a Metal adapter. The same compiled
test passed when run with native hardware access. Universal compressed textures
are a separate export mode; this exact-pixel proof uses lossless embedded maps.

## Screenshots and native authoring

The committed before/after screenshots are 1600×1000 captures from the production
renderer. Painting changed 85,498 RGB pixels. The two saved sources have identical
terrain dimensions, resolution and all 4,225 height values. PNG conversion preserves
every captured RGB byte. The editor screenshot shows the actual native application
and its material controls.

In the native editor, a one-second Dirt stroke changed 110 paint-weight vertices
and published a new immutable source. Heights, object fields and collision stayed
unchanged. Saving after Undo restored the original scene JSON exactly. The editor
screenshot was captured after that Undo, with the workshop's original painted
terrain restored.

## Exported game

The workshop's `game.bozzard.json` exported successfully with universal cooking:
one cooked terrain asset, 1,480,200 cooked bytes and a 919,046-byte game pack. The
exported macOS application was moved to another directory and its executable ran
from an empty working directory, with no scene or project argument:

```sh
/path/to/relocated-export/Game.app/Contents/MacOS/Game \
  --smoke --hardware --backend metal --output /tmp/terrain-package-proof
```

The native Metal smoke run passed, loaded `terrain-1` and uploaded one surface with
one embedded image. This verifies the normal compressed package path in addition
to the exact-pixel lossless cooking regression above.
