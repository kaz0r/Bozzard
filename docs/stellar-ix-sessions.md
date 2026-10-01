# Stellar-IX saves and Steam sessions

The requested scope is persistent games and Steam co-op for one host and up to three guests.
This work is in progress; the checklist below records the complete scope, not a reduced milestone.

The [two-computer Steam checklist](stellar-ix-coop-test.md) covers the live guest
verification, including separate-planet production, host save/load, and rejoining.

- Save and load both planets, factories, wiring, recipes, buffers, storage, progression, inventories, and the world clock.
- Host alone owns multiplayer saves and loads. Manual slots and a twenty-minute autosave show progression and day/night cycle information.
- Steam friends-only lobby: create and invite from the main menu, select a save, chat, and start with zero to three guests.
- Steam invitations and new joins continue during play. Joining clients receive the authoritative current world.
- Host validates gameplay requests, owns simulation, and records each player's discoveries.
- Independent players use blue (host), red, orange, and green, with nearby name labels.
- Enter opens chat in game; typed input must not trigger gameplay shortcuts. Lobby chat is also visible.
- Editor Play, the native player, and source-independent exports use the same services.

## Implementation boundaries

Steam overlay support initializes the SDK before window/GPU creation for a factory
startup in the player and editor. A process-wide activation callback survives editor
Play/Stop and lobby changes. Opening or closing the overlay clears held gameplay
input and drags; game UI, chat and native shortcuts ignore input while it is active.
Production and networking continue. The co-op panel provides **Open Steam overlay**
and readiness status; Steam owns its configurable shortcut and invitation dialog.
Offline solo startup remains available if SDK initialization fails.

The local native Vulkan check received a real `GameOverlayActivated` callback after
opening Steam Friends through the authored button: 539 frames were presented while
the overlay was active, out of 600 total frames.
The production/input regression and optional-Steam startup tests pass. Both native
apps compile with Steam enabled and disabled. This overlay check used the installed
Steam overlay renderer with a temporary process environment; it did not change
Steam settings or send invitations.

The user subsequently tested two separate computers and confirmed working guest
connections, machines, electricity, and travel between planets. They reported a
delay specifically in the guest's own movement display. Keyboard movement now
predicts the local cursor using unacknowledged inputs, replayed on the latest host
position. Host replies retire accepted or rejected inputs without replaying them
twice. Inventory, production, discovery and travel stay host-authoritative.
Moves arriving in a network batch are processed one per player per simulation tick
with subsequent actions kept in order, rather than discarded by the movement rate
limit. Local validation covers delayed replies, partial acknowledgements, mixed
mouse/keyboard input, rejected in-flight moves, cross-chunk movement, world resets,
send failures and retries. All 19 co-op integration cases pass (the retry case was
rerun after updating its old wait-for-host expectation), as do three request-gate
unit tests. Player and editor builds check successfully. The user subsequently
confirmed the two-computer retest works correctly, including the movement fix.

Rhai continues to own factory rules and presentation. Native code owns local files and authenticated Steam transport.
`examples/demo/src/factory/state.rs` defines a bounded, data-only snapshot; it contains no executable scripts, asset paths, or render handles.
Scene and controller blackboard patches validate all fields before publication.
Factory scripts archive the occupied region at a tick boundary. File reads, encoding, and atomic writes run on a background worker.
Loading validates the file and the runtime schema before replacing visuals and restoring the saved archives.
Failed reads leave the running world intact. Saves are separate from the project's authored scene.

The Flap Woods reference (`crates/bozzard-network/src/steam.rs` and `docs/multiplayer.md`) provides Steam callback lifecycle, authenticated identities, friends-only lobbies, relay networking, invitations, and bounded chat.
Its four-bird/three-pipe protocol is not a factory world protocol. Factory sessions need a separate protocol supporting late joins, independent planetary locations, and host-validated actions.
App ID 480 remains the development default, as in Flap Woods; a shipping game needs its own App ID.

## Verification still required

### Requirement audit — 2026-09-27

Current test package: `stellar-ix-coop-20260927-v4-ba3db37fbbd6` (Linux release, guest movement prediction and Steam overlay support).
The package was launched for 120 presented frames and its 76 files verified again
after archive extraction. The user reports working two-computer guest connections,
machines, electricity and planetary travel, and confirmed the guest-local movement
correction works in the follow-up retest. Save/load, chat, reconnection and
four-player details remain unreported.

