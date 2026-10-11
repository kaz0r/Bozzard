# Batch renderer optimization plan

The renderer already uses indexed meshes and batches compatible opaque surfaces across the scene. This follow-up targets CPU preparation and uniform uploads while preserving visibility, depth ordering, shadows, materials, and temporal effects.

## Work list

- [x] **Share frame uniforms.** Camera matrices, lighting, fog, viewport, and the graph clock use one 320-byte frame buffer. The 256-byte instance records retain object transforms, material overrides, and previous object transforms. Camera and daylight changes no longer rebuild or upload stationary instances.
- [x] **Retain more of the batch plan.** Stable draw groups reuse their order through modest object movement and orthographic camera changes. Check only affected ordering dependencies. Geometry, visibility, material eligibility, new overlap constraints, and uncertain projections trigger a rebuild.
- [x] **Increase batch capacity.** Up to 64 instances fit exactly within the portable 16 KiB uniform-buffer limit. Lit and unlit objects use separate groups so an unlit member does not force otherwise compatible casters into individual shadow draws. The shadow pass below additionally batches partial light frusta.
- [x] **Upload changed ranges and reuse buffers.** Adjacent changed records share an upload; untouched records stay resident. Texture changes rebind the existing buffer. Retain at most eight spare buffers (128 KiB) through temporary culling or batch shrinkage.
- [x] **Profile local lighting separately.** Point and spot lights whose range cannot reach a surface are rejected before fragment shading. Conservative masks fit in the existing 256-byte record. A repeatable 400-build factory fixture compares full light loops with the masks, including GPU timestamps and reference captures.
- [x] **Batch shadow casters independently.** Group depth casters independently of camera visibility and color ordering, including offscreen objects. Draw consecutive visible instance ranges within each light's frustum, avoid unused individual uniform uploads, and compare exact pixels, triangle counts, and dense-factory CPU/GPU measurements against the former fallback.
- [x] **Avoid unnecessary frustum corner transforms.** Accept a surface as soon as its first corner rules out every rejection plane. Retain the efficient plane-major fallback and the exact homogeneous distances and relative tolerance; compare the original predicate, CPU workloads, and factory captures.
- [x] **Cache shadow preparation where safe.** Unchanged opaque state skips local-caster scans after receiver-only edits. Identical fitted sun uniforms can reuse the complete map; moving casters render over a copied static depth layer when enough static groups justify it. Exact metadata, asset retirement, fitted projection, target, and failure guards protect reuse.
- [x] **Reuse fitted sun bounds.** Retain per-surface light-space extrema and transform only changed model/local-bound inputs. Compare exact fitted matrix/range/texel bytes with the original loop, and report direct fitting-stage and factory timings.
- [x] **Reduce repeated shadow metadata allocation and classification.** Compare successful-frame metadata directly with current draws, share opaque/full field comparisons when they refer to the same row, and supply the static membership mask in that pass. Refresh retained storage only after submission, copying changed keys and preserving failure invalidation. Profile against full snapshot rebuilding with direct CPU timing and exact factory captures.

