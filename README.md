# Bozzard

<p align="center">
  <img src="docs/images/bozz.svg" width="360" alt="A portrait of Bozz, with long dark-fringed ears, warm sable fur, gentle eyes and a white muzzle and chest." />
</p>
<p align="center">
  <em>In loving memory of Bozz ❤️<br />The inspiration behind Bozzard.</em>
</p>

Bozzard is a native 2D/3D game engine written in Rust, with its own ECS, a native editor, and WebGPU rendering through `wgpu`. It runs on Linux, Windows, and macOS without Bevy dependencies.

Author gameplay with visual Blueprints, Rhai scripts, or compiled Rust systems. Scenes, prefabs, assets, and scripts can be packaged into standalone native games.

[Quick start](#quick-start) · [Example games](#example-games) · [Editor](#editor) · [Documentation](#documentation) · [Verification](#verification) · [Exporting](#exporting)

## Quick start

Install [Rust through rustup](https://rustup.rs/). The repository pins **Rust 1.95.0** in [rust-toolchain.toml](rust-toolchain.toml).

| Platform | Requirements | Default graphics API |
| --- | --- | --- |
| Linux | C linker, Vulkan drivers, window-system and audio development packages; see the [CI package list](.github/workflows/ci.yml) | Vulkan |
| Windows | Rust MSVC toolchain and Visual Studio C++ build tools | DX12 |
| macOS | Xcode Command Line Tools (`xcode-select --install`) | Metal |

Run these commands from the repository root:

```sh
# Open Earth Factory in the editor, then click Play.
cargo run --release --locked -p bozzard-editor-app -- --scene examples/earth-factory/scenes/earth.json

# Play the same factory directly.
cargo run --release --locked -p bozzard-player -- --scene examples/earth-factory/scenes/earth.json

# Open a general-purpose scene and asset workshop.
cargo run --release --locked -p bozzard-editor-app -- --scene examples/demo/scenes/model-lab.json

# Run 120 simulation ticks without a window or GPU.
cargo run --release --locked -p bozzard-server -- --scene examples/demo/scenes/scene-lab.json --ticks 120
```

Use release builds when evaluating frame rate. Omit `--release` for a faster development build; add `--offline` once dependencies are cached. The first release build takes longer.

The player also runs an embedded reference scene with `cargo run --release -p bozzard-player`; add `-- --view 2d` for its sprite view. Use `--backend metal|dx12|vulkan` to select a backend, `--software` to require a software adapter, or `--hardware` to require an integrated/discrete GPU. Missing adapters report an error.

## Example games

### Earth Factory

The [Earth Factory Prototype](examples/earth-factory/README.md) is an isometric 3D factory driven by Rhai. It includes:

- Seeded exploration across a 17 × 17 region grid, chunk streaming, an `M` map, and smooth camera transitions.
- Tier 1 deliveries, grouped action bars, a three-page journal, and animated machine rotation.
- Mining, smelting, assembly, conveyors, storage, and manual collection of machine output.
- Day/night lighting with stars, mouse-wheel zoom, and a debug HUD with frame and simulation timings.

Start beside the landing pod, hold **F** on iron and copper deposits, then open **J** to deliver materials and unlock smelting. **E** collects nearby machine output or opens storage. **Escape** opens the menu while production continues. See the prototype's README for all controls and recipes.

Only the occupied region simulates production in this version. Region state lasts for the current Play session; the factory menu's Save and Load buttons are placeholders.

### Bozz-torio

[Bozz-torio](apps/bozz-torio/README.md) is a standalone 2D factory game with a generated 256 × 256 world, tier progression, electricity, original pixel-art sprites, autosaves, and Steam multiplayer.

```sh
cargo run --release --locked -p bozz-torio -- --offline
```

Its [game guide](apps/bozz-torio/README.md) covers controls, saves, multiplayer, the dedicated factory editor, and export. Opening its scene in the regular editor supports visual authoring; running its compiled factory simulation in editor Play uses the `bozz-torio-editor` entry point described there.

### More scenes to try

Pass a scene path to either the editor or player with `--scene PATH`.

| Example | Scene or guide | Demonstrates |
| --- | --- | --- |
| First Trail | [Scene](examples/demo/scenes/first-trail.json) · [Guide](docs/playable-demo.md) | Third-person movement, jumping, checkpoints, collectibles, and a goal |
| Target Range | [Blueprint scene](examples/demo/scenes/target-range.json) · [Rhai scene](examples/demo/scenes/target-range-rs.json) | The same first-person shooter implemented through either authoring path |
| Flap Woods | [Guide](docs/flap-woods.md) | A complete Blueprint game with scoring, pause, game over, and retry |
| Flap Woods Together | [Multiplayer guide](docs/multiplayer.md) | Steam lobbies, invitations, and host-authoritative play for 2–4 players |
| Gold Yard | [Scene](examples/demo/scenes/gold-yard.json) · [Guide](docs/gold-yard.md) | Physics playground and spawned convex bodies |
| Bonfire | [Scene](examples/demo/scenes/bonfire-lab.json) · [Guide](docs/bonfire.md) | Fire lighting and Blueprint-spawned embers |
| Materials and lighting | [Showcases](docs/showcases.md) | PBR materials, fog, and colored lighting |
| Atmosphere | [Scene](examples/demo/scenes/atmosphere-lab.json) · [Guide](docs/atmosphere-effects.md) | Particles, temporal AA, motion blur, and reflections |
| Middleware | [Guide](docs/middleware.md) | Audio, animation, timelines, UI, tilemaps, navigation, and particles |
| Compute | [Waves scene](examples/demo/scenes/compute-waves.json) · [Guide](docs/compute.md) | WGSL compute jobs and generated material textures |
| Sponza | [Setup and licensing](docs/sponza.md) | Imported architecture and renderer verification; assets download separately |

## Editor

The native egui editor shares its scene document, simulation, and renderer with the player. The workspace includes a scene hierarchy, component inspector, viewport, content browser, Blueprint and shader graph editors, scene settings, and diagnostics.

Use **File → New project…** for a 2D collection game or 3D exploration starter. Import textures and models through the content browser, edit objects and components, and save reusable hierarchies as prefabs. **Play** runs a separate simulation; **Stop** restores the authored scene. Undo/redo and scene saves operate on the authored document.

| Action | Control |
| --- | --- |
| Save / undo / redo | Ctrl/Cmd+S, Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z |
| Move / rotate / scale | W / E / R over the idle viewport |
| Frame selection / all | F / Shift+F over the viewport |
| 3D navigation | Right-drag to look; WASD to fly, Space/Ctrl for up/down, Shift for speed |
| Trackpad fly mode | Tab to toggle; Tab or Escape to release |
| Pan / zoom | Middle-drag / mouse wheel; 2D also supports right-drag panning |
| Snapping | Viewport Snap toggle; hold Ctrl during a drag to invert it temporarily |
| Cancel a transform drag | Escape |

Editing shortcuts yield to text entry, navigation, and Play. Imported models support selection and editing of individual surfaces; **Alt-click** selects the whole model. See [asset and submesh editing](docs/assets.md), [level building](docs/level-building.md), and [prefabs](docs/prefabs.md).

Scene opening, imports, asset preparation, and save preparation use background jobs. GPU uploads are staged on the render thread. Failed asset reloads retain the last working resource. Open **Debug** to inspect CPU stages, GPU passes, render counters, and logs, or export a profiling capture; see [debugging](docs/debugging.md).

## Player and headless tools

Gameplay scenes define their own controls. The native player provides **F5** to save a scene snapshot and **F6** to reload scripted/Blueprint scenes. Player Controller scenes use **WASD**, **Space**, and mouse-look, with physical **R** restarting the run; individual examples document their own behavior.

For scenes without gameplay logic, **1/2** switch views, **Space** pauses animation, arrow keys pan, **R** reloads, and **Escape** closes the window. F5 defaults to `work/saved-scene.json`; `--save-path FILE` changes the destination. A snapshot captures the current runtime scene state, which may include animated transforms.

```sh
# Write the embedded scene without opening a window.
cargo run -p bozzard-player -- --write-scene work/my-scene.json

# Run a finite simulation and save its resulting scene.
cargo run -p bozzard-server -- --scene work/my-scene.json --ticks 120 --save-scene work/simulated.json
```

The headless tool supports `--realtime` for 60 Hz pacing and `--ticks 0` to run until Ctrl-C/SIGTERM. It is a scene simulation harness; Steam multiplayer uses a player-hosted session. See [multiplayer setup and runtime behavior](docs/multiplayer.md).

## Performance and threading

Native player and editor Play run simulation on a dedicated worker while the main thread renders a prepared frame. Systems within a simulation tick execute serially. Add `--single-threaded` to either application to compare the two modes; headless runs and debugger stepping remain synchronous.

Rendering uses cached transforms, frustum and occlusion culling, and instancing. Consecutive scripted prefab spawns and removals without lifecycle callbacks share scene validation work. Earth Factory also streams distant chunk models and budgets surrounding residency changes across ticks.

FPS measures presentation intervals. CPU draw time excludes simulation and GPU execution; the factory HUD reports simulation CPU time and the remaining worker wait separately. Use the [Debug profiler](docs/debugging.md), [performance guide](docs/performance.md), and [architecture notes](docs/architecture.md) when investigating a bottleneck.

## Documentation

Detailed authoring instructions, supported formats, and current limits live in the focused guides:

| Area | Guides |
| --- | --- |
| Projects and scenes | [Project templates](docs/projects.md) · [Scene format](docs/scenes.md) · [Scene loading](docs/scene-loading.md) · [Level building](docs/level-building.md) |
| Gameplay | [Blueprints](docs/blueprints.md) · [Blueprint authoring depth](docs/blueprint-depth.md) · [Rhai scripting](docs/scripting.md) · [Game flow](docs/game-flow.md) |
| Assets | [Importing and editing](docs/assets.md) · [Prefabs](docs/prefabs.md) · [Texture compression](docs/texture-compression.md) · [LOD](docs/lod.md) |
| Rendering | [Materials](docs/materials.md) · [Lighting and baked GI](docs/lighting.md) · [Shader editor](docs/shader-editor.md) · [Occlusion](docs/occlusion.md) |
| Effects | [Atmosphere](docs/atmosphere-effects.md) · [Post-processing](docs/post-processing.md) · [Camera effects](docs/camera-effects.md) · [Fog](docs/fog.md) · [Volumetrics](docs/volumetrics.md) |
| Physics | [Physics surface](docs/physics.md) · [Mesh colliders and Rigidbody settings](docs/mesh-colliders.md) |
| UI and middleware | [Text and HUD rendering](docs/text-rendering.md) · [Audio, animation, UI, navigation, and particles](docs/middleware.md) |
| Compute and diagnostics | [WGSL compute](docs/compute.md) · [Debugging](docs/debugging.md) · [Performance](docs/performance.md) |
| Distribution and multiplayer | [Native export](docs/exporting.md) · [Content packs](docs/content-packs.md) · [Steam multiplayer](docs/multiplayer.md) |
| Engine development | [Architecture](docs/architecture.md) · [Editor extensions](docs/editor-extensions.md) · [Roadmap](docs/roadmap.md) |

## Workspace

| Package | Responsibility |
| --- | --- |
| `bozzard-ecs` | Generational entities, sparse-set storage, queries, and resources |
| `bozzard-app` | System scheduling, fixed ticks, background jobs, and the simulation worker |
| `bozzard-scene` | Scene documents, hierarchy, gameplay, physics, scripts, and middleware |
| `bozzard-assets` | CPU asset imports, handles, and hot reload |
| `bozzard-render` / `bozzard-render-assets` | WebGPU rendering and the editor/player asset upload bridge |
| `bozzard-text` / `bozzard-audio` / `bozzard-compute` | Text, audio, and compute support |
| `bozzard-diagnostics` | Profiling, logs, and render/simulation metrics |
| `bozzard-network` | Networking, session pacing, and Steam integration |
| `bozzard-project` | Project manifests, native export, and content bundles |
| `bozzard-editor` / `bozzard-editor-app` | Editor transactions and the native UI shell |
| `bozzard-demo` | Shared simulation setup and reference scenes |
| `bozzard-player` / `bozzard-server` | Native player and graphics-free simulation harness |
| `bozz-torio` | Standalone 2D factory game |

Workspace crates forbid unsafe Rust. The renderer stays independent of ECS and scene documents, and the headless server has no window, GPU, audio backend, or image-decoder dependency. [Architecture](docs/architecture.md) describes the dependency boundaries and runtime ownership.

## Verification

Run CPU checks from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 tools/check_headless.py
```

Graphics checks are explicit and require a compatible adapter; they fail rather than silently skip:

```sh
cargo run --release -p bozzard-player -- --smoke
cargo run --release -p bozzard-player -- --frames 3
cargo run --release -p bozzard-player -- --view 2d --frames 3
cargo run --release -p bozzard-editor-app -- --smoke work/editor-smoke
```

The player smoke checks rendered output, scene round-trips, and asset behavior. The editor smoke exercises the native UI, editing, Play, and save/load, and captures diagnostics in `work/editor-smoke/`. These are correctness checks; use release profiling for performance comparisons.

[CI](.github/workflows/ci.yml) covers Ubuntu/Vulkan, Windows/DX12, and macOS/Metal, including lints, tests, headless dependency checks, release builds, and packaged rendering. Hosted Linux and Windows checks use software adapters. The manual [hardware workflow](.github/workflows/hardware.yml) checks real GPUs and desktop presentation on provisioned runners. Consult the workflow runs for current results.

## Exporting

Use **File → Export game…** to package a standalone game that runs without Rust or the source checkout. **File → Build content pack…** produces reusable cooked content and address catalogs. See [native game export](docs/exporting.md) and [content packs](docs/content-packs.md) for platform requirements and packaging details.

To build a development bundle containing the player, editor, server, and example assets:

```sh
cargo build --release --locked -p bozzard-player -p bozzard-server -p bozzard-editor-app
python3 tools/package.py --verify --window --editor-window
```

The host-native ZIP is written to `dist/`; macOS bundles include `.app` packages. OS runtimes and graphics drivers remain prerequisites. Public distribution also requires appropriate licenses/notices, signing, and platform packaging.
