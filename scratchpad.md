# Bozzard working scratchpad

Live handoff. Replace superseded status; use Git history for completed narratives.

## Current checkpoint — release profiling and accelerated picking

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

Wait for the user's home test of surface selection and material controls; obtain direction before another subsystem. Local commits remain unpushed. Run cross-platform CI only when a push is requested. Picking optimization is complete. Further frame-performance work should start with GPU pass timings/full viewport profiling; another possible feature is per-surface texture replacement with managed asset dependencies.

Launch: `cargo run -p bozzard-editor-app --locked --offline -- --scene examples/sponza/scene.json`. Click geometry or choose Imported surfaces; edit Tint, enable Metallic/Roughness, Undo/Redo, Reset override, Save As/reopen, Play/Stop. Duplicate via Select whole model and verify independent edits on the copy. Use Save As into `work/sponza/` to keep the tracked sample view clean. F over viewport / double-click a row frames a surface; Shift+F frames all.

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
