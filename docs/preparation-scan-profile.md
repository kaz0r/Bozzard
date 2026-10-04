# CPU preparation scan profile

This is an **instrumentation-only follow-up** to
[retained batch preparation](batch-preparation-residency.md), on
`hfx/batch-preparation-residency`. No resource lookup, validation, culling,
object-uniform, shader or upload behavior was optimized in this follow-up.

## What is measured

`FrameStats::preparation_stages()` exposes 19 disjoint CPU intervals plus an
unaccounted remainder, in execution order. The editor's **Preparation scan ·
CPU** panel shows the same stages. The factory profile records per-stage
median/p95/p99 distributions and each frame's stage timings/work counts.

The requested costs now have explicit boundaries:

- **Resource checks and bounds:** `object_binding_prepare_ms` includes texture
  identity comparisons, object-binding creation/rebinding, and imported/model
  mesh validation. `bounds_collect_ms` includes local mesh-bounds lookup and the
  allocation/fill of its temporary vector. Initial dimensions/camera/material
  validation is separately measured by `scene_validation_ms`.
- **Object sources and light masks:** `object_uniform_prepare_ms` wraps the
  original source/mask loop without changing its order. It includes previous
  model/material lookup, light-cache checks, source comparison, conditional
  combined-matrix validation, and any dirty normal/uniform construction.
  `light_selection_ms` separately measures light-list revision checks/refresh.
  Counters distinguish source checks, matrix checks, mask checks/hits/builds and
  uniform builds. There are **no per-object clock reads**; the tiny individual
  checks are deliberately not given misleading micro-timers.
- **Graph bookkeeping:** `graph_prepare_ms` covers discovery/cache retirement
  and missing ordinary variants; `graph_instanced_prepare_ms` covers batch
  checks and missing instanced variants. Cold compilation is included in these
  stages, not misrepresented as warm bookkeeping.
- **Temporary masks and uploads:** `visibility_ms` includes the frustum flag
  vector and CPU tests. `visibility_bookkeeping_ms` includes visible-item mask
  allocation/marking/counting. `individual_prepare_ms` covers allocation and
  marking of the individual-upload mask; `individual_upload_ms` covers its scan
  and conditional dirty queue writes. Optional transparent-index filtering and
  particle preparation have separate timers.

Additional parents cover renderer setup, complete color batching, occlusion and
complete shadow preparation so the requested timings can be interpreted within
nearly the whole preparation budget. **Do not add child timers twice**:
`batch_prepare_ms` includes plan/diagnostic/buffer timing and cold pipelines;
`shadow_prepare_ms` includes comparison, fitting, groups and buffers. The existing
`shadow_state_ms` also includes post-submission metadata refresh/retirement, so
it is not a disjoint preparation child.

On successful frames, the disjoint stages plus `prepare_unaccounted_ms` sum to
`prepare_ms`. Failed frames may have partial counters/timers without a completed
total. GPU execution, readback, extraction and post-submission retirement are
outside these preparation intervals.

## Release measurements

Intel Iris Xe / Vulkan / Mesa 26.2.3, 1280 × 800. Three paired runs per workload,
12 warm-up and 60 measured frames, alternating the previous per-frame batching
reference with retained batching over the same scene input. The table below
uses the **retained mode**, with medians of three run medians. For grouped rows,
component durations are summed **per frame before calculating the median**.

| Requested cost | Active factory | Frozen factory | Camera-only factory |
| --- | ---: | ---: | ---: |
| Resource checks + bounds collection | 0.371 ms | 0.521 ms | 0.499 ms |
| Object source/light-mask loop | 0.352 ms | 0.531 ms | 0.666 ms |
| Ordinary + instanced graph bookkeeping | 0.027 ms | 0.035 ms | 0.033 ms |
| Item/individual/transparent masks + individual upload scan | 0.029 ms | 0.065 ms | 0.061 ms |

The mask row **excludes frustum testing** and bounds collection, which already
have separate stages. These timings measure allocation/fill/scan work together,
not allocator calls in isolation. Resources split as follows:

| Individual stage | Active | Frozen | Camera-only |
| --- | ---: | ---: | ---: |
| Object binding checks/mesh validation | 0.243 ms | 0.327 ms | 0.310 ms |
| Bounds lookup/vector collection | 0.128 ms | 0.191 ms | 0.186 ms |
| Initial scene validation | 0.033 ms | 0.035 ms | 0.038 ms |
| CPU frustum visibility | 0.188 ms | 0.274 ms | 0.218 ms |
| Visibility/item bookkeeping | 0.011 ms | 0.017 ms | 0.017 ms |
| Individual-mask preparation | 0.009 ms | 0.030 ms | 0.027 ms |
| Individual-mask scan/uploads | 0.009 ms | 0.015 ms | 0.014 ms |

These individual medians need not add to a grouped median or total median.

The complete preparation medians are **2.588 / 2.884 / 3.934 ms**. Other notable
parents are shadow preparation (**0.687 / 0.280 / 0.251 ms**) and occlusion
preparation (**0.295 / 0.322 / 0.862 ms**). The shadow parent can be nonzero even
when maps are cached: comparison/classification still occurs, whereas shadow
regrouping/buffer work is skipped. CPU color-batch preparation is
**0.284 / 0.344 / 0.788 ms**. These are additional candidates, not part of the
four requested cost groups.

