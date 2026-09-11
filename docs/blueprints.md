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

1. Select an object in Hierarchy. For imported meshes, select the **owner**, not an imported surface (Alt-click also selects the owner).
2. Expand **Properties → BLUEPRINTS**. Choose **+ New**, **+ Spin example**, or **Load…**. The **Blueprint** workspace tab opens its own node-editor pane, separate from the Scene viewport. **View → Blueprint Editor** switches to it too.
3. Use **+ Add node** and search by name. Drag node headers to position them. Select a node and use **Delete node** or Delete to remove it and its wires.
4. Click an output pin, then an input pin (drag/release also works). White pins carry execution; green numbers, red booleans, and blue vectors carry data. Only matching types connect. A new connection replaces that input's existing wire. Right-click an input to disconnect. Escape cancels a pending connection/selection.
5. Edit unconnected input values directly on nodes. **Variables** adds named numbers and their starting values; Get/Set Variable nodes select from those names. A referenced variable cannot be deleted.
6. Middle/right-drag or scroll pans; Ctrl+scroll/pinch zooms; **Fit graph** frames all nodes.
7. Click Play. Switch to Scene for keyboard input, or stay in Blueprint to inspect the latest **Print Number** message. Editing is disabled during Play. Stop to edit; Ctrl/Cmd+Z and Redo use normal scene history.

For rotation, connect **On Update → Rotate** (white), **Delta Seconds → Scale Vector / Factor** (green), and **Scale Vector → Rotate / Value** (blue). Set the vector to `[0, 45, 0]`: the object rotates 45 degrees per second around Y. The Spin example contains this graph.

## Save, reuse, and attach multiple graphs

- **Save graph…** exports the selected graph to a `.blueprint.json` file. Existing files require explicit replacement confirmation; writes use a sibling temporary file and atomic replacement. Scene Undo does not undo file exports.
- **Load copy…**, Properties **Load…**, or dropping a `.blueprint.json` file into the editor attaches an **independent copy** to the selected owner. It does not replace existing attachments. This is deliberately not a live source-file link: subsequent edits to a file or another attachment cannot silently change an object.
- A scene embeds all attachments, node positions, constants, variable defaults, and wires. Normal Save/Open and Save As need no extra blueprint files at runtime. Shared mesh geometry is untouched.
- Add up to 16 graphs per object. Checkboxes enable/disable individual graphs; **↑** changes their execution order; **×** detaches one. All are undoable. Graphs run top to bottom, each with private runtime variables and event state. Object duplication also creates independent state.
- Prefab capture includes each member's attachments. **Apply to prefab** and **Refresh instances** propagate an unchanged attachment list; a locally edited list is retained as one component-level override. Root placement remains independent, as with other prefab components. Attach to a child mesh when that child, rather than the prefab root, should move.
- The checked-in reusable files are `examples/demo/scenes/assets/spin.blueprint.json` and `toggle-visibility.blueprint.json`.

## Nodes and runtime semantics

| Category | Nodes |
|---|---|
| Events | On Start, On Update, On Input Pressed, On Overlap Enter/Exit |
| Inputs | Number, Boolean, Vector, Delta Seconds, Elapsed Seconds, Input Held, Move Axis X/Y |
| Object reads | Get Position, Get Rotation, Get Scale |
| State/flow | Get Variable, Set Variable, Branch, Print Number |
| Number math | Add, Subtract, Multiply, Divide, Sine, Greater Than, Less Than, Equal |
| Boolean math | Not, And, Or |
| Vector math | Make Vector, Scale Vector, Add Vectors |
| Actions | Translate, Rotate, Set Position/Rotation/Scale, Set Color, Set Visible, Set Light Intensity, Move With Collision, Jump |

Actions target the attached **object itself**. Transform values are in parent/model coordinates; rotations are Y-X-Z Euler degrees. Translate and setters are direct transform edits, **not collision-safe movement**. Multiply rates by Delta Seconds for frame-rate-independent motion. **Move With Collision** accepts world-space displacement and requires an enabled box collider; **Jump** uses the existing grounded Gravity behavior. Existing limitations on compound colliders still apply.

Set Color needs a Mesh Renderer and linear RGB in `0..1`. Set Visible affects only the owner's mesh, not descendants, collision, or lights. Set Light Intensity needs a Light and accepts `0..100000`. Scale must remain finite and invertible. Invalid runtime values/missing required components freeze simulation and report an error instead of continuing a broken world. Stop, repair the graph/components, and Play again.

Input uses the engine's existing physical controls: Forward W, Backward S, Left A, Right D, Jump Space. Opposing movement keys cancel at the axis level. Movement press events fire on an inactive→active transition; Space is a queued press edge consumed once, including when no Player Controller exists. Focus loss, dialogs, and switching away from the viewport clear gameplay input. Input Held/Move Axis nodes can drive continuous behavior from On Update. Custom key mapping is not included yet.

Overlap events use the owner's enabled Trigger volume, or Box Collider if it has no Trigger, against other enabled solid box colliders. Enter fires when the volume goes from empty to occupied; Exit when the last overlap ends. These are aggregate events, not per-body notifications. They do not supply an “other actor” reference. Existing collectible/checkpoint/goal trigger behavior remains independent.

On Start runs once at the first simulation tick; On Update runs each tick. Blueprints execute **after existing motion, gravity and gameplay interactions**. Objects use document order, attachments use list order, events use node order, and execution fan-out is queued in wire order. Get Variable/transform reads are evaluated afresh for each action, so later actions see earlier writes. Runtime variables, event state, visibility, and the bounded print log reset on Play; they are not saved-game checkpoints.

## Current boundaries

This is a working first **gameplay** graph system, not Unreal file/API compatibility or an animation/material graph editor. No skeletal animation, blend graphs, arbitrary code nodes, object spawning, cross-object references, custom events/functions, audio, networking, or runtime graph editing is included. Mesh sub-surfaces and shared mesh asset defaults are not attachment targets.

Graphs are versioned, typed, and validated before acceptance. Cycles are rejected (use On Update plus variables); disconnected pins use their editable defaults and disconnected actions do nothing. Limits: 128 nodes, 512 wires, 64 number variables per graph, 16 attachments per object, 1 MiB per imported graph, 100,000 executed actions per scene tick. Enabled blueprint owners and their descendants are excluded from static GI geometry, since graphs can move or recolor them. No runtime filesystem access, dynamic code loading, or new dependencies are needed.
