# Gameplay Blueprints

Blueprints are the engine's no-code gameplay authoring path, and [gameplay scripts](scripting.md) are the coding one: both drive the same engine actions and share the same object and scene blackboards, so a scene may mix them. Blueprints are: typed, validated graphs run unchanged in editor Play, the native player, and the headless server. The six pin types are **Exec, Text, Number, Bool, Vector, and Object**. Shared blackboards and bounded typed lists let multiple graphs cooperate. Existing scenes with legacy components and private number variables remain compatible. See [authoring depth, scene control, and checkpoint semantics](blueprint-depth.md).

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
3. Use **+ Add node** for an alphabetical list, and type in **Search nodes…** to filter by name. Clicking the search field keeps the menu open; selecting a node, clicking outside, or pressing Escape closes it. Drag node headers to position them. Select a node and use **Delete node** or Delete to remove it and its wires.
4. Click an output pin, then an input pin (drag/release also works). White pins carry execution; green numbers, red booleans, blue vectors, and purple object references carry data. Only matching types connect. A new connection replaces that input's existing wire. Right-click an input to disconnect. Escape cancels a pending connection/selection.
5. Edit unconnected input values directly on nodes. **Blackboards** adds typed scalars or bounded lists to Graph, Object, or Scene scope. Select a Get/Set Variable or List node, choose its scope and declaration above the canvas. **Variables** retains legacy private number defaults. Referenced declarations cannot be removed from an accepted scene.
6. Middle/right-drag or scroll pans; Ctrl+scroll/pinch zooms; **Fit graph** frames all nodes.
7. Click Play. Switch to Scene for keyboard input, or stay in Blueprint to inspect the latest **Print Number** message. Editing is disabled during Play. Stop to edit; Ctrl/Cmd+Z and Redo use normal scene history.

For rotation, connect **On Update → Rotate** (white), **Delta Seconds → Scale Vector / Factor** (green), and **Scale Vector → Rotate / Value** (blue). Set the vector to `[0, 45, 0]`: the object rotates 45 degrees per second around Y. The Spin example contains this graph.

The same game is also checked in as scripts — `examples/demo/scenes/target-range-rs.json`, every
rule in `scenes/scripts/target-range/*.rs` and no graph of its own — so the two authoring paths can
be compared rule for rule. See [gameplay scripts](scripting.md).

## Save, reuse, and attach multiple graphs

- The Content Browser's **Blueprints** folder lists saved graphs under scene-relative `assets/Blueprints/` (and legacy `assets/*.blueprint.json`) plus scene attachments. Double-click a saved graph to attach a copy; **New Blueprint** adds one to the selected object.
- **Save graph…** exports the selected graph to a `.blueprint.json` file, defaulting to `assets/Blueprints/`. Existing files require explicit replacement confirmation; writes use a sibling temporary file and atomic replacement. Scene Undo does not undo file exports.
- **Load copy…**, Properties **Load…**, or dropping a `.blueprint.json` file into the editor attaches an **independent copy** to the selected owner. It does not replace existing attachments. This is deliberately not a live source-file link: subsequent edits to a file or another attachment cannot silently change an object.
- A scene embeds all attachments, node positions, constants, variable defaults, and wires. Normal Save/Open and Save As need no extra blueprint files at runtime. Shared mesh geometry is untouched.
- Object data pins are typed **Object** pins. The **Object Reference** node supplies an explicitly bound scene object, while **Self** supplies the object that owns the graph. Action nodes with a **Target** pin default to Self, so older graphs keep their behavior after loading. **Same Object** compares two object references; **Is Valid Object** checks whether one currently resolves to an object with a transform.
- Add up to 16 graphs per object. Checkboxes enable/disable individual graphs; **↑** changes their execution order; **×** detaches one. All are undoable. Graphs run top to bottom, with private event/timer state and optional Graph, Object, or Scene variable scope. Object duplication creates a new object blackboard and remaps Object values in it.
- Prefab capture includes each member's attachments. Explicit Object Reference bindings use persistent document IDs and are remapped when objects are duplicated or copied into prefabs. References to missing objects are rejected; clear or reassign bindings before deleting their target. Prefab sources must be self-contained, so capture/Apply rejects references outside the prefab subtree. A standalone **Load copy…** clears explicit bindings to **None** so they can be reassigned to the intended scene objects; choose their replacements in the Inspector or directly on an unconnected Object pin. **Apply to prefab** and **Refresh instances** propagate an unchanged attachment list; a locally edited list is retained as one component-level override. Root placement remains independent, as with other prefab components. Attach to a child mesh when that child, rather than the prefab root, should move.
- The checked-in reusable files are `examples/demo/scenes/assets/spin.blueprint.json` and `toggle-visibility.blueprint.json`.

