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
- Implemented: store-scoped asset handles, pending/ready/failed state, scene-to-asset dependency mapping, background open/import/save/reload jobs, staged GPU residency, and last-good recovery.
- Implemented: textured 2D quads and indexed 3D cubes with depth testing, authored sun/ambient and point/spot lighting, shadows, procedural environment lighting, baked diffuse GI, PBR materials, bloom and HDR display transforms, culling, and state caching. PNG/JPEG, OBJ, and glTF/GLB import supplement these built-ins.
- Implemented: scene round-trip, texture/depth/camera rendering checks, and player/headless/package integration.

The current import slice supports static OBJ and glTF/GLB geometry, material dependencies, mipmaps, and scalable external-resource packaging. Streaming, panorama environments, reflection probes, and atmospheric simulation remain future extensions; baked diffuse GI is available as a bounded editor-authored volume.

## 3 — Editor module (current slice)

- Implemented: native egui/wgpu shell with resizable panels and persisted workspace settings.
- Implemented: scene hierarchy, inspector, viewport ray selection and move/rotate/scale axis handles.
- Implemented: validated document commands with bounded, gesture-coalescing undo/redo.
- Implemented: separate edit/play worlds; save, Save As, play, stop, unsaved-change prompts.
- Implemented: project-local PNG/JPEG/OBJ/glTF/GLB imports with catalog assignment, background preparation, sun/display, point/spot light, and baked GI controls, and staged GPU uploads.

Remaining editor extensions: generic component reflection, arbitrary tab docking, and production gizmo ergonomics.

## 4 — First playable third-person demo (implemented locally)

- Serialized Player Controller settings, selection-independent camera-relative movement and grounded jumping.
- Configurable follow camera height/distance, mouse orbit and conservative solid-box obstruction avoidance.
- Non-solid collectible/checkpoint/goal volumes, automatic fall respawn and visible progress/win feedback.
- First Trail: ready-to-play authored level with obstacles and reused local static-model scenery, shared by editor Play and standalone player.
- Shared headless fixed-step runtime, inspector authoring, validated references/settings and deterministic route/Play isolation regressions.

See [run commands and manual acceptance checklist](playable-demo.md). This is a single kinematic box character, not skeletal animation, full physics, scripting or a game export pipeline. Manual pointer/platform verification remains separate from automated simulation/rendering checks.

## 5 — First user-game export

- Project manifest and selected runtime modules.
- Asset cooker and deterministic package manifest.
- Native target builds dispatched through CI.
- Playable exported project tested outside its source directory.
- Minimum OS baselines, dependency notices, signing/notarization plan.

## 6 — Simulation and content systems

- Full physics, audio, animation, input actions, cascaded shadows, runtime rebaking, and production lighting/content systems.
- Scene/prefab composition and scripting design.
- Module dependency/lifecycle contract; evaluate runtime binary loading.
- Profiling and evidence-driven ECS/render improvements.

## 7 — Dedicated game servers

- Real-time pacing, shutdown and operational diagnostics.
- Networking transport, entity replication, authority and interest management.
- Client prediction/interpolation as required by a reference game.
- Multi-client integration tests with loss/latency scenarios.

Build both 2D and 3D reference scenes as engine acceptance fixtures. Each milestone should exercise a complete workflow before expanding feature breadth.
