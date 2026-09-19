# Architecture decisions

## Ownership and dependencies

Bozzard owns its ECS, scheduling, scene model, renderer design, module contract, and editor/export workflows. `wgpu` and `winit` provide native graphics and OS integration. We use standalone libraries where appropriate without adopting an existing engine core.

The dependency direction is intentional:

```text
bozzard-server -> bozzard-demo -> bozzard-app -> bozzard-ecs
bozzard-demo -> bozzard-scene -> bozzard-ecs + glam + serde + bozzard-text
bozzard-player -> bozzard-demo
bozzard-player -> bozzard-render -> wgpu
bozzard-player -> bozzard-assets -> bozzard-scene + image + tobj
bozzard-player -> bozzard-audio -> Kira + CPAL
bozzard-player -> winit + AccessKit
bozzard-player -> bozzard-network -> bozzard-ecs + serde
bozzard-network (optional steam feature) -> steamworks
bozzard-server -> bozzard-network (pacing only) + ctrlc
```

`bozzard-render` accepts render data, never an ECS world. This leaves room for separate extraction, interpolation, batching, cameras, 2D sprites, and 3D meshes. Server compilation cannot accidentally initialize a window; its entire normal dependency tree is checked in CI.

## Middleware boundary

Typed middleware components use the scene component registry and extensible document storage. Animation rigs/clips, curves, blackboards, UI layouts, sprite/tile data and baked navigation run on the CPU without assets, windows or audio devices. The shared `bozzard-text` crate supplies CPU text shaping/metrics so headless UI layout agrees with rendering. Its Epaint font stack is reviewed in the headless dependency allowlist; WGPU, Winit, Egui integration, importers and CPAL remain outside it.

Asset workers import glTF skin/animation and probe file-backed compressed audio. The render bridge supplies immutable skin palettes, sprite geometry, widget items and particle descriptors; the renderer owns compute skinning and particle motion/sorting. Native audio owns device handles and decoded/streaming sound caches. Gameplay sees typed Blueprint controls and events. UI and audio-completion Delay chains carry a wall-clock flag so paused menus can act while normal simulation timers stay frozen. Save games include middleware playback, paths, widget overrides and accessibility preferences. See [middleware authoring and limits](middleware.md).

## ECS baseline

Entity handles contain a world ID, slot index, and generation. Recycling a slot changes its generation; exhausted generations retire the slot. Cross-world handles are rejected, including the future editor/play-world boundary. Handles cannot be constructed from arbitrary integers through the public API and must never be serialized as persistent scene or network IDs.

Components use one sparse set per type: a sparse entity-to-dense-index lookup plus dense entity/entry arrays. Swap removal repairs the moved element's sparse index. Component types are `Any + Send + Sync`; resources use the same ownership bounds. The initial safe query API supports individual component iteration and a mutable/read-only two-type join. Rust's disjoint map borrowing prevents aliasing; same-type mutable joins return an error. This is a deliberately small API, not an archetype or parallel ECS implementation.

Each component records the change tick it was last written on. Mutating access goes through a guard that marks the component when it is dereferenced mutably, so a reader can ask what changed since it last looked instead of rescanning the world; reads, inserts of nothing, and holding the guard are free, and `insert` marks on the current tick. Ticks are per world and advanced once per step by `App::step`, not per system, which is the honest granularity while systems run serially in one step. Removal and despawn are not tracked: presence is a query, not a tick. The first consumers are replication (what to send) and animation (which clip changed); nothing in the renderer needs it, because `bozzard-render` never borrows a world and already dedupes uniforms by value.

Sparse storage scales with the highest entity slot used per component type. Benchmark realistic workloads before adding archetypes, more flexible queries, filters, and parallel access. Query order can change on removal; never infer stable simulation order from it.

## Scheduling and time

Systems execute in registration order. Direct mutations are visible to later systems; deferred commands execute once, in queue order, after the tick's final system. A queued spawn/despawn is therefore visible to systems on the next tick. Queue closures are infallible at the scheduler boundary; callers handle operation errors inside them. No rollback is promised after a panic.

`App::step` advances exactly one fixed tick. `App::advance` accumulates elapsed wall time, limits catch-up, reports discarded whole ticks as a duration, and preserves the fractional remainder for interpolation. The headless harness uses `step`, so it never drops requested ticks. The headless harness also supports `--realtime`: a bounded 60 Hz pacer, overload diagnostics, and Ctrl-C/SIGTERM shutdown with optional final save. Steam multiplayer uses a player-hosted listen server; its pump runs independently of redraw events.

### Network determinism contract

Fixed ticks and serial scheduling do not guarantee cross-platform floating-point determinism.
Steam Flap Woods replicates host-authoritative bird components, pipe state, round and phase;
it does not use lockstep or exchange input-only world replays. Persistent network identities
are Steam IDs and bird slots, never ECS handles. Host-generated snapshots are the source of
truth for collision, elimination and score. Per-recipient acknowledged change ticks select
component deltas, while the complete lobby roster handles despawns and interest.

