# Bozzard working scratchpad

Live handoff. Replace superseded status; use Git history for completed narratives.

## Active goal

Load actual Sponza with working lighting/shadows and a visually good result. Keep the full agreed scope active: measured import and justified budgets; scalable packaging; shared textures; budgeted GPU uploads; performance counters and measured culling/draw improvements; filtering/mipmaps; full PBR metallic/roughness, normal/tangents, occlusion/emissive; lighting/shadows; environment lighting, exposure and tone mapping. Completion requires actual rendered Sponza and relevant CPU/GPU/native evidence, not only fixture tests.

- User requires **a commit after each validated subsystem**. No push requested for this goal.
- Delegate documentation/small tasks to existing `async_docs` (GPT-5.6 Luna). Root owns code/scratchpad. Preserve Bozz memorial artwork and README dedication.
- Avoid moving the mouse. If pointer automation is used, restore its original bottom-left position to avoid locking the Mac. CLI native Metal/offscreen checks need no pointer movement.
- Goal remains active and incomplete. No evidence of cross-platform CI for these local commits yet.

## Validated subsystems

1. **Shared textures — `01bb996`.** glTF surfaces reuse Arc images by source and opaque-alpha variant; GPU model upload deduplicates matching image slices. Base-color-only Sponza originally exceeded 128 MiB; sharing reduced it to 25 unique images / 96 MiB, ~5.7 s debug. Workspace tests/Clippy and real Sponza Metal smoke passed.
2. **Scalable project packaging — `424098e`.** glTF/GLB imports produce `assets/<id>/model.gltf` plus generated external buffers/images, preserving metadata. Exclusive directory reservation and cancelled/stale job cleanup protect neighbours. OBJ retains legacy single-file packaging. Actual editor import/save/reopen: ~23 s debug, 97727-byte document / ~50 MiB folder. Relocation/cleanup tests, workspace checks and packaged Sponza Metal smoke passed.
3. **Model mipmaps — `193f717`.** GPU-generated sRGB mip chains downsample in linear light; model base-color textures use trilinear repeat filtering. Standalone/procedural textures retain nearest sampling. GPU texture-byte stats include mip storage. Renderer tests, workspace Clippy and actual Sponza Metal smoke passed. Pixel checks cover extreme checker minification, sRGB averaging and odd/narrow image chains. Inspected `work/sponza/mipmaps/loaded-3d.ppm`; distant filtering improves, lighting remains unfinished.
4. **CPU PBR import — `72bf30e`.** `crates/bozzard-assets/src/pbr.rs` preserves PBR factors/maps, per-map samplers/UVs, double-sided state, tangent frames and mirrored handedness. `MeshPart.shading: Option<SurfaceShading>` contains a vertex start and extra attributes: tangent XYZW then normal, metallic/roughness, occlusion, emissive UV pairs (12 floats per local vertex). Existing 8-float geometry/base UV stream stays unchanged. glTF image budget increased to 512 MiB based on measured 272 MiB for Sponza's 69 shared images; OBJ remains 128 MiB. Full import ~15.34 s debug, unchanged check ~91 ms. Geometry unchanged: 192496 vertices / 262267 triangles / 103 surfaces. Actual source had 26 tangents parallel to normals; these are repaired from UV derivatives with warnings. Missing tangents use accumulated UV derivatives (not MikkTSpace). Source tangents otherwise preserved. Workspace tests and Clippy passed; regressions cover shared images across maps, distinct UV sets, sampler filtering/wrapping, factors, missing UV rejection, mirrored authored/generated tangents and repair. The following GPU subsystem consumes these imported attributes.

