# Stellar-IX — game design brief

*Design recorded 2026-09-25; user-defined starter-world progression updated 2026-09-26.*

The [Stellar-IX Earth prototype](../examples/earth-factory/README.md) runs inside Bozzard editor Play and the native player. It includes seeded neighboring regions, machine locks and deliveries, grouped action bars, animated machine rotation, a journal, a map, Survival/Creative world creation, wired power, and an assembler interface. All built regions on both planets simulate together, including unloaded and off-planet factories, and conveyor lines can cross region boundaries.

**Design status:** The user approved the two-tier progression, seven deliveries, and early journal-crafting path on 2026-09-26. The prototype implements these phases, material recipes, stone/sand/silver nodes, Constructor, player inventory, the landing-site upgrade, both rocket stages, and the destination interface. The user clarified that the whole landing site is **4 × 4 × 1** and that launch should initially wait for fuel and Moon design. On 2026-09-26, the user subsequently approved playable Stella-Z2 and temporary fuel-free return travel. On 2026-09-27, the user approved persistent saves and four-player Steam co-op. Manual saves, twenty-minute autosaves, and the save browser are implemented; Steam world synchronization and lobby UI remain in progress. See [session implementation scope](stellar-ix-sessions.md).

## Core idea

A single-player, isometric 3D factory game with a voxel visual style. The player explores worlds, builds factories on resource nodes, manufactures increasingly complex parts, and constructs spaceships to reach new worlds. The initial inspiration is the sense of planetary exploration in *Astroneer* combined with the production chains of *Satisfactory*. Destructible or sculptable terrain is **not** part of this concept.

The game begins on an Earthlike world. The player soon realizes they are alone: there are no other people there. The first major objective is to build a spaceship rather than start with one. Doing so requires a functioning factory, a launchpad, and fuel.

Factory equipment includes miners, conveyors, splitters, mergers, assemblers, improved assemblers, manufacturers, storage containers, power generation, and resource-specific processing machines. Resource nodes determine where extraction can happen; the player designs the transport and production lines between them.

## World structure and progression

- The intended game has **ten distinct worlds**, beginning with the Earthlike **Stellar-BX**, followed by the moonlike **Stella-Z2**. The other eight destinations display **Coming soon**. Their identities and progression remain future design work. **Stellar-IX** is the game title, not the starter planet's name.
- Every world provides something needed to progress. The Earthlike world must **always** contain every resource required to build and fuel the first spaceship.
- The starter world has **two tiers of four phases**. Tier 1 Phase 1 is available from the start; seven subsequent deliveries advance to the remaining phases. The rocket is complete at Tier 2 Phase 4. Stella-Z2 currently preserves those unlocks; its own tiers and requirements are not yet defined.
- Unlocks persist between worlds. Arriving on the moon does not mean relearning miners, conveyors, and assemblers; its phases introduce new challenges and technology.
- Production should move from hand-gathered starter materials to continuous automated lines, then to cross-world supply chains.

### Procedural resource placement

Node locations must change between playthroughs. A player should not be able to memorize where a useful node will appear in the next game. World generation should use a saved seed so the layout stays stable within one playthrough.

Random placement needs progression guarantees: required node types must exist, be reachable, and support the manufacturing and fuel needed to leave that world. On Earth, basic nodes should be reasonably accessible from the landing area, while later resources can require an outpost or a longer conveyor route. Node positions and richness can vary without producing an unwinnable seed.

The current resource balance uses 2–4 spaced deposits per ordinary region. Iron, copper, stone, sand, and limestone are common; quartz, coal, and water are uncommon; silver and oil are rare. The landing region guarantees the five common materials, with coal in a neighboring region and the remaining resources guaranteed at seeded outposts farther away. Deposit centers stay at least three tiles apart, including across region boundaries.

## Stellar-BX — Earthlike starter world

Earth has an atmosphere broadly like Earth's. It has a familiar sky, clouds and sunsets, with a day-and-night cycle. Factories continue to run at night; their lighting reinforces the feeling that the world is empty. The exact cycle length and any effects on production remain undecided.

Keep the existing resources and add **Stone**, **Sand**, and **Silver ore** as gatherable nodes:

| Node or source | Early purpose | Later purpose |
| --- | --- | --- |
| Iron ore | Basic parts, miners, conveyors, machine frames | Steel and spaceship structure |
| Copper ore | Wire and power connections | Electronics |
| Stone | Concrete, combined with limestone | Landing-site construction |
| Limestone | Concrete, combined with stone | Landing-site construction |
| Sand | Glass | Power poles and later construction |
| Silver ore | New node; processing recipe and use not yet specified | To be designed |
| Coal | Sustained factory power | Steel production |
| Quartz | Retained resource; no new early recipe specified | Possible silica and circuit components |
| Crude oil | Plastic and machine parts | Spaceship components |
| Water | Processing | Hydrogen and oxygen for rocket fuel |

Glass comes from **sand**, replacing the earlier suggestion of quartz-based glass. Steel, electronics, plastic, and water-derived rocket fuel remain possible future chains; they are not requirements in the confirmed two-tier plan. Silver processing and rocket fuel need their own recipes before either can gate progression.

### Confirmed tier and phase plan

The landing pod supplies hand tools, a crafting bench, and limited initial power. The user moved starter power poles and cables from Tier 1 Phase 4 into **Phase 2**, so the first machines can connect to the pod. Phase 4 now introduces the coal generator.

| Tier | Phase | Unlock | Purpose |
| --- | ---: | --- | --- |
| **1 — First factory** | 1 | Available by default: manual gathering | No automation. |
| | 2 | Smelter Mk1, Miner Mk1, Conveyor Mk1, starter power poles and cables | Establish the first powered production line. |
| | 3 | Storage and player inventory | Open the player inventory with **I**. |
| | 4 | Coal generator | Expand factory power beyond the landing pod. |
| **2 — Space program** | 1 | Constructor and assembler | Constructor: one input ingredient type and one output. Assembler: two input ingredient types and one output. |
| | 2 | Bigger landing site and fuel dock | Upgrade the whole landing-pod ground to **4 × 4 × 1**, with asphalt; place an **INPUT** dock at its outer edge for rocket fuel. |
| | 3 | Space rocket assembly: **50%** | Show a visibly half-built rocket. |
| | 4 | Space rocket assembly: **100%** | Complete the rocket, add blinking lights, and allow the player to enter with **E**. |

Inside the finished rocket, show the current planet **Stellar-BX**, the available next destination **Stella-Z2**, and eight other destinations marked **Coming soon**. The user approved temporary fuel-free travel in both directions. Inventory and unlocks carry over, and each planet retains its factories and exploration for the session. Fuel production and consumption still need design.

The whole landing site is **4 × 4 tiles, one block thick**, with its top flush with the existing build surface. The rocket and dock occupy tiles (0,1) and (2,1) beside the pod at (0,0). Those two footprints must be clear before the expansion delivery succeeds; other equipment can remain on the asphalt. The upgrade never removes existing machines or their contents.

Splitters, mergers, upgraded conveyors, foundries, refineries, manufacturers, and later processing machines are not assigned phases in this replacement plan. Preserve existing Creative tools; do not silently add extra Survival unlocks.

### Material and construction recipes

The ingredients and machine assignments below come from the user. Where a quantity was omitted, **one input item and one output item per craft** is the implemented default. Nuts and bolts explicitly yield four items.

| Output | Process | Ingredients | Output quantity |
| --- | --- | --- | ---: |
| Concrete | Assembler | 2 stone + 1 limestone | 1 |
| Iron ingot | Smelter | 1 iron ore | 1 |
| Copper ingot | Smelter | 1 copper ore | 1 |
| Iron sheet | Constructor | 1 iron ingot | 1 |
| Nuts and bolts | Constructor | 1 iron ingot | **4 confirmed** |
| Cable | Constructor | 1 copper ingot | 1 |
| Glass | Smelter | 1 sand | 1 |
| Coal generator | Equipment construction | **80 nuts and bolts + 20 iron sheets + 2 Miner Mk1 items + 4 copper ingots** | 1 generator |
| Power pole | Equipment construction | **20 nuts and bolts + 4 iron sheets + 2 glass** | 1 pole |

The generator consumes **two crafted Miner Mk1 items**, as clarified by the user. It must not silently consume miners already placed in the world. The old inexpensive generator and conductive-alloy pole costs are superseded by these recipes.

The previously approved journal recipe **1 iron ingot + 1 copper ingot → 1 conductive alloy ingot** remains part of the design. Its use in the new progression is unassigned; it is no longer an ingredient in the pole recipe.

Approaching an assembler and pressing **E** opens its interface to select the output recipe. Retain manual loading and output collection for early factories. A recipe's ingredient counts are separate from its number of input types: concrete uses two stone items and one limestone item across the assembler's two ingredient types.