Clients predict only vertical bird motion and replay a bounded queue of unacknowledged
inputs after restoring host state. Numerical differences are corrected by each snapshot;
bit-identical floats across platforms are not required. The same-build CPU tests require
convergence after inputs drain, not identical intermediate predicted trajectories. Cross-OS
physics determinism, full-world rollback and portable input-only replay/save recordings are
not promised. Any future replay feature must record authoritative snapshots and versioned
game rules, or independently establish a deterministic simulation contract. See
[Steam multiplayer](multiplayer.md) for protocol bounds and acceptance limits.

## Modules

The current `Plugin` interface is a compiled-in registration hook identified by a unique name. It registers components/resources/systems through ordinary application APIs. Duplicate registration is rejected before the build hook runs. Modules do not yet have dependency resolution, runtime activation, unloading, or binary compatibility guarantees.

Next, define module manifests, dependency ordering, stage registration, and lifecycle cleanup. Keep editor-only modules out of runtime exports. If we add independently compiled native plugins, use a versioned C ABI and opaque engine handles rather than Rust layouts, references, or trait objects across the boundary. Live unloading must account for registered code, component values, callbacks, and jobs before freeing a library.

## Rendering and tests

Native backends: Metal on macOS, DX12 on Windows, Vulkan on Linux. Start with WGSL and baseline/downlevel limits. The first DX12 path uses wgpu's default FXC compiler to avoid a separate compiler DLL in the demo; evaluate bundled DXC before adding modern shader features.

The original graphics test renders a solid triangle to an RGBA8 target, copies it into a padded readback buffer, and checks interior/background pixels against an independent CPU half-plane oracle with a two-byte color tolerance. It then advances the shared ECS simulation and verifies a second image at an independently expected position. Pixels close to geometry boundaries are excluded to avoid rasterization-edge variability. A clear-only output must fail.

Offscreen testing does not cover desktop presentation. The player separately handles resize, zero-sized windows, surface reconfiguration, close/Escape, and bounded frame-count runs. Device loss/unrecoverable errors terminate with failure; full device recreation is a later task.

## Scenes and future exporting

Scenes now use persistent document-local object IDs, schema version 1, and explicit parent/camera references. See `scenes.md` for the current snapshot contract. The optional asset catalog maps persistent IDs to typed, relative source paths. Components are a registry: one row per component owns its scene key, editor label, availability, add/remove, prefab merge, and field metadata, and a component key an older build does not know is preserved as data rather than rejected. Editor play mode creates a separate world. Editor changes go through commands with undo/redo; saving serializes authored state, not incidental runtime data.

The future exporter will validate scenes, collect asset dependencies, cook assets for the target, compile selected runtime/game modules, and package the result. Current development packaging bundles the two fixed demos and their imported files. Public distribution additionally needs licenses/notices, platform signing/notarization, installer decisions, and minimum OS/runtime baselines verified on clean target systems.

The scene renderer adds reusable indexed quad/cube meshes, per-object MVP and normal matrices, opaque textured materials, and a depth target recreated on resize. Separate fixtures check texture quadrants, near/far occlusion in reversed draw orders, camera panning, and identical rendering after scene serialization. Math/serialization are allowed in the headless dependency tree; wgpu and winit are not.

## Imported asset lifecycle

The scene document owns stable asset IDs and source paths, and exposes reverse object dependencies. `bozzard-assets` loads file data on the CPU; it never owns a GPU resource. Handles are private store/index pairs and reject cross-store access. This first store is append-free after construction, so no slot reuse can invalidate a handle. Recreating a scene constructs a new store.

The player uploads CPU data into renderer-owned caches keyed by asset ID. The renderer accepts vertices, indices, and RGBA pixels and retains no dependency on the importer or scene crate. Changed textures invalidate object bind groups; changed meshes replace their buffers. Replacing a scene first loads a complete new store/world/renderer, then swaps them in. Suspending and recreating a window uploads the retained CPU assets to the new device.

Asset refresh runs in cancellable workers at a bounded interval. Images/models compare source/dependency contents; audio uses size/time as a fast path and streams a digest plus metadata when changed, retaining no compressed file bytes in the catalog. Explicit reload bypasses the audio fast path. Failed imports retain the last good data/revision. Native sound decoding uses a separate bounded cache or streaming reader. See `assets.md` and [middleware](middleware.md) for current limits.

Editor/player GPU residency requests the imported assets used by the extracted view,
including material overrides and shadow casters. A configurable soft budget reserves
upload storage, evicts unused assets in LRU order, and restores them through the same
staged queue. Required resources remain pinned and report budget pressure. CPU
catalog decoding remains separate. See [asset budget scope](assets.md#gpu-asset-budget).