5. **GPU PBR — validated, committing this checkpoint.** New renderer PBR pipeline consumes normal/tangent, metallic/roughness, occlusion and emissive maps/factors; respects double-sided state and runtime reflection. Each map uses its authored sampler and independent UVs. sRGB base/emissive and linear data maps share GPU allocations by image identity plus color space; mip generation supports both formats. The new graphics-only `bozzard-render-assets` bridge provides one conversion/upload implementation for editor and player; importer/renderer/headless remain independent. View rays are reconstructed from inverse view-projection, supporting both orthographic and perspective cameras without separate camera state. Current illumination is explicitly fixed preview directional + small ambient, not the final lighting system.
   - Validation: full workspace tests, Clippy with denied warnings, headless dependency audit passed; native Metal editor smoke passed (`work/editor-pbr-smoke`). New native pixel suite (`apps/player/src/smoke/pbr.rs`) checks map channels, normal scale, linear AO affecting only ambient, sRGB emission, authored wrapping and UVs, double-sided culling, reflected surfaces and color-space-aware texture sharing. Actual Sponza Metal smoke with that suite passed, capture at `work/sponza/pbr/loaded-3d.ppm`; visible normal/specular shading, but still dark without proper environment/display treatment. Goal is NOT complete.

## Next work

After committing GPU PBR, implement budgeted GPU uploads across frames in the shared bridge and both application lifecycles, preserving transactional replacement/cancellation. Then add authored lights/shadows/environment/exposure/tone mapping, and measured culling/performance improvements. All remain required; current fixed preview illumination is not final lighting.

## Assets and reproduction

- Download: `work/sponza/glTF/Sponza.gltf`, official Khronos glTF-Sample-Assets version; 71 files / 50.2 MiB, verified against upstream hashes and resource presence. README/license in `work/sponza/`. Entire /work/ is ignored; never commit the dataset.
- Benchmark scene: `work/sponza/scene.json`, camera under an arch looking along the atrium. Packaged scene: `work/sponza/imported/scene.json` (default demo camera).
- CPU probe: `cargo run -p bozzard-assets --example inspect --locked --offline -- work/sponza/glTF/Sponza.gltf`. Counts ALL decoded PBR maps now.
- Packaging probe: `cargo run -p bozzard-editor --example import_model --locked --offline -- SOURCE NEW_SCENE` (new destination required).
- GPU: `cargo run -p bozzard-player --locked --offline -- --scene work/sponza/scene.json --smoke --output work/sponza/mipmaps --hardware --backend metal`.
- Captures: `work/sponza/baseline`, `work/sponza/packaged-smoke`, `work/sponza/mipmaps`.
- Offscreen smoke uses RGBA8Unorm (linear bytes); consider display conversion when judging exposure. Editor uses an sRGB target. Final tone mapping must explicitly handle output color space.

## Architecture / guardrails

- Custom ECS; native wgpu renderer independent of assets/ECS/windowing. Headless server remains GPU-free.
- glTF budgets: 1M vertices, 3M indices, 4096 surfaces, 512 MiB unique decoded images, 128 MiB decoded buffers, 32 MiB per source, 128 MiB source dependencies. Unsupported skins/animations/morphs/required extensions rejected.
- Renderer synchronously uploads model geometry/textures; shares vertex buffer and images within a model. ModelUploadStats reports surfaces, unique images, texture bytes and CPU submission ms (not GPU time). Opaque then center-sorted transparent passes; no culling/shadows/IBL yet.
- Async CPU open/import/save/reload and cancellation implemented; GPU upload still on render thread. Preserve last-good reload/model data, failed-save/history integrity, Play isolation and owned-resource cleanup.
- Existing editor: hierarchy/search/reparenting, Inspector/gizmos/Undo, project assets/import, collision/physics and third-person gameplay. User confirmed prior playable milestone works; published `29370d3`. No assumption of specific manual Linux/Windows coverage.
- Navigation: RMB fly or Tab toggle for trackpads; WASD, Space/Ctrl, Shift faster. Tab/Escape releases flight; focus/Play/dialog changes cancel capture. Escape stops Play. Preserve native Tab interception before egui focus navigation.

## Useful checks

```sh
cargo fmt --all -- --check
cargo test --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
python3 tools/check_headless.py
cargo run -p bozzard-editor-app -- --smoke work/editor-smoke --hardware --backend metal
git diff --check
```

Latest full test log: `/tmp/bozzard-pbr-render-tests.log`. Durable docs: README, docs/assets.md, docs/playable-demo.md, docs/roadmap.md. Earlier detailed handoff in `424098e:scratchpad.md`; older debugging history in `bc22cc8:scratchpad.md`.
