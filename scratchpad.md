# Bozzard working scratchpad

Live handoff. Replace superseded status; use Git history for completed narratives.

## Current checkpoint — Windows material-save CI fixture fix (CI passed)

All three rendering subsystems were pushed to `main` through `f50fe42`. [CI run 34594913733](https://github.com/kaz0r/Bozzard/actions/runs/34594913733) passed Linux/Vulkan and macOS/Metal, but Windows failed CPU tests before reaching DX12 verification. The same material-save failure also occurred on the preceding `db5295c` run, before the new rendering subsystems.

Cause: `materials::tests::overrides_save_reopen_duplicate_and_play_without_mutating_source` loads assets from the repository checkout and saves into the system temp directory. On Windows CI these are on different filesystem roots, so Save As correctly rejects relative references across drives. The test now allocates its temporary output under ignored `work/material-tests/` on the checkout filesystem, preserving collision-safe allocation, cleanup, and all save/reopen assertions. Production asset-path validation is unchanged.

Validation passed: all 38 editor CPU tests (32 unit + 6 integration), editor all-target Clippy with warnings denied, formatting and diff checks. Logs: `/tmp/bozzard-windows-material-fixture-{tests,clippy}.log`. Fix committed and pushed as `d66d2e1`. [CI run 34596562891](https://github.com/kaz0r/Bozzard/actions/runs/34596562891) completed successfully on Linux/Vulkan, macOS/Metal and Windows/DX12, including CPU/headless tests, release builds, extracted-package pixel verification and artifact uploads. Windows passed the formerly failing material-save test stage and its previously blocked graphics verification. Watch log: `/tmp/bozzard-ci-d66d2e1-watch.log`. This follow-up changes only documentation and skips redundant CI; the tested code is unchanged. Pointer untouched.

## Previous checkpoint — local lights, bloom, baked GI (validated)

User approved implementing all three subsystems and committing each after validation. Root owns code and this scratchpad; GPT-5.6 Luna (`lighting_docs`) owns the light documentation. Subsequently pushed through `f50fe42` at the user's request; see the current CI checkpoint above. User confirmed Escape works.

1. Point/spot lights: IMPLEMENTED and committed `e12b983`; CPU, native Metal and native editor capture passed. Authored object component, ECS/transforms, bounded shared renderer light list, PBR+Lambert lighting, editor creation/inspector/guides, save/history/Play, CPU and native GPU validation. Local-light shadows remain future work as discussed; sun shadows unchanged.
2. Bloom: IMPLEMENTED and CPU/Metal validated, committed `b61b545`. HDR threshold/downsample/upsample composite before display mapping, editor controls and persistence, disabled/2D/raw parity and resize/GPU checks.
3. Baked GI: IMPLEMENTED and validated, committed `f50fe42`. CPU diffuse transport with occlusion/color bounce, saved probe data and source invalidation, async editor workflow, renderer integration and CPU/native GPU/native editor tests. All three subsystems are complete; hosted results are recorded above.

Validation: meaningful scene/editor tests, native Metal pixel/smoke checks, formatting/Clippy/headless boundary and visual review for each subsystem. CI workflow exercises new GPU fixtures on Metal/Vulkan/DX12. Preserve Bozz artwork. Avoid moving pointer.

### Light subsystem evidence (`e12b983`)

- Scene `Object.light` and ECS capture/view, max32 validated authored components, 3D-only extraction; inherited transforms, world-unit range, local −Z spotlight cones. Renderer uses shared bounded 2064-byte frame uniform (group2binding3), no hardware ray features; same GGX/direct Lambert responses and unchanged sun-shadow policy.
- Editor + Light creation, component controls, history/duplicate/save/Play isolation, fixed-size clickable markers (including disabled), selected sphere/cone guides. New asset-free `examples/demo/scenes/lighting-lab.json` demonstrates colored points and warm spot.
- Full workspace tests, all-target Clippy, headless boundary passed. Native Metal fixture suite passed existing rendering regressions plus local color, inverse-square falloff, smooth range cutoff, cone penumbra/equal-angle edge/direction, multiple/removal/lastslot32/overflow/invalid values/unlit. Logs `/tmp/bozzard-local-lights-{tests,clippy,final-gpu,ui-tests,final-clippy}.log`.
- Release Lighting Lab 30frames800×500: optimizedCPUmedian0.068ms, synchronizedwall0.497ms,5draws60triangles; exact reference/culling/cache pixels. `work/lighting-lab/loaded-3d.png` visually reviewed. Full viewport/compositor cost not measured.
- Native editor capture now passed after Mac unlock, including light inspector/guides and GI bake controls/save. Final log `/tmp/bozzard-editor-gi-final-validation.log`; captures under `work/editor-gi-final-validation/`. Earlier locked-screen attempt was superseded. Pointer never moved. Hosted CI remains pending push; existing Metal/Vulkan/DX12 workflow runs all new smoke fixtures.

### Bloom subsystem evidence

- Scene `display.bloom`: enabled(defaultfalse), intensity0.15 [0..10], threshold1 [0..60000] scene-linear/pre-exposure, scatter0.7 [0..1]. Editor Display3D controls/Reset/history/Save/Play; 2D extraction disables.
- Max6 half-resolution RGBA16Float levels, normalized bilinear downsample + tent upsample, 50% soft knee, convex broad-level blend to preserve constant energy across sizes. Add to HDR before exposure/Reinhard/sRGB; preserve alpha. Disabled/zero intensity/raw release pyramid resources and produce original pixels. No geometry illumination claim.
- Full workspace tests/Clippy/headless audit/fmt passed. Native Metal suites pass halo/intensity/threshold/spread, disable/raw exactpixel parity, alpha, known HDR arithmetic, hardware/shader sRGB parity, constant energy at64×64/97×53/1×1/1×17/3×5, invalid input, and all previous render fixtures. `work/bloom-gpu/bloom-{off,on}.png` visually reviewed; `work/bloom-lab/loaded-3d.png` shows soft highlight glow. Save/reload actual Lighting Lab produces identical pixels.
- Release Lighting Lab30frames800×500 bloom on: optimized CPU0.200ms/synchronizedwall0.978ms. Bloom-off comparison in `/tmp/bozzard-bloom-lab-off.log`; hardware/compositor/fullviewport timings not implied.
- Logs `/tmp/bozzard-bloom-{tests,clippy,gpu,lab,lab-off}.log`. Tests automatically execute in existing hosted smoke matrix but hosted CI pending push. Native editor capture subsequently passed after unlock; no pointer movement.

## Previous checkpoint — Escape deselection

User is now home and manually confirmed Sponza navigation, picking and material/color editing. Implemented Escape to clear both object/surface selection and the yellow outline without changing the scene. Existing Play stop, gizmo drag cancellation, fly/navigation release, text editing, popup and dialog interactions retain priority. Read keyboard focus in the raw input hook before egui clears it on Escape; key repeats cannot deselect after the first press cancels another action. Viewport hint and Luna's README/Sponza docs updated.

Validated with focused editor/core tests (including an actual egui focus-loss regression), editor all-target denied-warning Clippy, formatting and diff checks. Logs: `/tmp/bozzard-escape-deselect-{tests,clippy}.log`. This checkpoint commits after `5358ec5`; **do not push**. User should restart/rebuild the editor to try Escape. No native windows launched and pointer untouched.

## Previous checkpoint — release profiling and accelerated picking (`5358ec5`)

Release-mode Sponza performance pass implemented and validated; this checkpoint commits after `d489ca3`. **Do not push.** User is at work and will announce when home to test. No pointer movement or user interaction used. Latest CI still covers pushed `0469807`, not the local surface/material/picking commits. Root owns implementation/scratchpad; Luna wrote `docs/performance.md` and linked it from `docs/sponza.md`.

### Measurement and implementation

- Apple M2 Pro release baseline: editor scene extraction/overlays each ~0.003ms; picking1.2ms; renderer CPU0.64ms and synchronized CPU+GPU+wait3.32ms at800×500,shadow4096. Chosen optimization is picking, the largest measured editor CPU operation. Picking happens on clicks; no FPS/rendering-speed claim. Full viewport UI/compositor cost and GPU timestamps are not measured. No ECS-cache rewrite.
- `crates/bozzard-assets/src/picking.rs`: balanced median-split BVH, leaves <=8triangles, near-first traversal, conservative bounds, unchanged triangle test and first-source-triangle ties. Original geometry/order unchanged. Built with each mesh replacement on the editor's loader thread; cancellation checks during construction. Traversal uses bounded stack recursion without heap allocation.
- Private Arc index on AssetStore Entry cannot become detached from its immutable mesh. Catalog/Undo snapshots and duplicate instances share it. Reload publishes matching data/index; failure preserves both last-good versions. Texture/material edits cannot mutate the index. No GPU dependency introduced; scene/headless boundary unchanged.
- `Entry::raycast_reference` and `Editor::pick_surface_reference_with_projection` retain the linear oracle. `benchmark_editor` compares paired accelerated/reference results outside timing, alternates which runs first, reports midpoint median/nearest-rankp95, index resident bytes/build cost and1681 wider-grid oracle rays per view. Player benchmark now reports prepare/encode/submit CPU medians.

### Results and validation

- Paired200-sample Sponza corridor: center BVH0.006667ms vslinear1.309292ms; grid0.009834ms vs1.295583ms. Atrium center0.006646/1.330771ms, grid0.011355/1.321001ms. Overview center0.005750/1.316250ms, grid0.009708/1.290750ms. Full tables/methodology in `docs/performance.md`.
- Index:262267triangles,65781nodes,3154060resident bytes (~3.01MiB),35.4–36.6ms build. Temporary build scratch/source geometry excluded from resident index size. Single warm-cache open measurements vary665–786ms; no precise cold-load claim.
- Full workspace tests, all-target denied-warning Clippy, formatting, headless audit and whitespace checks passed. Tests cover many rays, exact distance/triangle parity, edge/parallel/inside/missed rays, nonunit directions, scales, degenerate/coincident triangles, source ordering, mirrored/parented overlapping instances, cancellation and shared/reloaded index lifetime.
- 5043 untimed Sponza rays across3views matched exact object/surface results, plus paired timed comparisons. Hit counts1681/1675/1661 include misses in atrium/overview.
- Release native Model Workshop editor smoke passed authored commands/Play/async save/open/import/cancel/material controls; UI visually reviewed at `work/editor-picking-release/editor-surface.ppm`. Native Metal graphics suite and final100frameSponza benchmark passed. All16PPM diagnostics in `work/sponza/performance-{before,after}/` are byte-identical. Final renderer CPU0.671ms/synchronizedwall3.330ms: same counts/pixels, no timing improvement claimed.
- Logs: `/tmp/bozzard-sponza-cpu-{before,after}.log`, `/tmp/bozzard-sponza-{atrium,overview}-picking.log`, `/tmp/bozzard-picking-workspace-tests.log`, `/tmp/bozzard-picking-clippy.log`, `/tmp/bozzard-editor-picking-release.log`, `/tmp/bozzard-sponza-render-{before,after}.log`.

## Previous checkpoint — editable surface materials (`d489ca3`)

Implemented and validated tint/metallic/roughness overrides. This checkpoint commits the subsystem after `e6b3085`; **do not push**. User is at work and will announce when home to test; no scheduled reminder. Latest verified CI still covers pushed `0469807`, not these two local commits. Root owns implementation/scratchpad; Luna updated documentation. Pointer never moved.

### Implementation

- `bozzard-scene::Drawable.material_overrides`: optional sparse entries (surface index, source signature, linear RGB tint, optional metallic/roughness). Legacy scenes default to none. Validation rejects duplicates, malformed signatures, invalid factors, excessive indices/counts, and overrides on built-in geometry.
- Importer assigns deterministic source keys from structural node/mesh/primitive/material-slot identity, bounded names and indexed geometry; source pixels/factors are excluded. Saved mismatches stay stored but inactive, with an inspector warning. Geometry-changing reimports require explicit new edits; no automatic remapping.
- Renderer applies per-draw tint and PBR factor uniforms while retaining shared source textures/materials/geometry. Metallic/roughness replace source constants and still multiply the source map channels. Both editor and player extraction support overrides. Shared Arc override lists avoid per-surface list copies. Uniform layout/shaders agree at 368 bytes.
- Inspector exposes compact tint/metallic/roughness controls, reset, and indicators on edited surface rows. Edits use normal gesture history; duplicate owns an independent copy, Save/Open and Play retain authored values. Changing the owning mesh clears its old overrides; assigning the same mesh preserves them. OBJ material parts support tint only; unpartitioned OBJ remains whole-object-only.
- Limits: no per-surface texture replacement or alpha/emissive/normal-factor overrides; no global/shared material editing or independent primitive transforms. Source details remain read-only.

### Validation

- Full workspace tests and denied-warning Clippy passed. Subsequent same-mesh-assignment regression passed; final editor UI Clippy, formatting, headless dependency audit and diff whitespace check passed.
- CPU tests cover one-step drag Undo/Redo/cancel/reset, immutable asset identity, duplicate independence, Save As/reopen, Play isolation, invalid edits, stale keys and mesh reassignment. Import tests cover source-factor stability versus geometry/material-slot changes; schema tests cover optional/invalid data.
- Native Metal pixel tests passed surface/instance isolation, tint, reset, stale-source rejection, separate/combined metallic/roughness and exact parity with authored factors using a real metallic-roughness map. Shared source upload statistics remain unchanged. Existing PBR/shadow/environment/display/culling/staged-upload suites passed.
- Native editor smoke passed normal authored commands/Play/async save/open/import/cancel plus material Undo/Redo, shared residency and saved scene. Final compact controls visually reviewed at `work/editor-material-overrides-final/editor-surface.ppm`; saved sample `work/editor-material-overrides-final/material-scene.json`.
- Actual Sponza example picked surface 37, edited tint/metallic/roughness, validated Undo/Redo, Save As/reopen/source identity and Play. Ignored output: `work/sponza/material-override-scene.json`. Native player loaded/rendered this scene successfully (103 surfaces,69 images); visually reviewed blue-tinted wall at `work/sponza/material-overrides-gpu/loaded-3d.ppm`.
- Logs: `/tmp/bozzard-overrides-tests.log`, `/tmp/bozzard-overrides-clippy.log`, `/tmp/bozzard-overrides-ui-clippy.log`, `/tmp/bozzard-materials-final-test.log`, `/tmp/bozzard-editor-material-overrides-final.log`, `/tmp/bozzard-sponza-overrides.log`, `/tmp/bozzard-sponza-overrides-gpu.log`. Reproduction/checklist: `docs/sponza.md`.

## Previous checkpoint — imported surface inspection (`e6b3085`)

User asked to check CI and, on success, implement selection of imported surfaces, material inspection and F framing. **CI passed for pushed `0469807`**: [run 34555701299](https://github.com/kaz0r/Bozzard/actions/runs/34555701299), macos-15/Metal, ubuntu-24.04/Vulkan, windows-2025/DX12. The new inspection subsystem is implemented and validated; this checkpoint commits it. No new push requested.

- User requires a commit after each validated subsystem. Previous Sponza subsystem commits are already pushed through `0469807`.
- Delegate docs/small tasks to existing `async_docs` (GPT-5.6 Luna); root owns implementation and scratchpad. Preserve Bozz memorial artwork/dedication.
- Avoid moving the mouse. If pointer automation is used, restore its original bottom-left position to avoid locking the Mac. This checkpoint used CLI/native screenshots; pointer never moved.

### Implementation

- `crates/bozzard-assets/src/lib.rs`: `MeshPart` retains bounded source node/mesh/primitive labels and material names. Display names cap each source name at128 Unicode characters and normalize controls; original source files unchanged. glTF hierarchy transforms remain baked into geometry. OBJ material parts retain available labels; unpartitioned OBJ stays a whole object with no synthetic surface list.
- `crates/bozzard-editor/src/selection.rs`: nearest-triangle `Pick` includes a surface index; `select_object` clears surface inspection. Transient `SurfaceSelection` holds weak immutable CPU asset identity and cached local bounds. Failed reload preserves last-good selection, replacement invalidates it, Play clears it, Undo cannot revive discarded indices. Cached bounds transformed through parents feed surface F framing and clipped gold outline.
- `apps/editor/src/surfaces.rs`: searchable virtual surface list, double-click row / Frame surface / F, read-only material factors and image dimensions, authored sampler details on hover. Select whole model or a hierarchy row returns to owner editing. Surface inspection guards gizmos/rename/duplicate/delete/asset assignment. No independent primitive transforms, editable source hierarchy or material overrides. A primitive may contain disconnected geometry; triangle picking does not alpha-test texture cutouts.
- `Residency::is_current` compares CPU/GPU immutable identities. Viewport picking pauses while model geometry differs from GPU last-good data; surface framing/outline wait for that model's graphics. CPU labels/factors can still be inspected.
- Native smoke waits for GPU residency before screenshots, and imported models receive a second framed surface/inspector capture. Renderer metrics now sit below collider metrics, fixing overlap noticed during visual review.

### Validation

- Full workspace tests and denied-warning Clippy passed. Follow-up selection tests cover parent transforms, mirrored scale, gaps, nearest-object occlusion, selection-only bounds, Undo/Redo, reload identity, destructive-command guards, Play and unchanged document/history. Final UI Clippy/fmt and headless dependency audit passed.
- Native Model Workshop editor smoke passed with authored commands, Play isolation, collision/gravity, async save/open/import/cancel, projected cube pixel oracle and framed surface inspector capture. Output: `work/editor-surfaces-smoke/editor-surface.ppm`; final UI review also checks metrics separation.
- Native Metal graphics suite passed: `work/surface-residency-smoke`. New assertions prove last-good `has_all` differs from `is_current` during staged replacement; failed reload preserves current identity; completed upload publishes the new identity. Existing PBR/shadow/environment/display/culling/upload suites passed.
- Actual Sponza CPU inspection passed all103surface selections/bounds and3screen rays (indices37,6,71), leaving document/history unchanged. Reproduce: `cargo run -p bozzard-editor --example inspect_surfaces --locked --offline -- examples/sponza/scene.json`.
- Logs: `/tmp/bozzard-surfaces-tests.log`, `/tmp/bozzard-surfaces-clippy.log`, `/tmp/bozzard-selection-tests.log`, `/tmp/bozzard-surfaces-ui-clippy.log`, `/tmp/bozzard-editor-surfaces.log`, `/tmp/bozzard-surface-residency.log`, `/tmp/bozzard-sponza-surfaces.log`.
- README, `docs/assets.md`, `docs/sponza.md` updated by Luna and reviewed by root. CI status above covers the previous pushed commit, not this new checkpoint.

## Next step

The lighting, bloom and baked-GI commits plus the Windows test-fixture fix are pushed through `d66d2e1`, with all three hosted platforms green. Suggested next rendering subsystem: local-light shadows (spot first), followed by probe-quality improvements around thin walls. Await user direction before starting it. Path tracing remains a later renderer project.

Light demo: `cargo run -p bozzard-editor-app --locked --offline -- --scene examples/demo/scenes/lighting-lab.json`. CPU/GPU commands and honest current limits: `docs/lighting.md`.

## Completed Sponza rendering milestone

All agreed systems implemented and separately committed: measured budgets/shared textures; scalable project packaging; mipmaps; complete supported static glTF PBR import/rendering; GPU residency and staged uploads; authored sun/ambient; filtered sun shadows; HDR/exposure/tone mapping; procedural environment IBL; counters/culling/state caching. Final commit `0469807` adds durable views/docs. Goal marked complete after visual checks of corridor, atrium and overview. Detailed checkpoint history is in `0469807:scratchpad.md`; durable behavior/limitations are in `docs/assets.md` and `docs/sponza.md`.

- Dataset `work/sponza/glTF/Sponza.gltf`: official Khronos sample,71files/~50.2MiB, verified upstream resources; README/license under `work/sponza/`. `/work/` ignored; never commit downloaded model/textures.
- Tiny committed views: `examples/sponza/{scene,atrium,overview}.json`. Packaged project at `work/sponza/imported/scene.json` uses a default demo camera.
- Actual Sponza:192496vertices/262267triangles/103surfaces/69sharedPBRimages,380283556GPUtexturebytes including mipmaps. Current budgets:1Mvertices/3Mindices/4096surfaces/512MiBuniqueglTFimages/128MiBdecodedbuffers/32MiBsource/128MiBdependencies. OBJimages128MiB.
- Native Metal30frame debug benchmark, Apple M2 Pro800×500/shadow4096: reference103draws/103pipelinebinds/262267colortris; optimized89draws/1bind/257752tris; shadows103draws/262267tris unchanged; exact same pixels. CPUmedian18.189→17.045ms, synchronized CPU+GPU+wait23.977→22.924ms. No GPU-timestamp/FPS claim.
- Staged uploads: hard4MiB work slice, soft4ms CPU loop; observed maximum31.80ms in final debug reproduction. Atomic publication, stale cancellation and last-good retention. No hard latency guarantee.
- Lighting: one global sun/ambient, one camera-independent shadow map, alpha-aware casters/PCF, procedural distant diffuse/specular IBL and sky; HDRRgba16Float/exposure/Reinhard/sRGB once. No panorama import, local probes/GI/multibounce, cascades, point/spot lights, GPU timestamps, multidraw/instancing or occlusion culling.

## Architecture and safeguards

- Rust/custom ECS; renderer independent of scene/ECS/importers. `bozzard-render-assets` bridges CPU imports and graphics residency. Server remains GPU/window/image-decoder free.
- Async CPU open/import/save/reload and cancellation; GPU preparation worker plus bounded render-thread upload slices. Preserve last-good data, save/history integrity, Play isolation and owned import-resource cleanup.
- Static OBJ/glTF/GLB and PNG/JPEG. Unsupported skins/animations/morphs/required extensions rejected. UV-derivative tangent generation is not MikkTSpace.
- Existing editor hierarchy/search/reparenting, Inspector/gizmos/Undo, asset panel/import, box collisions/physics and third-person gameplay. Last gameplay checkpoint `29370d3` was manually confirmed by user.
- Navigation: RMB fly or Tab toggle for trackpads; WASD, Space/Ctrl, Shift faster. Tab/Escape releases flight; focus/Play/dialog changes cancel capture. Escape stops Play. Preserve native Tab interception before egui focus navigation.

## Useful checks

```sh
cargo fmt --all -- --check
cargo test --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
python3 tools/check_headless.py
cargo run -p bozzard-editor-app --locked --offline -- --scene examples/demo/scenes/model-lab.json --smoke work/editor-surfaces-smoke --hardware --backend metal
git diff --check
```


### Baked GI subsystem evidence (this checkpoint)

- CPU background static diffuse bake:9 cosine-convolved SH coefficients plus8×8 directional distance moments per probe;41vec4/probe;2..16probes/axis(max4096),64..1024 power-of-two rays/probe,1..4 diffuse bounces. At most1024staticinstances/1Mstatictriangles. Trace reuses immutable mesh BVHs; Fit transforms cached bounds without scanning vertices on UI thread. Cancellation checked throughout CPU transport; codecs/BVH retain their existing cancellation boundaries.
- Texture/material transport includes source sRGB/base color, UV sets/wrap/filter, alpha cutouts, emissive and metallic factors/maps, per-surface overrides and explicit whole-object base texture. Alpha-blended surfaces do not transport light; >128 cutout layers terminate conservatively. No glossy/caustic/normal-map bounce transport; CPU samples mip0. Runtime diffuse response uses receiving normals, retains direct lights and specular sky. Low-frequency probes/moment maps can leave mottling/leaks/extra darkness around thin walls; bias and volume placement/resolution matter. Local direct lights still have no realtime shadows.
- Known moving objects (Spin/enabled Gravity/PlayerController) and descendants are excluded from casters; other drawables opt out with `gi_static`. A queue handles arbitrarily ordered deep hierarchies without repeated full scans. Dynamic receivers still sample GI. Content-only last-good asset hashes plus static transforms/materials/lights/sky/volume expire stale bakes; camera/display/name/intensity/bias changes preserve them. Changed static components during Play disable GI until Stop restores authored state. CPU/GPU asset-generation mismatch also disables stale GI until residency is current.
- Inline serialized Arc bake data survives Undo/Redo/Play/Save As. Validation retains the immutable data allocation, so unchanged data is not scanned each frame; Arc mutation gets a new identity and is revalidated. GPU data upload cached by retained Arc identity; metadata validated each draw. Max packed data2.5625MiB. One current GPU volume retained; no streaming/multivolume.
- Editor Scene lighting → Baked global illumination:Enable,Show volume/probes,Fit/Bake/Clear,intensity,bias,grid/samples/bounces,bounds. Async bake/cancel, revision/path/content publication guard, one-step Undo/Redo and saved result. Cyan current/amber stale-or-unbaked/gray invalid probes, editor-only; overlay absent in Play/2D. Native verification caught idle egui RGB↔HSV roundoff expiring bakes: all scene/light/surface color pickers now publish only intentional edits; exact no-input color regression passes.
- Full workspace tests, denied-warning all-target Clippy, formatting, whitespace and headless dependency audit passed. Additional final focused CPU tests include1000-level reverse hierarchy exclusion, source asset corruption/replacement/rebase, constant-environment energy, deterministic bake, red diffuse bounce, second-bounce gain, roof occlusion, invalid solid probes, schema/identity mutation, alpha/PBR/UV/emission/override behavior. Editor tests cover cancellation, stale revision/path/Play rejection, history/Save/reopen/current source, runtime static movement and material invalidation.
- Final native Metal full suite passes all prior lighting/bloom/render/upload regressions plus numeric GI tests:SH constant/direction, visibility, PBR/diffuse/metallic/unlit,intensity/disable/outside bounds,max grid,data replacement,invalid inputs and resize. CPU bake→JSON→player extraction→GPU oracle:14984 affected pixels,683 previously neutral pixels gain red/green. Actual room and Sponza save/reload/render pass; reference/culling/cache pixels identical. Final logs `/tmp/bozzard-gi-final-validation.log`, `/tmp/bozzard-gi-{workspace-tests,cpu,clippy,editor-tests}.log`.
- Native Model Workshop editor acceptance passed light controls/guides, material controls, GI async bake/current source/extraction/volume guides/save plus existing authored commands/Play/import/save/open/cancel workflows. Final `/tmp/bozzard-editor-gi-final-validation.log`, captures `work/editor-gi-final-validation/`; final layout visually reviewed at `work/editor-gi-final-validation/editor-gi.png`. Mac unlocked during final runs. Pointer untouched. Hosted CI not run; all fixtures are included in existing cross-platform smoke workflow.

### Reproduce and inspect

- Asset-free room:`examples/demo/scenes/gi-lab.json`. Bake `cargo run --release -p bozzard-editor --example bake_gi --locked --offline -- examples/demo/scenes/gi-lab.json work/gi-lab/baked.json`; optional `--fit` computes bounds. Open resulting JSON in editor. Toggle Enable baked GI to compare; change a source material/light to see stale status, Bake then Save. Documentation:`docs/lighting.md`.
- Room384probes/251904packedbytes; final CPU release bake109.7ms M2Pro. Native room comparison `work/gi-lab/final-on/loaded-3d.png`, tuned bias0.2 review `work/gi-lab/bias/loaded-3d.png`; final smoke under `work/gi-final-validation/`. Release editor extraction50samples median0.014583ms; log `/tmp/bozzard-gi-extract-final.log`.
- Actual Sponza ignored artifacts:`work/sponza/{gi-source,gi-baked,gi-off}.json`; source references untouched `glTF/Sponza.gltf`. Grid[12,8,8],min[-12,-.3,-5],max[12,10,5],256rays/3bounces;768probes/503808bytes,4258ms release bake, Save/reopen current. `work/sponza/gi-final-{on,off}/loaded-3d.png` reviewed: covered corridor becomes darker than unoccluded diffuse sky, as expected. All downloaded assets/bakes remain ignored.
- Same release binary Metal30frame800×500 Sponza on:CPUmedian0.667ms,synchronizedwall5.652ms;off:CPU0.887ms,wall3.827ms. Same89visible/14culled/257752colortriangles/103shadowdraws/262267shadowtriangles. No CPU speedup/FPS/GPU-timestamp claim. Logs `/tmp/bozzard-sponza-gi-{bake,final-on,final-off}.log`. Thin-wall and low-resolution interpolation quality remain explicit limitations.