## Nodes and runtime semantics

Every node kind is one row of the table in `crates/bozzard-scene/src/blueprint.rs`. That row is the
single declaration of a node: it generates the enum (whose snake_case names are the saved wire format),
the default pin lists, the alphabetically sorted add-node menu, and the `event`/`action` classification
the runtime uses to start chains. `Node::input_pins` and `Node::output_pins` resolve the declared type
for variable, list and reroute pins. Adding a node is one row plus its runtime evaluation arm and any
type-specific behavior. A test guards that the table stays complete, ordered, uniquely titled and
round-trips every name through serde.

| Category | Nodes |
|---|---|
| Events | On Start/Update/Enable/Disable/Destroy, On Input Pressed, On Overlap Enter/Exit, On Object Enter/Exit, On Collision Enter (Other, Normal, Impulse) |
| Inputs | Number, Boolean, Vector, Delta Seconds, Elapsed Seconds, Input Held, Move Axis X/Y, Mouse Delta X/Y |
| Object reads | Object Reference, Self, Same Object, Is Valid Object, Is Rigidbody, Get Position, Get Rotation, Get Scale, Overlap Count |
| State/flow | Get/Set Variable (Graph/Object/Scene), List Get/Push/Set/Remove/Clear/Length, Branch, Delay / After, Set Graph Enabled, Print Number, typed Reroute, Comment |
| Number math | Add/Subtract/Multiply/Divide, Lerp, Min/Max, Abs, Modulo, Power, seeded Random, Sine/Cosine/Tangent, Arc Sine/Arc Cosine/Atan2, Degrees↔Radians, Floor/Ceil/Round/Sqrt, Clamp, Greater/Less/Equal |
| Boolean math | Not, And, Or |
| Vector math | Make/Break Vector, Scale/Add/Lerp Vectors, Forward Vector, Length, Normalize, Dot, Cross, Distance |
| Queries | Raycast, Sphere Overlap, Box Overlap, Line of Sight |
| Scene/state | Load Scene, Load Scene Additively, Restart Scene, Save Game State, Load Game State |
| Actions | Translate, Rotate, Set Position/Rotation/Scale, Set Color, Set Visible, Set Text, Set Light Intensity, Move With Collision (Grounded output), Jump, Set Velocity, Lock Cursor, Unlock Cursor, Spawn Prefab, Destroy Prefab |

Actions target their **Target** object, defaulting to the attached object (**Self**). Transform reads also accept a Target. Transform values are in parent/model coordinates; rotations are Y-X-Z Euler degrees. Translate and setters are direct transform edits, **not collision-safe movement**. Multiply rates by Delta Seconds for frame-rate-independent motion. **Move With Collision** accepts world-space displacement and requires an enabled box collider; **Jump** uses the existing grounded Gravity behavior. Existing limitations on compound colliders still apply.

Set Color needs a mesh/Material or Text Rendering and linear RGB in `0..1`; it updates an attached Material when present, otherwise the drawable's base color, and also updates text RGB if attached (preserving its opacity). Set Visible affects the target's mesh and text, not descendants, collision, or lights. Text also follows existing Transform actions; Text, Number to Text (0–6 decimals), Join Text, Get Text, and Set Text support bounded dynamic labels. Text pins accept at most 4096 UTF-8 bytes; blackboard variables can hold any data pin type. See [Text Rendering](text-rendering.md). Set Light Intensity needs a Light and accepts `0..100000`. Scale must remain finite and invertible. Invalid runtime values/missing required components freeze simulation and report an error instead of continuing a broken world. Stop, repair the graph/components, and Play again.

**On Input Pressed** and **Input Held** each watch one button, and the scene picks it: the node's dropdown lists the seven aliases and every assignable key (`A`–`Z`, `0`–`9`, `Space`, `Enter`, `Escape`, `Tab`, `Backspace`, `Delete`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`, `Shift`, `Ctrl`, `Alt`, `ArrowUp`/`ArrowDown`/`ArrowLeft`/`ArrowRight`, `F1`–`F12`, `MouseLeft`, `MouseRight`, `MouseMiddle`). Typing the name in the scene file works too, and `KeyF`/`Digit1`/`Num1` spellings are accepted and normalized. An unbound name is a load error, so a typo cannot become a node that never fires.

