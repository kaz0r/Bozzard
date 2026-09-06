# Bozzard working scratchpad

Updated: 2026-09-06. This is the live handoff log; update it at implementation and validation checkpoints.

## Current objective

Build the first usable native editor: hierarchy, viewport, component inspector, object creation/duplication/deletion, transform editing, asset import/assignment, undo/redo, save/load, and separate Play/Stop simulation. Keep our custom ECS and native wgpu renderer. The next larger workflow is a controllable character/collisions/game logic followed by exporting a user project; those follow the editor milestone.

The user explicitly requests continuous progress and next-step logging here. No agents are delegated. Prior authorization includes committing/pushing completed stages and watching CI.

## Starting state

- Branch main, clean at `9b342ba` (pushed).
- Foundation and asset-stage CI passed Linux Vulkan/llvmpipe, Windows DX12/WARP, macOS Metal: https://github.com/kaz0r/Bozzard/actions/runs/34020090346.
- 22 CPU tests, local Metal pixel checks, extracted release bundles, 2D/3D presentation, and live asset reload have passed.
- `bozzard-scene` owns versioned documents and validated ECS instances. `bozzard-demo::SceneDemo` supplies fixed-step Spin simulation. `bozzard-render::SceneRenderer` consumes render data and owns GPU caches. `bozzard-assets` imports PNG/JPEG/OBJ synchronously. Player and headless server are separate binaries.
- Existing scene hierarchy membership is fixed per instance; editor transactions can rebuild validated edit instances. Preserve authored documents separately from the play world.

## Plan and current work

1. Inspect compatible egui/wgpu integration and choose the editor shell without changing runtime backend guarantees. IN PROGRESS.
2. Add a testable editor document/command model, selection, undo/redo, separate play state, and project-local imports.
3. Build a native editor UI with hierarchy, inspector, asset panel, viewport selection/manipulation, scene controls, and persisted workspace settings.
4. Verify real editing/play/save workflows, add appropriate CPU and native CI checks, document usage.
5. Commit/push the completed editor slice, watch CI, and record exact results and any remaining limitations.

## Validation and next action

No editor changes yet. Next action: verify egui renderer compatibility with wgpu 30 and implement the CPU editor model first. Do not claim editor completion until the native UI and save/play workflows actually run.

## Checkpoint — editor model started

- Added `crates/bozzard-editor` and `apps/editor` (binary will be `bozzard-editor`). egui/eframe 0.36.1 uses wgpu 30, so no backend downgrade or duplicate renderer API is needed.
- Implemented validated document transactions, bounded snapshot undo/redo, drag gesture coalescing, subtree duplication/deletion, Play/Stop isolation, save/rebase, project-local import copies, and geometry ray picking in the core. CPU tests are now being run.
- Native shell is not implemented yet. Next: render Bozzard into an egui-registered GPU texture; add hierarchy, inspector, asset import controls, viewport interactions, unsaved-change handling, and workspace persistence.
- Current changes are uncommitted. `cargo test -p bozzard-editor --locked --offline` is the first core check; inspect its output before expanding tests.

## Checkpoint — native shell implemented, compiling

- Core's initial 4 tests pass (subtree commands/history, gesture validation, Play isolation, ray selection).
- Native eframe shell now has resizable hierarchy/inspector/assets panels, authored-scene toolbar, file browser, import/assignment, unsaved-change prompts, persisted workspace, GPU viewport, ray picking, axis handles for move/rotate/scale, and independent pan/orbit/zoom.
- Added bounded `--smoke DIRECTORY`: exercises create/transform/undo/redo/play/save/load and requests a screenshot from the actual native UI. This has not run yet; do not assume it works before compilation and inspection.
- Next: fix compiler/API issues, run the native smoke on unlocked Mac, inspect UI screenshot, test real interactions, strengthen file/import/history tests, add editor build/smoke to CI and bundles, and update docs.

## Checkpoint — first native UI verified

- `cargo run -p bozzard-editor-app -- --smoke work/editor-smoke --hardware --backend metal` passed on Apple M2 Pro and produced `work/editor-smoke/editor.ppm` (converted to PNG for inspection).
- Visually inspected the first real editor screenshot: resizable hierarchy, viewport, inspector, assets, toolbar, and status are visible; the scene is rendered through native Metal. Initial UI uses fixed panel docking, not arbitrary drag-to-dock tabs.
- Added separate asset-cache revision so transform drags do not re-upload imports. Remaining validation work: actual pointer interactions, save/import edge cases, native screenshot/viewport pixel checks, CI and packaging, and documentation.

## Checkpoint — pointer interaction testing

- Opened the packaged debug editor using CUA. Verified clicking the 3D cube selects it and populates the inspector; changing X numerically moves it; Play animates with editing disabled; Stop + Undo restores the original authored transform and clean state.
- Found a fast-drag gizmo bug: drag initialization used the current pointer instead of the press origin, losing the first delta. Fixed to use the press origin; rebuild/retest in progress.
- Added editor binary and macOS app to development packages. CI now builds editor on every OS and runs extracted editor UI smoke under Linux Xvfb; hardware workflow also tests editor windows. Headless dependency audit still passes.
- Native UI uses egui fixed resizable side panels. Generic component reflection, arbitrary tab docking, and production gizmo ergonomics remain future extensions; current fields and axis handles are explicit.

## Checkpoint — editor slice validated and documented

- Re-ran `cargo run -p bozzard-editor-app -- --smoke work/editor-smoke --hardware --backend metal` with the gizmo press-origin fix in place: passed on Apple M2 Pro (`editor_smoke_ok authored_commands play_isolation save_load native_ui_capture`).
- Strengthened `bozzard-editor` core tests from 4 to 8. Added: save round-trip incl. save-during-Play persisting authored state and Save As discarding old-root history; project-local import copies (fresh ids, no overwrite, undo/redo of catalog entry, corrupt/unsupported files rejected before any bytes are copied); bounded 100-entry history with oldest changes falling off; active-camera subtree deletion rejected. One new test initially failed because the demo camera's initial X (4.0) collided with the test's transform values; fixed by offsetting test values.
- Documented the editor in `README.md` (intro, run command, Editor section with controls/shortcuts, workspace table rows for `bozzard-editor`/`bozzard-editor-app`, verification and CI notes, bundle instructions) and marked milestone 3 implemented-with-extensions in `docs/roadmap.md`.
- Full local verification passed: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, all 19 workspace test binaries, `python3 tools/check_headless.py`.
- Validated packaging locally: `python3 tools/package.py --profile debug --verify --editor-window --backend metal --hardware` passed; extracted player/server/editor ran from an empty working directory and the packaged editor smoke captured its UI (`work/editor-package-smoke/`).
- Remaining before the milestone is fully closed: commit/push this slice, watch the three-platform CI matrix plus a hardware run, and record exact remote results. Fast-drag gizmo retest with a real pointer is still pending (fix is in, smoke passes, but no CUA drag re-verification yet).
