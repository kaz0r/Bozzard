# Mk1 machine models

Smelter and miner modeled from the project's supplied 2D sketches. The smelter
has a through-belt furnace, overhead heating strips and a four-legged stand.
The miner has a tall vented motor housing, an exposed helical drill and an
output chute. The constructor has one press and a straight feed deck; the
assembler has two tooling heads and two incoming decks. The coal generator
shares the miner’s exposed auger, with a combustion housing, twin exhaust stacks
and side alternator. All five powered
machines have a small red/off or green/on indicator.

The low splitter and merger models have four belt mouths with directional
arrows: cyan marks an inlet, gold an outlet. The splitter has one rear inlet
and three outlets; the merger has three inlets and one forward outlet. Their
existing 10-item buffers and routing behavior are unchanged.

Storage has a raised bin over a feed tunnel, a cyan rear input and a gold
forward output. The roller-bed conveyor shares its deck height with every
transport port. Power poles have anchored feet, braced arms, ribbed insulators
and a lamp cage. Cable sockets and runtime lights follow the modeled fittings.

- `mk1-machines.blend`: editable, individually named parts and studio setup.
- `preview.png`: Blender studio render, shown with power on.
- `engine-day.png` and `engine-night.png`: original six-machine native-engine previews.
- `new-models-engine-day.png` and `new-models-engine-night.png`: storage, conveyor,
  generator and pole in Dev World, with working generator/pole lighting.
- Game meshes: `examples/earth-factory/scenes/assets/models/*-mk1.glb`.

Regenerate from the repository root:

```sh
blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_machines.py
python3 examples/earth-factory/tools/generate_scene.py
```

The GLBs are Y-up, in tile units, with output facing +X and an origin at the
ground plane. Each fits one tile and uses three to five material surfaces.
Export logs report triangle counts (under 2,500 per machine). The Blender
source retains named parts and a collection per machine; exported static
parts are grouped by material. Drill and tooling are modeled geometry without
independent production animations. The existing whole-machine rotation
animation remains supported.

Runtime indicator lenses and heater strips are separate emissive prefabs, driven
by the actual power graph in `factory/power.rhai`. A powered smelter keeps its
heater warm even while idle. Disconnecting or overloading the circuit turns the
heater off and its status light red. Nearby machines share the existing 32-light
pool with poles and rocket lights, so adding machines cannot grow the local-light
budget. Model rotation, chunk residency and Steam replicas reuse the same effects.

## Mk1 expansion pack

Fourteen additional production-machine models share the original graphite enamel,
machined steel, dark recesses, cyan inputs and gold outputs. Their silhouettes
identify the process even at the isometric game camera distance.

| Model | Distinguishing geometry |
| --- | --- |
| Water pump | Centrifugal impeller, finned motor and raised outlet pipe |
| Oil extractor | Pumpjack walking beam, horsehead and counterweight |
| Crusher | Open tapered hopper and twin crushing rollers |
| Ore washer | Banded wash drum, spray header and drain tray |
| Foundry | Open alloy crucible, casting spout and extraction canopy |
| Refinery | Distillation tower, tray bands, ladder and separator vessel |
| Chemical plant | Agitated reaction vessel and separate reagent tank |
| Electrolyzer | Exposed electrode stack and twin gas reservoirs |
| Kiln | Octagonal refractory tunnel and short chimney |
| Glassworks | Melting furnace, forming rollers and glass sheet |
| Greenhouse | Pitched glazed roof, growing trays and climate controls |
| Electronics fabricator | Filtered enclosure and fine circuit placement heads |
| Manufacturer | Heavy gantry, twin robot arms and three component feeds |
| Recycler | Open twin-shaft shredder and separation roller |

- `mk1-expansion-machines.blend`: editable named parts, one collection per machine,
  studio camera, lighting and preview labels. Collections are positioned on the
  studio sheet; exported model pivots remain at the local ground origin.