The aliases are the engine's original fixed bindings and read its axes instead: `forward`/`backward`/`left`/`right` are W/S/A/D, `jump` is Space, `fire` is the left mouse button and `interact` is E. **Opposing movement keys cancel at the axis level**, which is why the aliases are not simply the key names they default to. Assigned keys are physical positions, so a non-QWERTY layout still presses the button the scene asked for, and they are tracked as a level: `On Input Pressed` fires on the inactive→active transition, while `Input Held` stays true for as long as the button is down. Jump, fire and interact stay queued press edges consumed once per tick, including when no Player Controller exists. `Move Axis` keeps reading the movement axes.

Focus loss, dialogs, and switching away from the viewport clear gameplay input, including every assigned key. One name binds exactly one button; an alias never doubles as a key name. The apps keep a few buttons for themselves: Escape stops editor Play and closes the standalone player, F5 saves, and the standalone player also uses Enter, R and Q for its game menus. A scene binding one of those sees the app act instead.

**Mouse Delta X/Y** (`mouse_x` / `mouse_y`) expose the app's look control, including in Blueprint-only scenes: the native player locks and hides the pointer while it owns gameplay, so plain mouse motion turns the view with no button held, and falls back to a confined pointer, then to plain pointer motion, when a platform refuses the grab. The editor keeps **right-drag** because eframe cannot lock the cursor. Values are logical pointer points (positive right/down), not degrees or normalized axes. Deltas accumulate until a simulation tick and are consumed once, even during catch-up ticks; multiple graphs see the same sample. Multiply by your sensitivity, not Delta Seconds. Focus loss, menus and viewport cancellation discard pending motion; the native player also clears it on display-scale changes. [Gold Yard](gold-yard.md) includes a working graph that turns its gold block with these nodes.

Overlap events use the owner's enabled Trigger volume, Box Collider, or Mesh Collider against other enabled colliders. Box/box and box/mesh pairs are supported, plus Rapier contacts involving dynamic mesh bodies (including sleeping contacts); static mesh/mesh pairs are not queried. **On Overlap Enter** and **On Overlap Exit** fire when occupancy changes from empty to occupied or when the last overlap ends. They remain aggregate events and do not supply an “other actor” reference. **On Object Enter** and **On Object Exit** emit once per collider entering or leaving and provide that collider's object through their **Other** Object output. **Other** resolves only along execution from that event; outside that event it is None. **Overlap Count** reports the current number of overlapping colliders. Contacts are snapshotted before graph actions, and per-object events use object-ID order, so mutations affect contact events on the next tick. Choose **Sensor (Blueprints)** for a trigger with no built-in gameplay effects. Existing collectible/checkpoint/goal trigger behavior remains independent.

Actions resolve their Object **Target** at runtime. A **None** target is an error; use **Is Valid Object** to guard an action when a reference may be absent or removed. Explicit references are persistent IDs rather than ECS handles, and are only valid while their target object exists.

On Start runs once at the first simulation tick; On Update runs each tick. Blueprints execute **after existing motion, gravity and gameplay interactions**. Objects use document order, attachments use list order, events use node order, and execution fan-out is queued in wire order. Get Variable/transform reads are evaluated afresh for each action, so later actions see earlier writes. Runtime variables, event state, visibility, and the bounded print log reset on Play. Save/Load Game State provides explicit checkpoints that also preserve timers and physics velocities.

## Spawn and destroy prefabs

Import or save a `.prefab.json`, add **Spawn Prefab**, select its asset, and connect an execution input. **Position** sets its root's world position. **Instance** returns its new root Object reference; connect that to **Destroy Prefab → Target**. Destroy accepts any member and removes the entire linked instance, not a shared source asset. Ordinary scene objects and active cameras cannot be destroyed through this node. Use **Is Valid Object** before reusing a potentially destroyed reference.

Spawned members have independent components, remapped internal references, and graph state. Their graphs start on the next tick. Each Spawn node retains its latest result per attachment; it is not an object-array variable. Stop discards runtime spawns and restores the authored scene. Editor Save during Play saves the authored scene, not these spawns.

Editor Play, player, and server load referenced prefab templates and dependencies before simulation, including prefabs referenced by spawned graphs. Missing or invalid files fail startup; ticks never load files. Template limits: 1024 files, 32 MiB combined JSON, 100,000 template objects, with normal scene limits still enforced on spawning.

