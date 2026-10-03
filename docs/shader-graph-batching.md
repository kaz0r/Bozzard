# Shader-graph batching

Opaque shader-graph surfaces now use the existing portable instance path. This
follow-up starts from singleton diagnostics rather than assuming every singleton
can batch. Existing global grouping, ordering certificates, shared frame constants,
partial uploads, visibility and shadow caching remain in place.

## Eligibility and diagnostics

`SceneRenderer::frame_stats().batching` is a `BatchingStats` record, also available
in exported profiler JSON and **Debug → Profiler → Viewport rendering → Batch
eligibility · before occlusion**.

The counters describe the frustum-visible plan **before occlusion**, excluding
HUD and particles. `planned_draws` can therefore exceed submitted `color_draws`.
`size_histogram` counts draws with 1, 2–3, 4–7, 8–15, 16–31, 32–63, and 64 members.
`graph_surfaces` and `graph_instanced_surfaces` count surfaces, not commands.

The following reasons partition `singleton_draws`, with earlier reasons taking
precedence when several exclusions apply:

1. `singleton_disabled`: all instancing disabled for diagnosis.
2. `singleton_transparent`: the renderer's existing transparent classification.
3. `singleton_deformed`: per-object deformed/skinned mesh buffers.
4. `singleton_shader`: opaque graph instancing disabled for diagnosis.
5. `singleton_unsupported_mesh`: text/sprites or another unsupported mesh flavor.
6. `singleton_unique_key`: no other visible eligible surface has its exact key.
7. `singleton_split`: eligible peers exist, but the plan left a singleton. This
   includes conservative ordering, visibility filtering, capacity tails, and the
   consecutive diagnostic path; it does **not** identify one of those causes alone.

Peer counting borrows mesh/texture keys instead of cloning per-surface asset
strings. The diagnostic traversal is included in `batch_plan_ms` on both sides of
the benchmark.

## Implementation

Color keys include mesh identity, texture identity, PBR/basic host flavor, lighting
eligibility, and `ShaderSource::id`, the existing WGSL content hash. Stock and graph
shaders cannot share a color batch; different specialized programs cannot either.
Per-object tint, UV repeat, roughness/metallic overrides, winding, alpha cutoff,
light masks and current/previous transforms remain in each 256-byte object record.
The graph clock remains in the shared 320-byte frame buffer.

Instanced graph modules reuse the stock instancing transform: select the object
with `instance_index` in the vertex entry point and a flat instance varying in the
fragment entry point. Splicing the graph surface function does not change this
selection or the host's auxiliary outputs. Only the host's first declarations and
entry points are rewritten, leaving appended graph helpers untouched.

Opaque graph pipelines compile lazily, only for the basic/PBR and auxiliary-output
flavors actually used by multi-member batches. Variants live inside the existing
bounded graph cache and retire with their parent graph. `graph_instanced_compilations`
counts newly compiled variants per frame. Transparent graph pipelines remain the
ordinary individual variants. Graph-only batches do not force creation of the
stock instanced color pipelines.

The limit stays **64 instances / 16 KiB**, without storage-buffer features or
increased device limits. Global and consecutive grouping both understand graph
identity. Independent shadow grouping can also pack eligible graph casters, using
the same existing depth shaders and caster alpha semantics. Conservative overlap
ordering remains unchanged.

For comparison:

```rust
renderer.set_shader_graph_instancing_enabled(false);
```

This leaves stock instancing enabled. Changing the switch invalidates the color
plan, occlusion state and shadow preparation/maps; cached graph pipelines can be
reused when it is enabled again. `set_instancing_enabled(false)` still disables
all instancing. Transparent, deformed, text and sprite surfaces remain individual.

## Factory diagnosis and measured results

The existing synthetic 400-build Earth factory produced this final active frame:

| Metric | Individual graphs | Instanced graphs |
| --- | ---: | ---: |
| Frustum-visible surfaces | 1,590 | 1,590 |
| Color draw commands | 401 | **63** |
| Instanced commands | 27 | **34** |
| Surfaces covered by instanced commands | 1,216 | **1,561** |
| Singleton commands | 374 | **29** |
| Graph surfaces instanced | 0 / 346 | **345 / 346** |
| Shader-excluded singletons | 346 | **0** |
| Unique-key singletons | 18 | 19 |
| Split-group/tail singletons | 10 | 10 |

