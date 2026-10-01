# Performance measurements

For the September 2026 optimization pass, including shadow reuse, idle editor drawing, CPU caches, and before/after results, see [the optimization review](optimization-results.md). For the subsequent live collision, per-light shadow, shader pipeline, render attachment, and editor document work, see [the follow-up review](optimization-followup.md). The recorded measurements below describe an earlier pass.

The benchmarks below separate factory simulation, editor CPU work, and synchronized rendering. Their elapsed CPU or synchronized wall times are not windowed FPS measurements.

## Factory simulation

Run the real factory scene in an empty creative world with a fixed seed, including its normal HUD and production updates:

```sh
cargo run -p bozzard-demo --example benchmark_factory --locked --offline
```

An optional scene path selects a copy of the factory for before/after comparisons. The benchmark warms up for 60 ticks, then reports median/p95 timings for 180 ticks and the recorded CPU stages. Script timings separate read-view preparation, Rhai hooks, and command application. It does not open a window or measure GPU work.

Debug builds optimize Rhai and the `bozzard-scene`/`bozzard-render` engine packages at level 2, retaining debug information and assertions. Player, editor and demo application code remain unoptimized. This applies to ordinary `cargo run` and tests; the first build after changing the profile recompiles those packages. For instruction-by-instruction engine debugging, override the relevant package's optimization level to zero. Release settings are unchanged. Compare the same profile and workload: when a fixed simulation tick exceeds its 16.7 ms budget, repeated catch-up ticks can turn a modest overrun into a much larger frame stall.

## Editor CPU paths

Run the editor example against a scene file. A release build is the intended comparison point:

```sh
cargo run --release -p bozzard-editor --example benchmark_editor --locked --offline -- \
  examples/sponza/scene.json 200
```

The final argument is the number of samples (10–2000; the default is 200). Scene opening is printed as one `load_ms` measurement. Each repeated path performs 10 untimed warm-up iterations, then records wall-clock CPU elapsed time for `render_extract`, collision extraction, selected-surface bounds, center picking, and a grid of pick queries. The output reports the median and p95 in milliseconds. These spans include the Rust work executed by the operation and exclude GPU submission or presentation. The example checks that the scene, dirty state, and undo history are unchanged.

## Synchronized renderer benchmark

The player benchmark requires smoke mode and a scene. `N` is restricted to 1–1000:

```sh
cargo run --release -p bozzard-player --locked --offline -- \
  --smoke --scene examples/sponza/scene.json \
  --hardware --backend metal --benchmark-frames 30 \
  --output work/sponza/performance
```

The benchmark first renders reference, culling, and full state-cache configurations and requires exact pixel equality. It then runs three warm-up iterations and interleaves the configurations for `N` measured frames. For every frame, `cpu_ms`, `prepare_ms`, `encode_ms`, and `submit_ms` come from renderer CPU statistics. `synchronized_wall_median_ms` is an `Instant` span around draw plus an explicit device wait, so it includes CPU work, GPU completion, and wait overhead. It is useful for comparing the same device and workload; it is not FPS or a GPU timestamp.

Keep device, backend, scene, render size, shadow settings, and build mode fixed when comparing runs. Record the printed medians together with surface, visibility, triangle, shadow, and pipeline-bind counts. Those counts are reported from the last measured frame for each mode; the timing fields are medians across the measured frames.

The benchmark’s reference/culling/cache comparison is a diagnostic for renderer correctness and CPU-side cost. It does not establish image quality, power use, GPU occupancy, or a windowed presentation rate. For Sponza setup and the ignored dataset location, see [the Sponza reproduction](sponza.md).

## Scene-wide opaque batching

The scene renderer groups visible repeated meshes across intervening model parts,
using the same mesh, texture, lighting eligibility and stock shader flavor. Each indexed draw packs up
to 64 instances within the portable 16 KiB uniform limit. Shared frame constants
leave each instance with a 256-byte record; changed records upload independently.
A cached plan also retains grouping through modest movement and orthographic
camera changes when conservative ordering checks remain valid. Visibility,
geometry or grouping changes and uncertain projections rebuild the plan.
Packed instances reuse their buffers; individual uniform uploads are deferred
until a color or shadow draw needs them.

