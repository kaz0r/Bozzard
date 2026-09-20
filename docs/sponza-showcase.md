# Sponza — The Gilded Hour

A warm atrium showcase built with the engine's existing renderer: textured stone
and woven banners, baked diffuse bounce, shadowed sunlight, a little haze, and a
slowly rotating bronze armillary with emissive inlays. Six bronze lanterns lead
the eye through the courtyard. This is a separate scene; the original Sponza
fixtures are unchanged.

## Open it

From the repository root:

```sh
python3 tools/download_sponza.py
cargo run --release --locked -p bozzard-editor-app -- \
  --scene examples/sponza/showcase.json --hardware
```

The downloader reuses verified files if Sponza is already installed. The original
architecture and textures stay under ignored `work/sponza/`; see the
[source and licensing notes](sponza.md). The small armillary meshes are original,
checked-in assets with their own [CC0 notice](../examples/sponza/assets/gilded-hour/README.md).

Press **Play** for the 7°/second armillary rotation, gently breathing emission,
and sparse airborne dust. **Stop** restores the authored scene. Right-drag or
press **Tab** in Edit mode to fly; use **WASD**, **Space/Ctrl** to rise/fall,
and **Shift** to move faster. The viewport's **View → Reset view** restores the
active scene camera. Hide **Content Browser** and **Scene Settings** from the
main **View** menu for a larger viewport.

For a clean presentation without editor panels:

```sh
cargo run --release --locked -p bozzard-player -- \
  --scene examples/sponza/showcase.json --hardware
```

The player starts the animation immediately. **Space** pauses, **R** reloads,
and **Escape** closes it.

## Three authored cameras

| Camera ID | Composition |
| --- | --- |
| `camera` | Default hero view down the sunlit atrium |
| `camera-detail` | Bronze sculpture, embossed cloth and nearby stone |
| `camera-overlook` | Elevated view across the galleries and courtyard |

In the editor, select the camera in Hierarchy and choose **Camera → Use for 3D**,
then **View → Reset view** in the viewport. The sculpture inlays are independent
editable entities. Select an inlay child and
open the **Shader** workspace to inspect its Time → Sine → Emissive graph.

## Visual choices

- Warm sun and cool sky, a 4096 directional shadow map, and a saved 1,280-probe
  GI bake (256 rays per probe, three diffuse bounces).
- Low-density volumetric haze and small dust flecks; the stone and cloth remain
  readable. Only the two nearest lanterns cast point-light shadows.
- Subtle screen-space reflections on the courtyard paving, with a rough plinth
  to keep reflections from overwhelming the sculpture.
- Fixed exposure, restrained bloom and mild color grading. Motion blur, depth
  of field, temporal AA, heat shimmer, film grain and auto exposure are off.

Reflections have the engine's normal screen-space limitations, particularly
around thin geometry and objects outside the frame. The emissive shaders add
visible radiance; separate point lights illuminate the surrounding architecture.
Animated objects and shader surfaces do not contribute to the static GI bake.
Sponza stays a whole-model drawable: the current GI baker's triangle budget counts
the full source mesh for each independent surface, so splitting this large model
into entities exceeds that budget. Keep the architecture intact when rebaking.

## Reproduce a screenshot

The capture example uses the player's actual render extraction and native GPU
backend. It writes an unretouched PPM at the specified simulation tick; 240 ticks
is four seconds. The final optional argument selects a camera.

```sh
cargo run --release --locked -p bozzard-player --example capture_scene -- \
  examples/sponza/showcase.json work/sponza-showcase/hero.ppm 1920 1200 240
cargo run --release --locked -p bozzard-player --example capture_scene -- \
  examples/sponza/showcase.json work/sponza-showcase/detail.ppm 1600 1000 240 camera-detail
```

On macOS, `sips -s format png INPUT.ppm --out OUTPUT.png` converts the capture
losslessly for sharing. Captures stay in ignored `work/`.

After editing static geometry, materials or lighting, rebuild GI through
**Scene Settings → Global illumination → Bake**, or run:

```sh
cargo run --release --locked -p bozzard-editor --example bake_gi -- \
  examples/sponza/showcase.json examples/sponza/showcase.json
python3 tools/gen_sponza_showcase_assets.py --check
```

The bake command verifies that the saved result is current after reopening.
Camera and display adjustments do not invalidate the bake. The mesh generator
uses Python's standard library; omit `--check` to regenerate the two meshes.