No transparent, deformed or unsupported-mesh singletons occur in this measured
factory frame. Those paths are covered by separate regressions. The remaining
29 draws are not all removable: 19 lack an eligible visible peer, and 10 belong
to split groups/tails. Raising capacity does not address unique keys.

Three fresh release paired runs used Intel Iris Xe (TGL GT2), Vulkan, Mesa
26.2.3, 1280×800, 12 warmup frames and 60 measured frames per workload/mode.
Both renderers consume the **same retained input**, with normal shadow caches
and stock/global/incremental batching enabled. Mode order alternates each frame.
Simulation/input extraction happens outside the measured renderer path.

The following are **medians of the three per-run medians**, not pooled samples:

| Workload | Final color commands | Renderer CPU ms | Synchronized renderer ms | GPU pass sum ms |
| --- | ---: | ---: | ---: | ---: |
| Active | 401 → **63** | 4.427 → **3.589** | 16.684 → **14.915** | 10.914 → 10.456 |
| Moving camera | 463 → **78** | 6.714 → **5.064** | 17.446 → **15.094** | 10.569 → 9.768 |
| Frozen | 401 → **63** | 5.461 → **4.159** | 15.365 → **13.971** | 9.398 → 9.214 |

Active encoding falls from 0.228 to 0.097 ms and submission from 1.515 to
0.786 ms. The active renderer CPU reduction is approximately 19%; camera and
frozen reductions are approximately 25% and 24%. All three individual runs show
lower CPU medians with graph instancing. Scheduling/clock variation is noticeable,
particularly in the first moving-camera run; absolute timings are hardware- and
fixture-specific. GPU pass sums are neither frame time nor FPS, and the smaller
GPU differences should not be generalized to other adapters/workloads. The
synchronized measurement includes drawing and waiting, but excludes extraction,
simulation, presentation and GUI work.

All nine workload runs match exact pixels at four camera headings, submitted
color geometry and gameplay/checkpoint state. Submitted shadow triangle counts
also happen to match in these runs; the harness reports rather than assumes
this. In other scenes graph grouping can change whether the static-depth-copy
heuristic is profitable, so one mode may submit additional static casters or a
utility depth-copy triangle while producing identical shadow pixels. A separate
motion parity test disables shadow preparation caching to compare identical
caster geometry directly.

Local raw JSON, PPM captures, per-frame samples, test logs and the three-run
summary are under `work/shader-graph-batching/`. Those are ignored generated
artifacts, not source fixtures. `summary.json` contains the median-of-medians
summary; `paired-1`, `paired-2` and `paired-3` contain the original reports.

## Verification and reproduction

Regular renderer tests cover singleton-reason precedence/partitioning, hidden
peers, capacity tails, graph identity and the diagnostic switch, basic/PBR WGSL
validation with baseline capabilities, exact graph versus individual pixels,
clock/material edits, mirrored transforms, model/image republication, coplanar
winners, transparency, skinning, shadows, auxiliary/temporal outputs and warm
switching. The existing retained-plan regressions remain enabled.

```sh
cargo test --release --locked -p bozzard-render -- --test-threads=1
cargo clippy --locked -p bozzard-render -p bozzard-editor \
  -p bozzard-editor-app --all-targets -- -D warnings
cargo fmt --all -- --check
```

Exact factory motion parity, including authoritative gameplay/checkpoints:

```sh
cargo test --release --locked -p bozzard-editor --test retained_render \
  graph_instancing_factory_pixels_and_checkpoints_match_during_motion \
  -- --ignored --exact --nocapture --test-threads=1
```

Three paired profiles, requiring a hardware graphics adapter:

```sh
for trial in 1 2 3; do
  BOZZARD_GRAPH_INSTANCING_OUTPUT="$PWD/work/shader-graph-batching/paired-$trial" \
    cargo test --release --locked -p bozzard-editor --test retained_render \
      profile_earth_factory_graph_instancing \
      -- --ignored --exact --nocapture --test-threads=1
done
```

The output path is absolute because Cargo runs integration tests from the package
directory. The fixture uses in-memory gameplay and does not access user saves.