- `expansion-preview.png`: labeled isometric studio sheet.
- `expansion-engine.png`: capture through Bozzard's native renderer.
- `expansion-manifest.json`: model paths, bounds, triangle counts, surface counts,
  cable attachment, indicator position and solid/fluid port metadata. Coordinates
  use the game's X/Y(up)/Z axes. Runtime cable sockets, lamps and heat strips
  follow these physical attachments; solid and fluid routing follows their sides.
- Game meshes and prefabs: `examples/earth-factory/scenes/assets/`. Each GLB is
  self-contained, grounded, Y-up, at unit scale, and fits within one tile. Output
  faces +X. Conveyor mouths share the existing 0.34-unit deck height. The models
  use 1,264–3,504 triangles and four to seven material surfaces, without textures.
- `examples/earth-factory/scenes/machine-expansion-showroom.json`: independent
  editor/player art showroom. All fourteen meshes, preview lamps and heat strips
  appear together. Open it with `bozzard-editor --scene PATH`.

Every machine has a modeled power socket and bezel. The separate `*-power-on`
and `*-power-off` prefabs match the existing indicator convention; the foundry,
kiln and glassworks also have separate `*-heat` prefabs. Static mesh exports have
no baked emission. The greenhouse adds subdued foliage and smoky transparent
glazing while retaining the same metal frame and industrial palette.

All fourteen machines are playable in the main Earth scene and Dev World.
Placement, power, recipe selection, ingredient loading, production, byproducts,
solid/fluid transport, saves and host-authoritative co-op use the normal game
systems. Survival unlocks them at Component automation; Creative unlocks them
immediately. See the game's README for controls, recipes and fluid port rules.
The separate machine parts remain static during production; whole-machine
rotation and power/heat indicators are animated by gameplay.

`gameplay-expansion-manufacturer.png`, `gameplay-expansion-refinery.png` and
`gameplay-dev-world.png` show the implemented machines in the native renderer.

Regenerate and validate from the repository root:

```sh
blender --background -noaudio --threads 4 --python examples/earth-factory/tools/generate_expansion_machines.py
python3 examples/earth-factory/tools/validate_machine_expansion.py
```

Add `-- --no-render` to the Blender command to export assets and source without
rendering the preview. The validator checks all fourteen self-contained GLBs,
indices, finite vertex data, unit normals, geometry budgets, grounded bounds,
unit-scale pivots, prefab references and matching power-lens coordinates.

## Pipes and corner conveyors

The logistics pack adds a straight pressure pipe, a 90-degree elbow pipe, and
left/right 90-degree conveyor models. Pipes share the machines' 0.34-unit fluid
port height, with real hollow bores, steel couplings, saddle supports and a cyan
fluid band. Their two tile-edge ports are bidirectional; rotating the elbow
covers all four adjacent-side combinations. No extra powered equipment is added.

Corner belts retain the straight conveyor's 0.46-unit bed width, 0.305-unit deck
center, 0.34-unit tread height, steel rails and gold travel arrows. Radial treads
follow a quarter-circle deck. Both variants output at +X: the left turn enters
from -Z, and the right turn enters from +Z. Separate mirrored models keep a
positive unit scale and correct normals when rotated.

- `mk1-logistics.blend`: editable named parts and the four-model studio.
- `logistics-preview.png` and `logistics-engine.png`: studio and native renders.
- `logistics-manifest.json`: bounds, triangle budgets, port positions and facing.
- GLBs: `pipe-straight-mk1.glb`, `pipe-elbow-mk1.glb`, `belt-turn-left-mk1.glb`
  and `belt-turn-right-mk1.glb` in the game's existing models directory.
- Matching `machine-*.prefab.json` files are in the game assets directory.
- `examples/earth-factory/scenes/logistics-showroom.json`: independent art scene.

Both pipe shapes and corner belts are playable from the Logistics bar. Pipes
carry one fluid at a time with a 100-unit buffer and conservative pressure flow;
corner belts carry one solid item along their modeled curve. All four rotations,
backpressure, region seams, off-planet simulation, saves and co-op are supported.

```sh
blender --background -noaudio --threads 12 --python examples/earth-factory/tools/generate_logistics_models.py
python3 examples/earth-factory/tools/validate_logistics_models.py
```
