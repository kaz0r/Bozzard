# Earth Factory Prototype

An editor-playable, isometric 3D voxel-style factory slice for the first Earthlike world. The scene, machines, nodes, and HUD are ordinary Bozzard scene objects and prefabs. Gameplay runs in the attached Rhai script.

From the repository root:

```sh
cargo run -p bozzard-editor-app --bin bozzard-editor -- --scene examples/earth-factory/scenes/earth.json
```

Click the green **▶ Play** button in the editor's top bar. Confirm the bar says **PLAY MODE** and its **■ Stop** button is enabled. The objective panel starts tracking deliveries. Earth nodes, an iron line, a copper line, a coal generator, an assembler, and storage appear automatically. Iron and copper ingots feed the assembler; finished machine parts reach storage. The blue cursor responds to **W/A/S/D**. Press **N** during Play to generate another layout. The authored scene preview does not accept gameplay keys.

The scene also runs in `bozzard-player`. There, **1–8** select machines and **R** rotates as they do in editor Play; **F6** reloads the source scene.

| Key | Action |
| --- | --- |
| W/A/S/D | Move the build cursor across the 15 × 15 grid |
| 1–8 | Select miner, belt, smelter, storage, assembler, generator, splitter, or merger |
| R | Rotate the machine under the cursor; on an empty tile, rotate the next placement |
| Ctrl + R | Smoothly orbit the camera 90° around the factory |
| E | Open the nearest storage on or adjacent to the cursor; press again to close |
| Space | Place the selected machine |
| X | Remove the machine under the cursor |
| N | Generate a new node layout and reset the demonstration factory |

Each camera turn takes 0.55 seconds with an eased start and finish. Additional presses queue
quarter turns; holding the keys does not repeat. After a turn, WASD follows the new viewing
direction. Machine rotation updates both its visible orientation and the direction items leave.
Factory simulation and proximity labels keep updating during camera movement.

Storage has its own **4 × 4 inventory**, with up to **100 items per stack**. Press **E** on a
container or within one tile (including diagonals). The interface fades and slides in over
0.22 seconds, and closes over 0.18 seconds with **E** or **Close**. Movement and construction
are blocked while it is open or closing; production and deliveries continue behind it.

- Drag a stack to an empty slot to move it, onto a matching stack to merge (up to 100), or
  onto another item to swap. Any overflow stays in the source slot. Dropping outside or
  losing window focus cancels the drag without losing items.
- Right-click a populated slot for an animated menu at the pointer, clamped inside the view.
  **Split** moves half to the first empty slot (the original keeps the extra item for odd counts).
  It is disabled for single items or a full grid. **Delete all** removes that item type from
  every slot in this container, leaving other containers untouched.
- Full storage blocks further deliveries. The HUD totals reflect contents across all storages;
  deleting items or removing a container updates those totals. Rerolling the world resets them.

Inventory data, interaction rules, drag previews, and opening/closing animations are Rhai code.
The editor and player forward generic UI pointer events to scripts.

Construction is free in this first prototype. Miners require a node; generators require coal. Smelters turn iron and copper ore into ingots. Assemblers take one of each ingot and output a machine part. Storage collects arriving items, shown in the HUD. The landing pod supplies eight units of power; a generator on coal adds ten. An overloaded factory stops production until power capacity rises, while items already on belts can keep moving. Splitters alternate their output between the two side directions; mergers accept items from any side and send them forward.

The rounded, translucent HUD shows a live **deliver 8 machine parts** objective, stored resources,
power demand/capacity, and a numbered build bar with the selected tool highlighted. This delivery
is a demonstration milestone; tier unlocks are not implemented yet. Labels above nearby nodes and
machines fade in as the blue build cursor approaches (including an adjacent diagonal tile), then
fade out as it leaves. Miners show their resource, such as **Miner: Quartz**.

HUD text and hints use the engine's bundled Roboto font. Text is antialiased at its displayed
pixel size, including display scaling, to keep smaller UI text legible when resizing the window.

Items retain their visual object while traveling. The Rhai script interpolates their positions
every update between the factory's 0.32-second simulation steps, including arrivals at the
assembler and storage. Blocked belts still stop naturally. Gameplay, proximity detection,
animation, and HUD updates are all in Rhai; the engine exposes reusable widget controls.

Production iterates occupied machine cells and reuses delivered item visuals. Nearby labels
inspect the surrounding tiles, and inventory contents refresh only when their data or selection
changes; opening, closing, and item motion still animate every update. The editor shares one
final game UI layout between accessibility and drawing during Play.

To compare CPU costs for simulation, UI layout, pointer input, scene extraction, and UI drawing
with storage closed and open (timings depend on the machine and build profile):

```sh
cargo test -p bozzard-editor --test earth_factory profile_earth_factory_cpu -- --ignored --nocapture
```

The graphics benchmark below renders the running factory at 1280 × 800 on the available GPU.
It reports renderer CPU preparation, encoding, submission, and shadow draw commands after
warmup; these timings exclude simulation and do not measure windowed FPS.

```sh
cargo test -p bozzard-editor --test earth_factory profile_earth_factory_render -- --ignored --nocapture
```

Rendering checkpoint (Intel Iris Xe / Vulkan, debug build, seed 4, 12 warmup + 60 measured
frames at 1280 × 800): shadow instancing and CPU uniform reuse reduced median renderer CPU
time from **8.18 ms to 3.27 ms**, preparation from **4.49 ms to 1.43 ms**, and shadow draw
commands from **122 to 10**, while retaining 2,556 shadow triangles. These are local component
measurements, not whole-editor FPS. GPU tests compare the optimized path with uncached,
individual draws, including offscreen casters, local lights, material edits, and temporal effects.

The day-and-night cue currently changes exposure while production continues. A moving sun, construction costs, persistence between Play sessions, tier deliveries, and the spaceship are later prototype steps. A fresh Play session gets a new seed; **N** gets another. The sample factory moves and rotates with its iron, copper, and coal nodes, and the other deposits scatter around it. Every layout guarantees the seven Earth resources. Set the scene's `seed` blackboard number above zero to reproduce a particular layout.

The fixed 15 × 15 voxel ground and its outer cliff are baked into one OBJ mesh with five material shades. Resource deposits and machines remain separate Rhai-spawned prefabs, so the ground mesh does not constrain factory placement or resource rerolls. The source of truth for scene geometry and prefabs is [`tools/generate_scene.py`](tools/generate_scene.py); rerun it after modifying that generator. Gameplay code is [`scenes/scripts/earth_factory.rs`](scenes/scripts/earth_factory.rs). You can also edit and save the generated scene and prefabs directly in Bozzard. Regenerating files will replace those manual edits.

Node and machine prefabs use an unscaled pivot with separate base and upright detail blocks. Their pivot sits on the ground surface, so their intended height is visible in editor Play and the player.
