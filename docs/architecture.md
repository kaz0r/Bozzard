# Architecture decisions

## Ownership and dependencies

Bozzard owns its ECS, scheduling, scene model, renderer design, module contract, and editor/export workflows. `wgpu` and `winit` provide native graphics and OS integration. We use standalone libraries where appropriate without adopting an existing engine core.

The dependency direction is intentional:

```text
bozzard-server -> bozzard-demo -> bozzard-app -> bozzard-ecs
bozzard-demo -> bozzard-scene -> bozzard-ecs + glam + serde
bozzard-player -> bozzard-demo
bozzard-player -> bozzard-render -> wgpu
bozzard-player -> bozzard-assets -> bozzard-scene + image + tobj
bozzard-player -> winit
```

`bozzard-render` accepts render data, never an ECS world. This leaves room for separate extraction, interpolation, batching, cameras, 2D sprites, and 3D meshes. Server compilation cannot accidentally initialize a window; its entire normal dependency tree is checked in CI.

## ECS baseline

Entity handles contain a world ID, slot index, and generation. Recycling a slot changes its generation; exhausted generations retire the slot. Cross-world handles are rejected, including the future editor/play-world boundary. Handles cannot be constructed from arbitrary integers through the public API and must never be serialized as persistent scene or network IDs.

Components use one sparse set per type: a sparse entity-to-dense-index lookup plus dense entity/value arrays. Swap removal repairs the moved element's sparse index. Component types are `Any + Send + Sync`; resources use the same ownership bounds. The initial safe query API supports individual component iteration and a mutable/read-only two-type join. Rust's disjoint map borrowing prevents aliasing; same-type mutable joins return an error. This is a deliberately small API, not an archetype or parallel ECS implementation.

Sparse storage scales with the highest entity slot used per component type. Benchmark realistic workloads before adding archetypes, more flexible queries, filters, and parallel access. Query order can change on removal; never infer stable simulation order from it.

## Scheduling and time

Systems execute in registration order. Direct mutations are visible to later systems; deferred commands execute once, in queue order, after the tick's final system. A queued spawn/despawn is therefore visible to systems on the next tick. Queue closures are infallible at the scheduler boundary; callers handle operation errors inside them. No rollback is promised after a panic.

`App::step` advances exactly one fixed tick. `App::advance` accumulates elapsed wall time, limits catch-up, reports discarded whole ticks as a duration, and preserves the fractional remainder for interpolation. The headless harness uses `step`, so it never drops requested ticks. A real server will need wall-clock pacing, overload policy, graceful shutdown, and networking.

Fixed ticks and serial scheduling do not guarantee cross-platform floating-point determinism. Replication/replay design must state its actual determinism requirements separately.

## Modules

The current `Plugin` interface is a compiled-in registration hook identified by a unique name. It registers components/resources/systems through ordinary application APIs. Duplicate registration is rejected before the build hook runs. Modules do not yet have dependency resolution, runtime activation, unloading, or binary compatibility guarantees.

Next, define module manifests, dependency ordering, stage registration, and lifecycle cleanup. Keep editor-only modules out of runtime exports. If we add independently compiled native plugins, use a versioned C ABI and opaque engine handles rather than Rust layouts, references, or trait objects across the boundary. Live unloading must account for registered code, component values, callbacks, and jobs before freeing a library.

## Rendering and tests

Native backends: Metal on macOS, DX12 on Windows, Vulkan on Linux. Start with WGSL and baseline/downlevel limits. The first DX12 path uses wgpu's default FXC compiler to avoid a separate compiler DLL in the demo; evaluate bundled DXC before adding modern shader features.

The original graphics test renders a solid triangle to an RGBA8 target, copies it into a padded readback buffer, and checks interior/background pixels against an independent CPU half-plane oracle with a two-byte color tolerance. It then advances the shared ECS simulation and verifies a second image at an independently expected position. Pixels close to geometry boundaries are excluded to avoid rasterization-edge variability. A clear-only output must fail.

Offscreen testing does not cover desktop presentation. The player separately handles resize, zero-sized windows, surface reconfiguration, close/Escape, and bounded frame-count runs. Device loss/unrecoverable errors terminate with failure; full device recreation is a later task.

## Scenes and future exporting

Scenes now use persistent document-local object IDs, schema version 1, and explicit parent/camera references. See `scenes.md` for the current snapshot contract. The optional asset catalog maps persistent IDs to typed, relative source paths. Generic component schemas remain future work. Editor play mode will create a separate world. Editor changes should go through commands with undo/redo; saving must serialize authored state, not incidental runtime data.

The future exporter will validate scenes, collect asset dependencies, cook assets for the target, compile selected runtime/game modules, and package the result. Current development packaging bundles the two fixed demos and their imported files. Public distribution additionally needs licenses/notices, platform signing/notarization, installer decisions, and minimum OS/runtime baselines verified on clean target systems.

The scene renderer adds reusable indexed quad/cube meshes, per-object MVP and normal matrices, opaque textured materials, and a depth target recreated on resize. Separate fixtures check texture quadrants, near/far occlusion in reversed draw orders, camera panning, and identical rendering after scene serialization. Math/serialization are allowed in the headless dependency tree; wgpu and winit are not.

## Imported asset lifecycle

The scene document owns stable asset IDs and source paths, and exposes reverse object dependencies. `bozzard-assets` loads file data on the CPU; it never owns a GPU resource. Handles are private store/index pairs and reject cross-store access. This first store is append-free after construction, so no slot reuse can invalidate a handle. Recreating a scene constructs a new store.

The player uploads CPU data into renderer-owned caches keyed by asset ID. The renderer accepts vertices, indices, and RGBA pixels and retains no dependency on the importer or scene crate. Changed textures invalidate object bind groups; changed meshes replace their buffers. Replacing a scene first loads a complete new store/world/renderer, then swaps them in. Suspending and recreating a window uploads the retained CPU assets to the new device.

Polling reads source bytes every 500 ms and compares contents, avoiding timestamp granularity problems. Failed imports report an error once per changed source and retain the previous data and revision. This is a small synchronous baseline: background import jobs, memory budgets, cancellation, incremental dependency graphs, and streaming are deliberately later work. See `assets.md` for current limits.
