# Isometric voxel factory game — design brief

*Working concept, recorded 2026-09-25. The game has no final title yet.*

The first [Earth Factory Prototype](../examples/earth-factory/README.md) now runs inside Bozzard editor Play. It demonstrates randomized nodes, a baked voxel-style ground mesh, basic machines, and working iron and copper lines feeding an assembler. The later tiers, rocket, and other worlds remain design goals.

## Core idea

A single-player, isometric 3D factory game with a voxel visual style. The player explores worlds, builds factories on resource nodes, manufactures increasingly complex parts, and constructs spaceships to reach new worlds. The initial inspiration is the sense of planetary exploration in *Astroneer* combined with the production chains of *Satisfactory*. Destructible or sculptable terrain is **not** part of this concept.

The game begins on an Earthlike world. The player soon realizes they are alone: there are no other people there. The first major objective is to build a spaceship rather than start with one. Doing so requires a functioning factory, a launchpad, and fuel.

Factory equipment includes miners, conveyors, splitters, mergers, assemblers, improved assemblers, manufacturers, storage containers, power generation, and resource-specific processing machines. Resource nodes determine where extraction can happen; the player designs the transport and production lines between them.

## World structure and progression

- The intended game has **ten distinct worlds**, beginning with an Earthlike world and then a moonlike world. Later destinations may include gas worlds, worlds rich in nodes, and worlds with scarce nodes. The remaining worlds are future design work.
- Every world provides something needed to progress. The Earthlike world must **always** contain every resource required to build and fuel the first spaceship.
- Each world has **four tiers**, each with **four phases**: 16 progression milestones per world. A phase requires delivery of manufactured goods and unlocks a machine, upgrade, logistics tool, process, or major project. The fourth phase of a tier is its larger milestone.
- Unlocks persist between worlds. Arriving on the moon does not mean relearning miners, conveyors, and assemblers; its phases introduce new challenges and technology.
- Production should move from hand-gathered starter materials to continuous automated lines, then to cross-world supply chains.

### Procedural resource placement

Node locations must change between playthroughs. A player should not be able to memorize where a useful node will appear in the next game. World generation should use a saved seed so the layout stays stable within one playthrough.

Random placement needs progression guarantees: required node types must exist, be reachable, and support the manufacturing and fuel needed to leave that world. On Earth, basic nodes should be reasonably accessible from the landing area, while later resources can require an outpost or a longer conveyor route. Node positions and richness can vary without producing an unwinnable seed.

## Earthlike starter world

Earth has an atmosphere broadly like Earth's. It has a familiar sky, clouds and sunsets, with a day-and-night cycle. Factories continue to run at night; their lighting reinforces the feeling that the world is empty. The exact cycle length and any effects on production remain undecided.

The proposed starter resource set is:

| Node or source | Early purpose | Later purpose |
| --- | --- | --- |
| Iron ore | Basic parts, miners, conveyors, machine frames | Steel and spaceship structure |
| Copper ore | Wire and power connections | Electronics |
| Limestone | Foundations and concrete | Launchpad |
| Coal | Sustained factory power | Steel production |
| Quartz | Glass or silica | Circuit components |
| Crude oil | Plastic and machine parts | Spaceship components |
| Water | Processing | Hydrogen and oxygen for rocket fuel |

The exact recipes and quantities still need balancing. The intended dependencies are clear: iron and coal enable steel; copper and processed quartz enable electronics; oil enables plastic; and water supports the first rocket's fuel chain.

### Earth tier and phase plan

The landing pod supplies hand tools, a crafting bench, and limited initial power. Phase deliveries are produced with equipment already available before that phase, avoiding circular unlocks.

| Tier | Phase | Unlock | Purpose |
| --- | ---: | --- | --- |
| **1 — First factory** | 1 | Smelter | Process hand-mined iron and copper. |
| | 2 | Small generator and power poles | Power machines beyond the landing pod. |
| | 3 | Miner Mk1 | Automate extraction from resource nodes. |
| | 4 | Conveyor Mk1 and storage container | Form a continuous production line. |
| **2 — Automation** | 1 | Assembler Mk1 | Combine two inputs into machine parts. |
| | 2 | Splitter | Feed multiple lines from one supply. |
| | 3 | Merger | Bring multiple lines together. |
| | 4 | Coal generator and Conveyor Mk2 | Scale power and item transport. |
| **3 — Advanced materials** | 1 | Foundry | Produce steel from iron and coal. |
| | 2 | Oil extractor and refinery | Produce plastic and other oil products. |
| | 3 | Quartz processor | Produce silica for electronics. |
| | 4 | Assembler Mk2 | Produce more complex, three-input parts. |
| **4 — Space program** | 1 | Manufacturer | Produce major spaceship components. |
| | 2 | Shipyard and launchpad | Assemble a visible spaceship. |
| | 3 | Water extractor and electrolyzer | Produce and store rocket fuel. |
| | 4 | Launch control | Launch the ship and unlock the moon. |

