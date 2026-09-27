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
