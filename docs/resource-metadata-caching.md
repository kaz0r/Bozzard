# Retained static resource metadata

Follow-up to [CPU preparation scan profiling](preparation-scan-profile.md), on
`hfx/batch-preparation-residency`. This pass removes redundant static resource
validation/bounds/sidedness lookup; it does not optimize object source/light-mask
comparisons, change shaders, or add full-instance visibility indirection.

## Implementation and correctness boundaries

A prepared surface can now retain an inline resource certificate containing its
**local mesh bounds and static double-sided flag**. Other static material fields
(opacity, cutoff, PBR factors/host) were already retained by surface preparation.

The certificate belongs to the prepared surface's resolved mesh identity, not a
positional object binding or visible batch slot:

- Existing exact static-source comparisons retain or replace the whole prepared
  draw. Mesh/material/override/graph changes rebuild its certificate; camera and
  model-transform changes can reuse local bounds.
- Existing mesh/model/image publication and removal paths clear retained surfaces through
  `invalidate_object_bindings`. This includes same-ID replacement, aliases,
  imported-resource clearing, model/mesh/image publication, and changes to the
  generated resource identity set. A future mutator of bounds/static shading must
  preserve this invalidation invariant.
- **Skinned models, text and sprites always bypass certification**, including
  skinned models without a currently active pose. Their current mesh/bounds and
  model sidedness are resolved through the original paths each frame.
- Object texture-identity comparison and binding creation/rebinding remain in
  their original order. Only certified mesh validations are skipped. Bounds are
  resolved after the complete original validation pass, so new/missing resources
  do not change error precedence or become unchecked indexed lookups.
- Object-source construction, light-mask guards, normal inversion, and combined
  camera/model finite checks remain intact. Camera-only frames still validate
  every combined matrix. Shared frame-uniform retry stamping is unchanged.
- Draws publish back into retained surface storage only after successful
  submission. All failed-frame paths clear certificates with surface storage and
  reset the reported retained metadata payload bytes.
- Transparency sorting moves the certificate with its draw; no independent
  positional metadata cache needs synchronization.

The default-enabled reference switch is:

```rust
renderer.set_resource_metadata_caching_enabled(false);
```

This disables/clears certificates while keeping surface preparation, batching,
shadow and other normal caches enabled. Global state caching and retained surface
preparation must both be enabled to retain certificates. Disabling either
bypasses the optimization. Existing `FrameStats` remain the last frame snapshot
until another draw, as with the other renderer switches.

No new map, GPU handle cache, persistent bounds array or per-object clock is
introduced. The transient bounds vector is still filled each frame, from cached
local bounds where eligible. Storage is inline and follows the existing
current-scene vector compaction rules rather than retaining historical identities.

## Diagnostics

New `FrameStats` fields, also displayed by **Preparation scan · CPU** and
included in each factory profile sample:

- `resource_metadata_hits`, `resource_metadata_builds`,
  `resource_metadata_bypasses` partition successfully prepared surfaces.
- `resource_bounds_lookups` counts actual bounds-resolution calls before encoding,
  not the remaining per-batch encode-time mesh lookups. It equals builds plus
  bypasses on successful frames.
- `resource_metadata_bytes` reports populated inline payload bytes, **already
  included** in `surface_preparation_bytes`; it is not additional allocation
  traffic or reserved-capacity measurement.

Existing `mesh_validation_checks` now counts only actual validations, and the
same disjoint stage timers cover reference/cached work. The bounds timer includes
cold certificate construction; the object-uniform timer includes the remaining
source/mask work and sidedness fallback for uncertified geometry. Nested timing
boundaries from the profiling pass are unchanged.

## Repeated paired release measurements

Intel Iris Xe / Vulkan / Mesa 26.2.3, 1280 × 800. Three paired runs of each workload,
12 warm-up + 60 measured frames per renderer per run. Each input is shared between
renderers; order alternates. Both sides enable the same surface, batch, residency,
shadow and graph caches. Only resource certification differs.

Numbers below are **medians of three run medians**. Reference and cached medians
are computed separately; percentage reductions are their ratio, not the median
of per-run percentages. These comparisons do not reuse the historical profiling
numbers from another session.

| Workload | Renderer CPU reference → cached | Reduction | Prepare reference → cached |
| --- | ---: | ---: | ---: |
| Active factory | 3.460959 → 3.271615 ms | **5.5%** | 2.470977 → 2.279677 ms |
| Frozen factory | 3.923416 → 3.540040 ms | **9.8%** | 2.746192 → 2.324196 ms |
| Camera-only factory | 5.194774 → 4.976428 ms | **4.2%** | 3.806980 → 3.518215 ms |

Every individual run has lower renderer CPU and preparation medians in cached
mode. Active CPU improvements vary more (approximately 2.3–8.3% per run) than the
more consistent resource-stage reductions.

Resource-stage groups are summed **per frame before calculating medians**:

| Workload | Binding/mesh checks + bounds reference → cached | Reduction |
| --- | ---: | ---: |
| Active | 0.317908 → 0.105043 ms | 67.0% |
| Frozen | 0.451405 → 0.132097 ms | 70.7% |
| Camera-only | 0.446748 → 0.130871 ms | 70.7% |

