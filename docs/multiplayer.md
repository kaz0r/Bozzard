# Steam multiplayer / Flap Woods Together

Stage 7 uses Steam friends-only lobbies and Steam Networking Messages. The lobby creator
runs a listen server in the player or editor Play. Two to four Steam accounts control separate coloured
birds through shared pipes; each bird has its own score and elimination state. A round
ends when every bird is out. Only the original host can start or retry, after a friend
has completed the protocol handshake. A shared **5, 4, 3, 2, 1** countdown precedes every
round; birds, pipes and flap input wait until it finishes. Guests never send Start, score
or position commands. If the last guest leaves during the countdown, it returns to the lobby.

The example uses Valve's **Spacewar App ID 480**. This is a development example; publishing
requires your own Steam App ID. No Steam emulator or account credentials are included.

## Run with friends

Start Steam, sign in and stay online. Each participant needs a different Steam account
and their own machine/session. Both must use the same project revision. Native editor and
player builds include Steam support by default; Python is not required.

```sh
cargo build -p bozzard-editor-app -p bozzard-player
# Standalone player:
target/debug/bozzard-player --project examples/demo/flap-woods-multiplayer.bozzard.json
```

Cargo stages the matching Steam API redistributable beside the executable. Linux/macOS
binaries locate it relative to themselves; Windows uses its normal adjacent DLL lookup.
The pinned `steamworks-sys` dependency supplies the SDK, or `STEAM_SDK_LOCATION` overrides
it. The build script reads that downloaded package directly from `CARGO_HOME`; it never
invokes Cargo metadata or downloads unrelated workspace dependencies. Vendored/path builds
can set `STEAM_SDK_LOCATION` to the selected crate's `lib/steam` directory.
Keep the native library beside the executable when moving a build. The optional
`tools/steam.py`, `.sh` and `.ps1` scripts remain convenience wrappers. Explicit
`--no-default-features` editor/player builds disable Steam; the headless server remains
Steam-independent.

1. Host clicks **Create lobby** (C).
2. Click **Invite friends** (I). If Steam’s overlay is ready, it opens the invite dialog;
   otherwise the game opens its own friend picker. **Invite without overlay** always uses
   the picker. Select a friend to request an invitation through Steam.
3. Friends launch this example and accept the invite. The running player handles
   `GameLobbyJoinRequested`. Names appear in the lobby; the host's Start button enables
   only after at least one guest has exchanged valid packets.
4. Only the host clicks **Start game** (Enter). Both players see the five-second countdown,
   then can tap Space to flap their birds. The scoreboard
   identifies your slot with YOU; eliminated birds shrink and their score shows OUT.
5. After all birds are out, only the host can retry (Enter/R). L leaves the lobby;
   Q or Escape quits. There is no shared pause: opening the Steam overlay or losing focus
   does not pause the host. Closing the host ends the session for everyone.

Click **Lobby chat** before or between rounds to talk to everyone in the lobby, without
requiring the overlay. Type a message and press Enter or click Send; Backspace edits and
Escape returns to the lobby. Gameplay shortcuts are suppressed while typing. Messages
are limited to 160 characters; the panel keeps the four most recent messages, and leaving
the lobby clears its history. Steam sends messages to all lobby members, including the
sender. The chat panel closes when the host starts the countdown.

This revision uses network protocol 3; all friends should rebuild/update together.
Lobby admission also compares the selected player and round script fingerprints,
so different gameplay script revisions cannot silently predict different rules.

Steam’s overlay availability depends on the local Steam/graphics setup. Invitations now
work through the in-game friend picker as well. As another fallback, share the displayed
lobby ID and run:

```sh
python3 tools/steam.py run --join-lobby 109775240000000000
```

Replace that number with the host's actual lobby ID. The player also accepts Steam's
`+connect_lobby ID` launch argument. With shared App ID 480, accepting an invite while
this example is closed can launch Valve's Spacewar instead; **start this example first**
or pass the lobby ID explicitly. App 480 is shared by other developers: the game checks
its own lobby identity/version and original host metadata before joining.

## Test inside the editor

Open the multiplayer scene in the regular editor and press **Play**. From a source checkout:

```sh
cargo run -p bozzard-editor-app -- --project examples/demo/flap-woods-multiplayer.bozzard.json
# Optional: join a known lobby when you next press Play
# Add --join-lobby YOUR_LOBBY_ID to the command above.
```