## Current boundaries

Blueprints remain the gameplay authoring path. [Middleware nodes](middleware.md) control audio, skeletal clips/blend trees, timelines/tweens, navigation, UI and sprite animation through typed object references. Those components have dedicated inspector editors; shader graphs author materials. Arbitrary code nodes, custom functions, networking and runtime graph editing remain outside the current graph system. Imported surface entities support attachments; shared mesh asset defaults do not.

Graphs are versioned, typed, and validated before acceptance. Cycles are rejected (use On Update plus variables); disconnected pins use their editable defaults and disconnected actions do nothing. Limits: 128 nodes, 512 wires, 64 declarations per blackboard (legacy number variables count toward the graph limit), 16 attachments per object, 1 MiB per imported graph, 100,000 event/action executions and 1,000,000 overlap tests per scene tick. Enabled blueprint owners and their descendants are excluded from static GI geometry, since graphs can move or recolor them. Scene templates and rendering assets are preloaded. Explicit Save/Load Game State performs bounded checkpoint I/O at a tick boundary; ordinary graph evaluation does not read assets or load code.

## Pressure plate example

Open `examples/demo/scenes/pressure-plate-lab.json`, press Play, and use WASD to walk the orange player onto either teal plate. Its amber door rises; leaving the plate closes it. The two gates are instances of `assets/pressure-gate.prefab.json`, each bound to its own door. Select either plate to inspect its graph and Target bindings. Stop restores both doors.

The enter event checks that Other is valid before opening the door. The exit event closes it only when Overlap Count is zero, so one departing body cannot close it on another. Duplicate an entire gate root to get another independent pair; duplicating only a plate intentionally preserves its reference to the original door.

Fixed action targets and their descendants are excluded from static GI. A graph writing to an event-dependent target conservatively excludes all scene geometry from the bake, since any collider could become its target.

### Post-processing actions

**Forward Vector** returns the target's world-space facing (its local -Z axis, the engine's forward), so aiming a follow camera's yaw and pitch is one node. **Break Vector** splits a vector into separate X/Y/Z numbers. **Clamp** bounds one number between Min and Max. **Set Velocity** writes a rigidbody's linear velocity in world units per second, capped at 1000; it needs an enabled Gravity body that is not a Player Controller, and a body spawned by Spawn Prefab in the same tick receives the velocity when physics builds it on the next tick, so "spawn then launch" is a single graph. **Is Rigidbody** is true for exactly those bodies, which is how a graph tells a projectile apart from the floor, the player or another static object inside On Object Enter.

**Move With Collision** also returns **Grounded**: true when that move ended on a floor-facing contact (normal within 60° of up). It is the last result for that node, so an On Input Pressed chain can gate a jump on the grounding found by the On Update chain. Together with a velocity variable this is enough for a complete character in graphs: add `gravity * Delta Seconds` to a vertical speed, move by `horizontal input * speed * Delta Seconds` plus `vertical speed * Delta Seconds`, zero the speed when Grounded, and write the camera from Get Position plus an eye offset. Objects driven this way need a Box Collider but no Gravity component, so they are resolved by Move With Collision instead of Rapier, are never pushed by other bodies, and stay out of Is Rigidbody. The bundled Target Range scene is exactly that controller.

**Lock Cursor** and **Unlock Cursor** ask the hosting app to capture or release the pointer; they need no target. The request is a mode, not an edge: it stays until another graph changes it, and it is reset when the scene restarts. `None` (no node ever ran) keeps each app's own policy, which is to capture while it owns gameplay. A request for capture still loses to focus loss, pause and Game Flow menus, which always release the pointer; a request for release is honoured during play, for scenes that want a visible cursor. Only the native player can truly capture (it locks and hides the pointer, falling back to a confined pointer and then to plain pointer motion); the editor cannot lock the cursor, so a capturing scene gets no-button look while the viewport is hovered.

