# Bozzard

A native 2D/3D game engine in Rust, with our own ECS and WebGPU rendering through `wgpu`. No Bevy dependencies.

The current slice includes scene objects, parent transforms, cameras, textured sprites, indexed cubes with depth and basic directional lighting, and scene save/load. It is an engine foundation, not a full editor or game exporter. PNG/JPEG textures and OBJ meshes can be imported and reloaded while running. Physics, audio, networking, and editor tooling remain future milestones.

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
```

The headless executable runs finite ticks as fast as possible and exits. It does not listen for clients yet.

## Workspace

| Package | Responsibility |
| --- | --- |
| `bozzard-ecs` | Generational entities, sparse-set storage, safe queries, resources, commands |
| `bozzard-app` | Serial system scheduling, compiled-in plugins, fixed ticks, bounded catch-up |
| `bozzard-scene` | Versioned JSON, persistent IDs, validated hierarchy, camera math, ECS instances |
| `bozzard-assets` | CPU image/OBJ imports, store-scoped handles, load states, last-good hot reload |
| `bozzard-render` | Native WebGPU, indexed geometry, texture sampling, depth, GPU readback |
| `bozzard-demo` | Embedded reference scene, movement/rotation systems, scene file helpers |
| `bozzard-player` | Window/input, scene controls, render extraction, GPU verification |
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
```

Ordinary Cargo tests require no GPU. They cover entity lifetimes, scheduling, scene hierarchy/validation, projection conventions, animation, scene round-trips, control commands, CLI save behavior, asset reload failure/recovery, and relocated asset references. Graphics checks are explicit and never silently skip.

The smoke suite preserves the original triangle check, then verifies texture quadrants, indexed cube depth occlusion in both draw orders, camera translation, resized targets, animated 2D/3D scenes, and identical images after save/reload. Imported-asset checks verify UV orientation, sRGB decoding, corrupt-file recovery, and mesh replacement. Actual PPM diagnostics appear in `work/gpu-smoke/`. Reference color checks tolerate two byte values; same-device save/reload and draw-order comparisons are exact. These checks are correctness fixtures, not performance or image-quality benchmarks.

Foundation CI passed on all three hosted platforms at [`d319721`](https://github.com/kaz0r/Bozzard/actions/runs/34019215092), including Linux presentation. The initial run exposed a missing X11 keyboard runtime, which is now installed explicitly. The asset slice adds CPU and GPU acceptance checks to the same matrix; see the latest run for its remote validation status.

## Development bundles

```sh
cargo build --release --locked -p bozzard-player -p bozzard-server
python3 tools/package.py --verify --window
```

This builds a host-native ZIP in `dist/`, including a macOS `.app` on macOS editable built-in/imported scenes, and their PNG/OBJ files. Default scene data, shaders and procedural textures are embedded, so the executables can run without the source checkout. Verification extracts the ZIP, runs a headless imported-scene save, loads that snapshot in the GPU suite, and optionally presents both native views from an empty working directory.

This is development demo packaging, not a general user-game export pipeline. OS runtimes and drivers remain prerequisites. Public distribution still needs license/notices, signing/notarization, installer choices, and minimum OS/runtime baselines.

## Cross-platform CI

[CI](.github/workflows/ci.yml) defines native Ubuntu x86-64/Vulkan, Windows x86-64/DX12, and macOS Apple Silicon/Metal jobs. Each runs lints, CPU tests, headless dependency checks, release builds, and rendering from extracted packages. Linux uses Mesa software Vulkan; Windows requests WARP; macOS requests Metal. Linux also presents both views under Xvfb. No missing-adapter skips are allowed.

[Hardware GPU](.github/workflows/hardware.yml) runs manually on a provisioned desktop runner with labels `self-hosted`, `bozzard-gpu`, and the OS label. It requires Rust, Python 3, Bash (Git Bash on Windows), working graphics drivers, and an interactive desktop. It verifies real GPU classification and both windows. Only the currently booted OS of a dual-boot computer is available. Run trusted revisions only on a personal hardware runner; external PR code is never dispatched there automatically.

Local Metal checks have passed on an Apple M2 Pro. The foundation matrix has passed on Linux/Vulkan (llvmpipe), Windows/DX12 (WARP), and GitHub-hosted macOS/Metal. Hosted offscreen checks do not establish native Windows/macOS desktop presentation; macOS presentation is tested locally, and Windows desktop presentation remains a hardware-runner check. OS labels and Cargo dependencies are pinned, while runner image contents and OS packages continue to receive updates. Additional GPU vendors and Intel macOS remain separate future coverage tiers.
