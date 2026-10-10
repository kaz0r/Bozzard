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
| [x] Split the player | `apps/player/src/main.rs`, 3,000 lines, becomes `cli`, `view` (window, surface, device), `player` (mouse look, title, keyboard commands), `handler` (event loop) and `controls_tests`. One `Player::new` replaces three struct literals. | 17 unit tests and the three-OS device-recreation smoke in CI |
| [x] Renderer construction helpers | `scene/gpu_util.rs` holds `color_texture` (the HDR target four passes each redefined) and `fullscreen_pipeline` (seven copies of the same full-screen pass descriptor). Both run at construction and resize time only. | The renderer's 174 tests, including post-processing, optics, volumetric and temporal |
| [x] Runtime crate | `bozzard-demo`'s library, tests and benchmarks move to `crates/bozzard-runtime`, and `SceneDemo` becomes `SceneRuntime`. `examples/demo` is now scenes, assets and project files only, like the other examples. | Workspace tests; `tools/check_headless.py` for the server's dependency tree |

## Phase 2: settle duplicated behaviour

- **Runtime crate boundaries.** Document I/O (`save_json`, `save_atomic`,
  `relative_reference`, `prepare_document_from`) belongs in `bozzard-scene`. The Steam
  wrappers (`ShutdownGuard`, the overlay checks, idle callbacks) duplicate
  `bozzard_network::steam`. The Stellar-IX `factory` session code should be its own crate
  that the runtime installs, rather than code `SceneRuntime` calls directly. The demo
  movement plugin (`Position`, `Velocity`, `demo()`) is used only by the player smoke test.
- **Shared scene actions.** Blueprints and scripts each write transform, color, text,
  visibility, light, cursor, log and end-game effects themselves. Scripts skip transform
  writes that change nothing and blueprints don't. Quit replaces the game session from a
  script but edits it from a blueprint. One `actions` module should define each effect once.
- **Prefab destroy and trigger overlap.** There are six copies of "collect the prefab's
  members, refuse an active camera", and two trigger-overlap passes, of which only the
  blueprint one enforces the budget.
- **One session builder per host.** The player half is done: startup and reload (R/F6)
  share `start_session`, and every player renderer comes from one `RendererSettings`
  (`461f9b5`, see the bugs below). In the editor, `start_play` and `play_job`/`accept_play`
  still build Play separately.
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
- **Renderer buffers and layouts.** Five hand-rolled power-of-two buffer growths
  (`hud`, `submission`, `occlusion/gpu`, `instancing/arena`, `compute`) and the
  bind-group layout entries in `Shadows::new` and `SceneRenderer::new` repeat each other.
  The growth policies differ, and the arena also shrinks, while tests assert allocation
  counts. So a shared `GrowBuffer` has to state which policy it keeps.
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

Each was checked against `d2ad869` on 2026-10-10. Four were real and are fixed; one was not
a bug as stated.

- [x] **Fixed in `0ea52fb`.** Mesh export accepted 8,192-pixel images, but the importer caps
  them at 4,096, so an exported model could fail to re-import. A 4,097-pixel texture did
  export and then fail to import. 4,096 is the documented cap
  ([assets](assets.md), [texture compression](texture-compression.md)), and no importer,
  cooked model or compressed texture can hold more, so export now refuses wider images.
  One `MAX_IMAGE_SIDE` constant serves the decoder, cooked models, compressed textures and
  export. Test: `generated_mesh_export_only_writes_images_the_importer_accepts`.
- [x] **Fixed in `461f9b5`.** The player's reload lost multiplayer, game-pack hot-reload and
  profiling settings. All three were confirmed: after F6 Earth Factory's co-op menu no
  longer opened, R on a packed game turned hot reload back on, and a `--frames` run stopped
  recording GPU timings at R. Exported games keep R, because [exporting](exporting.md)
  verifies a physical-R restart; R now rebuilds the session through the same
  `start_session` as startup. Tests: `reload_keeps_the_game_pack_and_multiplayer_session_settings`,
  and the windowed `reload_keeps_gpu_profiling_in_frame_runs` (ignored; needs a desktop).
- [x] **Not a bug as stated.** Compute errors in the editor do set the status directly
  instead of going through `App::result`, but they still reach the console:
  `debug_end_frame` (`apps/editor/src/debug.rs`) logs every status change at the end of
  the frame, at Error level while `error` is set. A narrow gap remains. If something else
  changes the status later in the same frame, the compute error is never logged. If a
  later successful `App::result` clears `error`, it is logged as Info. Routing these
  per-frame errors through `App::result` would close that gap. But a persistent compute
  failure would then add a console row every frame, interleaved with the scene's own log
  lines, which defeats repeat collapsing. So it was left as is.
- [x] **Fixed in `270b629`.** The editor benchmark and the Debug pane computed frame-time
  percentiles with different formulas. For 30 frames, p95 was the 29th sample in the pane
  and the 28th in the benchmark. Both, and the player's `--frames` summary, now call
  `bozzard_diagnostics::percentile`, which uses nearest rank. Test:
  `percentiles_use_the_nearest_rank`.
- [x] **Fixed in `bd5abe0`.** `script_runtime` printed script log lines with `println!` from
  library code, on top of recording them in the engine log. Script lines landed on the
  editor's stdout and among the server's machine-readable output. Library code no longer
  prints. `Diagnostics::echo` lets a host without a console receive each log line. The
  player and server install `bozzard_diagnostics::terminal`, so `print` still writes one
  line to stdout there, and warnings and errors go to stderr. Tests:
  `an_installed_echo_receives_every_logged_line`, `script_log_lines_reach_the_terminal`.
