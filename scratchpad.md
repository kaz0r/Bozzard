# Bozzard working scratchpad

Updated: 2026-09-07. This is the live handoff log; update it at implementation and validation checkpoints.

## Current objective

Build the first usable native editor: hierarchy, viewport, component inspector, object creation/duplication/deletion, transform editing, asset import/assignment, undo/redo, save/load, and separate Play/Stop simulation. Keep our custom ECS and native wgpu renderer. The next larger workflow is a controllable character/collisions/game logic followed by exporting a user project; those follow the editor milestone.

The user explicitly requests continuous progress and next-step logging here. The earlier collider milestone was committed and pushed as requested. User confirmed gravity works and requested commit, push and CI verification. Terra supplied gravity regression tests and review; Luna supplied inspector controls.

## Manual validation reminder

- User manually verified snapping (Move/Rotate/Scale, Ctrl override, Undo), Escape cancellation, and camera framing (selection/all, 2D/3D, empty cases) successfully before publishing. This checkpoint is complete.

## Current state and next action

- Current step complete, committed and CI verified: collision response plus optional fixed-step gravity, inspector controls, runtime grounding status and gravity-lab.json (included in bundle manifest).
- Gravity configuration serializes; velocity/grounding are runtime-only. Disabling gravity/collider resets velocity. World-down swept movement supports landing and falling off edges. Grounded jumping is available; no dynamic pushing, rotational sweeps or compound movers.
- Try: cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/gravity-lab.json. Select Falling Box, Play, hover viewport, WASD. Stop restores authored scene.
- Validation: workspace tests including seven gravity regressions pass; native Metal smoke passed gravity_landing, response, Play isolation and pixel oracle. Headless dependency audit passed. Clippy with denied warnings and diff/format checks passed.
- Mouse untouched; no computer use. Previous collider commit passed cross-platform CI; new changes have local checks only.
- Latest small step: grounded-only Space jumping implemented and locally validated, uncommitted. Next: user tries jumping in gravity-lab; jump speed is now exposed in the inspector and validated locally. Next: user tries tuning it before Play.

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

## Finalization — pushed and CI passed

- Code commit: `68f57eaa1eeda57589878b9f745901bdea443ffa` (main, pushed). Includes collider milestone and the previously uncommitted camera/gizmo improvements.
- CI run https://github.com/kaz0r/Bozzard/actions/runs/34132231726 completed successfully on Ubuntu/Vulkan, Windows/DX12 and macOS/Metal, including tests, release builds, extracted package verification and GPU checks; Linux also verified the native editor UI.
- This final documentation-only result update skips redundant CI. No code changes after the verified commit. No computer use or mouse movement during finalization.

## Gravity — active implementation

- Added optional Gravity config and runtime-only GravityState, fixed-step integration in SceneDemo, inspector controls and Play grounded status. Gravity uses swept box motion; Space/Ctrl remains available only for boxes without enabled gravity.
- Added gravity-lab.json and native smoke falling/landing assertion. Validation in progress; no commit/push requested. Next: finish tests, native Metal smoke and document results.

## Collision response — implementation checkpoint

- Added move_box(world,id,world_delta) with translational swept SAT, earliest-contact stopping, tangential sliding, up to eight contact/recovery iterations, world→parent-local translation, contact IDs, and transactional failure. Initial penetration recovers or errors; compound movers with enabled child colliders are rejected. Obstacles are held static per query. No rotation sweeps, gravity or pushing.
- Editor Play supports selected box WASD movement, Space/Ctrl vertical movement and Shift faster while hovering 3D viewport. Edit-mode noclip remains unchanged. Status reports blocked/sliding contacts. Added standalone response-lab.json with Move Me, floor and two thin walls, included in development bundles.
- Native smoke now adds a temporary runtime-only floor and asserts a 20-unit downward sweep stops on contact, then verifies Stop preserves the authored scene.
- Initial workspace check passed. Delegated tests compiling/running now; root identified a corner-test fixture that had an open diagonal gap and requested real extended walls instead. Final validation pending. No computer use/mouse movement planned.

