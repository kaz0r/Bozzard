# Demo player mannequin

A stylized neutral mannequin in an A-pose, about 1.84 meters tall. Its feet sit
at the asset origin and its face points toward **-Z** in the Y-up GLB.

- `mannequin.blend`: editable source with articulated parts and armature.
- `mannequin.glb`: compact static mesh used by the 3D starter scene.
- `mannequin_rigged.glb`: skinned version with named humanoid bones for future
  animation authoring.
- `mannequin_preview.png`: studio preview.

The 3D starter project includes its own copy of `mannequin.glb` beside its scene.
The character has no animation clips yet. Regenerate the source assets from the
repository root with:

```sh
blender --background --python tools/generate_character_assets.py
```