| Requirement | Evidence inspected | Remaining proof |
| --- | --- | --- |
| Manual saves and loads; host owns multiplayer persistence | Four `saves_` integration tests passed: fresh-runtime Earth/Moon round trip, invalid-file preservation, player records, and guest save/load rejection. `coop_host_load_and_new_world_replace_epoch_and_reject_previous_requests` passed. | Connected save/load with a real Steam guest. |
| Autosave every twenty minutes | `saves_autosave_after_twenty_minutes_defers_flight_and_rejects_guest_requests` verifies the 1200-second boundary, flight deferral, file creation, and timer reset with a controlled clock. | The checklist includes an elapsed-time live-session check. |
| Progression and day/night metadata | Round-trip test asserts `Tier 2 / Phase 4 · Day 1 / Night 08:06:44`; the native populated save-browser preview was inspected. | Confirm the live test's saved world metadata. |
| Steam sessions | Native host checks passed; the user confirmed a working guest session on two separate computers and a successful movement-fix retest. | Specific invitation paths. |
| Host plus three guests, maximum four | Steam lobby creation requests four slots; the same bound is enforced when validating membership. The four-peer replication test passes and rejects a fifth member. | Four-account session and lobby capacity check. |
| Guest discoveries reach the host | Actor movement/discovery and two-instance cross-region tests pass through host validation and replication. | Observe both maps during a real guest session. |
| Blue/red/orange/green players and nearby names | Host/guest color assertions pass; populated native renders show guest markers/names. Distance, other-planet, and open-panel visibility checks pass. | Confirm identities and colors across real accounts. |
| Enter chat and lobby chat | Real native-player input test verifies Enter captures typing and Escape restores gameplay. Long-message/draft scrolling and populated render tests pass. | Delivery of messages between Steam accounts. |
| Main-menu lobby, invites, save selection, solo start | Real Steam single-member test creates through UI, starts alone, and returns to world selection. Save-browser loading passes separately. Invite/friend-picker controls route to Steam APIs. | Invite before start and start from a saved world with a guest. |
| In-game invites and late joins | Steam lobby remains joinable after start; late snapshots, rejoin identities, transport retries, and world resets pass in process. | Live late join, cold rejoin, and host departure. |
| Reuse Flap Woods guidance without regressing it | Shared Steam lifecycle helpers are used; all eight existing Flap editor integration tests pass. | No additional local gate identified. |

The `coop_` integration tests cover real game rules and game-loop
instances with in-process authenticated packets. They do not impersonate a second
Steam account. The remaining proof depends on the user's separate-account test;
the goal must not be marked complete on the local evidence alone.

Save round trips, malformed-file rejection, autosave timing, and guest authority checks pass. Native title and save-browser previews have been inspected at 900–1920-pixel widths. The source-independent export test also passes.
`crates/bozzard-network/src/coop_lobby.rs` implements the Steam lobby/transport layer, reusing Flap Woods lifecycle helpers while permitting solo start and in-game joins. The native player/editor adapter connects it to the factory protocol and authored lobby UI; the user's two-computer session confirms basic gameplay integration.
The Steam-enabled build and packet scope/size tests pass. The implementation follows Valve's [lobby lifecycle](https://partner.steamgames.com/doc/features/multiplayer/matchmaking) and [Networking Messages](https://partner.steamgames.com/doc/api/ISteamNetworkingMessages) APIs.

The factory replication model now lives in `examples/demo/src/factory/shared.rs` and
`replication/`. A canonical Earth-first world is separate from private player records.
Live snapshots include the occupied chunk's latest edits and buffers without moving
the camera or completing a rotation. Each recipient gets only their own backpack;
the public roster contains names, positions, and stable blue/red/orange/green slots.
The host keeps disconnected player records for rejoin, and saves preserve them by
Steam identity. Older solo files remain readable.

Full snapshots travel in bounded 16 KiB fragments, with a 32 MiB transfer cap and
timeouts. Changed archive pages and scalar values use revision-based deltas. Only
the authenticated host can publish them; clients validate an entire candidate before
replacing their replica. Invalid, partial, stale, or missing-base updates never
partially replace the world. Host loads must start a new monotonically increasing
epoch; guest reconnections receive a new connection token.

`network_send(kind, payload_map)` now provides a bounded local script request queue.
The factory adapter parses a closed gameplay action schema, authenticates Steam
membership, and bounds/orders/deduplicates requests per connection. Save/load and
direct world or inventory replacement are not guest actions. The gate authenticates
intent; it does **not** substitute for game-rule checks when executing that intent.

The new protocol tests use real factory state with four in-process peers: independent
inventories, current-chunk edits, late snapshots, deltas, reconnects, malformed input,
transfer timeout, and host save/load. They do not constitute live Steam testing.
`factory/authority.rs` now executes validated actor transactions with the loaded
Rhai recipe, inventory, deposit, and power rules. `factory/host.rs` connects that
executor to the actual simulation worker at a tick boundary. It captures the
host's latest local edits and production, executes guest requests, commits their
results, and only then acknowledges them. Disconnected players stop gathering;
their private records remain available for rejoining and saves. Loading or creating
a world starts a fresh epoch, discarding requests from the previous world.

`factory/live.rs` merges those accepted edits into both archives and the occupied
chunk's live arrays. `factory/host_view.rhai` updates affected resident models before
the next gameplay tick. Unchanged models, the host camera, open unrelated panels,
and private inventory remain local. Guest edits refresh open storage, removal closes
interfaces for deleted machines, and remote rotations pause the relevant machines
on either planet. The native adapter starts `HostRuntime` after a host creates a world
or finishes loading a save, and starts `GuestRuntime` for a joining Steam member.

