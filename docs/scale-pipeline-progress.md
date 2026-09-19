# Scale and content pipeline verification

Section 5 of `FUTURE-PLAN.md`, implemented on `feature/scale-pipeline`.
Verification below was performed on 2026-09-18 and 2026-09-19 on Apple M2 Pro/Metal.
Cross-platform results are reported by the PR's native CI matrix.

## Feature coverage

| Requirement | Implementation and verification |
| --- | --- |
| Rendering scale | Consecutive opaque instancing, distance LOD and hysteresis, cancellable mesh simplification, conservative hierarchical-depth occlusion. Reference pixels, moving/camera/alpha cases, native controls and relocated player verified. [LOD](lod.md), [occlusion](occlusion.md). |
| Texture compression and budgets | BC3/ASTC cooking for images and embedded model maps, lossless fallback, bounded staged GPU residency, eviction/restoration and diagnostics. CPU, native GPU and portable exports verified. [Compression](texture-compression.md). |
| Async/additive scenes | File/content acquisition, preparation workers, cancellation, guarded publication, additive ownership/unload and checkpoints, Blueprint/script APIs. Up to 16 editor documents with independent history, visibility and saves. CPU and native editor/player checks verified. [Scene loading](scene-loading.md). |
| Prefabs | Nested sources, inherited variants, isolated source-hierarchy editing, reference remapping, overrides, refresh and history. CPU, native editing and relocated exports verified. [Prefabs](prefabs.md). |
| Material instances | Shared material sources, inheritance/overrides, static shader keywords, bounded source/pipeline caches and shared texture storage. CPU/GI/prefab/additive tests, native editing and exported/content-pack players verified. [Materials](materials.md). |
| Cooking and bundles | Dependency-based incremental cooking, platform targets, bounded packs, address catalogs, HTTPS acquisition, cancellation and immutable publication. CPU/HTTP fixtures, editor builder and source-independent packages verified. [Content packs](content-packs.md). |
| Importers | Custom TTF/OTF, variable axes and ordered fallbacks; editor/history/prefab/export integration verified. FBX uses the explicitly permitted [Blender conversion workflow](fbx-conversion.md); native FBX decoding is not implemented. [Fonts](text-rendering.md). |
| Level building | Typed terrain/sculpting and mesh collision, deterministic foliage, shared blockout brushes, snapping, grid/measurement, custom inspectors and persisted docking. CPU/history/prefab tests, native authoring/Play and relocated export verified. [Level tools](level-building.md), [editor extensions](editor-extensions.md). |
| Project ergonomics | Editor/CLI project creation, standalone 2D/3D samples, structural three-way scene merge with explicit conflicts. CLI, actual wizard and source-independent packages verified. [Projects](projects.md). |

## Optimization review

- Instancing reuses packed CPU buffers instead of allocating a temporary vector per
  batch. Shared material maps retain CPU pixels and GPU storage across instances.
  Asset budgets count aliased texture storage once and preserve required resources.
- Occlusion consumes completed results only while depth inputs and bounds still
  match. On this host at 320×320, 1,024 hidden spheres reduce color commands from 33
  to one after readback. Synchronized release medians are 2.722 → 1.203 ms static
  and 2.802 → 2.019 ms with a moving shutter. These are fixture measurements, not
  general FPS guarantees. Reference rendering remains available for comparison.
- Simplification uses attribute-aware reduction, per-surface borders, compact
  vertices and cache optimization. A flat 2,048-triangle fixture reduces to 410
  triangles with byte-identical 128×128 native pixels.
- Cooking reuses dependency digests and validated immutable outputs. Compression
  happens during cooking, never during GPU eviction/restoration. Background jobs
  reject stale generations and bound decoded data and publication work.
- Shader caches, font snapshots, content sizes, scene-document counts and authoring
  operations have explicit bounds. Docking uses fixed pane arrays.
- Terrain shares immutable decoded geometry/collision, checks source paths without
  per-frame allocation and caches world transforms by editor revision. Revision
  allocation advances monotonically instead of rescanning every older stroke.
  Foliage enforces spacing through a spatial grid and caps placement attempts.

## Native authoring evidence

- Custom inspectors: changed Spin from 15 to 360 degrees/second; Undo/Redo and saved
  `[0,360,0]` readback verified. The custom editor example builds through the public
  library entry point; headless crates remain independent of egui.
- Docking: detached/redocked Inspector, restored the saved layout after restart,
  dragged tabs, switched tabs and reset the layout. Regression coverage includes
  native-style coalesced motion/release events.
- Terrain: a native stroke changed 110 samples. Undo restored the original source
  revision; Redo restored the sculpt. Collision and prefab tests prove earlier
  immutable geometry survives later edits and save/reopen.
- Blockout and foliage: native placement shares existing geometry with one-step
  history. Scattering 12 three-object trees plus a group changes 116 → 153 objects;
  Undo returns to 116. Grid and measurement show a 10.568-unit span. Play/Stop passes.
- The saved workshop exports five cooked meshes (248,816 bytes). Its packaged player
  runs 20 Metal frames from an empty directory/PATH while the authoring folder is
  unavailable. The original source folder was restored after the check.

## Final verification

- Formatting, strict workspace/all-target Clippy and headless dependency audit pass.
- Release player, server and editor build successfully.
- Extracted development package passes GPU pixel checks and native player/editor
  smoke checks from an empty working directory.
- First Trail and Flap Woods relocated exports pass their route/game-loop checks.
- Compute Waves and Compute Numbers relocated exports pass GPU execution/readback
  checks, including resource retirement after stopping.
- All 77 editor tests pass, including floating-panel pointer ownership and the
  final terrain transform-cache regression. Strict all-target Clippy passes again.
- The full workspace suite passes on macOS, Linux and Windows. The macOS run
  reports 679 passing tests and nine explicitly ignored tests; native GPU checks
  run separately in package verification. Current matrix and export results are
  available in [PR #32's checks](https://github.com/kaz0r/Bozzard/pull/32/checks).

Reproduce the CPU checks with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --locked --offline
python3 tools/check_headless.py
```

For native package checks, build the three release executables, then run
`python3 tools/package.py --verify --window --editor-window --backend metal --hardware`.
Use the appropriate backend on another host. `.github/workflows/ci.yml` also verifies
relocated First Trail, Flap Woods and both compute examples on Metal, Vulkan and DX12.
The feature documents above include fixtures and focused reproduction commands.
