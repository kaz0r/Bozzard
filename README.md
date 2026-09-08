# Bozzard

<p align="center">
  <img src="docs/images/bozz.svg" width="360" alt="A portrait of Bozz, with long dark-fringed ears, warm sable fur, gentle eyes and a white muzzle and chest." />
</p>
<p align="center">
  <em>In loving memory of Bozz.<br />The inspiration behind Bozzard.</em>
</p>

A native 2D/3D game engine in Rust, with our own ECS and WebGPU rendering through `wgpu`. No Bevy dependencies.

The current slice includes scene objects, parent transforms, cameras, textured sprites, indexed cubes with depth and basic directional lighting, scene save/load, and a first native editor. PNG/JPEG textures and OBJ meshes can be imported and reloaded while running. It is not yet a game exporter; physics, audio, and networking remain future milestones.

## Run

Install [Rust through rustup](https://rustup.rs/) and Xcode Command Line Tools on macOS (`xcode-select --install`). The repository pins Rust 1.95.0.

```sh
# Start the rotating 3D scene on Metal (macOS), DX12 (Windows), or Vulkan (Linux).
cargo run -p bozzard-player

# Start in the 2D sprite view.
cargo run -p bozzard-player -- --view 2d

# Load the editable scene file rather than the embedded default.
cargo run -p bozzard-player -- --scene examples/demo/scenes/scene-lab.json

# Load file-backed textures and meshes (edit the source assets to hot reload).
cargo run -p bozzard-player -- --scene examples/demo/scenes/asset-lab.json

# Run the same scene for 120 fixed ticks without a graphics adapter or window.
cargo run -p bozzard-server -- --ticks 120
```

Windows needs Rust's MSVC toolchain and Visual Studio C++ build tools. Linux needs a C linker, Vulkan drivers and window-system development packages; the CI workflow lists Ubuntu packages. `--backend metal|dx12|vulkan` selects one graphics API explicitly. `--software` requires a software adapter; `--hardware` requires a reported integrated/discrete GPU. Missing adapters fail visibly.

## Player controls

| Key | Action |
| --- | --- |
| `1` / `2` | Switch to 2D / 3D |
| Space | Pause/resume fixed-step scene animation |
| Arrow keys | Pan the active camera in parent-space X/Y |
| F5 | Save current scene state to `work/saved-scene.json` |
| R | Reload the `--scene` source, or reset the embedded default |
| Escape | Close |

Use `--save-path FILE` to choose the F5 destination. Saving captures current object transforms (including animation and camera movement); it does not yet distinguish authored state from a play-mode world. Reload validates a replacement before changing the running world. A failed reload preserves the current scene and reports the error in the title/terminal. R reloads the original source, not the last F5 destination unless they are the same file.

```sh
# Write the embedded scene to a file without opening a window or requesting a GPU.
cargo run -p bozzard-player -- --write-scene work/my-scene.json

# Edit JSON, press R to reload, and F5 to save to the same file.
cargo run -p bozzard-player -- --scene work/my-scene.json --save-path work/my-scene.json

# Headless simulation can load and save the same document.
cargo run -p bozzard-server -- --scene work/my-scene.json --ticks 120 --save-scene work/simulated.json

# Open the native editor on a scene file (creates the path on first save).
cargo run -p bozzard-editor-app -- --scene work/my-scene.json
```

The headless executable runs finite ticks as fast as possible and exits. It does not listen for clients yet.

## Editor

`bozzard-editor` is a native egui/wgpu shell over the same scene document and renderer. It edits the authored scene with validated commands: hierarchy with create/duplicate/delete of subtrees, an inspector for names, parents, transforms, cameras, spin, and drawable layers/meshes/textures/colors, and a GPU viewport with click selection plus move/rotate/scale axis handles. Rotation rings are draggable along their arcs, with one undo entry per gesture. Hover highlights the nearest axis with a warm outline; the captured axis stays emphasized throughout the drag. In 3D, right-drag captures the mouse for world-upright noclip look (release right mouse or press Escape to release); hold right mouse and WASD to fly forward/back/sideways, Space up / Ctrl down, and Shift to move faster. Middle-drag pans and scroll dollies forward/backward. In 2D, right/middle-drag pans and scroll zooms. Reset view restores the authored camera viewpoint; navigation never changes the scene document. The 2D/3D toggle switches the edited layer.

Undo/redo is bounded to 100 changes and coalesces each drag into one entry. Play starts a separate simulated world; editing is disabled while it runs and Stop restores the untouched authored scene. Saving always writes the authored document, even during Play. Imports copy PNG/JPEG/OBJ files into an `assets/` folder next to the scene before adding them to the catalog, so projects stay relocatable; dropped files import (or open, for `.json`). Unsaved changes prompt before New/Open/close, and the workspace layout persists in the platform application-data directory. New scenes default to fresh filenames under `~/Documents/Bozzard Projects` (`%USERPROFILE%/Documents/Bozzard Projects` on Windows); use Save As to choose another location.

Shortcuts: Cmd/Ctrl+S save, Cmd/Ctrl+Z undo, Cmd/Ctrl+Shift+Z redo, Cmd/Ctrl+D duplicate, Delete removes the selected subtree. An active camera's subtree cannot be deleted.

### 3D box colliders

Objects may have an optional `BoxCollider` with a local-space center, full local dimensions, and an enabled flag. The inspector adds or removes the component and edits these values; the box follows the object's complete parent transform, including rotation, nonuniform or mirrored scale, and shear. Collision detection is discrete: touching counts as overlap. `SceneInstance::collisions(&World)` is available headlessly and returns sorted, unique overlap pairs by object ID. `SceneInstance::move_box(&mut World, id, world_delta)` sweeps one box through static enabled box colliders, stops and slides on contact, and reports the requested and applied motion plus contacts in `MoveResult`; the world delta is converted into the moving object's parent-local space. Rotation sweeps, rigidbodies, and dynamic pushing are not included.

Enable **Colliders** in the 3D viewport to see cyan wire boxes; overlapping boxes turn orange and the overlay lists pairs. These debugging wires show through scene geometry. The default demo includes colliders on the hero cube, coral cube, and floor.

Select an enabled collider object and start Play, hover the 3D viewport, and use WASD for world-horizontal movement, Space/Ctrl for up/down, and Shift for faster movement.

Try `cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/response-lab.json`, select **Move Me**, and press **Play**. Move toward the walls with WASD or down onto the floor with Ctrl; Stop resets the authored scene.

Movement supports one collider at a time; movers carrying enabled child colliders are rejected. Deep initial penetration is recovered within a bounded budget or returns an error without changing the world. Pair scanning remains unaccelerated, suitable for these initial demo scenes.

## Workspace

| Package | Responsibility |
| --- | --- |
| `bozzard-ecs` | Generational entities, sparse-set storage, safe queries, resources, commands |
| `bozzard-app` | Serial system scheduling, compiled-in plugins, fixed ticks, bounded catch-up |
| `bozzard-scene` | Versioned JSON, persistent IDs, validated hierarchy, camera math, ECS instances, box overlap queries |
| `bozzard-assets` | CPU image/OBJ imports, store-scoped handles, load states, last-good hot reload |
| `bozzard-render` | Native WebGPU, indexed geometry, texture sampling, depth, GPU readback |
| `bozzard-demo` | Embedded reference scene, movement/rotation systems, scene file helpers |
| `bozzard-editor` | Validated document transactions, undo/redo, play isolation, imports, ray picking |
| `bozzard-player` | Window/input, scene controls, render extraction, GPU verification |
| `bozzard-editor-app` | Native egui editor shell: hierarchy, inspector, assets, viewport gizmos |
| `bozzard-server` | Graphics-free scene simulation and snapshots |

Our crates forbid unsafe Rust. ECS/app use only the standard library. Scenes add `glam`, Serde, and JSON; the renderer never depends on the ECS or scene document crate. The player alone adds the image/OBJ importers; the server still has no image decoder, GPU, or window dependency. See [asset imports](docs/assets.md), [scene format](docs/scenes.md), [architecture](docs/architecture.md), and [milestones](docs/roadmap.md).

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 tools/check_headless.py
cargo run -p bozzard-player -- --smoke --backend metal
cargo run -p bozzard-player -- --frames 3
cargo run -p bozzard-player -- --view 2d --frames 3
cargo run -p bozzard-editor-app -- --smoke work/editor-smoke --backend metal
```

Ordinary Cargo tests require no GPU. They cover entity lifetimes, scheduling, scene hierarchy/validation, projection conventions, animation, scene round-trips, control commands, CLI save behavior, asset reload failure/recovery, relocated asset references, and editor document transactions (subtree commands, gesture coalescing, bounded history, play isolation, save/Save As, project-local imports, and ray picking). Graphics checks are explicit and never silently skip.

The editor smoke opens the real native UI, exercises create/transform/undo/redo/play/save/load, captures a window screenshot, and verifies the viewport rendered more than a clear color. Collider verification covers the editor's Colliders viewport toggle, orange overlap/cyan non-overlap wire boxes, and the overlap-pair overlay; headless checks exercise `SceneInstance::collisions(&World)`. Diagnostics land in `work/editor-smoke/`.

The smoke suite preserves the original triangle check, then verifies texture quadrants, indexed cube depth occlusion in both draw orders, camera translation, resized targets, animated 2D/3D scenes, and identical images after save/reload. Imported-asset checks verify UV orientation, sRGB decoding, corrupt-file recovery, and mesh replacement. Actual PPM diagnostics appear in `work/gpu-smoke/`. Reference color checks tolerate two byte values; same-device save/reload and draw-order comparisons are exact. These checks are correctness fixtures, not performance or image-quality benchmarks.

Foundation CI passed on all three hosted platforms at [`d319721`](https://github.com/kaz0r/Bozzard/actions/runs/34019215092), including Linux presentation. The initial run exposed a missing X11 keyboard runtime, which is now installed explicitly. The asset slice adds CPU and GPU acceptance checks to the same matrix; see the latest run for its remote validation status.

## Development bundles

```sh
cargo build --release --locked -p bozzard-player -p bozzard-server -p bozzard-editor-app
python3 tools/package.py --verify --window --editor-window
```

This builds a host-native ZIP in `dist/`, including macOS `.app` bundles for the player and editor on macOS, editable built-in/imported scenes, and their PNG/OBJ files. `--editor-window` additionally runs the packaged editor smoke and captures its UI. Default scene data, shaders and procedural textures are embedded, so the executables can run without the source checkout. Verification extracts the ZIP, runs a headless imported-scene save, loads that snapshot in the GPU suite, and optionally presents both native views from an empty working directory.

This is development demo packaging, not a general user-game export pipeline. OS runtimes and drivers remain prerequisites. Public distribution still needs license/notices, signing/notarization, installer choices, and minimum OS/runtime baselines.

## Cross-platform CI

[CI](.github/workflows/ci.yml) defines native Ubuntu x86-64/Vulkan, Windows x86-64/DX12, and macOS Apple Silicon/Metal jobs. Each runs lints, CPU tests, headless dependency checks, release builds, and rendering from extracted packages. Linux uses Mesa software Vulkan; Windows requests WARP; macOS requests Metal. Linux also presents both player views and runs the packaged editor UI smoke under Xvfb. No missing-adapter skips are allowed.

[Hardware GPU](.github/workflows/hardware.yml) runs manually on a provisioned desktop runner with labels `self-hosted`, `bozzard-gpu`, and the OS label. It requires Rust, Python 3, Bash (Git Bash on Windows), working graphics drivers, and an interactive desktop. It verifies real GPU classification and both windows. Only the currently booted OS of a dual-boot computer is available. Run trusted revisions only on a personal hardware runner; external PR code is never dispatched there automatically.

Local Metal checks have passed on an Apple M2 Pro. The foundation matrix has passed on Linux/Vulkan (llvmpipe), Windows/DX12 (WARP), and GitHub-hosted macOS/Metal. Hosted offscreen checks do not establish native Windows/macOS desktop presentation; macOS presentation is tested locally, and Windows desktop presentation remains a hardware-runner check. OS labels and Cargo dependencies are pinned, while runner image contents and OS packages continue to receive updates. Additional GPU vendors and Intel macOS remain separate future coverage tiers.

### Gravity

Open `examples/demo/scenes/gravity-lab.json`, select **Falling Box**, and press **Play**. The box falls onto the floor; use WASD over the viewport to move it off an edge. Stop restores the authored scene.

The inspector's **Gravity** component adds a box collider when needed, with positive world-down acceleration and a maximum fall speed. Play displays Grounded/Falling. Gravity runs at the shared fixed simulation timestep in editor, player and headless server. Disabling gravity or its collider resets fall velocity; Space/Ctrl vertical movement is available only without enabled gravity. Configuration saves with the scene; velocity and grounding reset on spawn.

This is kinematic box gravity, with no dynamic pushing or rigidbody simulation. Bodies step sequentially in object-ID order. `SceneDemo::check_simulation()` surfaces simulation failures; built-in applications check it.

In editor Play, press **Space** over the 3D viewport to jump with the selected grounded gravity box (set **Jump speed** under Gravity; default 5 units/s). Midair presses and held-key repeats do not jump. Ceiling contact cancels ascent. The headless API is `SceneInstance::jump_box(world, id, speed)`, returning whether a jump was accepted.

Jump speed is saved per object with the scene. Existing scenes that omit it retain the 5 units/s default. Adjust it before Play; higher values produce higher jumps.

The Gravity inspector estimates jump height and airtime (landing at the same height without obstacles, including the fall speed limit). **Reset gravity defaults** restores tuning values while preserving Enabled; Undo restores your previous settings.

The Hierarchy search filters object names and IDs without case sensitivity, including nested objects. Clear it with **×** to restore the full tree. During Play, the Gravity inspector displays **Rising**, **Falling**, or **Grounded** and signed vertical speed.

Creating a Cube switches to 3D; creating a Sprite switches to 2D. Successful creation or duplication clears the hierarchy filter so the new object is listed. Duplicate/Delete require a selected object; their tooltips show shortcuts and explain that children are included.

### Transform snapping

Enable **Snap** in the viewport toolbar, then drag a Move, Rotate, or Scale gizmo. **Snap settings** sets the increments (defaults: 0.5 local units, 15°, and 0.1 scale multiplier). Hold **Ctrl** while dragging to temporarily invert Snap. Changes are relative to the start of each drag, preserving existing offsets; this is not absolute world-grid alignment. Scale snapping preserves mirrored axes and avoids zero scale. Numeric inspector edits remain exact, each drag remains one Undo action, and preferences persist between editor sessions.

Press **Escape** during a gizmo drag to restore its starting transform without adding an Undo entry or clearing Redo history. Document keyboard shortcuts are paused while dragging.

### Framing the viewport

In Edit, use **Frame selected** (**F** over the viewport) to fit an object and its descendants, or **Frame all** (**Shift+F**) to fit drawable objects in the active layer. Imported mesh geometry and parent transforms are included. Selections without visible geometry center on their origin. Framing retains the 3D viewing direction and works in perspective and orthographic views; **Reset view** restores the authored camera view. It changes editor navigation only, without modifying scene cameras or Undo history. If geometry exceeds the authored camera's depth clipping range, the editor reports this instead of changing that camera.
