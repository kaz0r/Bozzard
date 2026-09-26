# Earth Factory Prototype

An editor-playable, isometric 3D voxel-style factory slice for the first Earthlike world. The scene, machines, nodes, and HUD are ordinary Bozzard scene objects and prefabs. Gameplay runs in the attached Rhai script.

From the repository root:

```sh
cargo run -p bozzard-editor-app --bin bozzard-editor -- --scene examples/earth-factory/scenes/earth.json
```

Click the green **▶ Play** button. A fresh game begins beside the landing pod with hand tools and a workbench. Walk onto iron or copper nodes and hold **F** to gather. Open **J**, then deliver four iron ore and four copper ore on page I to unlock smelting. The same scene and controls work in the native player.

A small debug HUD beneath the objective shows FPS, frame time, CPU draw time, loaded and explored chunks, simulating chunks, visible entities, draw calls, and submitted triangles. It refreshes four times per second in editor Play and the native player. FPS uses completed-frame wall time; CPU draw time measures renderer preparation and submission, not GPU execution. Visible entities exclude the HUD and count objects inside the camera frustum before GPU occlusion. Draws and triangles cover color-pass meshes. Headless runs show `--` for unavailable rendering measurements.

Loaded chunks have live terrain, deposits, and machine models. Explored chunks include distant
regions whose models have been unloaded, up to 289 regions. Draws is a per-frame count,
so it can rise when more neighboring ground is visible and fall when it leaves the view.
CPU draw excludes simulation, UI layout, scene extraction, and presentation waits; a low draw
time alone does not guarantee high FPS. Static transforms are reused, HUD labels update on
state changes, and UI layout queries UI components independently of retained scenery.
Systems without matching components avoid traversing scenery, and silent scenes skip audio
transform extraction. Hidden, fully closed storage and journal panels also skip updates.

**Simulation threading.** Native player and editor Play use a dedicated simulation worker
while the main thread submits a prepared frame. The HUD's `Sim worker` line shows the last
simulation batch's CPU time and the main thread's remaining `Wait` after rendering. Add
`--single-threaded` to either launch command for comparison; the line then reads `Sim main`.
Both modes use the same prepared-frame ordering. This can add one frame of visual input
latency compared with rendering immediately after each tick. A slow simulation batch can
still delay the next frame: this first stage overlaps simulation and rendering but does not
yet run individual chunks on multiple cores. Headless tests and debugger single-step keep
their synchronous behavior; the factory menu still leaves production running.

A local release comparison on Intel Iris Xe/Vulkan (seed 4, demonstration factory,
35 explored regions, zoom 32, 1024 × 640, 240 presented frames per mode) measured median
CPU frame-path wall time of **10.45 ms serial / 8.38 ms threaded**. Median presentation
interval was **19.22 / 17.09 ms**, with p95 **36.13 / 33.13 ms**. GPU-pass medians stayed
near 4.1 ms. This is one local comparison, including startup samples, not a guaranteed
speedup or a sustained-60-FPS claim. Whole-frame timings include waits; the HUD's simulation
CPU and Wait fields isolate the completed update and its remaining join cost.

The concurrency test verifies actual overlap and worker reuse. A paired input route checks
identical factory state, menu handling, chunk travel, skipped draws and Stop/restart; a native
GPU test compares exact world pixels across the two modes:

```sh
cargo test -p bozzard-editor --test threaded_render -- --ignored --nocapture
```

| Key | Action |
| --- | --- |
| W/A/S/D | Move the build cursor; approaching an edge reveals the next region |
| Ctrl + 1 / 2 / 3 | Select the Production / Logistics / Power action bar |
| 1–8 | Select a slot on the current bar; each bar remembers its selection |
| R | Lift, rotate, and lower the machine under the cursor; on empty ground, turn the next placement |
| Ctrl + R | Smoothly orbit the camera by 90° |
| F (hold) | Hand-gather a solid resource beneath the cursor |
| J | Open or close the journal |
| M | Open or close the region map |
| Escape | Open the factory menu, or Continue when it is open |
| Mouse wheel | Smoothly zoom in or out over the world |
| Left / Right, or journal tabs | Turn journal pages while the book is open |
| E | Collect nearby machine output, or open/close storage |
| Space | Build the selected unlocked machine, paying its material cost |
| X | Demolish a machine and discard its contents |
| N | Reset the world, discoveries, backpack, and progression with a new seed |
| F6 (player) | Reload the source scene |

**Action bars.** Production contains smelters, miners, and assemblers; Logistics contains belts, storage, splitters, and mergers; Power contains generators. Locked slots stay visible and explain their requirements through the journal. Ctrl-number chords switch bars without also selecting a slot.