The spaceship is the first large factory project. Building its hull, systems, and fuel should feel like the result of the player's production lines rather than a simple crafting recipe.

## Moonlike second world

The moonlike world is **always nightlike**. Its black sky, cold ground, and visible factory lights create a different mood from Earth's day-and-night cycle. A suit light and the rocket provide enough visibility on arrival; floodlights can be an early moon unlock. Nodes and placement indicators must remain readable in the dark.

The moon's opening challenge is to establish a powered outpost using supplies carried in the rocket, then develop local production. A first-pass structure for its four tiers is:

1. Establish a lunar factory and lighting.
2. Extract and process the moon's progression resources.
3. Automate shipping between Earth and the moon.
4. Build the upgrade needed to reach the third world.

Its exact nodes, recipes, and 16 phase unlocks are not yet defined. The permanent darkness should shape its atmosphere and factory planning without making routine building hard to see. Any local power source and the supplies guaranteed on first landing must be designed so the player cannot become stranded.

## Interplanetary logistics

The spaceship has a storage container and can carry materials between worlds. Earth factories remain useful after the first launch: the player can send Earth-made parts outward and return with materials available elsewhere. Cargo capacity, launch costs, route automation, and the persistence of factories while the player is away still need rules.

An **interdimensional portal** is a possible later transport system, reserved for future content. Rockets are the initial way to move goods between worlds.

## Story direction

The central mystery begins with the player's solitude on Earth and their attempt to reach other people. Clues can appear as the player advances without resolving the mystery in the first two worlds.

**Future-content ending:** On the final world, the player learns that the people they were trying to reach are dead; they had gone to the sun. The circumstances and explanation are not yet defined. This reveal is a long-term story plan, not part of the initial two-world build.

## Bozzard feasibility

**Yes, this is buildable as a staged Bozzard game.** The first playable version should cover Earth, a small randomized node field, basic factory machines, the four Earth tiers, one rocket, and arrival on the moon. Bozzard already has 3D scenes and orthographic cameras, imported meshes and prefabs, UI, lighting, save/load, scene transitions, and [Rhai scripting](scripting.md). Voxel *style* can use modular 3D assets without a destructible voxel-terrain system.

The existing [Bozz-torio](../apps/bozz-torio/README.md) game demonstrates seeded node placement, conveyors, machine production, power, tier-and-phase progression, and saves in **2D**. Its factory simulation is native game code rather than Rhai, so it is evidence for the gameplay approach, not a ready-made 3D implementation.

Rhai is suitable for prototyping phase requirements, unlocks, machine interactions, clocks, UI state, and rocket events. A large world with many machines and visible moving items will likely need a dedicated, efficient game simulation and rendering approach. Cross-world factory state and cargo saves also need an explicit design. The Earth day-and-night cycle may require a small Bozzard extension or native game code for a moving global sun: the current documented Rhai API can adjust some lighting and display values but does not expose sun direction. Bozzard also limits local lights, so a moon factory full of floodlights should use emissive visuals and selective dynamic lighting rather than one shadow-casting light for every fixture.

These are implementation tasks, not blockers to the concept. Performance and save behavior should be checked with an Earth factory prototype before expanding to ten worlds.

## Decisions still to make

- The moon's resource list, complete 16-phase plan, and route to world three.
- The identities and progression roles of the remaining eight worlds, including how gas worlds are visited or exploited.
- Factory building rules: grid size, vertical construction, conveyor routing, and node richness or depletion.
- Rocket cargo capacity, fuel cost, travel time, and when routes become automatic.
- Earth day length, moon lighting and power rules, and whether other worlds have distinct sky cycles.
- The pace and form of story clues before the final reveal.
