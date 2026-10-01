# Batch renderer optimization plan

The renderer already uses indexed meshes and batches compatible opaque surfaces across the scene. This follow-up targets CPU preparation and uniform uploads while preserving visibility, depth ordering, shadows, materials, and temporal effects.

## Work list

- [x] **Share frame uniforms.** Camera matrices, lighting, fog, viewport, and the graph clock use one 320-byte frame buffer. The 256-byte instance records retain object transforms, material overrides, and previous object transforms. Camera and daylight changes no longer rebuild or upload stationary instances.
- [x] **Retain more of the batch plan.** Stable draw groups reuse their order through modest object movement and orthographic camera changes. Check only affected ordering dependencies. Geometry, visibility, material eligibility, new overlap constraints, and uncertain projections trigger a rebuild.
- [x] **Increase batch capacity.** Up to 64 instances fit exactly within the portable 16 KiB uniform-buffer limit. Lit and unlit objects use separate groups so an unlit member does not force otherwise compatible casters into individual shadow draws. The shadow pass below additionally batches partial light frusta.
- [x] **Upload changed ranges and reuse buffers.** Adjacent changed records share an upload; untouched records stay resident. Texture changes rebind the existing buffer. Retain at most eight spare buffers (128 KiB) through temporary culling or batch shrinkage.
- [x] **Profile local lighting separately.** Point and spot lights whose range cannot reach a surface are rejected before fragment shading. Conservative masks fit in the existing 256-byte record. A repeatable 400-build factory fixture compares full light loops with the masks, including GPU timestamps and reference captures.
- [x] **Batch shadow casters independently.** Group depth casters independently of camera visibility and color ordering, including offscreen objects. Draw consecutive visible instance ranges within each light's frustum, avoid unused individual uniform uploads, and compare exact pixels, triangle counts, and dense-factory CPU/GPU measurements against the former fallback.
- [ ] **Cache shadow preparation where safe.** Profile rebuilding caster groups and repeating per-light bounds tests. Retain membership or culling results only when geometry, materials, transforms, and the relevant light frustum remain valid; keep bounded storage and the individual reference path.

## Baseline and verification

- Starting instance records: 496 bytes; maximum batch size: 32 instances. This pass reduces records to 256 bytes and raises capacity to 64.
- Existing 324-machine release measurement: 1,356 to 80 color draws and 9.665 to 3.762 ms median renderer CPU time after global batching. See [performance notes](performance.md) for fixture and GPU details.
- Compare optimized output with unbatched output for interleaved meshes, coplanar surfaces, transparency, mirrored transforms, shader graphs, fog, shadows, and temporal effects.
- Measure stationary scenes, camera movement, changing lighting, one moving object, and many moving objects. Report renderer CPU time and synchronized frame time separately; reduced uploads alone do not establish an FPS improvement.
- Record implementation results below and update checkboxes as each item is completed. Do not raise operation limits or relax rendering correctness checks to obtain a performance result.

## Results

October 1, 2026: shared uniforms, larger batches, partial uploads, and bounded buffer reuse are implemented. The shadow cache also tracks changes to an object's lit flag, ensuring those edits refresh the depth maps.

The native renderer suite covers exact reference pixels, batching boundaries, sparse edits, shrink/regrowth, texture rebinding, mirrored transforms, shadows, fog, motion blur, temporal antialiasing, and shader graph Time. The release benchmark additionally compares raw captures against the renderer from commit `d720dde`.

Validation completed: 52 renderer tests passed, with three explicitly manual tests skipped; the six release batching regressions also passed. Renderer Clippy checks are clean. Release player and editor executables were rebuilt.

### Release measurements

Intel Iris Xe / Vulkan (Mesa 26.2.3), 320 × 320, 1,024 alternating cubes and spheres, shadows disabled. Each workload uses ten warm-up frames and eighty measured frames. The table gives the median of three run medians, alternating the original and optimized executable. Every optimized capture matches the original executable byte for byte in these five workloads. CPU time covers renderer work; synchronized time includes an explicit GPU wait and does not measure windowed FPS.

