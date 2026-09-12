# Gameplay Blueprints

Blueprints are an **optional alternative to writing gameplay code**. They run in the same fixed-step simulation as coded components, in editor Play, the native player, and the headless server. They do not replace Rust plugins or existing Spin/Gravity/Player Controller components.

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/blueprint-lab.json
cargo run -p bozzard-player -- --scene examples/demo/scenes/blueprint-lab.json
cargo run -p bozzard-server -- --scene examples/demo/scenes/blueprint-lab.json --ticks 120
```

The **Coral Cube** has a **Bounce Z** blueprint: it oscillates one unit either side of its starting Z position every two seconds, keeping X/Y fixed. Its graph uses Elapsed Seconds → Multiply (π) → Sine → Add (base Z) → Make Vector → Set Position.

The **Hero Cube** has two blueprints: **Spin** rotates it, and **Space toggles visibility** independently hides/shows its mesh. In the editor, press Play and hover the Scene viewport before pressing Space. Stop restores the authored scene.

## Build without code

1. Select an object or imported mesh child in Hierarchy. Components and graphs belong to that entity, not its shared mesh asset.
2. Choose **Properties → Add Component**, search for **Blueprint**, and add it. Expand **BLUEPRINTS** for **+ New**, **+ Spin example**, or **Load…**. The **Blueprint** workspace tab opens its own node-editor pane, separate from the Scene viewport. **View → Blueprint Editor** switches to it too.
3. Use **+ Add node** and search by name. Drag node headers to position them. Select a node and use **Delete node** or Delete to remove it and its wires.
4. Click an output pin, then an input pin (drag/release also works). White pins carry execution; green numbers, red booleans, blue vectors, and purple object references carry data. Only matching types connect. A new connection replaces that input's existing wire. Right-click an input to disconnect. Escape cancels a pending connection/selection.
5. Edit unconnected input values directly on nodes. **Variables** adds named numbers and their starting values; Get/Set Variable nodes select from those names. A referenced variable cannot be deleted.
6. Middle/right-drag or scroll pans; Ctrl+scroll/pinch zooms; **Fit graph** frames all nodes.
7. Click Play. Switch to Scene for keyboard input, or stay in Blueprint to inspect the latest **Print Number** message. Editing is disabled during Play. Stop to edit; Ctrl/Cmd+Z and Redo use normal scene history.

For rotation, connect **On Update → Rotate** (white), **Delta Seconds → Scale Vector / Factor** (green), and **Scale Vector → Rotate / Value** (blue). Set the vector to `[0, 45, 0]`: the object rotates 45 degrees per second around Y. The Spin example contains this graph.

## Save, reuse, and attach multiple graphs

- The Content Browser's **Blueprints** folder lists saved graphs under scene-relative `assets/Blueprints/` (and legacy `assets/*.blueprint.json`) plus scene attachments. Double-click a saved graph to attach a copy; **New Blueprint** adds one to the selected object.
- **Save graph…** exports the selected graph to a `.blueprint.json` file, defaulting to `assets/Blueprints/`. Existing files require explicit replacement confirmation; writes use a sibling temporary file and atomic replacement. Scene Undo does not undo file exports.
- **Load copy…**, Properties **Load…**, or dropping a `.blueprint.json` file into the editor attaches an **independent copy** to the selected owner. It does not replace existing attachments. This is deliberately not a live source-file link: subsequent edits to a file or another attachment cannot silently change an object.
- A scene embeds all attachments, node positions, constants, variable defaults, and wires. Normal Save/Open and Save As need no extra blueprint files at runtime. Shared mesh geometry is untouched.
- Object data pins are typed **Object** pins. The **Object Reference** node supplies an explicitly bound scene object, while **Self** supplies the object that owns the graph. Action nodes with a **Target** pin default to Self, so older graphs keep their behavior after loading. **Same Object** compares two object references; **Is Valid Object** checks whether one currently resolves to an object with a transform.
- Add up to 16 graphs per object. Checkboxes enable/disable individual graphs; **↑** changes their execution order; **×** detaches one. All are undoable. Graphs run top to bottom, each with private runtime variables and event state. Object duplication also creates independent state.
- Prefab capture includes each member's attachments. Explicit Object Reference bindings use persistent document IDs and are remapped when objects are duplicated or copied into prefabs. References to missing objects are rejected; clear or reassign bindings before deleting their target. Prefab sources must be self-contained, so capture/Apply rejects references outside the prefab subtree. A standalone **Load copy…** clears explicit bindings to **None** so they can be reassigned to the intended scene objects; choose their replacements in the Inspector or directly on an unconnected Object pin. **Apply to prefab** and **Refresh instances** propagate an unchanged attachment list; a locally edited list is retained as one component-level override. Root placement remains independent, as with other prefab components. Attach to a child mesh when that child, rather than the prefab root, should move.
- The checked-in reusable files are `examples/demo/scenes/assets/spin.blueprint.json` and `toggle-visibility.blueprint.json`.

## Nodes and runtime semantics

| Category | Nodes |
|---|---|
| Events | On Start, On Update, On Input Pressed, On Overlap Enter/Exit, On Object Enter/Exit |
| Inputs | Number, Boolean, Vector, Delta Seconds, Elapsed Seconds, Input Held, Move Axis X/Y, Mouse Delta X/Y |
| Object reads | Object Reference, Self, Same Object, Is Valid Object, Get Position, Get Rotation, Get Scale, Overlap Count |
| State/flow | Get Variable, Set Variable, Branch, Print Number |
| Number math | Add, Subtract, Multiply, Divide, Sine, Greater Than, Less Than, Equal |
| Boolean math | Not, And, Or |
| Vector math | Make Vector, Scale Vector, Add Vectors |
| Actions | Translate, Rotate, Set Position/Rotation/Scale, Set Color, Set Visible, Set Light Intensity, Move With Collision, Jump, Spawn Prefab, Destroy Prefab |

Actions target their **Target** object, defaulting to the attached object (**Self**). Transform reads also accept a Target. Transform values are in parent/model coordinates; rotations are Y-X-Z Euler degrees. Translate and setters are direct transform edits, **not collision-safe movement**. Multiply rates by Delta Seconds for frame-rate-independent motion. **Move With Collision** accepts world-space displacement and requires an enabled box collider; **Jump** uses the existing grounded Gravity behavior. Existing limitations on compound colliders still apply.

Set Color needs a mesh/Material or Text Rendering and linear RGB in `0..1`; it updates an attached Material when present, otherwise the drawable's base color, and also updates text RGB if attached (preserving its opacity). Set Visible affects the target's mesh and text, not descendants, collision, or lights. Text also follows existing Transform actions; string ports/Set Text are not included in this first pass. See [Text Rendering](text-rendering.md). Set Light Intensity needs a Light and accepts `0..100000`. Scale must remain finite and invertible. Invalid runtime values/missing required components freeze simulation and report an error instead of continuing a broken world. Stop, repair the graph/components, and Play again.

Input uses the engine's existing physical controls: Forward W, Backward S, Left A, Right D, Jump Space. Opposing movement keys cancel at the axis level. Movement press events fire on an inactive→active transition; Space is a queued press edge consumed once, including when no Player Controller exists. Focus loss, dialogs, and switching away from the viewport clear gameplay input. Input Held/Move Axis nodes can drive continuous behavior from On Update. Custom key mapping is not included yet.

**Mouse Delta X/Y** (`mouse_x` / `mouse_y`) expose the existing **right-mouse drag** input, including in Blueprint-only scenes. Values are logical pointer points (positive right/down), not degrees or normalized axes. Deltas accumulate until a simulation tick and are consumed once, even during catch-up ticks; multiple graphs see the same sample. Multiply by your sensitivity, not Delta Seconds. Focus loss and viewport cancellation discard pending motion; the native player also clears it on display-scale changes. The pointer is not locked. [Gold Yard](gold-yard.md) includes a working graph that turns its gold block with these nodes.

Overlap events use the owner's enabled Trigger volume, Box Collider, or Mesh Collider against other enabled colliders. Box/box and box/mesh pairs are supported, plus Rapier contacts involving dynamic mesh bodies (including sleeping contacts); static mesh/mesh pairs are not queried. **On Overlap Enter** and **On Overlap Exit** fire when occupancy changes from empty to occupied or when the last overlap ends. They remain aggregate events and do not supply an “other actor” reference. **On Object Enter** and **On Object Exit** emit once per collider entering or leaving and provide that collider's object through their **Other** Object output. **Other** resolves only along execution from that event; outside that event it is None. **Overlap Count** reports the current number of overlapping colliders. Contacts are snapshotted before graph actions, and per-object events use object-ID order, so mutations affect contact events on the next tick. Choose **Sensor (Blueprints)** for a trigger with no built-in gameplay effects. Existing collectible/checkpoint/goal trigger behavior remains independent.

Actions resolve their Object **Target** at runtime. A **None** target is an error; use **Is Valid Object** to guard an action when a reference may be absent or removed. Explicit references are persistent IDs rather than ECS handles, and are only valid while their target object exists.

On Start runs once at the first simulation tick; On Update runs each tick. Blueprints execute **after existing motion, gravity and gameplay interactions**. Objects use document order, attachments use list order, events use node order, and execution fan-out is queued in wire order. Get Variable/transform reads are evaluated afresh for each action, so later actions see earlier writes. Runtime variables, event state, visibility, and the bounded print log reset on Play; they are not saved-game checkpoints.

## Spawn and destroy prefabs

Import or save a `.prefab.json`, add **Spawn Prefab**, select its asset, and connect an execution input. **Position** sets its root's world position. **Instance** returns its new root Object reference; connect that to **Destroy Prefab → Target**. Destroy accepts any member and removes the entire linked instance, not a shared source asset. Ordinary scene objects and active cameras cannot be destroyed through this node. Use **Is Valid Object** before reusing a potentially destroyed reference.

Spawned members have independent components, remapped internal references, and graph state. Their graphs start on the next tick. Each Spawn node retains its latest result per attachment; it is not an object-array variable. Stop discards runtime spawns and restores the authored scene. Editor Save during Play saves the authored scene, not these spawns.

Editor Play, player, and server load referenced prefab templates and dependencies before simulation, including prefabs referenced by spawned graphs. Missing or invalid files fail startup; ticks never load files. Template limits: 1024 files, 32 MiB combined JSON, 100,000 template objects, with normal scene limits still enforced on spawning.

## Current boundaries

This is a working first **gameplay** graph system, not Unreal file/API compatibility or an animation/material graph editor. No skeletal animation, blend graphs, arbitrary code nodes, custom events/functions, audio, networking, or runtime graph editing is included. Imported surface entities support attachments; shared mesh asset defaults do not.

Graphs are versioned, typed, and validated before acceptance. Cycles are rejected (use On Update plus variables); disconnected pins use their editable defaults and disconnected actions do nothing. Limits: 128 nodes, 512 wires, 64 number variables per graph, 16 attachments per object, 1 MiB per imported graph, 100,000 event/action executions and 1,000,000 overlap tests per scene tick. Enabled blueprint owners and their descendants are excluded from static GI geometry, since graphs can move or recolor them. No runtime filesystem access, dynamic code loading, or new dependencies are needed.

## Pressure plate example

Open `examples/demo/scenes/pressure-plate-lab.json`, press Play, and use WASD to walk the orange player onto either teal plate. Its amber door rises; leaving the plate closes it. The two gates are instances of `assets/pressure-gate.prefab.json`, each bound to its own door. Select either plate to inspect its graph and Target bindings. Stop restores both doors.

The enter event checks that Other is valid before opening the door. The exit event closes it only when Overlap Count is zero, so one departing body cannot close it on another. Duplicate an entire gate root to get another independent pair; duplicating only a plate intentionally preserves its reference to the original door.

Fixed action targets and their descendants are excluded from static GI. A graph writing to an event-dependent target conservatively excludes all scene geometry from the bake, since any collider could become its target.

### Post-processing actions

**Set Exposure (EV)**, **Set Bloom Intensity**, **Set Saturation**, **Set Heat Strength**, **Set Grain Intensity**, and **Set Vignette Intensity** accept execution plus a numeric Value. They override the current global look after volume blending for this Play session. They do not require a target object. Values are validated before the write; stopping Play discards them. See [post-processing animation](post-processing.md#blueprint-animation).
