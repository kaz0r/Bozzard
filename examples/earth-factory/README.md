# Stellar-IX — Earth and Moon prototype

An editor-playable, isometric 3D voxel-style factory slice for the first Earthlike world. The scene, machines, nodes, and HUD are ordinary Bozzard scene objects and prefabs. Gameplay runs in a Rhai entry script and imported modules.

All 49 materials and equipment items have dedicated low-poly 3D assets matching
the Mk1 machines. Conveyors, machine output and co-op transfers use the new meshes
with reusable item entities and per-model deck heights. Fluid samples have drum,
canister or cylinder art; pipes carry their contents normally. See
[`assets/factory-materials`](../../assets/factory-materials/README.md) for the
catalog, editable Blender source, native showroom and regeneration instructions.

From the repository root:

```sh
cargo run -p bozzard-editor-app --bin bozzard-editor -- --scene examples/earth-factory/scenes/earth.json
```

Click the green **▶ Play** button to open the **Stellar-IX** main menu. Select **Survival** or **Creative**, then **Create world**. Survival starts on **Stellar-BX** with manual gathering: hold **F** on iron, copper, and stone nodes, then deliver **12 iron ore + 8 copper ore + 8 stone** through journal **J**, page I. This unlocks the first miners, smelters, conveyors, poles, cables, and workbench recipes. Creative unlocks all implemented tools, recipes, and the completed rocket; construction and ingredient loading are free, but machines still need a working power connection. The same scene and controls work in the native player. Board the completed rocket with **E** and choose **Launch to Moon** to visit **Stella-Z2**; return trips currently require no fuel.

**Dev World** opens directly from the main menu. Its fixed 2 × 2 chunks form a 30 × 30-tile checkerboard, one block thick. Spaced rows contain all 15 Earth and Moon deposits and every machine type, including the fourteen expansion machines, straight/elbow pipes and left/right corner belts, plus a cable between two poles. Isolated conveyors and pipes display all 49 item types; the backpack starts with ten of the original 25 types. The four chunks remain loaded, with no procedural deposits or neighboring regions. Machines use their ordinary simulation and power rules. **N** restores the showcase; save/load retains edits. Planet travel is unavailable in this world.

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
| Ctrl + 1–5 | Select Production / Logistics / Power / Processing / Advanced |
| 1–8 | Select a slot on the current bar |
| R | Lift, rotate, and lower the machine under the cursor; on empty ground, turn the next placement |
| Ctrl + R | Smoothly orbit the camera by 90° |
| F (hold) | Hand-gather a solid resource beneath the cursor |
| J | Open or close the journal |
| I | Open or close player inventory, unlocked in Tier 1 Phase 3 |
| M | Open or close the region map |
| Escape | Open the factory menu, or Continue when it is open |
| Left click | Select an action-bar tool, then place it on the highlighted world tile; clicking a machine moves the player marker beneath it |
| Right click | Inspect machine buffers; Link/Unlink power terminals and cables; cancel placement |
| Mouse wheel | Smoothly zoom in or out over the world |
| Left / Right, or journal tabs | Turn journal pages while the book is open |
| E | Collect miner/smelter output; open a machine interface, storage, rocket, or fuel dock |
| Space | Build the selected unlocked machine, paying its material cost |
| X | Demolish a machine and discard its contents |
| N | Reset the world, discoveries, backpack, and progression with a new seed |
| F6 (player) | Reload the source scene |

**Action bars.** Production contains smelters, miners, Constructors, assemblers, water pumps, oil extractors, crushers and ore washers. Logistics contains belts, storage, splitters, mergers, straight pipes, elbow pipes and left/right corner belts. Power contains generators, Mk1 poles (slot 2), cables (slot 3), solar panels (slot 4), two-spot solar arrays (slot 5), and small wind turbines (slot 6). Processing contains foundries, refineries, chemical plants, electrolyzers, kilns, glassworks, greenhouses and electronics fabricators. Advanced contains manufacturers and recyclers. Locked slots stay visible and explain their requirements through the journal. Ctrl-number chords switch bars without also selecting a slot.

**Player stacks.** Inventory **I** has 25 freely arranged slots, each holding up to 100 of one item. Drag to move, swap different items, or merge matching stacks (overflow stays in the source). Right-click a stack for **Split**, **Destroy**, or **Cancel**. Split puts half into an empty slot. **Destroy All**, at the bottom right, clears only carried items. Gathering and collection respect available room; uncollected machine output and stored items remain in place. Crafting checks the space available after spending ingredients before committing the transaction. Storage keeps its separate 16 × 100 capacity.

**Machine rotation.** A turn takes 0.6 seconds: lift, quarter-turn, then lower. Repeated presses queue up to eight turns on each machine, while holding R does not repeat. The machine's production and item transfers wait until it lands, when its output direction changes. Other machines continue running. Removing an animating machine clears its pending turns, ingredients, and progress. Camera turns remain separate and take 0.55 seconds.