The median unaccounted interval is approximately **0.002–0.003 ms**. Across the
retained samples, the maximum remainder is 0.0102 ms; stage totals match the
complete interval to floating-point rounding (maximum discrepancy below
2e-15 ms).

### Checks versus rebuilds

All measured frames have 2,891 surfaces and 1,424 scene items. In retained mode:

| Work per frame | Active (median) | Frozen | Camera-only |
| --- | ---: | ---: | ---: |
| Object source checks | 2,891 | 2,891 | 2,891 |
| Light-cache checks | 2,891 | 2,891 | 2,891 |
| Light-cache hits | 2,883 | 2,891 | 2,891 |
| Light-mask builds | 8 | 0 | 0 |
| Combined view/model matrix checks | 8 | 0 | 2,891 |
| Object-uniform builds | 8 | 0 | 0 |
| Imported/model mesh validations | 1,873 | 1,873 | 1,873 |
| Object binding allocations | 0 | 0 | 0 |
| Ordinary graph source checks | 2,891 | 2,891 | 2,891 |
| Instanced graph batch checks | 34 | 34 | 59 |
| Individual upload candidates | 29 | 29 | 19 |

The frozen/camera check and build counts above are constant across all measured
frames except camera instanced groups (57–59) and individual candidates (19–20).
Frozen individual queue writes are zero; camera writes can be one on a returning
singleton. Active outliers reach 2,891 mask builds and 569 uniform/matrix builds;
the full reports retain those samples rather than treating every active frame
as eight changes.

Camera movement correctly preserves the original **all-surface combined-matrix
validation**, despite rebuilding no object uniforms. Any future fast path must
preserve overflow/non-finite camera validation and retry behavior.

### Temporary vector capacities

Observed capacities per frame are constant across these workloads:

| Vector | Logical capacity |
| --- | ---: |
| Bounds | 69,384 bytes |
| Frustum visibility flags | 2,891 bytes |
| Visible item flags | 1,424 bytes |
| Individual upload flags | 2,891 bytes |
| Optional transparent indices | 0 bytes |
| Sum | **76,590 bytes (74.8 KiB)** |

This is a sum of five observed capacities, **not peak live memory, retained cache
storage, upload traffic or allocator instrumentation**. It excludes other
internal scratch data. Most of these bytes belong to the bounds vector, whose
stage also performs mesh/resource lookups; its elapsed time cannot be attributed
to allocation alone.

## Interpretation and next target

Within the four requested areas, the highest-value candidates are **repeated
object-source/light-cache checks** and **stable resource/bounds lookups and
validation**. They still process the whole scene on frozen frames while building
nothing. Ordinary graph bookkeeping and mask preparation are much smaller;
optimizing their allocations first is not justified by this fixture.

A sensible next optimization experiment is retaining validated resource/bounds
metadata behind exact asset publication and deformation guards, followed by
reducing redundant source/mask input work using existing retained change
information. Do not skip camera-dependent matrix checks, trust stale resource
identities, or extrapolate these cost ceilings into promised speedups.

This follow-up **does not claim a performance improvement or zero instrumentation
overhead**. Coarse clocks are O(stages), not O(objects), but counters and timing
calls still cost work. A saved pre-instrumentation executable was run before and
after the measured sessions; its own totals changed substantially with session
conditions (for example, frozen preparation 1.680 → 2.858 ms). Those cross-session
controls cannot establish a small overhead or an optimization gain. Rank these
within-frame intervals and compare future optimizations using the same
instrumentation in paired modes.

The static resource/bounds follow-up is now implemented; see [retained resource
metadata](resource-metadata-caching.md) for its separately paired measurements,
invalidation regressions and memory tradeoff. The profiling numbers above remain
the historical pre-optimization results.

## Verification and reproduction

- Full native renderer targets: **88 passed, 5 manual tests ignored**.
- All three preparation regressions also pass in release, including new warm,
  camera, hidden-material-edit and empty-frame work-count/reset checks.
- Accounting checks cover disjoint-stage totals, nested-parent inclusion,
  mask hit/build partitioning, and upload candidate counts.
- Nine release workload runs preserve exact RGBA parity between batching modes,
  submitted counts, diagnostics and authoritative gameplay/checkpoints at the
  usual four capture headings.
- **72 PPM comparisons** with the saved uninstrumented executable match byte
  for byte (24 captures per run × three instrumented runs).
- Renderer/test-target Clippy, editor application check, formatting and diff
  checks pass.

```sh
cargo test --locked -p bozzard-render --tests -- --test-threads=1
cargo test --release --locked -p bozzard-render --test batch_preparation \
  -- --test-threads=1 --nocapture

BOZZARD_BATCH_PREPARATION_OUTPUT="$PWD/work/preparation-stage-profile/repeat" \
  cargo test --release --locked -p bozzard-editor --test retained_render \
  profile_earth_factory_batch_preparation \
  -- --ignored --exact --nocapture --test-threads=1
```

The factory profile requires a hardware adapter. Do not run competing builds or
profiles during measurements. Reports, captures, controls and logs are local
ignored artifacts under `work/preparation-stage-profile/`; `summary.json` contains
all stages/counter ranges, and `summary-groups.json` aggregates the requested
cost groups. This profiling work remains uncommitted alongside the earlier pass.
