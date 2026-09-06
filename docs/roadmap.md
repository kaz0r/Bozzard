# Milestones

## 1 — Runnable foundation

- Custom ECS with entity lifetime checks, dense component iteration, queries, resources, commands.
- Serial scheduling, fixed ticks, compiled-in module registration.
- Native WebGPU triangle and a graphics-free headless simulation.
- Pixel readback checks, native CI definitions, development binary bundles.

Completed: local checks and the first successful GitHub matrix run verify all three native targets (`d319721`, run 34019215092). Linux also presents both views under Xvfb.

## 2 — Scene and asset foundation (current slice)

- Implemented: transform hierarchy with orthographic/perspective camera projections.
- Implemented: persistent object IDs and versioned scene snapshots. Stable imported-asset IDs now map to relative source paths.
- Implemented: store-scoped asset handles, pending/ready/failed state, scene-to-asset dependency mapping, and synchronous polling hot reload with last-good recovery.
- Implemented: textured 2D quads and indexed 3D cubes with depth testing/basic lighting. PNG/JPEG and OBJ import now supplement these built-ins.
- Implemented: scene round-trip, texture/depth/camera rendering checks, and player/headless/package integration.

The initial import slice supports opaque images and static OBJ geometry; async loading, glTF/material dependencies, mipmaps, and streaming are future extensions.

## 3 — Editor module (next)

- Native egui integration, docking/workspace persistence.
- Scene hierarchy, inspector, viewport selection and transform gizmos.
- Editable component metadata and command-based undo/redo.
- Separate edit/play worlds; save, load, play, stop.

## 4 — First user-game export

- Project manifest and selected runtime modules.
- Asset cooker and deterministic package manifest.
- Native target builds dispatched through CI.
- Playable exported project tested outside its source directory.
- Minimum OS baselines, dependency notices, signing/notarization plan.

## 5 — Simulation and content systems

- Physics, audio, animation, input actions, materials and lighting.
- Scene/prefab composition and scripting design.
- Module dependency/lifecycle contract; evaluate runtime binary loading.
- Profiling and evidence-driven ECS/render improvements.

## 6 — Dedicated game servers

- Real-time pacing, shutdown and operational diagnostics.
- Networking transport, entity replication, authority and interest management.
- Client prediction/interpolation as required by a reference game.
- Multi-client integration tests with loss/latency scenarios.

Build both 2D and 3D reference scenes as engine acceptance fixtures. Each milestone should exercise a complete workflow before expanding feature breadth.