**Exploration.** Regions contain 15 × 15 cells. The planet extends eight regions north, south, east, and west of the landing region: a 17 × 17 grid, including diagonals, for up to 289 regions. Neighboring ground and deposits appear before crossing an edge. Their layouts depend on the planet seed and region coordinates, so discovery order does not reroll a location. Ordinary regions have **2–4 deposits**, with no repeated resource type and at least **three tiles between deposit centers**, including across region boundaries. Iron, copper, stone, sand, and limestone are common; quartz, coal, and water are uncommon; silver and oil are rare. The landing region guarantees the five common materials outside the future landing pad, and one cardinal neighbor guarantees coal. Seeded outposts farther away guarantee water, quartz, oil, and silver somewhere on the finite planet. Solid nodes can be gathered by hand; water and crude oil require their dedicated pump or extractor. Silver has no processing recipe yet. Revisited regions retain machine placement, facing, buffered items, production progress, and storage contents. The optional production-test demonstration retains its compact factory and full resource set.

Every region containing factory equipment on Earth or Stella-Z2 simulates, including regions
on a planet you have left. Production and transport share one clock across both planets: conveyors,
splitters, mergers, and machine inputs/outputs connect across region boundaries with the same
capacity and backpressure rules as neighboring tiles. Power networks also span regions.
Each planet retains its own power networks, recipes, buffers, and storage totals. Machines still
require power and stop when their buffers or destinations fill. The debug HUD's **Simulating**
count reports factory regions across both planets; loaded/explored counts describe the planet in view.
Distant chunk models unload while their compact factory state keeps advancing. Approaching
again restores the models with their latest buffers, production progress, and storage contents.
The loaded neighborhood includes a margin for tall machines and shadows and expands for zooming
out or a wider viewport; camera orbit is covered throughout its animation. Unloading does not
reroll deposits or pause factories. Save the world to retain region state across closing the game.
**N** deliberately starts a new world. Chunks share
the same ground mesh, and the authored landing-site and rocket models remain resident.

To profile one versus eight producing regions (including unloaded factories), or factories
running on one versus both planets, run:

```sh
cargo test -p bozzard-demo --release --test earth_factory profile_world_factories -- --ignored --nocapture
cargo test -p bozzard-demo --release --test earth_factory profile_planet_factories -- --ignored --nocapture
```

This reports headless CPU tick timings separately for ordinary frames and production beats;
it does not measure GPU time or windowed FPS.

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

**Map.** Press **M** or click **Map** to see the current planet: **17 × 17** regions on Stellar-BX or **13 × 13** on Stella-Z2. Green cells are currently loaded; blue-gray cells were explored but have unloaded; dark cells are unexplored. Orange marks your current region, and **H** marks the landing site. Counts match the debug HUD. North stays at the top when the camera rotates. The map blocks movement, building, collection, and camera zoom while factories continue running. **M** or **Close** returns to play; **Escape** opens the factory menu. Opening the map closes the journal or storage.

**Journal.** The book fades in with three pages:

- **I — Unlocks:** available and locked equipment, the next phase's delivery, carried materials, and a delivery button.
- **II — Recipes:** select a material or equipment recipe to see its ingredients, output quantity, and crafting order. The workbench offers **Craft once** and **Craft up to 10**. Hand-crafting requires standing on or adjacent to the landing pod in region 0,0. Concrete requires an assembler. Pole and generator entries show their construction costs; place them from the Power action bar.
- **III — Spaceship:** the landing-site upgrade, visible 0%/50%/100% rocket assembly, and the route to Stella-Z2.

Movement, construction, gathering, and action-bar changes pause while a modal is open or closing. Factory production continues.

**Player inventory.** Phase 1 already retains gathered materials for journal crafting and deliveries. Tier 1 Phase 3 unlocks **I**, showing all carried materials and crafted Miner Mk1 items without resetting those quantities. The screen shows the 25 arranged backpack stacks described above, including the expansion materials. Storage containers retain their separate stack system.

**Machine interfaces.** **E** opens an assembler or Constructor within one tile. Assemblers choose conductive alloy, machine parts, or concrete; Constructors choose iron sheets, nuts and bolts, or cable. **Load ingredients** transfers up to ten recipe batches (ten inputs for a single-input machine), accounting for missing ingredients and the shared 100-item limit. Creative supplies ingredients freely. Conveyors can supply the same materials. **Collect output** transfers the finished stack. Collect existing output before changing recipes. Inputs return to carried stock when ingredient types or output batch sizes change; otherwise the machine keeps them. Recipes and buffers survive chunk unloading. **E** or **Close** returns to play.

A smelter with finished output gives it to you immediately on **E**. When it has no finished output, **E** opens its loading interface for iron ore, copper ore, or sand. Production continues while the interface is open; use **Collect output** for the finished ingots or glass.

**Factory menu.** Escape opens a menu with Continue, Save, Load, Main menu, and Exit. Continue or Escape returns to play. Factories, conveyors, animations, and diagnostics continue running behind the menu, while gameplay input is blocked. Save opens five manual slots; Load also offers the automatic slot. **Load world** is available from the main menu. Selecting a manual slot replaces that save; selecting a saved world replaces the running session. Main menu returns to world creation. Save before exiting; Exit itself does not create a save.

