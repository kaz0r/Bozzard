# Bozzard working scratchpad

Updated: 2026-09-06. This is the live handoff log; update it at implementation and validation checkpoints.

## Current objective

Build the first usable native editor: hierarchy, viewport, component inspector, object creation/duplication/deletion, transform editing, asset import/assignment, undo/redo, save/load, and separate Play/Stop simulation. Keep our custom ECS and native wgpu renderer. The next larger workflow is a controllable character/collisions/game logic followed by exporting a user project; those follow the editor milestone.

The user explicitly requests continuous progress and next-step logging here. No agents are delegated. Prior authorization includes committing/pushing completed stages and watching CI.

## Current state and next action

- Reviewing the completed editor commits through `5f4d727` at the user's request. Earlier implementation checkpoints below are historical.
- Editor UI, command model, asset import, Play isolation, packaging, and Linux native UI CI are implemented. Prior CI results are recorded below.
- Review found: unchanged asset catalogs were reloaded during Undo/Redo; Save replaced the destination before validating imported assets; Finder launches used `/work` for default projects; failed pixel checks omitted the viewport diagnostic.
- Fixes in progress: preserve asset caches during ordinary history operations, prepare/validate saves before replacement, use a user-owned default project folder and platform workspace storage, retain failed pixel captures.
- Next: run regression/full workspace checks and native Metal smoke, inspect the final diff, commit/push fixes, and check CI. Native fast-drag gesture retest remains an explicit follow-up; automated smoke does not inject pointer gestures.
- No agents delegated. Maintain this section and append validation results as work proceeds.

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
- Committed as `219c11f` and pushed. CI run 34036164738 passed all three platforms (ubuntu-24.04/Vulkan, windows-2025/DX12, macos-15/Metal); the Linux job ran the packaged editor UI smoke under Xvfb (`editor_smoke_ok authored_commands play_isolation save_load native_ui_capture`). Windows/macOS hosted jobs verify the packaged editor via `--help`; real editor windows on those OSes remain hardware-runner checks.
- Editor milestone is now closed on CI. Remaining known limitations: fast-drag gizmo retest with a real pointer is still pending (fix is in and smoke passes, but no CUA drag re-verification yet); generic component reflection, arbitrary tab docking, and production gizmo ergonomics are future extensions. Next larger workflow per the objective: controllable character/collisions/game logic, then user-project export.

## Checkpoint — viewport pixel oracle

- Test audit found one real gap: the editor smoke only asserted the viewport was "not a clear color". Added a pixel oracle to `apps/editor/src/acceptance.rs`: it projects the smoke-created cube's center through the editor's own view projection and requires the majority of a 13×13 region at that pixel to be the expected teal tint (green-dominant thresholds chosen because the tint is multiplicative under the 0.3..1.0 diffuse light and the clear color is blue-dominant).
- The oracle immediately caught a real placement flaw: the smoke cube was created at [0.5, 0.5, 0], overlapping the white textured hero-cube, so the projected pixel was occluded (0/169 teal). Moved the smoke cube to an isolated [0, 2.5, 0]; oracle passes. This also proves the check fails when the expected object is not visibly rendered.
- Success line is now `editor_smoke_ok authored_commands play_isolation save_load native_ui_capture viewport_pixel_oracle`.
- Validated: fmt, clippy -D warnings, all 19 workspace test binaries, headless audit, native Metal smoke, and `tools/package.py --profile debug --verify --editor-window` (packaged editor smoke passes the oracle from an empty working directory).
- Committed as `380a446`, pushed. CI run 34040350695 passed all three platforms; the Linux job's packaged editor smoke passed with `viewport_pixel_oracle` under Xvfb.

## Review checkpoint — reliability fixes validated

- Added a regression covering corrupt imported files: transform Undo/Redo preserves the existing cache and handles; failed Save and Save As preserve destination bytes, dirty state, document path, and undo history.
- Split save preparation from atomic writing so the editor validates rebased imports before replacing the destination. Existing player/server save API remains intact.
- Default new projects now use fresh filenames under the user's Documents/Bozzard Projects, independent of Finder's working directory. Normal workspace persistence uses eframe's platform application-data location; smoke output stays explicitly directed.
- Viewport PPM is now saved before the pixel oracle runs so failed CI keeps diagnostic evidence.
- Passed workspace tests (30 tests), Clippy with warnings denied, headless dependency audit, and native Apple M2 Pro Metal editor smoke including the projected-cube pixel oracle. Final smoke isolation adjustment also passed the native Metal smoke. Ready to commit and verify CI.
