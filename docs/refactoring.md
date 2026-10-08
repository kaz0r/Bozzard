# Engine refactoring roadmap

This roadmap comes from a survey of `main` at `aa95577` on 2026-10-09. It covers four
areas: the runtime crate, `bozzard-scene`, the player and editor hosts, and the renderer and
asset crates. Each phase lands as reviewable commits that keep tests green.

Rules:

- **Phase 1** only moves code. Behaviour and public API stay identical, and the compiler and
  existing tests check every step.
- **Phase 2** removes duplication that has already drifted apart, so each item states the
  behaviour it settles on and adds a test for it.
- **Phase 3** touches per-frame hot paths. Record the benchmarks in
  [batch renderer optimizations](batch-renderer-optimizations.md) before and after each
  change, and keep the pixel-identity checks passing.

## Phase 1: structure only

| Item | What moves | Guarded by |
| --- | --- | --- |
| [x] Split `bozzard-scene` script runtime | `script_runtime.rs`, 4,600 lines, becomes `script_runtime/` modules for host, conversion, the Rhai API groups, compile, reload, tick, commands, loading and tests. `pub use` lists stay. | 28 unit tests in the file, plus demo, editor and network script tests |
| [x] Split `bozzard-assets` root | `lib.rs`, 2,800 lines, becomes `store`, `source`, `import/{obj,gltf,image}` and `portable`. | 52 unit tests, 16 of them importer tests |
| [x] Editor job boilerplate | `Loading` forwards fraction, label and cancellation through one `Progress` handle (`Job::progress`) instead of four 11-arm matches. | Editor app tests and the editor smoke run |
| [ ] Split the player | `apps/player/src/main.rs`, 3,000 lines, becomes `cli`, `view`, `keyboard`, `frame_stats` and `handler`. One `Player::new` replaces three struct literals. | 17 unit tests and the three-OS device-recreation smoke in CI |
| [ ] Renderer construction helpers | `hdr_target`, `fullscreen_pipeline`, bind-group layout entries and a growable buffer replace 5–7 copies each in the post-processing and shadow passes. These run at construction and resize time only. | Post-processing, optics, volumetric and temporal GPU tests |
| [ ] Runtime crate | `bozzard-demo`'s library moves to `crates/bozzard-runtime`. `examples/demo` keeps its scenes, tests and benchmarks. Document I/O (`save_json`, `save_atomic`, …) moves to `bozzard-scene`; the Steam wrappers go to `bozzard_network::steam`. | Workspace tests; `tools/check_headless.py` for the server's dependency tree |

## Phase 2: settle duplicated behaviour

- **Shared scene actions.** Blueprints and scripts each write transform, color, text,
  visibility, light, cursor, log and end-game effects themselves. Scripts skip transform
  writes that change nothing and blueprints don't. Quit replaces the game session from a
  script but edits it from a blueprint. One `actions` module should define each effect once.
- **Prefab destroy and trigger overlap.** There are six copies of "collect the prefab's
  members, refuse an active camera", and two trigger-overlap passes, of which only the
  blueprint one enforces the budget.
- **One session builder per host.** The player's reload (R/F6) skips three startup steps:
  `enable_multiplayer`, `disable_hot_reload` for game packs, and profiling. Exported games
  still accept R. In the editor, `start_play` and `play_job`/`accept_play` build Play
  separately.
- **Shared Play input.** Key → UI input mapping, wheel scaling, "the scene owns the
  keyboard", focus-loss handling, chat routing and audio pausing are written twice, once
  in the player and once in the editor viewport. Modifier blocking already differs between
  them.
- **One way to start an editor job.** The "wait for the current operation" guards use
  four different messages, and the interaction reset before a job (drag, canvas drag,
  timeline scrub, gameplay controls) is repeated in four places. A `begin_job` helper
  should settle on one message and one reset.
- **Component rows own spawn and capture.** `ComponentType` gains the
  `spawn`/`capture`/`validate` hooks middleware entries already have. That removes the
  three hand-kept core field lists in `bozzard-scene/src/lib.rs`.
- **Mesh building.** The OBJ and glTF importers share vertex caps, index checks, normals
  generation and part limits through one `MeshBuilder`.

## Phase 3: hot paths, measured

- Split `bozzard-render/src/scene.rs` (3,200 lines). Then break `draw_frame_inner`, about
  1,240 lines, into its phases: validation, targets, residency, visibility, uniforms,
  batching, shadows, uploads, color, post and finish.
- Implement the synchronous `upload_model`/`upload_image` through the staged
  `PendingUpload` path, after a parity test proves identical pixels and stats.
- Fail shader splicing loudly: `splice_once` instead of `replace`, which silently does
  nothing when the text is missing. Then move graph WGSL generation from `bozzard-scene`
  into `bozzard-render-assets`.

## Suspected bugs found by the survey

Verify each one before fixing it:

- Mesh export accepts 8,192-pixel images, but the importer caps them at 4,096, so an
  exported model could fail to re-import.
- The player's reload loses multiplayer, game-pack hot-reload and profiling settings
  (see Phase 2).
- Compute errors in the editor set the status directly instead of going through
  `App::result`, so they never reach the console.
- The editor benchmark and the Debug pane compute frame-time percentiles with different
  formulas.
- `script_runtime` prints script log lines with `println!` from library code.