## Collision response — validated, uncommitted

- All ten independent response regressions pass: free motion, high-speed wall, floor/down/up, slide, corner, parent-local conversion, disabled obstacles, error rollback, initial-overlap recovery and rotated-wall sweep. Full workspace count: 53 passing tests.
- Reviewed test fixtures and solver: the original corner fixture contained a real diagonal opening; corrected the fixture to extended walls rather than changing the solver to block free space. No concrete solver defect found by the bounded independent review.
- Native smoke passed with marker `collision_response`: runtime-only floor blocks 20-unit downward motion, Stop retains authored data, save/load and pixel oracle remain correct. Clippy/headless checks pass. New demo included in debug package.
- Kept all files uncommitted and did not move the mouse. No CI run triggered for this step.

## Gravity — validation checkpoint

- Workspace tests and native Metal editor smoke passed, including a runtime-only falling box landing at the expected floor height. Seven independent gravity tests cover fall/cap, stable landing, leaving an edge, disabling, round-trip configuration and invalid inputs.
- Native output: work/editor-gravity-smoke. Initial Clippy found nested inspector conditionals; collapsed them and suppressed misleading Falling status when gravity/collider is disabled. Final Clippy with denied warnings and git diff --check passed.

## Response and gravity — publishing

- User verified the gravity demo works and authorized committing and pushing both pending milestones. Local tests, Clippy, headless audit and native Metal smoke passed. Committed as bc71c81 and pushed to main; cross-platform CI passed.

## Response and gravity — CI verified

- Commit bc71c8113b457919c30f2d0120b75d2ffd2f3bcb pushed to main.
- CI https://github.com/kaz0r/Bozzard/actions/runs/34157380591 completed successfully on macOS 15 / Metal, Windows 2025 / DX12, and Ubuntu 24.04 / Vulkan, including lint, tests, headless boundary, release packaging and pixel verification.
- User confirmed gravity works. No outstanding failures; next proposed focused step remains jumping/basic character control. No mouse movement during publishing or verification.

## Jumping — complete, uncommitted

- Small user-requested step: Space launches a grounded gravity box at 5 units/s in Play, ignores key repeat, and immediately consumes grounding. Ceiling impacts cancel upward velocity. No new dependencies. Scene/editor tests pass, including two new jump regressions (grounding, midair rejection, landing/re-jump, invalid speeds, disabled gravity, ceiling impact). Workspace Clippy with denied warnings, formatting and diff checks pass. Initial test fixture lacked required transform fields; corrected before passing. UI input compiled; no live mouse testing.

## Configurable jump speed — complete, uncommitted

- User requested a small follow-up: Gravity.jump_speed defaults to 5 units/s for existing scenes, validates positive finite values, appears in the inspector, and drives editor Play jumps from the runtime component. Scene/editor tests pass: custom value survives JSON round-trip, invalid values are rejected, editor jump uses configured 8 units/s and Stop preserves authoring. Workspace Clippy with denied warnings, formatting and diff checks pass. No mouse movement or native UI test.

## Gravity inspector polish — complete, uncommitted

- User authorized 2–3 small follow-ups. Chosen: estimated jump height/airtime and Reset gravity defaults. Estimates include capped descent and explain same-height/no-obstacle assumptions. Reset uses existing authored edit/undo path and preserves Enabled, avoiding unintended activation.
- Corrected obsolete collider hint claiming no gravity. No new dependencies or changes to simulation. Workspace Clippy with denied warnings, formatting and diff checks passed. Reset follows existing component editing/history machinery. No native UI/mouse test performed. Next: user tries tuning/reset in the inspector; pending jumping and inspector changes remain uncommitted.

## Editor convenience pass — complete, uncommitted

