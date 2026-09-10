# Bozzard working scratchpad

Live handoff, condensed after publication of `bc22cc8`. Keep this file current at implementation and validation checkpoints; replace superseded status rather than continually appending it.

## Current status and next step

- **Active goal: Sponza with working lighting and shadows, visually verified.** Required scope from the agreed list: measured trial import; justified import budgets; scalable project-local packaging; shared textures; budgeted GPU uploads; performance counters and measured culling/draw improvements; filtering/mipmaps; PBR metallic/roughness, normal/tangent, occlusion/emissive; lighting/shadows; environment lighting, exposure and tone mapping. Completion requires actual Sponza renders and relevant runtime/CPU/GPU evidence, not only small fixtures. No publication requested. Initial source inspection: 103 primitives, 25 materials, 69 images, 192496 source vertices, 9.1 MiB geometry buffer, no required extensions. Beginning a reproducible CPU import probe.
- User now requests **a commit after each validated subsystem**; push was not requested for this goal. First subsystem: glTF surfaces share Arc-decoded images by source/alpha handling, and GPU upload shares matching image slices and alpha scans within a model. Opaque variants remain separate to preserve source alpha semantics. No limits raised. Sponza initially failed at 128 MiB after 7.6 s debug import; now succeeds at ~5.7 s with 25 unique images / 96 MiB, 192496 vertices and 262267 triangles. Unchanged-source scan ~21 ms. Repeat with `cargo run -p bozzard-assets --example inspect -- work/sponza/glTF/Sponza.gltf` (debug figures, not release benchmarks).
- Validation for texture sharing: workspace tests and denied-warning Clippy passed, including CPU sharing/alpha isolation and GPU allocation-count regression; native Metal player smoke on real Sponza passed. Baseline scene is `work/sponza/scene.json`; captures `work/sponza/baseline/loaded-3d.ppm`. This is base-color rendering only, NOT goal completion. Next: scalable project import packaging, then full material/texture pipeline and budgeted GPU uploads, lighting/shadows/environment/tone mapping and measured rendering improvements. Keep all goal requirements active.
- Texture sharing committed as **01bb996**. Next validated subsystem: glTF/GLB project packages now use `assets/<id>/model.gltf` with separate generated buffer/image files instead of base64 embedding. Existing single-file material OBJ conversion remains. Exclusive directory reservation and cancellation/stale-result cleanup protect neighbouring files; accepted/undoable imports retain resources. Tests delete original fixture downloads and reopen packaged GLTF/GLB, assert no base64 expansion, and check abandoned directory cleanup.
- Real Sponza editor import/save/reopen succeeded (~23 s debug end-to-end), resulting in a 97727-byte document and ~50 MiB folder at `work/sponza/imported/assets/Sponza-1/`. Reproduce with `cargo run -p bozzard-editor --example import_model -- SOURCE NEW_SCENE` (destination must be new). Full workspace tests, Clippy with denied warnings, and native Metal smoke of `work/sponza/imported/scene.json` passed (`work/sponza/packaged-smoke`). Packaging committed as **424098e**.
- **Model mipmaps validated, committing this checkpoint.** GPU-generated full mip chains downsample sRGB in linear light; model base-color textures use trilinear repeat filtering. Standalone/procedural textures retain nearest sampling. GPU texture-byte stats include mip storage. Renderer tests and workspace all-target Clippy passed; native Metal smoke includes checker minification linear-average oracle and odd/narrow image cases, and actual Sponza capture (`work/sponza/mipmaps/loaded-3d.ppm`) was inspected. Filtering is smoother, lighting still dark and unfinished. Offscreen smoke writes linear RGBA8Unorm bytes; final display/tone mapping must handle color space explicitly. Authored glTF samplers remain unsupported. Next: full PBR texture/material representation, then upload budgeting, lighting/shadows/environment/tone mapping and measured culling/performance work. Goal remains active and incomplete.