Use the scene’s Create lobby / Invite friends / Start game buttons. A
**Steam lobby ID → Join lobby** field appears in the editor’s Play toolbar. Keep the pointer
over the viewport for Space and lobby shortcuts. Guests can use another editor or the
standalone player on their own Steam account. No Python launcher, SDK copying or shell
library-path setup is needed.

**Stop**, Escape or the game’s Quit action leave the lobby and return to editing; they
never close the editor. The original edit scene is unchanged. New Play creates a fresh
session. Preparation workers never initialize Steam: initialization occurs when the main
thread publishes Play, after cancellation/stale-document guards have passed. Failures
leave the editor in Edit mode with an actionable error. Ordinary non-Steam builds explain
how to rebuild instead of showing inert lobby buttons in Play.

The SDK initializes before GPU creation only when the starting scene has a
`steam_multiplayer` component. Blank and solo scenes never initialize Steam and do not
require the Steam client. If you open a multiplayer scene later, its first Play initializes
Steam; restart with that scene if the overlay needs initialization before graphics.
Once initialized, the SDK stays alive across Play/Stop for overlay compatibility.
Lobby callbacks and the network worker belong to
Play and are cleaned up on Stop. Idle editor frames pump late Steam results so a lobby
operation completing after Stop is immediately left. Networking continues while redraw is idle or the editor
is minimized. Blueprint Pause/Step is disabled for network Play; use Stop to disconnect.

## Overlay troubleshooting and invitations

