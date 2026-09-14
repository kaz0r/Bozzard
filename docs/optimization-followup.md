# Optimization follow-up

This pass builds on the locally committed first pass, `9e76473`, on `codex/engine-performance`. Measurements here compare against that commit, **not against main**. See [the first review](optimization-results.md) for the earlier results against main. No scene assets or Sponza showcase files are included.

## Changes

1. **Live box collision broad phase.** Overlap queries sort conservative bounds along the widest scene axis, skip distant pairs, and run the existing SAT test on candidates. Bounds include SAT's relative and absolute tolerances using each transformed box's inradius. This preserves near-touching contacts under rotation, shear, mirroring, and very small dimensions. Degenerate geometry falls back conservatively. Output pairs remain unique and sorted by object ID.
2. **Cheaper box movement.** Movement builds validated collider geometry without computing every overlap in the scene, both before moving and during final validation. It reuses the initial world matrices, indexes parents once, and evaluates response SAT axes lazily without allocating a vector per pair. Collision response, sliding, initial-overlap recovery, and transactional failure behavior are preserved.
3. **Individual shadow-map retention.** Directional shadows invalidate separately from local lights. Each spotlight map and each point-light face retains its own exact visible-caster state. Moving one point light redraws its six faces; moving a spotlight redraws its one map. Moving geometry invalidates maps whose caster list or depth-producing inputs change. Entering/leaving a light frustum, array reallocation, same-ID asset replacement, and error paths invalidate affected retained state. The existing whole-frame shortcut still handles completely static scenes.
4. **Bounded GPU shader pipeline cache.** Keep every active graph variant plus the eight most recently absent variants. Switching back to a recent preview reuses compiled pipelines. A variant distinguishes full auxiliary output from color-only output; each contains basic/PBR and opaque/transparent pipelines. Diagnostic caching-off mode retires absent variants immediately. Counters expose compilation and residency.
5. **Color-only rendering when auxiliary outputs have no consumers.** Stock, PBR, shader-graph, and sky pipelines can render with just HDR color and depth. The three RGBA16F auxiliary attachments are omitted when TAA, motion blur, reflections, and particles do not require them. Normal/motion/specular textures are allocated on first need at the current viewport size and retained until resize, avoiding repeated allocation when toggling effects. At 800×500, a scene that never needs these targets avoids 9.6 MB of logical texture allocation; at 1920×1080, 49.77 MB. These figures exclude other render targets, alignment, driver overhead, and textures retained by effect history. Enabled effect quality and sample counts are unchanged.
6. **Shared editor document snapshots.** Hierarchy, entity inspection, and Effects share one immutable snapshot per document revision. Entity edits copy the document when applying a change. Hierarchy rows use their existing object to look up imported geometry instead of finding that object again with a full scene scan.
7. **Small settings drafts in editor panels.** Lighting and Effects copy only editable scene settings; they no longer clone the full entity/asset document on every UI pass. Applying an edit preserves entities, assets, cameras, prefabs, and Undo. This also fixes the existing Game Flow edit check, which omitted Game Flow changes from its change detection.
8. **Reuse authored GI freshness.** Authoring queries and the isolated Effects preview reuse freshness for the current document revision and actual resident asset-content fingerprints. Direct public `AssetStore` reloads invalidate it even when the editor's asset-publication counter is unchanged. Play and the public runtime extraction path still capture and fingerprint live state, so moving a baked static object correctly disables stale GI.
9. **Sphere consistency.** The shadow/culling mesh helper now uses sphere geometry for the sphere primitive. Motion history also gives spheres their own mesh identity. This is a correctness repair, not a claimed performance gain.
10. **Reproducible diagnostics.** Added release examples for live collision scaling, independently animated shadow histories, shader preview switches, document reads, and GI freshness. The existing local-shadow culling smoke explicitly disables cache reuse while counting draws; exact count and pixel assertions are preserved.

## Measurements

Three process runs per version, alternating before/after order, with no builds or other GPU tests running. Values are the median of per-run medians; parentheses show their range.

