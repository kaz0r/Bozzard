# Bozzard working scratchpad

Updated: 2026-09-07. This is the live handoff log; update it at implementation and validation checkpoints.

## Current objective

Build the first usable native editor: hierarchy, viewport, component inspector, object creation/duplication/deletion, transform editing, asset import/assignment, undo/redo, save/load, and separate Play/Stop simulation. Keep our custom ECS and native wgpu renderer. The next larger workflow is a controllable character/collisions/game logic followed by exporting a user project; those follow the editor milestone.

The user explicitly requests continuous progress and next-step logging here. No agents are delegated. Current user instruction: finish, commit and push the pending work (2026-09-07), superseding earlier no-commit requests.

## Current state and next action

- Completed: draggable rotation rings, coherent axis highlights, noclip camera with Space/Ctrl vertical controls, 3D box collider components, SAT overlap queries, inspector controls and debug bounds/readout.
- Local validation passed: 43 tests across the workspace (the final cross-axis regression ran separately), Clippy with warnings denied, formatting, headless dependency audit, native Metal smoke on default and positive-overlap scenes, visual inspection of collider wires/readout.
- User now explicitly authorizes commit and push of the completed pending changes. Next: push, watch cross-platform CI, record final result.
- User mouse requirement: if computer use moves the mouse, restore it to its starting position in the bottom-left corner of the main monitor, otherwise the Mac locks. Finalization uses no computer use.
- Luna handled inspector/docs, Terra handled independent tests and a bounded SAT review. Root reviewed and integrated all changes. No active delegation remains.
- Detection only: no gravity, contact response, continuous collision detection or spatial acceleration (queries are O(n²)). Next proposed feature is physical collision response or a character controller, chosen one at a time with the user.
- Native right-button look/fly and middle-pan still need a hands-on feel check; CUA cannot hold those buttons while moving/typing. Unit math tests and native smoke pass.

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

## Review checkpoint — native gizmo capture fixed

- Refreshed the packaged Mac preview to the current build and reproduced the unresolved no-op drag. The full viewport captured drag responses before gizmo handles could act.
- Handles now begin from pointer press events inside their hit region and retain the gesture through release. The global gesture finisher waits for an active gizmo drag to complete.
- Real CUA pointer checks passed: Move X 0 → 1.108, Rotate X 12 → 65.570 degrees, Scale X 1.6 → 2.734. One Undo after each restored the initial value and clean document state.
- Finder-style launch also confirmed the default path is under /Users/andre/Documents/Bozzard Projects instead of /work.
- Initial review fixes committed/pushed as `4083d27`; CI run https://github.com/kaz0r/Bozzard/actions/runs/34053085889 is running. Gesture follow-up passed Clippy and native pointer validation; final native smoke and push next.

## Review checkpoint — final platform verification

- `9db246b` is pushed; native Metal smoke still passed after the pointer capture fix.
- Final CI: https://github.com/kaz0r/Bozzard/actions/runs/34053362291. All three jobs passed: Linux/Vulkan, Windows/DX12, and macOS/Metal, including extracted-package verification. Linux also ran the native editor UI/pixel smoke.
- The earlier run for `4083d27` was superseded by the final commit (macOS passed before cancellation; other jobs were cancelled by workflow concurrency).
- Next engine work after this review: a controllable character, collisions and game logic, then export of a user project. Arbitrary docking, generic component reflection, and automated Windows/macOS editor pointer checks remain future work.

- Final result: all-platform success confirmed from GitHub Actions for exact code commit `9db246b28803b95e3306cf002e5edb371e864ba8`. This final log-only commit skips redundant CI; engine code is unchanged from the verified commit.

## Quality-of-life follow-up — rotation rings and camera navigation

- User explicitly requested these fixes without automatic commits. All changes in this follow-up remain uncommitted; do not push them without subsequent authorization.
- Rotation rings now use projected arc hit testing with a 7-point tolerance and continuous angle unwrapping. A gesture captures one axis until release, including press/move/release events arriving together; axis endpoint handles still work.
- Native CUA verification: dragged an arc away from its axis endpoint, X rotation changed 12 → 51.556 degrees with Y/Z unchanged, and one Undo restored 12/25/0 and clean state.
- 3D camera uses an editor-only local pose: right-drag look, hold right + WASD/QE fly, Shift faster, middle-drag pan, scroll dolly. Camera input captures the viewport press directly. 2D retains pan/zoom. Reset view clears offsets. Authored game camera and Play remain separate.
- Native scroll/dolly was visually verified. CUA only exposes primary-button dragging, so right-button look/fly and middle-button pan have not been directly exercised through CUA; hands-on verification remains useful. Unit checks cover fly heading and ring hit/450-degree angle continuity.
- All 32 workspace tests and Clippy with warnings denied passed. Native Metal smoke/pixel oracle passed before the final overlap-capture adjustment; final smoke is being rerun now. README controls updated.