Conservative projected bounds and depth intervals retain ordering dependencies
for potentially coincident samples. Orthographic views also check world bounds,
expanded by the inverse camera's projection-roundoff footprint, so physically
separate objects need not retain false projected overlaps. This preserves
coplanar winners. Transparent objects keep their back-to-front individual draws;
custom shaders and deformed meshes remain individual. Shadow maps group compatible
opaque, lit casters independently of camera visibility, including offscreen
objects. Partial light frusta draw contiguous accepted instance ranges without
repacking the shared buffer or submitting rejected triangles.
Occlusion tests enclose every member of a potentially nonconsecutive batch.

Compare the previous consecutive batcher with scene-wide grouping using real game
assets in six loaded regions, including 324 multipart machine prefabs:

```sh
cargo test --release --offline -p bozzard-editor --test earth_factory \
  profile_earth_factory_scene_batching -- --ignored --exact --nocapture
```

The benchmark interleaves both modes on the same simulated frames, warms up for
12 frames, then reports 60 CPU and synchronized wall samples at 1280 × 800. Four
camera headings must produce exact matching pixels and triangle counts. These
renderer times exclude simulation and window presentation.

For an interleaved-mesh stress test, including all-moving instances:

```sh
cargo test --release --offline -p bozzard-render --test instancing \
  scale_benchmark -- --ignored --exact --nocapture
```

September 30 local release measurements, before the shared-uniform pass and with
32-instance batches, on Intel Iris Xe / Vulkan, using the
six-region fixture above (419 visible items / 1,430 visible surfaces):

| Batching | Color draw commands | Renderer CPU median / p95 | Synchronized wall median |
| --- | ---: | ---: | ---: |
| Previous consecutive runs | 1,356 | 9.665 / 13.086 ms | 23.474 ms |
| Scene-wide grouping | 80 | 3.762 / 6.248 ms | 10.844 ms |

The measurements preserve triangle counts and exact pixels at all four camera
headings. Four indoor/outdoor/window/door captures also remain pixel-identical to
the earlier foundation previews. Native 180-frame runs of the same fixture at
1024 × 640 reduced median whole-frame CPU from **8.201 to 5.459 ms**, and median
presentation interval from **18.239 to 16.740 ms** (about 55 to 60 FPS). Presentation
p95 changed from **36.507 to 33.701 ms**; startup outliers remain. GPU-pass medians
were similar (**8.165 / 8.047 ms**). These are local measurements of this fixture,
not a frame-rate guarantee for other saves, hardware or resolutions.

For the October 1 shared-uniform, batch-capacity and partial-upload work, including
incremental ordering checks, conservative local-light masks, and repeatable benchmarks, see
[batch renderer optimizations](batch-renderer-optimizations.md).

## Independent shadow batching

The October 1 follow-up batches sun, spot, and point shadow casters independently
of color-pass ordering. Cached shadow maps still skip unchanged work. The
400-build release fixture exercises updated maps during an active wind gust,
using the ordinary simulation hook budget and real factory assets. Three paired
runs at 1280 × 800 on Intel Iris Xe / Vulkan compare the former fallback with the
new groups; each run uses twelve warm-up frames and sixty interleaved samples.
Exact pixels and submitted triangle counts match at all four camera headings.

| Shadow mode | Shadow draws | Renderer CPU median | Synchronized median | GPU pass median |
| --- | ---: | ---: | ---: | ---: |
| Former individual fallback | 1,702 | 11.110 ms | 26.167 ms | 13.138 ms |
| Independent batches | 710 | 9.856 ms | 24.083 ms | 13.148 ms |