| Live CPU workload | First pass, ms | Follow-up, ms | Time reduction |
| --- | ---: | ---: | ---: |
| 384 boxes / overlaps | 1.035 (1.029–1.052) | 0.136 (0.136–0.139) | 86.9% |
| 384 boxes / move one box | 2.624 (2.611–2.627) | 0.304 (0.304–0.311) | 88.4% |
| 1,536 boxes / overlaps | 15.462 (15.386–15.500) | 0.649 (0.645–0.658) | 95.8% |
| 1,536 boxes / move one box | 35.263 (35.113–35.606) | 1.284 (1.282–1.291) | 96.4% |

| Renderer workload / measurement | First pass, ms | Follow-up, ms |
| --- | ---: | ---: |
| Moving caster / CPU | 0.799 (0.788–0.806) | 0.489 (0.478–0.502) |
| Moving caster / synchronized wall | 2.246 (2.236–2.280) | 1.359 (1.343–1.376) |
| Moving point light / CPU | 0.758 (0.750–0.788) | 0.457 (0.438–0.460) |
| Moving point light / synchronized wall | 2.230 (2.214–2.246) | 1.296 (1.289–1.296) |
| Warm graph switch / CPU | 6.303 (6.182–6.307) | 0.116 (0.112–0.120) |
| Warm graph switch / synchronized wall | 7.279 (7.274–7.439) | 0.971 (0.962–0.990) |
| Static Sponza point lights / CPU | 0.341 (0.332–0.355) | 0.342 (0.335–0.355) |
| Static Sponza point lights / synchronized wall | 2.497 (2.488–2.499) | 2.401 (2.391–2.425) |
| Water Lab, 400 frames / CPU | 0.252 (0.247–0.253) | 0.241 (0.241–0.263) |
| Water Lab, 400 frames / synchronized wall | 1.166 (1.164–1.168) | 1.166 (1.139–1.213) |

Moving-caster shadow draws fell from 33,749 to 20,161 over 100 frames; moving-light shadow draws fell from 33,493 to 6,993. Color geometry and shadow resolutions were unchanged. Static Sponza synchronized time improved about 3.8%, with essentially unchanged CPU time. The color-only attachment prototype showed no clear timing gain on the small synthetic fixture, so it is not counted as a separate dynamic-rendering speedup.

The initial 100-frame Water Lab comparison measured 1.163 ms before versus 1.191 ms after. A longer, three-run 400-frame recheck produced equal synchronized medians of 1.166 ms; its ranges and CPU timings are shown above. These measurements establish neither an additional Water Lab speedup nor a sustained regression.

| Large document CPU operation | Median (range), ms |
| --- | ---: |
| Clone 1,546-object document | 0.135 (0.132–0.189) |
| Reuse immutable document | <0.001 (below useful timer resolution) |
| Uncached GI freshness | 2.288 (2.288–2.298) |
| Cached authored GI freshness | <0.001 (below useful timer resolution) |
| Live extraction / reference | 3.952 (3.952–3.973) |
| Authored extraction | 0.690 (0.688–0.704) |
| Effects preview extraction | 0.714 (0.697–0.716) |

Cached GI freshness is not zero-cost; the synthetic fixture has no imported source assets, so its warm key comparison is particularly small. Authored extraction is about 82.5% lower than the live reference on this fixture.

The dynamic fixture has 96 cubes in two separated groups, two shadowed point lights, two shadowed spotlights, and a directional shadow. A single caster or point light moves every measured frame. The graph workload switches among four graphs. Each mode has its own renderer and complete frame history; a reference draw cannot warm the optimized renderer's shadow cache. Pixel comparisons repeat sampled poses outside timing. Each process alternates mode order, with eight warmups and 100 measured shadow frames or 24 graph switches.

The collision fixtures contain rotated boxes on a sparse grid. Queries update a live transform; they do not use the editor's collision cache. Each path has ten warmups and 100 samples, and an exhaustive SAT oracle runs outside timing. Dense overlapping scenes can still require quadratic work; box/mesh queries continue using the existing mesh BVHs.