For context, individual stage medians are:

| Stage | Active reference → cached | Frozen reference → cached | Camera reference → cached |
| --- | ---: | ---: | ---: |
| Binding/mesh checks | 0.199920 → 0.075226 | 0.259583 → 0.093743 | 0.255503 → 0.091674 |
| Bounds collection | 0.123831 → 0.028420 | 0.186732 → 0.038596 | 0.186321 → 0.038589 |
| Object source/mask loop | 0.340530 → 0.312651 | 0.522626 → 0.437507 | 0.660467 → 0.563189 |

Stage values are milliseconds. The object-loop reduction comes from reading
retained sidedness instead of repeated model/material lookup, **not skipping
source/mask or matrix checks**. Individual medians do not add to total or grouped
medians. Other stages varied: retained surface refresh and occlusion were slightly
slower in cached mode, particularly active frames; the total CPU measurements
include those effects rather than assuming all saved lookup time is recoverable.

### GPU time and uploads

GPU pass-sum medians remain effectively unchanged:

| Workload | GPU reference → cached | Synchronized reference → cached |
| --- | ---: | ---: |
| Active | 10.462656 → 10.553984 ms | 14.875587 → 14.761500 ms |
| Frozen | 9.261041 → 9.208099 ms | 13.669100 → 13.252202 ms |
| Camera-only | 9.668646 → 9.720390 ms | 15.464381 → 15.191530 ms |

GPU differences are within about 1% and change sign by workload. There is no GPU
optimization or promised proportional FPS gain. Color/shadow instance upload
totals match between modes in every run: active 122,880/122,880 bytes over 60
frames; frozen 0/0; camera 7,504,128/0. The prior group-residency limitation for
partially visible groups is unchanged.

### Actual work and memory

There are 2,891 surface records. Frozen/camera frames change from **1,873 mesh
validations + 2,891 bounds lookups to zero of each**, with 2,891 certificate hits,
no builds/bypasses, and no object binding allocations. Active frames also have
zero mesh validations; certificate hits are 2,890–2,891, with at most one new
primitive certificate/bounds lookup from a static source edit.

Object-source/light-cache checks remain 2,891 per frame. Camera combined-matrix
checks remain 2,891; frozen combined-matrix checks remain zero. Active checks
retain their original 8–569 range. Uniform and light-mask build/write behavior is
unchanged.

Populated certificate payload is **80,948 bytes (79.1 KiB)**. Retained surface
capacity accounting rises from 3,214,112 bytes in the preceding executable to
3,312,416 bytes here: **98,304 bytes (96 KiB) more**, including layout/capacity
slack. The cache-disabled CPU reference uses the same enlarged draw type, so its
surface capacity also includes the inline option slots. Do not add payload bytes
to the surface-capacity total or interpret zero disabled payload as removal of
those slots.

Scratch capacities/upload bytes are unchanged. Empty scenes drop all populated
certificates; existing vector compaction bounds retained capacity after large
scenes shrink. Native dynamic-geometry regressions demonstrate that skinning,
text and sprite bounds remain uncached.

## Verification and reproduction

- Native renderer targets: **91 passed, 5 manual tests ignored**.
- Preparation suite: **five passed** in debug and release, including three new
  resource-specific regressions covering camera/transforms, static source edits,
  reordering, cache switches, dynamic skin bounds, same-ID mesh/model publication,
  sidedness changes, model-to-mesh replacement, failed uploads, missing mesh/model
  errors, asset removal/clearing, image aliases, failed-frame retry and empty scenes.
- Existing three batching/residency regressions pass in release.
- Existing overflowing camera/model regression still rejects repeated failures and
  exactly recovers the original pixels/frame-uniform upload.
- Factory resource motion parity: **20 moving frames**, exact RGBA, geometry,
  batching diagnostics and authoritative gameplay checkpoints.
- Nine release workload profiles pass exact counts/diagnostics and four-heading
  RGBA parity. Submitted shadow triangles match in every run.
- **36 PPM comparisons** against the saved preceding executable match byte for
  byte (12 retained-mode captures × three runs).
- Renderer/test-target Clippy, editor-app check, formatting and diff checks pass.

```sh
cargo test --locked -p bozzard-render --tests -- --test-threads=1
cargo test --release --locked -p bozzard-render --test preparation \
  --test batch_preparation -- --test-threads=1

cargo test --release --locked -p bozzard-editor --test retained_render \
  resource_metadata_factory_pixels_and_checkpoints_match_during_motion \
  -- --ignored --exact --nocapture --test-threads=1

BOZZARD_RESOURCE_METADATA_OUTPUT="$PWD/work/resource-metadata/repeat" \
  cargo test --release --locked -p bozzard-editor --test retained_render \
  profile_earth_factory_resource_metadata \
  -- --ignored --exact --nocapture --test-threads=1
```

Hardware graphics is required for the profile; do not run competing builds or
profiles alongside it. Local ignored reports/captures/controls/logs are under
`work/resource-metadata/`; `summary.json` and `summarize.jq` preserve aggregation
and per-run values. Renderer and editor test harnesses were release-built; player
and editor application release executables were not rebuilt. No commit or push
was made; previous uncommitted work is preserved.