All production machines share a **100-item total buffer**, including inputs and outputs. Storage retains **16 stacks of 100**. Constructors must reserve enough capacity for an entire output batch: one iron ingot becoming four nuts and bolts increases the stored item count by three. Pause production if the resulting total would exceed 100; do not discard or partially create a batch. The player inventory's capacity has not been specified. Conveyors have no storage buffer: each tile carries one item in transit. Splitters and mergers hold **10 items** each. Splitters have one rear input and three outputs (forward, left, right); mergers have three inputs (rear, left, right) and one forward output. Splitters share items between available outputs; mergers give waiting inputs fair turns. Blocked routes retain their items.

### Approved starting-craft rules

The approved manual crafting path resolves the first-machine dependency:

1. **First power connection:** poles require iron sheets, nuts and bolts, and glass before the Constructor unlocks. Powered smelting also needs that first pole. At Tier 1 Phase 2, unlock manual journal crafting at the landing-pod workbench for iron ingots, copper ingots, glass, iron sheets, nuts and bolts, and cable. Use the same ingredient ratios as the later machines. The Constructor at Tier 2 Phase 1 then automates the existing sheet, fastener, and cable recipes. Concrete remains locked to the assembler.
2. **Gathering before the inventory screen:** keep gathered materials in the existing carried-material stock from Phase 1, with counts and deliveries accessible in **J**. Phase 3 unlocks the dedicated **I** inventory interface and storage. Opening that interface must preserve everything already gathered.

With these ratios, one pole needs **9 iron ore + 2 sand** through the manual recipes: five iron ingots become 20 nuts and bolts, four become sheets, and two sand become glass. Two cables additionally need two copper ore. This gives a complete path from hand gathering to a wired smelter without relying on a locked machine.

Retain the existing eight-power landing pod and wiring rules unless revised: five cables per pole, one connection per machine/source, and independently checked networks. A successful Survival connection consumes **one cable item**. Failed attempts consume nothing; cable length does not affect this initial cost.

### Approved phase deliveries

These approved initial balancing values are implemented in the prototype. A delivery is paid to **enter** its target phase; the associated unlock then becomes available. Tier 1 Phase 1 requires no delivery. Ordinary equipment still has its separate build cost. The landing-site and rocket milestones construct their associated project as part of the delivery, without charging the same project materials twice.

| Enter phase | Material delivery | Available production before delivery |
| --- | --- | --- |
| Tier 1 Phase 1 | None — unlocked by default | Manual gathering |
| Tier 1 Phase 2 | 12 iron ore + 8 copper ore + 8 stone | Manual gathering |
| Tier 1 Phase 3 | 16 iron ingots + 8 copper ingots + 4 glass | Phase 2 smelting or journal crafting |
| Tier 1 Phase 4 | 12 iron sheets + 40 nuts and bolts + 8 cables | Phase 2 journal crafting |
| Tier 2 Phase 1 | 20 iron sheets + 80 nuts and bolts + 16 cables + 8 glass | Existing smelters and journal crafting |
| Tier 2 Phase 2 | 80 concrete + 40 iron sheets + 80 nuts and bolts | Assembler and Constructor unlocked at Tier 2 Phase 1 |
| Tier 2 Phase 3 | 160 iron sheets + 240 nuts and bolts + 80 cables + 40 glass | Automated component production |
| Tier 2 Phase 4 | 200 iron sheets + 320 nuts and bolts + 120 cables + 80 glass | Automated component production |

The larger rocket deliveries encourage automation while using only the specified material chains. Quantities above a machine's 100-item buffer require multiple collection cycles or storage; they do not increase machine capacity. Fuel is separate from rocket construction and excluded until its recipe and launch amount are decided. Silver ore and conductive alloy are also excluded from delivery requirements while their uses remain unassigned.

### Implementation boundary

The prototype implements both tiers through the completed rocket and its destination interface. Creative also exposes the splitter and merger. The fuel INPUT dock identifies the future connection point but does not accept materials yet. Stella-Z2 and fuel-free return travel are now playable. Lunar recipes and phases, rocket fuel, and disk saves remain deferred. Factories on Earth and the Moon advance on the same clock, with separate power networks and inventories, whether the player is on either planet or in flight.

The spaceship is the first large factory project. Building its hull, systems, and fuel should feel like the result of the player's production lines rather than a simple crafting recipe.