**Machine rotation.** A turn takes 0.6 seconds: lift, quarter-turn, then lower. Repeated presses queue up to eight turns on each machine, while holding R does not repeat. The machine's production and item transfers wait until it lands, when its output direction changes. Other machines continue running. Removing an animating machine clears its pending turns, ingredients, and progress. Camera turns remain separate and take 0.55 seconds.

**Exploration.** Regions contain 15 × 15 cells. The planet extends eight regions north, south, east, and west of the landing region: a 17 × 17 grid, including diagonals, for up to 289 regions. Neighboring ground and deposits appear before crossing an edge. Their layouts depend on the planet seed and region coordinates, so discovery order does not reroll a location. Every region contains all seven Earth resources. Revisited regions retain machine placement, facing, buffered items, production progress, and storage contents.

Only the currently occupied region simulates production in this version. Distant chunk models unload;
compressed archives retain deposits, machine placement and facing, buffered items, production
progress, and storage inventory. Approaching again restores the models before they enter view.
The loaded neighborhood includes a margin for tall machines and shadows and expands for zooming
out or a wider viewport; camera orbit is covered throughout its animation. Unloading does not
reroll deposits. Conveyors do not transfer across region boundaries yet. Region state lasts for
the current Play session, not across closing the game. **N** deliberately clears it. Chunks share
the same ground mesh, and the small authored landing-pod scene remains resident.

Crossing a seam eases the camera to the next region over 0.55 seconds. Reversing direction
retargets from its current position; orbit and zoom can continue during the pan. Residency
covers both the moving view and its destination, and surrounding loads/removals drain one
region per tick. Chunk switches archive occupied machine/storage cells and reuse item models.
The engine batches consecutive scripted prefab spawns and callback-free removals into single
scene validations, while preserving lifecycle callbacks for scripted machines.

To measure the simulation cost of crossing, discovery, and streaming separately in release mode:

```sh
cargo test --release -p bozzard-demo --test earth_factory profile_chunk_transitions -- --ignored --exact --nocapture
```

**Map.** Press **M** or click **Map** to see the full 17 × 17 planet grid. Green cells are currently loaded; blue-gray cells were explored but have unloaded; dark cells are unexplored. Orange marks your current region, and **H** marks the landing site. Counts match the debug HUD. North stays at the top when the camera rotates. The map blocks movement, building, collection, and camera zoom while factories continue running. **M** or **Close** returns to play; **Escape** opens the factory menu. Opening the map closes the journal or storage.

**Journal.** The book fades in with three pages:

- **I — Unlocks:** available and locked equipment, the next delivery, backpack counts, and a delivery button.
- **II — Recipes:** unlocked recipes, exact ingredients, crafting order, the selected machine's build cost, and crafting buttons. Hand-crafting requires standing on or adjacent to the landing pod in region 0,0. Machines can process the same ingot recipes automatically.
- **III — Spaceship:** Tier 1 progress and the hull, flight systems, launchpad, and fuel milestones. Those systems remain locked, so launch readiness currently starts at 0/4; later tiers and spaceship construction are not implemented.

Movement, construction, gathering, and action-bar changes pause while the journal or storage is open or closing. Factory production continues.

**Factory menu.** Escape opens a menu with Continue, Save, Load, and Exit. Continue or Escape returns to play. Factories, conveyors, animations, and diagnostics continue running behind the menu, while gameplay input is blocked. Save and Load are disabled placeholders; no save/load logic is connected. Exit closes the standalone player or stops editor Play without saving the session.

**Zoom.** Scroll up to zoom in and down to zoom out. The camera eases between a vertical view span of 9–32 world units (starting at 19), keeping its isometric angle. HUD panels, storage, the journal, and the factory menu consume scrolling. New worlds reset zoom; orbiting and chunk travel preserve it.

**Tier 1.** Deliveries consume backpack materials. They unlock equipment in this order:

| Delivery | Required materials | Unlock |
| --- | --- | --- |
| 1 | 4 iron ore + 4 copper ore | Smelter and both ingot recipes |
| 2 | 4 iron ingots + 4 copper ingots | Coal generator |
| 3 | 8 iron ingots + 4 copper ingots | Miner |
| 4 | 12 iron ingots + 8 copper ingots | Belts and storage |

One ore makes one matching ingot. Assemblers, splitters, mergers, and machine-part crafting remain locked for the later automation tier. A smelter costs four iron ore, allowing the first processing machine to be built before the player has ingots. Miners and generators cost four iron ingots and two copper ingots; belts cost one iron ingot; storage costs four iron ingots. The landing pod provides eight power, and a generator on coal adds ten. Overload stops production while items already on belts can continue traveling.

