# First Trail — playable third-person demo

A small authored game, not a user-project exporter. One orange box player, three gold collectibles, a blue checkpoint, a green goal, a jump obstacle, a camera-obstruction wall and imported scenery. The static CC0 `assets/octahedron.obj` already in the repository supplies the crystals; no downloads are needed. The same JSON and fixed-step gameplay run in editor Play, the native player and the graphics-free server.

## Fastest start

From the repository root:

```sh
cargo run -p bozzard-editor-app --locked --offline -- --scene examples/demo/scenes/first-trail.json
cargo run -p bozzard-player --locked --offline -- --scene examples/demo/scenes/first-trail.json
```

Run one command at a time. `--offline` assumes the repository's dependencies are already cached; omit it for the first dependency download. On macOS append `--hardware --backend metal` to require native Metal hardware. Other native backends are DX12 and Vulkan.

In the editor, choose **3D**, optionally turn **Colliders** off for a clean view, and click **Play**. No player selection is necessary. The standalone player starts playing immediately.

| Input | Gameplay |
| --- | --- |
| Physical WASD positions | Move relative to the follow camera's horizontal direction; diagonals have the same speed |
| Space press | Jump when grounded; holding/repeats do not auto-jump |
| Right mouse drag | Orbit; release to stop orbiting. Deliberately unconfined/visible cursor, not pointer lock |
| Escape | Editor: Stop; standalone: close |
| Physical R position | Standalone controller scenes: restart the original scene; editor: use Stop then Play |

Editor gameplay input requires the focused 3D viewport (hover it, or drag there); typing, panels, dialogs, loading and focus loss clear input. Ctrl/Cmd/Alt combinations do not move the player. Edit-mode RMB/Tab fly navigation remains separate and never edits this camera. Standalone focus/cursor loss clears held input. After same-window viewport/dialog cancellation, release and press movement/jump keys again. After editor focus loss, releases made outside the editor may be missed: a yellow hint lists unresolved keys. **Press and release those physical keys inside the refocused editor, then press again to play.** The hint clears per observed release. This conservative latch prevents still-held raw repeats from restarting movement or jumping; seamless first-press focus recovery is not promised. Gameplay uses physical key positions in both apps (including non-QWERTY layouts); standalone controller scenes reserve WASD/Space from logical commands and restart only at physical R. Older scenes keep logical commands; editor shortcuts and text remain logical. Orbit sensitivity is degrees per logical point, independent of display scaling; moving the standalone window between scales resets its cursor baseline. The editor displays instructions/progress above the viewport; the standalone window title shows controls, collection count, checkpoint, falls and **YOU WIN!** (keep the window wide enough to read the title). Failed standalone commands, including physical R reload and F5 save, keep an **ERROR** prefix in the title across redraws/gameplay until the next successful command; normal movement and key repeats do not dismiss it.

## Shortest input-boundary recheck (not yet manually verified)

Use the two launch commands above, one at a time:

1. **Editor Play:** hover the 3D viewport, hold W/Space, switch apps, release them outside, return. The yellow hint names unresolved keys (unless native releases arrived). Press/release them inside the editor, then press again: hint clears and movement/jump works. Repeat while keeping them held through refocus: repeats must not move/jump. Release/repress to recover.
2. **Same editor window:** hold W/Space, leave/reenter the viewport or open/close a dialog without switching apps. Input stops and stays stopped until release/repress; no new focus-loss hint should appear.
3. **Standalone:** on Colemak if available, physical S/logical R moves backward without resetting progress; physical R (logical P on Colemak) resets. Holding Space must not auto-jump. An older scene retains logical R reload/Space behavior. Actual alternate-layout/focus gestures remain manual checks, not native smoke coverage.

## Full demo acceptance checklist (not automated pointer coverage)