| Workload | Renderer CPU before → after | Synchronized time before → after | Instance uploads before → after |
| --- | ---: | ---: | ---: |
| Stationary | 0.840 → 0.787 ms | 2.239 → 2.247 ms | 0 → 0 bytes |
| Changing daylight and fog | 1.753 → 0.703 ms | 3.261 → 1.947 ms | 507,904 → 0 bytes |
| Moving camera | 2.154 → 1.149 ms | 3.923 → 2.779 ms | 507,904 → 0 bytes |
| One rotating object | 1.347 → 1.115 ms | 2.722 → 2.493 ms | 15,872 → 256 bytes |
| All objects rotating | 2.331 → 1.510 ms | 4.196 → 3.146 ms | 507,904 → 262,144 bytes |

Color draws fall from 32 to 16 in every workload. Daylight/fog and camera changes additionally upload the shared 320-byte frame buffer once. Stationary synchronized time is effectively unchanged. At this first stage, a rotating object still rebuilt the ordering plan despite its small upload.

Reproduce the optimized workload with:

```sh
cargo test --release --offline -p bozzard-render --test instancing \
  uniform_update_benchmark -- --ignored --exact --nocapture
```

Optional `BOZZARD_UNIFORM_REFERENCE_DIR` selects a directory for historical raw captures: an absent file is written, and an existing file must match exactly. Use a separate directory for each fixture/device/backend combination. The initial reference executable used renderer code from `d720dde` with the benchmark added before implementation.

### Actual factory assets

The release 324-machine fixture at 1280 × 800 retains 419 visible items and 1,430 visible surfaces. Global batching uses **55 color draws**, versus 1,356 for consecutive batching in the same run. Exact pixels and triangle counts match at all four camera headings. Shadow draws are zero in this fixture; shadow correctness is covered separately by the renderer regressions.

| Current renderer mode | Renderer CPU median / p95 | Synchronized median |
| --- | ---: | ---: |
| Consecutive batching | 7.705 / 10.982 ms | 15.332 ms |
| Global batching | 2.816 / 5.328 ms | 9.403 ms |

These compare the two batching modes in the current renderer. The older global-batching result of 80 draws is recorded in [performance notes](performance.md); its timings were collected in a separate run.

```sh
cargo test --release --offline -p bozzard-editor --test earth_factory \
  profile_earth_factory_scene_batching -- --ignored --exact --nocapture
```

### Incremental planning and local-light selection

The follow-up keeps batch membership while conservative ordering checks remain valid. Each reordered opaque surface has a bounded movement envelope. Only reversed draw pairs with overlapping envelopes need further checks; a moved surface updates its world bounds and only relevant projected bounds. Orthographic camera changes reuse world-space separation. Perspective changes with reordered draws, escaped envelopes, new overlap constraints, and metadata or visibility edits fall back to a full rebuild. Retained dependency storage is bounded to eight pairs per surface, with a ceiling of 32,768 pairs. Unchanged plans still use the existing fast path.

Surface light masks conservatively test each point/spot range sphere against the transformed mesh bounds, including current skin bounds. Directional lights remain available everywhere. Zero-radiance lights can be excluded. Projective transforms and uncertain bounds keep all lights. Relative padding protects tangent lights and large world coordinates. Two exactly representable 16-bit halves use the reserved instance fields; record size and 64-instance capacity are unchanged.

Shaders visit selected lights in ascending index order. Light arrays and shadow-map slots keep their original numbering. The masks cache independently of positive intensity/color changes, and revisions survive failed frames. This pass reduces surface shading work; it does not remove shadow-map rendering, particle lighting, volumetric lighting, or full rebuilds when visibility changes.

New regressions compare exact pixels with planning disabled and with unbatched/full-light reference paths. They cover small camera and rotor edits, a new coplanar overlap inside the movement envelope, all 32 light slots, hard spotlight cones, tangent lights, imported PBR surfaces, transparency, local shadows, zero radiance, light reordering, large coordinates, projective models, and error/retry cache recovery. The native renderer suite passes 54 tests, with four manual tests skipped; final batching regressions and renderer Clippy also pass. The edited factory test target passes Clippy with `--no-deps`. A broader editor dependency check stops on two existing Clippy warnings in `bozzard-scene` (`manual_is_multiple_of` and `collapsible_if`). Release player and editor executables are rebuilt.

