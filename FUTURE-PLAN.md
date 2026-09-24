# Future plan

For the next implementation sequence, use the [Claude UI todo list](TODO-CLAUDE.md)
and [Codex runtime todo list](TODO-CODEX.md), reviewed against `main` at `27381da`
on 2026-09-24. This older feature inventory contains stale unchecked entries;
in particular, Blueprint depth has shipped as documented in
[Blueprint authoring depth](docs/blueprint-depth.md), and the multiplayer CPU
optimization baseline is recorded in [multiplayer performance](docs/multiplayer.md#cpu-performance-benchmark).

What Bozzard still needs to be an engine a team ships games in, judged against Unity and Unreal.
Checked items exist today and are covered by tests or docs; unchecked items are work, sized
`S`/`M`/`L`/`XL` with the dependency that makes them tractable.

The verdict behind this list: the renderer and the document spine are already at or above what a
team this size normally has, and the two layers that make Unity and Unreal what they are — a
component/reflection model and a gameplay middleware stack — are missing. The risk is not missing
features, it is that the hardcoded scene schema makes every added feature cost 30–60 edit sites.

## 0 — Already engine-grade

- [x] Own ECS with generational handles, sparse-set components, fixed ticks, deferred commands, shared headless runtime (`crates/bozzard-ecs`, `crates/bozzard-app`).
- [x] Diffable textual scenes with persistent object IDs, schema version, validated references and round-trip tests (`crates/bozzard-scene/src/lib.rs`, `docs/scenes.md`).
- [x] Command-based undo/redo, edit/play world separation, save/Save As and unsaved-change prompts (`crates/bozzard-editor`).
- [x] Prefab instances with overrides, apply, refresh and unpack (`docs/prefabs.md`).
- [x] Blueprint graphs (~90 nodes) and shader node graphs compiling to WGSL, both portable files, both exercised headlessly (`docs/blueprints.md`, `docs/shader-editor.md`).
- [x] Rendering breadth: PBR, baked GI, volumetrics, fog, SSR, DOF, bloom, auto-exposure, TAA/motion blur, particles, sun/spot/point shadows, environment sky, HUD text.
- [x] Asset lifecycle: typed handles, pending/ready/failed, background jobs, staged GPU residency, last-good recovery, reverse dependency map (`docs/assets.md`).
- [x] Game export: project manifest, transitive packaging, deterministic inventory/ZIP, verified on all three native targets (`docs/exporting.md`).
- [x] Native CI matrix on macOS/Metal, Windows/DX12, Ubuntu/Vulkan; measured performance work with recorded medians (`docs/optimization-results.md`).
- [x] Loud failure over silent loss: `deny_unknown_fields` everywhere, unsupported glTF features rejected rather than dropped, `unsafe_code = "forbid"`.

## 1 — Component model and change tracking (the blocker)

`Object` is a fixed struct of `Option<...>` fields (`crates/bozzard-scene/src/lib.rs:319`) and is the
whole component set. Field sites per component today: `collider` 58 sites/12 files, `gravity` 49/13,
`player_controller` 42/7, `mesh_collider` 29/6, `text_rendering` 22/7.

- [x] **L** Component registry row per component (`crates/bozzard-scene/src/component.rs`): scene key, editor label, availability, add, remove, prefab three-way merge, field metadata, and typed `field`/`set_field` per component. Typed structs stayed; no derive-macro framework.
- [x] **M** Add Component menu, removal cascades, section hints and prefab refresh all read the registry. The prefab merge previously kept a parallel `merge!` field list that silently omitted shader graphs; refresh now merges every registered component.
- [x] **M** Field metadata covers booleans, bounded numbers, vectors with roles and per-axis minimums, text, option lists, filtered object references with view activation, and asset references. Variant-only fields hide behind a `visible` predicate, and tag changes carry values across variants.
- [x] **M** Generic field renderer (`apps/editor/src/component_ui.rs`) with small `extras` hooks for derived readouts and scene-touching buttons. Spin, Rigidbody, Box Collider, Player Controller, Light and Camera migrated; the jump estimate, gravity reset and camera view buttons survive as extras.
- [x] **M** Blueprint node registration table: one row per kind generates the enum, pin lists, add-node menu and the `event`/`action` classification, replacing `ALL`, `title`, `inputs` and `outputs` tables. Adding a node is one row plus its runtime arm.
- [x] **M** Migrate the remaining hand-written inspector sections to fields: Text Rendering, Mesh Renderer (including mesh settings), Material, Mesh Collider and Particle Emitter. Field vocabulary grew to cover whole numbers, two-axis vectors with ranges, body text, and texture and mesh pickers; asset-derived readouts and scene-touching buttons stayed as small `extras` hooks. Only Blueprint and Shader Graph keep hand-written sections, because a graph and an attachment list are not field-shaped. A Trigger is field-driven (its action is an option list and its respawn is a vector shown only for Checkpoint); the scene-derived safe respawn it needs is a small hook next to the fields, and picking Checkpoint fills it in rather than leaving a placeholder.
- [x] **L** Forward-compatible components. An object's components are its own keys; a key with no registry row is preserved in `Object::extras`, written back unchanged, and listed in the Inspector as unrecognized. A typo inside a *known* component still fails loudly with the component and field named. No version bump was needed: the shape never changed, only the strictness of the component set, so v1 files load and save unchanged. `ComponentType::load` is the per-component migration seam for a future rename or reshape.
- [x] **M** `register_component` puts a game-local row in the same registry, backed by `Object::extras`, so a gameplay type does not have to be compiled into the engine's schema; prefab refresh, add/remove, the field list and the generic UI treat it like a built-in. Typed Rust access still needs a built-in row.
- [x] **M** Change tracking per component, in the ECS component store where it cannot lie (`crates/bozzard-ecs`): every component records the tick it was last written on, mutating access goes through a `Mut` guard that marks on `DerefMut`, and `changed_tick`/`is_changed_since`/`changed_since` answer what moved since a bookmark. `App::step` advances the tick once per step, so a bookmark taken at the end of one step sees exactly the next step's writes.  *Not* per component in the scene document: typed fields are mutated directly at ~200 sites and edits are whole-object clone-then-apply, so revisions there would have to be touched at every write site or lie. The scene document already has one `revision` counter for GI freshness.
- [x] **S** Consume the change ticks where rescanning is real work: replication in section 7 (what to send since the last acknowledgement) and animation once clips exist (which pose changed). Steam Flap Woods now consumes bird change ticks per acknowledged recipient in `bozzard-network`; animation remains deferred. The renderer never borrows a world and dedupes uniforms by value, `ShadowFrame::same_sun` compares five `Lighting` fields, physics cooks colliders once at boot, and the demo's spin and movement systems must run every step regardless.

Definition of done: adding a component is a module plus one registry row; adding a node is one table
row plus its runtime arm; and a scene saved by a newer engine loads in an older one with the unknown
component preserved.

## 2 — Blueprint authoring depth

Blueprints are typed, validated and headless-testable — this is the differentiator and stays the
primary gameplay path. Its value types are the 6 scalars `Exec/Text/Number/Bool/Vector/Object`, and
the other authoring path is [Rhai scripting](docs/scripting.md), which calls the same engine actions
and shares the same blackboards.

- [ ] **M** Shared variable scopes: object-level and scene-level blackboards that several graphs can address. Variables are currently per graph attachment, which is why a single player controller ends up as one 97-node graph.
- [ ] **S** Missing events: `On Destroy`, `On Enable`, `On Disable`, collision (non-trigger) entry with contact normal/impulse, and a stateful `Delay`/`After` node (`Elapsed Time` plus a comparison is not a timer).
- [ ] **S** Missing math: `Lerp`, `Min`/`Max`, `Abs`, `Length`, `Normalize`, `Dot`, `Cross`, `Distance`, `Modulo`, `Power`, `Random`, plus angle/trig helpers beyond `Sine`.
- [ ] **M** Query nodes: raycast, sphere/box overlap, and line-of-sight, sharing the collision code the editor picking already uses.
- [ ] **M** Container values (array, map) or a bounded list type, since no gameplay state that grows is expressible today.
- [ ] **M** Runtime scene control: load/additive-load a scene, restart, and save/load game state. There is currently exactly one scene per process.
- [ ] **S** Blueprint node UX: comments/reroute nodes, per-node search, copy/paste of subgraphs, and a stale-wire diff when a graph fails validation.
- [x] **M** Blueprint debugger: saved breakpoints, pause/continue, node/tick stepping, runtime instance selection, live pin/blackboard/timer watches, execution highlighting and bounded exportable traces. Headless continuations preserve simulation order; see [debugging](docs/debugging.md#blueprint-debugger).
- [x] **M** Scripting as the second gameplay path: a **Script Manager** component running Rhai scripts whose hooks call every engine action a blueprint node calls, reading and writing the same object and scene blackboards so one scene can mix both.
- [ ] **S** Script authoring depth: reload a script while Play is running instead of on the next open, a script pane with completion for the exposed functions, and per-script hook/command statistics next to the blueprint stats.

- [x] **L** Script-accessible compute: validated WGSL assets, shared Rust/Rhai resource and dispatch API, asynchronous readback, generated material textures, hot reload, runtime inspection, profiling, export, and sample scenes. See [compute authoring](docs/compute.md) and [measurements](docs/compute-validation.md).

## 3 — Physics surface

Rapier is in and owns velocities, inertia and sleeping (`crates/bozzard-scene/src/physics.rs`); what is
exposed to authors was thin. See [the physics surface](docs/physics.md) for the authoring model and
its limits.

- [x] **M** Collision layers/masks. `layers`/`mask` live on Box Collider, Mesh Collider and Trigger;
eight layers are named for authoring and the rest are reserved. One rule
(`a.layers & b.mask != 0 && b.layers & a.mask != 0`) drives Rapier's `InteractionGroups`, the CPU
swept-box mover and overlap/contact reporting, so a gameplay volume on its own layer stops seeing
everything. `crates/bozzard-scene/tests/layers.rs`; registry fields in
`crates/bozzard-scene/src/component.rs`.
- [x] **M** Raycast/overlap queries in Blueprints, with results addressable as objects. Shipped with
section 2 (Raycast, Sphere/Box Overlap, Line of Sight) and documented in
[docs/blueprint-depth.md](docs/blueprint-depth.md#math-and-queries).
- [x] **M** Joints/constraints through the component registry: Fixed, Hinge (revolute), Ball socket
(spherical), Slider (prismatic) and Rope, with anchors and local axes per body and optional limits.
Both endpoints resolve to their Rigidbody root, so a joint on a compound child constrains the body
that owns it; a zero-anchor Fixed joint keeps the authored relative pose. Rapier impulse joints,
rebuilt when their values or a body change. `crates/bozzard-scene/src/joint.rs`,
`crates/bozzard-scene/tests/joints.rs`.
- [x] **M** Capsule character controller. The Player Controller is a Rapier
`KinematicCharacterController`: capsule radius/height in world units, step height, slope limit,
ground snap and swept movement. One `move_shape` per tick combines walking, gravity and platform
carry, and a teleported or dynamic platform's motion is passed through to its rider. The authored
Box Collider stays for the CPU queries (triggers, respawn, camera, Blueprints).
`crates/bozzard-scene/tests/capsule.rs`; demo routes re-verified in
`crates/bozzard-scene/tests/gameplay.rs` and `crates/bozzard-editor/tests/gold_yard.rs`.
- [x] **S** Per-body mass, drag, gravity scale, friction and restitution; compound colliders.
Mass, friction and restitution existed; `linear_damping` and `gravity_scale` join them on the
Rigidbody row. Colliding descendants now compose into one Rapier body with multiple shapes instead
of being rejected, and contacts still name the object that owns the shape.
`crates/bozzard-scene/tests/compound.rs`.
- [x] **S** Contact events carrying impulse and normal for non-trigger collisions. Shipped with
section 2 as **On Collision Enter** and documented in
[docs/blueprint-depth.md](docs/blueprint-depth.md#events-and-timers); the geometric overlap events
remain for trigger volumes.
- [x] **S** Continuous collision for fast projectiles. Every dynamic body already has Rapier CCD
enabled with four substeps (`crates/bozzard-scene/src/physics.rs`), and the player's controller is
swept, so neither tunnels; the target-range projectile's cleanup graph is no longer the only thing
keeping it inside the arena.

## 4 — Middleware

These content classes are authored through typed scene components and controlled by Blueprints.
See [middleware authoring, limits and verification](docs/middleware.md), with complete 3D and 2D
reference scenes in `examples/demo/scenes/middleware-lab.json` and `ui-2d-lab.json`.

- [x] **L** Audio: device output, mixing buses, 3D attenuation/panning, streaming and compressed formats, an audio component, and blueprint play/stop/parameter nodes.
- [x] **XL** Skeletal animation: glTF skin/animation import, skinning in the PBR pipeline, clips and a state machine with blend trees, events, root motion, and a timeline for cinematics.
- [x] **M** Tween/curve evaluation for authored motion (translation, rotation, scale, colour, material values) so simple motion stops needing hand-built graph chains.
- [x] **L** UI and 2D: widget/layout system, canvas and anchors, sprite atlases, 2D animation, tilemaps, nine-slice, localization, accessibility, and authorable menus replacing the engine-drawn game-flow overlays.
- [x] **L** AI/navigation: navmesh generation and baking, pathfinding, agents with steering, perception, and a state-machine authoring surface.
- [x] **M** Particles depth: authorable emitters in the inspector, curve modules, GPU simulation, and sorting against transparency.

## 5 — Scale and content pipeline

- [x] **L** Batching/instancing and LOD. Consecutive opaque surfaces batch into up to 32 instances; authored distance LOD supports bounded hysteresis, explicit far culling and cancellable static-mesh simplification. Conservative hierarchical-depth occlusion uses current-frame GPU queries and reuses completed visibility only for identical depth inputs and bounds. Reference-pixel checks, moving/hidden/open release measurements, native warehouse controls and a source-independent exported player pass. On M2 Pro/Metal at 320×320, 1,024 hidden spheres reduce 33 color commands to one after readback; synchronized medians are 2.722 → 1.203 ms static and 2.802 → 2.019 ms with a moving shutter. These are local fixture measurements, not FPS guarantees. See [asset rendering](docs/assets.md), [LOD](docs/lod.md) and [occlusion](docs/occlusion.md).
- [x] **M** Texture compression (BC/ASTC) and a GPU memory budget with eviction. Imported GPU assets have a configurable soft budget, LRU eviction, staged restoration and editor/player diagnostics; required frame and shadow resources stay pinned. Checked BTEX/BMESH cooking covers standalone images and embedded PBR maps, with platform selection and lossless fallback. Automatic project export includes models, skins, animation, runtime-scene and prefab dependencies. CPU, native Metal, editor history/UI and relocated package checks pass; see [texture/model cooking](docs/texture-compression.md) and [asset residency](docs/assets.md#gpu-asset-budget).
- [x] **M** Async and additive scene loading, plus multi-scene editing in the editor. Runtime-library preparation now has a worker, progress/cancellation, guarded publication, additive ownership/unload and checkpoint support, exposed to Blueprints and scripts. Lazy file/content acquisition now prepares dependencies and decoded catalogs before publication, and exports follow local scene references. File/content loading, cancellation/retry, ownership and fresh-runtime checkpoint restoration are verified through CPU tests and native editor/player workflows. The editor opens up to 16 independent documents in a shared viewport with per-scene visibility, picking/activation, history and saves; decoded assets are shared by the preview. Native editing, Undo, visibility and dirty-document close/save checks pass; see [scene loading](docs/scene-loading.md).
- [x] **M** Prefab nesting, variants and source-hierarchy editing. Nested sources and inherited variants resolve through a shared bounded loader in editor, runtime and export. Source documents have isolated previews, hierarchy editing and independent history; refresh preserves component overrides and root placement. CPU regressions, relocated exports and native source edit/save/refresh/Play checks pass; see [prefabs](docs/prefabs.md).
- [x] **M** Shared material sources, inherited variants and per-object overrides have typed validation, portable import/export and guarded background source editing. Static shader keywords prune inactive code and share bounded source/pipeline caches. Identical material maps share CPU pixels and GPU storage, including across variants and property-only edits. CPU/GI/prefab/additive tests, native editor history/Play, texture residency and source-independent exported/content-pack players pass; see [materials](docs/materials.md).
- [x] **M** Asset cooking and bundles: platform-specific model/image cooking and dependency-driven incremental rebuilds cover editor/CLI export and content packs. Cache keys include source dependencies, target and cooker/codec version; corruption rebuilds safely, and current baked GI survives export. Bounded streaming bundles, typed address catalogs, HTTPS downloads, cancellation and immutable cache generations are verified through CPU/HTTP tests, the editor builder, and relocated native player launches after source deletion. See [content packs](docs/content-packs.md).
- [x] **S** More importers: [documented FBX → Blender → glTF conversion](docs/fbx-conversion.md), plus custom TTF/OTF fonts (up to 4 MiB), variable-font axes, ordered custom fallback chains and optional bundled glyph fallback. Font import validation, inspector controls, layout/picking, Undo/Redo, prefab remapping and portable export are verified; see [text rendering](docs/text-rendering.md). WAV, OGG/Vorbis, MP3 and FLAC audio are supported by section 4.
- [x] **M** Level building: typed terrain heightfields with sculpt brushes and synchronized collision, deterministic foliage scattering, reusable Box/Ramp/Stairs/Cylinder brushes, viewport snapping, grid and measurement. Guarded background jobs publish one Undo step and preserve saved prefab geometry. Custom component inspectors plug into an embeddable editor build; resizable dock groups, tabs and floating panels persist workspace layout. CPU tests, native authoring/history/Play and a source-independent exported player pass; see [level building](docs/level-building.md) and [editor extensions](docs/editor-extensions.md).
- [x] **S** Project ergonomics: File → New project and `bozzard-project new` create independent 2D collection and 3D exploration projects. Both are included under `examples/starter-{2d,3d}` and pass relocated native-package checks. `bozzard-project merge` performs structural three-way JSON merges by persistent object ID, preserves input files, and reports conflicting edits or invalid merged hierarchies. See [project workflows](docs/projects.md).

## 6 — Runtime and shipping

- [x] **M** In-editor profiler: optional CPU system/stage timings, asynchronous GPU pass timings with availability reporting, render counters, live graphics memory, and a searchable source-linked console. Bounded captures export to JSON; see [debugging](docs/debugging.md).
- [ ] **S** Device-loss recovery and a bounded crash report path; unrecoverable device errors terminate the process today.
- [ ] **M** Distribution completion: selected-module builds, certified minimum OS baselines verified on clean machines, dependency notices, signing/notarization, installer decisions (`docs/exporting.md`).
- [ ] **M** Module manifest with dependency ordering, staged registration, and lifecycle cleanup; modules are compiled-in hooks without dependency resolution today (`docs/architecture.md`).
- [ ] **L** Text scripting with hot reload, evaluated *before* native dylib plugins: an embedded VM (Rhai/Lua/WASM) calling the same action layer blueprints use. Native plugins stay behind a versioned C ABI.
- [ ] **S** Ops basics a shipped game expects: save-game format and versioning, localization pipeline, accessibility options, telemetry/crash reporting hooks.
- [ ] **XL** Additional platforms: web (wasm), mobile, consoles. Only after one game ships natively.

## 7 — Networking

Section 1 cleared the blocker: components have per-component load/save hooks, so a component can be
serialized by name, and every component carries a change tick, so replication can ask what moved
since the last acknowledgement instead of diffing the world. The headless harness now supports bounded real-time pacing; Steam reference multiplayer runs a
listen server in the player or editor Play. See [Steam multiplayer](docs/multiplayer.md).

- [x] **M** Real-time pacing, overload policy, graceful shutdown and operational diagnostics. Shared 60 Hz/eight-step pacer; independent Steam event pump, counters and timeout handling; headless `--realtime` and Ctrl-C/SIGTERM final-save path.
- [x] **L** Transport, entity replication, authority model, and interest management for the reference game. Optional Steam Networking Messages transport, friends-only lobbies, invitations, original-host-only start/retry, 2–4 independently controlled birds, acknowledged ECS component deltas and complete lobby relevance/despawn rosters. This is a bounded Flap Woods implementation, not arbitrary-scene replication.
- [x] **L** Client prediction/interpolation and rollback if the reference game needs it. Local vertical motion prediction with bounded input replay/reconciliation; remote bird/pipe interpolation; collisions and scores remain authoritative. Full-world rollback is not needed by this reference.
- [x] **M** Multi-client integration tests with scripted loss and latency, serialized packets, duplication/reordering, lost acknowledgements, roster changes, stale rounds, authority rejection and eventual prediction convergence; authored scene and scoring fixtures.
- [x] **S** `docs/architecture.md` determinism statement: host snapshots are authoritative; prediction corrections tolerate cross-platform float differences. No lockstep, full-world rollback or portable input-only replay guarantee.
- [x] **Editor Play** Shared player/editor session, main-thread publication after async preparation, independent networking worker, Stop/Quit cleanup and edit-world isolation. Starting with a multiplayer scene initializes Steam before graphics; solo/blank scenes never initialize it. Overlay-free friend invitations use the same authored UI in both apps. Headless lifecycle tests cover stop/restart, no-redraw pumping, input/UI routing and disabled-build errors.
- [x] **Native build and export** Standard editor/player builds enable Steam and stage the SDK without Python. Editor Export bundles the native library and inventories it; relocated executable tests remove tool/library-path overrides. The component exposes App ID; Spacewar development exports include its ID file, while store exports use Steam launch context and omit development overrides. Multiplayer exports validate the companion player's target and SDK hash.
- [ ] **M** Profile and optimize scripted Flap Woods multiplayer. Investigate the reported lag with repeatable editor Play and exported-player captures, comparing debug and release builds with 2–4 players and simulated latency/loss. Record CPU/GPU frame times (median/p95/p99), network tick time, replay depth and allocation costs before changing behavior. Suspected overheads, not yet measured bottlenecks: every presentation update runs the full scene scripting pipeline (including collision snapshots/overlaps), and simulation/prediction repeatedly converts Rust state through JSON into Rhai and back. Use measurements to reduce redundant presentation work, avoid unused collision/query preparation, and cache or streamline script calls/state conversion while retaining editable Rhai gameplay, host authority and prediction/reconciliation correctness. Publish before/after results and add a reproducible performance regression benchmark; functional tests alone do not establish smooth frame pacing.
- [ ] **Acceptance** Live two-account Steam invite/overlay/relay and native-window run on target machines. CPU regressions and Steam-feature compilation do not establish Valve backend or platform-overlay acceptance. Windows/macOS packaging also needs target-machine verification.

## Deliberately not doing

- [ ] **Not planned** Archetype or parallel ECS. Current measurements do not justify it; the docs already say to benchmark realistic workloads first.
- [ ] **Not planned** Copying Unity's GameObject/MonoBehaviour shape. Typed validated graphs plus diffable textual scenes are the moat.
- [ ] **Not planned** A derive-macro reflection framework. Hand-written `schema()` per component until it demonstrably hurts.
- [ ] **Not planned** Cascades, AI or platform ports before a reference game needs them. Networking now has the Flap Woods Together reference.
- [ ] **Not planned** Rewriting physics, assets or rendering on a new dependency, or dropping WGSL, `wgpu` and `winit`.

## Sequencing

| Phase | Unlocks | Why first |
| --- | --- | --- |
| 1 Component model + change tracking | 2, 3, 4, 6 (scripting), 7 (replication) | Every later phase becomes cheaper; today each one pays the 30–60 edit-site tax |
| 2 Blueprint depth | Content authoring, prototypes | Cheapest gameplay win per line; keeps gameplay in the validated path |
| 3 Physics surface | Action games, character work | Rapier already does the work; only the exposing layer is missing |
| 4 Middleware | Anything with characters, sound, menus or AI | Largest content gaps; each is independent, so order by the reference game |
| 5 Scale and pipeline | Big levels, real projects | Do it when a scene actually hurts, and measure before and after |
| 6 Runtime and shipping | Releasing a game | Export exists; the remaining work is signing, tooling and one reference game |
| 7 Networking | Multiplayer | Needs 1 first, and a reference game to define authority and prediction |

Build both 2D and 3D reference scenes as engine acceptance fixtures. Each milestone should exercise a
complete workflow before expanding feature breadth.
