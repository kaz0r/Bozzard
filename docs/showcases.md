# Lighting and material showcases

These scenes use checked-in assets; no Sponza download is required.

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/material-gallery.json
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/neon-gallery.json
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/shader-lab.json
```

## Material Gallery

Eight imported glTF PBR exhibits compare polished/satin/matte gold, copper,
textured walnut, textured marble, red lacquer and soft-touch plastic. Wood and
stone include base-color, normal and metallic/roughness maps. Coatings use core
PBR, not a separate clearcoat layer. Subtle distance and ground fog give depth;
disable fog to compare materials without atmospheric tint.

Select an exhibit, expand its imported surface row and adjust the material
factor overrides in the Inspector. Choose **Select whole model** to transform
it again. The gallery assets are original CC0 procedural assets, generated with
Python's standard library:

```sh
python3 examples/demo/scenes/assets/material-gallery/generate.py --check
# Omit --check to regenerate the assets and scene.
```

## Neon Gallery

Amber/cyan point lights, a violet spot and a directional fill illuminate standard
materials alongside normals, checker and toon exhibits. Press **Play** to rotate
the shader sculptures. Expand a pedestal in the Hierarchy to select its child.
Toggle each light's **Enabled** control to isolate its contribution. Directional
lights aim down local −Z and ignore distance; point/spot lights have finite range.
The scene sun casts shadows; spotlights can opt into **Cast shadows** (up to eight
1024 px maps), and point lights can opt in independently (up to four six-face
512 px maps). Object-directional lights remain unshadowed. The Neon preset keeps
point-light shadows opt-in unless its scene JSON enables them.

The demonstration effects deliberately replace full material shading: normals
show orientation, checker shows UVs, and toon uses scene sun direction/shadows.
Use **White** in **Texture / material effect** to restore ordinary PBR lighting.
Bloom and FXAA run in the shared display path; raw diagnostics bypass them.

## Fog

Open **Scene lighting → Fog (3D)** in the Inspector. Distance density controls
uniform haze after Start distance; Height density adds a ground layer below
Base height and decays above it according to Height falloff. Both layers can
be combined; zero densities leave surfaces unchanged. Settings save with the
scene and support Undo/Redo. Fog is disabled by default in older scenes.

This is analytic surface fog, not volumetric light shafts: it does not scatter
individual lights or alter the sky background. Distances start at the camera
near plane so perspective and orthographic views both work. Fog preserves
surface alpha, is applied before bloom/display mapping, and is disabled for 2D
and raw diagnostic rendering.

## Transform controls

- Click a model to select its owner; **Alt-click** inspects a source surface.
- Hover the idle viewport and press **W / E / R** for Move / Rotate / Scale.
- Drag arrows to move, colored rings to rotate, or axis squares to resize.
- In Scale mode, drag the white **All** center up/right to enlarge uniformly,
  down/left to shrink. Mirrored axes keep their sign.
- **Snap** sets increments; hold **Ctrl** to invert snapping temporarily.
- **Escape** cancels a drag; completed drags are one undoable action.

Tool shortcuts do not intercept typing, Play or fly-navigation movement. Source
surfaces remain material-selection entries, not independently editable scene
transforms; authored child objects can be transformed normally.

## Validation

```sh
cargo test --workspace
cargo run -p bozzard-player -- --scene examples/demo/scenes/material-gallery.json \
  --smoke --hardware --output work/material-gallery-check
```

On Linux a working Vulkan driver is required. For Arch with Intel graphics,
install `vulkan-intel`; `vulkaninfo --summary` from `vulkan-tools` checks discovery.