Repeatable lighting measurements:

```sh
cargo test --release --offline -p bozzard-render --test instancing \
  local_lighting_benchmark -- --ignored --exact --nocapture
cargo test --release --offline -p bozzard-editor --test earth_factory \
  profile_earth_factory_local_lighting -- --ignored --exact --nocapture
```

The synthetic fixture uses 1,024 cubes, 32 point lights, 1280 × 800, ten warm-up frames and eighty measured frames. It retains 1,584 of 32,768 object/light candidates (about 4.8%). Its historical capture matches the first-stage executable exactly. The factory fixture places 400 builds across two chunks: 80 generators, 80 power poles, and 240 kilns. It uses normal production updates and the ordinary script-operation budget, alternating full-light and mask modes over the same simulated frames. All four camera headings must match exactly.


#### Follow-up release measurements

The same Intel Iris Xe / Vulkan device and five 320 × 320 workloads compare the first-stage executable (shared uniforms and 64-instance batches) with the final incremental/light-mask implementation. Values are medians of three alternating run medians, collected after compilation completed. All five historical captures still match exactly. Instance uploads and draw counts remain those of the first stage.

| Workload | Renderer CPU first stage → follow-up | Synchronized first stage → follow-up |
| --- | ---: | ---: |
| Stationary | 0.638 → 0.787 ms | 1.816 → 1.987 ms |
| Changing daylight and fog | 0.721 → 0.727 ms | 1.989 → 1.839 ms |
| Moving camera | 1.046 → 0.866 ms | 2.385 → 2.045 ms |
| One rotating object | 1.064 → 0.775 ms | 2.326 → 2.044 ms |
| All objects rotating | 1.502 → 1.248 ms | 3.252 → 2.733 ms |

The final measured frame retains the plan in all five workloads: zero projected bounds updated for camera translation, one world bound for one rotor, and 1,024 world bounds for all rotors. These spatially separated objects need no reversed-pair checks. A coplanar-overlap regression separately verifies that an unsafe retained order rebuilds. Lightless views bypass mask decoding in the shader and skip redundant light-cache writes. Stationary CPU time adds 0.149 ms in this run; daylight CPU time is essentially unchanged. The improvement is concentrated in movement and lighting workloads.

In the synthetic 32-light fixture, renderer CPU changes from **1.934 to 1.110 ms**, and synchronized time from **13.570 to 3.406 ms** (three alternating run medians). The optimized image matches the first-stage image byte for byte. This synchronized time includes CPU submission and the explicit GPU wait; it is not a GPU timestamp or windowed FPS.


The final live factory run at 1280 × 800 keeps **780 visible items, 1,590 visible surfaces, 401 color draws, and 1,702 shadow draws** in both lighting modes. Exact pixels and triangle counts match at all four camera headings. The last measured frame retains **10,116 of 50,880** object/light pairs (about 19.9%). Twelve warm-up frames precede sixty interleaved samples; GPU profiling is enabled in both modes.

| 400-build factory lighting mode | Renderer CPU median / p95 | Synchronized median | Sum of GPU pass timestamps, median |
| --- | ---: | ---: | ---: |
| Full light loop | 16.773 / 33.402 ms | 48.340 ms | 28.940 ms |
| Conservative masks | 17.132 / 36.723 ms | 34.620 ms | 14.416 ms |

GPU pass time falls by about 50%, and synchronized time by about 28%. CPU time is slightly higher in this lighting comparison. GPU values sum measured render/compute passes and exclude gaps between passes; all sixty timestamp samples are complete. Shadow draw counts stay identical, so shadow rendering and CPU preparation remain substantial costs in this dense fixture.