Integration tests now exercise guest-built production through `advance_with_frame`,
four authenticated actors, conflicting placement, rendered machine edits, discovery,
timed rotation, a guest traveling to/collecting on the Moon while the host stays on
Earth, disconnect/rejoin, storage panels, and host load/new-world epoch changes.
They use in-process authenticated packets and the real game loop, not live Steam.

`factory/guest.rs` projects complete replicas into a separate game instance. Guests
send their build, move, gather, craft, delivery, inventory, recipe, wiring, and travel
intent through the host's request gate. They do not run local production. Bootstrap,
planet changes, and new host epochs clear/restore presentation through a handshake;
ordinary updates retain the camera, panels, and local action-bar selection. Two-instance
tests cover keyboard building, rejected placements, collection, cross-chunk movement,
independent Moon travel, simultaneous host production, and stale-input removal.

`factory/link.rs` sends bounded fragments, retries queue pressure, and advances a
recipient's delta base only after that peer acknowledges the completed replica.
Unacknowledged input is retried after interruption and deduplicated at the host.
Tests exercise fragment retry, withheld acknowledgements, lost actions, and duplicates.
`factory/network.rs` pumps Steam callbacks and delivery on the native main thread while
retaining the existing simulation worker. The native title and Escape menu now open
an authored co-op panel with create/invite/friend-picker/leave controls and lobby chat.
Hosts choose a new world or saved slot from the existing title screen to start, including
when alone. Joining players wait for the host; returning to the title retains the host's
lobby for another world. In-game invitations remain available. Enter captures chat input
and suppresses gameplay keys; names and colors are drawn by `factory/players.rhai`.
Host is blue, with red/orange/green guest slots. Only nearby players on the same planet
receive name labels. No remote model paths or UI instructions come from the network.

Verification this stage: 16 `coop_` integration tests, bounded Unicode/chat keyboard
checks, host identity transfer after a solo load, and native player/editor builds pass.
The native 900–1920-pixel lobby render check passes with graphics-device access.
A signed-in Steam client also created a friends-only lobby through the actual UI,
started the world with one host, ran the worker, and returned to world selection.
This is a real Steam **single-member** test, not verification of invitations or guest
traffic. No invitations or chat messages were sent to other users by that test.

Guest conveyor presentation now uses the host's actual transfer records from both
planets. The transient `factory-transports` blackboard contains source/destination
cells and item/machine kinds, never model handles. Validated replicas carry those
records for the recipient's planet; guests interpolate resident models between
snapshots without advancing production. A two-instance regression checks a cross-chunk
Earth belt and storage delivery while the host is on the Moon, including stalled
packet delivery, invalid effects, and no duplicated items. The 17 co-op tests pass.

The factory scene now declares its `steam_coop` App ID, which the native adapter and
export validator share. Export checks reject mismatched native runtimes and include
the Steam redistributable, development App ID file, and launch instructions. A relocated
export created a real single-member Steam lobby, started its host simulation, explored,
opened the journal, and traveled to the Moon after its source copy was deleted.
Offline solo startup remains available when Steam is unavailable.

Native editor/player frame scheduling now distinguishes Flap's prediction worker from
the factory's normal simulation worker. A real Steam editor test covers pending lobby
creation, serial Play ticks, and the threaded frame path after joining. This catches
integration failures that calling `SceneDemo::advance_with_frame` directly cannot.
The real Steam native-player check also passes through its keyboard dispatcher and
frame scheduling: movement still works, Enter captures typing, Escape ends typing
without leaving a movement key held, and factory beats keep advancing. The existing
eight Flap editor integration tests pass after this scheduling change.

The source-independent factory export regression lives in
`apps/player/tests/stellar_export.rs`, where Cargo supplies the freshly built native
player automatically. Set `BOZZARD_EXPORT_LIVE_STEAM=1` for the optional single-member
Steam lobby check; the ordinary export test sends no Steam requests.

Populated native-render fixtures now cover four member names, maximum-length chat
messages/drafts, long friend names, and nearby red/orange/green player markers at
900, 1280, and 1920 pixels wide. They exposed fixed-height text clipping: the roster,
friend list, chat history, and draft now scroll within their panels. Changed messages
and typed text follow the end; idle frames preserve manual scrollback. These use
synthetic local names and messages, not traffic from other Steam accounts.

**Still required:** live Steam invite/join/rejoin and four-player verification across
separate accounts, including the interfaces in the two-computer checklist. The complete
multiplayer feature is not yet verified end to end.

For native saves, `factory::Session` now holds `local_peer` and the persistent player
registry. The Steam adapter supplies actual authority/identity, and the host worker keeps that
registry synchronized before snapshots are saved.
Changing a loaded save's host
identity transfers the local host record instead of duplicating its inventory.

Multiplayer requires four-peer protocol and gameplay integration tests, join/rejoin snapshots, conflicting-action tests, native lobby/chat UI checks, and live Steam invitation/join verification with separate accounts.
Do not mark the whole feature complete based on persistence tests or a transport-only mock.
