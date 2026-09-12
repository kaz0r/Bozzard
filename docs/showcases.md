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

## Light Shafts Lab

Open `examples/demo/scenes/light-shafts-lab.json` for warm, shadowed beams entering a stone room through tall windows. Play moves the density field. Use **Effects → Fine tuning → Post Processing → Volumetric fog & light shafts** to tune it; see [volumetric fog](volumetrics.md) for controls and limitations.

## Lens Lab

Open `examples/demo/scenes/lens-lab.json` for a gold sculpture against rows of distant warm lights. Play slowly shifts focus between the sculpture and lights using a Blueprint. The scene also uses bounded eye adaptation; see [camera effects](camera-effects.md) for controls and rendering details.

## Fog

Open **Scene Settings → Fog (3D)**. Distance density controls
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

- Click imported geometry to inspect its surface; **Alt-click** selects its owner.
- Hover the idle viewport and press **W / E / R** for Move / Rotate / Scale.
- Drag arrow tips/shafts to move, colored rings to rotate, or axis squares/shafts to resize.
- Gizmos stay visible while right-dragging or flying; navigation disables editing, not drawing.
- In Scale mode, drag the white **All** center up/right to enlarge uniformly,
  down/left to shrink. Mirrored axes keep their sign.
- **Snap** sets increments; hold **Ctrl** to invert snapping temporarily.
- **Escape** cancels a drag; completed drags are one undoable action.

Tool shortcuts do not intercept typing, Play or fly-navigation movement. Imported
surfaces have per-instance transforms around their source bounds center, plus texture,
UV repeat and material overrides in Properties. Source assets stay unchanged;
components and colliders remain attached to the owner. See [submesh editing](assets.md#editing-a-submesh).

## Validation

```sh
cargo test --workspace
cargo run -p bozzard-player -- --scene examples/demo/scenes/material-gallery.json \
  --smoke --hardware --output work/material-gallery-check
```

On Linux a working Vulkan driver is required. For Arch with Intel graphics,
install `vulkan-intel`; `vulkaninfo --summary` from `vulkan-tools` checks discovery.

## Atmosphere and motion

Open `examples/demo/scenes/atmosphere-lab.json` for the bonfire clearing with curling smoke, windblown ash and sparks, temporal anti-aliasing, motion blur, and wet ground reflections. The Effects panel has live preview, common controls and application shortcuts. See [atmosphere effects](atmosphere-effects.md).
