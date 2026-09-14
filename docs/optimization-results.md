# Engine optimization review

Base: `main` at `25d36e8` (includes the merged explorer fix, PR #20). Branch: `codex/engine-performance`. This work is intended for local review before pushing. The separate Sponza showcase PR is not part of this change.

## Changes

1. **Reuse unchanged shadow maps.** The renderer compares the depth-producing state before rebuilding directional, spot, and point shadow maps. Camera and light-color changes can reuse the maps. Geometry, transforms, transparency, UVs, alpha cutoff, shadow settings, and successful asset replacement invalidate the cache. Transparent receivers remain in the key because they affect the directional map's fitted bounds. Invalidated maps render at the original resolution and with the existing culling rules.
2. **Skip redundant object uniform uploads.** Per-object GPU buffers are rewritten only when their exact contents change. The common inverse view/projection, lighting, and fog values are calculated once per frame; packing uses a fixed stack buffer instead of a heap allocation per draw. Shader-graph Time continues to update; stock shaders omit an unused clock value.
3. **Discard unused auxiliary render-target contents.** Normal, motion, and specular buffers are stored only when later passes consume them. TAA/motion blur retain normal and motion; reflections retain normal and specular; particles retain motion coverage. The reference diagnostic still stores all three buffers. Texture allocation sizes and enabled effect settings are unchanged.
4. **Retain the idle editor viewport.** Editor-only UI changes can reuse the scene texture. Scene revision, asset content/residency, camera, viewport size, scale, layer, display settings, and Play state trigger a new draw. Gameplay, live particle/atmosphere animation, temporal effects, and exposure adaptation continue rendering.
5. **Reduce idle UI polling.** Idle housekeeping is scheduled at 500 ms instead of 16 ms. Input requests immediate egui repaint. Loading, GPU uploads, shader preview, camera navigation, Play, and animated viewports keep the faster schedule.
6. **Share an immutable authoring world.** Rendering, picking, collision queries, and framing reuse one world until the document revision changes, avoiding repeated scene validation, cloning, and ECS/simulation setup. Play continues to use its own live world. Repeated authoring queries also preserve object motion identities.
7. **Cache authored collision overlays and remove SAT allocations.** Unchanged edit-mode collision snapshots reuse the computed overlaps and bounds; mesh data remains shared. Play collisions are queried live. Box SAT tests now traverse axes lazily without allocating a vector for every pair or calculating edge-cross axes after an earlier separating axis already rejected the pair.
8. **Cache shader source generation.** A bounded cache retains the 32 most recently used graphs per thread. Exact graph equality guards reuse; edits and invalid graphs go through the compiler. Identical instances share the resulting WGSL source.
9. **Share thumbnail geometry.** The Content Browser shares cached mesh preview samples through `Arc`, avoiding repeated copies of vertex/index arrays and warnings in UI snapshots.
10. **Index hierarchy children.** Each hierarchy UI pass builds a child lookup table instead of scanning every object for every expanded row. Selection synchronization also uses an object lookup table. Document order and collapsed/reparented/deleted behavior are preserved.
11. **Address selected model parts directly.** An expanded model-surface entity skips directly to its selected part instead of scanning all of its sibling surfaces during render preparation.
12. **Repair a pre-existing smoke-test clock mismatch.** Unmodified main failed the animated water save/reload pixel comparison because the restored scene's shader clock differed. The comparison now uses the same shader time, as it already did for display effects. Scene serialization and animation behavior are unchanged.

## Measurements

All timings are release builds on Apple M2 Pro, using Metal for graphics, with the same scene and settings before and after. Before binaries were built and preserved from the exact base commit. Three process runs were made per version, alternating before/after order. Tables report the median of the three per-run medians; ranges cover those medians.

| Renderer workload / measurement | Main median (range), ms | Optimized median (range), ms | Reduction |
| --- | ---: | ---: | ---: |
| Sponza point lights / CPU | 1.398 (1.385–1.439) | 0.339 (0.336–0.355) | 75.8% |
| Sponza point lights / synchronized wall | 4.587 (4.511–4.673) | 2.525 (2.486–2.583) | 45.0% |
| Water Lab / CPU | 0.288 (0.284–0.296) | 0.256 (0.244–0.259) | 11.1% |
| Water Lab / synchronized wall | 1.255 (1.254–1.265) | 1.193 (1.183–1.201) | 4.9% |

| Editor CPU workload | Main, ms | Optimized, ms | Reduction |
| --- | ---: | ---: | ---: |
| Water Lab / render extraction | 0.051437 | 0.003417 | 93.4% |
| Water Lab / center pick | 0.023875 | 0.001042 | 95.6% |
| Shader Node Lab / render extraction | 0.056000 | 0.004459 | 92.0% |
| First Trail / render extraction | 0.022875 | 0.005625 | 75.4% |
| First Trail / center pick | 0.019750 | 0.002708 | 86.3% |
| Small baked GI scene / render extraction | 0.018125 | 0.013000 | 28.3% |
| 128 shader cubes (synthetic) / render extraction | 0.724812 | 0.081145 | 88.8% |
| 128 shader cubes (synthetic) / center pick | 0.213584 | 0.032209 | 84.9% |
| 384 boxes (synthetic) / collision overlay | 3.685542 | 0.009667 | 99.7% |
| 384 boxes (synthetic) / center pick | 0.367833 | 0.118209 | 67.9% |

Editor timings varied between runs. For example, Water Lab extraction measured 0.050959–0.052020 ms on main and 0.003291–0.008063 ms after optimization. The synthetic box-overlay result was 3.665667–3.703917 ms before and 0.009667–0.009833 ms after. Empty collision snapshots can complete below the timer resolution; they are not reported as zero-cost work.

| Additional check | Before | After |
| --- | ---: | ---: |
| Hierarchy traversal, 2,000 objects (synthetic) | 5.066104 ms | 0.335000 ms |
| Six static editor UI frames | Scene drawn on every pass | Zero scene draws; retained pixels identical |

The renderer benchmark runs at 800×500, with three warm-up iterations and 100 measured frames per mode. Each run interleaves reference, culling, and optimized modes. `CPU` is render preparation, encoding, and submission. `Synchronized wall` includes CPU, GPU completion, and an explicit wait. It excludes scene simulation/extraction, editor UI, and window presentation. These numbers are **not FPS or isolated GPU timings**.

The static Sponza point-light scene retains 89 visible color surfaces, 257,752 color triangles, and one color pipeline bind. Once warm, its shadow work falls from 443 draws / 1,353,009 triangles to zero. Its object uniform writes also fall to zero on repeated identical frames. With no consumers of the three auxiliary buffers, the optimized opaque pass can discard 9,600,000 bytes of logical attachment contents per frame. This is a store-policy count, not a measurement of hardware memory traffic; the textures remain allocated.

The editor CPU benchmark includes 10 warm-ups and 200 timed queries per path, plus 1,681 untimed BVH-versus-linear picking comparisons per scene/run. Measurements combine authoring-world and shader-source reuse; they do not isolate each change's contribution. The 128-object shader grid and 384-box grid are synthetic scaling tests, not normal gameplay scenes. The baked-GI fixture is the small scene produced by the editor acceptance test.

The hierarchy benchmark compares the old full-scene traversal with the child index, including index construction, for 2,000 synthetic objects. It alternates order for 100 samples after 10 warm-ups and checks exact visitation order. It measures hierarchy preparation/traversal, not egui painting or GPU rendering.

## Validation

- Full final workspace suite: **334 passed, zero failures**; 5 intentionally ignored tests/benchmarks. The hierarchy benchmark was also run explicitly in release mode and passed.
- Formatting, all-target Clippy with warnings denied, and the headless dependency-boundary check passed.
- Native Metal player smoke and render benchmarks passed for Sponza point lights and Water Lab. Three final runs of each workload passed.
- Native final editor acceptance passed: retained viewport, camera, authored commands, save/open, queued asset imports/cancellation, GI bake, prefabs, blueprints, Play isolation, and exported-game verification. It recorded 11 scene draws and 35 texture reuses across the acceptance flow; the dedicated idle check recorded zero scene draws for six consecutive UI frames.
- **57 before/after saved-image comparisons were byte-identical**: 29 from the Sponza run and 28 from Water Lab. These include loaded scenes and renderer diagnostics; they are not 57 distinct gameplay workloads.

New checks cover:

- Static shadow reuse and camera/color changes; moving and mirrored casters; transparent receiver bounds; point/spot movement, range and bias; resizing maps; texture/mesh replacement using the same asset ID; removal and enable/disable transitions. Cached output is compared pixel-for-pixel against caching disabled.
- Independent renderer histories with TAA, motion blur, and reflections toggled after their buffers were discarded, including moving geometry. Outputs match the reference exactly.
- Shader-graph edits, invalid values, eviction, and live Time updates with uniform caching enabled.
- Repeated edit queries, stable identities, material/transform edits, undo/redo, collision bounds, picking, and Play isolation.
- Native retained viewport pixels, six idle UI frames with zero new scene draws, immediate camera redraw, and exact pixels after restoring the camera.
- Hierarchy order after reparenting/deletion and the existing native authoring, asset, prefab, GI, blueprint, and export paths.

The baseline water smoke failure was reproduced before edits. Its render benchmark and saved common images remain usable comparisons; only the old clock-mismatched save/reload assertion fails. The corrected branch passes that check.

## Findings and limits

- Existing frustum/shadow culling, shared model textures, mesh picking BVHs, staged asset uploads, and empty particle/physics early-outs were already present on main. They remain useful foundations; this work does not claim them as new optimizations.
- Shadow reuse is conservative and covers the entire shadow frame. A moving caster or shadow light invalidates all retained maps. Animated scenes still pay for shadow rendering; per-light invalidation or spatial partitioning would require a separate measured design.
- Active box-overlap queries still use an all-pairs loop. Removing SAT allocations and caching edit-mode queries reduces work, but it does not change the live query's quadratic scaling. Rapier's existing broad phase continues to handle its own simulated bodies.
- GI freshness checks still capture and fingerprint the runtime scene. The small baked-GI query measured here is inexpensive; larger baked worlds should be profiled before changing the invalidation contract. Asset polling still checks file contents so same-size/same-timestamp replacements are detected.
- Auxiliary textures are still allocated, and their shader outputs are still produced. A rendering path with fewer attachments could save more work, but would require additional pipeline variants and backend validation. This change only eliminates unneeded stores.
- Shader WGSL generation is cached, but GPU pipelines for graphs absent from the current draw set still retire under the existing policy. Switching graph previews could benefit from a bounded GPU pipeline cache if measured as a significant source of stalls.
- The hierarchy and inspector still perform some linear scene cloning/scanning. The quadratic child traversal was removed; virtualized rows or shared document snapshots could help substantially larger projects.
- A separate pre-existing correctness issue was found: the shared shadow mesh helper maps the renderer's `Sphere` primitive to cube geometry. The current shader preview disables shadows, and authored scene meshes do not expose that primitive. This was kept separate from performance changes.
- Caches trade some memory for reuse: one authoring world, one authored collision snapshot, up to 32 shader sources per thread, a depth-state snapshot, and 496 cached bytes per object binding. They are bounded or replaced on revision/asset changes; no scene assets are added.
- All implementation changes use portable Rust/wgpu/egui paths. Timings and native validation here are from this Mac; Windows/DX12 and Linux/Vulkan CI have not run because the branch has not been pushed.

## Reproduction

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
python3 tools/check_headless.py
cargo build --release --locked --offline -p bozzard-player -p bozzard-editor-app
cargo run --release --locked --offline -p bozzard-editor --example benchmark_editor -- \
  examples/demo/scenes/water-lab.json 200
cargo test --release --locked --offline -p bozzard-editor-app \
  hierarchy_traversal_benchmark -- --ignored --nocapture
target/release/bozzard-player --smoke --scene examples/sponza/point-lights.json \
  --hardware --backend metal --benchmark-frames 100 --output work/performance-sponza
target/release/bozzard-editor --hardware --backend metal --smoke work/performance-editor
```

The GPU commands and the shader integration tests require access to a native graphics adapter. See [Sponza setup](sponza.md) for its optional dataset. Local before/after binaries, fixture files, logs, and pixel evidence are in the ignored `work/engine-performance/` directory. The measurements table records the results so review does not depend on retaining those local files.
