# Bozzard university dorm

## Engine scene and glTF

Open `examples/demo/scenes/bozzard-dorm.json` in Bozzard. The room is placed at
ground level with an orthographic camera, daylight, desk lighting, pink wall
spill, and HDR bloom enabled for the Bozzard sign. Both dog pictures are included.

```sh
cargo run -p bozzard-editor-app -- --scene examples/demo/scenes/bozzard-dorm.json
```

`bozzard_dorm.gltf` uses the adjacent `bozzard_dorm.bin`, `dorm_basecolor.png`,
`dorm_normal.png`, `dog_portrait.png`, and `dog_photo.png`. Keep these six files
together. `bozzard_dorm.glb` is the same
model packaged as one file. The model uses Y-up coordinates, one unit per meter,
28 material surfaces, and about 239,000 triangles. It excludes the Blender studio
ground, cameras, and lights. The scene supplies those presentation settings.

Procedural materials are baked into 4096px base-color and normal atlases. Both
pictures keep their original image textures and UVs for full detail. The sign
retains emission strength 6 through the standard
`KHR_materials_emissive_strength` extension. Bloom is a renderer post effect,
configured in the Bozzard scene; other glTF viewers must enable their own bloom.

To regenerate both exports without changing the authored Blender file:

```sh
blender -b assets/dorm_room/bozzard_dorm.blend --python assets/dorm_room/export_room.py
```

To capture the actual engine scene and a bloom-disabled comparison:

```sh
cargo run -p bozzard-player --example capture_scene --no-default-features -- examples/demo/scenes/bozzard-dorm.json work/dorm-room
```

`bozzard_dorm_engine.png` is the verified in-engine view with bloom enabled.

## Blender source

Open `bozzard_dorm.blend` in Blender 5.2 or newer. The scene is a furnished,
stylized dorm shown as an open-sided cutaway, with an orthographic hero camera.

Includes a closed paneled door, bed with draped bedding, skateboard, glowing
“Bozzard” neon lettering, six-string acoustic guitar, open laptop and study desk,
and an open woven laundry basket filled with clothes. Additional details include
a window, radiator, books, pinboard, desk lamp, plants, backpack, sneakers, phone,
and a woven rug. A framed painting of the supplied dog artwork hangs on the left
wall beside the window. The second supplied dog photograph hangs in a matching
frame below the neon sign, above the headboard.

All geometry and materials are editable and organized in named collections.
Both dog picture textures are packed inside the Blender file; no external textures
are required to open or render it. All other materials are procedural.
The second camera provides a closer interior composition. The front and right
walls and ceiling are deliberately omitted for the cutaway presentation.

`bozzard_dorm_preview.png` is the rendered hero view.

To rebuild the scene and render with Blender installed:

```sh
blender -b --python assets/dorm_room/create_room.py
```

The script writes the `.blend` and preview beside itself. Rendering uses Cycles,
48 samples, denoising, and a subtle compositor glow for the neon sign.
Keep `dog_portrait.png`, `dog_photo.png`, and `add_portrait.py` beside the generator
to rebuild both pictures from the original supplied images.