**Machine buffers.** Miners, smelters, assemblers, conveyors, splitters, and mergers hold at most **100 items total per machine**, counting ingredients and finished output together. Smelters keep separate ore and ingot stacks within that shared limit; an output stack must empty before switching to the other ingot type. Assemblers reserve room for a missing ingredient so a single feed cannot fill all 100 spaces. With two ingredients per part, an assembler may need its output collected before it can fit another complete recipe. Full machines block incoming transfers. Buffers survive region changes; demolition and **N** clear them.

Buffering does not speed up production or transfers: each connection still moves at most one item per factory beat. Quantities share one representative item model per occupied output, plus transient transfer visuals, so filling a buffer does not spawn 100 entities. The coal-node generator retains its existing power behavior without a fuel inventory.

**Machine collection.** Press **E** on or within one tile of a miner, smelter, or assembler (including diagonals) to take **all finished output** into your backpack. The nearest machine/container wins, and the tooltip shows the target. The tooltip shows total buffer usage out of 100 and the collectible quantity. Unfinished ingredients and recipe progress stay inside; collection frees capacity for further production. Check the backpack in **J**.

**Storage.** Each container has 16 slots with 100 items per stack. Press **E** within one tile, including diagonals. Drag stacks to move, merge, or swap them. Right-click for Split or Delete all; Delete all affects only that container. **Take items into backpack** transfers contents for crafting and deliveries. Full storage blocks incoming items. Demolition discards its inventory and updates stored totals.

For the original prebuilt production demonstration and profiling fixtures, set the scene blackboard's **`demo_mode` to true** before Play. This unlocks all existing machines, makes construction free, and restores the iron/copper/assembler sample. Normal play defaults to the new Tier 1 start. Set **`seed` above zero** for a reproducible planet.

To compare CPU costs for simulation, UI layout, pointer input, scene extraction, and UI drawing
with storage closed and open (timings depend on the machine and build profile):

```sh
cargo test -p bozzard-editor --test earth_factory profile_earth_factory_cpu -- --ignored --nocapture
```

For a repeatable exploration route through 1, 3, 6, 11, and 13 discovered regions, including
per-system CPU spans and total scene membership:

```sh
cargo test -p bozzard-editor --test earth_factory profile_earth_factory_exploration -- --ignored --exact --nocapture
```

Earlier exploration checkpoint, before chunk unloading (local debug build, seed 4, 60 samples at 13 regions / 657 scene
objects): transform reuse, change-driven HUD updates, and UI component queries reduced
median fixed-tick CPU time from **9.13 to 6.25 ms**, HUD layout from **1.90 to 1.08 ms**,
and scene extraction from **4.75 to 3.21 ms**. These are separate component timings,
not a windowed FPS comparison; machine load and build profile affect the measurements.

The native player also reports whole-frame CPU, completed-presentation intervals, and GPU-pass
percentiles when run with `--frames 180`. Presentation intervals include pacing; renderer CPU
times alone do not. A drawable player window now lets FIFO/vsync pace frames instead of adding
another 16 ms wait after frame work. Minimized, occluded, and recovering windows retain a retry
delay; editor Play also requests continuous repaint while its presentation backend handles pacing.

Latest exploration checkpoint (same local debug build and seed, 13 explored regions at 8,4):
streaming retains **3 loaded regions / 283 scene objects**, down from 657 objects. Median audio
extraction fell from **1.50 to 0.013 ms**, collision-overlay extraction from **1.40 to 0.009 ms**,
and fixed-tick CPU from **6.53 to 3.92 ms**. In the native player at 1024 × 640 over 180 frames,
median whole-frame CPU fell from **23.21 to 11.33 ms**. With the extra pacing wait removed,
median presentation interval was **17.25 ms (about 58 FPS)**, with a **37.17 ms p95**; this is
a local measurement, not a guarantee of sustained 60 FPS. Initialization and occasional frame
spikes remain in the samples.

The GPU regression below compares world pixels with all explored chunks
retained against streamed chunks at multiple zoom levels, orbit angles, and viewport widths:

```sh
cargo test -p bozzard-editor --test earth_factory chunk_streaming_matches_retained_world_rendering -- --ignored --exact --nocapture
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

The roughly 63-second day/night cycle smoothly fades the sun, ambient light, sky colors, and exposure into dark blue moonlight. Stars fade into the sky behind the terrain at night and disappear at dawn; the HUD stays readable and production continues. Stars use the existing sky pass without spawning entities or adding draw calls. A moving sun, disk saves, advanced tiers, and the spaceship remain later steps.

The source of truth for the scene, tiled ground, UI, and prefabs is [`tools/generate_scene.py`](tools/generate_scene.py). Gameplay is [`scenes/scripts/earth_factory.rs`](scenes/scripts/earth_factory.rs). Rerunning the generator replaces manual edits to its generated files. Machine prefabs use unscaled pivots, so their children retain their intended height during rotation.