The document fixture contains 1,536 additional static cubes and a small, valid synthetic GI payload with a matching source fingerprint. Its timings compare uncached and cached operations in the same release binary with ten warmups and 200 samples. They measure document preparation and extraction, not egui layout/painting, baking quality, or runtime gameplay speed.

CPU timings exclude GPU execution. Synchronized wall timings include rendering, device completion, and explicit wait overhead; they are not isolated GPU times or FPS. All measurements are local Apple M2 Pro / Metal results. Cross-platform CI has not run because the branch remains local.

## Validation

- Final workspace suite: **341 passed, zero failures; 5 intentionally ignored**.
- Formatting, all-target Clippy with warnings denied, and headless dependency checks passed.
- Three before/after native Sponza and Water Lab smoke runs passed, plus three longer Water Lab rechecks per version. The independent dynamic renderer oracle passed in all three runs per version.
- **59 saved before/after images were byte-identical**: 29 from Sponza and 30 from Water Lab, comparing the first complete run of each version. The loaded scenes and renderer diagnostics are included; this is not a claim of 59 distinct gameplay workloads.
- Final native editor acceptance passed: scene/property/effects editing, camera and gizmos, imports, GI bake, prefabs, blueprints and object references, Play isolation, save/open, and exported-game verification. Six idle UI frames required zero scene redraws; camera movement redrew immediately and restoring it restored exact pixels. The full flow recorded 12 draws and 33 reuses.
- Native editor capture was visually inspected. No new scene assets were added.

New checks exercise exhaustive SAT equivalence across 1,003 varied boxes; live movement and existing gameplay/contact behavior; independent shadow histories with light/caster movement, range changes, removal, empty scenes, alpha reloads, mirroring, and effect toggles; six-face versus one-map invalidation; GPU graph eviction and active sets larger than the idle bound; sphere shadow triangle counts; GI asset replacement outside editor counters; immutable document revisions; settings preservation and Undo.

## Remaining limits

- Runtime GI still validates/fingerprints live scene inputs. Safely caching arbitrary public ECS mutations needs a separate mutation-tracking design; authoring revisions cannot substitute for it.
- The collision broad phase targets box overlaps. Swept movement still checks the mover against obstacles, and densely overlapping bounds can degenerate to quadratic candidate traversal. Rapier's simulation broad phase is separate and unchanged.
- A moving caster still invalidates the directional map because that map covers the scene. Local shadow keys still inspect candidate geometry per light face; there is no new renderer spatial index.
- The auxiliary path is deliberately binary: color-only or all three auxiliary outputs. Effects needing only a subset still use the full attachment layout, with unused stores discarded. WGSL still declares its original outputs; backend compilation handles outputs without render targets. Per-output shader specialization and isolated GPU profiling remain future work.
- Caches consume memory: one additional document snapshot, per-map caster keys, and up to eight inactive GPU graph variants. Static scene data/probe arrays and imported assets remain shared where their existing types support sharing.
- Full hierarchy virtualization, UI paint batching, and persistent runtime transform caches remain separate opportunities. This pass removes redundant document copies and row lookups without changing the interaction model.

## Reproduction

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
python3 tools/check_headless.py
cargo run --release --locked --offline -p bozzard-scene --example benchmark_collisions -- 1536
cargo run --release --locked --offline -p bozzard-render-assets --example benchmark_dynamic
cargo run --release --locked --offline -p bozzard-editor --example benchmark_documents
cargo build --release --locked --offline -p bozzard-player -p bozzard-editor-app
target/release/bozzard-player --smoke --scene examples/sponza/point-lights.json \
  --hardware --backend metal --benchmark-frames 100 --output work/followup-sponza
target/release/bozzard-editor --hardware --backend metal --smoke work/followup-editor
```

The native GPU commands require graphics-adapter access. Sponza requires its optional dataset; see [setup](sponza.md). Local preserved binaries, three-run logs, pixel comparisons, and the comparison runner live in ignored `work/engine-performance-2/`.