1. **No selection dependency:** select the camera or a crystal, then Play. Hover the viewport and hold W: only the orange player moves, and the camera follows. Stop restores the original player/camera and all gold. Repeat with another selection.
2. **Complete the trail:** from the fresh start, keep near the center line. W collects the first gold. Just before the brown step, press Space while continuing W; cross the step and collect its gold. Walk over the blue pad: checkpoint feedback changes. Continue straight past the block on your left, collect the last gold, then enter the green pad: **3/3 / YOU WIN!** Movement stops at victory; orbit still works. The initial camera needs no orbit adjustment. The deterministic test wins with 340 fixed ticks of W and one jump on tick 80.
3. **Goal gating / once:** restart; bypass a gold and enter the goal: no win until all three are collected. Revisit a collected location: its gold stays hidden and the counter does not increase again.
4. **Grounding / collision:** push into the step without jumping: blocked/sliding. Hold Space after a jump: no repeated bounce after landing. Another fresh press jumps. The side wall blocks movement.
5. **Respawn:** before the blue pad, walk off either side: return to the start. After activating blue, walk off: return to blue with collected gold retained and the fall count incremented. Stop/Play or standalone physical R resets checkpoint, progress, falls and win.
6. **Camera:** near the tall side wall, right-drag until the wall lies between the player and desired camera: the camera pulls inward instead of crossing the collider. Orbit back: normal distance returns. Release RMB, leave the viewport, switch applications, and return: no stuck orbit/movement or hidden cursor. There is no gameplay cursor capture to release.
7. **Authoring / isolation:** Stop. Select Player and change **Player Controller → Move speed / Controller jump speed / Follow distance / Follow height**; Undo/Redo restores tuning. Save As to `work/trail-copy.json`, reopen it in both apps and verify the tuning. During Play, Save writes the editor's authored scene, not run progress. Stop restores its camera pose. Do not overwrite the sample just to test standalone F5: F5 is a runtime scene snapshot, not a progress save.
8. **Input safety:** in Edit, typing W/Space in Inspector or Hierarchy search must not play the scene. During Play, move the pointer to a panel or open a save dialog while holding W: movement stops. Switch focus away from the standalone player while holding W: no continued held input on return. Older `gravity-lab.json` still uses selected-box controls; its Space jump and default-player Space pause are unchanged.

9. **Review-fix authoring:** Stop; change the blue checkpoint to Goal, then back to Checkpoint: it succeeds using the safe player start, and the world respawn fields remain editable. Tune the respawn; reopening the dropdown must not reset it. Duplicate the root camera. Choose that camera in Player Controller, then Undo/Redo: the controller and active 3D camera change together. Repeat via the duplicate camera's **Use for 3D** button.
10. **Review-fix input/status parity:** if available, compare physical WASD on a non-QWERTY layout in both apps and equivalent logical RMB drags on 1×/2× displays. Move the standalone window between scales while dragging: no cursor-baseline jump. In editor Play, press Ctrl/Cmd/Alt while moving: input clears before the next simulation advance; leaving/reentering the viewport or Stop/Play while still holding a key must not restart motion. To test a standalone error safely, launch with `--save-path work` (an existing directory), press F5, then move/wait: ERROR stays visible. Press physical R successfully: ERROR clears. Do not alter the sample source to provoke an error.

## Authoring contract

Use **Player Controller** on one root object without Spin, with enabled Box collider and Gravity. Adding it in Inspector enables/preserves those components. `move_speed` and `jump_speed` are controller settings; the latter overrides Gravity's legacy selected-box jump tuning. Acceleration/fall speed still come from Gravity.

Choose the active **3D root perspective camera** with no Spin, Gravity, collider or trigger. The Controller camera dropdown and eligible camera **Use for 3D** button update the controller reference and active view together in one undoable transaction. Missing/wrong camera references, multiple controllers, invalid numbers, unsupported parented players/cameras, child collider movers and unsafe authored spawn points fail validation without replacing the current document. Remove the controller before disabling its required components or duplicating the player. Scenes without controllers keep their previous behavior.

**Trigger volume** is a separate non-solid box: enabling it converts the object away from solid collider/Gravity/Controller. Its center/size are local and follow the complete transform. Choose Collectible, Checkpoint (explicit **world-space** respawn position), or Goal. All enabled collectibles count toward every goal. Disabled volumes are ignored. Newly chosen Checkpoint actions default to the validated player world start, or the marker world position when there is no player. Existing checkpoint respawn tuning is preserved. Checkpoints should place the player above a safe floor, clear of solids and above Fall Y; positions inside authored solids or below Fall Y are rejected. No object references are needed for triggers. Simultaneous overlaps resolve in stable object-ID order, with goals evaluated after collection. Trigger dimensions can differ from the rendered marker; the existing Colliders debug overlay shows solids, not trigger volumes.

