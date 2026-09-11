# Bozzard working scratchpad

Live handoff. Replace superseded status; use Git history for completed narratives.

## Current checkpoint — imported surface inspection

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

Let the user try selecting surfaces in Sponza and inspect materials; obtain direction before another subsystem. A useful follow-up is per-surface material overrides (editable tint/roughness with scene persistence and Undo), which is not implemented. New checkpoint has not been pushed; run CI when a push is requested.

Launch: `cargo run -p bozzard-editor-app --locked --offline -- --scene examples/sponza/scene.json`. Click geometry or choose Imported surfaces; F over viewport / double-click a row frames it. Shift+F frames all; Select whole model returns to transforms.

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
