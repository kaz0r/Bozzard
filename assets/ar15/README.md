# AR-15 game prop

This folder contains an original, stylized static prop made in Blender. It is
visual game artwork with approximate proportions, not a manufacturing model.

- `ar15.blend` is the editable source. Components and attachment empties have
  descriptive names.
- `ar15.glb` is the engine asset. Geometry is joined into six PBR material
  groups to reduce draw calls. It has no external textures or dependencies.
- `ar15_preview.png` shows the authored appearance.

The model uses meters, points its muzzle along **+X**, and exports from Blender
Z-up to glTF/Bozzard Y-up. Its origin is near the receiver. It is a static mesh:
no rig, animations, gameplay logic, or collision mesh are included.

In Bozzard, use **File → Import asset…** and select `ar15.glb`. The editor copies
it into the scene's asset library. To regenerate all three outputs after editing
the generator, run from the repository root:

```sh
blender --background --python tools/generate_ar15.py
```