- Native editor milestone and **first playable third-person demo are implemented**, retaining the custom ECS/native wgpu and headless boundary. Automated validation passed; user confirmed “it all works” and authorized commit/push of this milestone. No detailed manual test breakdown was supplied.
- Previous published code: **`9462782`**, committed and pushed to `main`: collapsible Hierarchy branches and scratchpad cleanup, following reparenting/unparenting in `bc22cc8`. This commit records the playable milestone; use Git history for its publication hash.
- User confirmed CI passed for `9462782`; not independently rechecked this turn. Previous Hierarchy release `0406da6` also passed CI according to the user.
- Shared fixed-step Player Controller, camera-relative movement/jumping, follow/orbit obstruction camera, collectible/checkpoint/goal triggers and fall respawn are implemented. Inspector and editor/standalone input adapters are connected; `examples/demo/scenes/first-trail.json` reuses the local CC0 octahedron model for scenery.
- Longer-term: develop basic character control into a usable game-logic workflow, then user-project export. A compatible Sponza stress test remains a useful asset/upload performance investigation; no completed benchmark is recorded.
- Sponza download is available locally at `work/sponza/glTF/Sponza.gltf` (Khronos glTF-Sample-Assets version): 71 asset files, 50.2 MiB, each verified against GitHub blob hashes and all referenced resources present. Upstream README/license are in `work/sponza/`. The existing `/work/` ignore rule keeps it out of Git. Download only; import/render performance has not been tested yet.
- User authorized publication of the playable milestone and its documentation. Hosted CI for this milestone has not yet been checked; do not infer it from earlier CI or local validation.

## Current milestone: first playable third-person demo

User authorized the full playable milestone and, after confirming it works, its commit and push. Export remains a separate milestone. Goal: move from an editor with movable objects to an authored, playable game workflow, retaining the custom ECS and native renderer.

1. **Player Controller component:** designate the player and configure movement/jumping. Gameplay input must not depend on editor selection. Build on existing collision response, gravity and grounded jumping.
2. **Follow camera:** configurable distance/height, mouse orbit and collision avoidance to prevent walls from obscuring the character. Gameplay camera behavior remains separate from editor navigation.
3. **Basic interactions:** trigger volumes, collectibles, checkpoints and respawning after falling.
4. **Playable sample level:** imported scenery, obstacles, collectibles and a reachable goal. The same authored scene must work in editor Play and the standalone player.

**Implementation / validation checkpoint:**

- **115 workspace tests passed**, including **three input-boundary regressions** beyond the prior 112: combined physical/logical standalone dispatch, missing editor focus-release recovery and still-held raw-repeat safety. The six original parent-approved review fixes remain complete: safe checkpoint defaults/preserved tuning, atomic controller/active-camera Undo/Redo, physical editor gameplay with logical shortcuts, logical-point standalone orbit/scale reset, pre-advance modifier cancellation and persistent command errors.
- The two follow-up boundaries are complete under the approved conservative contract: standalone controller scenes reserve physical WASD/Space from logical commands (Colemak physical S/logical R cannot restart); restart uses physical R, while legacy scenes retain logical commands. Editor latches are preserved because egui-winit raw events discard native repeat information. Focus discontinuities mark unresolved held keys and display a yellow rearm hint after refocus: **press/release those physical keys inside the editor, then press again to play**. Observed releases clear hints per key. Same-window viewport/dialog cancellation does not create a focus hint and still requires release/repress. Seamless focus recovery is deliberately not claimed.
- `cargo fmt --all -- --check`, focused tests (33), workspace tests and Clippy with `-D warnings` (`--locked --offline`), headless dependency audit and diff checks passed. Headless First Trail ran 120 ticks / 16 entities. Latest logs: `work/playable-boundary-{focused-tests,workspace-tests,clippy,fmt-check,headless-audit,diff-check,server}.log`.
- Native **Metal / Apple M2 Pro** editor smoke passed authored commands, Play isolation, collision/gravity, async save/open/import/cancellation, native capture and viewport pixel oracle (`work/editor-playable-boundary-smoke`, `work/playable-boundary-editor-smoke.log`). It uses the established fixture, not pointer-played First Trail, the rearm hint or new Inspector gestures.
- Standalone First Trail Metal GPU smoke passed model/material/imported-asset/render/save-reload checks; native window presented **120 frames** (`work/player-playable-boundary-smoke`, `work/playable-boundary-player-{smoke,window}.log`). Winning route remains deterministic injected-input coverage, not automated OS pointer input.
- README and `docs/playable-demo.md` provide launch commands, controls, authoring constraints, review-fix cases and manual expected outcomes. User confirmed the demo works; specific pointer/focus, HiDPI/layout, Inspector, error-title and Windows/Linux test coverage was not supplied. No agent-performed manual gesture acceptance is claimed.
- Deliberate limits remain one root kinematic box player/root perspective camera, unconfined RMB drag, sampled trigger overlaps, conservative box camera probing and title-based standalone HUD; no full physics, skeletal animation, scripting, saved progress or export. Existing checkpoint tuning, legacy selected-box controls and Bozz memorial are preserved.
- Independent review confirmed all six original findings resolved. Final focused review of the two input boundaries found no issues; its remaining notes concern unperformed native manual acceptance. Parent inspected the final status, whitespace/index checks and test/Metal/headless logs.

