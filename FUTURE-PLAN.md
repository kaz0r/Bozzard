# Future plan

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
- [x] Loud failure over silent loss: `deny_unknown_fields` everywhere, glTF skins/animations rejected rather than dropped, `unsafe_code = "forbid"`.

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
- [ ] **S** Consume the change ticks where rescanning is real work: replication in section 7 (what to send since the last acknowledgement) and animation once clips exist (which pose changed). Deliberately no consumer yet — the renderer never borrows a world and dedupes uniforms by value, `ShadowFrame::same_sun` compares five `Lighting` fields, physics cooks colliders once at boot, and the demo's spin and movement systems must run every step regardless.

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
- [x] **M** Scripting as the second gameplay path: a **Script Manager** component running Rhai scripts whose hooks call every engine action a blueprint node calls, reading and writing the same object and scene blackboards so one scene can mix both.
- [ ] **S** Script authoring depth: reload a script while Play is running instead of on the next open, a script pane with completion for the exposed functions, and per-script hook/command statistics next to the blueprint stats.

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

Each of these is a content class the engine cannot represent at all today.

- [ ] **L** Audio: device output, mixing buses, 3D attenuation/panning, streaming and compressed formats, an audio component, and blueprint play/stop/parameter nodes.
- [ ] **XL** Skeletal animation: glTF skin/animation import (currently rejected), skinning in the PBR pipeline, clips and a state machine with blend trees, events, root motion, and a timeline for cinematics.
- [ ] **M** Tween/curve evaluation for authored motion (translation, rotation, scale, colour, material values) so simple motion stops needing hand-built graph chains.
- [ ] **L** UI and 2D: widget/layout system, canvas and anchors, sprite atlases, 2D animation, tilemaps, nine-slice, localization, accessibility, and authorable menus replacing the engine-drawn game-flow overlays.
- [ ] **L** AI/navigation: navmesh generation and baking, pathfinding, agents with steering, perception, and behavior trees or a state-machine authoring surface.
- [ ] **M** Particles depth: authorable emitters in the inspector, curve modules, GPU simulation, and sorting against transparency (particles exist but are graph-driven).

## 5 — Scale and content pipeline

- [ ] **L** Batching/instancing and LOD. The renderer issues one draw per object with no instancing, LOD or occlusion culling; Sponza's 89 visible surfaces are fine, a real level is not.
- [ ] **M** Texture compression (BC/ASTC) and a GPU memory budget with eviction. `docs/architecture.md` already lists memory budgets and streaming as deliberate omissions.
- [ ] **M** Async and additive scene loading, plus multi-scene editing in the editor.
- [ ] **M** Prefab nesting, variants and source-hierarchy editing (`docs/prefabs.md` lists these as remaining).
- [ ] **M** Material instances/inheritance and shader variants/keywords; the shader graph compiles per object today with a 32-entry source cache.
- [ ] **M** Asset cooking and bundles: imported ONNX-free cooking to a platform format, dependency-driven incremental rebuilds, and downloadable/addressable content packs.
- [ ] **S** More importers: FBX (or a documented conversion path), custom fonts (fonts are a fixed `TextFont` enum), and audio formats once audio exists.
- [ ] **M** Authoring tools for level building: terrain/landscape, foliage scattering, blockout brushes, snapping, measurement and a grid, custom inspectors, and docking.
- [ ] **S** Project ergonomics: template/project wizard, sample projects beyond the demo scenes, and a scene merge helper on top of the diffable JSON.

## 6 — Runtime and shipping

- [ ] **M** In-editor profiler: CPU/GPU frame breakdown, ECS system timings, draw/instance counts, memory, and a log console.
- [ ] **S** Device-loss recovery and a bounded crash report path; unrecoverable device errors terminate the process today.
- [ ] **M** Distribution completion: selected-module builds, certified minimum OS baselines verified on clean machines, dependency notices, signing/notarization, installer decisions (`docs/exporting.md`).
- [ ] **M** Module manifest with dependency ordering, staged registration, and lifecycle cleanup; modules are compiled-in hooks without dependency resolution today (`docs/architecture.md`).
- [ ] **L** Text scripting with hot reload, evaluated *before* native dylib plugins: an embedded VM (Rhai/Lua/WASM) calling the same action layer blueprints use. Native plugins stay behind a versioned C ABI.
- [ ] **S** Ops basics a shipped game expects: save-game format and versioning, localization pipeline, accessibility options, telemetry/crash reporting hooks.
- [ ] **XL** Additional platforms: web (wasm), mobile, consoles. Only after one game ships natively.

## 7 — Networking

Section 1 cleared the blocker: components have per-component load/save hooks, so a component can be
serialized by name, and every component carries a change tick, so replication can ask what moved
since the last acknowledgement instead of diffing the world. The server is a fixed-step loop with no
transport (`apps/server/src/main.rs`).

- [ ] **M** Real-time pacing, overload policy, graceful shutdown and operational diagnostics.
- [ ] **L** Transport, entity replication, authority model, and interest management.
- [ ] **L** Client prediction/interpolation and rollback if the reference game needs it.
- [ ] **M** Multi-client integration tests with scripted loss and latency.
- [ ] **S** `docs/architecture.md` determinism statement: decide and document what replication/replay actually requires, since fixed ticks and serial scheduling do not guarantee cross-platform float determinism.

## Deliberately not doing

- [ ] **Not planned** Archetype or parallel ECS. Current measurements do not justify it; the docs already say to benchmark realistic workloads first.
- [ ] **Not planned** Copying Unity's GameObject/MonoBehaviour shape. Typed validated graphs plus diffable textual scenes are the moat.
- [ ] **Not planned** A derive-macro reflection framework. Hand-written `schema()` per component until it demonstrably hurts.
- [ ] **Not planned** Cascades, AI, networking or platform ports before a reference game needs them.
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