**Saves.** Auto-save runs every twenty minutes of active world time, including time spent in menus, and waits for rocket landing before taking a snapshot. Manual saves reset that interval. The browser shows tier/phase, day/night cycle, planet, and save age. Both planets' explored deposits, factories, wiring, recipes, buffers, storage, the arranged player inventory, progression, and world clock survive loading. The file contains game data, not scene assets or render handles. File reads and atomic replacement writes run on a background worker. Invalid or incompatible saves are reported without replacing the running world. The native authority check rejects guest save/load requests; the Steam gameplay integration is still in progress.

Factory exports use the controller's `steam_coop` settings (development App ID **480**, four players) and require a matching Steam-enabled player. The export includes the SDK library and development launch file. Steam remains optional for solo play. Guests receive the host's actual conveyor transfers and animate them locally, including factories on a different planet from the host.

Save files live in `stellar-ix/saves` beneath the platform's application-data directory: `$XDG_DATA_HOME` or `~/.local/share` on Linux, `%APPDATA%` on Windows, and `~/Library/Application Support` on macOS. Slot zero is `autosave.json`; manual slots are `slot-1.json` through `slot-5.json`. Saves are shared between editor Play and the native player for the same user.

Guest keyboard movement is predicted on the next local simulation tick and reconciled with host acknowledgements. Builds, inventories, production, new terrain and travel remain host-authoritative. Batched movement requests retain their order instead of losing steps to same-tick rate checks.

The user confirmed the movement fix in a two-computer retest. For further live
verification, follow the [two-computer Steam checklist](../../docs/stellar-ix-coop-test.md).