The game cannot force a disabled or uninjected Steam overlay to appear. Steam says the SDK
must initialize before graphics setup, and recommends launching through Steam when the
overlay fails to appear. Overlay availability can also remain false briefly during startup.
Both native apps now initialize before graphics; the editor previously had no Steam
initialization at all. See [Valve’s overlay requirements](https://partner.steamgames.com/doc/features/overlay).

To test overlay injection, add the native editor/player executable to Steam’s library as
a non-Steam game, enable Steam’s in-game overlay and launch that entry. Set launch options
to `--project` followed by the absolute path to the multiplayer project. The optional
`tools/steam.py editor --prepare-only` still generates a convenience wrapper for this.
Do not relaunch App 480 itself to fix this: it can start Valve’s Spacewar instead of this
executable. Native overlay injection still needs verification on your graphics platform.

**Invite without overlay** uses `InviteUserToGame` with a strictly parsed
`+connect_lobby ID` payload. The recipient accepts in Steam; running editor Play/player
sessions handle `GameRichPresenceJoinRequested`. The UI reports an invite *request*,
since the Rust wrapper does not return delivery confirmation. Cold-start App 480 invites
retain the Spacewar limitation above; launch this example (and press Play in the editor)
before accepting. These are ordinary Steam invitations, not a replacement overlay.

## Scenes, scripts and export

`examples/demo/scenes/flap-woods.json` remains the solo Blueprint game and points to the
multiplayer variant. The multiplayer project is
`examples/demo/flap-woods-multiplayer.bozzard.json`, using
`scenes/flap-woods-multiplayer.json`. Regenerate its artwork, four slots and lobby widgets:

```sh
python3 tools/gen_flapwoods_multiplayer.py
```

The example's gameplay is editable Rhai, loaded from the scene's ordinary script
catalog and attached through **Script Manager**:

- `scenes/scripts/flap-woods-multiplayer/player.rs`: input binding, spawn position,
  flap strength, gravity, client prediction, bird tilt and elimination appearance.
- `scenes/scripts/flap-woods-multiplayer/round.rs`: countdown, pipe movement and
  recycling, collision, scoring, round completion and scoreboard text.
- `scenes/scripts/flap-woods-multiplayer/pipes.rs`: pipe and gap presentation.

Edit a script, then stop and restart Play to load the new source. No native rebuild
is needed for game-rule changes. Every participant needs the updated scripts.
Regenerating the scene preserves these hand-authored script files and reattaches
them. Export packages them as ordinary script assets.

The host and local client prediction call the same `network_predict` function at
60 Hz. Only the host calls `network_step`, `network_resolve` and `network_finished`.
These functions take explicit state and return state; the existing Rhai interpreter
runs them with fresh scopes, bounded execution and f32 arithmetic for replay.
They cannot issue scene commands. The normal `on_update` hooks use
`network_active()`, `network_object(me)` and `network_state()` to read the latest
interpolated/predicted view, then call the usual `set_position`, `set_rotation`,
`set_scale` and `set_text` APIs. They never run a competing local simulation.

`steam_multiplayer` selects `player_script` and `world_script` catalog IDs alongside
the game ID, protocol, App ID and player limit. `network_player.slot` and
`network_obstacle.index` bind protocol slots to locally authored scene objects;
the Rust presentation adapter does not assume bird/pipe object names. Player
and obstacle bindings appear as **Network Player** and **Network Obstacle** in
the inspector. **Steam Multiplayer** exposes the game ID and both script selectors.
Player bindings must carry the enabled selected player script. Missing sources, disabled
attachments, bad signatures and script failures are reported with their source
and function; there is no built-in gameplay fallback.

Rust retains Steam lifecycle, lobby widgets/chat, authenticated transport, bounded
replication, input ownership, fixed pacing, interpolation and acknowledgements.
The packet schema remains this reference's four-player/three-obstacle schema;
this is **not arbitrary scene replication**. Steam account access is not exposed
to Rhai. Both Editor Play and the standalone player use this shared path when
built with the `steam` feature. The solo example retains its Blueprint game.

In the editor choose **File → Export game…**. The ordinary export bundles the matching
Steam library beside the game executable and lists it, its metadata and development
settings in `package.json`. **Play game** launches Spacewar development exports directly.
The native player uses the same exporter:

```sh
cargo build --release -p bozzard-editor-app -p bozzard-player
target/release/bozzard-player --export-project examples/demo/flap-woods-multiplayer.bozzard.json --export-dir dist/flap-woods-steam
```

Share the **complete folder**. Recipients open `Game`, `Game.exe` or `Game.app` with Steam
running; no Python, Rust, environment overrides or source checkout are required. Runtime
files are `libsteam_api.so` on Linux, `steam_api64.dll` on Windows and `libsteam_api.dylib`
on macOS (inside `Game.app/Contents/MacOS`). Multiplayer export checks that the companion
player has the same Steam support, SDK hash and target before publishing. Missing or
incompatible players produce an actionable error instead of an incomplete package.
Even solo exports from a Steam-enabled engine include the library needed by that runtime.

For a real Steam game, select **bird-0 → Steam Multiplayer → Steam App ID** and replace
480 with your own App ID. Restart the editor with that scene/project before testing another
App ID. App 480 exports are clearly marked as development builds and include
`steam_appid.txt`. Other App IDs produce store exports without that file, launch wrappers
or development environment overrides; their player initializes from Steam’s launch context
and verifies the App ID. Configure the exported executable in your Steam launch options.
Use editor Play for local tests; **Play game** explains that store exports launch through
Steam. [Valve’s API setup documentation](https://partner.steamgames.com/doc/sdk/api)
requires removing the development App ID file before uploading to a Steam depot.

Packages target the builder’s OS/architecture. Signing, store/depot configuration and
live testing on each supported platform remain release tasks; Windows/macOS packages
have not been live-tested locally.

## Authority, replication and pacing

- Host simulation runs at 60 Hz independently of redraws, including minimized windows.
  A pump executes at most eight catch-up ticks, discards excess whole ticks, retains the
  fractional remainder, and reports discarded time. Normal snapshots are sent at 20 Hz.
- Steam identities authenticate transport senders. Both transport admission and message
  handling check lobby membership; guests accept state only from the original host.
  Joining during a round is disabled. Steam automatic owner migration ends this session;
  there is deliberately no implicit host migration or surviving dedicated game server.
- Birds are ECS components. Deltas use `World::is_changed_since` against each recipient's
  acknowledged snapshot tick. Unacknowledged writes are resent. Each snapshot carries a
  complete roster (despawns/relevance), current phase/round and three shared pipes.
  A new round resets baselines and sends complete state. Interest is the bounded lobby,
  not proximity: everyone sees the entire shared arena.
- Inputs contain only bounded, sequenced flap frames. Pending inputs are redundantly
  retransmitted; a host consumes at most one input per bird per simulation tick. Duplicate,
  old-round, nonmember and invalid messages cannot start games or grant score. Messages
  are versioned/lobby-scoped, limited to 16 KiB and 120 input frames; each pump receives
  at most 64 Steam messages. No external scene paths, scripts or object graphs are accepted.
- The local bird predicts vertical motion, restores authoritative state on receipt and
  replays up to 120 unacknowledged inputs. Remote birds and pipes interpolate one snapshot
  behind; recycling pipes snap to their new cycle. Collision/death/score remain host-only.
  This is bounded input replay, not full-world rollback or lag-compensated collision.
  Large latency can visibly correct motion; prediction stops growing at its history limit.
- Fifteen seconds without valid host snapshots returns guests to the lobby screen.
  Host-side inactive members are removed from simulation and cannot reconnect without
  leaving/rejoining. Lobby callbacks time out and stale callback results are discarded.
  Graceful leave sends Goodbye and leaves Steam matchmaking; host loss is also detected
  by owner/membership polling. Operational counters log every five seconds.

The bounded replication schema is purpose-built for this reference, not arbitrary scene replication.
Wider pipe gaps make multiplayer latency more forgiving; the scene acceptance test checks
that rendered pipe clearances and bird sizes agree with authoritative collision rules.
A modified host can cheat. Competitive anti-cheat, host migration, dedicated Steam servers,
voice, arbitrary network components and persistent reconnect identities are outside this
reference scope. See [the determinism contract](architecture.md#network-determinism-contract).

## CPU performance benchmark

The headless benchmark measures the Flap Woods host simulation, replica snapshot apply/replay,
scripted presentation accessors, and the authored scene's Rhai presentation
hooks. It uses four in-process peers and does not need Steam, a renderer, or another player.
Run from the repository root:

```sh
# Debug build, useful for checking the harness but not representative of shipped performance:
cargo run -p bozzard-network --example flap_multiplayer_perf
# Release build, use this for performance comparisons:
cargo run --release -p bozzard-network --example flap_multiplayer_perf
# Optional headless benchmark of the complete editor lobby presentation pump:
cargo test --release -p bozzard-editor --test multiplayer benchmark_multiplayer_presentation_pump -- --ignored --nocapture
```

The Flap Woods benchmark warms up the game state, then records 6,000 individual ticks
across 200 thirty-tick scenarios. It reports median, p95 and p99 microseconds per tick
for each path, including a no-op script run that isolates scene dispatch overhead.
The basic replica path measures snapshot apply/replay with immediate acknowledgements;
input prediction is outside that timer. A second path measures client input prediction
and snapshot apply/replay with snapshots produced at 20 Hz and delivered six simulation
ticks later (about 100 ms). It primes that delay pipeline before sampling. Host simulation
and snapshot production are outside that path's timer. The scene-hook baseline repeatedly
presents one unchanged network frame. A second scene-hook path updates the frame from
evolving replica state before each timed script step, so it measures changing gameplay
presentation; frame construction and resource publication are outside that timer. The
editor benchmark records the idle lobby's complete main-thread pump using a fake backend;
it does not measure the threaded snapshot publication optimization. Compare debug
results only with other debug runs, and release results only with release runs on the same
machine and toolchain. Lower values mean less CPU time in that specific path; this benchmark
does not measure Steam transport, real network latency or packet loss, GPU/rendering cost,
full editor/player frame time, or multi-machine behaviour. The in-process peers do not
establish whether a change improves a live two-account session.

On the same macOS machine in a release build, the September 2026 performance pass measured:

| CPU path | Before median / p99 (µs) | After median / p99 (µs) |
| --- | ---: | ---: |
| Four-peer host simulation | 50.667 / 77.958 | 38.208 / 68.042 |
| Flap Woods unchanged-frame scene presentation | 235.125 / 318.792 | 46.500 / 106.000 |
| Idle editor lobby pump | 205.167 / 266.000 | 44.708 / 84.584 |

These timings are local CPU evidence for reduced work and frame-time tails, not a claim about
live Steam latency. The historical table predates the delayed-snapshot and moving-frame
measurements and has no before/after values for those paths. Re-run the commands above for a
comparable baseline.

On Linux on 2026-09-25, the B1–B6 branch was compared with `main` using the same release
profile, toolchain and machine. Each binary ran four times, alternating order and pinned
to one CPU. The numbers below are medians of the four per-run medians; each run sampled
6,000 ticks. They are microseconds per tick, so lower is faster.

| CPU path | `main` | B1–B6 branch |
| --- | ---: | ---: |
| Four-peer host receive and scripted simulation | 42.342 | 42.006 |
| Delayed replica prediction and snapshot replay | 7.694 | 7.829 |
| Flap Woods unchanged-frame scene presentation | 54.694 | 56.825 |
| Flap Woods moving-frame scene presentation | 85.297 | 92.037 |
| Flap Woods no-op presentation scripts | 16.673 | 17.586 |

The moving-frame and no-op scene paths were slower in this CPU-only measurement. This
does not establish a full native frame-rate change; inspect those paths under a native
capture before attributing the difference to any one allocation or optimizing it.
For a debug build (`cargo run`), Flap Woods unchanged-frame scene presentation measured
3,237.021 / 3,660.083 µs median / p99 before and 602.250 / 927.250 µs after.

## Verification

```sh
python3 tools/steam.py test
cargo test -p bozzard-demo --test multiplayer --test flapwoods
cargo test -p bozzard-player --features steam
cargo test -p bozzard-editor --test multiplayer
cargo test -p bozzard-editor-app --features steam
python3 tools/check_headless.py
cargo run -p bozzard-server -- --realtime --ticks 120
# Continuous headless scene harness; Ctrl-C/SIGTERM saves and exits cleanly:
cargo run -p bozzard-server -- --realtime --ticks 0 --save-scene /tmp/server-stop.json
# Named, seeded 2–4 peer fault runs; use the same seed to reproduce event order:
cargo run -p bozzard-network --example multiplayer_lab -- cafe-wifi 42 4
cargo run -p bozzard-network --example multiplayer_lab -- satellite 42 2
```

The lab accepts `clean`, `cafe-wifi`, or `satellite`, then a seed and a peer count of 2–4.
It prints an event hash, packet loss/duplication, peak packet and input-history sizes,
oldest queued packet age, and convergence. CI asserts seeded repeatability and buffer
limits. In Steam Play, the editor profiler and player frame capture report snapshot age,
oldest outstanding input age in simulation ticks, replay depth, command queue depth,
worker time and publication age. These are local diagnostics; the lab does not simulate
Valve's invite, relay or overlay services.

Automated tests exercise three clients over serialized packets with scripted loss,
latency, duplication, reordering, dropped acknowledgements and final convergence; host
start authority, input ownership, bounded queues, stale rounds, relevance removal,
prediction reconciliation, full game scoring and authored scene/UI compatibility.
The same tests execute the shipped Rhai assets. Additional tests edit movement,
input, countdown, pipe speed, score and visual scripts to verify the changes take
effect in host simulation, prediction/replay and Script Manager presentation.
Native export tests load and execute the bundled scripts after moving the package.
These tests do **not** connect to Valve's backend. A real two-account Steam invite,
relay, overlay and native-window acceptance run remains necessary on target machines.

Validation recorded for this change on Linux: six protocol/gameplay tests, two multiplayer
scene tests, three solo Flap Woods regressions, and 21 player unit/CLI tests passed (three
existing GPU-only tests skipped). Strict Clippy checks passed with Steam enabled, as did
formatting and the headless dependency audit. The launcher built and staged the SDK, and a
renamed export loaded its bundled multiplayer scene from an unrelated working directory.
The real-time server ran 12 ticks in approximately 0.206 seconds; SIGTERM produced a clean
exit and a final saved scene. Native smoke was attempted but could not find a compatible
GPU adapter in this environment. No live two-account Steam session was available.

The editor follow-up passed 47 editor unit/lifecycle tests and 191 Steam-enabled
demo/player/editor-app/network tests (four existing tests ignored), plus four non-Steam
editor lifecycle/error-path tests. These verify shared lobby/input routing, overlay-free friend selection,
edit-scene isolation, repeated Stop cleanup, independent worker pumping and actionable
non-Steam-build errors. The native editor launcher builds/stages successfully and its
command-line entry point was checked. The broader editor suite reached the existing GPU
compute tests and terminated with SIGSEGV; an isolated run without the Steam feature
reported no compatible graphics adapter. GPU and live Steam acceptance remain unverified.

Native export regression coverage exercises the same exporter as the editor with App IDs
480 and a custom ID, checks every package inventory entry and the bundled SDK bytes, renames
the result, and launches it with an empty working directory/PATH and no Steam/library-path
environment overrides. It checks scene loading and runtime capabilities without initializing
Steam or graphics, and rejects an incompatible companion player before publication. Direct
editor startup without Python reaches the SDK; the local smoke attempt reports Steam is not
running and no graphics adapter is available, so it does not establish live multiplayer acceptance.

Primary API references: [Valve Spacewar example](https://partner.steamgames.com/doc/sdk/api/example),
[Steam networking](https://partner.steamgames.com/doc/features/multiplayer/networking),
[Steam matchmaking](https://partner.steamgames.com/doc/api/ISteamMatchmaking), and
[steamworks-rs 0.13.1](https://docs.rs/steamworks/0.13.1/steamworks/).
