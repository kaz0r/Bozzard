# Bozzard working scratchpad

Live handoff, condensed after publication of `bc22cc8`. Keep this file current at implementation and validation checkpoints; replace superseded status rather than continually appending it.

## Current status and next step

- Native editor milestone is implemented. Current work improves scene organization while retaining our custom ECS and native wgpu renderer.
- Latest code: **`bc22cc8`**, committed and pushed to `main`: world-preserving Hierarchy reparenting and unparenting. User confirmed the unparenting follow-up works.
- **Next action: verify cross-platform CI for `bc22cc8`.** No result has been recorded yet. Previous Hierarchy release `0406da6` passed CI according to the user.
- **Collapsible Hierarchy branches implemented locally**, authorized by the user; details below. User confirmed the feature works and authorized commit/push. Next: verify CI for this publication.
- Longer-term: develop basic character control into a usable game-logic workflow, then user-project export. A compatible Sponza stress test remains a useful asset/upload performance investigation; no completed benchmark is recorded.
- Publishing scratchpad cleanup and the collapsible-branches feature together under the user's explicit commit/push authorization. Earlier reparenting code is published. Future work requires fresh publication authorization.

## Current implementation: collapsible branches (publishing)

- Added per-parent disclosure arrows and Expand all / Collapse all. Flat search still traverses collapsed branches and disables bulk collapse controls; clearing search restores the stored branch state.
- Transient state in `apps/editor/src/hierarchy.rs`, separate from documents/history. New/Open reset it; removed IDs are pruned. A changed selection ancestry, explicit rename or successful reparenting reveals ancestors. Manual collapse stays collapsed while the selection is unchanged.
- Validation: **38 editor-core/editor-app tests passed** (22 core + 16 app), including three new tests for traversal/search, expand/collapse, selection reveal, manual-collapse persistence, reparented ancestry and deleted-ID pruning. Editor-app all-target Clippy with denied warnings, formatting and diff checks passed. Native Metal smoke passed all existing authoring/physics/async/pixel checks (`work/editor-collapse-smoke`); it does not exercise disclosure clicks. README updated; no mouse automation/manual gesture verification.
- User confirmed the collapsible-branches feature works; no detailed manual test breakdown was supplied. User authorized commit/push. Next: verify this publication's CI. Possible later improvement: keyboard tree navigation; not yet authorized.

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