**Next:** check hosted CI after publication, then discuss the standalone user-project export milestone. User confirmed the playable demo works; retain the documented manual checklist for regression testing and unreported platform/input edge cases. Further implementation/publication requires fresh authorization.

**Scope boundaries:** begin with a simple character shape. Defer skeletal animation, full physics and a general scripting system; this milestone should produce a small playable demo, not expand into every gameplay subsystem.

**Following milestone:** export that game as a standalone user project. Keyboard Hierarchy navigation and other small editor conveniences are deferred for now.

## Latest completed implementation: collapsible branches (`9462782`)

- Added per-parent disclosure arrows and Expand all / Collapse all. Flat search still traverses collapsed branches and disables bulk collapse controls; clearing search restores the stored branch state.
- Transient state in `apps/editor/src/hierarchy.rs`, separate from documents/history. New/Open reset it; removed IDs are pruned. A changed selection ancestry, explicit rename or successful reparenting reveals ancestors. Manual collapse stays collapsed while the selection is unchanged.
- Validation: **38 editor-core/editor-app tests passed** (22 core + 16 app), including three new tests for traversal/search, expand/collapse, selection reveal, manual-collapse persistence, reparented ancestry and deleted-ID pruning. Editor-app all-target Clippy with denied warnings, formatting and diff checks passed. Native Metal smoke passed all existing authoring/physics/async/pixel checks (`work/editor-collapse-smoke`); it does not exercise disclosure clicks. README updated; no mouse automation/manual gesture verification.
- User confirmed the collapsible-branches feature works; no detailed manual test breakdown was supplied. Published as `9462782`; user confirmed CI passed. Keyboard tree navigation is deferred in favor of discussing a larger milestone.

## Published implementation: Hierarchy organization

- Double-click selects and frames an object and descendants in the current viewport layer, without changing authored cameras/history. Works in filtered results.
- Inline rename uses a separate draft: Enter applies one transaction; Escape/focus loss cancels. Starting rename clears search and focuses/scrolls to the row. IDs remain unchanged.
- Right-click acts on the clicked row: Rename, Duplicate, Frame Selection, Delete, Unparent. Root objects cannot be unparented. Editing operations are guarded during Play/loading/captured navigation.
- Platform-primary menu shortcuts use readable text to avoid missing font glyphs:
  - Mac: Cmd+Return, Cmd+D, Cmd+Shift+F, Cmd+Backspace.
  - Windows/Linux: F2, Ctrl+D, Ctrl+Shift+F, Delete.
  - Alternate rename bindings remain supported; F/Shift+F still frame selection/all over the viewport. Unparent has no shortcut yet.
- Drag onto another row to parent; drop onto **Scene root** or the **blank area below rows** to unparent. Drop targets highlight. Row gaps/toolbar are not unparent targets; sibling ordering is not implemented.
- `crates/bozzard-editor/src/hierarchy.rs`: `Editor::reparent` computes inverse-parent × old-world, decomposes using scene YXZ TRS, and verifies local/world reconstruction. Preserves descendant world transforms and supports Undo/Redo. Rejects cycles, missing objects, Play, invalid/noninvertible transforms and unsupported shear/precision loss. Same-parent drops do not create history.
- UI integration: `apps/editor/src/main.rs`; viewport framing: `apps/editor/src/viewport.rs`. README contains user-facing controls.

## Published reparenting validation evidence

