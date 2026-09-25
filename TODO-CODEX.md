# Codex: runtime, reliability and game integration

Planning snapshot: 2026-09-24, based on `main` at `27381da`. The implementation
items below are complete on `codex/todo-runtime-reliability`. Linux local tests,
packaging, export, and a Vulkan device-recreation smoke passed. Native CI and
review are tracked by [PR #37](https://github.com/kaz0r/Bozzard/pull/37); a live
two-account Steam acceptance run still requires target accounts and machines.
UI counterparts are in
[Claude's list](TODO-CLAUDE.md).

Start B1 while Claude works on C1/C2, then integrate C3. B2 and B3 are the next
verification priorities. Keep later architecture work behind concrete reference-game
requirements and the tests that make refactoring safe.

## B1: Transactional Rhai reload and authoring data — P1 / M, two slices

- [x] Define a reload request/result contract and expose engine function/hook
  descriptions plus bounded per-attachment hook/command statistics for C3.
- [x] Compile changed sources away from the fixed-tick path and publish a validated
  candidate atomically at a tick boundary. Keep the last valid program on failure.
- [x] Define attachment state precisely: preserve world/blackboard state and
  started/enabled status; specify fresh top-level script-scope initialization;
  do not rerun gameplay `on_start` implicitly. Reject stale results after Stop,
  scene replacement, attachment removal or a newer edit.

**Evidence:** [script loading](crates/bozzard-scene/src/script_runtime.rs) reads sources
when opening a scene; `ScriptRuntimeStats` currently aggregates hooks and commands.
[Multiplayer instructions](docs/multiplayer.md) require Stop/restart to load edits.
Existing `register_script` is a useful seam, not a complete reload transaction.

**Done when:** a valid `script-lab` edit takes effect without restarting Play; a bad
source/signature preserves the old behavior and reports its asset and line; queued
actions, prefab/additive attachments and repeated reload/Stop cycles remain correct.
Keep reads/compilation off simulation ticks and source/diagnostic memory bounded.
For the first slice, reject live code replacement in active multiplayer sessions
and require a coordinated restart; silently changing host or prediction rules is unsafe.

**Handoff to Claude:** request/status types, diagnostics, API descriptions and an
attachment-keyed stats snapshot before C3 integrates. No GUI dependency in scene-core.

## B2: Bozz-torio runtime and relocated-package acceptance — P1 / M, two slices

- [x] Add an offline acceptance route through the real factory simulation and
  scene adapter: start from the authored scene, build/produce/deliver, update the
  HUD, save, reopen and verify the restored factory.
- [x] Extend the game's package tool with validation of its asset inventory and a
  bounded smoke invocation from an unrelated directory after relocation.
- [x] Add that packaged-game check to native CI, using an isolated save directory
  and offline mode so it never needs a Steam login or touches a developer's save.

**Evidence:** [the editor integration test](crates/bozzard-editor/tests/bozz_torio.rs)
checks editing/history and a rendered sprite preview. Factory behavior lives in
[its native runtime](apps/bozz-torio/src/runtime.rs).
[Its packager](apps/bozz-torio/tools/package.py) copies a fixed file list, while
[native CI](.github/workflows/ci.yml) currently packages the engine and demo games,
not the Bozz-torio executable. Workspace unit tests already include the game.

**Done when:** portable CPU tests exercise a deterministic factory route, scene/HUD
synchronization and save restoration with semantic assertions. Test repeated save
replacement and failed writes; retain the previous valid save on failure. A separate
native slice starts the relocated binary on Windows, Linux and macOS without repository
assets, using bounded `--offline --play --screenshot` smoke checks and the route's
saved fixture. Missing assets produce an actionable failure. Add an explicit save-directory
override for fixtures rather than assuming the same user-data environment variables
on every platform. Use a development App ID for CI packaging; offline launch must
not initialize Steam. A screenshot alone does not establish simulation correctness.

## B3: Multiplayer worker and native-frame test lab — P1 / L, staged

- [x] Build injectable transport and clock boundaries around the existing in-process
  simulator to cover the session worker, publication queue, timeout handling,
  stop/rejoin and native presentation boundary.
  Add selectable, seeded latency/jitter, asymmetric loss, duplication and reordering.
- [x] Record input/snapshot age, replay depth, queue sizes, network worker time and
  full editor/player CPU frame median/p95/p99 alongside available GPU timings.
- [x] Add Bozz-torio-specific fragmented snapshot cases: missing/reordered/duplicate
  chunks, a newer snapshot replacing a partial older one, malformed compression,
  timeout and subsequent complete resend. Publish no partial factory state.

**Evidence:** [Flap Woods protocol tests](crates/bozzard-network/tests/multiplayer.rs)
already simulate loss/delay/reorder and assert convergence. The
[CPU benchmark](crates/bozzard-network/examples/flap_multiplayer_perf.rs) already
measures delayed replay and moving presentation, but excludes full native frame
time, real transport and threaded publication. Extend those foundations.
Bozz-torio's [codec](apps/bozz-torio/src/multiplayer.rs) and
[Steam adapter](apps/bozz-torio/src/multiplayer/steam_net.rs) use a separate chunked
full-state protocol and need their own recovery assertions.

**Done when:** one developer can reproduce a named 2–4-peer scenario without extra
Steam accounts; the same seed reproduces event order; input history, publication and
packet queues remain within explicit limits while awaiting timeout; supported
recovery converges or produces the documented disconnect; stale
sessions cannot publish into a replacement. Compare release builds on the same
machine and optimize only measured expensive paths. Functional CI must not depend
on brittle absolute wall-time thresholds.

**Boundary:** start with worker/lifecycle regressions, then add native captures.
This does not replace a two-account Steam invite/overlay/relay acceptance run.

## B4: Share Steam session lifecycle between the games — P2 / M; after B3

- [x] Extract the duplicated create/join/invite, membership checks, callback
  generation tracking, timeout and leave/Drop cleanup into shared support.
- [x] Keep game authority, packet formats and prediction rules in their respective
  adapters. Preserve the existing protocol and lobby compatibility checks.

**Evidence:** [Bozz-torio's adapter](apps/bozz-torio/src/multiplayer/steam_net.rs)
explicitly derives its lobby/session handling from
[Flap Woods' adapter](crates/bozzard-network/src/steam.rs). The games have different
state and transport needs; shared lifecycle does not require arbitrary-scene replication.

**Done when:** both reference games use the same tested lifecycle primitives;
late callbacks after cancellation leave no lobby or worker behind; unauthorized
senders and owner changes retain current behavior; invite parsing, guest save
isolation and solo fallback remain intact. Use B3's injectable session boundary and
fault cases before and after the extraction.

## B5: Compiled gameplay modules that work in editor Play — P2 / L, staged

- [x] Define a small compiled-in module descriptor with explicit dependencies,
  deterministic registration order, duplicate/cycle errors and runtime cleanup.
- [x] Separate Bozz-torio's factory simulation and scene synchronization from its
  native window shell, then host the same module in a game-specific editor build.
- [x] Make module requirements visible to project opening and export. Report a
  missing runtime module rather than silently launching a visual-only preview.

**Evidence:** [the current plugin contract](crates/bozzard-app/src/lib.rs) has only
`name` and `build`. The [Bozz-torio README](apps/bozz-torio/README.md) explicitly
distinguishes editor scene preview from its native factory simulation.

**Done when:** a game-specific editor's Play button runs production and delivery
through the same implementation as the standalone game; Stop restores the authored
document and joins workers; repeated Play does not duplicate systems or callbacks;
an exported compatible runtime behaves the same. Reuse B2's route as acceptance.
Ship descriptor/lifecycle support first, then the game integration. This does not
require hot-loading native libraries, a Rust plugin ABI or rewriting gameplay in Rhai.

## B6: Recoverable graphics lifecycle and useful failure reports — P2 / L

- [x] First handle recoverable surface loss by reconfiguring and retrying safely;
  distinguish it from a lost GPU device and out-of-memory failures.
- [x] Then define bounded device recreation that restores resident assets and
  renderer state while preserving the authored scene and CPU simulation.
- [x] Retire pending compute/readback work with explicit outcomes, and emit a
  bounded diagnostic report if recovery fails instead of looping indefinitely.

**Evidence:** [the player](apps/player/src/main.rs) currently exits on a lost
surface; [the renderer](crates/bozzard-render/src/lib.rs) records device loss.
Recovery is still open in the runtime roadmap, and compute now adds resource/job
lifetime requirements to it.

**Done when:** lifecycle fault injection proves one controlled recovery, preserved
scene state, safe pending-job completion/failure and a useful terminal error after
bounded retries. Verify supported native backends; identify any platform path that
cannot be exercised rather than treating a mocked recovery as hardware evidence.
Keep editor-specific integration separate where its window framework owns recovery.

## Working agreement and later work

Keep each change reviewable; the staged items above are several PRs, not one giant
branch. Preserve public behavior, typed validation, headless tests and Edit/Play
isolation. Agree C3's data contracts early; let Claude own their UI. For C6, provide
or review a side-effect-free preview API rather than duplicating runtime evaluation.

Before a code PR is ready, review allocations and per-frame work, run checks for the
affected contracts, and wait for all native CI jobs. Put cheap Python/tooling checks
before expensive compilation when next touching CI; the recent Windows cleanup
failures showed why early feedback matters. Do not claim real Steam acceptance from
fake transports, or a general frame-rate improvement from an isolated CPU benchmark.

After these priorities, consider explicit Bozz-torio save-version migrations,
project-wide asset dependency repair, and release notices/signing/minimum-OS checks.
Signing and live Steam acceptance require the relevant accounts and target machines.
Defer new platforms, native dylib plugins, broad renderer rewrites and a parallel ECS
until a concrete game or measurement justifies them.
