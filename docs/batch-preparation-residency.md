# Retained batch preparation and culling residency

Implemented on `hfx/batch-preparation-residency`, starting from `46f1e10`.
This pass reduces **CPU preparation**, not draw counts or shader work. Both sides
of the comparison enable the existing global/incremental color plan, opaque graph
instancing, independent shadow batching, and normal shadow/occlusion caches.

## Profile first

Instrumentation was added before changing preparation. An initial release A/A
profile separated the following costs on Intel Iris Xe / Vulkan / Mesa 26.2.3:

- Singleton diagnostics: approximately **0.14–0.33 ms/frame** across active,
  frozen, and moving-camera workloads.
- Independent shadow grouping: approximately **0.26–0.28 ms/frame** during
  active updates. Frozen and camera-only frames already reuse depth maps and do
  not run this stage.
- Color buffer preparation: approximately **0.08–0.37 ms/frame**. The camera
  workload uploaded **7,844,096 bytes** over 60 measured frames.

The initial instrumented executable and reports are retained locally in
`work/batch-preparation-residency/baseline/`. These are not comparable absolute
FPS measurements across sessions; the results below use paired modes in the
same final executable.

## Implementation

### Diagnostics

A certified color plan owns compact peer-group IDs, reusable count/visibility
arrays, and its last histogram/singleton classification. Exact plan metadata
checks already invalidate these IDs when eligibility or keys change.

- Identical visibility reuses the complete classification.
- Visibility changes recount only actual output members using integer group
  IDs; hidden compatible peers do not count.
- A borrowed-key hash table is needed only to initialize IDs for a new plan,
  rather than every frame.
- The per-frame reference retains the original combined histogram/peer-count
  traversal and singleton classification; it does not incur an extra surface
  traversal merely to make the optimized comparison look faster.

### Shadow membership

Successful shadow groups and their index vectors return to a retained plan after
submission. Exact metadata comparisons cover mesh, texture, graph identity, PBR
host, lit/transparency flags, and whether a draw is deformed. Transform, tint,
light, and camera changes do not require regrouping; their uniform updates,
shadow preparation, frustum tests, and depth-map invalidation still run normally.

Membership covers all eligible casters, including camera-culled casters. Partial
shadow ranges and nonzero first-instance offsets are unchanged. No group vector
is cloned on a warm update. Scene-size changes discard old grouping storage even
when shadows do not need rendering.

### Color buffer residency

Each filtered output batch remembers its certified plan identity. Reusable
owner/slot/active arrays associate that identity with a packed buffer instead of
assigning buffers by current visible output position.

- Reserve every surviving group's buffer before assigning newcomers.
- Recycle inactive allocations without disturbing survivors.
- Retain at most **eight inactive color buffers** (128 KiB); prune extra spares
  and repair slot references when vector entries move. Surviving GPU allocations
  and their cached records remain intact.
- A temporary singleton uses its ordinary individual binding. A fully hidden
  frame submits no color geometry and retains at most eight color buffers.
- A plan rebuild resets ownership but still reuses available allocations and
  the existing per-record byte comparisons.

Asset publication, failed frames, instancing/graph switches and disabled state
caching invalidate the relevant preparation caches. Disabling only preparation
caching releases its CPU metadata while keeping the ordinary buffer-reuse path.
Additional CPU storage is proportional to current plan membership and the bounded
pool, with oversized residency scratch capacity trimmed after scene shrinkage.

**Limitation:** visible members within a group are still packed contiguously.
Removing an early member can therefore move later records inside that one buffer.
This pass stabilizes **group-to-buffer residency**, not per-member offsets across
partial-group culling. It adds no shader remap table, storage-buffer dependency,
GPU feature, extra draw, or change to indirect/occlusion instance arguments.

## Instrumentation

`FrameStats` and the editor profiler expose:

- `batch_diagnostics_ms` / `batch_diagnostics_reused`;
- `instance_prepare_ms`;
- `shadow_batch_plan_ms` / `shadow_batch_plan_reused` / rebuild count;
- `shadow_instance_prepare_ms`;
- logical resident color/shadow buffer bytes, alongside existing uploads and
  allocation counters.

Diagnostics are included in `batch_plan_ms`. All four preparation stage timers
are included in `prepare_ms`; buffer timers exclude shader pipeline compilation
and GPU execution. A zero shadow-stage time means the stage was skipped because
no depth work was needed, not a measured instantaneous grouping operation.

`set_batch_preparation_caching_enabled(false)` selects rebuilt diagnostics/shadow
membership and positional color buffers, without changing batch eligibility,
ordering, capacity, visibility, or depth-map caching. Disabled state caching also
bypasses these preparation caches.

## Final release results

Intel Iris Xe / Vulkan / Mesa 26.2.3, 1280 × 800, 12 warm-up and 60 measured frames,
three independent paired runs, alternating mode order over shared retained scene
inputs. Timings exclude extraction. Values are **medians of three run medians**.
Both modes submit identical color/shadow draw and triangle counts and identical
batch diagnostics.