## Stella-Z2 — Moonlike second world

The moonlike world is **always nightlike**. Its black sky, cold ground, and visible factory lights create a different mood from Earth's day-and-night cycle. Neutral moonlight and the powered rocket keep arrival readable; a suit light and floodlights remain future design options. Nodes and placement indicators must remain readable in the dark.

The moon's opening challenge is to establish a powered outpost using supplies carried in the rocket, then develop local production. Earlier ideas for its progression, still unassigned to tiers or phases, are:

1. Establish a lunar factory and lighting.
2. Extract and process the moon's progression resources.
3. Automate shipping between Earth and the moon.
4. Build the upgrade needed to reach the third world.

The approved and implemented surface is gray, with **six regions from landing in each direction** (13 × 13 / 169 regions, 15 × 15 tiles each). Most regions are empty and the remainder ordinarily have one deposit. It contains only **Amorium** (beige), **Moondust** (gray-white), and **Techtorium** (orange, black, and white). Two distant, distinct Techtorium sites are guaranteed on every seed. One Amorium and one Moondust deposit occur near landing. All three support manual gathering, miners, conveyors, storage, and player inventory.

Recipes, tier count, and phase unlocks are not yet defined. The permanent darkness should shape its atmosphere and factory planning without making routine building hard to see. Any local power source and the supplies guaranteed on first landing must be designed so the player cannot become stranded.

## Interplanetary logistics

The spaceship has a storage container and can carry materials between worlds. Earth factories remain useful after the first launch: the player can send Earth-made parts outward and return with materials available elsewhere. Cargo capacity, launch costs, route automation, and the persistence of factories while the player is away still need rules.

An **interdimensional portal** is a possible later transport system, reserved for future content. Rockets are the initial way to move goods between worlds.

## Story direction

The central mystery begins with the player's solitude on Earth and their attempt to reach other people. Clues can appear as the player advances without resolving the mystery in the first two worlds.

**Future-content ending:** On the final world, the player learns that the people they were trying to reach are dead; they had gone to the sun. The circumstances and explanation are not yet defined. This reveal is a long-term story plan, not part of the initial two-world build.

## Bozzard feasibility

**Yes, this is buildable as a staged Bozzard game.** The first playable version should cover Stellar-BX, randomized nodes, basic factory machines, its two tiers, one rocket, and arrival on Stella-Z2. Bozzard already has 3D scenes and orthographic cameras, imported meshes and prefabs, UI, lighting, save/load, scene transitions, and [Rhai scripting](scripting.md). Voxel *style* can use modular 3D assets without a destructible voxel-terrain system. The game now uses bounded data-only saves on top of the engine's runtime, with atomic file writes and validation before loading.

The existing [Bozz-torio](../apps/bozz-torio/README.md) game demonstrates seeded node placement, conveyors, machine production, power, tier-and-phase progression, and saves in **2D**. Its factory simulation is native game code rather than Rhai, so it is evidence for the gameplay approach, not a ready-made 3D implementation.

Rhai is suitable for prototyping phase requirements, unlocks, machine interactions, clocks, UI state, and rocket events. A large world with many machines and visible moving items will likely need a dedicated, efficient game simulation and rendering approach. Cross-world factory state and cargo saves also need an explicit design. The Earth day-and-night cycle may require a small Bozzard extension or native game code for a moving global sun: the current documented Rhai API can adjust some lighting and display values but does not expose sun direction. Bozzard also limits local lights, so a moon factory full of floodlights should use emissive visuals and selective dynamic lighting rather than one shadow-casting light for every fixture.

These are implementation tasks, not blockers to the concept. Performance and save behavior should be checked with an Earth factory prototype before expanding to ten worlds.

## Decisions still to make

- Set a player carrying-capacity limit, if desired; the initial inventory retains aggregate material counts.
- Define coal generator fuel behavior and rocket fuel production and launch requirements.
- Assign silver ore and conductive alloy uses; review build costs for machines without newly specified recipes.
- The moon's recipes, tiers and phases, and route to world three; its three resource types and bounds are now implemented.
- The identities and progression roles of the remaining eight worlds, including how gas worlds are visited or exploited.
- Factory building rules: grid size, vertical construction, conveyor routing, and node richness or depletion.
- Rocket cargo capacity, fuel cost, travel time, and when routes become automatic.
- Earth day length, moon lighting and power rules, and whether other worlds have distinct sky cycles.
- The pace and form of story clues before the final reveal.