- User requested more small quality-of-life changes. Added case-insensitive hierarchy search by name/ID (flat results, traverses descendants, clear button, match count and empty state), actionable empty inspector guidance, and Rising/Grounded/Falling plus vertical-speed readout during Play. Search is transient UI state and does not edit the scene. Workspace Clippy with denied warnings, formatting and diff checks passed. Reviewed traversal to ensure children remain searchable when their parents do not match. No live UI or mouse use. Next: user tries hierarchy filtering and Play readouts; pending changes remain uncommitted.

## Object action polish — complete, uncommitted

- User requested another small pass. Duplicate/Delete buttons and keyboard actions now require a selection; tooltips explain shortcuts and subtree behavior. Successful creation/duplication clears hierarchy filtering, including keyboard duplication. Creating a Cube now switches to 3D, matching Sprite's switch to 2D; failed creation preserves the current view/filter. Existing undo/redo flow unchanged. Workspace Clippy with denied warnings, formatting and diff checks pass. Reviewed both button and keyboard duplication paths. No native UI or mouse testing. Next: try filtered creation/duplication and switching between Cube/Sprite; all pending QoL work remains uncommitted.

## Transform snapping — complete, uncommitted

- User authorized a slightly larger task. Implementing editor-only move/rotation/scale snapping with persisted increments and toolbar toggle; Ctrl temporarily inverts snapping while dragging. Relative to drag-start transform, preserving offsets and mirrored scale. Existing gesture undo remains one action per drag. Three focused snapping tests pass (signed relative movement, rotation wrap, modifier inversion, mirrored/nonzero scale, persisted/default preferences and invalid increments). All nine editor-app tests, workspace Clippy with denied warnings, formatting and diff checks pass. Native Metal smoke passed authored commands, Play isolation, collision response, gravity landing, save/load and pixel oracle; artifacts in work/editor-snapping-smoke. No manual gizmo drag or mouse movement. Next: user tries toolbar Snap and Ctrl override; pending jumping/QoL/snapping changes remain uncommitted.

## Cancel gizmo drag — complete, uncommitted

- Next small editor workflow: Escape restores active gesture start without adding Undo or clearing Redo. Core restoration uses transactional apply; editor suppresses document shortcuts during a drag to avoid finalizing the gesture behind the gizmo. All 12 editor-core tests pass, including cancel restoration, clean dirty-state, retained Redo, and no-op cancellation. Workspace Clippy with denied warnings, formatting and diff checks pass. Manual interaction is pending the user’s at-home test. No mouse movement. Next: remind user of the pre-commit manual checks before committing.

## Publishing editor improvements

- User explicitly requested commit/push now and will verify CI after their nap. This supersedes waiting for the manual pre-commit check; reminded them snapping and Escape cancellation remain manually unverified. Publishing jumping, configurable jump speed, inspector/hierarchy QoL, snapping and cancellation. Next authorized task: Frame Selection / Frame All camera navigation.

## Camera framing — complete, ready to publish

- Published prior jumping/editor improvements as 9deead1; user will review CI after nap, no CI success claimed.
- New uncommitted task: Frame Selection / Frame All buttons and F / Shift+F while hovering viewport in Edit. Bounds include selected descendants, imported vertices, transformed cubes/quads and layer filtering. Empty selections frame their origin; empty layers show a message. Perspective fitting respects aspect/FOV, orthographic fitting adjusts editor-only zoom. Scene cameras and history remain untouched. Reports clipping-range failures.
- User manually verified snapping/cancel/framing checks successfully. All 25 editor/core tests pass, covering transformed group bounds, imported mesh vertices, layer filtering, point fallback, portrait/wide perspective fit and orthographic centering. Workspace Clippy with denied warnings, formatting and diff checks pass. Native Metal acceptance smoke passed; artifacts: work/editor-framing-smoke. No CI verification performed yet.