**Set Exposure (EV)**, **Set Bloom Intensity**, **Set Saturation**, **Set Heat Strength**, **Set Grain Intensity**, and **Set Vignette Intensity** accept execution plus a numeric Value. They override the current global look after volume blending for this Play session. They do not require a target object. Values are validated before the write; stopping Play discards them. See [post-processing animation](post-processing.md#blueprint-animation).

`End Game` is a terminal execution action with an optional Text message (240 UTF-8 bytes). Enable [Game Flow](game-flow.md) in scene settings to show its retry menu. Remaining graph actions stop when the run ends.

## Target Range example

Open `examples/demo/scenes/target-range.json` and press Play, or run:

```sh
cargo run -p bozzard-player -- --scene examples/demo/scenes/target-range.json
```

WASD walks, Space jumps, the mouse looks around (no button held), and left-click fires at the centered crosshair. A table on the right holds three weapons: walk up to one and press **E** to take it. Four colored cubes stand on the platform; a projectile launched into one makes it vanish, and clearing all four wins.

The player has **no Player Controller component and no Gravity**: movement, gravity, jumping, looking, the camera, the weapons and the shot are all graphs, using the pattern described above. The scene's camera is a plain object whose transform the graphs write every tick.

- **Player / look, move and jump**, **Player / shoot and recoil**, and **Player / weapon selection** share an object blackboard containing `yaw`, `pitch`, `vy`, `kick`, `kick_amount`, `speed` and `size`. Mouse Delta X/Y feed yaw and pitch (pitch through **Clamp**), the camera gets the fresh rotation plus a `+0.5` eye offset, gravity accumulates into `vy`, camera-relative movement comes from **Forward Vector** with its Y component zeroed through **Break Vector**/**Make Vector**, and one **Move With Collision** resolves the whole step. Its **Grounded** output zeroes the fall speed on landing and gates the jump in its On Input Pressed chain. The shooting graph's On Input Pressed (Fire) chain bumps the recoil, spawns the projectile from the eye and launches it along the camera's forward vector, so shots land at the crosshair, and the weapon selection graph's On Input Pressed (E) chain is the weapon switch below.
- **Player controller / respawn below the world** puts the player back above the start when it falls past the platform edge. The respawn graph needs no variables; the landed-grounding branch recovers the shared fall speed.
- **First person / hide own body, lock cursor** hides the rendered mesh and runs **Lock Cursor** on start; the win graph runs **Unlock Cursor** just before **End Game**, so the pointer comes back with the retry menu.

The former 97-node player graph is now three attachments (50, 16, and 33 nodes). Weapon selection writes `speed`, `size`, and `kick_amount` in Object scope; shooting and camera movement read the same declarations. Pure calculations needed by multiple event chains are copied, while the mutable values are shared. The existing movement, aiming, weapon-switch, recoil, and win regressions cover this split.

### The weapon table

The `weapon-table` object is a solid slab, so the player walks into it and stops at a known spot. The three `weapon-*` cubes on top have no collider: they are props, and the pickups are position tests, not overlaps. Pressing **E** compares the player's own **Position** (split by **Break Vector**) against one shared x window and a per-weapon z window, and the matching **Branch** writes that weapon's profile:

| Weapon | Launch speed | Bullet size | Recoil kick |
| --- | --- | --- | --- |
| AR | 45 | 0.16 | 0.8° |
| Pistol | 22 | 0.22 | 1.6° |
| Shotgun | 60 | 0.34 | 3.4° |

**Set Scale** sizes the projectile instance and **Set Velocity** launches it, both aimed through the **Forward Vector** of the camera rather than the instance's own rotation. **Set Text** writes the equipped weapon into the `hud-weapon` screen text, so the label always says what the shot will do.

Recoil is a `kick` variable rather than a permanent aim change: firing adds `kick_amount` to it, the camera renders `clamp(pitch) + kick`, and the move chain multiplies `kick` by `0.82` every tick, so the view rises on the shot and settles back on target. All three weapons fire one projectile per click; nothing here is automatic or spread yet.

A screen-anchored Text Rendering object holding `+` draws the crosshair, the same HUD mechanism as `hud-lab.json`: no depth, no camera transform, anchored at `[0.5, 0.5]`.

The projectile prefab carries its own graphs: On Object Enter destroys the instance on any impact, and On Update retires it once it falls below the platform or leaves the arena (the original squared-radius test is retained; Length and Distance are now available). That last part is not just tidiness: the sun shadow map is fitted to every lit draw, so a round that flew on into the void for 300 units would drag the fitted box and its texels along with it.

Each cube owns a **pop when hit by a physical body** graph: On Object Enter supplies **Other**, Is Rigidbody rejects the platform and the player, and a Branch hides the cube and moves it out of play. A non-rendered `game-rules` object polls the four cube positions in one On Update graph and fires **End Game** with "You win! All four targets destroyed." once every cube has dropped below the platform. Game Flow shows the controls before the run and the win message afterwards.