- **35 editor-core/editor-app tests passed** (22 core + 13 app); this is not a full-workspace test count.
- Reparent regressions cover subtree/world preservation, reflected/rotated/scaled parents, unparenting, Undo/Redo, restored clean state, cycle/missing-ID rejection, Play rejection, no-op history and transactional shear rejection.
- Editor-app all-target Clippy with denied warnings, formatting and diff checks passed after the unparenting follow-up.
- Initial reparent implementation passed native Metal acceptance on Apple M2 Pro: authoring, Play isolation, collision response, gravity landing, background save/open, queued imports, cancellation and viewport pixel oracle. Artifacts: `work/editor-hierarchy-smoke`. Headless dependency audit passed. This smoke preceded the blank-drop/menu follow-up and does not exercise the new pointer gesture.
- User confirmed Hierarchy framing, rename/context-menu functionality and the unparenting follow-up. Earlier snapping, Escape drag cancellation, camera framing and Tab fly mode were also user-confirmed. Do not infer more specific manual coverage than reported.
- Native Windows/Linux context-menu/pointer behavior has not been manually verified. Hosted CI verifies packaged editor UI on Linux/Xvfb; native macOS/Windows editor windows remain local/hardware checks.

## Established capabilities and constraints

- **Editor:** hierarchy/search, inspector, creation/duplication/deletion, bounded snapshot Undo/Redo, gesture coalescing/cancellation, snapping, camera framing, save/load/import, workspace persistence, separate Play/Stop. Camera navigation never edits the authored game camera. Panels are fixed/resizable, not arbitrary docking; component fields are explicit, not generic reflection.
- **Navigation:** RMB look/fly or trackpad-friendly Tab toggle over 3D viewport; WASD, Space/Ctrl vertical, Shift faster. Tab/Escape releases latched flight; focus loss, Play, 2D, dialogs and loading also release capture. Tab must be intercepted before egui focus navigation. Escape also stops Play or cancels an Edit gizmo gesture as appropriate.
- **Physics:** transformed box SAT queries and translational swept collision response/sliding; optional fixed-step gravity and configurable grounded jumping. Gravity config serializes; velocity/grounding are runtime-only. No dynamic pushing, rotational sweeps or compound movers; overlap broad phase is O(n²).
- **Assets:** static OBJ/MTL, glTF/GLB, multipart base-color materials, PNG/JPEG textures, alpha blend/mask. Resource-bearing imports are packed into self-contained glTF. Browser previews/search/details/usage, assignment, reload and remove-unused (catalog only, never source deletion). Corrupt reloads preserve last-good data. No full PBR, skeletal animation or mipmaps; transparency uses simple center sorting.
- **Async loading:** background CPU initial load/open/import/save validation/reload, progress and cooperative cancellation; serial file-drop queue capped at 32. Stale/cancelled imports clean up only their owned new files; save acceptance checks document revision. Undo/Redo shares asset snapshots rather than decoding again. GPU uploads and atomic scene writes still run on UI/render thread; codec calls finish before cancellation is observed. Player/CLI startup stays synchronous. Cancelling reload pauses polling until Reload.
- **Reliability:** preserve authored Play isolation, failed-save destination/history integrity, and headless dependency boundaries. Native smoke has a projected-object pixel oracle and writes diagnostic pixels before asserting. New projects live under Documents/Bozzard Projects, not the launch working directory.
- **Personal memorial:** `docs/images/bozz.svg` and README dedication honor the user's late dog **Bozz**; engine name is **Bozzard**. Preserve with care. SVG is editable vector artwork with transparent background.

## Useful checks and demos

```sh
cargo fmt --all -- --check
cargo test -p bozzard-editor -p bozzard-editor-app --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
python3 tools/check_headless.py
cargo run -p bozzard-editor-app -- --smoke work/editor-smoke --hardware --backend metal
python3 tools/package.py --profile debug --verify --editor-window --backend metal --hardware
git diff --check
```

Open a demo with `cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/<name>.json`: `gravity-lab`, `response-lab`, or `model-lab`.

## History policy

Keep only current work, unresolved limitations, useful evidence and the next logical step here. Completed debugging narratives and old publication instructions belong in Git history, not the active handoff. README, `docs/assets.md` and `docs/roadmap.md` hold durable product documentation.

Full pre-cleanup log (including older commits/CI links) is preserved at **`bc22cc8:scratchpad.md`**. Retrieve only when needed:

```sh
git show bc22cc8:scratchpad.md
```

Resolved historical issues include the temporary-directory reservation race (`a05f06b`) and the reported Ubuntu XKB panic, which the user identified as an older run; the current hosted workflow already installs `libxkbcommon-x11-0`.