- [x] **Diagnose singleton draws and batch opaque shader graphs.** Report mutually exclusive singleton reasons and batch-size histograms before occlusion. Matching mesh/texture/host/lighting/graph-hash keys share the existing 64-instance path; lazy opaque graph variants retire with the bounded graph cache. Transparent and deformed surfaces remain individual. See [shader-graph batching](shader-graph-batching.md) for exact motion parity and repeated paired factory measurements.
- [x] **Keep renderer caches across asset and text changes.** Retire only the cached state that the last successful frame could have used for a replaced or evicted asset, and keep the text and HUD renderers through short gaps. The same pass records mip chains in one command buffer, reuses TAA ping-pong bind groups, streams the temporal frame signature, keeps opaque bundles while particles are live, and resolves meshes once per surface. See [renderer caches](#renderer-caches-across-asset-and-text-changes).
- [ ] **Draw the sky after opaque geometry.** Measured and deferred; see [deferred follow-ups](#deferred-follow-ups).
- [ ] **Trim small per-frame overheads.** Item and override validation, light and GI uniform uploads, and HUD text keys were measured and deferred.
- [ ] **Resolve surface meshes across frames.** Deferred: every geometry retirement would have to reach the cache.

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

### Early frustum acceptance

The camera and local shadow maps share an optimized version of the original homogeneous clip predicate. It transforms the first AABB corner and checks the same six distances with the same relative tolerance. If that corner is accepted by every plane, no plane can reject all eight corners, so the other seven transforms are unnecessary. Otherwise a separate, non-inlined helper transforms those seven corners and retains the reference's plane-major rejection checks. Planes already ruled out by the first corner are skipped. This keeps scratch storage and the longer fallback out of the small acceptance path without changing the conservative visibility decision.

No clip distances, tolerance, shadow settings, draw grouping, or scene contents change. `SceneRenderer::set_frustum_early_acceptance_enabled(false)` selects the original eight-corner predicate for CPU and exact-image comparisons. A deterministic regression compares 50,000 affine/projective cases, including mirrored and scaled transforms, then tangent/near-plane boundaries, very small and large homogeneous coordinates, and non-finite matrices.

The full native renderer suite passes **56 tests**, with five explicitly manual tests skipped, using `--test-threads=1`. Renderer all-target and factory-target Clippy checks pass with warnings denied and `--no-deps`. Formatting and diff checks pass, and release player and editor executables are rebuilt. The factory profile now additionally reports preparation, encoding, and submission medians; those medians need not sum to the median of total CPU time.

Reproduce the paired factory and isolated CPU comparisons with:

```sh
cargo test --release --offline -p bozzard-editor --test earth_factory \
  profile_earth_factory_frustum_acceptance -- --ignored --exact --nocapture
cargo test --release --offline -p bozzard-render --lib \
  frustum_predicate_benchmark -- --ignored --nocapture
```

#### Frustum release measurements

The same Intel Iris Xe / Vulkan host, Mesa 26.2.3, 1280 × 800, 400-build active-gust factory compares only the original and optimized frustum predicates. Both retain independent shadow batching, global color batching, and local-light masks. Three paired runs use twelve warm-up frames and sixty alternating measured frames each. Every run matches exact pixels and submitted color/shadow triangles at all four camera headings, and has sixty complete GPU timestamp samples per mode.

| Frustum predicate | Renderer CPU median / p95 | Preparation median | Encoding median | Submission median | Synchronized median | GPU pass median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Original eight-corner scan | 9.724 / 12.223 ms | 7.323 ms | 0.310 ms | 1.948 ms | 24.219 ms | 13.099 ms |
| First-corner acceptance with plane fallback | 9.244 / 12.604 ms | 6.571 ms | 0.316 ms | 1.868 ms | 23.900 ms | 13.148 ms |

Values are medians of the three run medians; p95 is the median of the three run p95 values. Median renderer CPU falls by **5%**, preparation by **10%**, and synchronized time by **1%**. GPU time is essentially unchanged. The p95 value is slightly higher, so this does not establish a tail-latency improvement. Both modes retain 780 visible items, 1,590 visible surfaces, 401 color draws, 710 shadow draws, and 10,134 of 50,880 object/light pairs. Synchronized time includes a device wait; GPU values sum measured passes. These are renderer comparisons, not windowed FPS.

The paired CPU medians vary across the runs:

| Run | Original CPU median | Optimized CPU median | Original / optimized CPU p95 |
| --- | ---: | ---: | ---: |
| 1 | 9.976 ms | 8.980 ms | 12.094 / 15.246 ms |
| 2 | 9.724 ms | 9.244 ms | 12.223 / 12.604 ms |
| 3 | 9.350 ms | 9.335 ms | 12.239 / 11.614 ms |

The isolated CPU fixture uses 1,024 transformed bounds per workload, ten warm-up samples, sixty alternating measured samples, and sixteen repeats per sample. The table gives medians of three run medians on the same host, including the function-call and benchmark-loop overhead. Accepted counts match in every workload; the separate randomized test compares individual decisions.

| CPU workload | Original predicate | Optimized predicate |
| --- | ---: | ---: |
| Fully inside | 22.631 ns/check | 5.132 ns/check |
| Fully outside | 26.435 ns/check | 25.605 ns/check |
| Crossing the near plane | 24.579 ns/check | 25.612 ns/check |
| Mixed perspective placements | 26.926 ns/check | 21.441 ns/check |

Inside checks are about 4.4 times faster; outside checks retain similar throughput. Near-plane crossing is about one nanosecond slower in this fixture. The separate fallback avoids the substantial rejected-object slowdown observed in the initial whole-corner-mask experiment. No heap allocation or retained per-surface storage is added.

### Static sun depth and shadow preparation

October 1, 2026: moving wind streaks in the factory are opaque lit cubes. They still need current depth, so freezing the complete map would leave incorrect shadows. The renderer now retains a separate depth map for unchanged opaque geometry, copies that depth into the sampled sun map, then renders the changed casters. The fullscreen copy writes exact texel depth through `textureLoad`; it is inside a timestamped render pass and contributes one draw and one triangle to the shadow counters. Color rendering, sun fitting, resolution, bias, and shadow sampling remain unchanged.

Reuse requires exactly matching fitted sun uniform bytes and an unchanged target. Static membership compares model, mesh, texture, UV scale, opacity, alpha cutoff, lit eligibility, and deformation state against the successful preceding frame. Deformed meshes stay dynamic. Static depth also validates its own retained membership and caster metadata; starting or stopping movement rebuilds it when necessary. Replacing or evicting an asset clears the cache when the last successful frame used it as a shadow input, even if the ID is reused. A new ID, or an asset that frame did not use, leaves it intact (see [renderer caches](#renderer-caches-across-asset-and-text-changes)). Resizing/disabling the sun target or disabling the diagnostic cache releases it. Cache stamps publish only after successful submission; failed frames cannot publish partially updated state.

The copy path requires at least 64 static surfaces, at least 32 groups containing static surfaces, more static than dynamic surfaces, and at least one dynamic caster. Smaller scenes keep the full depth pass. The cache retains one extra `Depth32Float` texture: **16 MiB at 2048²**, or 64 MiB at 4096², plus metadata proportional to its static surfaces. Only the 2048² factory configuration is timed below; the group threshold is a heuristic, and other scene/resolution/device combinations need measurement. A rebuild adds another depth pass before the copy; steady-state timings do not describe that rebuild cost.

Separately, unchanged opaque state and identical fitted sun bytes can reuse the complete sun map even when transparent receivers changed. Unchanged local casters skip per-map membership scans, while existing projection, bias, resolution, light-slot and asset guards still invalidate affected maps. `SceneRenderer::set_shadow_preparation_caching_enabled(false)` restores full preparation and sun depth rendering for comparison, retaining the earlier whole-frame shadow cache. New `FrameStats` fields report local scans avoided, fitted-sun reuse, static/dynamic caster counts, static cache reuse, and depth-copy count.

The targeted native tests compare exact pixels through moving shadows, static start/stop, membership reorder, UV and lit edits, same-ID texture/model publication, fitted-bound changes, resized maps, point/spot edits, failed-frame retry, mode switches, and the older partial-group fallback. The full native renderer suite passes **59 tests**, with five manual tests skipped, using `--test-threads=1`. Renderer all-target and factory-target Clippy checks pass with warnings denied and `--no-deps`. Edited-file formatting and diff checks pass, and release player/editor executables are rebuilt.

#### Cached-depth release measurements

Same Intel Iris Xe / Vulkan host (Mesa 26.2.3), 1280 × 800, 2048² sun map, 400-build active-gust fixture. Three paired release runs each use twelve warm-up frames and sixty alternating measured frames, with profiling enabled. Both modes retain the same batching, culling, local-light masks, materials and scene. Exact color captures match at all four camera headings in every run, and all sixty GPU timestamp samples per mode are complete. Submitted shadow triangles intentionally fall when cached depth replaces geometry; color triangles remain identical.

| Shadow preparation | Sun depth draws | Renderer CPU median / p95 | Preparation median | Encoding median | Submission median | Synchronized median | GPU pass median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Full preparation and sun depth | 710 | 9.738 / 11.858 ms | 6.947 ms | 0.324 ms | 1.978 ms | 24.082 ms | 13.059 ms |
| Static depth copy and dynamic casters | 2 | 9.372 / 12.653 ms | 7.579 ms | 0.188 ms | 1.265 ms | 21.662 ms | 11.248 ms |

Values are medians of three run medians; p95 is the median of the three run p95 values. Sun draw commands fall by **99.7%**, total GPU pass time by **14%**, synchronized time by **10%**, and median renderer CPU time by **4%**. Preparation rises by **9%** and CPU p95 by **7%**, so this establishes neither a preparation nor a tail-latency improvement. Per-stage medians need not sum to total CPU medians. The steady-state cache contains **2,883 static casters** and draws **8 moving wind cubes** after one depth copy. Both modes retain 780 visible items, 1,590 visible surfaces, 401 color draws, and 10,134 of 50,880 object/light pairs. This fixture has **zero active local shadow maps**, so its timing gain comes from the sun cache; no factory speedup is attributed to local-scan reuse.

| Run | Full / cached CPU median | Full / cached CPU p95 | Full / cached synchronized median | Full / cached GPU pass median |
| --- | ---: | ---: | ---: | ---: |
| 1 | 9.817 / 9.372 ms | 14.019 / 12.351 ms | 24.602 / 21.662 ms | 13.086 / 11.248 ms |
| 2 | 9.738 / 9.442 ms | 11.858 / 12.653 ms | 24.080 / 21.454 ms | 13.059 / 11.100 ms |
| 3 | 9.735 / 8.997 ms | 11.660 / 13.017 ms | 24.082 / 21.890 ms | 13.035 / 11.255 ms |

The separate 25-caster point/spot regression verifies **175 local caster checks → 0** after transparent-receiver edits, with all seven maps reused and exact pixels preserved. That is a work-count/correctness result, not a local-map timing claim. Synchronized factory time includes an explicit device wait; GPU values sum measured passes including the depth copy and exclude gaps. These measurements are local renderer comparisons, not windowed FPS.

```sh
cargo test --release --offline -p bozzard-editor --test earth_factory \
  profile_earth_factory_shadow_preparation -- --ignored --exact --nocapture
cargo test --offline -p bozzard-render --test instancing \
  static_sun_depth_matches_full_render_through_moving_casters_and_invalidations -- --exact
cargo test --offline -p bozzard-render --test instancing \
  unchanged_local_shadow_casters_skip_scans_and_preserve_invalidation -- --exact
```

### Retained sun-fit bounds

The directional fitter reuses the mesh bounds already collected by the renderer and retains each lit surface's light-space extrema. Model and local-bound keys compare floating-point **bits**, including signed zero; a changed sun-view matrix invalidates every retained extent. Lit eligibility clears an excluded slot. Retiring an asset that the last frame used as a shadow input clears the cache. Reordering, insertion, and deformation are safe because reuse depends on the actual current local bounds and model at each slot, rather than an asset ID. Resolution changes still recompute the fitted projection using retained extrema.

Changed surfaces retain the original eight-corner loop and its two separate transforms (`model`, then sun view). Combining those transforms first would change rounding. Global extrema reduce in original surface order; projection, texel snapping, range and bias calculations keep their original expressions. Any non-finite transformed corner falls back to the original complete reduction, preserving its NaN/infinity behavior. These are pure CPU values, so a failed frame cannot publish incorrect GPU state through this cache. The earlier depth-map submission guards remain in effect.

The allocation retains one scalar entry per current surface, with no per-entry heap allocation. It requests shrinking after large count reductions; the measured factory allocation is **335,356 bytes (about 328 KiB)**. Disabling `set_sun_fit_caching_enabled` releases it, as does retiring an asset that the last frame used as a shadow input. `FrameStats` reports fitting CPU time, reused/recomputed counts, fallback, and allocated bytes. The diagnostic restores the original whole-scene corner loop, including its original mesh lookup path; both modes retain the static sun depth cache from the preceding pass.

A deterministic test compares exact fitted matrix, range, and texel-size bits through **5,000 edit sequences** over 64 surfaces, including translation, rotation, mirrored/nonuniform scale, local-bound edits, lit changes, reorder, vertical/opposite/nearly vertical sun views, and changing resolution. Separate zero/signed-zero, tiny/large, empty, overflow/fallback and recovery cases pass. Native static-depth captures compare the original fitter with cached fitting while moving eight casters and verify the expected 249 reused and eight recomputed bounds in that fixture. The full native renderer suite passes **60 tests**, with five manual tests skipped, serially. Renderer all-target and factory-target Clippy pass with warnings denied and `--no-deps`; formatting and diff checks pass, and release player/editor executables are rebuilt.

#### Sun-fitting release measurements

Same Intel Iris Xe / Vulkan (Mesa 26.2.3), 1280 × 800, 2048² sun map, 400-build active-gust factory. Three instrumented paired release runs use twelve warm-up frames and sixty alternating measured frames each. Exact captures and color/shadow triangle counts match at four camera headings in every run; all sixty GPU timestamp samples per mode are complete. Compilation and other validation finish before each measured run.

| Fitter | Direct fit CPU median | Renderer CPU median / p95 | Preparation median | Encoding median | Submission median | Synchronized median | GPU pass median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Original corner loop and mesh lookup | 0.185437 ms | 9.251 / 12.209 ms | 7.525 ms | 0.190 ms | 1.292 ms | 21.649 ms | 11.284 ms |
| Retained light-space extrema | 0.056986 ms | 8.251 / 12.705 ms | 6.279 ms | 0.198 ms | 1.302 ms | 21.434 ms | 11.186 ms |

Values are medians of three run medians; p95 is the median of the three run p95s. **Direct fit time falls 69% (about 0.13 ms per updating frame)**. The factory recomputes **8 bounds instead of 2,891**, reusing 2,883. All modes retain 780 visible items, 1,590 visible surfaces, 401 color draws, two shadow draws, one sun depth copy, and 10,134 of 50,880 object/light pairs, with no individual object uniform writes. GPU time is essentially unchanged, as expected for identical submitted work.

The instrumented runs show 11% lower total renderer CPU median and 17% lower preparation median, but these larger changes vary beyond the direct fitting savings and should not all be attributed to the fitter. Synchronized time changes by only 1%, and CPU p95 rises 4%; no tail-latency improvement is established. Two preliminary runs before adding direct-stage timing showed smaller total CPU improvements and one preparation regression, recorded below. The stable result is reduced fitting work and its directly measured cost. Stage medians need not sum to total CPU medians; synchronized time includes a device wait, GPU values sum measured passes, and neither is windowed FPS.

| Instrumented run | Full / cached fit median | Full / cached CPU median | Full / cached CPU p95 | Full / cached preparation | Full / cached synchronized | Full / cached GPU passes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.181062 / 0.057885 ms | 9.400 / 8.568 ms | 12.124 / 13.756 ms | 7.662 / 6.729 ms | 21.649 / 21.618 ms | 11.284 / 11.186 ms |
| 2 | 0.187258 / 0.056860 ms | 9.222 / 8.251 ms | 14.483 / 11.570 ms | 7.525 / 6.166 ms | 21.744 / 21.434 ms | 11.369 / 11.182 ms |
| 3 | 0.185437 / 0.056986 ms | 9.251 / 8.120 ms | 12.209 / 12.705 ms | 7.505 / 6.279 ms | 21.524 / 21.327 ms | 11.189 / 11.207 ms |

| Preliminary run, without direct-stage timing | Full / cached CPU median | Full / cached preparation | Full / cached synchronized | Full / cached GPU passes |
| --- | ---: | ---: | ---: | ---: |
| 1 | 9.054 / 8.368 ms | 6.892 / 6.703 ms | 21.851 / 21.657 ms | 11.350 / 11.333 ms |
| 2 | 8.859 / 8.717 ms | 6.674 / 7.253 ms | 21.807 / 21.226 ms | 11.111 / 11.169 ms |

```sh
cargo test --release --offline -p bozzard-editor --test earth_factory \
  profile_earth_factory_sun_fit -- --ignored --exact --nocapture
cargo test --offline -p bozzard-render --lib \
  cached_extents_match_original_fit_bytes_through_edits -- --nocapture
```

### Retained shadow metadata and direct classification

Shadow-state checks now compare the previous successful snapshot directly with current prepared draws. A single ordered traversal supplies full-frame, sun, opaque-caster and static-membership results. The full and opaque iterators share a field comparison when they reference the same stored row; transparent receiver insertion/removal can shift them, so those rows retain separate exact comparisons. A changed opaque count or culling mode clears static membership conservatively, matching the previous classifier.

After successful submission, the existing snapshot retains matching rows and refreshes changed rows. Mesh/texture keys clone only when changed; scalar depth fields update independently. New rows use the original constructor, removed rows drop, and large reductions request shrinking. The feature keeps one retained snapshot rather than building another complete snapshot while the previous one is live. Classification uses two temporary boolean arrays proportional to the draw count, with no additional GPU texture or instance buffer.

The snapshot is still removed before shadow writes. Any later failure drops the unpublished candidate and forces complete revalidation on retry. Retiring an asset that the last frame used as a shadow input drops the retained snapshot, even when IDs match. `set_shadow_metadata_reuse_enabled(false)` restores original construction/comparison/retirement; disabling general state caching also selects that path. `FrameStats` exposes snapshot comparison/classification/construction/retirement CPU time and counts of built, refreshed and retained caster records, plus mesh/texture clone calls. Those calls are work counts, not allocator counts; primitive keys can clone without allocation.

A deterministic 5,000-sequence test compares the direct classifier against original snapshot equality, sun equality, opaque equality and static masks, then compares refreshed storage with a freshly constructed snapshot. It covers model motion, transparency, lit changes, UVs, texture/mesh keys, deformation, opacity/cutoff, reorder, insertion/removal, sun/bias/culling edits and local-light activation/order/settings. The native sun/local regressions compare rebuilding against retained storage through geometry/material edits, publications, failure/retry and mode changes; the moving-caster fixture verifies record/clone counts. The full native renderer suite passes **61 tests**, with five manual tests skipped, serially. Renderer all-target and factory-target Clippy pass with warnings denied and `--no-deps`; formatting/diff checks pass, and release player/editor are rebuilt.

#### Metadata release measurements

Intel Iris Xe / Vulkan (Mesa 26.2.3), 1280 × 800, 2048² sun map, the same 400-build active-gust factory. Three paired release runs use twelve warm-up frames and sixty alternating measured frames each, with both modes retaining fitted-bound and static-depth caches. Compilation and validation are outside measurements. Exact pixels and submitted color/shadow triangles match at four camera headings in every run; all sixty GPU timestamp samples per mode are complete.

| Snapshot/classification | Direct state CPU median | Renderer CPU median / p95 | Preparation median | Encoding median | Submission median | Synchronized median | GPU pass median |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Construct new snapshot and compare | 0.575732 ms | 8.435 / 13.016 ms | 6.822 ms | 0.194 ms | 1.340 ms | 21.286 ms | 11.078 ms |
| Compare current draws and refresh retained rows | 0.193701 ms | 7.728 / 11.547 ms | 6.106 ms | 0.189 ms | 1.284 ms | 20.630 ms | 11.241 ms |

Values are medians of three run medians; p95 is the median of the three run p95s. **Direct state work falls 66% (about 0.38 ms)**, median renderer CPU time **8%**, preparation **10%**, and synchronized time **3%**. GPU pass time is about 1.5% higher with identical submitted work; this establishes no GPU speedup. Median CPU p95 is 11% lower, but one run has a small rise, so tail improvement is not universal. Stage medians need not sum to total CPU medians; synchronized values include a device wait and GPU values sum measured passes, excluding gaps. These are local renderer comparisons rather than windowed FPS.

| Measured work per frame | Rebuilt snapshot | Retained snapshot |
| --- | ---: | ---: |
| New caster records | 2,891 | 0 |
| Refreshed caster records | 0 | 8 |
| Reused caster records | 0 | 2,883 |
| Mesh/texture clone calls | 5,782 | 0 |

Both modes retain 780 visible items, 1,590 visible surfaces, 401 color draws, two shadow draws, one sun depth copy, 10,134 of 50,880 object/light pairs, and zero individual object-uniform writes. Fitting medians remain near 0.053 ms in both modes. Cold/publication frames still build all needed metadata; the table describes warmed steady state.

| Run | Full / retained state CPU | Full / retained CPU median | Full / retained CPU p95 | Full / retained synchronized | Full / retained GPU passes |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.575732 / 0.193488 ms | 8.713 / 7.728 ms | 11.785 / 11.884 ms | 21.360 / 20.630 ms | 11.074 / 11.165 ms |
| 2 | 0.589288 / 0.198170 ms | 8.435 / 7.902 ms | 14.575 / 11.547 ms | 21.079 / 20.568 ms | 11.233 / 11.241 ms |
| 3 | 0.554153 / 0.193701 ms | 8.203 / 7.659 ms | 13.016 / 11.472 ms | 21.286 / 20.735 ms | 11.078 / 11.303 ms |

```sh
cargo test --release --offline -p bozzard-editor --test earth_factory \
  profile_earth_factory_shadow_metadata -- --ignored --exact --nocapture
cargo test --offline -p bozzard-render --lib \
  direct_classification_and_retained_metadata_match_original_snapshots -- --nocapture
```

### Renderer caches across asset and text changes

October 10, 2026. Before this pass, every publication called a full invalidation,
even for an ID the renderer had never seen. This covered `remove_asset`, the
synchronous uploads and `PendingUpload::finish`. The invalidation dropped:

- surface preparation and occlusion;
- every object and instance binding;
- the shadow depth plan and metadata snapshot;
- the static sun depth and fit;
- every spot and point map.

A streaming world therefore did all of the following whenever an unrelated asset
arrived or was evicted:

- re-rendered its shadows;
- reallocated per-surface uniform buffers;
- re-prepared every item;
- recompiled its render bundle.

Text appearing, disappearing or growing its atlas did the same.

Retirement now follows what the last successfully submitted frame used:

- **New IDs.** A frame that draws a missing ID fails validation before it caches anything. Publishing a new ID therefore invalidates nothing.
- **Bind groups.** Replacing or evicting an asset releases only the object, portable and native instance, shadow singleton and native, and compacted shadow range bindings that captured its view. The compacted range caches also compare the view they bound. HUD bindings are rebuilt after any retirement. Render bundles and merged world text compare resource identities themselves.
- **Surface preparation.** Expansion compares asset IDs, not contents. It is rebuilt when one of the last frame's source items or surfaces referred to the asset. An item that names a model surface the resident model lacks also counts, because a replacement model can add that surface.
- **Shadow caches.** The depth plan, metadata snapshot, static sun depth, sun fit and local maps are rebuilt when the asset was a *shadow input*: a texture sampled by a lit opaque caster, or geometry on any lit surface. Lit transparent receivers write no depth, but their bounds extend the fitted sun box.
- **Occlusion.** Retired geometry still resets occlusion snapshots, which key depth by mesh ID.
- **Unknown history.** If state caching is disabled or the last frame failed, every retirement keeps the former full invalidation.
- **Text.** Atlas changes rebind only text users. They refresh shadows only if a lit opaque caster sampled the atlas. The text and HUD renderers release their bound views but stay alive for 60 idle frames before being dropped.

The same pass removes other repeated work:

- Mip chains record into one command buffer per texture or staged upload slice,
  rather than one submission per level (`2fe122f`).
- TAA, exposure, depth of field, bloom and display passes cache bind groups for both
  history targets. `FrameStats::post_bind_groups` reports the bind groups they create
  (`5ad9316`).
- The temporal frame signature uses a streaming hasher built on xxHash rounds. It
  hashes settings field by field and caches each GI probe allocation's hash
  (`a20860a`). A 4,000-edit replay requires the same repeat decisions as the former
  signature.
- Live particles no longer disable bundles and multi-draw for the opaque pass.
  Transparent batches are still drawn between particle buckets (`94d6114`).
- Each surface resolves its mesh once per frame for culling and occlusion
  (`bc7237f`).

A review of the first version found three stale-cache cases, fixed in `f6a8cd7` and
`b1387b9`:

- replaced geometry of a lit transparent receiver;
- a model surface that was named before it existed;
- a compacted shadow range still bound to a replaced texture view.

Each fix has a test that fails without it. The `renderer_caches` tests compare
same-ID replacements, evictions, text growth and these cases against a fresh
renderer.

[Interleaved release measurements](measurements/renderer-caches.md) used seven pairs
on an RTX 3060 / Vulkan. The scene was 640×400 with 1,280 surfaces. Every capture
matches the original renderer byte for byte.

| Workload | Renderer CPU base → branch | Synchronized base → branch | Explaining counters |
| --- | ---: | ---: | --- |
| Unrelated image published each frame | 6.925 → 0.951 ms | 9.073 → 1.183 ms | Shadow maps 1 → 0, surface records 1,280 → 0, bundle compilations 1 → 0 per frame |
| HUD label toggled each frame | 9.383 → 1.078 ms | 11.459 → 1.323 ms | As above |
| TAA, play | 2.914 → 1.366 ms | 4.223 → 2.509 ms | Opaque bundle replayed instead of direct encoding; post bind groups 8 → 0 |
| 512 live particles | 1.806 → 1.066 ms | 2.173 → 1.310 ms | Opaque bundle replayed instead of direct encoding |
| Sky behind a wall, no shadows | 0.940 → 0.790 ms | 1.185 → 1.007 ms | Mesh lookups 3,360 → 1,280 |

The temporal frame signature for 4,096 items, 16,384 particles and 16³ GI falls from
3.81 to 0.485 ms. A synchronous upload of eight mipmapped 1024² images falls from
6.03 to 4.73 ms. The commits overlap in several workloads and were not timed
separately.

#### Deferred follow-ups

- **Sky after opaque geometry.** Drawing the sky last with `LessEqual` at depth 1
  would shade only uncovered pixels. Colour pipelines use `Less` against a 1.0 clear,
  with no reversed Z or MSAA, so the depth test is compatible.
  - Temporary GPU timestamps on the RTX 3060 put the sky pass at about 2 µs at
    640×400 and 0.04–0.08 ms at 1080p. The 4K result was inconclusive on a busy GPU.
  - Without particles, transparent surfaces share the opaque render bundle. That
    bundle would have to split around the sky.
  - Deferred until a measured view is limited by sky shading.
- **Small per-frame overheads.** Temporary timers in `renderer_caches` measured each
  of these at under 1–2% of a roughly 1 ms frame:
  - item and override validation: about 6–9 µs per frame for 1,280 items without
    overrides;
  - light and GI uniform uploads with text preparation: a similar 6–9 µs;
  - HUD text keys: negligible with a few labels.
  Scenes with many overrides or HUD labels were not profiled.
- **Mesh resolution across frames.** Surfaces resolve their meshes once per frame;
  in `renderer_caches`, string-keyed lookups fell from 3,758 to 1,280 per frame.
  Caching resolved handles across frames would remove the remaining lookups. Every
  geometry replacement and eviction would then have to reach that cache.