## Quality-of-life follow-up — gizmo axis highlight (uncommitted)

- User request: rotation rings and move/scale arrows already grab well, but nothing shows which axis is hovered or grabbed. Implemented in `apps/editor/src/viewport.rs`, left uncommitted per the no-commit instruction above.
- Thicker base drawing: axis lines 2.0 → 3.0, rotation rings 2.5 → 3.5, endpoint handles 10 → 12 px. Hovered axis: line 4.5 / ring 5.5, handle 14 px, translucent glow underlay plus white handle outline. Grabbed axis: line 6.0 / ring 7.0, handle 16 px, stronger glow, white outline, white axis label. While any axis is grabbed, the other two fade to 35% brightness. Hover highlighting reuses the exact press hit predicates (22 px handle rect, 7 pt ring tolerance), and hover is suppressed on non-grabbed axes during a drag.
- Validation: `cargo fmt --check` clean, workspace Clippy `-D warnings` clean, all workspace CPU tests pass, native Metal smoke passes with the pixel oracle (`editor_smoke_ok ... viewport_pixel_oracle`), and the smoke screenshot confirms the thicker idle gizmo renders.
- Live verification: the user confirmed the hover highlight and grabbed-axis glow work in the running editor. (During my CUA session the desktop was shared with a concurrent session that kept relaunching its own preview editor and stealing focus, so scripted drag/hover captures were unreliable; I stopped all instances I launched and left the user's own testing to confirm. The scene file under `work/` absorbed a few test edits but is an untracked, regenerated smoke artifact.) Do not commit or push without subsequent authorization.

## Highlight refinement — in progress, uncommitted

- Reviewed the user's glow additions. Multiple rings could independently highlight at intersections, while each 64-segment ring painted its glow separately, producing overlapping caps and lumpy strokes.
- Built one shared nearest-axis picker for hover and press with a deterministic tie break; only one axis is emphasized. Capture occurs before painting and the emphasized axis is drawn last.
- Replaced wide translucent glows with continuous joined paths, a narrow dark outline, and a warm highlight. Handles stay 10 points and labels stay 12 points across states, avoiding jumps and overlaps. Idle axis colors remain RGB; other axes dim during dragging.
- Face-on rotation rings remain available when their axis shaft projects to almost zero length. Clipped ring pieces are separate paths instead of being connected across invisible gaps.
- Four editor-app tests passed, including overlap selection/out-of-bounds hit rejection, clipped path joins, full-turn ring angle continuity, and camera-relative flight. Workspace Clippy with warnings denied and the native editor build passed.
- Next: inspect native hover/drag/Undo visuals, run final Metal smoke and full workspace tests, record results. Do not commit/push.

## Highlight refinement — validated, ready for user review

- Visually inspected the isolated `Bozzard Highlight Review.app`: joined outlines are smooth, handle/label sizes stay stable, and a single warm-colored ring is emphasized above intersections.
- Native CUA arc drag (away from endpoint handles) changed Z rotation 0 → 49.375 degrees with X/Y unchanged. One Undo restored 12/25/0 and the clean document. Endpoint dragging and Undo also passed. Closed only the isolated preview afterward.
- Full workspace tests: 34 passed. Clippy `--workspace --all-targets -- -D warnings`, formatting, native build, and Metal acceptance including the viewport pixel oracle all passed. The earlier usage-limit-blocked final smoke is now completed successfully.
- Existing limitation: CUA cannot hold right/middle mouse while dragging or typing, so right-button look/fly and middle-pan still need hands-on verification; native scroll/dolly was checked in the previous follow-up.
- All requested quality-of-life changes remain uncommitted, as requested. Next: user review of highlights and navigation; no automatic commit or push.

## Noclip follow-up — in progress, uncommitted

- User requested Space/left Ctrl instead of Q/E and proper noclip rotation. Egui exposes a combined Ctrl modifier, so left Ctrl works and right Ctrl also descends.
- Found the old look offset was composed inside the authored camera's tilted coordinate frame, causing yaw around a tilted up axis. The viewport also used bounded screen-pointer delta without mouse capture.
- Replaced offsets with a standalone editor camera initialized from the authored world pose: world-Y yaw, local pitch clamped short of vertical, no roll, world position independent of look. WASD follows the view; Space/Ctrl follow world up/down. Diagonal speed is normalized.
- Right-button flight now captures/hides the cursor and uses raw mouse motion when available. Windows uses confined capture; macOS/Linux use locked capture. Release, Escape, focus loss, Play, or switching to 2D restores the cursor. Editing shortcuts are suspended during captured flight.
- Reset view and New/Open initialize from the new authored camera; scene documents remain unchanged. Old serialized navigation offsets are ignored. Validation in progress; do not commit/push.

## Noclip validation — complete, uncommitted

- All 35 workspace tests passed; Clippy with warnings denied, formatting, diff checks, and native Metal smoke including the viewport pixel oracle passed.
- Regression checks verify authored-pose initialization, position unchanged by rotation, world-upright yaw after a tilted starting pose, world-vertical ascent/descent, forward movement along the view, normalized diagonal speed, and bounded pitch after large mouse deltas.
- README and viewport hints now describe Space/Ctrl. Combined right-button mouse/keyboard capture still requires a hands-on feel check: the available CUA interface cannot hold right mouse while moving/typing. No claim of direct CUA verification for that gesture.
- No commits or pushes. Next action: user can try right-button noclip flight; release right mouse or Escape restores the cursor.

## Box collider milestone — implementation checkpoint

- Optional serialized BoxCollider (local center, full size, enabled) added to scene schema without breaking old files. Scene validation rejects bad/overflowing bounds before spawn; ECS spawn/capture round trips the component.
- SceneInstance::collisions(world) returns enabled transformed boxes and deterministic unique overlap pairs; headless with existing dependencies. SAT checks six face normals and nine edge-cross axes, supporting parent rotation/nonuniform scaling, reflection and shear. Touching counts; no gravity, physical response, CCD, or accelerated broad phase (O(n²)).
- Editor inspector add/remove/enable/size/center delegates completed by Luna; independent integration tests completed by Terra (five passing). Reviewed both agents' changes locally. Root integrates query/Play isolation, viewport rendering and acceptance checks.
- Colliders toggle defaults on in 3D. Cyan wire boxes turn orange when overlapping; overlay lists count and up to four pairs. Clip edges in homogeneous WebGPU space before perspective division. Editor query uses live Play ECS or current authored document.
- Default demo hero/floor/coral cube now have unit local colliders (scaled by transforms). Native smoke's created cube also has a collider and asserts it appears in the query.
- Initial workspace check and Clippy passed. Next: full tests/headless boundary, Metal smoke, visually verify bounds and overlaps, final handoff. No commit/push authorization for this slice.

## Box collider milestone — validation checkpoint

- Reviewed delegated inspector/docs and all five initial independent integration tests; all pass. Root added editor collider undo/redo and live Play-world isolation regression plus near-plane wire clipping test.
- Full workspace suite (42 tests so far), Clippy, and headless dependency allowlist passed. No new dependencies and server still has no graphics/window crates.
- Native Metal smoke passed for the default demo and a temporary positive-overlap fixture. Visually inspected native screenshot: 4 enabled collider boxes, orange hero/floor bounds, cyan non-overlapping bounds, inspector component fields, overlay `Overlaps: 1` / `floor ↔ hero-cube`. Screenshot at work/editor-collider-overlap-smoke/editor.png; no scene files outside ignored work/ were changed for this positive-overlap test.
- Terra's bounded independent review found no concrete SAT/validation/order defect and suggested a cross-axis-only separating case. That last regression is being added before completion.

## Finalization — authorized commit/push

- Added the final fixed edge×edge-only separating fixture: all six face-normal intervals overlap but the SAT query correctly rejects the pair. Six collision integration tests passed, and final workspace Clippy/format/diff checks passed.
- Finishing the pending editor quality-of-life work together with the collider milestone. Earlier uncommitted/no-push entries above are historical; latest user authorization supersedes them.