These are medians of three run medians. Shadow draws fall by 58%, CPU time by 11%,
and synchronized time by 8%; GPU pass time is essentially unchanged. Both modes
retain 401 color draws, 780 visible items, and 1,590 visible surfaces. GPU values
sum render/compute pass timestamps; synchronized values include a device wait.
Neither measures windowed FPS. Separate shadow instance buffers add 16 KiB per
active group and retain at most eight spare buffers. See the
[implementation and validation notes](batch-renderer-optimizations.md#independent-shadow-batches)
for eligibility, caching, and measurement limits.

```sh
cargo test --release --offline -p bozzard-editor --test earth_factory \
  profile_earth_factory_shadow_batching -- --ignored --exact --nocapture
```

## Early frustum acceptance

Camera and local-shadow culling now accept a bound after its first corner when
that corner is accepted by all six homogeneous planes. Other bounds retain the
original plane-major rejection checks over the remaining corners, with exact
distances and tolerance. A separate fallback keeps its scratch storage out of
the small acceptance path. A diagnostic restores the original predicate.

Three paired release runs of the same 400-build, active-gust factory at
1280 × 800 on Intel Iris Xe / Vulkan report medians of run medians:

| Predicate | Renderer CPU median / p95 | Preparation median | Synchronized median | GPU pass median |
| --- | ---: | ---: | ---: | ---: |
| Original eight-corner scan | 9.724 / 12.223 ms | 7.323 ms | 24.219 ms | 13.099 ms |
| First-corner acceptance | 9.244 / 12.604 ms | 6.571 ms | 23.900 ms | 13.148 ms |

CPU median improves by 5% and preparation by 10%; GPU time is essentially
unchanged. p95 is slightly higher, so no tail-latency gain is established.
Both modes retain 401 color draws and 710 shadow draws, and match exact captures
and triangles at all four camera headings. A 50,000-case regression checks the
original predicate; the full native renderer suite passes 56 tests. See
[the frustum methods and measurements](batch-renderer-optimizations.md#early-frustum-acceptance)
for individual runs, isolated inside/outside/near-plane workloads, and reproduction.

## Recorded Sponza measurements

The editor picking comparison used 200 same-process samples, alternating BVH and linear traversal order for each paired measurement. The reported values use midpoint medians in milliseconds; p95 values, when printed by the example, use nearest-rank selection. The wider 1,681-ray checks were untimed and compared object and surface identities against the linear oracle.

| View | Center BVH | Center linear | Grid BVH | Grid linear |
| --- | ---: | ---: | ---: | ---: |
| Corridor | 0.006667 | 1.309292 | 0.009834 | 1.295583 |
| Atrium | 0.006646 | 1.330771 | 0.011355 | 1.321001 |
| Overview | 0.005750 | 1.316250 | 0.009708 | 1.290750 |

The wider checks recorded exact object/surface identity agreement for 1,681 corridor hits, 1,675 atrium hits, and 1,661 overview hits (5,043 rays total). The BVH contains 262,267 triangles, 65,781 nodes, and 3,154,060 resident bytes; construction took 35.4–36.6 ms. A successful mesh replacement builds its index once; catalog clones, undo/redo, and duplicates reuse it. A failed reload keeps the matching last-good index. Picking is an interaction path, so these measurements do not imply a viewport frame-rate improvement. The original ECS-cache hypothesis was rejected because the measured editor operation baseline was about 0.003 ms outside picking.

For context, an Apple M2 Pro/Metal release renderer run at 800×500 with a 4096 shadow map and 100 frames reported these medians:

| Mode | CPU | Synchronized wall | Prepare | Encode | Submit |
| --- | ---: | ---: | ---: | ---: | ---: |
| Reference | 0.678 ms | 3.286 ms | 0.378 ms | 0.275 ms | 0.018 ms |
| Optimized | 0.644 ms | 3.319 ms | 0.368 ms | 0.249 ms | 0.019 ms |

The optimized mode retained 89 of 103 color surfaces and one pipeline bind while retaining all 103 shadow draws. The close synchronized wall medians do not establish a renderer timing gain. Editor CPU paths and the renderer loop are measured separately; full viewport UI cost and GPU timestamp data are outside this report.

The documented results were validated locally on the Mac test host with workspace tests, formatting, all-target Clippy with warnings denied, headless checks, release native editor smoke, and the release native player graphics suite plus Sponza benchmark. Sixteen before/after PPM diagnostics, including loaded Sponza output, were byte-identical. These are local validations for the unpushed work and do not represent CI results.