| Workload | Renderer CPU: per-frame → retained | Prepare: per-frame → retained | Diagnostics: per-frame → retained | Shadow grouping: per-frame → retained |
| --- | ---: | ---: | ---: | ---: |
| Active factory | 3.849 → 3.474 ms | 2.950 → 2.577 ms | 0.1571 → 0.0006 ms | 0.2931 → 0.0943 ms |
| Frozen factory | 4.058 → 3.964 ms | 2.931 → 2.800 ms | 0.2559 → 0.0007 ms | skipped → skipped |
| Camera-only factory | 5.233 → 5.136 ms | 3.944 → 3.769 ms | 0.3283 → 0.0231 ms | skipped → skipped |

Renderer CPU improves approximately **9.7%, 2.3%, and 1.9%**, respectively. The
camera buffer stage itself is **not faster** (0.360 → 0.385 ms): ownership work
and remaining within-group repacking offset its modest upload reduction. Most
of the measured CPU benefit is diagnostics and retained shadow membership.

| Workload | Synchronized time: per-frame → retained | GPU pass sum: per-frame → retained | Color upload bytes over 60 frames: per-frame → retained |
| --- | ---: | ---: | ---: |
| Active factory | 16.362 → 15.794 ms | 10.563 → 10.564 ms | 122,880 → 122,880 |
| Frozen factory | 13.881 → 13.770 ms | 9.253 → 9.252 ms | 0 → 0 |
| Camera-only factory | 15.882 → 15.841 ms | 9.749 → 9.768 ms | 7,844,096 → 7,504,128 |

Camera uploads fall **4.3%**, without increasing measured allocation counts (two
color allocations per camera run in each mode). Maximum logical packed GPU
storage is unchanged: 544 KiB color for active/frozen, 944 KiB color for camera,
and 944 KiB shadow in all three workloads. Active diagnostics and shadow
membership reuse occur on 58/60 frames; frozen diagnostics reuse on 60/60.

GPU time is effectively unchanged. Synchronized improvements are small and are
not a general FPS or GPU-performance claim. CPU tails and run-to-run variation
remain; the full reports retain per-frame samples and p95/p99 values.

The targeted 1,024-object whole-group culling regression alternates views after
warm-up while retaining identical pixels and submitted counts. Across its four
measured transitions, positional buffers upload **524,288 bytes**, while retained
groups upload **zero bytes** and allocate no buffers. It also tests an all-hidden
frame, bounded-spare eviction/regrowth, and a singleton view. This demonstrates
where stable group residency is worthwhile even though factory culling often
changes members inside groups.

## Verification and reproduction

- Full native renderer test targets: **87 passed, 5 explicitly manual tests
  ignored**. This includes randomized diagnostic/slot-map checks and new exact
  parity regressions for motion, key/mesh/lit edits, removal/reordering, alpha
  publication, missing-resource failure/retry, switches, and disabled shadows.
- Both new native regressions also pass in release mode.
- Factory motion parity: **20 moving frames**, exact pixels, submitted geometry,
  diagnostic classifications, and gameplay checkpoints, with shadow preparation
  caching disabled to exercise grouping.
- All nine final workload runs match exact captures at four camera headings and
  preserve authoritative scene/checkpoint state.
- Renderer Clippy, retained factory test-target Clippy, editor application check,
  formatting, and diff checks pass.

```sh
cargo test --locked -p bozzard-render --tests -- --test-threads=1
cargo test --release --locked -p bozzard-render --test batch_preparation \
  -- --test-threads=1 --nocapture
cargo test --release --locked -p bozzard-editor --test retained_render \
  batch_preparation_factory_pixels_and_checkpoints_match_during_motion \
  -- --ignored --exact --nocapture --test-threads=1

BOZZARD_BATCH_PREPARATION_OUTPUT="$PWD/work/batch-preparation-residency/repeat" \
  cargo test --release --locked -p bozzard-editor --test retained_render \
  profile_earth_factory_batch_preparation \
  -- --ignored --exact --nocapture --test-threads=1
```

The profile requires a hardware adapter. Native parity accepts ordinary native
adapters; the factory motion test also accepts software graphics adapters. Do not
run competing profiling/build work alongside measurements. Final local reports,
captures, command logs and aggregate JSON are under
`work/batch-preparation-residency/final-{1,2,3}/` and `summary.json` (ignored work
artifacts, not committed benchmark data).

## Preparation-scan profiling follow-up

The release measurements above were collected before the subsequent
instrumentation-only profiling pass. That pass adds disjoint preparation stages,
source/mask/resource work counters and logical temporary-vector capacities; it
does not add another rendering optimization. See [CPU preparation scan
profiling](preparation-scan-profile.md) for the new measurements and recommended
next targets. Its local reports are under `work/preparation-stage-profile/`.