**Steam overlay.** Launch through Steam with its in-game overlay enabled, then use
**Shift+Tab** (Steam's default shortcut) or **Steam co-op → Open Steam overlay**.
The co-op panel reports whether the overlay is ready. While it is open, gameplay
input is blocked and factories keep running. **Invite friends** opens Steam's lobby
invite dialog; the friend picker works when the overlay is unavailable.
To prepare a source-build launcher for Steam's non-Steam-game library entry:

```sh
./tools/steam.sh run --release --prepare-only --project examples/earth-factory/bozzard.project.json
```

Add the printed launcher path to Steam and launch that entry. Use `editor` instead
of `run` for an editor launcher. Exported packages can use their `Play-test.sh`
launcher. No Steam settings are changed by the game.

**Steam co-op (integration in progress).** Open **Steam co-op** from the title or **Steam co-op / Invite friends** from Escape. Create a friends-only lobby for one host and up to three guests. Close the lobby panel and create a world or load a save to start; the host can start alone. Invite friends through Steam's overlay or the friend picker, including during play. Guests use the host's world and cannot save or load it. **Enter** opens chat; Escape stops typing. Blue marks the host, with red, orange, and green for guests; nearby names appear above their cursors. Solo play remains available without Steam. The current development App ID is 480. In-process gameplay tests, native editor/player Steam host checks, and source-independent export checks pass; populated UI previews pass. The user has confirmed two-computer guest play, machines, electricity and planetary travel; save/load, chat, reconnect and four-player details still need live verification. See [session implementation and verification](../../docs/stellar-ix-sessions.md).

**Zoom.** Scroll up to zoom in and down to zoom out. The camera eases between a vertical view span of 9–32 world units (starting at 19), keeping its isometric angle. HUD panels, storage, the journal, and the factory menu consume scrolling. New worlds reset zoom; orbiting and chunk travel preserve it.

**Progression.** Deliveries consume carried materials to enter the target phase. Tier 1 Phase 1 starts unlocked.

| Enter phase | Delivery | Unlock |
| --- | --- | --- |
| T1 P1 | None | Manual gathering |
| T1 P2 | 12 iron ore + 8 copper ore + 8 stone | Smelter Mk1, Miner Mk1, Conveyor Mk1, poles, cables, basic journal crafting |
| T1 P3 | 16 iron ingots + 8 copper ingots + 4 glass | Storage and player inventory [I] |
| T1 P4 | 12 iron sheets + 40 nuts and bolts + 8 cables | Coal generator |
| T2 P1 | 20 iron sheets + 80 nuts and bolts + 16 cables + 8 glass | Constructor, assembler, all fourteen expansion machines, pipes, corner belts, splitters and mergers |
| T2 P2 | 80 concrete + 40 iron sheets + 80 nuts and bolts | 4 × 4 × 1 asphalt landing site and fuel INPUT dock |
| T2 P3 | 160 iron sheets + 240 nuts and bolts + 80 cables + 40 glass | Rocket 50% assembled |
| T2 P4 | 200 iron sheets + 320 nuts and bolts + 120 cables + 80 glass | Rocket complete, blinking lights, E destination interface |

Creative and the demonstration unlock every implemented tool immediately.

**Recipes.** Journal crafting for basic materials unlocks at T1 P2 so the first pole can be built before machinery is powered. Constructors later automate those same recipes.

| Output | Ingredients per craft | Machine |
| --- | --- | --- |
| 1 iron ingot | 1 iron ore | Smelter |
| 1 copper ingot | 1 copper ore | Smelter |
| 1 glass | 1 sand | Smelter |
| 1 iron sheet | 1 iron ingot | Constructor |
| 4 nuts and bolts | 1 iron ingot | Constructor |
| 1 cable | 1 copper ingot | Constructor |
| 1 concrete | 2 stone + 1 limestone | Assembler only |
| 1 conductive alloy | 1 iron ingot + 1 copper ingot | Journal or assembler |
| 1 machine part | 1 iron ingot + 1 copper ingot | Journal or assembler, T2 P1 |

The expansion recipes unlock at Component automation (T2 P1). Open a machine
with **E** to choose its recipe, load ingredients and collect every output. All
fourteen require wired power and share a 100-unit buffer for inputs and products.
Production waits for the entire ingredient batch and enough room for every
byproduct. Changing recipes refunds ingredients atomically; collect existing
products first. Pipes supply fluids and belts supply solids. Creative also lets
you load ingredients freely for testing.

| Machine | Recipes per cycle | Power |
| --- | --- | --- |
| Water pump | Water deposit → 1 water | 2 |
| Oil extractor | Oil deposit → 1 crude oil | 3 |
| Crusher | 1 stone → 1 gravel; 1 iron/copper ore → 1 crushed ore | 2 |
| Ore washer | 1 crushed iron/copper ore + 1 water → 1 purified ore | 3 |
| Foundry | 2 iron ingots + 1 coal → 2 steel; 1 purified ore → 3 matching ingots | 4 |
| Refinery | 3 crude oil → 2 fuel + 1 heavy oil | 5 |
| Chemical plant | 2 heavy oil → 1 plastic; 2 fuel → 1 rubber; 1 heavy oil + 1 limestone → 1 fertilizer | 4 |
| Electrolyzer | 2 water → 2 hydrogen + 1 oxygen | 6 |
| Kiln | 1 clay → 1 brick; 1 limestone → 1 lime; 1 quartz → 1 silicon | 3 |
| Glassworks | 1 sand → 1 glass; 2 glass → 1 lens | 3 |
| Greenhouse | 1 seed + 2 water + 1 fertilizer → 4 biomass + 1 returned seed | 2 |
| Electronics fabricator | 1 copper ingot + 1 silicon + 1 plastic → 1 circuit | 4 |
| Manufacturer | 1 steel + 2 cables → 1 motor; 2 motors + 2 circuits + 2 steel + 2 rubber → 1 machinery + 1 scrap | 6 |
| Recycler | 2 scrap → 1 iron ingot + 1 plastic; 1 machinery → 2 iron ingots + 1 copper ingot + 1 plastic | 3 |

Clay is guaranteed in Earth region **1,1** and wild seeds in **-1,-1**, including
newly explored regions in existing saves. Gather either with **F** or a miner.
Water pumps and oil extractors must stand on matching deposits. Miners now
extract solid resources; liquids use the dedicated pump or extractor. Existing
saves retain their factories and upgrade to the expanded material catalog.

Each expansion machine costs **8 iron ingots + 4 copper ingots + 2 machine parts**.
A pipe costs **1 iron ingot + 1 copper ingot**; a corner belt costs **1 iron ingot**.

**Fluid routing.** Straight pipes connect opposite sides; elbows connect two
adjacent sides. **R** rotates their ports. Each holds up to 100 units of one
fluid: water, crude oil, fuel, heavy oil, hydrogen or oxygen. Up to five units
move per connection each production beat, with pressure equalization between
pipes. New arrivals wait until the next beat and different fluids never mix.
Storage accepts fluids at its rear and exports them at its front. Machine fluid
inputs are at the rear, except washers and greenhouses, whose water inlet is on
their local +Z side. Outputs face forward; refinery heavy oil and electrolyzer
oxygen use a separate +Z outlet. Empty or collect a line before changing fluids.
Pipes need no power and continue draining buffered products from idle machines.
Solid products leave through gold belt ports, alternating between byproducts.

**Equipment costs.** A smelter costs four iron ore; a Miner Mk1 costs four iron ingots and two copper ingots; a belt costs one iron ingot; storage costs four iron ingots. Constructors and assemblers each cost eight iron ingots and four copper ingots. A pole costs **20 nuts and bolts + 4 iron sheets + 2 glass**. A coal generator costs **80 nuts and bolts + 20 iron sheets + 2 crafted Miner Mk1 items + 4 copper ingots**. Craft the portable miners in journal page II. Placing a miner uses a carried Miner Mk1 item first, otherwise pays its material cost; generator construction consumes only carried miners, never placed machines.

**Wired power.** The pod supplies eight power; a generator placed on a coal node supplies ten. Miners use one, smelters and Constructors two, and assemblers three. Each circuit checks its own supply. Unwired or overloaded machines stop producing while logistics continue moving existing output. One pole can be hand-crafted from **9 iron ore + 2 sand** through ingots, sheets, nuts and bolts, and glass. Craft cable separately from copper ingots.

1. Press **Ctrl+3**, then **2**, and **Space** on an empty tile to place a pole.
2. Choose power-bar slot **3** for the cable tool. Stand on the landing pod and press **Space**, then stand on the pole and press **Space** again.
3. Repeat from the pole to a miner, smelter, Constructor, assembler, generator, or another pole. Each successful connection consumes **one cable item** in Survival. Failed or duplicate connections consume nothing; disconnecting does not refund the cable. Cables can run in any horizontal direction, including diagonals, and cross region seams.
4. **R** cancels an unfinished cable. **X** with the cable tool disconnects all cables at the current terminal; with a machine selected, X demolishes it and removes its cables.

Every cable must have a pole at one end. Each pole has **five connections total**, counting sources, machines, and neighboring poles; machines and sources have one port each. Duplicate and self-connections are rejected. Wires attach to raised terminals and follow machine rotation animations. Connected, adequately supplied poles show a glowing green lamp. All loaded poles retain their status lamps; up to **32** nearby powered poles also cast real, unshadowed light on ground and machines through a reusable light pool. The completed rocket borrows one slot while its home region is loaded and the landing pod's circuit has power: its red and green navigation lights alternate and illuminate nearby surfaces. An overloaded or missing pod supply switches them off. Unused lights do no fragment-lighting work. This is a visual light budget, not a limit on pole placement or power networks.

**Machine buffers.** Miners, smelters, Constructors, and assemblers hold at most **100 items total per machine**, counting ingredients and finished output together. Smelters and Constructors keep input and output stacks within that shared limit. Constructors reserve capacity for a complete output batch, including the extra three items created when one ingot becomes four nuts and bolts. Assemblers reserve enough room for the other recipe ingredient, including two stone for concrete, so one feed cannot fill all 100 spaces. With two ingredients per part, an assembler may need its output collected before it can fit another complete recipe. Full machines block incoming transfers. Buffers survive region changes; demolition and **N** clear them.

**Logistics.** Conveyors carry one visible solid item per tile, with no storage buffer. Straight belts accept rear or side feeds, never through the forward outlet. Left/right corner belts accept only their curved inlet and animate the item around a 90-degree arc. Splitters and mergers each hold up to **10 items**, sharing one material stack. Facing indicates the forward direction: a splitter accepts only from the rear and sends to the front, left, and right in turn, skipping blocked outputs. A merger accepts from the rear, left, and right and sends forward. Its inputs take turns; a waiting different material lets the current batch drain before entering. Cyan model ports are inputs; gold ports are outputs. Full or disconnected routes back up without discarding items. Routing turns and contents survive region changes.

Buffering does not speed up production or transfers: each connection still moves at most one item per factory beat. Quantities share one representative item model per occupied output, plus transient transfer visuals, so filling a buffer does not spawn 100 entities. The coal-node generator retains its existing power behavior without a fuel inventory.

**Machine collection.** Press **E** on or within one tile of a miner or smelter (including diagonals) to take **finished output that fits** into your backpack. Assemblers and Constructors use their interface’s Collect output button instead. The nearest machine/container wins, and the tooltip shows the target. The tooltip shows total buffer usage out of 100 and the collectible quantity. Unfinished ingredients and recipe progress stay inside; collection frees capacity for further production. Check carried materials in **J**, or **I** after T1 P3.

**Storage.** Each container has 16 slots with 100 items per stack. Its cyan rear port accepts input and its opposite gold port outputs one item per factory beat, starting with the first occupied slot. **R** rotates both ports. A full or missing downstream machine leaves items in storage; newly received items wait until the next beat before they can leave. Transfers continue across chunk boundaries and on other planets. Press **E** within one tile, including diagonals. Drag stacks to move, merge, or swap them. Right-click for Split or Delete all; Delete all affects only that container. **Take items into backpack** transfers contents for crafting and deliveries. Full storage blocks incoming items. Demolition discards its inventory and updates stored totals.

**Landing site and rocket.** The upgraded platform is **4 × 4 tiles total and one block thick**, with its top flush with the build surface. Its power cabinet replaces the pod at tile (0,0); the upright rocket occupies (0,1), and the fuel dock is at (2,1) on the east edge. Before the upgrade, remove any machines on the rocket and dock tiles; a blocked delivery consumes nothing and leaves machines intact, even when the home region is unloaded. Other machines can remain on the platform. Future building on the pod, rocket, and dock footprints is blocked.

The T2 P3 delivery builds the lower rocket; T2 P4 adds the upper hull, nose, cockpit, and alternating navigation lights. Approach the completed rocket and press **E**. Its interface shows **Stellar-BX**, **Stella-Z2** as the next destination, and eight **Coming soon** entries. Choose **Launch to Moon** to close the destination panel and lift off. The rocket ascends for two seconds, the planet changes while it is offscreen, and it descends for 2.4 seconds onto Stella-Z2. World controls are suspended during flight; simulation continues. On the Moon, the same interface offers **Return to Earth**. Fuel is temporarily free in both directions; the other eight planets remain unavailable. The fuel dock's **E** interface identifies its INPUT role; it accepts no materials yet. New-world creation and **N** reset projects with progression. Creative begins with the completed site and rocket.

**Stella-Z2.** The Moon extends **six regions in each direction**, including diagonals: **13 × 13 / 169 regions**, each still 15 × 15 build tiles. Its gray surface stays dark with stars throughout play. Most regions have no deposits; ordinary occupied regions have one. Only three resources appear:

- **Amorium:** beige ore.
- **Moondust:** low gray-white deposits.
- **Techtorium:** exceptionally rare orange, black, and white crystals. Every seed guarantees **two** deposits in separate, widely spaced regions, four to six regions from landing.

The landing region contains one Amorium and one Moondust deposit, keeping the rocket platform clear. All three can be gathered with **F**, extracted by powered miners, conveyed, stored, and carried home. They appear in inventory **I**; no lunar processing recipes or new phases are invented yet. The arriving rocket shares the existing powered landing-site/workbench setup. Bring Earth construction materials to establish an outpost.

Travel preserves each planet's discovered nodes, factories, recipes, buffers, storage, and power wiring. Inventory, equipment unlocks, and progression travel with the player. Powered factories on both planets keep producing during flight and while you explore elsewhere; returning restores their latest items and storage contents. Departed terrain, deposits, machines, items, and cables unload, so background production creates no off-planet models. **N** or creating a new world resets both planets; existing save slots remain available.

For a prebuilt, wired production demonstration and profiling fixtures, set the scene blackboard's **`demo_mode` to true** before Play. This unlocks all existing machines, makes construction free, and restores the iron/copper/assembler sample. Normal play opens the Survival/Creative menu. Automated gameplay fixtures can set the controller’s `title_open` to false to begin directly; demonstration mode also skips the menu. Set **`seed` above zero** for a reproducible planet.

To compare CPU costs for simulation, UI layout, pointer input, scene extraction, and UI drawing
with storage closed and open (timings depend on the machine and build profile):

```sh
cargo test -p bozzard-editor --test earth_factory profile_earth_factory_cpu -- --ignored --nocapture
```

For Creative mode with the journal closed, open, and under an eight-event mouse burst
(approximately a 500 Hz mouse at 60 FPS), profile layout, input, and simulation separately:

```sh
cargo test --release -p bozzard-editor --test earth_factory profile_stellar_journal_cpu -- --ignored --nocapture
```

Omit `--release` to measure debug overhead. Compare FPS using the same build profile;
debug Rhai execution is substantially slower. Journal, machine-interface, and inventory
labels refresh when their data changes. The engine reuses unchanged UI layout while updating
hover, focus, and pressed feedback immediately; live component edits, scrolling, resizing,
localization, and camera-projected labels invalidate the cached geometry as needed.

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

Scene-wide opaque batching groups repeated meshes across intervening model parts
and reuses packed instance buffers, with ordering checks for coplanar surfaces.
A local release test with six loaded regions and 324 multipart machine prefabs
reduced color draws from **1,356 to 80** and renderer CPU from **9.67 to 3.76 ms**
at 1280 × 800. The four camera angles match the former batcher's pixels exactly;
indoor/window/door captures are unchanged. See [the batching reproduction and
native frame measurements](../../docs/performance.md#scene-wide-opaque-batching).

The roughly 63-second day/night cycle smoothly fades the sun, ambient light, sky colors, and exposure into dark blue moonlight. Stars fade into the sky behind the terrain at night and disappear at dawn; the HUD stays readable and production continues. Stars use the existing sky pass without spawning entities or adding draw calls. Stella-Z2 holds a separate, permanent night sky with stars and neutral moonlight over gray regolith. A moving sun, disk saves, and rocket fuel remain later steps.

The source of truth for the scene, tiled ground, UI, and prefabs is [`tools/generate_scene.py`](tools/generate_scene.py). Gameplay starts in [`scenes/scripts/earth_factory.rs`](scenes/scripts/earth_factory.rs), which only coordinates lifecycle hooks and frame order. The implementation lives in [`scenes/scripts/factory/`](scenes/scripts/factory/). The generator registers every `.rhai` module there as a script asset without rewriting it. Rerunning the generator replaces manual edits to its generated files. Machine prefabs use unscaled pivots, so their children retain their intended height during rotation.

Native visual checks cover world creation, journal recipes, machine and inventory panels, the landing site, both rocket stages, destinations, and actual nighttime pole illumination:

```sh
cargo test -p bozzard-editor --test stellar_ix -- --ignored --nocapture
```

## Space rocket and launch pad

The travel craft is an upright space rocket with stacked ivory hull sections,
a pointed nose, four vertical fins, blue cockpit ports, and three engine bells
underneath. It shares its armor, gold identification bars, and cyan service lines
with the wreckage. The launch pad has a graphite deck, an octagonal landing target,
perimeter guidance markings, a power cabinet, and a fuel input cabinet with an umbilical.

The existing construction deliveries reveal the pad at phase 5, engines/lower
fuel tank/fins at phase 6, and upper hull/cockpit/nose at phase 7. The upgraded power
cabinet replaces the starter pod visually, retaining its cable socket at `(0, 1.26, 0)`. Boarding
and fuel inspection remain on tiles `(0, 1)` and `(2, 1)`. Both ship halves share
one flight pivot; the pad stays on the ground. Powered navigation lamps follow
the hull, and three downward exhaust plumes appear during vertical flight.
Travel, fuel rules, saves, and co-op use the existing gameplay.

Review [`scenes/travel-ship-showroom.json`](scenes/travel-ship-showroom.json).
Editable Blender source, construction previews, and the manifest are in
[`../../assets/travel-ship/`](../../assets/travel-ship/); the GLBs are in
[`scenes/assets/models/travel/`](scenes/assets/models/travel/).

```sh
blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_travel_ship.py
python3 examples/earth-factory/tools/generate_scene.py
python3 examples/earth-factory/tools/validate_travel_ship.py
cargo test --offline -p bozzard-demo --test earth_factory rocket
```

## Spaceship debris prototypes

Four wreck sections share one survey-ship design: a broken cockpit, split cargo hull,
torn wing, and ruptured engine. Ivory armor, graphite interiors, gold identification
bands, and cyan service lines tie them together. The meshes include open fractures,
exposed ribs, severed cables, and loose armor fragments.

Every generated planet has **2–6 wrecks in total**. Two are guaranteed; each of four
optional slots has a **12% chance**, so most planets have two. They occupy distinct
regions at least two regions from the landing site. Seeded placement and rotation
stay the same through exploration, streaming, travel, and save/load. Their 3 × 3
footprints avoid deposits and are reserved against building in solo and co-op.
The production demonstration and fixed Dev World keep their authored layouts.
These are story props; salvage and story interactions are not implemented yet.

Open [`scenes/debris-showroom.json`](scenes/debris-showroom.json) in the editor to
review all four models. The self-contained GLBs and reusable prefabs live in
[`scenes/assets/models/debris/`](scenes/assets/models/debris/) and `scenes/assets/`.
Editable Blender source, a preview, and the geometry/spawn manifest are in
[`../../assets/spaceship-debris/`](../../assets/spaceship-debris/).

```sh
blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_spaceship_debris.py
python3 examples/earth-factory/tools/generate_scene.py
python3 examples/earth-factory/tools/validate_spaceship_debris.py
cargo test --offline -p bozzard-demo --test earth_factory spaceship_debris
```

## Foundations and indoor factories

The construction kit includes concrete and wood floors; concrete, brick, metal,
and wood walls; concrete and wood roofs; an automatic sliding door; and a modern
wall window. Floors and roofs occupy independent layers, so machines, conveyors,
and power connections continue to use their normal tiles. Walls, doors, and
windows occupy tile edges and can connect across region boundaries.

In Survival, the kit unlocks at Tier 2. Creative makes it available immediately.
Use **Ctrl+6** for floors, walls, doors, and windows; **Ctrl+7** for roofs.
Select a slot with **1–8**, press **R** to choose the edge, then **Space** to build.
**X** removes the selected layer or edge without demolishing its machine. Remove
a roof before removing its floor. Concrete pieces cost two concrete; brick walls
cost four bricks; metal walls cost two iron sheets. Wooden pieces use four biomass
as a prototype timber material. Doors use two iron sheets, two glass, and one
circuit board; windows use one iron sheet and two glass.

A connected floor area becomes indoors when every floor tile has a roof and its
perimeter is closed by walls, doors, or windows. Outside, the roof and shell conceal
the room and its machinery. Approaching a window reveals at most two tiles along
its inward sight line. Sliding doors take approximately **0.65 seconds** to open
and block entry until fully open. Walls and windows also block keyboard movement,
mouse selection paths, and machine interaction through them.

Inside a sealed room, its roof and camera-facing walls cut away, the room remains
clearly lit, and the exterior is darkened. Other sealed rooms stay concealed.
Removing a roof or opening a gap in the perimeter makes that area outdoors again.
Production continues during these view changes. Structures survive streaming,
planet travel, and disk saves; co-op shares construction and collision while each
player sees their own room view.

Open [`scenes/foundations-showroom.json`](scenes/foundations-showroom.json) and
press Play for a powered factory using this kit. It starts inside; leave through
the south sliding door to inspect the roof and the east window. This uses the
normal game controls and simulation. The main `earth.json` scene includes the
same building system.

Editable Blender source, a model preview, and the mesh manifest are in
[`../../assets/foundations/`](../../assets/foundations/). The self-contained GLBs
are in [`scenes/assets/models/foundations/`](scenes/assets/models/foundations/).

```sh
blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_foundations.py
python3 examples/earth-factory/tools/generate_scene.py
python3 examples/earth-factory/tools/generate_foundation_showroom.py
python3 examples/earth-factory/tools/validate_foundations.py
cargo test --offline -p bozzard-demo --test earth_factory foundations
```

Indoor presentation caches the occupied structure slots and door locations when
a region changes. Steady door updates visit only existing doors and move their
reused leaves while opening or closing. Interaction searches discard empty tiles
before checking walls. Archive change detection compares native blackboard lists
in place, and view refreshes reuse decoded neighboring room pages.

For a repeatable CPU profile of a sealed factory across a region boundary:

```sh
cargo run --offline -p bozzard-demo --example benchmark_foundations -- 6
```

The argument is the factory's side length, from 3 to 18 tiles. The benchmark
compares ordinary play, individual indoor routines, and forced cutaway refreshes.
Local debug measurements for a 6 × 6 factory in two loaded regions reduced median
fixed-tick CPU time from **15.95 to 1.04 ms**, and forced view refreshes from
**13.02 to 2.96 ms**. A 180-frame native run of that factory on Intel Iris Xe / Vulkan
recorded a **17.20 ms median presentation interval (about 58 FPS)** and **34.46 ms
p95**. Startup shader/asset work remains visible in the larger outliers. These are
local measurements, not a sustained frame-rate guarantee on other hardware.

## Gameplay source layout

| Module | Responsibility |
| --- | --- |
| `data.rhai` | Item/equipment metadata, recipes, costs, session flags |
| `grid.rhai` | Coordinates, packing helpers, region cache access |
| `deposits.rhai` | Seeded Earth and Moon resource placement |
| `debris.rhai` | Sparse seeded story wreckage, footprint reservation, streamed visuals |
| `architecture.rhai` | Shared construction layers, edge addressing, placement validation and room enclosure |
| `interiors.rhai` | Streamed structure models, door animation, collision and local indoor visibility |
| `world.rhai` | New worlds, region state, planet travel |
| `dev_world.rhai` | Fixed checkerboard showroom and complete asset catalog |
| `chunks.rhai` | Discovery, visual loading, bounded residency |
| `simulation.rhai` | Production beats, conveyor/splitter/merger routing |
| `buffers.rhai` | Shared production buffers and atomic recipe/feed/refund/collection rules |
| `fluids.rhai` | Conservative pipe pressure, fluid inputs and separate product outlets |
| `model_ports.rhai` | Expansion-model cable, lamp and heat attachment coordinates |
| `factory_state.rhai` | Per-planet snapshots and writeback for all built regions, including off-planet factories |
| `host_view.rhai` | Reconcile accepted co-op host edits and remote rotations with resident models |
| `item_fx.rhai` | Resident item models, cross-region motion and bounded visual reuse |
| `building.rhai` | Placement, removal, rotation requests, action bars |
| `power.rhai` | Circuits, cables, supply, power lamps |
| `inventory.rhai` | Machine collection and storage stacks/UI |
| `backpack.rhai`, `backpack_ui.rhai` | Free player stacks, atomic capacity checks, drag/drop and stack menus |
| `pointer.rhai`, `inspection.rhai` | Camera-aware world picking, placement, cable menus and buffer inspection |
| `flight.rhai` | Departure, planet handoff and landing animation |
| `machines.rhai` | Recipe selection, feeding, machine interface input |
| `progression.rhai` | Gathering, deliveries, workbench crafting |
| `environment.rhai` | Camera orbit/pan/zoom and planet lighting |
| `visuals.rhai` | Item/machine animation, landing site and rocket visuals |
| `hud.rhai` | HUD, contextual tooltip, debug statistics |
| `journal.rhai` | Journal pages, recipes, crafting controls |
| `panels.rhai` | Panel state and shared interface drawing |
| `navigation.rhai` | Title, menu, map, destination input |

Cross-module calls use explicit namespaces, such as `power::update_power()`. Imports use catalog
IDs (`import "factory-power" as power;`), so editor Play, player, and exported builds resolve the
same source. Keep mutable game state in the existing blackboards; module files contain functions.
See [Rhai import rules and hot reload](../../docs/scripting.md#import-shared-rhai-modules).

## Renewable power

The Power toolbar (**Ctrl+3**) now includes a **solar panel** in slot **4**,
**solar array** in slot **5**, and **small wind turbine** in slot **6**. They unlock
at Tier 2, and are immediately available in Creative and Dev World. Place them
on clear ground and cable each source to a power pole using slot 3.

| Source | Footprint | Output | Build cost |
| --- | --- | --- | --- |
| Solar panel | 1 spot | 4 power during Earth's daytime | 4 iron ingots, 2 copper ingots, 2 glass, 2 cables |
| Solar array | 2 adjacent spots | 8 power during Earth's daytime | 8 iron ingots, 4 copper ingots, 4 glass, 4 cables |
| Small wind turbine | 1 spot | 6 power continuously | 6 iron ingots, 4 copper ingots, 2 cables |

**R** rotates the array around its first spot; the second spot follows its facing.
Both spots must be explored, clear of deposits, machines and wreckage, and inside
the world. Arrays can cross region boundaries. Occupied rotation destinations are
rejected, and **X** from either spot removes the whole array and its cable. There
is one saved machine and one circuit terminal per array. The host enforces the
same footprint and costs in multiplayer.

Solar-only circuits stop at night and restart at dawn, including when Earth is
simulating in the background. Solar has no output on the permanently dark lunar
map. Wind is steady in this prototype, without a weather simulation. Dawn/dusk
refreshes the circuit graph once per transition; unchanged frames reuse the
existing power state. The turbine rotor is static in these prototype meshes.

Open [`scenes/renewables-showroom.json`](scenes/renewables-showroom.json) in Editor
Play to inspect a wired solar/wind circuit. Editable meshes, the model manifest,
and the studio preview are in [`../../assets/renewables/`](../../assets/renewables/).
The indexed GLBs use 560 / 1,220 / 348 triangles with 5–6 shared material groups.

```sh
blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_renewables.py
python3 examples/earth-factory/tools/generate_scene.py
python3 examples/earth-factory/tools/generate_renewables_showroom.py
python3 examples/earth-factory/tools/validate_renewables.py
cargo test --offline -p bozzard-demo --test earth_factory renewables_
```