The final 324-machine global/consecutive comparison also passes all four camera captures. It keeps 419 visible items and 1,430 surfaces: 1,356 versus 55 color draws, CPU medians 7.725 versus 2.833 ms, and synchronized medians 14.655 versus 8.167 ms. These are same-run comparisons in the final renderer, with profiling enabled. Release player/editor builds, the native renderer suite, exact historical captures, both real-asset profiles, formatting checks, and renderer Clippy all completed successfully.

### Independent shadow batches

October 1, 2026: shadow maps now group compatible opaque, lit casters independently of camera visibility and color-pass ordering. This includes offscreen casters, which were a substantial source of individual shadow draws in the dense factory. Depth-only writes can be reordered without changing the stored depth, including equal-depth ties. Color ordering and transparency handling retain their existing rules.

Each group holds at most 64 instances in a separate 16 KiB uniform buffer. Per-light culling submits contiguous accepted ranges using the original instance indices, including nonzero starting indices. Rejected instances contribute neither draws nor triangles. Custom shaders, deformed meshes, and other ineligible surfaces retain individual draws; transparent and unlit surfaces retain their previous shadow exclusions. Sun, spot, and point maps use the same path, and unchanged maps still reuse their cached depth textures.

Shadow buffers use the existing changed-range upload and texture-rebinding logic, retaining at most eight spare buffers beyond the active group count. This adds one 16 KiB allocation per active packed shadow group, plus at most 128 KiB of spares, without additional shadow-map textures. Individual object uniforms upload only when a color or shadow singleton needs them. `FrameStats` reports shadow instance upload bytes and buffer allocations separately. `SceneRenderer::set_shadow_batching_enabled(false)` restores the former color-batch/individual fallback for comparison and invalidates cached maps when toggled.

The native regression compares exact pixels and submitted shadow triangles through partial light frusta, nonzero instance ranges, mirrored and unlit edits, moved casters, cache reuse, mode changes, and frames with no camera-visible surfaces. The full native renderer suite passes **55 tests**, with four explicitly manual tests skipped, using `--test-threads=1`. Renderer all-target Clippy and the edited factory test target's Clippy checks pass with warnings denied and `--no-deps`. Formatting and diff checks pass, and both release player and editor executables are rebuilt.

#### Dense-factory release measurements

Intel Iris Xe / Vulkan (Mesa 26.2.3), 1280 × 800, the same 400-build fixture: 80 generators, 80 poles, and 240 kilns across two chunks. The fixture starts at 61 real seconds, within seed 4's first wind gust, so moving ground effects exercise shadow-map updates under the new game clock. Calm frames can reuse shadow maps and would not measure this work.

Three paired release runs each use twelve warm-up frames and sixty interleaved measured frames, alternating mode order. Both modes retain global color batching and conservative light masks. The values below are medians of the three reported run medians; the p95 column is the median of the three run p95 values. Compilation completed before measurement. Exact pixels and color/shadow triangle counts match at all four camera headings in every run.

| Shadow mode | Color draws | Shadow draws | Renderer CPU median / p95 | Synchronized median | Sum of GPU pass timestamps, median |
| --- | ---: | ---: | ---: | ---: | ---: |
| Former individual fallback | 401 | 1,702 | 11.110 / 14.661 ms | 26.167 ms | 13.138 ms |
| Independent batches and visible ranges | 401 | 710 | 9.856 / 13.288 ms | 24.083 ms | 13.148 ms |

Shadow draw commands fall by **58%**, renderer CPU time by **11%**, and synchronized time by **8%**. GPU pass time is essentially unchanged. Both modes retain 780 visible items, 1,590 visible surfaces, and 10,134 of 50,880 object/light pairs. The last measured frame needs zero individual object-uniform writes in both modes, and all sixty GPU timestamp samples per mode are complete. Synchronized time includes an explicit device wait; GPU values sum render/compute passes and exclude gaps. These measurements are local renderer comparisons, not windowed FPS or a guarantee for other hardware and saves.

```sh
cargo test --release --offline -p bozzard-editor --test earth_factory \
  profile_earth_factory_shadow_batching -- --ignored --exact --nocapture
```