Serialized components remain optional schema-v1 additions; omitted controller fields use defaults. Example fields on a player object:

```json
"player_controller": {
  "camera": "camera",
  "move_speed": 4.0,
  "jump_speed": 6.0,
  "camera_distance": 6.0,
  "camera_height": 1.0,
  "camera_radius": 0.3,
  "orbit_sensitivity": 0.2,
  "fall_height": -8.0
}
```

Example trigger component on a separate object:

```json
"trigger": {
  "volume": {"center": [0, 0, 0], "size": [2, 2, 2], "enabled": true},
  "action": {"kind": "checkpoint", "respawn": [0, 0.65, -9]}
}
```

Actions can instead be `{"kind":"collectible"}` or `{"kind":"goal"}`. Run input, grounding, camera yaw/pitch, collection/checkpoint/win state and fall counters are runtime-only. Capturing a runtime scene preserves authored collectible drawables, but captures live transforms; it is not a saved game. Editor Save always uses the untouched authoring document.

## Repeatable checks

```sh
cargo fmt --all -- --check
cargo test --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
python3 tools/check_headless.py
cargo test -p bozzard-scene -p bozzard-editor --test gameplay --locked --offline
cargo run -p bozzard-server --locked --offline -- --scene examples/demo/scenes/first-trail.json --ticks 120
cargo run -p bozzard-editor-app --locked --offline -- --smoke work/editor-playable-smoke --hardware --backend metal
cargo run -p bozzard-player --locked --offline -- --scene examples/demo/scenes/first-trail.json --smoke --output work/player-playable-smoke --hardware --backend metal
cargo run -p bozzard-player --locked --offline -- --scene examples/demo/scenes/first-trail.json --frames 120 --hardware --backend metal
```

**Input-boundary retry validation:** 115 workspace tests passed (three boundary regressions beyond the prior 112), including combined standalone physical S/logical R dispatch, physical restart/legacy command preservation, missing editor focus-release recovery, per-key hint clearing and still-held unmarked-repeat safety. Same-window viewport/dialog cancellation does not create a focus hint. Prior checkpoint/camera/modifier/orbit/error regressions remain passing. Formatting, locked/offline workspace Clippy with denied warnings, headless audit and diff checks passed. Native Apple M2 Pro/Metal editor smoke, standalone sample GPU smoke, 120-frame window and 120-tick/16-entity headless run passed again. Logs: `work/playable-boundary-{focused-tests,workspace-tests,clippy,fmt-check,headless-audit,diff-check,editor-smoke,player-smoke,player-window,server}.log`; GPU artifacts: `work/{editor,player}-playable-boundary-smoke`.

The deterministic route/Play isolation tests inject shared gameplay input, not mouse events. Native smoke verifies rendering, existing editor commands and sample startup; it does **not** certify the manual checklist above. Adapter/inspector regressions exercise extracted logic, not native widget clicks, OS keyboard layouts, focus switching, visible rearm hints or cross-display drags. No manual mouse/trackpad, inspector gesture, HiDPI migration, error-title observation or Windows/Linux desktop acceptance is claimed. Independent follow-up review remains required.

## Deliberate limits

Kinematic box movement, not rigidbody physics, stairs/step-up, slopes or dynamic pushing. Trigger overlap is sampled each fixed tick, so very small volumes/extreme movement speeds can be missed. Camera avoidance uses enabled solid boxes with conservative expanded slabs (including transformed/sheared boxes), not render-mesh raycasts; corner cases can pull in early, and a target already inside a solid collapses the boom to the target. Configure adequate clearance for unusually wide aspect ratios/large near planes. Moving obstacles can invalidate a previously safe checkpoint; normal bounded collision recovery still applies at runtime. One player/one active follow camera; no skeletal animation, scripts, persistent saved progress, sound or general game export in this milestone.
